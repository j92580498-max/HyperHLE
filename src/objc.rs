/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0.
 * If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Objective-C runtime.
//!
//! Apple's [Programming with Objective-C](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/ProgrammingWithObjectiveC/Introduction/Introduction.html)
//! is a useful introduction to the language from a user's perspective.
//! There are further resources in the child modules of this module, but they
//! are more implementation-specific.
//!
//! The strategy for this emulator will be to provide our own implementations of
//! an Objective-C runtime and libraries for it (Foundation etc). These
//! implementations will be "host code": Rust code forming part of the emulator,
//! not emulated code. The runtime will need to be able to handle classes that
//! originate from the guest app, classes defined by the host, and sometimes
//! classes that are both (considering Objective-C's support for inheritance,
//! categories and dynamic class editing).

use crate::dyld::{
    export_c_func, export_c_func_aliased, ConstantExports, FunctionExports, HostConstant, HostDylib,
};
use crate::MutexId;
use std::collections::{HashMap, HashSet};

mod classes;
mod messages;
mod methods;
mod objects;
mod properties;
mod selectors;
mod synchronization;
mod weak;

pub use classes::{
    class_addMethod, class_copyIvarList, class_copyMethodList, class_copyPropertyList,
    class_copyProtocolList, class_getClassMethod, class_getInstanceMethod, class_getInstanceSize,
    class_getMethodImplementation, class_getMethodImplementation_stret, class_getName,
    class_getProperty, class_getSuperclass, class_replaceMethod, class_respondsToSelector, class_setSuperclass,
    method_exchangeImplementations,
    method_getImplementation, method_getTypeEncoding, method_setImplementation, objc_storeStrong,
    objc_alloc, objc_allocateClassPair, objc_autorelease, objc_autoreleasePoolPop,
    objc_autoreleasePoolPush, objc_autoreleaseReturnValue, objc_begin_catch, objc_classes,
    objc_copyClassNamesForImage, objc_disposeClassPair, objc_end_catch, objc_exception_throw,
    objc_getClass, objc_getMetaClass, objc_getProtocol, objc_getRequiredClass, objc_lookUpClass,
    objc_readClassPair, objc_registerClassPair,
    objc_release, objc_retain, objc_retainAutorelease, objc_retainAutoreleaseReturnValue,
    objc_retainBlock, __objc_deallocOnMainThreadHelper,
    objc_retainAutoreleasedReturnValue, objc_unsafeClaimAutoreleasedReturnValue, object_getClass, object_getClassName, object_getIndexedIvars,
    protocol_getName, objc_getClassList, protocol_conformsToProtocol, Class, ClassExports, ClassTemplate,
};
pub use messages::{
    autorelease, msg, msg_class, msg_send, msg_send_no_type_checking, msg_send_super2, msg_super,
    objc_super, release, retain,
};
pub use methods::{HostIMP, IMP};
pub use objects::{
    id, impl_HostObject_with_superclass, nil, AnyHostObject, HostObject, TrivialHostObject,
};
pub use properties::todo_objc_setter;
pub use selectors::{selector, SEL};

use crate::objc::classes::___objc_personality_v0;
use crate::objc::classes::{objc_msgForward, objc_msgForward_stret};
use crate::Environment;
use classes::{ClassHostObject, FakeClass, UnimplementedClass};
use messages::{
    objc_msgSendSuper2, objc_msgSendSuper2_stret, objc_msgSend_stret, MsgSendSignature,
    MsgSendSuperSignature,
};
use methods::method_list_t;
use objects::{objc_object, HostObjectEntry};
use properties::{ivar_list_t, objc_copyStruct, objc_getProperty, objc_setProperty};
use properties::{
    objc_setProperty_atomic_copy, objc_setProperty_nonatomic, objc_setProperty_nonatomic_copy,
};
use properties::objc_setProperty_atomic;
use selectors::sel_registerName;
use synchronization::{objc_sync_enter, objc_sync_exit};

/// Публичная обёртка над `messages::objc_msgSend` (которая `pub(super)`),
/// экспортируемая внутри крейта.
pub(crate) fn objc_msgSend(env: &mut Environment, receiver: id, selector: SEL) {
    messages::objc_msgSend(env, receiver, selector)
}

/// Typedef for `NSZone *`. This is a [fossil type] found in the signature of
/// `allocWithZone:` and similar methods. Its value is always ignored.
///
/// [fossil type]: https://en.wiktionary.org/wiki/fossil_word
pub type NSZonePtr = crate::mem::MutVoidPtr;

/// Main type holding Objective-C runtime state.
pub struct ObjC {
    /// Known selectors (interned method name strings).
    selectors: HashMap<String, SEL>,

    /// Mapping of known (guest) object pointers to their host objects.
    ///
    /// If an object isn't in this map, we will consider it not to exist.
    objects: HashMap<id, HostObjectEntry>,

    /// Known classes.
    ///
    /// Look at the `isa` to get the metaclass for a class.
    classes: HashMap<String, Class>,

    /// Mutexes used in @synchronized blocks (objc_sync_enter/exit).
    sync_mutexes: HashMap<id, MutexId>,

    /// Per-object recursive mutexes used to make `atomic` properties
    /// thread-safe (`objc_getProperty(..., atomic=true)` /
    /// `objc_setProperty(..., atomic=true, ...)`).
    ///
    /// Apple's `objc-accessors.mm` (open-source `objc4`) uses a striped
    /// table of 8 spinlocks indexed by ivar address — that is purely a
    /// space optimisation. A per-object recursive mutex implements the
    /// same observable contract (a single-writer/single-reader barrier
    /// per object's atomic property accesses) and additionally cannot
    /// false-share the lock between unrelated objects.
    ///
    /// The mutex is recursive because Apple's accessors can re-enter
    /// themselves indirectly: a `@property (atomic, copy)` setter calls
    /// `[newValue copyWithZone:]` while holding the lock, and the
    /// `copyWithZone:` of e.g. `NSMutableString` may itself synthesize
    /// further atomic-property reads on the same object. Lazy created
    /// on first contended access; cleaned up in `dealloc_object`.
    property_locks: HashMap<id, MutexId>,

    /// Temporary storage for optional type information when sending a message.
    /// Type information isn't part of the `objc_msgSend` ABI, so an alternative
    /// channel is needed.
    message_type_info: Option<(std::any::TypeId, &'static str)>,

    /// Set of classes that have already had `+initialize` sent to them
    /// (or were determined not to need it). Used to implement Apple's lazy
    /// `+initialize` dispatch contract:
    /// <https://developer.apple.com/documentation/objectivec/nsobject/1418639-initialize>
    pub(super) initialized_classes: HashSet<Class>,

    /// ARC weak-reference side table.
    ///
    /// `objc_storeWeak(location, value)` registers `location` (a guest
    /// `id*`) as a slot that should be zeroed when `value` is
    /// deallocated. Per the clang ARC specification this is the only
    /// way the runtime can guarantee that "weak references load nil
    /// once the referenced object has been destroyed". We mirror that
    /// behaviour exactly: every `dealloc_object` walks the entry for
    /// the object being torn down and writes `nil` into every
    /// registered location before freeing the object's memory.
    ///
    /// References:
    /// - clang ARC documentation, "Runtime support",
    ///   `objc_storeWeak` / `objc_loadWeakRetained` / `objc_destroyWeak`.
    /// - Apple `objc-runtime-new.mm` `weak_table_t` (open-source on
    ///   <https://opensource.apple.com/source/objc4/>).
    pub(crate) weak_refs: HashMap<id, Vec<crate::mem::MutPtr<id>>>,

    /// Per-object table of values set via `objc_setAssociatedObject`.
    ///
    /// Key: `(object id, key pointer)`.
    /// Value: `(associated id, is_retained)`.  `is_retained` is true for
    /// RETAIN and COPY policies; the value will be released when the
    /// association is overwritten, removed, or the object is deallocated.
    /// ASSIGN policy (0) stores without retaining — the caller is
    /// responsible for ensuring the value outlives the association.
    ///
    /// References:
    /// - <https://developer.apple.com/documentation/objectivec/1418509-objc_setassociatedobject>
    /// - Apple `objc-references.mm` (open-source `objc4`).
    pub(crate) associated_objects: HashMap<(id, crate::mem::GuestUSize), (id, bool)>,
}

impl ObjC {
    pub fn new() -> ObjC {
        ObjC {
            selectors: HashMap::new(),
            objects: HashMap::new(),
            classes: HashMap::new(),
            sync_mutexes: HashMap::new(),
            property_locks: HashMap::new(),
            message_type_info: None,
            initialized_classes: HashSet::new(),
            weak_refs: HashMap::new(),
            associated_objects: HashMap::new(),
        }
    }

    /// Returns the name of a selector, panicking if it is unknown.
    pub fn get_selector_name(&self, sel: SEL) -> &str {
        self.selectors
            .iter()
            .find(|(_k, v)| **v == sel)
            .map(|(k, _v)| k.as_str())
            .expect("get_selector_name: unknown selector")
    }
}


// ──────────────────────────────────────────────────────────────────
// Associated objects  (<objc/runtime.h>)
// ──────────────────────────────────────────────────────────────────

/// Policy constants for `objc_setAssociatedObject`.
/// Source: Apple `<objc/runtime.h>`, `objc_AssociationPolicy` enum.
type ObjcAssociationPolicy = crate::mem::GuestUSize;
const OBJC_ASSOCIATION_ASSIGN: ObjcAssociationPolicy = 0;
const OBJC_ASSOCIATION_RETAIN_NONATOMIC: ObjcAssociationPolicy = 1;
const OBJC_ASSOCIATION_COPY_NONATOMIC: ObjcAssociationPolicy = 3;
/// 0o1401 (octal) = 769 decimal
const OBJC_ASSOCIATION_RETAIN: ObjcAssociationPolicy = 0o1401;
/// 0o1403 (octal) = 771 decimal
const OBJC_ASSOCIATION_COPY: ObjcAssociationPolicy = 0o1403;

/// `void objc_setAssociatedObject(id object, const void *key, id value,
///                                 objc_AssociationPolicy policy)`
///
/// Sets an associated value for a key on an object.
/// <https://developer.apple.com/documentation/objectivec/1418509-objc_setassociatedobject>
fn objc_setAssociatedObject(
    env: &mut Environment,
    object: id,
    key: crate::mem::ConstVoidPtr,
    value: id,
    policy: ObjcAssociationPolicy,
) {
    if object.is_null() {
        log_dbg!("objc_setAssociatedObject: object is nil, ignoring");
        return;
    }

    let map_key = (object, key.to_bits());

    // Release any old retained value for this slot.
    if let Some((old_val, was_retained)) = env.objc.associated_objects.remove(&map_key) {
        if was_retained && !old_val.is_null() {
            release(env, old_val);
        }
    }

    // A nil value means "remove the association" — we already removed it above.
    if value.is_null() {
        return;
    }

    let is_retained = match policy {
        OBJC_ASSOCIATION_RETAIN_NONATOMIC | OBJC_ASSOCIATION_RETAIN => {
            retain(env, value);
            true
        }
        OBJC_ASSOCIATION_COPY_NONATOMIC | OBJC_ASSOCIATION_COPY => {
            // Send `-copy` to get an independent copy, then retain is implicit
            // because the copy call already returns a +1 object.
            let copied: id = msg![env; value copy];
            env.objc.associated_objects.insert(map_key, (copied, true));
            return;
        }
        // OBJC_ASSOCIATION_ASSIGN (0) and any unknown policy: weak storage.
        _ => false,
    };

    env.objc.associated_objects.insert(map_key, (value, is_retained));
}

/// `id objc_getAssociatedObject(id object, const void *key)`
///
/// Returns the value associated with a given object for a given key.
/// <https://developer.apple.com/documentation/objectivec/1418625-objc_getassociatedobject>
fn objc_getAssociatedObject(
    env: &mut Environment,
    object: id,
    key: crate::mem::ConstVoidPtr,
) -> id {
    if object.is_null() {
        return nil;
    }
    env.objc
        .associated_objects
        .get(&(object, key.to_bits()))
        .map(|(val, _)| *val)
        .unwrap_or(nil)
}

/// `void objc_removeAssociatedObjects(id object)`
///
/// Removes all associations for a given object.
/// <https://developer.apple.com/documentation/objectivec/1418718-objc_removeassociatedobjects>
fn objc_removeAssociatedObjects(env: &mut Environment, object: id) {
    if object.is_null() {
        return;
    }
    // Collect keys and values to avoid borrowing issues.
    let to_remove: Vec<((id, crate::mem::GuestUSize), (id, bool))> = env
        .objc
        .associated_objects
        .iter()
        .filter(|((obj, _), _)| *obj == object)
        .map(|(k, v)| (*k, *v))
        .collect();

    for (k, (val, was_retained)) in to_remove {
        env.objc.associated_objects.remove(&k);
        if was_retained && !val.is_null() {
            release(env, val);
        }
    }
}

pub const DYLIB: HostDylib = HostDylib {
    path: "/usr/lib/libobjc.A.dylib",
    aliases: &["/usr/lib/libobjc.dylib"],
    class_exports: &[],
    constant_exports: &[CONSTANTS],
    function_exports: &[FUNCTIONS, weak::FUNCTIONS],
};

const CONSTANTS: ConstantExports = &[
    // We don't use these in our Objective-C runtime, but exporting useless
    // symbols for these silences the warning about the unhandled relocation,
    // and avoids a linker error for the integration tests.
    ("__objc_empty_vtable", HostConstant::NullPtr),
    ("__objc_empty_cache", HostConstant::NullPtr),
    ("_OBJC_EHTYPE_$_NSException", HostConstant::NullPtr),
    ("_OBJC_EHTYPE_id", HostConstant::NullPtr),
    // `NSObject`'s only ivar (`isa`) lives at offset 0 in the object layout
    // on 32-bit iOS, so resolving the ivar-offset symbol to a 4-byte zero
    // gives any binary that does `obj + _OBJC_IVAR_$_NSObject.isa` the
    // correct address (i.e. the object base).
    ("_OBJC_IVAR_$_NSObject.isa", HostConstant::NullPtr),
    ("_kCFTypeArrayCallBacks", HostConstant::NullPtr),
    ("_NSKeyValueChangeNewKey", HostConstant::NSString("new")),
];

const FUNCTIONS: FunctionExports = &[
    export_c_func!(objc_msgSend(_, _)),
    export_c_func!(objc_msgSend_stret(_, _, _)),
    export_c_func!(objc_msgSendSuper2_stret(_, _)),
    export_c_func!(objc_msgSendSuper2(_, _)),
    export_c_func!(objc_getProperty(_, _, _, _)),
    export_c_func!(objc_setProperty(_, _, _, _, _, _)),
    export_c_func!(objc_setProperty_nonatomic_copy(_, _, _, _)),
    export_c_func!(objc_setProperty_atomic_copy(_, _, _, _)),
    export_c_func!(objc_setProperty_atomic(_, _, _, _)),
    export_c_func!(objc_copyStruct(_, _, _, _, _)),
    export_c_func!(objc_sync_enter(_)),
    export_c_func!(objc_sync_exit(_)),
    export_c_func!(sel_registerName(_)),
    export_c_func!(objc_getClass(_)),
    export_c_func!(objc_getMetaClass(_, _)),
    export_c_func!(object_getClassName(_)),
    export_c_func!(object_getClass(_)),
    export_c_func!(objc_retainAutoreleasedReturnValue(_)),
    export_c_func!(objc_unsafeClaimAutoreleasedReturnValue(_)),
    export_c_func!(objc_autoreleaseReturnValue(_)),
    export_c_func!(objc_retainAutoreleaseReturnValue(_)),
    export_c_func!(objc_autoreleasePoolPush(_)),
    export_c_func!(objc_autoreleasePoolPop(_)),
    export_c_func!(objc_retain(_)),
    export_c_func!(objc_retainAutorelease(_)),
    export_c_func!(objc_alloc(_)),
    export_c_func!(objc_autorelease(_)),
    export_c_func!(objc_retainBlock(_)),
    export_c_func!(objc_release(_)),
    export_c_func!(objc_setProperty_nonatomic(_, _, _, _)),
    export_c_func!(objc_exception_throw(_)),
    export_c_func!(objc_begin_catch(_)),
    export_c_func!(objc_end_catch(_)),
    export_c_func!(class_getSuperclass(_)),
    export_c_func!(class_getInstanceSize(_, _)),
    export_c_func!(class_getInstanceMethod(_, _)),
    export_c_func!(class_getClassMethod(_, _)),
    export_c_func!(class_respondsToSelector(_, _)),
    export_c_func!(class_addMethod(_, _, _, _)),
    export_c_func!(class_getProperty(_, _)),
    export_c_func!(class_copyPropertyList(_, _)),
    export_c_func!(class_copyMethodList(_, _)),
    export_c_func!(class_copyIvarList(_, _)),
    export_c_func!(class_copyProtocolList(_, _)),
    export_c_func!(class_replaceMethod(_, _)),
    export_c_func!(method_getImplementation(_, _)),
    export_c_func!(method_setImplementation(_, _)),
    export_c_func!(method_getTypeEncoding(_, _)),
    export_c_func!(method_exchangeImplementations(_, _)),
    export_c_func!(class_getMethodImplementation(_, _)),
    export_c_func!(class_getMethodImplementation_stret(_, _)),
    export_c_func!(objc_storeStrong(_, _)),
    export_c_func!(___objc_personality_v0(_, _, _, _, _)),
    export_c_func_aliased!("_objc_msgForward", objc_msgForward(_, _)),
    export_c_func_aliased!("_objc_msgForward_stret", objc_msgForward_stret(_, _, _)),
    // Additional ObjC runtime helpers needed by iOS 5/6 binaries.
    export_c_func!(class_getName(_)),
    export_c_func!(objc_lookUpClass(_)),
    export_c_func!(objc_getRequiredClass(_)),
    export_c_func!(objc_allocateClassPair(_, _, _)),
    export_c_func!(objc_readClassPair(_, _)),
    export_c_func!(objc_registerClassPair(_)),
    export_c_func!(objc_disposeClassPair(_)),
    export_c_func!(class_setSuperclass(_, _)),
    export_c_func!(objc_getProtocol(_)),
    export_c_func!(protocol_getName(_)),
    export_c_func!(objc_getClassList(_, _)),
    export_c_func!(protocol_conformsToProtocol(_, _)),
    export_c_func!(objc_copyClassNamesForImage(_, _)),
    export_c_func!(object_getIndexedIvars(_)),
    // `_objc_deallocOnMainThreadHelper` (one leading underscore in the C
    // name) is exported by libobjc as the Mach-O symbol
    // `__objc_deallocOnMainThreadHelper` (two leading underscores). The
    // Rust function uses two leading underscores so that the alias above
    // matches the Mach-O symbol exactly.
    export_c_func_aliased!(
        "_objc_deallocOnMainThreadHelper",
        __objc_deallocOnMainThreadHelper(_)
    ),
    // Associated objects — available since iOS 3.1 / Mac OS X 10.6.
    // Reference: <https://developer.apple.com/documentation/objectivec/1418509-objc_setassociatedobject>
    export_c_func!(objc_setAssociatedObject(_, _, _, _)),
    export_c_func!(objc_getAssociatedObject(_, _)),
    export_c_func!(objc_removeAssociatedObjects(_)),
];
