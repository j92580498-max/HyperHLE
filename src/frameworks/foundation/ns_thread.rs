/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `NSThread`.

use super::NSTimeInterval;
use crate::dyld::HostFunction;
use crate::frameworks::core_foundation::CFTypeRef;
use crate::frameworks::foundation::ns_string;
use crate::frameworks::foundation::{ns_time_interval_to_duration, NSUInteger};
use crate::libc::pthread::thread::{
    pthread_attr_init, pthread_attr_setdetachstate, pthread_attr_setstacksize, pthread_attr_t,
    pthread_create, pthread_self, pthread_t, PTHREAD_CREATE_DETACHED,
};
use crate::mem::{guest_size_of, Mem, MutPtr};
use crate::objc::{
    id, msg_send, msg_send_no_type_checking, nil, objc_classes, release, retain,
    Class, ClassExports, HostObject, NSZonePtr, SEL,
};
use crate::Environment;
use crate::{msg, msg_class};
use std::collections::HashMap;

#[derive(Default)]
pub struct State {
    is_multi_threaded: bool,
    ns_threads: HashMap<pthread_t, id>,
}
impl State {
    fn get(env: &mut Environment) -> &mut Self {
        &mut env.framework_state.foundation.ns_thread
    }
}

#[derive(Default)]
struct NSThreadHostObject {
    target: id,
    selector: Option<SEL>,
    object: id,
    /// `NSMutableDictionary*`
    thread_dictionary: id,
    /// `NSString*` — human-readable name
    name: id,
    owned: bool,
    finished: bool,
    executing: bool,
    cancelled: bool,
    stack_size: NSUInteger,
    tolerate_type_mismatch: bool,
    is_main_thread: bool,
    /// Thread quality-of-service (NSQualityOfService enum, stored as i32)
    quality_of_service: i32,
    /// Thread priority 0.0–1.0
    thread_priority: f64,
}
impl HostObject for NSThreadHostObject {}

// NSQualityOfService constants
const QOS_CLASS_USER_INTERACTIVE: i32 = 0x21;
const QOS_CLASS_USER_INITIATED: i32 = 0x19;
const QOS_CLASS_DEFAULT: i32 = 0x15;
const QOS_CLASS_UTILITY: i32 = 0x11;
const QOS_CLASS_BACKGROUND: i32 = 0x09;

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation NSThread: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(NSThreadHostObject {
        target: nil,
        selector: None,
        object: nil,
        thread_dictionary: nil,
        name: nil,
        owned: false,
        finished: false,
        executing: false,
        cancelled: false,
        stack_size: Mem::SECONDARY_THREAD_DEFAULT_STACK_SIZE,
        tolerate_type_mismatch: false,
        is_main_thread: false,
        quality_of_service: QOS_CLASS_DEFAULT,
        thread_priority: 0.5,
    });
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

// =========================================================================
// MARK: - Class methods
// =========================================================================

+ (bool)isMultiThreaded {
    env.framework_state.foundation.ns_thread.is_multi_threaded
}

+ (bool)isMainThread {
    env.current_thread == 0
}

+ (id)mainThread {
    // Collect all (pthread_t, ns_thread id) pairs first to avoid holding
    // the mutable borrow on env while also borrowing env.objc in the closure.
    let pairs: Vec<(pthread_t, id)> = State::get(env)
        .ns_threads
        .iter()
        .map(|(&pt, &ns_t)| (pt, ns_t))
        .collect();
    // Mutable borrow on env via State::get ends here.

    for (_pt, ns_t) in &pairs {
        if env.objc.borrow::<NSThreadHostObject>(*ns_t).is_main_thread {
            return *ns_t;
        }
    }

    // Create the main thread object on demand.
    let ns_thread: id = msg_class![env; NSThread alloc];
    let ns_thread: id = msg![env; ns_thread init];
    env.objc.borrow_mut::<NSThreadHostObject>(ns_thread).is_main_thread = true;
    ns_thread
}

+ (id)currentThread {
    let pthread = pthread_self(env);
    #[allow(clippy::map_entry)]
    if !State::get(env).ns_threads.contains_key(&pthread) {
        let ns_thread: id = msg_class![env; NSThread alloc];
        let ns_thread: id = msg![env; ns_thread init];
        if env.current_thread == 0 {
            env.objc.borrow_mut::<NSThreadHostObject>(ns_thread).is_main_thread = true;
        }
        State::get(env).ns_threads.insert(pthread, ns_thread);
    }
    *State::get(env).ns_threads.get(&pthread).unwrap()
}

+ (id)callStackReturnAddresses {
    log!("WARNING: [NSThread callStackReturnAddresses] — returning empty array");
    msg_class![env; NSArray new]
}

+ (id)callStackSymbols {
    log!("WARNING: [NSThread callStackSymbols] — returning empty array");
    msg_class![env; NSArray new]
}

+ (())sleepForTimeInterval:(NSTimeInterval)ti {
    log_dbg!("[NSThread sleepForTimeInterval:{:?}]", ti);
    if let Some(d) = ns_time_interval_to_duration(ti) {
        env.sleep(d);
    } else {
        log!(
            "Warning: [NSThread sleepForTimeInterval:{:?}] given an out-of-range \
             value (NaN/inf/negative); not sleeping.",
            ti
        );
    }
}

+ (())sleepUntilDate:(id)date { // NSDate*
    let ti: NSTimeInterval = msg![env; date timeIntervalSinceNow];
    if let Some(d) = ns_time_interval_to_duration(ti) {
        if !d.is_zero() {
            env.sleep(d);
        }
    } else {
        log!(
            "Warning: [NSThread sleepUntilDate:] computed an out-of-range \
             interval ({:?}); not sleeping.",
            ti
        );
    }
}

+ (())exit {
    // Terminate the current thread. In touchHLE this is a best-effort stub;
    // we log and let the thread function return naturally.
    log!("TODO: [NSThread exit] — thread will finish at next natural return point");
}

+ (f64)threadPriority {
    let thread: id = msg![env; this currentThread];
    msg![env; thread threadPriority]
}

+ (bool)setThreadPriority:(f64)priority {
    let thread: id = msg![env; this currentThread];
    msg![env; thread setThreadPriority:priority]
}

+ (())detachNewThreadSelector:(SEL)selector
                     toTarget:(id)target
                   withObject:(id)object {
    detach_new_thread_inner(env, selector, target, object, false)
}

// =========================================================================
// MARK: - Initializers
// =========================================================================

- (id)init {
    this
}

- (id)initWithTarget:(id)target
            selector:(SEL)selector
              object:(id)object {
    retain(env, target);
    retain(env, object);
    {
        let host = env.objc.borrow_mut::<NSThreadHostObject>(this);
        host.target = target;
        host.selector = Some(selector);
        host.object = object;
    }
    this
}

// =========================================================================
// MARK: - Dealloc
// =========================================================================

- (())dealloc {
    log_dbg!("[(NSThread*){:?} dealloc]", this);
    let host = env.objc.borrow::<NSThreadHostObject>(this);
    let (thread_dictionary, name) = (host.thread_dictionary, host.name);
    release(env, thread_dictionary);
    release(env, name);
    env.objc.dealloc_object(this, &mut env.mem)
}

// =========================================================================
// MARK: - Lifecycle
// =========================================================================

- (())start {
    log_once!("First [NSThread start] (app spawned a worker thread)");

    let symb = "__touchHLE_NSThreadInvocationHelper";
    let hf: HostFunction = &(_touchHLE_NSThreadInvocationHelper as fn(&mut Environment, _) -> _);
    let gf = env.dyld.create_guest_function(&mut env.mem, symb, hf);

    let attr: MutPtr<pthread_attr_t> = env.mem.alloc(guest_size_of::<pthread_attr_t>()).cast();
    pthread_attr_init(env, attr);
    let stack_size = env.objc.borrow::<NSThreadHostObject>(this).stack_size;
    pthread_attr_setstacksize(env, attr, stack_size);
    pthread_attr_setdetachstate(env, attr, PTHREAD_CREATE_DETACHED);

    let thread_ptr: MutPtr<pthread_t> = env.mem.alloc(guest_size_of::<pthread_t>()).cast();
    pthread_create(env, thread_ptr, attr.cast_const(), gf, this.cast());

    let pthread = env.mem.read(thread_ptr);
    assert!(!State::get(env).ns_threads.contains_key(&pthread));
    State::get(env).ns_threads.insert(pthread, this);

    env.framework_state.foundation.ns_thread.is_multi_threaded = true;
}

- (())main {
    let &NSThreadHostObject {
        target,
        selector,
        object,
        tolerate_type_mismatch,
        ..
    } = env.objc.borrow(this);
    if tolerate_type_mismatch {
        let _: () = msg_send_no_type_checking(env, (target, selector.unwrap(), object));
    } else {
        let _: () = msg_send(env, (target, selector.unwrap(), object));
    }
}

- (())cancel {
    env.objc.borrow_mut::<NSThreadHostObject>(this).cancelled = true;
    log_dbg!("[(NSThread*){:?} cancel]", this);
}

// =========================================================================
// MARK: - State queries
// =========================================================================

- (bool)isMainThread {
    env.objc.borrow::<NSThreadHostObject>(this).is_main_thread
}

- (bool)isExecuting {
    env.objc.borrow::<NSThreadHostObject>(this).executing
}

- (bool)isFinished {
    env.objc.borrow::<NSThreadHostObject>(this).finished
}

- (bool)isCancelled {
    env.objc.borrow::<NSThreadHostObject>(this).cancelled
}

// =========================================================================
// MARK: - Name
// =========================================================================

- (id)name { // NSString*
    env.objc.borrow::<NSThreadHostObject>(this).name
}

- (())setName:(id)name { // NSString*
    let old = env.objc.borrow::<NSThreadHostObject>(this).name;
    release(env, old);
    retain(env, name);
    env.objc.borrow_mut::<NSThreadHostObject>(this).name = name;
}

// =========================================================================
// MARK: - Stack size
// =========================================================================

- (NSUInteger)stackSize {
    env.objc.borrow::<NSThreadHostObject>(this).stack_size
}

- (())setStackSize:(NSUInteger)size {
    env.objc.borrow_mut::<NSThreadHostObject>(this).stack_size = size;
}

// =========================================================================
// MARK: - Priority
// =========================================================================

- (f64)threadPriority {
    env.objc.borrow::<NSThreadHostObject>(this).thread_priority
}

- (bool)setThreadPriority:(f64)priority {
    env.objc.borrow_mut::<NSThreadHostObject>(this).thread_priority = priority.clamp(0.0, 1.0);
    true
}

// =========================================================================
// MARK: - Quality of service
// =========================================================================

- (i32)qualityOfService {
    env.objc.borrow::<NSThreadHostObject>(this).quality_of_service
}

- (())setQualityOfService:(i32)qos {
    env.objc.borrow_mut::<NSThreadHostObject>(this).quality_of_service = qos;
}

// =========================================================================
// MARK: - Thread dictionary
// =========================================================================

- (id)threadDictionary {
    let thread_dictionary = env.objc.borrow::<NSThreadHostObject>(this).thread_dictionary;
    if thread_dictionary == nil {
        let dict = msg_class![env; NSMutableDictionary new];
        env.objc.borrow_mut::<NSThreadHostObject>(this).thread_dictionary = dict;
        dict
    } else {
        thread_dictionary
    }
}

// =========================================================================
// MARK: - Description
// =========================================================================

- (id)description {
    let (name, is_main, executing, finished, cancelled) = {
        let host = env.objc.borrow::<NSThreadHostObject>(this);
        (host.name, host.is_main_thread, host.executing, host.finished, host.cancelled)
    };
    let name_str = if name != nil {
        ns_string::to_rust_string(env, name).into_owned()
    } else {
        "(null)".to_string()
    };
    let s = format!(
        "<NSThread: {:?}>{{name = {}, main = {}, executing = {}, finished = {}, cancelled = {}}}",
        this, name_str, is_main, executing, finished, cancelled
    );
    let cstr = env.mem.alloc_and_write_cstr(s.as_bytes());
    msg_class![env; NSString stringWithUTF8String:cstr]
}

@end

};

type NSThreadRef = CFTypeRef;

pub fn _touchHLE_NSThreadInvocationHelper(env: &mut Environment, ns_thread_obj: NSThreadRef) {
    let class: Class = msg![env; ns_thread_obj class];
    let thread_class = env.objc.get_known_class("NSThread", &mut env.mem);
    if !env.objc.class_is_subclass_of(class, thread_class) {
        log!("Warning: _touchHLE_NSThreadInvocationHelper called with unexpected class, skipping");
        return;
    }

    env.objc
        .borrow_mut::<NSThreadHostObject>(ns_thread_obj)
        .executing = true;

    let _: () = msg![env; ns_thread_obj main];

    {
        let host = env.objc.borrow_mut::<NSThreadHostObject>(ns_thread_obj);
        host.finished = true;
        host.executing = false;
    }

    let &NSThreadHostObject {
        target,
        object,
        owned,
        ..
    } = env.objc.borrow(ns_thread_obj);
    release(env, object);
    release(env, target);

    let pthread = pthread_self(env);
    if State::get(env).ns_threads.remove(&pthread).is_none() {
        log!(
            "Warning: _touchHLE_NSThreadInvocationHelper: pthread {:?} \
             not found in ns_threads map",
            pthread
        );
    }

    if owned {
        release(env, ns_thread_obj);
    }
}

pub fn detach_new_thread_inner(
    env: &mut Environment,
    selector: SEL,
    target: id,
    object: id,
    tolerate_type_mismatch: bool,
) {
    let new: id = msg_class![env; NSThread alloc];
    let new: id = msg![env; new initWithTarget:target selector:selector object:object];
    {
        let host = env.objc.borrow_mut::<NSThreadHostObject>(new);
        host.owned = true;
        host.tolerate_type_mismatch = tolerate_type_mismatch;
    }
    env.framework_state.foundation.ns_thread.is_multi_threaded = true;
    let _: () = msg![env; new start];
}
