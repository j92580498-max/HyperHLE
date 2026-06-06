/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */

#![allow(dead_code)]
//! SCNetworkReachability

use crate::abi::GuestFunction;
use crate::dyld::{export_c_func, FunctionExports};
use crate::frameworks::core_foundation::cf_allocator::CFAllocatorRef;
use crate::frameworks::core_foundation::{CFRelease, CFRetain, CFTypeRef};
use crate::mem::{ConstPtr, MutPtr, MutVoidPtr};
use crate::objc::{objc_classes, ClassExports, HostObject};
use crate::Environment;

type SCNetworkReachabilityFlags = u32;
const kSCNetworkReachabilityFlagsTransientConnection: SCNetworkReachabilityFlags = 1 << 0;
const kSCNetworkReachabilityFlagsReachable: SCNetworkReachabilityFlags = 1 << 1;
const kSCNetworkReachabilityFlagsConnectionRequired: SCNetworkReachabilityFlags = 1 << 2;
const kSCNetworkReachabilityFlagsConnectionOnTraffic: SCNetworkReachabilityFlags = 1 << 3;
const kSCNetworkReachabilityFlagsInterventionRequired: SCNetworkReachabilityFlags = 1 << 4;
const kSCNetworkReachabilityFlagsConnectionOnDemand: SCNetworkReachabilityFlags = 1 << 5;
const kSCNetworkReachabilityFlagsIsLocalAddress: SCNetworkReachabilityFlags = 1 << 16;
const kSCNetworkReachabilityFlagsIsDirect: SCNetworkReachabilityFlags = 1 << 17;
const kSCNetworkReachabilityFlagsIsWWAN: SCNetworkReachabilityFlags = 1 << 18;

pub const CLASSES: ClassExports = objc_classes! {
    (env, this, _cmd);
    @implementation _touchHLE_SCNetworkReachability: NSObject
    - (())dealloc {
        env.objc.dealloc_object(this, &mut env.mem)
    }
    @end
};

#[derive(Default)]
struct SCNetworkReachabilityHostObject {
    name: Option<String>,
    callout: Option<GuestFunction>,
    context: MutVoidPtr,
}
impl HostObject for SCNetworkReachabilityHostObject {}

type SCNetworkReachabilityRef = CFTypeRef;

pub fn SCNetworkReachabilityRetain(
    env: &mut Environment,
    target: SCNetworkReachabilityRef,
) -> SCNetworkReachabilityRef {
    if !target.is_null() {
        CFRetain(env, target)
    } else {
        target
    }
}

pub fn SCNetworkReachabilityRelease(env: &mut Environment, target: SCNetworkReachabilityRef) {
    if !target.is_null() {
        CFRelease(env, target);
    }
}

fn SCNetworkReachabilityCreateWithName(
    env: &mut Environment,
    _allocator: CFAllocatorRef,
    name: ConstPtr<u8>,
) -> SCNetworkReachabilityRef {
    let name_str = env.mem.cstr_at_utf8(name).unwrap_or("").to_string();
    let isa = env
        .objc
        .get_known_class("_touchHLE_SCNetworkReachability", &mut env.mem);
    env.objc.alloc_object(
        isa,
        Box::new(SCNetworkReachabilityHostObject {
            name: Some(name_str),
            callout: None,
            context: MutVoidPtr::null(),
        }),
        &mut env.mem,
    )
}

fn SCNetworkReachabilityCreateWithAddress(
    env: &mut Environment,
    _allocator: CFAllocatorRef,
    _address: ConstPtr<u8>,
) -> SCNetworkReachabilityRef {
    let isa = env
        .objc
        .get_known_class("_touchHLE_SCNetworkReachability", &mut env.mem);
    env.objc.alloc_object(
        isa,
        Box::new(SCNetworkReachabilityHostObject {
            name: None,
            callout: None,
            context: MutVoidPtr::null(),
        }),
        &mut env.mem,
    )
}

fn SCNetworkReachabilityCreateWithAddressPair(
    env: &mut Environment,
    _allocator: CFAllocatorRef,
    _local: ConstPtr<u8>,
    _remote: ConstPtr<u8>,
) -> SCNetworkReachabilityRef {
    let isa = env
        .objc
        .get_known_class("_touchHLE_SCNetworkReachability", &mut env.mem);
    env.objc.alloc_object(
        isa,
        Box::new(SCNetworkReachabilityHostObject {
            name: None,
            callout: None,
            context: MutVoidPtr::null(),
        }),
        &mut env.mem,
    )
}

fn SCNetworkReachabilityGetFlags(
    env: &mut Environment,
    _target: SCNetworkReachabilityRef,
    flags: MutPtr<SCNetworkReachabilityFlags>,
) -> bool {
    // Принудительно говорим игре, что сеть доступна (Reachable)
    env.mem.write(flags, kSCNetworkReachabilityFlagsReachable);
    true
}

fn SCNetworkReachabilitySetCallback(
    env: &mut Environment,
    target: SCNetworkReachabilityRef,
    callout: GuestFunction,
    context: MutVoidPtr,
) -> bool {
    let host = env
        .objc
        .borrow_mut::<SCNetworkReachabilityHostObject>(target);
    host.callout = Some(callout);
    host.context = context;
    false
}

fn SCNetworkReachabilityScheduleWithRunLoop(
    _env: &mut Environment,
    _target: SCNetworkReachabilityRef,
    _run_loop: CFTypeRef,
    _run_loop_mode: CFTypeRef,
) -> bool {
    false
}
fn SCNetworkReachabilityUnscheduleFromRunLoop(
    _env: &mut Environment,
    _target: SCNetworkReachabilityRef,
    _run_loop: CFTypeRef,
    _run_loop_mode: CFTypeRef,
) -> bool {
    false
}
fn SCNetworkReachabilitySetDispatchQueue(
    _env: &mut Environment,
    _target: SCNetworkReachabilityRef,
    _queue: MutVoidPtr,
) -> bool {
    false
}

pub const FUNCTIONS: FunctionExports = &[
    export_c_func!(SCNetworkReachabilityRetain(_)),
    export_c_func!(SCNetworkReachabilityRelease(_)),
    export_c_func!(SCNetworkReachabilityCreateWithName(_, _)),
    export_c_func!(SCNetworkReachabilityCreateWithAddress(_, _)),
    export_c_func!(SCNetworkReachabilityCreateWithAddressPair(_, _, _)),
    export_c_func!(SCNetworkReachabilityGetFlags(_, _)),
    export_c_func!(SCNetworkReachabilitySetCallback(_, _, _)),
    export_c_func!(SCNetworkReachabilityScheduleWithRunLoop(_, _, _)),
    export_c_func!(SCNetworkReachabilityUnscheduleFromRunLoop(_, _, _)),
    export_c_func!(SCNetworkReachabilitySetDispatchQueue(_, _)),
];
