/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */

//! `NSURLConnection`.
//!
//! This is a stub implementation that does not perform real networking.
//!
//! Synchronous requests return empty NSData with a descriptive NSError.
//! Asynchronous connections immediately call `connection:didFailWithError:`
//! on the delegate (if it implements that method) so the app can handle
//! the failure gracefully instead of hanging or crashing.
//!
//! For block-based API (`sendAsynchronousRequest:queue:completionHandler:`),
//! we deliver an NSError to the completion handler so the app can handle
//! the offline state gracefully (e.g. Sonic Runners shows "Error" and retries).
//!
//! NOTE: Returning fake 200 OK with empty JSON `{}` causes crashes in games
//! like Sonic Runners which try to parse specific server-protocol fields from
//! the response body. Returning an error is always safe — all tested games
//! handle NSURLErrorNotConnectedToInternet gracefully.

use crate::mem::{MutPtr, MutVoidPtr};
use crate::objc::{
    autorelease, id, msg, msg_class, nil, objc_classes, release, retain, ClassExports, HostObject,
    NSZonePtr,
};

// NSError domain / code used when reporting "no network in emulator".
const NS_URL_ERROR_DOMAIN: &str = "NSURLErrorDomain";
const NS_URL_ERROR_NOT_CONNECTED_TO_INTERNET: i32 = -1009;

// ---------------------------------------------------------------------------
// Host object — stores the delegate so we can call it back.
// ---------------------------------------------------------------------------

#[derive(Default)]
struct NSURLConnectionHostObject {
    /// `id<NSURLConnectionDelegate>` — retained while the connection is
    /// alive, released on dealloc / cancel.
    delegate: id,
    /// Whether the connection has already been cancelled / finished.
    cancelled: bool,
}
impl HostObject for NSURLConnectionHostObject {}

// ---------------------------------------------------------------------------
// Helper — build an NSError for "not connected to internet".
// ---------------------------------------------------------------------------
fn make_network_error(env: &mut crate::Environment) -> id {
    use crate::frameworks::foundation::ns_string::{from_rust_string, get_static_str};

    let domain = from_rust_string(env, NS_URL_ERROR_DOMAIN.to_string());
    autorelease(env, domain);

    let desc_key = get_static_str(env, "NSLocalizedDescription");
    let desc_val = from_rust_string(
        env,
        "The network connection was lost. \
         (touchHLE: networking not supported)"
            .to_string(),
    );
    autorelease(env, desc_val);

    let user_info: id = msg_class![env; NSMutableDictionary new];
    autorelease(env, user_info);
    () = msg![env; user_info setObject:desc_val forKey:desc_key];

    let error: id = msg_class![env; NSError alloc];
    let error: id = msg![env;
        error initWithDomain:domain
                        code:NS_URL_ERROR_NOT_CONNECTED_TO_INTERNET
                    userInfo:user_info];
    autorelease(env, error);
    error
}

// ---------------------------------------------------------------------------
// Helper — call `connection:didFailWithError:` on the delegate.
// Uses msg! which already handles unimplemented selectors gracefully.
// ---------------------------------------------------------------------------
fn notify_delegate_failure(env: &mut crate::Environment, connection: id, delegate: id) {
    if delegate == nil {
        return;
    }
    log_dbg!("NSURLConnection: notifying delegate of failure");
    let error = make_network_error(env);
    () = msg![env; delegate connection:connection didFailWithError:error];
}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation NSURLConnection: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host = Box::new(NSURLConnectionHostObject {
        delegate: nil,
        cancelled: false,
    });
    env.objc.alloc_object(this, host, &mut env.mem)
}

// MARK: - canHandleRequest: (class method)

+ (bool)canHandleRequest:(id)_request {
    // Advertise support so the app doesn't take a different code path;
    // failure is reported via the delegate / error out-param instead.
    true
}

// MARK: - Synchronous API

+ (id)sendSynchronousRequest:(id)request
           returningResponse:(MutPtr<id>)response_ptr
                       error:(MutPtr<id>)error_ptr {

    log!("NSURLConnection sendSynchronousRequest: stub called (returning empty data + error)");

    // Even when request is nil we return non-nil NSData, because many
    // callers do not nil-check the return value and crash otherwise.
    if request == nil {
        log!(
            "NSURLConnection sendSynchronousRequest: nil request — \
             returning empty NSData to prevent caller crash"
        );
    }

    // Write nil into *response (no HTTP response to report).
    if !response_ptr.is_null() {
        env.mem.write(response_ptr, nil);
    }

    // Build and write an NSError so the caller knows why data is empty.
    if !error_ptr.is_null() {
        let error = make_network_error(env);
        // make_network_error already autoreleased; retain once more so the
        // caller owns a +1 ref through the out-pointer.
        retain(env, error);
        env.mem.write(error_ptr, error);
    }

    // Always return empty NSData (never nil) to avoid null-deref crashes
    // in callers that do not check the error out-pointer.
    let empty_data: id = msg_class![env; NSData data];
    empty_data
}

// MARK: - Asynchronous block API
//
// `+[NSURLConnection sendAsynchronousRequest:queue:completionHandler:]`
// — iOS 5+ block-based convenience. touchHLE has no live network stack,
// so we synthesise the "not connected to internet" error. The handler is
// called with (nil, nil, error) as Apple documents for failure cases.
// Games like Sonic Runners handle this gracefully — they show an error
// dialog and allow the user to retry.

+ (())sendAsynchronousRequest:(id)request
                        queue:(id)queue
            completionHandler:(MutVoidPtr)handler {
    if handler.is_null() {
        return;
    }
    log!(
        "NSURLConnection sendAsynchronousRequest:queue:completionHandler: \
         delivering NSURLErrorNotConnectedToInternet (touchHLE has no network)"
    );

    // Build the failure NSError.
    let _ = request;
    let error = make_network_error(env);

    // The completion handler is a `void (^)(NSURLResponse *, NSData *,
    // NSError *)` block. ARM32 ABI: the block struct's third word
    // (index 3 == byte offset 12) is the invoke function pointer.
    let invoke_ptr = env.mem.read(handler.cast::<u32>() + 3u32);
    if invoke_ptr == 0 {
        return;
    }
    use crate::abi::CallFromHost;
    let invoke = crate::abi::GuestFunction::from_addr_with_thumb_bit(invoke_ptr);

    let _ = queue;
    // Call with (nil_response, nil_data, error) — failure.
    let _: () = invoke.call_from_host(env, (handler, nil, nil, error));
}

// MARK: - Asynchronous API

+ (id)connectionWithRequest:(id)request
                   delegate:(id)delegate {
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithRequest:request delegate:delegate];
    autorelease(env, new);
    new
}

- (id)initWithRequest:(id)request
             delegate:(id)delegate {
    msg![env;
        this initWithRequest:request
                    delegate:delegate
            startImmediately:true]
}

- (id)initWithRequest:(id)request
             delegate:(id)delegate
     startImmediately:(bool)start_immediately {

    if request == nil {
        log!("NSURLConnection initWithRequest: nil request — returning nil");
        release(env, this);
        return nil;
    }

    log_dbg!(
        "NSURLConnection initWithRequest:... delegate:... \
         startImmediately:{} (stub — failure via delegate)",
        start_immediately,
    );

    retain(env, delegate);
    {
        let host = env.objc.borrow_mut::<NSURLConnectionHostObject>(this);
        host.delegate  = delegate;
        host.cancelled = false;
    }

    if start_immediately {
        // Per Apple's documentation, when startImmediately is YES the
        // connection begins loading data immediately. Since touchHLE has
        // no network stack, we schedule the delegate failure callback via
        // performSelector:withObject:afterDelay: so that it fires on the
        // next run-loop iteration rather than synchronously during init.
        // This matches real iOS timing behavior — delegates are never
        // called during the initializer itself.
        log_dbg!(
            "NSURLConnection: scheduling deferred failure notification \
             (networking not supported in touchHLE)"
        );
        let sel = env.objc.register_host_selector("_touchHLE_deliverFailure".to_string(), &mut env.mem);
        () = msg![env; this performSelector:sel withObject:nil afterDelay:0.0_f64];
    }

    this
}

// Internal helper method: delivers the failure callback to the delegate.
- (())_touchHLE_deliverFailure {
    let host = env.objc.borrow::<NSURLConnectionHostObject>(this);
    if host.cancelled {
        return;
    }
    let delegate = host.delegate;
    if delegate == nil {
        return;
    }
    notify_delegate_failure(env, this, delegate);
}

// MARK: - Instance methods

- (())start {
    log_dbg!(
        "NSURLConnection start: scheduling deferred failure \
         (networking not supported in touchHLE)"
    );
    let sel = env.objc.register_host_selector("_touchHLE_deliverFailure".to_string(), &mut env.mem);
    () = msg![env; this performSelector:sel withObject:nil afterDelay:0.0_f64];
}

- (())cancel {
    log_dbg!("NSURLConnection cancel");
    env.objc
        .borrow_mut::<NSURLConnectionHostObject>(this)
        .cancelled = true;
}

// MARK: - Dealloc

- (())dealloc {
    log_dbg!("NSURLConnection dealloc");
    let delegate = env.objc
        .borrow::<NSURLConnectionHostObject>(this)
        .delegate;
    release(env, delegate);
    env.objc.dealloc_object(this, &mut env.mem);
}

@end

};
