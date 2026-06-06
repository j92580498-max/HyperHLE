/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Handling of Objective-C objects.

use super::{Class, ClassHostObject};
use crate::mem::{guest_size_of, GuestUSize, Mem, MutPtr, Ptr, SafeRead};
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::num::NonZeroU32;
use std::sync::Mutex;

/// Per-(id, TypeId) cache of phantom host-object buffers used when a
/// `borrow`/`borrow_mut` call hits an object that has no real host-side
/// record. Each entry is a zero-initialised, leaked buffer large enough to
/// hold `T`; callers get a stable reference that isn't aliased with buffers
/// for other objects/types.
///
/// The cache is behind a single process-wide `Mutex` because touchHLE keeps
/// a single `ObjC` instance but this function is used from both immutable
/// (`&self`) and mutable (`&mut self`) receivers, and from many framework
/// modules. Contention here is only hit on the error path, so a plain
/// `Mutex` is fine.
static PHANTOM_STORE: Mutex<Option<HashMap<(TypeId, usize), usize>>> = Mutex::new(None);

fn phantom_buffer_for<T: 'static>(object: id, init: impl FnOnce() -> T) -> *mut u8 {
    let key = (TypeId::of::<T>(), object.to_bits() as usize);
    let mut guard = PHANTOM_STORE.lock().unwrap();
    let map = guard.get_or_insert_with(HashMap::new);
    if let Some(&ptr) = map.get(&key) {
        return ptr as *mut u8;
    }
    // Leak a buffer sized and aligned for T, then write a real `T` value
    // into it. Using raw `alloc_zeroed` plus `transmute` (the previous
    // behaviour) was unsound for types whose zero bit-pattern is not a
    // valid instance — most notably anything containing a `HashMap`, whose
    // internal `ctrl` pointer must point at hashbrown's static empty
    // sentinel rather than null. Performing a proper `T::default()`
    // (passed in by the caller) ensures the buffer holds a usable
    // instance even on this error path.
    let layout = std::alloc::Layout::new::<T>();
    // SAFETY: `layout.size()` is non-zero for any real host object, and
    // the allocator returns a pointer with `layout.align()` alignment.
    // Writing `init()` (a `T` value) into freshly allocated, uninitialised
    // memory of exactly that layout is well-defined.
    let ptr = unsafe { std::alloc::alloc(layout) };
    assert!(!ptr.is_null(), "phantom host object allocation failed");
    unsafe { std::ptr::write(ptr as *mut T, init()) };
    map.insert(key, ptr as usize);
    ptr
}

/// Return a `&T` pointing at a stable backing buffer for the given
/// missing-object id, initialised on first access via [`Default::default`].
/// Repeated calls with the same `object` and `T` return the same buffer
/// (which may have been mutated through [`phantom_host_object_mut`] in the
/// meantime).
fn phantom_host_object<T: Default + 'static>(object: id) -> &'static T {
    let ptr = phantom_buffer_for::<T>(object, T::default) as *const T;
    // SAFETY: `phantom_buffer_for` returns a stable allocation of
    // `size_of::<T>()` bytes with the required alignment, initialised on
    // first call via `T::default()`. Subsequent calls return the same
    // region, giving a stable `'static` reference to a valid `T`.
    unsafe { &*ptr }
}

/// Return a `&mut T` pointing at a stable backing buffer for the given
/// missing-object id, initialised on first access via [`Default::default`].
/// Repeated calls with the same `object` and `T` return a reference to the
/// same buffer.
fn phantom_host_object_mut<T: Default + 'static>(object: id) -> &'static mut T {
    let ptr = phantom_buffer_for::<T>(object, T::default) as *mut T;
    // SAFETY: See `phantom_host_object`. Additionally, because the cache
    // keys on `(TypeId, object)` each call-site gets an isolated buffer,
    // so mutations by one fake-borrow won't be visible to another fake-
    // borrow of a different object or type.
    unsafe { &mut *ptr }
}

#[repr(C, packed)]
pub struct objc_object {
    pub(super) isa: Class,
}
unsafe impl SafeRead for objc_object {}

#[allow(non_camel_case_types)]
pub type id = MutPtr<objc_object>;

#[allow(non_upper_case_globals)]
pub const nil: id = Ptr::null();

pub(super) struct HostObjectEntry {
    host_object: Box<dyn AnyHostObject>,
    refcount: Option<NonZeroU32>,
}

pub trait HostObject: Any + 'static {
    fn as_superclass<'a>(&'a self) -> Option<&'a (dyn AnyHostObject + 'static)> {
        None
    }
    fn as_superclass_mut<'a>(&'a mut self) -> Option<&'a mut (dyn AnyHostObject + 'static)> {
        None
    }
}

#[macro_export]
macro_rules! impl_HostObject_with_superclass {
    ( $ty:ty ) => {
        impl $crate::objc::HostObject for $ty {
            fn as_superclass<'a>(
                &'a self,
            ) -> Option<&'a (dyn $crate::objc::AnyHostObject + 'static)> {
                Some(&self.superclass)
            }
            fn as_superclass_mut<'a>(
                &'a mut self,
            ) -> Option<&'a mut (dyn $crate::objc::AnyHostObject + 'static)> {
                Some(&mut self.superclass)
            }
        }
    };
}
pub use crate::impl_HostObject_with_superclass;

pub trait AnyHostObject: HostObject {
    fn as_any<'a>(&'a self) -> &'a (dyn Any + 'static);
    fn as_any_mut<'a>(&'a mut self) -> &'a mut (dyn Any + 'static);
    fn type_name(&self) -> &'static str;
}
impl<T: HostObject> AnyHostObject for T {
    fn as_any<'a>(&'a self) -> &'a (dyn Any + 'static) {
        self
    }
    fn as_any_mut<'a>(&'a mut self) -> &'a mut (dyn Any + 'static) {
        self
    }
    fn type_name(&self) -> &'static str {
        std::any::type_name::<T>()
    }
}

pub struct TrivialHostObject;
impl HostObject for TrivialHostObject {}

impl super::ObjC {
    pub fn read_isa(object: id, mem: &Mem) -> Class {
        if object == nil {
            return Ptr::null();
        }
        mem.read(object).isa
    }

    fn alloc_object_inner(
        &mut self,
        isa: Class,
        instance_size: GuestUSize,
        host_object: Box<dyn AnyHostObject>,
        mem: &mut Mem,
        refcount: Option<NonZeroU32>,
    ) -> id {
        let guest_object = objc_object { isa };
        let ptr: MutPtr<objc_object> = mem.alloc(instance_size).cast();
        mem.write(ptr, guest_object);
        self.objects.insert(
            ptr,
            HostObjectEntry {
                host_object,
                refcount,
            },
        );
        ptr
    }

    pub fn alloc_object(
        &mut self,
        isa: Class,
        host_object: Box<dyn AnyHostObject>,
        mem: &mut Mem,
    ) -> id {
        let instance_size = self
            .get_host_object(isa)
            .and_then(|h| h.as_any().downcast_ref::<ClassHostObject>())
            .map(|c| c.instance_size)
            .unwrap_or(guest_size_of::<objc_object>());

        self.alloc_object_inner(
            isa,
            instance_size,
            host_object,
            mem,
            Some(NonZeroU32::new(1).unwrap()),
        )
    }

    pub fn alloc_static_object(
        &mut self,
        isa: Class,
        host_object: Box<dyn AnyHostObject>,
        mem: &mut Mem,
    ) -> id {
        let size = guest_size_of::<objc_object>();
        self.alloc_object_inner(isa, size, host_object, mem, None)
    }

    pub fn register_static_object(
        &mut self,
        guest_object: id,
        host_object: Box<dyn AnyHostObject>,
    ) {
        if guest_object == nil {
            return;
        }
        self.objects.insert(
            guest_object,
            HostObjectEntry {
                host_object,
                refcount: None,
            },
        );
    }

    pub fn get_host_object(&self, object: id) -> Option<&dyn AnyHostObject> {
        if object == nil {
            return None;
        }
        self.objects.get(&object).map(|entry| &*entry.host_object)
    }

    pub fn borrow<T: AnyHostObject + Default + 'static>(&self, object: id) -> &T {
        if let Some(entry) = self.objects.get(&object) {
            let mut host_object: &(dyn AnyHostObject + 'static) = &*entry.host_object;
            loop {
                if let Some(res) = host_object.as_any().downcast_ref() {
                    return res;
                } else if let Some(next) = host_object.as_superclass() {
                    host_object = next;
                } else {
                    break;
                }
            }
        }

        // Fallback for missing / wrong-type objects.
        //
        // Previously we returned a reference to a single shared
        // `static DUMMY_BUF: [u64; 256] = [0; 256]`. That one buffer was
        // aliased across EVERY fake borrow of EVERY type, so as soon as a
        // `borrow_mut` populated e.g. `UIViewHostObject.subviews` with a
        // non-empty Vec, every subsequent fake borrow saw the same list —
        // including of itself, causing `hitTest:` to recurse infinitely and
        // overflow the host stack.
        //
        // We now leak a fresh zero-initialized buffer per (id, type) pair
        // so the returned reference has stable, isolated storage. A proper
        // fix would register a real `Default::default()` host object, but
        // that requires a `T: Default` bound which many callers don't yet
        // provide.
        // POSIX/Objective-C semantics: a message to `nil` returns the
        // zero/empty form of the return type — the runtime is expected
        // to treat such calls as a no-op. Returning a zero-initialized
        // phantom host object preserves this without flooding the log.
        if object == nil {
            log_dbg!(
                "borrow on nil receiver of type {} — returning zero-initialized phantom",
                std::any::type_name::<T>()
            );
        } else if let Some(entry) = self.objects.get(&object) {
            // The object exists but its host object is a different type than
            // requested. Reporting the actual type makes these mismatches
            // diagnosable — it's usually either a guest pointer/type confusion
            // or a host class that forgot to embed its superclass host object
            // (see `impl_HostObject_with_superclass!`).
            log!(
                "Warning: SUPER HACK! Faking borrow for wrong-type object {:?}: \
                 requested {}, actual host type {}",
                object,
                std::any::type_name::<T>(),
                entry.host_object.type_name(),
            );
        } else {
            log!(
                "Warning: SUPER HACK! Faking borrow for missing object {:?} of type {}",
                object,
                std::any::type_name::<T>()
            );
        }
        phantom_host_object::<T>(object)
    }

    pub fn borrow_mut<T: AnyHostObject + Default + 'static>(&mut self, object: id) -> &mut T {
        if let Some(entry) = self.objects.get_mut(&object) {
            type Aho = dyn AnyHostObject + 'static;
            let mut host_object: &mut Aho = &mut *entry.host_object;
            loop {
                let current_ptr = host_object as *mut Aho;
                if let Some(res) = unsafe { &mut *current_ptr }.as_any_mut().downcast_mut() {
                    return res;
                }

                let has_super = unsafe { &*current_ptr }.as_superclass().is_some();
                if has_super {
                    host_object = unsafe { &mut *current_ptr }.as_superclass_mut().unwrap();
                } else {
                    break;
                }
            }
        }

        // See comment in `borrow` above for rationale.
        if object == nil {
            log_dbg!(
                "borrow_mut on nil receiver of type {} — returning zero-initialized phantom",
                std::any::type_name::<T>()
            );
        } else {
            log!(
                "Warning: SUPER HACK! Faking borrow_mut for missing object {:?} of type {}",
                object,
                std::any::type_name::<T>()
            );
        }
        phantom_host_object_mut::<T>(object)
    }

    pub fn get_refcount(&mut self, object: id) -> NonZeroU32 {
        let default_rc = NonZeroU32::new(1).unwrap();
        if object == nil {
            return default_rc;
        }

        self.objects
            .get(&object)
            .and_then(|e| e.refcount)
            .unwrap_or(default_rc)
    }

    pub fn increment_refcount(&mut self, object: id) {
        if object == nil {
            return;
        }
        if let Some(entry) = self.objects.get_mut(&object) {
            if let Some(refcount) = entry.refcount.as_mut() {
                if let Some(new_rc) = refcount.get().checked_add(1) {
                    *refcount = NonZeroU32::new(new_rc).unwrap();
                }
            }
        }
    }

    #[must_use]
    pub fn decrement_refcount(&mut self, object: id) -> bool {
        if object == nil {
            return false;
        }
        if let Some(entry) = self.objects.get_mut(&object) {
            if let Some(refcount) = entry.refcount.as_mut() {
                if refcount.get() == 1 {
                    entry.refcount = None;
                    return true;
                } else {
                    *refcount = NonZeroU32::new(refcount.get() - 1).unwrap();
                }
            }
        }
        false
    }

    pub fn dealloc_object(&mut self, object: id, mem: &mut Mem) {
        if object == nil {
            return;
        }

        // ARC weak-reference contract: zero out every `__weak` slot
        // that referred to this object before its memory is freed, so
        // subsequent `objc_loadWeakRetained` calls correctly observe
        // `nil`. This must happen before we drop the host object,
        // because the writeback uses guest memory only.
        self.zero_weak_references_for(object, mem);

        if let Some(entry) = self.objects.remove(&object) {
            std::mem::drop(entry.host_object);
            mem.free(object.cast());
        }
    }
}
