/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `NSLock family`.
//!
//! TODO: There is probably an opportunity to refactor common methods.
//! (Need to find a good way to do so! Common super class wouldn't
//! work as it breaks expected inheritance chain.)

use crate::environment::MutexType::PTHREAD_MUTEX_RECURSIVE;
use crate::environment::{MutexId, PTHREAD_MUTEX_DEFAULT};
use crate::msg;
use crate::objc::{id, nil, objc_classes, ClassExports, HostObject};
use std::sync::{Arc, Condvar, Mutex};

// NSInteger is i32 on 32-bit ARM.
type NSInteger = i32;

#[derive(Default)]
struct NSLockHostObject {
    mutex_id: MutexId,
    name: id,
}
impl HostObject for NSLockHostObject {}

// ---------------------------------------------------------------------------
// NSConditionLock
// ---------------------------------------------------------------------------

struct NSConditionLockState {
    locked: bool,
    condition: NSInteger,
}

struct NSConditionLockHostObject {
    /// Arc lets lock methods clone the reference, drop the ObjC borrow,
    /// and then block on the condvar without holding a borrow on env.objc.
    inner: Arc<(Mutex<NSConditionLockState>, Condvar)>,
}
impl HostObject for NSConditionLockHostObject {}

// ---------------------------------------------------------------------------

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation NSLock: NSObject

+ (id)alloc {
    log_dbg!("[NSLock alloc]");
    let mutex_id = env.mutex_state.init_mutex(PTHREAD_MUTEX_DEFAULT);
    let host_object = NSLockHostObject { mutex_id, name: nil };
    env.objc.alloc_object(this, Box::new(host_object), &mut env.mem)
}

// NSLocking protocol implementation
- (())lock {
    log_dbg!("[(NSLock *){:?} lock]", this);
    let host_object = env.objc.borrow::<NSLockHostObject>(this);
    env.lock_mutex(host_object.mutex_id).unwrap();
}
- (())unlock {
    log_dbg!("[(NSLock *){:?} unlock]", this);
    let host_object = env.objc.borrow::<NSLockHostObject>(this);
    if !env.mutex_state.mutex_is_locked(host_object.mutex_id) {
        echo!("*** -[NSLock unlock]: lock (<NSLock: {:?}> '{:?}') unlocked when not locked", this, host_object.name);
    }
    if let Err(e) = env.unlock_mutex(host_object.mutex_id) {
        // Error 16 = EBUSY, Error 1 = EPERM. On real iOS these are
        // non-fatal (the unlock simply fails silently for NSLock).
        // Apps like EnbornX trigger this during shutdown sequences
        // when locks are released out of order.
        log_dbg!(
            "*** -[NSLock unlock]: unlock_mutex returned error {} for lock {:?}, ignoring",
            e, this
        );
    }
}

- (bool)tryLock {
    let host_object = env.objc.borrow::<NSLockHostObject>(this);
    if env.mutex_state.mutex_is_locked(host_object.mutex_id) {
        false
    } else {
        env.lock_mutex(host_object.mutex_id).is_ok()
    }
}

- (())setName:(id)name { // NSString *
    // @property(copy), name has to be copied
    env.objc.borrow_mut::<NSLockHostObject>(this).name = msg![env; name copy];
}
- (id)name {
    env.objc.borrow::<NSLockHostObject>(this).name
}

- (())dealloc {
    log_dbg!("[(NSLock *){:?} dealloc]", this);
    let host_object = env.objc.borrow::<NSLockHostObject>(this);
    let _ = env.mutex_state.destroy_mutex(host_object.mutex_id);
    env.objc.dealloc_object(this, &mut env.mem)
}

@end

@implementation NSRecursiveLock: NSObject

+ (id)alloc {
    log_dbg!("[NSRecursiveLock alloc]");
    let mutex_id = env.mutex_state.init_mutex(PTHREAD_MUTEX_RECURSIVE);
    let host_object = NSLockHostObject { mutex_id, name: nil };
    env.objc.alloc_object(this, Box::new(host_object), &mut env.mem)
}

// NSLocking protocol implementation
- (())lock {
    log_dbg!("[(NSRecursiveLock *){:?} lock]", this);
    let host_object = env.objc.borrow::<NSLockHostObject>(this);
    env.lock_mutex(host_object.mutex_id).unwrap();
}
- (())unlock {
    log_dbg!("[(NSRecursiveLock *){:?} unlock]", this);
    let host_object = env.objc.borrow::<NSLockHostObject>(this);
    if !env.mutex_state.mutex_is_locked(host_object.mutex_id) {
        echo!("*** -[NSRecursiveLock unlock]: lock (<NSRecursiveLock: {:?}> '{:?}') unlocked when not locked", this, host_object.name);
    }
    if let Err(e) = env.unlock_mutex(host_object.mutex_id) {
        log_dbg!(
            "*** -[NSRecursiveLock unlock]: unlock_mutex returned error {} for lock {:?}, ignoring",
            e, this
        );
    }
}

- (bool)tryLock {
    let host_object = env.objc.borrow::<NSLockHostObject>(this);
    if env.mutex_state.mutex_is_locked(host_object.mutex_id) {
        false
    } else {
        env.lock_mutex(host_object.mutex_id).is_ok()
    }
}

- (())setName:(id)name { // NSString *
    // @property(copy), name has to be copied
    env.objc.borrow_mut::<NSLockHostObject>(this).name = msg![env; name copy];
}
- (id)name {
    env.objc.borrow::<NSLockHostObject>(this).name
}

- (())dealloc {
    log_dbg!("[(NSRecursiveLock *){:?} dealloc]", this);
    let host_object = env.objc.borrow::<NSLockHostObject>(this);
    let _ = env.mutex_state.destroy_mutex(host_object.mutex_id);
    env.objc.dealloc_object(this, &mut env.mem)
}

@end


};
