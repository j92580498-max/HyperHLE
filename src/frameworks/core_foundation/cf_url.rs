/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0.
 * If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//!
//! `CFURL`.
//!
//! This is toll-free bridged to `NSURL` in Apple's implementation. Here it is
//! the same type.
use super::cf_allocator::{kCFAllocatorDefault, CFAllocatorRef};
use super::{CFIndex, CFRelease, CFRetain};
use crate::dyld::{export_c_func, ConstantExports, FunctionExports, HostConstant};
use crate::frameworks::core_foundation::cf_string::{
    kCFStringEncodingUTF8, CFStringConvertEncodingToNSStringEncoding,
    CFStringEncoding, CFStringRef,
};
use crate::frameworks::foundation::ns_string::{
    from_rust_string, get_static_str, to_rust_string, NSUTF8StringEncoding,
};
use crate::frameworks::foundation::NSUInteger;
use crate::mem::{ConstPtr, MutPtr};
use crate::objc::{id, msg, msg_class, nil, release, retain};
use crate::Environment;

pub type CFURLRef = super::CFTypeRef;

// Path styles
type CFURLPathStyle = CFIndex;
pub const kCFURLPOSIXPathStyle: CFURLPathStyle = 0;
pub const kCFURLHFSPathStyle: CFURLPathStyle = 1;
pub const kCFURLWindowsPathStyle: CFURLPathStyle = 2;
// URL component indices
pub type CFURLComponentType = CFIndex;
pub const kCFURLComponentScheme: CFURLComponentType = 1;
pub const kCFURLComponentNetLocation: CFURLComponentType = 2;
pub const kCFURLComponentPath: CFURLComponentType = 3;
pub const kCFURLComponentResourceSpecifier: CFURLComponentType = 4;
pub const kCFURLComponentUser: CFURLComponentType = 5;
pub const kCFURLComponentPassword: CFURLComponentType = 6;
pub const kCFURLComponentUserInfo: CFURLComponentType = 7;
pub const kCFURLComponentHost: CFURLComponentType = 8;
pub const kCFURLComponentPort: CFURLComponentType = 9;
pub const kCFURLComponentParameterString: CFURLComponentType = 10;
pub const kCFURLComponentQuery: CFURLComponentType = 11;
pub const kCFURLComponentFragment: CFURLComponentType = 12;

// Helper function to validate allocator
fn validate_allocator(env: &mut Environment, allocator: CFAllocatorRef) -> bool {
    allocator == kCFAllocatorDefault
        || allocator.is_null()
        || env.mem.read(allocator).is_system_default()
}

// MARK: - Retain / Release

fn CFURLRetain(env: &mut Environment, url: CFURLRef) -> CFURLRef {
    if !url.is_null() {
        CFRetain(env, url)
    } else {
        url
    }
}

fn CFURLRelease(env: &mut Environment, url: CFURLRef) {
    if !url.is_null() {
        CFRelease(env, url);
    }
}

// MARK: - File System Representation

pub fn CFURLGetFileSystemRepresentation(
    env: &mut Environment,
    url: CFURLRef,
    resolve_against_base: bool,
    buffer: MutPtr<u8>,
    buffer_size: CFIndex,
) -> bool {
    if url.is_null() || buffer.is_null() || buffer_size <= 0 {
        return false;
    }

    let actual_url = if resolve_against_base {
        // Resolve against base URL if present
        let absolute_url: id = msg![env; url absoluteURL];
        if !absolute_url.is_null() {
            absolute_url
        } else {
            url
        }
    } else {
        url
    };

    let buffer_size: NSUInteger = buffer_size.try_into().unwrap_or(0);
    msg![env; actual_url getFileSystemRepresentation:buffer maxLength:buffer_size]
}

pub fn CFURLCreateFromFileSystemRepresentation(
    env: &mut Environment,
    allocator: CFAllocatorRef,
    buffer: ConstPtr<u8>,
    buffer_size: CFIndex,
    is_directory: bool,
) -> CFURLRef {
    if !validate_allocator(env, allocator) {
        return nil;
    }

    if buffer.is_null() || buffer_size < 0 {
        return nil;
    }

    let buffer_size: NSUInteger = buffer_size.try_into().unwrap_or(0);

    let string: id = msg_class![env; NSString alloc];
    let string: id = msg![env; string initWithBytes:buffer
                                             length:buffer_size
                                           encoding:NSUTF8StringEncoding];
    if string.is_null() {
        return nil;
    }

    let url: id = msg_class![env; NSURL alloc];
    let res = msg![env; url initFileURLWithPath:string isDirectory:is_directory];
    release(env, string);
    res
}

fn CFURLCreateFromFileSystemRepresentationRelativeToBase(
    env: &mut Environment,
    allocator: CFAllocatorRef,
    buffer: ConstPtr<u8>,
    buffer_size: CFIndex,
    is_directory: bool,
    base_url: CFURLRef,
) -> CFURLRef {
    if !validate_allocator(env, allocator) {
        return nil;
    }

    if buffer.is_null() || buffer_size < 0 {
        return nil;
    }

    let buffer_size: NSUInteger = buffer_size.try_into().unwrap_or(0);

    let string: id = msg_class![env; NSString alloc];
    let string: id = msg![env; string initWithBytes:buffer
                                             length:buffer_size
                                           encoding:NSUTF8StringEncoding];
    if string.is_null() {
        return nil;
    }

    let url: id = msg_class![env; NSURL alloc];
    let res = if base_url.is_null() {
        msg![env; url initFileURLWithPath:string isDirectory:is_directory]
    } else {
        let file_url: id = msg_class![env; NSURL fileURLWithPath:string isDirectory:is_directory];
        // Явное указание типа : id
        let absolute_string: id = msg![env; file_url absoluteString];
        msg![env; url initWithString:absolute_string relativeToURL:base_url]
    };

    release(env, string);
    res
}

// MARK: - Creation Functions

fn CFURLCreateWithBytes(
    env: &mut Environment,
    allocator: CFAllocatorRef,
    url_bytes: ConstPtr<u8>,
    length: CFIndex,
    encoding: CFStringEncoding,
    base_url: CFURLRef,
) -> CFURLRef {
    if !validate_allocator(env, allocator) {
        return nil;
    }

    if url_bytes.is_null() || length < 0 {
        return nil;
    }

    if length == 0 {
        return nil;
    }

    let encoding = CFStringConvertEncodingToNSStringEncoding(env, encoding);
    let length: NSUInteger = length.try_into().unwrap_or(0);

    let string: id = msg_class![env; NSString alloc];
    let string: id = msg![env; string initWithBytes:url_bytes
                                             length:length
                                           encoding:encoding];
    if string.is_null() {
        return nil;
    }

    let url: id = msg_class![env; NSURL alloc];
    let res = if base_url.is_null() {
        msg![env; url initWithString:string]
    } else {
        msg![env; url initWithString:string relativeToURL:base_url]
    };

    release(env, string);
    res
}

fn CFURLCreateWithString(
    env: &mut Environment,
    allocator: CFAllocatorRef,
    url_string: CFStringRef,
    base_url: CFURLRef,
) -> CFURLRef {
    if !validate_allocator(env, allocator) {
        return nil;
    }

    if url_string.is_null() {
        return nil;
    }

    let url: id = msg_class![env; NSURL alloc];
    if base_url.is_null() {
        msg![env; url initWithString:url_string]
    } else {
        msg![env; url initWithString:url_string relativeToURL:base_url]
    }
}

fn CFURLCreateAbsoluteURLWithBytes(
    env: &mut Environment,
    allocator: CFAllocatorRef,
    relative_url_bytes: ConstPtr<u8>,
    length: CFIndex,
    encoding: CFStringEncoding,
    base_url: CFURLRef,
    use_compatibility_mode: bool,
) -> CFURLRef {
    if !validate_allocator(env, allocator) {
        return nil;
    }

    // Create the relative URL first
    let relative_url = CFURLCreateWithBytes(
        env,
        allocator,
        relative_url_bytes,
        length,
        encoding,
        base_url,
    );
    if relative_url.is_null() {
        return nil;
    }

    // Get absolute URL
    let absolute_url: id = msg![env; relative_url absoluteURL];

    // Retain and release appropriately
    if !absolute_url.is_null() {
        retain(env, absolute_url);
    }
    release(env, relative_url);

    // Compatibility mode affects URL parsing - for now we ignore it
    let _ = use_compatibility_mode;
    absolute_url
}

fn CFURLCreateWithFileSystemPath(
    env: &mut Environment,
    allocator: CFAllocatorRef,
    file_path: CFStringRef,
    style: CFURLPathStyle,
    is_directory: bool,
) -> CFURLRef {
    if !validate_allocator(env, allocator) {
        return nil;
    }

    if file_path.is_null() {
        return nil;
    }

    // Handle different path styles
    let converted_path = match style {
        kCFURLPOSIXPathStyle => file_path,
        kCFURLHFSPathStyle => {
            // Convert HFS path to POSIX
            // HFS uses ":" as separator, starts with volume name
            log!("TODO: Full HFS path conversion");
            file_path
        }
        kCFURLWindowsPathStyle => {
            // Convert Windows path to POSIX
            log!("TODO: Full Windows path conversion");
            file_path
        }
        _ => {
            log!("Warning: Unknown path style {}", style);
            file_path
        }
    };

    let url: id = msg_class![env; NSURL alloc];
    msg![env; url initFileURLWithPath:converted_path isDirectory:is_directory]
}

fn CFURLCreateWithFileSystemPathRelativeToBase(
    env: &mut Environment,
    allocator: CFAllocatorRef,
    file_path: CFStringRef,
    style: CFURLPathStyle,
    is_directory: bool,
    base_url: CFURLRef,
) -> CFURLRef {
    if !validate_allocator(env, allocator) {
        return nil;
    }

    if file_path.is_null() {
        return nil;
    }

    // First create the file URL
    let file_url = CFURLCreateWithFileSystemPath(env, allocator, file_path, style, is_directory);
    if file_url.is_null() {
        return nil;
    }

    if base_url.is_null() {
        return file_url;
    }

    // Create relative to base
    let url_string: id = msg![env; file_url absoluteString];
    let url: id = msg_class![env; NSURL alloc];
    let result = msg![env; url initWithString:url_string relativeToURL:base_url];

    release(env, file_url);
    result
}

fn CFURLCreateCopyAppendingPathComponent(
    env: &mut Environment,
    allocator: CFAllocatorRef,
    url: CFURLRef,
    path_component: CFStringRef,
    is_directory: bool,
) -> CFURLRef {
    if !validate_allocator(env, allocator) {
        return nil;
    }

    if url.is_null() || path_component.is_null() {
        return nil;
    }

    // Явное указание типа : id
    let new_url: id =
        msg![env; url URLByAppendingPathComponent:path_component isDirectory:is_directory];
    if new_url.is_null() {
        return nil;
    }

    msg![env; new_url copy]
}

fn CFURLCreateCopyAppendingPathExtension(
    env: &mut Environment,
    allocator: CFAllocatorRef,
    url: CFURLRef,
    extension: CFStringRef,
) -> CFURLRef {
    if !validate_allocator(env, allocator) {
        return nil;
    }

    if url.is_null() || extension.is_null() {
        return nil;
    }

    // Явное указание типа : id
    let new_url: id = msg![env; url URLByAppendingPathExtension:extension];
    if new_url.is_null() {
        return nil;
    }

    msg![env; new_url copy]
}

fn CFURLCreateCopyDeletingLastPathComponent(
    env: &mut Environment,
    allocator: CFAllocatorRef,
    url: CFURLRef,
) -> CFURLRef {
    if !validate_allocator(env, allocator) {
        return nil;
    }

    if url.is_null() {
        return nil;
    }

    // Явное указание типа : id
    let new_url: id = msg![env; url URLByDeletingLastPathComponent];
    if new_url.is_null() {
        return nil;
    }

    msg![env; new_url copy]
}

fn CFURLCreateCopyDeletingPathExtension(
    env: &mut Environment,
    allocator: CFAllocatorRef,
    url: CFURLRef,
) -> CFURLRef {
    if !validate_allocator(env, allocator) {
        return nil;
    }

    if url.is_null() {
        return nil;
    }

    // Явное указание типа : id
    let new_url: id = msg![env; url URLByDeletingPathExtension];
    if new_url.is_null() {
        return nil;
    }

    msg![env; new_url copy]
}

// MARK: - Copying and Conversion

fn CFURLCopyAbsoluteURL(env: &mut Environment, url: CFURLRef) -> CFURLRef {
    if url.is_null() {
        return nil;
    }

    let absolute: id = msg![env; url absoluteURL];
    if absolute.is_null() {
        return nil;
    }

    msg![env; absolute copy]
}

pub fn CFURLCopyFileSystemPath(
    env: &mut Environment,
    url: CFURLRef,
    style: CFURLPathStyle,
) -> CFStringRef {
    if url.is_null() {
        return nil;
    }

    let path: CFStringRef = msg![env; url path];
    if path.is_null() {
        return nil;
    }

    // Handle different path styles
    match style {
        kCFURLPOSIXPathStyle => {
            msg![env; path copy]
        }
        kCFURLHFSPathStyle => {
            // Convert POSIX to HFS
            log!("TODO: Full POSIX to HFS path conversion");
            msg![env; path copy]
        }
        kCFURLWindowsPathStyle => {
            // Convert POSIX to Windows
            log!("TODO: Full POSIX to Windows path conversion");
            msg![env; path copy]
        }
        _ => {
            log!("Warning: Unknown path style {}", style);
            msg![env; path copy]
        }
    }
}

pub fn CFURLCopyPathExtension(env: &mut Environment, url: CFURLRef) -> CFStringRef {
    if url.is_null() {
        return nil;
    }

    // Явное указание типа : id
    let path: id = msg![env; url path];
    if path.is_null() {
        return nil;
    }

    // Явное указание типа : id
    let ext: id = msg![env; path pathExtension];
    if ext.is_null() {
        return nil;
    }

    msg![env; ext copy]
}

fn CFURLCopyLastPathComponent(env: &mut Environment, url: CFURLRef) -> CFStringRef {
    if url.is_null() {
        return nil;
    }

    let component: id = msg![env; url lastPathComponent];
    if component.is_null() {
        return nil;
    }

    msg![env; component copy]
}

fn CFURLCopyScheme(env: &mut Environment, url: CFURLRef) -> CFStringRef {
    if url.is_null() {
        return nil;
    }

    let scheme: id = msg![env; url scheme];
    if scheme.is_null() {
        return nil;
    }

    msg![env; scheme copy]
}

fn CFURLCopyNetLocation(env: &mut Environment, url: CFURLRef) -> CFStringRef {
    if url.is_null() {
        return nil;
    }

    // Net location is typically host:port
    let host: id = msg![env; url host];
    let port: id = msg![env; url port];

    if host.is_null() {
        return nil;
    }

    if port.is_null() {
        msg![env; host copy]
    } else {
        let port_str: id = msg![env; port stringValue];

        let colon = get_static_str(env, ":");
        let host_with_colon: id = msg![env; host stringByAppendingString:colon];
        let net_location: id = msg![env; host_with_colon stringByAppendingString:port_str];

        msg![env; net_location copy]
    }
}

fn CFURLCopyPath(env: &mut Environment, url: CFURLRef) -> CFStringRef {
    if url.is_null() {
        return nil;
    }

    let path: id = msg![env; url path];
    if path.is_null() {
        return nil;
    }

    msg![env; path copy]
}

fn CFURLCopyStrictPath(
    env: &mut Environment,
    url: CFURLRef,
    is_absolute: MutPtr<bool>,
) -> CFStringRef {
    if url.is_null() {
        return nil;
    }

    let path: id = msg![env; url path];
    if path.is_null() {
        return nil;
    }

    if !is_absolute.is_null() {
        // Check if it's an absolute path
        let path_str = to_rust_string(env, path);
        env.mem.write(is_absolute, path_str.starts_with('/'));
    }

    msg![env; path copy]
}

fn CFURLCopyResourceSpecifier(env: &mut Environment, url: CFURLRef) -> CFStringRef {
    if url.is_null() {
        return nil;
    }

    // Resource specifier is everything after the path (query + fragment)
    let resource_specifier: id = msg![env; url resourceSpecifier];

    if resource_specifier.is_null() {
        return nil;
    }

    msg![env; resource_specifier copy]
}

fn CFURLCopyHostName(env: &mut Environment, url: CFURLRef) -> CFStringRef {
    if url.is_null() {
        return nil;
    }

    let host: id = msg![env; url host];
    if host.is_null() {
        return nil;
    }

    msg![env; host copy]
}

fn CFURLCopyUserName(env: &mut Environment, url: CFURLRef) -> CFStringRef {
    if url.is_null() {
        return nil;
    }

    let user: id = msg![env; url user];
    if user.is_null() {
        return nil;
    }

    msg![env; user copy]
}

fn CFURLCopyPassword(env: &mut Environment, url: CFURLRef) -> CFStringRef {
    if url.is_null() {
        return nil;
    }

    let password: id = msg![env; url password];
    if password.is_null() {
        return nil;
    }

    msg![env; password copy]
}

fn CFURLCopyParameterString(
    env: &mut Environment,
    url: CFURLRef,
    _characters_to_leave_escaped: CFStringRef,
) -> CFStringRef {
    if url.is_null() {
        return nil;
    }

    let param_string: id = msg![env; url parameterString];
    if param_string.is_null() {
        return nil;
    }

    // TODO: Handle characters_to_leave_escaped
    msg![env; param_string copy]
}

fn CFURLCopyQueryString(
    env: &mut Environment,
    url: CFURLRef,
    _characters_to_leave_escaped: CFStringRef,
) -> CFStringRef {
    if url.is_null() {
        return nil;
    }

    let query: id = msg![env; url query];
    if query.is_null() {
        return nil;
    }

    // TODO: Handle characters_to_leave_escaped
    msg![env; query copy]
}

fn CFURLCopyFragment(
    env: &mut Environment,
    url: CFURLRef,
    _characters_to_leave_escaped: CFStringRef,
) -> CFStringRef {
    if url.is_null() {
        return nil;
    }

    let fragment: id = msg![env; url fragment];
    if fragment.is_null() {
        return nil;
    }

    // TODO: Handle characters_to_leave_escaped
    msg![env; fragment copy]
}

fn CFURLGetPortNumber(env: &mut Environment, url: CFURLRef) -> i32 {
    if url.is_null() {
        return -1;
    }

    let port: id = msg![env; url port];
    if port.is_null() {
        return -1;
    }

    msg![env; port intValue]
}

// MARK: - URL Properties

fn CFURLCanBeDecomposed(env: &mut Environment, url: CFURLRef) -> bool {
    if url.is_null() {
        return false;
    }

    // A URL can be decomposed if it has a scheme
    let scheme: id = msg![env; url scheme];
    !scheme.is_null()
}

fn CFURLHasDirectoryPath(env: &mut Environment, url: CFURLRef) -> bool {
    if url.is_null() {
        return false;
    }

    let path: id = msg![env; url path];
    if path.is_null() {
        return false;
    }

    // Check if path ends with "/"
    let path_str = to_rust_string(env, path);
    if path_str == "//" {
        // Special case
        return false;
    }

    // Note: cannot use `lastPathComponent` here!
    let components: id = msg![env; path pathComponents];
    let count: NSUInteger = msg![env; components count];

    if count == 0 {
        return false;
    }

    let last: id = msg![env; components objectAtIndex:(count - 1)];
    msg![env; last isEqual:(get_static_str(env, "/"))]
        || msg![env; last isEqual:(get_static_str(env, "."))]
        || msg![env; last isEqual:(get_static_str(env, ".."))]
}

fn CFURLGetBaseURL(env: &mut Environment, url: CFURLRef) -> CFURLRef {
    if url.is_null() {
        return nil;
    }

    msg![env; url baseURL]
}

fn CFURLGetString(env: &mut Environment, url: CFURLRef) -> CFStringRef {
    if url.is_null() {
        return nil;
    }

    // Return the relative string, not absolute
    msg![env; url relativeString]
}

fn CFURLGetBytes(
    env: &mut Environment,
    url: CFURLRef,
    buffer: MutPtr<u8>,
    buffer_length: CFIndex,
) -> CFIndex {
    if url.is_null() || buffer_length < 0 {
        return -1;
    }

    let url_string: id = msg![env; url absoluteString];
    if url_string.is_null() {
        return -1;
    }

    // Get UTF-8 bytes
    let c_string: ConstPtr<u8> = msg![env; url_string UTF8String];
    if c_string.is_null() {
        return -1;
    }

    // Calculate length
    let mut length: CFIndex = 0;
    loop {
        // Приведение типа к u32
        if env.mem.read(c_string + (length as u32)) == 0 {
            break;
        }
        length += 1;
    }

    if !buffer.is_null() && buffer_length > 0 {
        let copy_length = length.min(buffer_length - 1);
        for i in 0..copy_length {
            // Разделение read/write во избежание двойного заимствования
            let byte = env.mem.read(c_string + (i as u32));
            env.mem.write(buffer + (i as u32), byte);
        }
        // Приведение типа к u32
        env.mem.write(buffer + (copy_length as u32), 0);
        // Null terminate
    }

    length
}

fn CFURLGetByteRangeForComponent(
    env: &mut Environment,
    url: CFURLRef,
    component: CFURLComponentType,
    range_incl_separators: MutPtr<super::CFRange>,
) -> super::CFRange {
    if url.is_null() {
        return super::CFRange {
            location: super::kCFNotFound,
            length: 0,
        };
    }

    let url_string: id = msg![env; url absoluteString];
    if url_string.is_null() {
        return super::CFRange {
            location: super::kCFNotFound,
            length: 0,
        };
    }

    // Get the component string
    let component_str: id = match component {
        kCFURLComponentScheme => msg![env; url scheme],
        kCFURLComponentHost => msg![env; url host],
        kCFURLComponentPort => {
            let port: id = msg![env; url port];
            if port.is_null() {
                nil
            } else {
                msg![env; port stringValue]
            }
        }
        kCFURLComponentPath => msg![env; url path],
        kCFURLComponentQuery => msg![env; url query],
        kCFURLComponentFragment => msg![env; url fragment],
        kCFURLComponentUser => msg![env; url user],
        kCFURLComponentPassword => msg![env; url password],
        _ => {
            log!("TODO: Component type {} not fully supported", component);
            nil
        }
    };

    if component_str.is_null() {
        return super::CFRange {
            location: super::kCFNotFound,
            length: 0,
        };
    }

    // Find the component in the URL string
    use crate::frameworks::foundation::NSRange;
    let range: NSRange = msg![env; url_string rangeOfString:component_str];

    if range.location == crate::frameworks::foundation::NSNotFound as NSUInteger {
        return super::CFRange {
            location: super::kCFNotFound,
            length: 0,
        };
    }

    let cf_range = super::CFRange {
        location: range.location.try_into().unwrap_or(super::kCFNotFound),
        length: range.length.try_into().unwrap_or(0),
    };

    // TODO: Include separators if requested
    if !range_incl_separators.is_null() {
        env.mem.write(range_incl_separators, cf_range);
    }

    cf_range
}

// MARK: - Percent Escaping

fn CFURLCreateStringByReplacingPercentEscapes(
    env: &mut Environment,
    allocator: CFAllocatorRef,
    original_string: CFStringRef,
    _characters_to_leave_escaped: CFStringRef,
) -> CFStringRef {
    if !validate_allocator(env, allocator) {
        return nil;
    }

    if original_string.is_null() {
        return nil;
    }
    
    // Use NSString's built-in percent decoding
    let decoded: id = msg![env; original_string stringByRemovingPercentEncoding];
    if decoded.is_null() {
        msg![env; original_string copy]
    } else {
        msg![env; decoded copy]
    }
}

fn CFURLCreateStringByReplacingPercentEscapesUsingEncoding(
    env: &mut Environment,
    allocator: CFAllocatorRef,
    original_string: CFStringRef,
    characters_to_leave_escaped: CFStringRef,
    encoding: CFStringEncoding,
) -> CFStringRef {
    if !validate_allocator(env, allocator) {
        return nil;
    }

    if encoding == kCFStringEncodingUTF8 {
        return CFURLCreateStringByReplacingPercentEscapes(
            env,
            allocator,
            original_string,
            characters_to_leave_escaped,
        );
    }

    log!(
        "TODO: Percent escape replacement with encoding {:#x}",
        encoding
    );
    CFURLCreateStringByReplacingPercentEscapes(
        env,
        allocator,
        original_string,
        characters_to_leave_escaped,
    )
}

fn CFURLCreateStringByAddingPercentEscapes(
    env: &mut Environment,
    allocator: CFAllocatorRef,
    original_string: CFStringRef,
    characters_to_leave_unescaped: CFStringRef,
    legal_url_characters_to_be_escaped: CFStringRef,
    _encoding: CFStringEncoding,
) -> CFStringRef {
    if !validate_allocator(env, allocator) {
        return nil;
    }

    if original_string.is_null() {
        return nil;
    }

    let original = to_rust_string(env, original_string);

    let leave_unescaped: Option<String> = if characters_to_leave_unescaped.is_null() {
        None
    } else {
        Some(to_rust_string(env, characters_to_leave_unescaped).to_string())
    };

    let force_escaped: Option<String> = if legal_url_characters_to_be_escaped.is_null() {
        None
    } else {
        Some(to_rust_string(env, legal_url_characters_to_be_escaped).to_string())
    };

    // RFC 3986 unreserved characters that are never percent-encoded
    let is_unreserved = |c: char| -> bool {
        c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '~'
    };

    // RFC 3986 reserved characters that are normally kept as-is in URLs
    let is_reserved = |c: char| -> bool {
        matches!(
            c,
            ':' | '/'
                | '?'
                | '#'
                | '['
                | ']'
                | '@'
                | '!'
                | '$'
                | '&'
                | '\''
                | '('
                | ')'
                | '*'
                | '+'
                | ','
                | ';'
                | '='
        )
    };

    let mut result = String::with_capacity(original.len());
    for c in original.chars() {
        // Check if this character should be forced to escape
        let force_escape = force_escaped
            .as_ref()
            .is_some_and(|chars| chars.contains(c));

        // Check if this character should be left unescaped
        let leave_alone = leave_unescaped
            .as_ref()
            .is_some_and(|chars| chars.contains(c));

        if force_escape && !leave_alone {
            // Force-escape this character
            for byte in c.to_string().as_bytes() {
                result.push('%');
                result.push_str(&format!("{:02X}", byte));
            }
        } else if leave_alone || is_unreserved(c) || is_reserved(c) {
            // Keep as-is
            result.push(c);
        } else if c == '%' {
            // Keep existing percent escapes
            result.push(c);
        } else {
            // Percent-encode this character as UTF-8 bytes
            for byte in c.to_string().as_bytes() {
                result.push('%');
                result.push_str(&format!("{:02X}", byte));
            }
        }
    }

    
    from_rust_string(env, result)
}

// MARK: - Type Info

fn CFURLGetTypeID(_env: &mut Environment) -> u32 {
    // Return a fake CFTypeID for CFURL
    0x4346554C // 'CFUL' in hex
}

// MARK: - Exports

pub const FUNCTIONS: FunctionExports = &[
    // Retain/Release
    export_c_func!(CFURLRetain(_)),
    export_c_func!(CFURLRelease(_)),
    // File System Representation
    export_c_func!(CFURLGetFileSystemRepresentation(_, _, _, _)),
    export_c_func!(CFURLCreateFromFileSystemRepresentation(_, _, _, _)),
    export_c_func!(CFURLCreateFromFileSystemRepresentationRelativeToBase(
        _,
        _,
        _,
        _,
        _
    )),
    // Creation
    export_c_func!(CFURLCreateWithBytes(_, _, _, _, _)),
    export_c_func!(CFURLCreateWithString(_, _, _)),
    export_c_func!(CFURLCreateAbsoluteURLWithBytes(_, _, _, _, _, _)),
    export_c_func!(CFURLCreateWithFileSystemPath(_, _, _, _)),
    export_c_func!(CFURLCreateWithFileSystemPathRelativeToBase(_, _, _, _, _)),
    export_c_func!(CFURLCreateCopyAppendingPathComponent(_, _, _, _)),
    export_c_func!(CFURLCreateCopyAppendingPathExtension(_, _, _)),
    export_c_func!(CFURLCreateCopyDeletingLastPathComponent(_, _)),
    export_c_func!(CFURLCreateCopyDeletingPathExtension(_, _)),
    // Copying and Conversion
    export_c_func!(CFURLCopyAbsoluteURL(_)),
    export_c_func!(CFURLCopyFileSystemPath(_, _)),
    export_c_func!(CFURLCopyPathExtension(_)),
    export_c_func!(CFURLCopyLastPathComponent(_)),
    export_c_func!(CFURLCopyScheme(_)),
    export_c_func!(CFURLCopyNetLocation(_)),
    export_c_func!(CFURLCopyPath(_)),
    export_c_func!(CFURLCopyStrictPath(_, _)),
    export_c_func!(CFURLCopyResourceSpecifier(_)),
    export_c_func!(CFURLCopyHostName(_)),
    export_c_func!(CFURLCopyUserName(_)),
    export_c_func!(CFURLCopyPassword(_)),
    export_c_func!(CFURLCopyParameterString(_, _)),
    export_c_func!(CFURLCopyQueryString(_, _)),
    export_c_func!(CFURLCopyFragment(_, _)),
    export_c_func!(CFURLGetPortNumber(_)),
    // Properties
    export_c_func!(CFURLCanBeDecomposed(_)),
    export_c_func!(CFURLHasDirectoryPath(_)),
    export_c_func!(CFURLGetBaseURL(_)),
    export_c_func!(CFURLGetString(_)),
    export_c_func!(CFURLGetBytes(_, _, _)),
    export_c_func!(CFURLGetByteRangeForComponent(_, _, _)),
    // Percent Escaping (ИСПРАВЛЕНО КОЛИЧЕСТВО АРГУМЕНТОВ ЗДЕСЬ)
    export_c_func!(CFURLCreateStringByReplacingPercentEscapes(_, _, _)),
    export_c_func!(CFURLCreateStringByReplacingPercentEscapesUsingEncoding(
        _,
        _,
        _,
        _
    )),
    export_c_func!(CFURLCreateStringByAddingPercentEscapes(_, _, _, _, _)),
    // Type Info — CFURLGetTypeID is exported from cf_type; not duplicated.
];

// Resource-property keys exposed by `CFURLCopyResourcePropertyForKey`.
// We don't actually implement the property accessor APIs; exporting these
// CFStringRef constants merely silences the non-lazy-symbol warning so apps
// that link with `-framework CoreFoundation` and reference (but don't call)
// the keys can load.
pub const CONSTANTS: ConstantExports = &[
    ("_kCFURLFileLength", HostConstant::NSString("NSURLFileSize")),
    ("_kCFURLFileSize", HostConstant::NSString("NSURLFileSize")),
    (
        "_kCFURLFileSizeKey",
        HostConstant::NSString("NSURLFileSizeKey"),
    ),
    (
        "_kCFURLIsDirectoryKey",
        HostConstant::NSString("NSURLIsDirectoryKey"),
    ),
    (
        "_kCFURLIsRegularFileKey",
        HostConstant::NSString("NSURLIsRegularFileKey"),
    ),
    ("_kCFURLNameKey", HostConstant::NSString("NSURLNameKey")),
    (
        "_kCFURLLocalizedNameKey",
        HostConstant::NSString("NSURLLocalizedNameKey"),
    ),
];
