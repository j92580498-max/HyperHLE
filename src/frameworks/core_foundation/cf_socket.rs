/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0.
 * If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
#![allow(dead_code)]
//! `CFSocket`

use super::cf_allocator::{kCFAllocatorDefault, CFAllocatorRef};
use super::{CFRelease, CFRetain, CFTypeRef};
use crate::dyld::{export_c_func, FunctionExports};
use crate::mem::{ConstVoidPtr, MutVoidPtr};
use crate::objc::{nil, objc_classes, ClassExports, HostObject};
use crate::Environment;

pub type CFSocketRef = CFTypeRef;
pub type CFSocketNativeHandle = i32;
pub type CFSocketCallBackType = u32;
pub type CFSocketError = i32;

// CFSocketCallBackType flags
const kCFSocketNoCallBack: CFSocketCallBackType = 0;
const kCFSocketReadCallBack: CFSocketCallBackType = 1;
const kCFSocketAcceptCallBack: CFSocketCallBackType = 2;
const kCFSocketDataCallBack: CFSocketCallBackType = 3;
const kCFSocketConnectCallBack: CFSocketCallBackType = 4;
const kCFSocketWriteCallBack: CFSocketCallBackType = 8;

// CFSocketError values
const kCFSocketSuccess: CFSocketError = 0;
const kCFSocketError: CFSocketError = -1;
const kCFSocketTimeout: CFSocketError = -2;

// CFRunLoopSource — opaque stub ref reused from CFTypeRef
pub type CFRunLoopSourceRef = CFTypeRef;

#[derive(Default)]
struct CFSocketHostObject {
    native_handle: CFSocketNativeHandle,
}
impl HostObject for CFSocketHostObject {}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation _touchHLE_CFSocket: NSObject
- (())dealloc {
    env.objc.dealloc_object(this, &mut env.mem)
}
@end

};

// MARK: - Helpers

fn alloc_socket(env: &mut Environment, fd: CFSocketNativeHandle) -> CFSocketRef {
    let class = env.objc.get_known_class("_touchHLE_CFSocket", &mut env.mem);
    env.objc.alloc_object(
        class,
        Box::new(CFSocketHostObject { native_handle: fd }),
        &mut env.mem,
    )
}

// MARK: - Lifecycle

pub fn CFSocketRetain(env: &mut Environment, s: CFSocketRef) -> CFSocketRef {
    if !s.is_null() {
        CFRetain(env, s)
    } else {
        s
    }
}

pub fn CFSocketRelease(env: &mut Environment, s: CFSocketRef) {
    if !s.is_null() {
        CFRelease(env, s);
    }
}

// MARK: - Constructors

fn CFSocketCreate(
    env: &mut Environment,
    allocator: CFAllocatorRef,
    protocol_family: i32,
    socket_type: i32,
    protocol: i32,
    callback_types: CFSocketCallBackType,
    _callout: MutVoidPtr,
    _context: MutVoidPtr,
) -> CFSocketRef {
    assert!(allocator == kCFAllocatorDefault || env.mem.read(allocator).is_system_default());
    log!(
        "CFSocketCreate: family={} type={} protocol={} callbacks={} — stubbed, returning null",
        protocol_family,
        socket_type,
        protocol,
        callback_types
    );
    nil
}

fn CFSocketCreateWithNative(
    env: &mut Environment,
    _allocator: CFAllocatorRef,
    sock: CFSocketNativeHandle,
    callback_types: CFSocketCallBackType,
    _callout: MutVoidPtr,
    _context: MutVoidPtr,
) -> CFSocketRef {
    log!(
        "CFSocketCreateWithNative: fd={} callbacks={} — stubbed",
        sock,
        callback_types
    );
    alloc_socket(env, sock)
}

fn CFSocketCreateWithSocketSignature(
    _env: &mut Environment,
    _allocator: CFAllocatorRef,
    _signature: ConstVoidPtr, // const CFSocketSignature*
    _callback_types: CFSocketCallBackType,
    _callout: MutVoidPtr,
    _context: MutVoidPtr,
) -> CFSocketRef {
    log!("CFSocketCreateWithSocketSignature: stubbed, returning null");
    nil
}

fn CFSocketCreateConnectedToSocketSignature(
    _env: &mut Environment,
    _allocator: CFAllocatorRef,
    _signature: ConstVoidPtr,
    _callback_types: CFSocketCallBackType,
    _callout: MutVoidPtr,
    _context: MutVoidPtr,
    _timeout: f64,
) -> CFSocketRef {
    log!("CFSocketCreateConnectedToSocketSignature: stubbed, returning null");
    nil
}

// MARK: - Native handle

fn CFSocketGetNative(env: &mut Environment, s: CFSocketRef) -> CFSocketNativeHandle {
    if s.is_null() {
        return -1;
    }
    env.objc.borrow::<CFSocketHostObject>(s).native_handle
}

// MARK: - Enable / disable callbacks

fn CFSocketEnableCallBacks(
    _env: &mut Environment,
    _s: CFSocketRef,
    _callback_types: CFSocketCallBackType,
) {
    log!("CFSocketEnableCallBacks: stubbed");
}

fn CFSocketDisableCallBacks(
    _env: &mut Environment,
    _s: CFSocketRef,
    _callback_types: CFSocketCallBackType,
) {
    log!("CFSocketDisableCallBacks: stubbed");
}

fn CFSocketSetSocketFlags(_env: &mut Environment, _s: CFSocketRef, _flags: u32) {
    log!("CFSocketSetSocketFlags: stubbed");
}

fn CFSocketGetSocketFlags(_env: &mut Environment, _s: CFSocketRef) -> u32 {
    0
}

// MARK: - Address / data

fn CFSocketCopyAddress(_env: &mut Environment, _s: CFSocketRef) -> CFTypeRef {
    // Returns CFDataRef — return null (no real socket).
    nil
}

fn CFSocketCopyPeerAddress(_env: &mut Environment, _s: CFSocketRef) -> CFTypeRef {
    nil
}

fn CFSocketSetAddress(
    _env: &mut Environment,
    _s: CFSocketRef,
    _address: CFTypeRef, // CFDataRef
) -> CFSocketError {
    log!("CFSocketSetAddress: stubbed, returning error");
    kCFSocketError
}

fn CFSocketConnectToAddress(
    _env: &mut Environment,
    _s: CFSocketRef,
    _address: CFTypeRef, // CFDataRef
    _timeout: f64,
) -> CFSocketError {
    log!("CFSocketConnectToAddress: stubbed, returning error");
    kCFSocketError
}

fn CFSocketSendData(
    _env: &mut Environment,
    _s: CFSocketRef,
    _address: CFTypeRef, // CFDataRef
    _data: CFTypeRef,    // CFDataRef
    _timeout: f64,
) -> CFSocketError {
    log!("CFSocketSendData: stubbed, returning error");
    kCFSocketError
}

// MARK: - Run loop source

fn CFSocketCreateRunLoopSource(
    _env: &mut Environment,
    _allocator: CFAllocatorRef,
    _s: CFSocketRef,
    _order: i32,
) -> CFRunLoopSourceRef {
    log!("CFSocketCreateRunLoopSource: stubbed, returning null");
    nil
}

// MARK: - Validity

fn CFSocketIsValid(_env: &mut Environment, s: CFSocketRef) -> bool {
    !s.is_null()
}

fn CFSocketInvalidate(_env: &mut Environment, _s: CFSocketRef) {
    log!("CFSocketInvalidate: stubbed");
}

pub const FUNCTIONS: FunctionExports = &[
    export_c_func!(CFSocketRetain(_)),
    export_c_func!(CFSocketRelease(_)),
    export_c_func!(CFSocketCreate(_, _, _, _, _, _, _)),
    export_c_func!(CFSocketCreateWithNative(_, _, _, _, _)),
    export_c_func!(CFSocketCreateWithSocketSignature(_, _, _, _, _)),
    export_c_func!(CFSocketCreateConnectedToSocketSignature(_, _, _, _, _, _)),
    export_c_func!(CFSocketGetNative(_)),
    export_c_func!(CFSocketEnableCallBacks(_, _)),
    export_c_func!(CFSocketDisableCallBacks(_, _)),
    export_c_func!(CFSocketSetSocketFlags(_, _)),
    export_c_func!(CFSocketGetSocketFlags(_)),
    export_c_func!(CFSocketCopyAddress(_)),
    export_c_func!(CFSocketCopyPeerAddress(_)),
    export_c_func!(CFSocketSetAddress(_, _)),
    export_c_func!(CFSocketConnectToAddress(_, _, _)),
    export_c_func!(CFSocketSendData(_, _, _, _)),
    export_c_func!(CFSocketCreateRunLoopSource(_, _, _)),
    export_c_func!(CFSocketIsValid(_)),
    export_c_func!(CFSocketInvalidate(_)),
];
