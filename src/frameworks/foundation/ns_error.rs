/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `NSError`.

use crate::dyld::{ConstantExports, HostConstant};
use crate::frameworks::foundation::{ns_string, NSInteger};
use crate::objc::{
    id, msg, msg_class, nil, objc_classes, release, retain, ClassExports, HostObject, NSZonePtr,
};

/// `NSString*`
pub type NSErrorDomain = id;

pub const NSCocoaErrorDomain: &str = "NSCocoaErrorDomain";
pub const NSOSStatusErrorDomain: &str = "NSOSStatusErrorDomain";
pub const NSURLErrorDomain: &str = "NSURLErrorDomain";
pub const NSPOSIXErrorDomain: &str = "NSPOSIXErrorDomain";
pub const NSMachErrorDomain: &str = "NSMachErrorDomain";

// Common Cocoa error codes
pub const NSFileReadNoSuchFileError: NSInteger = 260;
pub const NSFileReadUnknownError: NSInteger = 256;
pub const NSFileWriteUnknownError: NSInteger = 512;
pub const NSFileWriteNoPermissionError: NSInteger = 513;
pub const NSFileWriteOutOfSpaceError: NSInteger = 640;
pub const NSKeyValueValidationError: NSInteger = 1024;
pub const NSFormattingError: NSInteger = 2048;
pub const NSUserCancelledError: NSInteger = 3072;
pub const NSFeatureUnsupportedError: NSInteger = 3328;
pub const NSExecutableNotLoadableError: NSInteger = 3584;
pub const NSPropertyListReadCorruptError: NSInteger = 3840;

// Common URL error codes
pub const NSURLErrorUnknown: NSInteger = -1;
pub const NSURLErrorCancelled: NSInteger = -999;
pub const NSURLErrorBadURL: NSInteger = -1000;
pub const NSURLErrorTimedOut: NSInteger = -1001;
pub const NSURLErrorNotConnectedToInternet: NSInteger = -1009;

// UserInfo dictionary keys (as Rust string literals — exported as constants
// below)
const KEY_LOCALIZED_DESCRIPTION: &str = "NSLocalizedDescription";
const KEY_LOCALIZED_FAILURE_REASON: &str = "NSLocalizedFailureReason";
const KEY_LOCALIZED_RECOVERY_SUGGESTION: &str = "NSLocalizedRecoverySuggestion";
const KEY_LOCALIZED_RECOVERY_OPTIONS: &str = "NSLocalizedRecoveryOptions";
const KEY_UNDERLYING_ERROR: &str = "NSUnderlyingError";
const KEY_URL: &str = "NSURL";
const KEY_FILE_PATH: &str = "NSFilePath";
const KEY_STRING_ENCODING: &str = "NSStringEncoding";
const KEY_DEBUG_DESCRIPTION: &str = "NSDebugDescription";
const KEY_FAILING_URL_STRING: &str = "NSErrorFailingURLStringKey";
const KEY_FAILING_URL: &str = "NSErrorFailingURLKey";

#[derive(Default)]
struct ErrorHostObject {
    domain: NSErrorDomain,
    code: NSInteger,
    user_info: id, // NSDictionary*
}
impl HostObject for ErrorHostObject {}

// =========================================================================
// MARK: - Helper — read NSString* from userInfo for a given key
// =========================================================================

fn user_info_string(env: &mut crate::Environment, this: id, key: &'static str) -> id {
    let user_info = env.objc.borrow::<ErrorHostObject>(this).user_info;
    if user_info == nil {
        return nil;
    }
    let key_ns = ns_string::get_static_str(env, key);
    msg![env; user_info objectForKey:key_ns]
}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation NSError: NSObject

// =========================================================================
// MARK: - Allocation / initialization
// =========================================================================

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(ErrorHostObject {
        domain: nil,
        code: 0,
        user_info: nil,
    });
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

+ (id)errorWithDomain:(NSErrorDomain)domain
                 code:(NSInteger)code
             userInfo:(id)user_info { // NSDictionary*
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithDomain:domain code:code userInfo:user_info];
    crate::objc::autorelease(env, new)
}

- (id)initWithDomain:(NSErrorDomain)domain
                code:(NSInteger)code
            userInfo:(id)user_info { // NSDictionary*
    retain(env, domain);
    retain(env, user_info);
    let host_obj = env.objc.borrow_mut::<ErrorHostObject>(this);
    host_obj.domain = domain;
    host_obj.code = code;
    host_obj.user_info = user_info;
    this
}

// =========================================================================
// MARK: - Dealloc
// =========================================================================

- (())dealloc {
    let &ErrorHostObject { domain, user_info, .. } = env.objc.borrow(this);
    release(env, domain);
    release(env, user_info);
    env.objc.dealloc_object(this, &mut env.mem)
}

// =========================================================================
// MARK: - Core accessors
// =========================================================================

- (NSErrorDomain)domain {
    env.objc.borrow::<ErrorHostObject>(this).domain
}

- (NSInteger)code {
    env.objc.borrow::<ErrorHostObject>(this).code
}

- (id)userInfo { // NSDictionary*
    env.objc.borrow::<ErrorHostObject>(this).user_info
}

// =========================================================================
// MARK: - Localized strings (read from userInfo, fall back to synthesised)
// =========================================================================

- (id)localizedDescription { // NSString*
    let from_dict = user_info_string(env, this, KEY_LOCALIZED_DESCRIPTION);
    if from_dict != nil {
        return from_dict;
    }
    // Synthesise: "The operation couldn't be completed. (domain error code.)"
    let (domain, code) = {
        let h = env.objc.borrow::<ErrorHostObject>(this);
        (h.domain, h.code)
    };
    let domain_str = if domain != nil {
        ns_string::to_rust_string(env, domain).into_owned()
    } else {
        "unknown domain".to_string()
    };
    let s = format!(
        "The operation couldn\u{2019}t be completed. ({} error {}.)",
        domain_str, code
    );
    let cstr = env.mem.alloc_and_write_cstr(s.as_bytes());
    msg_class![env; NSString stringWithUTF8String:cstr]
}

- (id)localizedFailureReason { // NSString*
    user_info_string(env, this, KEY_LOCALIZED_FAILURE_REASON)
}

- (id)localizedRecoverySuggestion { // NSString*
    user_info_string(env, this, KEY_LOCALIZED_RECOVERY_SUGGESTION)
}

- (id)localizedRecoveryOptions { // NSArray<NSString*>*
    user_info_string(env, this, KEY_LOCALIZED_RECOVERY_OPTIONS)
}

- (id)helpAnchor { // NSString*
    nil
}

// =========================================================================
// MARK: - Underlying error / URL / file path
// =========================================================================

- (id)underlyingError { // NSError*
    let user_info = env.objc.borrow::<ErrorHostObject>(this).user_info;
    if user_info == nil { return nil; }
    let key = ns_string::get_static_str(env, KEY_UNDERLYING_ERROR);
    msg![env; user_info objectForKey:key]
}

- (id)url { // NSURL*
    user_info_string(env, this, KEY_URL)
}

- (id)filePath { // NSString*  (NSFilePath from userInfo)
    user_info_string(env, this, KEY_FILE_PATH)
}

// =========================================================================
// MARK: - Debug description
// =========================================================================

- (id)debugDescription { // NSString*
    let from_dict = user_info_string(env, this, KEY_DEBUG_DESCRIPTION);
    if from_dict != nil {
        return from_dict;
    }
    msg![env; this description]
}

- (id)description { // NSString*
    let (domain, code) = {
        let h = env.objc.borrow::<ErrorHostObject>(this);
        (h.domain, h.code)
    };
    let domain_str = if domain != nil {
        ns_string::to_rust_string(env, domain).into_owned()
    } else {
        "(null)".to_string()
    };
    let s = format!("Error Domain={} Code={}", domain_str, code);
    let cstr = env.mem.alloc_and_write_cstr(s.as_bytes());
    msg_class![env; NSString stringWithUTF8String:cstr]
}

// =========================================================================
// MARK: - NSCopying
// =========================================================================

- (id)copyWithZone:(NSZonePtr)_zone {
    retain(env, this)
}

@end

};

pub const CONSTANTS: ConstantExports = &[
    // UserInfo keys
    (
        "_NSLocalizedDescriptionKey",
        HostConstant::NSString(KEY_LOCALIZED_DESCRIPTION),
    ),
    (
        "_NSLocalizedFailureReasonErrorKey",
        HostConstant::NSString(KEY_LOCALIZED_FAILURE_REASON),
    ),
    (
        "_NSLocalizedRecoverySuggestionErrorKey",
        HostConstant::NSString(KEY_LOCALIZED_RECOVERY_SUGGESTION),
    ),
    (
        "_NSLocalizedRecoveryOptionsErrorKey",
        HostConstant::NSString(KEY_LOCALIZED_RECOVERY_OPTIONS),
    ),
    (
        "_NSUnderlyingErrorKey",
        HostConstant::NSString(KEY_UNDERLYING_ERROR),
    ),
    ("_NSURLErrorKey", HostConstant::NSString(KEY_URL)),
    ("_NSFilePathErrorKey", HostConstant::NSString(KEY_FILE_PATH)),
    (
        "_NSDebugDescriptionErrorKey",
        HostConstant::NSString(KEY_DEBUG_DESCRIPTION),
    ),
    (
        "_NSStringEncodingErrorKey",
        HostConstant::NSString(KEY_STRING_ENCODING),
    ),
    (
        "_NSErrorFailingURLStringKey",
        HostConstant::NSString(KEY_FAILING_URL_STRING),
    ),
    (
        "_NSErrorFailingURLKey",
        HostConstant::NSString(KEY_FAILING_URL),
    ),
    // Error domains
    (
        "_NSCocoaErrorDomain",
        HostConstant::NSString(NSCocoaErrorDomain),
    ),
    (
        "_NSOSStatusErrorDomain",
        HostConstant::NSString(NSOSStatusErrorDomain),
    ),
    (
        "_NSURLErrorDomain",
        HostConstant::NSString(NSURLErrorDomain),
    ),
    (
        "_NSPOSIXErrorDomain",
        HostConstant::NSString(NSPOSIXErrorDomain),
    ),
    (
        "_NSMachErrorDomain",
        HostConstant::NSString(NSMachErrorDomain),
    ),
];
