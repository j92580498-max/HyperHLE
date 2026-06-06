/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `NSAutoreleasePool`.

use crate::objc::{id, msg, objc_classes, release, ClassExports, HostObject, NSZonePtr};
use crate::{Environment, ThreadId};
use std::collections::HashMap;
use std::num::NonZeroU32;

#[derive(Default)]
pub struct State {
    pool_stacks: HashMap<ThreadId, Vec<id>>,
}
impl State {
    fn get(env: &mut Environment) -> &mut Self {
        &mut env.framework_state.foundation.ns_autorelease_pool
    }
}

#[derive(Default)]
struct NSAutoreleasePoolHostObject {
    original_thread: ThreadId,
    /// This is allowed to contain duplicates, which get released several times!
    objects: Vec<id>,
}
impl HostObject for NSAutoreleasePoolHostObject {}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation NSAutoreleasePool: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(NSAutoreleasePoolHostObject {
        original_thread: env.current_thread,
        objects: Vec::new(),
    });
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

+ (())addObject:(id)obj {
    let current_thread = env.current_thread;
    if let Some(current_pool) = State::get(env)
        .pool_stacks
        .get(&current_thread)
        .and_then(|pool_stack| pool_stack.last().copied())
    {
        msg![env; current_pool addObject:obj]
    } else {
        log_dbg!(
            "Warning: no active NSAutoreleasePool, leaking {:?}, current thread {}",
            obj,
            current_thread
        );
    }
}

- (id)init {
    let current_thread = env.current_thread;
    let pool_stack = State::get(env).pool_stacks
        .entry(current_thread)
        .or_default();
    pool_stack.push(this);
    log_dbg!("New pool: {:?}, current thread {}", this, current_thread);
    this
}

- (())addObject:(id)obj {
    env.objc.borrow_mut::<NSAutoreleasePoolHostObject>(this).objects.push(obj);
}

- (id)retain {
    // Per Cocoa docs, sending retain to an NSAutoreleasePool throws an
    // exception. We don't have full exception support; log and return self
    // without bumping the refcount so the guest stays alive.
    log!(
        "Warning: -[{:?} (NSAutoreleasePool) retain] is not allowed; returning self.",
        this
    );
    this
}
- (id)autorelease {
    // Same situation as `retain` above — Cocoa would raise here. Log and
    // return self instead of panicking the host.
    log!(
        "Warning: -[{:?} (NSAutoreleasePool) autorelease] is not allowed; returning self.",
        this
    );
    this
}

- (i32)intValue {
    // Workaround: Некоторые старые игры имеют баги с висячими указателями
    // (dangling pointers).
    // Они обращаются к пулу автоосвобождения, думая, что это NSNumber.
    // Мы возвращаем 0, чтобы игра корректно продолжила работу без спама в
    // логах.
    0
}

- (())drain {
    msg![env; this release]
}

- (())dealloc {
    let current_thread = env.current_thread;
    log_dbg!(
        "Draining pool: {:?}, current thread {}",
        this,
        current_thread
    );
    let host_obj: &mut NSAutoreleasePoolHostObject = env.objc.borrow_mut(this);
    if host_obj.original_thread != current_thread {
        log!(
            "Warning: draining NSAutoreleasePool {:?} on thread {} but it was \
             created on thread {}; behaviour is undefined per Cocoa, continuing anyway.",
            this,
            current_thread,
            host_obj.original_thread
        );
    }
    let original_thread = host_obj.original_thread;
    // We resolve the pool stack for the thread the pool was originally created
    // on, falling back to the current thread if that one has none.
    let lookup_thread = if env
        .framework_state
        .foundation
        .ns_autorelease_pool
        .pool_stacks
        .contains_key(&original_thread)
    {
        original_thread
    } else {
        current_thread
    };
    let Some(pool_stack) = env
        .framework_state
        .foundation
        .ns_autorelease_pool
        .pool_stacks
        .get_mut(&lookup_thread)
    else {
        log!(
            "Warning: -[{:?} (NSAutoReleasePool) release] on thread {} found no \
             pool stack for thread {}; ignoring.",
            this,
            env.current_thread,
            lookup_thread
        );
        let host_obj: &mut NSAutoreleasePoolHostObject = env.objc.borrow_mut(this);
        let objects = std::mem::take(&mut host_obj.objects);
        env.objc.dealloc_object(this, &mut env.mem);
        for object in objects {
            release(env, object);
        }
        return;
    };
    // NSAutoReleasePool seems to keep popping until reaches the appropriate
    // pool object. If there are pools that are "above" it in the stack, it
    // deallocates them as well.
    let Some((index, _)) = pool_stack
        .iter()
        .enumerate()
        .rev()
        .find(|(_, pool)| **pool == this)
    else {
        log!(
            "Warning: -[{:?} (NSAutoReleasePool) release] on thread {} but the pool \
             is not on the active pool stack; dealloc'ing without draining the stack.",
            this,
            env.current_thread
        );
        let host_obj: &mut NSAutoreleasePoolHostObject = env.objc.borrow_mut(this);
        let objects = std::mem::take(&mut host_obj.objects);
        env.objc.dealloc_object(this, &mut env.mem);
        for object in objects {
            release(env, object);
        }
        return;
    };
    let to_drop: Vec<id> = pool_stack.drain(index..).collect();
    log_dbg!("Dropping pools {:?}", to_drop);
    for pool in to_drop.into_iter().rev() {
        if pool != this {
            // It's a bit ugly, but we cannot call a release on those other
            // pools as we already drained the shared pool stacks.
            // So we manually decrement and dealloc instead.
            // TODO: refactor this
            let rc = env.objc.get_refcount(pool);
            if rc != NonZeroU32::new(1).unwrap() {
                log!(
                    "Warning: stacked NSAutoreleasePool {:?} has refcount {} \
                     (expected 1) when its parent was drained; force-dealloc'ing anyway.",
                    pool,
                    rc
                );
            }
            _ = env.objc.decrement_refcount(pool);
        }
        let host_obj: &mut NSAutoreleasePoolHostObject = env.objc.borrow_mut(pool);
        let objects = std::mem::take(&mut host_obj.objects);
        env.objc.dealloc_object(pool, &mut env.mem);
        for object in objects {
            release(env, object);
        }
    }
}

@end

};
