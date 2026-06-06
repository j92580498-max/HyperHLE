/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `NSNotificationCenter`.

use super::ns_notification::NSNotificationName;
use super::ns_string;

use crate::abi::{CallFromHost, GuestFunction};
use crate::objc::{
    id, msg, msg_class, msg_send, nil, objc_classes, release, retain, ClassExports, HostObject,
    NSZonePtr, SEL,
};
use std::borrow::Cow;
use std::collections::HashMap;

#[derive(Default)]
pub struct State {
    default_center: Option<id>,
}

/// A registered observer.
///
/// Two flavours are supported:
/// 1. Classic target/selector pair (`-addObserver:selector:name:object:`).
/// 2. Block-based observer (`-addObserverForName:object:queue:usingBlock:`).
///    The `observer` field then points at the opaque token object that owns
///    the block, and `block` carries the block id that must be invoked when
///    the notification is delivered.
#[derive(Clone)]
struct Observer {
    observer: id,
    selector: SEL,
    object: id,
    /// Non-nil for block-based observers. The block is `^void(NSNotification *)`.
    block: id,
}

#[derive(Default)]
struct NSNotificationCenterHostObject {
    observers: HashMap<Cow<'static, str>, Vec<Observer>>,
}
impl HostObject for NSNotificationCenterHostObject {}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation NSNotificationCenter: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(NSNotificationCenterHostObject {
        observers: HashMap::new(),
    });
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

+ (id)defaultCenter {
    if let Some(c) = env.framework_state.foundation.ns_notification_center.default_center {
        c
    } else {
        let new: id = msg![env; this new];
        env.framework_state.foundation.ns_notification_center.default_center = Some(new);
        new
    }
}

- (())dealloc {
    let host_obj = env.objc.borrow_mut::<NSNotificationCenterHostObject>(this);
    let observers = std::mem::take(&mut host_obj.observers);
    for observer in observers.values().flatten() {
        release(env, observer.object);
        if observer.block != nil {
            // For block-based observers, also release the heap-copied block
            // and the opaque token we vended out.
            release(env, observer.block);
            release(env, observer.observer);
        }
    }
    env.objc.dealloc_object(this, &mut env.mem);
}

- (())addObserver:(id)observer
         selector:(SEL)selector
             name:(NSNotificationName)name
           object:(id)object {
    if name == nil &&
        env.bundle.bundle_identifier().starts_with("com.chillingo.cuttherope") &&
        selector == env.objc.lookup_selector("fetchUpdateNotification:").unwrap() {
        // As we nullified Flurry SDK, we also need to no-op
        // related notifications
        log!("Applying game-specific hack for Cut the Rope: ignoring addObserver:selector:name:object: for fetchUpdateNotification:");
        return;
    }
    // Per Apple: `name = nil` means "match notifications of any name from
    // `object`". We store nil-name observers under the empty-string key and
    // merge them in at `-postNotification:` time.
    let name: Cow<'static, str> = if name == nil {
        Cow::Borrowed("")
    } else {
        ns_string::to_rust_string(env, name)
    };

    log_dbg!(
        "[(NSNotificationCenter*){:?} addObserver:{:?} selector:{:?} name:{:?} object:{:?}",
        this,
        observer,
        selector,
        name,
        object,
    );

    // When adding an observer, only the object is retained so it doesn't get
    // deallocated before the notification is delivered. Some apps, such as
    // Dungeon Hunter 2, rely on this being the case.
    // The observer is not retained to avoid retain cycles.
    // https://stackoverflow.com/a/36582937
    // While not explicitly stated by the documentation, there's a paragraph
    // that hints at this behavior:
    // "If your app targets iOS 9.0 and later or macOS 10.11 and later, you do
    // not need to unregister an observer that you created with this function.
    // If you forget or are unable to remove an observer, the system cleans up
    // the next time it would have posted to it."
    // https://developer.apple.com/documentation/foundation/notificationcenter/addobserver(_:selector:name:object:)?language=objc
    // Implying that prior to these versions, it's unsafe to not remove an
    // observer. It's been observed that some apps expect and rely on this
    // behavior, such as Marmalade SDK games that use the Movie Player
    // (Pandemonium and COD Zombies, for example).

    retain(env, object);

    let host_obj = env.objc.borrow_mut::<NSNotificationCenterHostObject>(this);
    host_obj.observers.entry(name).or_default().push(Observer {
        observer,
        selector,
        object,
        block: nil,
    });
}

// `- (id<NSObject>)addObserverForName:(NSNotificationName)name
//                              object:(id)obj
//                               queue:(NSOperationQueue *)queue
//                          usingBlock:(void (^)(NSNotification *note))block;`
//
// Per Apple's [NSNotificationCenter Reference](https://developer.apple.com/documentation/foundation/nsnotificationcenter/1411723-addobserverforname):
// returns an opaque observer token, distinct from the receiver, that the
// caller must keep a strong reference to. When a notification matching
// `name`/`obj` is posted, the system enqueues `block` on `queue` (or
// invokes it synchronously on the posting thread when `queue` is nil)
// passing the notification as its only argument. touchHLE has no
// `NSOperationQueue` scheduling — we always invoke `block` synchronously
// from `-postNotification:`, which matches the `queue == nil` semantics
// Apple documents.
//
// Implementation note: the returned token is itself a freshly allocated
// `NSObject`. We store the block alongside the token in our observer
// list (with selector = a zero SEL, since it won't be used). `block` is
// retained — Apple's API contract is that the framework keeps the block
// alive until `-removeObserver:` is called.
- (id)addObserverForName:(NSNotificationName)name
                  object:(id)object
                   queue:(id)_queue
              usingBlock:(id)block {
    // The token can be any unique NSObject; an instance of NSObject is what
    // Apple's runtime uses too.
    let token: id = msg_class![env; NSObject new];

    // Block must be copied to the heap (Apple's contract for blocks captured
    // by the framework). `-copy` on a stack block produces a heap block; on a
    // heap block it's a retain.
    let block_copy: id = msg![env; block copy];

    // Retain the optional `object` filter so it can't dangle while we're
    // observing — mirrors the behaviour of -addObserver:selector:name:object:.
    retain(env, object);

    let name_key: Cow<'static, str> = if name == nil {
        Cow::Borrowed("")
    } else {
        ns_string::to_rust_string(env, name)
    };

    log_dbg!(
        "[(NSNotificationCenter*){:?} addObserverForName:{:?} object:{:?} queue:_ usingBlock:{:?}] -> token {:?}",
        this, name_key, object, block_copy, token
    );

    let host_obj = env.objc.borrow_mut::<NSNotificationCenterHostObject>(this);
    host_obj.observers.entry(name_key).or_default().push(Observer {
        observer: token,
        selector: SEL::null(),
        object,
        block: block_copy,
    });

    token
}

- (())removeObserver:(id)observer {
    msg![env; this removeObserver:observer name:nil object:nil]
}

- (())removeObserver:(id)observer
                name:(NSNotificationName)name
              object:(id)object {
    assert!(observer != nil); // TODO

    let name = if name == nil {
        None
    } else {
        // Usually a static string, so no real copy will happen
        Some(ns_string::to_rust_string(env, name))
    };

    log_dbg!(
        "[(NSNotificationCenter*){:?} removeObserver:{:?} name:{:?} object:{:?}",
        this,
        observer,
        name,
        object,
    );

    // TODO: is this the correct behaviour, can an observer be registered
    // several times?
    let mut removed_observers = Vec::new();

    let host_obj = env.objc.borrow_mut::<NSNotificationCenterHostObject>(this);
    if let Some(ref name) = name {
        let Some(observers) = host_obj.observers.get_mut(name) else {
            return;
        };
        remove_observers_internal(observers, &mut removed_observers, observer, object);
    } else {
        for observers in host_obj.observers.values_mut() {
            remove_observers_internal(observers, &mut removed_observers, observer, object);
        }
    };

    for removed_observer in removed_observers {
        release(env, removed_observer.object);
        if removed_observer.block != nil {
            release(env, removed_observer.block);
            // The observer field holds the token we vended out; release
            // the runtime's own retain on it.
            release(env, removed_observer.observer);
        }
    }
}

- (())postNotification:(id)notification {
    log_dbg!(
        "[(NSNotificationCenter*){:?} postNotification:{:?}]",
        this,
        notification,
    );

    let name_id: id = msg![env; notification name];
    // Usually a static string, so no real copy will happen.
    let name: Cow<'static, str> = if name_id == nil {
        Cow::Borrowed("")
    } else {
        ns_string::to_rust_string(env, name_id)
    };

    let notification_poster: id = msg![env; notification object];

    log_dbg!("Notification is a {:?} posted by {:?}", name, notification_poster);

    // Collect observers matching this notification: those registered for
    // exactly this name plus those registered with `name = nil` (stored
    // under the empty-string key). Both buckets are filtered by the
    // `object` predicate inside the dispatch loop below.
    let observers: Vec<Observer> = {
        let host_obj = env.objc.borrow::<NSNotificationCenterHostObject>(this);
        let mut merged: Vec<Observer> = Vec::new();
        if let Some(named) = host_obj.observers.get(&name) {
            merged.extend(named.iter().cloned());
        }
        // Avoid double-dispatching when the notification's own name is "".
        if !name.is_empty() {
            if let Some(any_name) = host_obj.observers.get(&Cow::Borrowed("")) {
                merged.extend(any_name.iter().cloned());
            }
        }
        merged
    };
    if observers.is_empty() {
        return;
    }
    for Observer { observer, selector, object, block } in observers {
        // The object argument is a filter for which notification sources the
        // observer is interested in.
        if object != nil && notification_poster != object {
            continue;
        }

        if block != nil {
            // Block-based observer (added via
            // -addObserverForName:object:queue:usingBlock:). Invoke
            //   block(notification)
            // using the standard 32-bit iOS block layout:
            //     struct Block_layout {
            //         void *isa, int flags, int reserved,
            //         void (*invoke)(void *block, ...),
            //         ...
            //     };
            // The invoke function expects the block id as its first arg and
            // the notification as its second arg.
            log_dbg!(
                "Notification {:?} observed, invoking block {:?} (token {:?})",
                notification, block, observer
            );
            let invoke_ptr: u32 = env.mem.read(block.cast::<u32>() + 3u32);
            if invoke_ptr != 0 {
                let invoke = GuestFunction::from_addr_with_thumb_bit(invoke_ptr);
                retain(env, observer);
                let _: () = invoke.call_from_host(env, (block, notification));
                release(env, observer);
            } else {
                log!("Warning: notification block {:?} has NULL invoke pointer; skipping", block);
            }
            continue;
        }

        log_dbg!(
            "Notification {:?} observed, sending {:?} message to {:?}",
            notification,
            selector.as_str(&env.mem),
            observer
        );

        // In some cases, observer could be removed during the
        // processing of the notification, effectively releasing it.
        // (This is happening with Spore Origins)
        // We need to retain it for correctness.
        retain(env, observer);
        // Signature should be `- (void)notification:(NSNotification *)notif`.
        let _: () = msg_send(env, (observer, selector, notification));
        release(env, observer);
    }
}
- (())postNotificationName:(NSNotificationName)name
                    object:(id)object {
    msg![env; this postNotificationName:name
                                 object:object
                               userInfo:nil]
}
- (())postNotificationName:(NSNotificationName)name
                    object:(id)object
                  userInfo:(id)user_info { // NSDictionary*
    let notification: id = msg_class![env; NSNotification alloc];
    let notification: id = msg![env; notification initWithName:name
                                                        object:object
                                                      userInfo:user_info];
    let _: () = msg![env; this postNotification:notification];
    release(env, notification);
}

@end

};

/// A helper function to populate `removed_observers` with observers
/// removed from `observers` based on `observer` and `object` criteria.
fn remove_observers_internal(
    observers: &mut Vec<Observer>,
    removed_observers: &mut Vec<Observer>,
    observer: id,
    object: id,
) {
    let mut i = 0;
    while i < observers.len() {
        if observers[i].observer == observer && (object == nil || object == observers[i].object) {
            removed_observers.push(observers.swap_remove(i));
        } else {
            i += 1;
        }
    }
}
