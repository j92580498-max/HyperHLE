/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `NSURLRequest and NSMutableURLRequest`.

use super::{ns_string, NSTimeInterval, NSUInteger};
use crate::frameworks::foundation::ns_string::to_rust_string;
use crate::objc::{
    autorelease, id, msg, msg_class, nil, objc_classes, release, retain, ClassExports, HostObject,
    NSZonePtr,
};

type NSURLRequestCachePolicy = NSUInteger;
const NSURLRequestUseProtocolCachePolicy: NSURLRequestCachePolicy = 0;
const NSURLRequestReloadIgnoringLocalCache: NSURLRequestCachePolicy = 1;
#[allow(dead_code)]
const NSURLRequestReloadIgnoringCacheData: NSURLRequestCachePolicy = 1; // alias
const NSURLRequestReturnCacheDataElseLoad: NSURLRequestCachePolicy = 2;
const NSURLRequestReturnCacheDataDontLoad: NSURLRequestCachePolicy = 3;
const NSURLRequestReloadRevalidatingCacheData: NSURLRequestCachePolicy = 4;

type NSURLRequestNetworkServiceType = NSUInteger;
const NSURLNetworkServiceTypeDefault: NSURLRequestNetworkServiceType = 0;
#[allow(dead_code)]
const NSURLNetworkServiceTypeVoIP: NSURLRequestNetworkServiceType = 1;
#[allow(dead_code)]
const NSURLNetworkServiceTypeVideo: NSURLRequestNetworkServiceType = 2;
#[allow(dead_code)]
const NSURLNetworkServiceTypeBackground: NSURLRequestNetworkServiceType = 3;
#[allow(dead_code)]
const NSURLNetworkServiceTypeVoice: NSURLRequestNetworkServiceType = 4;

#[derive(Default)]
struct NSURLRequestHostObject {
    /// `NSURL*`
    url: id,
    /// `NSURL*` — main document URL for cookie policy
    main_document_url: id,
    cache_policy: NSURLRequestCachePolicy,
    timeout_interval: NSTimeInterval,
    network_service_type: NSURLRequestNetworkServiceType,
    allows_cellular_access: bool,
    handles_cookies: bool,
    /// `NSString*`
    http_method: id,
    /// Whether http_method was set by the caller (true) or is the default
    /// static "GET" string (false). We must not release a static string.
    http_method_is_owned: bool,
    /// `NSData*`
    http_body: id,
    /// `NSInputStream*`
    http_body_stream: id,
    http_should_handle_cookies: bool,
    http_should_use_pipelining: bool,
    /// `NSMutableDictionary<NSString*, NSString*>*`
    http_header_fields: id,
}
impl HostObject for NSURLRequestHostObject {}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation NSURLRequest: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    // Start with an owned, empty mutable dictionary for headers.
    let http_header_fields: id = msg_class![env; NSMutableDictionary new];
    let host_object = Box::new(NSURLRequestHostObject {
        url: nil,
        main_document_url: nil,
        cache_policy: NSURLRequestUseProtocolCachePolicy,
        timeout_interval: 60.0,
        network_service_type: NSURLNetworkServiceTypeDefault,
        allows_cellular_access: true,
        handles_cookies: true,
        // Static string — must NOT be released in dealloc.
        http_method: ns_string::get_static_str(env, "GET"),
        http_method_is_owned: false,
        http_body: nil,
        http_body_stream: nil,
        http_should_handle_cookies: true,
        http_should_use_pipelining: false,
        http_header_fields,
    });
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

// MARK: - Constructors

+ (id)requestWithURL:(id)url {
    msg![env; this requestWithURL:url
                      cachePolicy:NSURLRequestUseProtocolCachePolicy
                  timeoutInterval:60.0]
}

+ (id)requestWithURL:(id)url
         cachePolicy:(NSURLRequestCachePolicy)cache_policy
     timeoutInterval:(NSTimeInterval)timeout_interval {
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithURL:url
                                cachePolicy:cache_policy
                            timeoutInterval:timeout_interval];
    autorelease(env, new)
}

- (id)initWithURL:(id)url {
    msg![env; this initWithURL:url
                   cachePolicy:NSURLRequestUseProtocolCachePolicy
               timeoutInterval:60.0]
}

- (id)initWithURL:(id)url
      cachePolicy:(NSURLRequestCachePolicy)cache_policy
  timeoutInterval:(NSTimeInterval)timeout_interval {
    if url == nil {
        release(env, this);
        return nil;
    }

    // NOTE: NSURLRequest is a pure value object (container for URL,
    // headers, cache policy, etc.) per Apple's documentation:
    // https://developer.apple.com/documentation/foundation/nsurlrequest
    // It must ALWAYS succeed regardless of network availability.
    // Network-level failures are reported later by NSURLConnection /
    // NSURLSession when the request is actually executed.

    let url_copy: id = msg![env; url copy];
    {
        let host = env.objc.borrow_mut::<NSURLRequestHostObject>(this);
        host.url              = url_copy;
        host.cache_policy     = cache_policy;
        host.timeout_interval = timeout_interval;
    }
    this
}

// MARK: - Dealloc

- (())dealloc {
    log_dbg!("[(NSURLRequest*){:?} dealloc]", this);
    let host = env.objc.borrow::<NSURLRequestHostObject>(this);
    let url               = host.url;
    let main_document_url = host.main_document_url;
    let http_body         = host.http_body;
    let http_body_stream  = host.http_body_stream;
    let http_header_fields = host.http_header_fields;
    // Only release http_method if we own it (i.e. it was set by the caller,
    // not the default static "GET" string from get_static_str).
    let http_method         = if host.http_method_is_owned { host.http_method } else { nil };
    release(env, url);
    release(env, main_document_url);
    release(env, http_method);
    release(env, http_body);
    release(env, http_body_stream);
    release(env, http_header_fields);
    env.objc.dealloc_object(this, &mut env.mem)
}

// MARK: - copy / mutableCopy
//
// Both variants do a FULL copy of all fields, including headers, so that
// the copy is fully independent of the original.

- (id)copy {
    let new: id = msg_class![env; NSURLRequest alloc];
    let host = env.objc.borrow::<NSURLRequestHostObject>(this);
    let url = host.url;
    let cp  = host.cache_policy;
    let ti  = host.timeout_interval;
    let nst = host.network_service_type;
    let aca = host.allows_cellular_access;
    let shc = host.http_should_handle_cookies;
    let sup = host.http_should_use_pipelining;
    let method      = host.http_method;
    let method_owned = host.http_method_is_owned;
    let body        = host.http_body;
    let body_stream = host.http_body_stream;
    let src_headers = host.http_header_fields;

    // init sets url/cache_policy/timeout_interval; patch the rest manually.
    let new: id = msg![env; new initWithURL:url cachePolicy:cp timeoutInterval:ti];
    if new == nil { return nil; }

    {
        let h = env.objc.borrow_mut::<NSURLRequestHostObject>(new);
        h.network_service_type     = nst;
        h.allows_cellular_access   = aca;
        h.http_should_handle_cookies = shc;
        h.http_should_use_pipelining = sup;
    }

    // Copy HTTP method only if it was an owned (caller-set) string.
    if method_owned && method != nil {
        let method_copy: id = msg![env; method copy];
        let h = env.objc.borrow_mut::<NSURLRequestHostObject>(new);
        h.http_method = method_copy;
        h.http_method_is_owned = true;
    }

    // Copy HTTP body.
    if body != nil {
        let body_copy: id = msg![env; body copy];
        env.objc.borrow_mut::<NSURLRequestHostObject>(new).http_body = body_copy;
    }

    // Retain body stream (streams aren't generally copyable).
    if body_stream != nil {
        retain(env, body_stream);
        env.objc.borrow_mut::<NSURLRequestHostObject>(new).http_body_stream = body_stream;
    }

    // Deep-copy the header dictionary so the copy is independent.
    if src_headers != nil {
        let old_headers = env.objc.borrow::<NSURLRequestHostObject>(new).http_header_fields;
        release(env, old_headers);
        let headers_copy: id = msg![env; src_headers mutableCopy];
        env.objc.borrow_mut::<NSURLRequestHostObject>(new).http_header_fields = headers_copy;
    }

    new
}

- (id)mutableCopy {
    let new: id = msg_class![env; NSMutableURLRequest alloc];
    let host = env.objc.borrow::<NSURLRequestHostObject>(this);
    let url = host.url;
    let cp  = host.cache_policy;
    let ti  = host.timeout_interval;
    let nst = host.network_service_type;
    let aca = host.allows_cellular_access;
    let shc = host.http_should_handle_cookies;
    let sup = host.http_should_use_pipelining;
    let method       = host.http_method;
    let method_owned = host.http_method_is_owned;
    let body         = host.http_body;
    let body_stream  = host.http_body_stream;
    let src_headers  = host.http_header_fields;

    let new: id = msg![env; new initWithURL:url cachePolicy:cp timeoutInterval:ti];
    if new == nil { return nil; }

    {
        let h = env.objc.borrow_mut::<NSURLRequestHostObject>(new);
        h.network_service_type       = nst;
        h.allows_cellular_access     = aca;
        h.http_should_handle_cookies = shc;
        h.http_should_use_pipelining = sup;
    }

    if method_owned && method != nil {
        let method_copy: id = msg![env; method copy];
        let h = env.objc.borrow_mut::<NSURLRequestHostObject>(new);
        h.http_method = method_copy;
        h.http_method_is_owned = true;
    }

    if body != nil {
        let body_copy: id = msg![env; body copy];
        env.objc.borrow_mut::<NSURLRequestHostObject>(new).http_body = body_copy;
    }

    if body_stream != nil {
        retain(env, body_stream);
        env.objc.borrow_mut::<NSURLRequestHostObject>(new).http_body_stream = body_stream;
    }

    if src_headers != nil {
        let old_headers = env.objc.borrow::<NSURLRequestHostObject>(new).http_header_fields;
        release(env, old_headers);
        let headers_copy: id = msg![env; src_headers mutableCopy];
        env.objc.borrow_mut::<NSURLRequestHostObject>(new).http_header_fields = headers_copy;
    }

    new
}

// MARK: - URL / policy accessors

- (id)URL {
    env.objc.borrow::<NSURLRequestHostObject>(this).url
}

- (id)mainDocumentURL {
    env.objc.borrow::<NSURLRequestHostObject>(this).main_document_url
}

- (NSURLRequestCachePolicy)cachePolicy {
    env.objc.borrow::<NSURLRequestHostObject>(this).cache_policy
}

- (NSTimeInterval)timeoutInterval {
    env.objc.borrow::<NSURLRequestHostObject>(this).timeout_interval
}

- (NSURLRequestNetworkServiceType)networkServiceType {
    env.objc.borrow::<NSURLRequestHostObject>(this).network_service_type
}

- (bool)allowsCellularAccess {
    env.objc.borrow::<NSURLRequestHostObject>(this).allows_cellular_access
}

- (bool)HTTPShouldHandleCookies {
    env.objc.borrow::<NSURLRequestHostObject>(this).http_should_handle_cookies
}

- (bool)HTTPShouldUsePipelining {
    env.objc.borrow::<NSURLRequestHostObject>(this).http_should_use_pipelining
}

// MARK: - HTTP accessors

- (id)HTTPMethod {
    env.objc.borrow::<NSURLRequestHostObject>(this).http_method
}

- (id)HTTPBody {
    env.objc.borrow::<NSURLRequestHostObject>(this).http_body
}

- (id)HTTPBodyStream {
    env.objc.borrow::<NSURLRequestHostObject>(this).http_body_stream
}

- (id)allHTTPHeaderFields {
    env.objc.borrow::<NSURLRequestHostObject>(this).http_header_fields
}

- (id)valueForHTTPHeaderField:(id)field { // NSString*
    let fields = env.objc.borrow::<NSURLRequestHostObject>(this).http_header_fields;
    msg![env; fields objectForKey:field]
}

// MARK: - Description

- (id)description {
    let url    = env.objc.borrow::<NSURLRequestHostObject>(this).url;
    let method = env.objc.borrow::<NSURLRequestHostObject>(this).http_method;
    let method_str = if method != nil {
        to_rust_string(env, method).into_owned()
    } else {
        "GET".to_string()
    };
    let url_desc: id = if url != nil { msg![env; url description] } else { nil };
    let url_str = if url_desc != nil {
        to_rust_string(env, url_desc).into_owned()
    } else {
        "<nil>".to_string()
    };
    let s = format!("<NSURLRequest {} {}>", method_str, url_str);
    let ns = crate::frameworks::foundation::ns_string::from_rust_string(env, s);
    autorelease(env, ns)
}

@end

// MARK: - NSMutableURLRequest

@implementation NSMutableURLRequest: NSURLRequest

// MARK: URL / policy setters

- (())setURL:(id)url {
    let old = env.objc.borrow::<NSURLRequestHostObject>(this).url;
    release(env, old);
    let url_copy: id = if url != nil { msg![env; url copy] } else { nil };
    env.objc.borrow_mut::<NSURLRequestHostObject>(this).url = url_copy;
}

- (())setMainDocumentURL:(id)url {
    let old = env.objc.borrow::<NSURLRequestHostObject>(this).main_document_url;
    release(env, old);
    let url_copy: id = if url != nil { msg![env; url copy] } else { nil };
    env.objc.borrow_mut::<NSURLRequestHostObject>(this).main_document_url = url_copy;
}

- (())setCachePolicy:(NSURLRequestCachePolicy)policy {
    env.objc.borrow_mut::<NSURLRequestHostObject>(this).cache_policy = policy;
}

- (())setTimeoutInterval:(NSTimeInterval)interval {
    env.objc.borrow_mut::<NSURLRequestHostObject>(this).timeout_interval = interval;
}

- (())setNetworkServiceType:(NSURLRequestNetworkServiceType)service_type {
    env.objc.borrow_mut::<NSURLRequestHostObject>(this).network_service_type = service_type;
}

- (())setAllowsCellularAccess:(bool)allows {
    env.objc.borrow_mut::<NSURLRequestHostObject>(this).allows_cellular_access = allows;
}

- (())setHTTPShouldHandleCookies:(bool)should {
    env.objc.borrow_mut::<NSURLRequestHostObject>(this).http_should_handle_cookies = should;
}

- (())setHTTPShouldUsePipelining:(bool)should {
    env.objc.borrow_mut::<NSURLRequestHostObject>(this).http_should_use_pipelining = should;
}

// MARK: HTTP setters

- (())setHTTPMethod:(id)http_method { // NSString*
    if http_method == nil { return; }
    let copy: id = msg![env; http_method copy];
    let host = env.objc.borrow_mut::<NSURLRequestHostObject>(this);
    // Only release the old value if we own it (not the default static string).
    let old_owned = host.http_method_is_owned;
    let old       = std::mem::replace(&mut host.http_method, copy);
    host.http_method_is_owned = true;
    if old_owned {
        release(env, old);
    }
}

- (())setHTTPBody:(id)http_body { // NSData*
    let copy: id = if http_body != nil { msg![env; http_body copy] } else { nil };
    let old = std::mem::replace(
        &mut env.objc.borrow_mut::<NSURLRequestHostObject>(this).http_body,
        copy,
    );
    release(env, old);
}

- (())setHTTPBodyStream:(id)stream { // NSInputStream*
    let old = env.objc.borrow::<NSURLRequestHostObject>(this).http_body_stream;
    release(env, old);
    if stream != nil { retain(env, stream); }
    env.objc.borrow_mut::<NSURLRequestHostObject>(this).http_body_stream = stream;
}

// MARK: Header fields

- (())setValue:(id)value forHTTPHeaderField:(id)field { // NSString*, NSString*
    if field == nil { return; }
    log_dbg!(
        "NSMutableURLRequest setValue:'{}' forHTTPHeaderField:'{}'",
        if value != nil { to_rust_string(env, value).into_owned() } else { "<nil>".into() },
        to_rust_string(env, field)
    );
    let fields = env.objc.borrow::<NSURLRequestHostObject>(this).http_header_fields;
    if value != nil {
        () = msg![env; fields setObject:value forKey:field];
    } else {
        // setValue:nil means remove the field (per Apple docs).
        () = msg![env; fields removeObjectForKey:field];
    }
}

- (())addValue:(id)value forHTTPHeaderField:(id)field { // NSString*, NSString*
    if field == nil || value == nil { return; }
    log_dbg!(
        "NSMutableURLRequest addValue:'{}' forHTTPHeaderField:'{}'",
        to_rust_string(env, value),
        to_rust_string(env, field)
    );
    let fields = env.objc.borrow::<NSURLRequestHostObject>(this).http_header_fields;
    // If a value already exists, append with comma per HTTP spec.
    let existing: id = msg![env; fields objectForKey:field];
    if existing != nil {
        let existing_str = to_rust_string(env, existing).into_owned();
        let new_str      = to_rust_string(env, value).into_owned();
        let combined = crate::frameworks::foundation::ns_string::from_rust_string(
            env,
            format!("{}, {}", existing_str, new_str),
        );
        // setObject:forKey: retains the value, so we autorelease our local ref.
        autorelease(env, combined);
        () = msg![env; fields setObject:combined forKey:field];
    } else {
        () = msg![env; fields setObject:value forKey:field];
    }
}

- (())setAllHTTPHeaderFields:(id)header_fields { // NSDictionary*
    let old = env.objc.borrow::<NSURLRequestHostObject>(this).http_header_fields;
    release(env, old);
    let copy: id = if header_fields != nil {
        msg![env; header_fields mutableCopy]
    } else {
        msg_class![env; NSMutableDictionary new]
    };
    env.objc.borrow_mut::<NSURLRequestHostObject>(this).http_header_fields = copy;
}

@end

};
