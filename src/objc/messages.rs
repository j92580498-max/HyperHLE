/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0.
 * If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//!
//! Handling of Objective-C messaging (`objc_msgSend` and friends).
//!
//! Resources:
//! - Apple's [Objective-C Runtime Programming Guide](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/ObjCRuntimeGuide/Articles/ocrtHowMessagingWorks.html)
//!
//!
//! - [Apple's documentation of `objc_msgSend`](https://developer.apple.com/documentation/objectivec/1456712-objc_msgsend)
//! - Mike Ash's [objc_msgSend's New Prototype](https://www.mikeash.com/pyblog/objc_msgsends-new-prototype.html)
//!
//!
//! - Peter Steinberger's [Calling Super at Runtime in Swift](https://steipete.com/posts/calling-super-at-runtime/) explains `objc_msgSendSuper2`

use super::{id, nil, Class, ObjC, IMP, SEL};
use crate::abi::{CallFromHost, GuestRet};
use crate::mem::{ConstPtr, MutVoidPtr, SafeRead};
use crate::Environment;
use std::any::TypeId;

/// Implements Apple's lazy `+initialize` contract:
/// > The runtime sends `initialize` to each class in a program just before the
/// > class, or any class that inherits from it, is sent its first message from
/// > within the program. Superclasses receive this message before their
/// > subclasses.
///
/// See <https://developer.apple.com/documentation/objectivec/nsobject/1418639-initialize>.
///
/// `class_to_init` must be a (regular) class, not a metaclass. The class is
/// marked as initialized *before* `+initialize` is dispatched, so that any
/// messages sent to it from within `+initialize` itself do not cause infinite
/// recursion.
fn ensure_class_initialized(env: &mut Environment, class_to_init: Class) {
    if class_to_init == nil {
        return;
    }
    if env.objc.initialized_classes.contains(&class_to_init) {
        return;
    }

    // Initialize the superclass first ("Superclasses receive this message
    // before their subclasses").
    let superclass = {
        let Some(host_object) = env.objc.get_host_object(class_to_init) else {
            // Class has no host object – nothing to initialize. Mark it as
            // done so we don't waste cycles re-checking on every dispatch.
            env.objc.initialized_classes.insert(class_to_init);
            return;
        };
        if let Some(co) = host_object
            .as_any()
            .downcast_ref::<super::ClassHostObject>()
        {
            co.superclass
        } else {
            // FakeClass / UnimplementedClass – nothing to initialize.
            env.objc.initialized_classes.insert(class_to_init);
            return;
        }
    };
    ensure_class_initialized(env, superclass);

    // Re-check after recursion (the recursive call could not have
    // initialised this class, but be defensive).
    if !env.objc.initialized_classes.insert(class_to_init) {
        return;
    }

    // Decide whether to actually dispatch `+initialize`. We send it iff
    // any class in the metaclass chain implements `initialize`. Otherwise
    // there is nothing to call (the inherited NSObject default is a no-op
    // anyway) and dispatching would just emit a "does not respond" warning.
    let metaclass = ObjC::read_isa(class_to_init, &env.mem);
    let Some(sel_initialize) = env.objc.lookup_selector("initialize") else {
        return;
    };
    if !env.objc.class_has_method(metaclass, sel_initialize) {
        return;
    }

    // `+initialize` only takes (self, _cmd); however, the *outer* message
    // dispatch we're nested inside has its real arguments sitting in r0..r3
    // (and possibly on the stack). Dispatching `+initialize` will clobber
    // r0..r3, so snapshot them and restore afterwards. SP/LR are already
    // preserved by `call_from_host`. Stack arguments and VFP registers used
    // for FP arguments aren't touched by a 2-argument `+initialize` call.
    let saved_r0_r3 = [
        env.cpu.regs()[0],
        env.cpu.regs()[1],
        env.cpu.regs()[2],
        env.cpu.regs()[3],
    ];
    log_dbg!("Dispatching +[{:?} initialize]", class_to_init);
    let _: () = msg_send_no_type_checking(env, (class_to_init, sel_initialize));
    let regs = env.cpu.regs_mut();
    regs[0..4].copy_from_slice(&saved_r0_r3);
}

/// The core implementation of `objc_msgSend`, the main function of Objective-C.
///
/// Note that while only two parameters (usually receiver and selector) are
/// defined by the wrappers over this function, a call to an `objc_msgSend`
/// variant may have additional arguments to be forwarded (or rather, left
/// untouched) by `objc_msgSend` when it tail-calls the method implementation it
/// looks up.
/// This is invisible to the Rust type system; we're relying on
/// [crate::abi::CallFromGuest] here.
///
/// Similarly, the return value of `objc_msgSend` is whatever value is returned
/// by the method implementation.
/// We are relying on CallFromGuest not
/// overwriting it.
#[allow(non_snake_case)]
fn objc_msgSend_inner(
    env: &mut Environment,
    receiver: id,
    selector: SEL,
    super2: Option<Class>,
    tolerate_type_mismatch: bool,
) {
    log_dbg!(
        "Dispatching {} for {:?}",
        selector.as_str(&env.mem),
        receiver
    );
    // Host-side recursion guard. If an Objective-C method (typically
    // `hitTest:withEvent:` or `pointInside:withEvent:`) ends up recursing
    // into itself indirectly, the host call stack balloons because every
    // round trip goes host -> guest -> host. Without this guard that path
    // SIGSEGVs the whole emulator once the native stack is exhausted.
    //
    // We use a thread-local counter instead of tracking it in
    // `Environment`, since `objc_msgSend_inner` is the single chokepoint
    // through which every dispatch (host or guest) must pass.
    //
    // 128 is a deliberate compromise: real iOS view hierarchies rarely go
    // deeper than ~50 nested `nextResponder`/`hitTest:` levels, and a small
    // limit keeps us well clear of Android's 1 MB default thread stack
    // (each `objc_msgSend_inner` host frame is several KB once Rust adds
    // local variables, log!() temporaries, and the dispatch trampoline).
    const MAX_DEPTH: usize = 128;
    thread_local! {
        static DISPATCH_DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }
    let depth = DISPATCH_DEPTH.with(|d| {
        let new = d.get() + 1;
        d.set(new);
        new
    });
    struct DepthGuard;
    impl Drop for DepthGuard {
        fn drop(&mut self) {
            DISPATCH_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
        }
    }
    let _guard = DepthGuard;
    if depth > MAX_DEPTH {
        let sel_name = selector.as_str(&env.mem).to_string();
        log!(
            "Warning: objc_msgSend recursion limit ({}) exceeded while dispatching \"{}\" to {:?}; bailing out with a nil return.",
            MAX_DEPTH,
            sel_name,
            receiver,
        );
        // Special handling for allocWithZone: — if recursion is hit during
        // allocation, perform the allocation directly using NSObject's
        // fallback path rather than returning nil (which causes cascading
        // failures like "texture cannot be nil!" in games).
        if sel_name == "allocWithZone:" {
            let obj = env.objc.alloc_object(
                receiver,
                Box::new(super::TrivialHostObject),
                &mut env.mem,
            );
            env.cpu.regs_mut()[0] = obj.to_bits();
            return;
        }
        env.cpu.regs_mut()[0..2].fill(0);
        return;
    }

    let message_type_info = env.objc.message_type_info.take();

    if receiver == nil {
        // https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/ObjectiveC/Chapters/ocObjectsClasses.html#//apple_ref/doc/uid/TP30001163-CH11-SW7
        log_dbg!("[nil {}]", selector.as_str(&env.mem));
        env.cpu.regs_mut()[0..2].fill(0);
        return;
    }

    let orig_class = super2.unwrap_or_else(|| ObjC::read_isa(receiver, &env.mem));
    // Graceful exit if isa is nil — this typically means the object was
    // already deallocated (use-after-free in guest code) or was never
    // properly allocated. Per Apple's Objective-C runtime behavior,
    // messaging a deallocated object is undefined behavior, but we handle
    // it gracefully by returning nil/0 instead of crashing.
    if orig_class == nil {
        // Rate-limit these warnings to avoid flooding the log when the
        // guest app has a use-after-free bug that triggers repeatedly.
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NIL_ISA_COUNT: AtomicUsize = AtomicUsize::new(0);
        const NIL_ISA_LOG_LIMIT: usize = 8;
        let count = NIL_ISA_COUNT.fetch_add(1, Ordering::Relaxed);
        if count < NIL_ISA_LOG_LIMIT {
            log!(
                "Warning: receiver {:?} has nil isa! Ignoring message \"{}\". \
                 (This usually means the object was already freed — use-after-free \
                 in guest code.) [{}/{}]",
                receiver,
                selector.as_str(&env.mem),
                count + 1,
                NIL_ISA_LOG_LIMIT,
            );
        } else if count == NIL_ISA_LOG_LIMIT {
            log!(
                "Warning: suppressing further nil-isa warnings ({} already logged). \
                 The guest app has use-after-free bugs.",
                NIL_ISA_LOG_LIMIT,
            );
        }
        env.cpu.regs_mut()[0..2].fill(0);
        return;
    }

    // Lazily dispatch `+initialize` to the receiver's class (and its
    // superclasses) before this message reaches its IMP. Skipped for super
    // calls — the calling class is already initialized by the time we reach
    // a `super` call site inside one of its methods.
    if super2.is_none() {
        let class_to_init = if let Some(host_object) = env.objc.get_host_object(orig_class) {
            if let Some(co) = host_object
                .as_any()
                .downcast_ref::<super::ClassHostObject>()
            {
                if co.is_metaclass {
                    // Class method: receiver itself is the class.
                    receiver
                } else {
                    // Instance method: orig_class is the class.
                    orig_class
                }
            } else {
                nil
            }
        } else {
            nil
        };
        if class_to_init != nil {
            ensure_class_initialized(env, class_to_init);
        }
    }

    // Traverse the chain of superclasses to find the method implementation.
    let mut class = orig_class;
    loop {
        if class == nil {
            assert!(class != orig_class);
            let class_host_object = env.objc.get_host_object(orig_class).unwrap();
            let &super::ClassHostObject {
                ref name,
                is_metaclass,
                ..
            } = class_host_object.as_any().downcast_ref().unwrap();

            // --- ИСПРАВЛЕНИЕ ЗДЕСЬ: заменили panic! на log! (мягкий фейл
            // форка) ---
            log!(
                "Warning: {} {:?} ({}class \"{}\", {:?}){} does not respond to selector \"{}\"! Returning 0.",
                if is_metaclass { "Class" } else { "Object" },
                receiver,
                if is_metaclass { "meta" } else { "" },
                name,
                orig_class,
                if super2.is_some() {
                    "'s superclass"
                } else {
                    ""
                },
                selector.as_str(&env.mem),
            );

            // Имитируем возврат nil/0, чтобы приложение продолжило работу
            env.cpu.regs_mut()[0..2].fill(0);
            return;
            // ------------------------------------------------------------
        }

        let Some(host_object) = env.objc.get_host_object(class) else {
            log_dbg!(
                "Warning: class {:?} in superclass chain of {:?} has no host object — stopping dispatch",
                class, receiver
            );
            env.cpu.regs_mut()[0..2].fill(0);
            return;
        };

        if let Some(&super::ClassHostObject {
            superclass,
            ref methods,
            ref name,
            ..
        }) = host_object.as_any().downcast_ref()
        {
            // Skip method lookup on first iteration if this is the super-call
            // variant of objc_msgSend (look up the superclass first)
            if super2.is_some() && class == orig_class {
                class = superclass;
                continue;
            }

            if let Some(imp) = methods.get(&selector) {
                log_dbg!("Found method on: {}", name);
                match imp {
                    IMP::Host(host_imp) => {
                        // TODO: do type checks when calling GuestIMPs too.
                        // That requires using Objective-C type strings,
                        // rather than Rust types, and should probably
                        // warn rather than panicking,
                        // because apps might rely on type punning.
                        if let Some((sent_type_id, sent_type_desc)) = message_type_info {
                            let (expected_type_id, expected_type_desc) = host_imp.type_info();
                            if sent_type_id != expected_type_id {
                                let msg = format!(
                                    "\
Type mismatch when sending message {} to {:?}!
- Message has type: {:?} / {}
- Method expects type: {:?} / {}",
                                    selector.as_str(&env.mem),
                                    receiver,
                                    sent_type_id,
                                    sent_type_desc,
                                    expected_type_id,
                                    expected_type_desc
                                );
                                if tolerate_type_mismatch {
                                    log!("Warning: {}", msg);
                                } else {
                                    log_dbg!("{}", msg); // Мягкий фейл, чтобы не падать
                                }
                            }
                        }
                        host_imp.call_from_guest(env)
                    }
                    // We can't create a new stack frame, because that would
                    // interfere with pass-through of stack arguments.
                    IMP::Guest(guest_imp) => guest_imp.call_without_pushing_stack_frame(env),
                }
                return;
            } else {
                class = superclass;
            }
        } else if let Some(&super::UnimplementedClass {
            ref name,
            is_metaclass,
        }) = host_object.as_any().downcast_ref()
        {
            log!(
                "Class \"{}\" ({:?}) is unimplemented. Call to {} method \"{}\".",
                name,
                class,
                if is_metaclass { "class" } else { "instance" },
                selector.as_str(&env.mem),
            );
            env.cpu.regs_mut()[0..2].fill(0);
            return;
        } else if let Some(&super::FakeClass {
            ref name,
            is_metaclass,
        }) = host_object.as_any().downcast_ref()
        {
            log!(
                "Call to faked class \"{}\" ({:?}) {} method \"{}\". Behaving as if message was sent to nil.",
                name,
                class,
                if is_metaclass { "class" } else { "instance" },
                selector.as_str(&env.mem),
            );
            env.cpu.regs_mut()[0..2].fill(0);
            return;
        } else {
            log!(
                "Item {class:?} in superclass chain of object {receiver:?}'s class {orig_class:?} has an unexpected host object type."
            );
            env.cpu.regs_mut()[0..2].fill(0);
            return;
        }
    }
}

/// Standard variant of `objc_msgSend`. See [objc_msgSend_inner].
#[allow(non_snake_case)]
pub(super) fn objc_msgSend(env: &mut Environment, receiver: id, selector: SEL) {
    objc_msgSend_inner(
        env, receiver, selector, /* super2: */ None, /* tolerate_type_mismatch: */ false,
    )
}

#[allow(non_snake_case)]
pub(crate) fn _touchHLE_objc_msgSend_tolerant(env: &mut Environment, receiver: id, selector: SEL) {
    objc_msgSend_inner(
        env, receiver, selector, /* super2: */ None, /* tolerate_type_mismatch: */ true,
    )
}

/// Variant of `objc_msgSend` for methods that return a struct via a pointer.
/// See [objc_msgSend_inner].
///
/// The first parameter here is the pointer for the struct return.
/// This is an
/// ABI detail that is usually hidden and handled behind-the-scenes by
/// [crate::abi], but `objc_msgSend` is a special case because of the
/// pass-through behaviour.
/// Of course, the pass-through only works if the [IMP]
/// also has the pointer parameter.
/// The caller therefore has to pick the
/// appropriate `objc_msgSend` variant depending on the method it wants to call.
pub(super) fn objc_msgSend_stret(
    env: &mut Environment,
    _stret: MutVoidPtr,
    receiver: id,
    selector: SEL,
) {
    objc_msgSend_inner(
        env, receiver, selector, /* super2: */ None, /* tolerate_type_mismatch: */ false,
    )
}

#[allow(non_snake_case)]
pub(crate) fn _touchHLE_objc_msgSend_stret_tolerant(
    env: &mut Environment,
    _stret: MutVoidPtr,
    receiver: id,
    selector: SEL,
) {
    objc_msgSend_inner(
        env, receiver, selector, /* super2: */ None, /* tolerate_type_mismatch: */ true,
    )
}

#[repr(C, packed)]
/// A pointer to this struct replaces the normal receiver parameter for
/// `objc_msgSendSuper2` and [msg_send_super2].
pub struct objc_super {
    pub receiver: id,
    /// If this is used with `objc_msgSendSuper` (not implemented here, TODO),
    /// this is a pointer to the superclass to look up the method on.
    /// If this is used with `objc_msgSendSuper2`, this is a pointer to a class
    /// and the superclass will be looked up from it.
    pub class: Class,
}
unsafe impl SafeRead for objc_super {}

/// Variant of `objc_msgSend` for supercalls. See [objc_msgSend_inner].
///
/// This variant has a weird ABI because it needs to receive an additional piece
/// of information (a class pointer), but it can't actually take this as an
/// extra parameter, because that would take one of the argument slots reserved
/// for arguments passed onto the method implementation.
/// Hence the [objc_super]
/// pointer in place of the normal [id].
#[allow(non_snake_case)]
pub(super) fn objc_msgSendSuper2(
    env: &mut Environment,
    super_ptr: ConstPtr<objc_super>,
    selector: SEL,
) {
    let objc_super { receiver, class } = env.mem.read(super_ptr);
    // Rewrite first argument to match the normal ABI.
    crate::abi::write_next_arg(&mut 0, env.cpu.regs_mut(), &mut env.mem, receiver);
    objc_msgSend_inner(
        env,
        receiver,
        selector,
        /* super2: */ Some(class),
        /* tolerate_type_mismatch: */ false,
    )
}

#[allow(non_snake_case)]
pub(super) fn objc_msgSendSuper2_stret(
    env: &mut Environment,
    super_ptr: ConstPtr<objc_super>,
    selector: SEL,
) {
    let objc_super { receiver, class } = env.mem.read(super_ptr);
    // Rewrite first argument to match the normal ABI.
    crate::abi::write_next_arg(&mut 0, env.cpu.regs_mut(), &mut env.mem, receiver);
    objc_msgSend_inner(
        env,
        receiver,
        selector,
        /* super2: */ Some(class),
        /* tolerate_type_mismatch: */ false,
    )
}

/// Trait that assists with type-checking of [msg_send]'s arguments.
///
/// - Statically constrains the types of [msg_send]'s arguments so that the
///   first two are always [id] and [SEL].
/// - Provides the type ID to enable dynamic type checking of subsequent
///   arguments and the return type.
///
/// See `impl_HostIMP` for implementations. See also [MsgSendSuperSignature].
pub trait MsgSendSignature: 'static {
    /// Get the [TypeId] and a human-readable description for this signature.
    fn type_info() -> (TypeId, &'static str) {
        #[cfg(debug_assertions)]
        let type_name = std::any::type_name::<Self>();
        // Avoid wasting space on type names in release builds.
        // At the time of writing this saves about 36KB.
        #[cfg(not(debug_assertions))]
        let type_name = "[description unavailable in release builds]";
        (TypeId::of::<Self>(), type_name)
    }
}

// --- Extended implementations for higher number of arguments (7, 8, 9
// parameters) ---
impl<
        R: 'static,
        P1: 'static,
        P2: 'static,
        P3: 'static,
        P4: 'static,
        P5: 'static,
        P6: 'static,
        P7: 'static,
    > MsgSendSignature for (R, (id, SEL, P1, P2, P3, P4, P5, P6, P7))
{
}
impl<
        R: 'static,
        P1: 'static,
        P2: 'static,
        P3: 'static,
        P4: 'static,
        P5: 'static,
        P6: 'static,
        P7: 'static,
        P8: 'static,
    > MsgSendSignature for (R, (id, SEL, P1, P2, P3, P4, P5, P6, P7, P8))
{
}
impl<
        R: 'static,
        P1: 'static,
        P2: 'static,
        P3: 'static,
        P4: 'static,
        P5: 'static,
        P6: 'static,
        P7: 'static,
        P8: 'static,
        P9: 'static,
    > MsgSendSignature for (R, (id, SEL, P1, P2, P3, P4, P5, P6, P7, P8, P9))
{
}

/// Wrapper around [objc_msgSend] which, together with [msg], makes it easy to
/// send messages in host code.
/// Warning: all types are inferred from the
/// call-site and they may not be checked, so be very sure you get them correct!
pub fn msg_send<R, P>(env: &mut Environment, args: P) -> R
where
    fn(&mut Environment, id, SEL): CallFromHost<R, P>,
    fn(&mut Environment, MutVoidPtr, id, SEL): CallFromHost<R, P>,
    (R, P): MsgSendSignature,
    R: GuestRet,
{
    // Provide type info for dynamic type checking.
    env.objc.message_type_info = Some(<(R, P) as MsgSendSignature>::type_info());
    if R::SIZE_IN_MEM.is_some() {
        (objc_msgSend_stret as fn(&mut Environment, MutVoidPtr, id, SEL)).call_from_host(env, args)
    } else {
        (objc_msgSend as fn(&mut Environment, id, SEL)).call_from_host(env, args)
    }
}

pub fn msg_send_no_type_checking<R, P>(env: &mut Environment, args: P) -> R
where
    fn(&mut Environment, id, SEL): CallFromHost<R, P>,
    fn(&mut Environment, MutVoidPtr, id, SEL): CallFromHost<R, P>,
    (R, P): MsgSendSignature,
    R: GuestRet,
{
    if R::SIZE_IN_MEM.is_some() {
        (_touchHLE_objc_msgSend_stret_tolerant as fn(&mut Environment, MutVoidPtr, id, SEL))
            .call_from_host(env, args)
    } else {
        (_touchHLE_objc_msgSend_tolerant as fn(&mut Environment, id, SEL)).call_from_host(env, args)
    }
}

/// Counterpart of [MsgSendSignature] for [msg_send_super2].
pub trait MsgSendSuperSignature: 'static {
    /// Signature with the [objc_super] pointer replaced by [id].
    type WithoutSuper: MsgSendSignature;
}

// --- Extended super-call implementations for higher number of arguments ---
impl<
        R: 'static,
        P1: 'static,
        P2: 'static,
        P3: 'static,
        P4: 'static,
        P5: 'static,
        P6: 'static,
        P7: 'static,
    > MsgSendSuperSignature for (R, (ConstPtr<objc_super>, SEL, P1, P2, P3, P4, P5, P6, P7))
{
    type WithoutSuper = (R, (id, SEL, P1, P2, P3, P4, P5, P6, P7));
}
impl<
        R: 'static,
        P1: 'static,
        P2: 'static,
        P3: 'static,
        P4: 'static,
        P5: 'static,
        P6: 'static,
        P7: 'static,
        P8: 'static,
    > MsgSendSuperSignature
    for (
        R,
        (ConstPtr<objc_super>, SEL, P1, P2, P3, P4, P5, P6, P7, P8),
    )
{
    type WithoutSuper = (R, (id, SEL, P1, P2, P3, P4, P5, P6, P7, P8));
}
impl<
        R: 'static,
        P1: 'static,
        P2: 'static,
        P3: 'static,
        P4: 'static,
        P5: 'static,
        P6: 'static,
        P7: 'static,
        P8: 'static,
        P9: 'static,
    > MsgSendSuperSignature
    for (
        R,
        (
            ConstPtr<objc_super>,
            SEL,
            P1,
            P2,
            P3,
            P4,
            P5,
            P6,
            P7,
            P8,
            P9,
        ),
    )
{
    type WithoutSuper = (R, (id, SEL, P1, P2, P3, P4, P5, P6, P7, P8, P9));
}

/// [msg_send] but for super-calls (calls [objc_msgSendSuper2]). You probably
/// want to use [msg_super] rather than calling this directly.
pub fn msg_send_super2<R, P>(env: &mut Environment, args: P) -> R
where
    fn(&mut Environment, ConstPtr<objc_super>, SEL): CallFromHost<R, P>,
    fn(&mut Environment, MutVoidPtr, ConstPtr<objc_super>, SEL): CallFromHost<R, P>,
    (R, P): MsgSendSuperSignature,
    R: GuestRet,
{
    // Provide type info for dynamic type checking.
    env.objc.message_type_info = Some(<(R, P) as MsgSendSuperSignature>::WithoutSuper::type_info());
    if R::SIZE_IN_MEM.is_some() {
        // Struct returns (stret) for super-calls aren't implemented yet.
        // Log this clearly and fall through to the non-stret path so the
        // host process keeps running and the caller will simply observe
        // the default-constructed return value via to_regs/to_mem below.
        log!(
            "Warning: msg_send_super2: struct-return (stret) super-call is not implemented; falling back to non-stret dispatch. Result may be unreliable.",
        );
        (objc_msgSendSuper2 as fn(&mut Environment, ConstPtr<objc_super>, SEL))
            .call_from_host(env, args)
    } else {
        (objc_msgSendSuper2 as fn(&mut Environment, ConstPtr<objc_super>, SEL))
            .call_from_host(env, args)
    }
}

/// Macro for sending a message which imitates the Objective-C messaging syntax.
///
/// See [msg_send] for the underlying implementation. Warning: all types are
/// inferred from the call-site and they may not be checked, so be very sure you
/// get them correct!
///
/// ```ignore
/// msg![env; foo setBar:bar withQux:qux];
/// ```
///
/// desugars to:
///
/// ```ignore
/// {
///     let sel = env.objc.lookup_selector("setFoo:withBar").unwrap();
///     msg_send(env, (foo, sel, bar, qux))
/// }
/// ```
///
/// Note that argument values that aren't a bare single identifier like `foo`
/// need to be bracketed.
///
/// See also [msg_class], if you want to send a message to a class.
#[macro_export]
macro_rules! msg {
    [$env:expr; $receiver:tt $name:ident $(: $arg1:tt $($($namen:ident)?: $argn:tt)*)?] => {
        {
            let sel = $crate::objc::selector!($($arg1;)? $name $($(, $($namen)?)*)?);
            let sel = $env.objc.lookup_selector(sel)
                .expect("Unknown selector");
            let args = ($receiver, sel, $($arg1, $($argn),*)?);
            $crate::objc::msg_send($env, args)
        }
    }
}
pub use crate::msg;
// #[macro_export] is weird...

/// Variant of [msg] for super-calls.
///
/// Unlike the other variants, this macro can only be used within
/// [crate::objc::objc_classes], because it relies on that macro defining a
/// constant containing the name of the current class.
///
/// ```ignore
/// msg_super![env; this init]
/// ```
///
/// desugars to something like this, if the current class is `SomeClass`:
///
/// ```ignore
/// {
///     let super_arg_ptr = push_to_stack(env, objc_super {
///         receiver: this,
///         class: env.objc.get_known_class("SomeClass", &mut env.mem),
///     });
///     let sel = env.objc.lookup_selector("init").unwrap();
///     let res = msg_send_super2(env, (super_arg_ptr, sel));
///     pop_from_stack::<objc_super>(env);
///     res
/// }
/// ```
#[macro_export]
macro_rules! msg_super {
    [$env:expr; $receiver:tt $name:ident $(: $arg1:tt $($($namen:ident)?: $argn:tt)*)?] => {
        {
            let class = $env.objc.get_known_class(
                _OBJC_CURRENT_CLASS,
                &mut $env.mem
            );
            let sel = $crate::objc::selector!($($arg1;)? $name $($(, $($namen)?)*)?);
            let sel = $env.objc.lookup_selector(sel)
                .expect("Unknown selector");
            let sp = &mut $env.cpu.regs_mut()[$crate::cpu::Cpu::SP];
            let old_sp = *sp;
            *sp -= $crate::mem::guest_size_of::<$crate::objc::objc_super>();
            let super_ptr = $crate::mem::Ptr::from_bits(*sp);
            $env.mem.write(super_ptr, $crate::objc::objc_super {
                receiver: $receiver,
                class,
            });
            let args = (super_ptr.cast_const(), sel, $($arg1, $($argn),*)?);
            let res = $crate::objc::msg_send_super2($env, args);

            $env.cpu.regs_mut()[$crate::cpu::Cpu::SP] = old_sp;
            res
        }
    }
}
pub use crate::msg_super;
// #[macro_export] is weird...

/// Variant of [msg] for sending a message to a named class.
/// Useful for calling class methods, especially `new`.
///
/// ```ignore
/// msg_class![env; SomeClass alloc]
/// ```
///
/// desugars to:
///
/// ```ignore
/// msg![env; (env.objc.get_known_class("SomeClass", &mut env.mem)) alloc]
/// ```
#[macro_export]
macro_rules! msg_class {
    [$env:expr; $receiver_class:ident $name:ident $(: $arg1:tt $($($namen:ident)?: $argn:tt)*)?] => {
        {
            let class = $env.objc.get_known_class(
                stringify!($receiver_class),
                &mut $env.mem
            );
            $crate::objc::msg![$env; class $name $(: $arg1 $($($namen)?: $argn)*)?]
        }
    }
}
pub use crate::msg_class;
// #[macro_export] is weird...

/// Shorthand for `let _: id = msg![env; object retain];`
pub fn retain(env: &mut Environment, object: id) -> id {
    if object == nil {
        // fast path
        return nil;
    }
    msg![env; object retain]
}

/// Shorthand for `() = msg![env; object release];`
pub fn release(env: &mut Environment, object: id) {
    if object == nil {
        // fast path
        return;
    }
    msg![env; object release]
}

/// Shorthand for `let _: id = msg![env; object autorelease];`
pub fn autorelease(env: &mut Environment, object: id) -> id {
    if object == nil {
        // fast path
        return nil;
    }
    msg![env; object autorelease]
}
