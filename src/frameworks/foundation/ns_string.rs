/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0.
 * If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//!
//! The `NSString` class cluster, including `NSMutableString`.
//!
//! Resources:
//! - Apple's [String Programming Guide](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/Strings/introStrings.html)

mod path_algorithms;

use super::{_nib_archive_decoder, ns_array, unichar, NSInteger};
use super::{
    NSComparisonResult, NSNotFound, NSOrderedAscending, NSOrderedDescending, NSOrderedSame,
    NSRange, NSUInteger,
};
use crate::abi::VaList;
use crate::frameworks::core_graphics::{CGFloat, CGPoint, CGRect, CGSize};
use crate::frameworks::foundation::ns_string;
use crate::frameworks::uikit::ui_font::{
    self, UILineBreakMode, UILineBreakModeWordWrap, UITextAlignment, UITextAlignmentLeft,
};
use crate::fs::GuestPath;
use crate::mach_o::MachO;
use crate::mem::{guest_size_of, ConstPtr, ConstVoidPtr, GuestUSize, Mem, MutPtr, MutVoidPtr, Ptr, SafeRead};
use crate::objc::{
    autorelease, id, msg, msg_class, nil, objc_classes, release, retain, Class, ClassExports,
    HostObject, NSZonePtr, ObjC,
};
use crate::{fs, Environment};
use encoding_rs::SHIFT_JIS;
use std::borrow::Cow;
use std::collections::HashMap;
use std::io::Write;
use std::iter::Peekable;
use std::string::FromUtf16Error;
use yore::code_pages::CP1252;

pub type NSStringEncoding = NSUInteger;
pub const NSASCIIStringEncoding: NSUInteger = 1;
pub const NSUTF8StringEncoding: NSUInteger = 4;
pub const NSISOLatin1StringEncoding: NSUInteger = 5;
pub const NSShiftJISStringEncoding: NSUInteger = 8;
pub const NSUnicodeStringEncoding: NSUInteger = 10;
pub const NSWindowsCP1252StringEncoding: NSUInteger = 12;
pub const NSMacOSRomanStringEncoding: NSUInteger = 30;
pub const NSUTF16StringEncoding: NSUInteger = NSUnicodeStringEncoding;
pub const NSNextStepLatinStringEncoding: NSUInteger = 0x422;
pub const NSUTF16BigEndianStringEncoding: NSUInteger = 0x90000100;
pub const NSUTF16LittleEndianStringEncoding: NSUInteger = 0x94000100;
pub const NSUTF32LittleEndianStringEncoding: NSUInteger = 0x9c000100;
pub const NSUTF32StringEncoding: NSUInteger = 0x8c000100;
pub const NSUTF32BigEndianStringEncoding: NSUInteger = 0x98000100;

pub type NSStringCompareOptions = NSUInteger;
pub const NSCaseInsensitiveSearch: NSUInteger = 1;
pub const NSLiteralSearch: NSUInteger = 2;
pub const NSBackwardsSearch: NSUInteger = 4;
pub const NSNumericSearch: NSUInteger = 64;

/// Encodings that C strings (null-terminated byte strings) can use.
const C_STRING_FRIENDLY_ENCODINGS: &[NSStringEncoding] = &[
    NSASCIIStringEncoding,
    NSUTF8StringEncoding,
    NSWindowsCP1252StringEncoding,
    NSMacOSRomanStringEncoding,
    NSISOLatin1StringEncoding,
    NSShiftJISStringEncoding,
    NSUTF32LittleEndianStringEncoding,
    NSNextStepLatinStringEncoding,
    NSUTF32StringEncoding,
    NSUTF32BigEndianStringEncoding,
];

pub const NSMaximumStringLength: NSUInteger = (i32::MAX - 1) as _;

#[derive(Default)]
pub struct State {
    static_str_pool: HashMap<&'static str, id>,
}
impl State {
    fn get(env: &mut Environment) -> &mut Self {
        &mut env.framework_state.foundation.ns_string
    }
}

#[allow(non_camel_case_types)]
struct cfstringStruct {
    _isa: Class,
    flags: u32,
    bytes: ConstPtr<u8>,
    length: NSUInteger,
}
unsafe impl SafeRead for cfstringStruct {}

type Utf16String = Vec<u16>;

enum StringHostObject {
    Utf8(Cow<'static, str>),
    Utf16(Utf16String),
}
impl Default for StringHostObject {
    // Phantom-fallback value; an empty borrowed UTF-8 string is the natural
    // "no content" form and doesn't allocate.
    fn default() -> Self {
        StringHostObject::Utf8(Cow::Borrowed(""))
    }
}
impl HostObject for StringHostObject {}
impl StringHostObject {
    fn decode(bytes: Cow<[u8]>, encoding: NSStringEncoding) -> StringHostObject {
        if bytes.is_empty() {
            return StringHostObject::Utf8(Cow::Borrowed(""));
        }

        match encoding {
            NSASCIIStringEncoding => {
                let string: String = bytes
                    .iter()
                    .map(|&b| if b.is_ascii() { b as char } else { '?' })
                    .collect();
                StringHostObject::Utf8(Cow::Owned(string))
            }
            NSMacOSRomanStringEncoding => {
                let string = CP1252.decode(&bytes).to_string();
                StringHostObject::Utf8(Cow::Owned(string))
            }
            NSISOLatin1StringEncoding => {
                let string: String = bytes.iter().map(|&b| b as char).collect();
                StringHostObject::Utf8(Cow::Owned(string))
            }
            NSUTF8StringEncoding => {
                let string = String::from_utf8_lossy(&bytes).into_owned();
                StringHostObject::Utf8(Cow::Owned(string))
            }
            NSUTF32LittleEndianStringEncoding
            | NSUTF32StringEncoding
            | NSUTF32BigEndianStringEncoding
            | NSNextStepLatinStringEncoding
            | NSWindowsCP1252StringEncoding => {
                let string = CP1252.decode(&bytes).to_string();
                StringHostObject::Utf8(Cow::Owned(string))
            }
            NSShiftJISStringEncoding => {
                let (cow, _encoding_used, _had_errors) = SHIFT_JIS.decode(&bytes);
                println!("ShiftJIS decoded {:?}", cow);
                StringHostObject::Utf8(Cow::Owned(cow.to_string()))
            }
            NSUTF16StringEncoding
            | NSUTF16BigEndianStringEncoding
            | NSUTF16LittleEndianStringEncoding => {
                assert!(bytes.len().is_multiple_of(2));
                let is_big_endian = match encoding {
                    NSUTF16BigEndianStringEncoding => true,
                    NSUTF16LittleEndianStringEncoding => false,
                    NSUTF16StringEncoding => match &bytes[0..2] {
                        [0xFE, 0xFF] => true,
                        [0xFF, 0xFE] => false,
                        _ => false,
                    },
                    _ => unreachable!(),
                };
                StringHostObject::Utf16(if is_big_endian {
                    bytes
                        .chunks(2)
                        .map(|chunk| u16::from_be_bytes(chunk.try_into().unwrap()))
                        .collect()
                } else {
                    bytes
                        .chunks(2)
                        .map(|chunk| u16::from_le_bytes(chunk.try_into().unwrap()))
                        .collect()
                })
            }
            _ => {
                println!("Warning: Unimplemented encoding {:#x}. Using lossy UTF-8 fallback to prevent crash.", encoding);
                StringHostObject::Utf8(Cow::Owned(String::from_utf8_lossy(&bytes).into_owned()))
            }
        }
    }
    fn to_utf8(&self) -> Result<Cow<'static, str>, FromUtf16Error> {
        match self {
            StringHostObject::Utf8(utf8) => Ok(utf8.clone()),
            StringHostObject::Utf16(utf16) => Ok(Cow::Owned(String::from_utf16(utf16)?)),
        }
    }
    fn convert_to_utf16_inplace(&mut self) -> (&mut Utf16String, bool) {
        let converted = match self {
            Self::Utf8(_) => {
                *self = Self::Utf16(self.iter_code_units().collect());
                true
            }
            Self::Utf16(_) => false,
        };
        let Self::Utf16(utf16) = self else {
            unreachable!();
        };
        (utf16, converted)
    }
    fn iter_code_units(&self) -> CodeUnitIterator<'_> {
        match self {
            StringHostObject::Utf8(utf8) => CodeUnitIterator::Utf8(utf8.encode_utf16()),
            StringHostObject::Utf16(utf16) => CodeUnitIterator::Utf16(utf16.iter()),
        }
    }
}

enum CodeUnitIterator<'a> {
    Utf8(std::str::EncodeUtf16<'a>),
    Utf16(std::slice::Iter<'a, u16>),
}
impl Iterator for CodeUnitIterator<'_> {
    type Item = u16;
    fn next(&mut self) -> Option<u16> {
        match self {
            CodeUnitIterator::Utf8(iter) => iter.next(),
            CodeUnitIterator::Utf16(iter) => iter.next().copied(),
        }
    }
}
impl Clone for CodeUnitIterator<'_> {
    fn clone(&self) -> Self {
        match self {
            CodeUnitIterator::Utf8(iter) => CodeUnitIterator::Utf8(iter.clone()),
            CodeUnitIterator::Utf16(iter) => CodeUnitIterator::Utf16(iter.clone()),
        }
    }
}
impl CodeUnitIterator<'_> {
    fn strip_prefix(&self, prefix: &CodeUnitIterator, case_insensitive: bool) -> Option<Self> {
        let mut self_match = self.clone();
        let mut prefix_match = prefix.clone();
        loop {
            match prefix_match.next() {
                None => return Some(self_match),
                Some(prefix_c) => {
                    let self_c = self_match.next();
                    if case_insensitive {
                        let self_c_value = self_c?;
                        let Some(a_c) = char::from_u32(self_c_value as u32) else {
                            // Half of a surrogate pair or an otherwise-invalid
                            // code unit; fall back to a direct comparison so
                            // we don't crash the host on malformed strings.
                            if self_c_value != prefix_c {
                                return None;
                            }
                            continue;
                        };
                        let Some(b_c) = char::from_u32(prefix_c as u32) else {
                            if self_c_value != prefix_c {
                                return None;
                            }
                            continue;
                        };
                        if !a_c.to_lowercase().eq(b_c.to_lowercase()) {
                            return None;
                        }
                    } else if self_c != Some(prefix_c) {
                        return None;
                    }
                }
            }
        }
    }
}

pub fn with_format(env: &mut Environment, format: id, args: VaList) -> String {
    let format_string = to_rust_string(env, format);
    println!("Formatting {:?} ({:?})", format, format_string);

    let res = crate::libc::stdio::printf::printf_inner::<true, _>(
        env,
        |_, idx| {
            if idx as usize == format_string.len() {
                b'\0'
            } else {
                format_string.as_bytes()[idx as usize]
            }
        },
        args,
    );
    String::from_utf8_lossy(&res).into_owned()
}

pub fn from_rust_ordering(ordering: std::cmp::Ordering) -> NSComparisonResult {
    match ordering {
        std::cmp::Ordering::Less => NSOrderedAscending,
        std::cmp::Ordering::Equal => NSOrderedSame,
        std::cmp::Ordering::Greater => NSOrderedDescending,
    }
}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation NSString: NSObject

+ (id)allocWithZone:(NSZonePtr)zone {
    // Apple ships a small family of `NSString` subclasses (e.g.
    // `NSLocalizableString`) that share the same private concrete
    // backing store. Their `+alloc` ends up here through normal
    // subclass-method inheritance, which would historically have
    // tripped a strict identity check on `NSString`. Always delegate
    // to the concrete backing store regardless of which subclass we
    // were sent to so storyboard-decoded string subclasses can be
    // constructed without rewriting their `+allocWithZone:`.
    msg_class![env; _touchHLE_NSString allocWithZone:zone]
}

+ (bool)supportsSecureCoding { true }

+ (id)string {
    let str: id = msg![env; this new];
    autorelease(env, str)
}

+ (id)stringWithString:(id)string {
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithString:string];
    autorelease(env, new)
}

+ (id)stringWithUTF8String:(ConstPtr<u8>)utf8_string {
    if utf8_string.is_null() {
        return nil;
    }
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithUTF8String:utf8_string];
    autorelease(env, new)
}

+ (id)stringWithCString:(ConstPtr<u8>)c_string {
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithCString:c_string];
    autorelease(env, new)
}

+ (id)stringWithCString:(ConstPtr<u8>)c_string length:(NSUInteger)length {
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithCString:c_string length:length];
    autorelease(env, new)
}

+ (id)stringWithCString:(ConstPtr<u8>)c_string encoding:(NSStringEncoding)encoding {
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithCString:c_string encoding:encoding];
    autorelease(env, new)
}

+ (id)stringWithContentsOfFile:(id)path {
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithContentsOfFile:path];
    autorelease(env, new)
}

+ (id)stringWithContentsOfURL:(id)url {
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithContentsOfURL:url];
    autorelease(env, new)
}

+ (id)stringWithContentsOfFile:(id)path encoding:(NSStringEncoding)encoding error:(MutPtr<id>)error {
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithContentsOfFile:path encoding:encoding error:error];
    autorelease(env, new)
}

+ (id)stringWithContentsOfFile:(id)path usedEncoding:(MutPtr<NSUInteger>)enc error:(MutPtr<id>)error {
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithContentsOfFile:path usedEncoding:enc error:error];
    autorelease(env, new)
}

+ (id)stringWithContentsOfURL:(id)url encoding:(NSStringEncoding)encoding error:(MutPtr<id>)error {
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithContentsOfURL:url encoding:encoding error:error];
    autorelease(env, new)
}

+ (id)stringWithContentsOfURL:(id)url usedEncoding:(MutPtr<NSUInteger>)enc error:(MutPtr<id>)error {
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithContentsOfURL:url usedEncoding:enc error:error];
    autorelease(env, new)
}

+ (id)stringWithFormat:(id)format, ...args {
    let res = with_format(env, format, args.start());
    let res = from_rust_string(env, res);
    let res = autorelease(env, res);
    msg![env; this stringWithString:res]
}

+ (id)stringWithCharacters:(ConstPtr<unichar>)characters length:(NSUInteger)length {
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithCharacters:characters length:length];
    autorelease(env, new)
}

+ (id)pathWithComponents:(id)components {
    let count: NSUInteger = msg![env; components count];
    if count == 0 { return get_static_str(env, ""); }
    let mut res = msg_class![env; NSString new];
    let enumerator: id = msg![env; components objectEnumerator];
    loop {
        let next: id = msg![env; enumerator nextObject];
        if next == nil { break; }
        let len: NSUInteger = msg![env; next length];
        if len == 0 { continue; }
        res = msg![env; res stringByAppendingPathComponent:next];
    }
    res
}

+ (NSStringEncoding)defaultCStringEncoding {
    NSUTF8StringEncoding
}

- (id)initWithUTF8String:(ConstPtr<u8>)utf8_string {
    msg![env; this initWithCString:utf8_string encoding:NSUTF8StringEncoding]
}

- (id)initWithCString:(ConstPtr<u8>)c_string {
    let encoding: NSStringEncoding = msg_class![env; NSString defaultCStringEncoding];
    msg![env; this initWithCString:c_string encoding:encoding]
}

- (id)initWithCString:(ConstPtr<u8>)c_string length:(NSUInteger)len {
    let encoding: NSStringEncoding = msg_class![env; NSString defaultCStringEncoding];
    msg![env; this initWithBytes:c_string length:len encoding:encoding]
}

- (id)initWithCString:(ConstPtr<u8>)c_string encoding:(NSStringEncoding)encoding {
    assert!(C_STRING_FRIENDLY_ENCODINGS.contains(&encoding), "encoding {encoding}");
    let len: NSUInteger = env.mem.cstr_at(c_string).len().try_into().unwrap();
    msg![env; this initWithBytes:c_string length:len encoding:encoding]
}

- (id)dataUsingEncoding:(NSStringEncoding)encoding {
    msg![env; this dataUsingEncoding:encoding allowLossyConversion:false]
}

- (NSUInteger)length {
    if this == nil { return 0; }
    let host_object = env.objc.borrow_mut::<StringHostObject>(this);
    let (utf16, did_convert) = host_object.convert_to_utf16_inplace();
    if did_convert { println!("[{:?} length]: converted string to UTF-16", this); }
    utf16.len().try_into().unwrap()
}

- (NSStringEncoding)fastestEncoding {
    fastest_encoding(env, this)
}

- (NSStringEncoding)smallestEncoding {
    smallest_encoding(env, this)
}

- (u16)characterAtIndex:(NSUInteger)index {
    let host_object = env.objc.borrow_mut::<StringHostObject>(this);
    let (utf16, did_convert) = host_object.convert_to_utf16_inplace();
    if did_convert { println!("[{:?} characterAtIndex:{:?}]: converted string to UTF-16", this, index); }

    let idx = index as usize;
    if idx >= utf16.len() {
        println!("WARNING: characterAtIndex: index {} out of bounds (len {})", index, utf16.len());
        return 0;
    }
    utf16[idx]
}

- (NSRange)rangeOfCharacterFromSet:(id)set {
    msg![env; this rangeOfCharacterFromSet:set options:0u32]
}

- (NSRange)rangeOfCharacterFromSet:(id)set options:(NSStringCompareOptions)options {
    let len: NSUInteger = msg![env; this length];
    let range = NSRange { location: 0, length: len };
    msg![env; this rangeOfCharacterFromSet:set options:options range:range]
}

- (NSRange)rangeOfCharacterFromSet:(id)set options:(NSStringCompareOptions)options range:(NSRange)search_range {
    let search_loc = search_range.location;
    let search_len = search_range.length;
    let len: NSUInteger = msg![env; this length];

    if set == nil || search_loc >= len || search_len == 0 {
        return NSRange { location: NSNotFound as NSUInteger, length: 0 };
    }

    let end_bound = (search_loc + search_len).min(len);
    let is_backwards = (options & NSBackwardsSearch) != 0;
    if is_backwards {
        for i in (search_loc..end_bound).rev() {
            let c: u16 = msg![env; this characterAtIndex:i];
            let is_member: bool = msg![env; set characterIsMember:c];
            if is_member { return NSRange { location: i, length: 1 }; }
        }
    } else {
        for i in search_loc..end_bound {
            let c: u16 = msg![env; this characterAtIndex:i];
            let is_member: bool = msg![env; set characterIsMember:c];
            if is_member { return NSRange { location: i, length: 1 }; }
        }
    }

    NSRange { location: NSNotFound as NSUInteger, length: 0 }
}

- (NSUInteger)lengthOfBytesUsingEncoding:(NSStringEncoding)encoding {
    let string = to_rust_string(env, this);
    if C_STRING_FRIENDLY_ENCODINGS.contains(&encoding) {
        // For byte-compatible encodings, the byte length equals the
        // string's UTF-8 byte length (for ASCII content) or the
        // character count (for single-byte charsets). Use the UTF-8
        // byte length as a safe upper bound — this matches what
        // Apple's implementation returns for UTF-8.
        string.len().try_into().unwrap()
    } else if encoding == NSUnicodeStringEncoding || encoding == NSUTF16StringEncoding {
        // UTF-16: each code unit is 2 bytes. Rust's encode_utf16()
        // gives exact code unit count.
        (string.encode_utf16().count() * 2) as NSUInteger
    } else if encoding == NSUTF16BigEndianStringEncoding {
        (string.encode_utf16().count() * 2) as NSUInteger
    } else if encoding == NSUTF32StringEncoding || encoding == NSUTF32LittleEndianStringEncoding {
        // UTF-32: each character is 4 bytes.
        (string.chars().count() * 4) as NSUInteger
    } else {
        // Fallback: return UTF-8 byte length rather than crashing.
        // This is a safe approximation and prevents the emulator from
        // aborting on an uncommon encoding.
        log!(
            "Warning: lengthOfBytesUsingEncoding: unknown encoding {}; returning UTF-8 byte count as fallback.",
            encoding
        );
        string.len().try_into().unwrap()
    }
}

- (NSRange)rangeOfString:(id)search_string {
    msg![env; this rangeOfString:search_string options:0u32]
}

- (NSRange)rangeOfString:(id)search_string options:(NSStringCompareOptions)options {
    let len: NSUInteger = msg![env; this length];
    let len_search: NSUInteger = msg![env; search_string length];
    if len_search == 0 { return NSRange { location: NSNotFound as NSUInteger, length: 0 }; }
    match options {
        NSLiteralSearch | 0 => {
            for i in 0..len {
                if is_match_at_position(env, this, search_string, i, len, len_search, |a, b| a == b) {
                    return NSRange { location: i, length: len_search }
                }
            }
        },
        NSCaseInsensitiveSearch => {
            let compare = |a, b| {
                let (Some(a_c), Some(b_c)) = (char::from_u32(a as u32), char::from_u32(b as u32)) else { return false; };
                a_c.to_lowercase().eq(b_c.to_lowercase())
            };
            for i in 0..len {
                if is_match_at_position(env, this, search_string, i, len, len_search, compare) {
                    return NSRange { location: i, length: len_search }
                }
            }
        },
        NSBackwardsSearch => {
            for i in (0..len).rev() {
                if is_match_at_position(env, this, search_string, i, len, len_search, |a, b| a == b) {
                    return NSRange { location: i, length: len_search }
                }
            }
        },
        _ => {
            println!("Warning: rangeOfString:options: unhandled options {}, falling back to literal search", options);
            for i in 0..len {
                if is_match_at_position(env, this, search_string, i, len, len_search, |a, b| a == b) {
                    return NSRange { location: i, length: len_search }
                }
            }
        }
    }
    NSRange { location: NSNotFound as NSUInteger, length: 0 }
}

- (NSRange)rangeOfString:(id)search_string options:(NSStringCompareOptions)options range:(NSRange)search_range {
    let search_loc = search_range.location;
    let search_len = search_range.length;
    let len: NSUInteger = msg![env; this length];
    let len_search: NSUInteger = msg![env; search_string length];
    if len_search == 0 || search_loc >= len || search_len == 0 {
        return NSRange { location: NSNotFound as NSUInteger, length: 0 };
    }

    let end_bound = (search_loc + search_len).min(len);
    let max_start = end_bound.saturating_sub(len_search);
    if search_loc > max_start {
        return NSRange { location: NSNotFound as NSUInteger, length: 0 };
    }

    let is_case_insensitive = (options & NSCaseInsensitiveSearch) != 0;
    let is_backwards = (options & NSBackwardsSearch) != 0;
    if is_backwards {
        for i in (search_loc..=max_start).rev() {
            if is_case_insensitive {
                let compare = |a: u16, b: u16| {
                    let (Some(a_c), Some(b_c)) = (char::from_u32(a as u32), char::from_u32(b as u32)) else { return a == b; };
                    a_c.to_lowercase().eq(b_c.to_lowercase())
                };
                if is_match_at_position(env, this, search_string, i, len, len_search, compare) {
                    return NSRange { location: i, length: len_search };
                }
            } else {
                if is_match_at_position(env, this, search_string, i, len, len_search, |a, b| a == b) {
                    return NSRange { location: i, length: len_search };
                }
            }
        }
    } else {
        for i in search_loc..=max_start {
            if is_case_insensitive {
                let compare = |a: u16, b: u16| {
                    let (Some(a_c), Some(b_c)) = (char::from_u32(a as u32), char::from_u32(b as u32)) else { return a == b; };
                    a_c.to_lowercase().eq(b_c.to_lowercase())
                };
                if is_match_at_position(env, this, search_string, i, len, len_search, compare) {
                    return NSRange { location: i, length: len_search };
                }
            } else {
                if is_match_at_position(env, this, search_string, i, len, len_search, |a, b| a == b) {
                    return NSRange { location: i, length: len_search };
                }
            }
        }
    }
    NSRange { location: NSNotFound as NSUInteger, length: 0 }
}

- (id)description { this }

- (NSUInteger)hash { super::hash_helper(&to_rust_string(env, this)) }

- (bool)isEqual:(id)other {
    if this == other { return true; }
    let class: Class = msg_class![env; NSString class];
    if !msg![env; other isKindOfClass:class] { return false; }
    to_rust_string(env, this) == to_rust_string(env, other)
}

- (bool)isEqualToString:(id)other {
    if this == other { return true; }
    if other == nil { return false; }
    to_rust_string(env, this) == to_rust_string(env, other)
}

- (bool)hasPrefix:(id)str {
    let str = to_rust_string(env, str).to_string();
    to_rust_string(env, this).starts_with(&str)
}

- (bool)hasSuffix:(id)str {
    let str = to_rust_string(env, str).to_string();
    to_rust_string(env, this).ends_with(&str)
}

- (NSComparisonResult)localizedCompare:(id)other {
    assert!(to_rust_string(env, this).is_ascii());
    assert!(to_rust_string(env, other).is_ascii());
    msg![env; this compare:other]
}

- (NSComparisonResult)compare:(id)other {
    msg![env; this compare:other options:NSLiteralSearch]
}

- (NSComparisonResult)caseInsensitiveCompare:(id)other {
    msg![env; this compare:other options:NSCaseInsensitiveSearch]
}

- (NSComparisonResult)compare:(id)other options:(NSStringCompareOptions)options range:(NSRange)range {
    let substr = msg![env; this substringWithRange:range];
    msg![env; substr compare:other options:options]
}

- (NSComparisonResult)compare:(id)other options:(NSStringCompareOptions)mask {
    fn ascii_number(iter: &mut Peekable<CodeUnitIterator>, leftmost_digit: char) -> u32 {
        let mut num = leftmost_digit.to_digit(10).unwrap();
        while let Some(a_digit_char) = iter.next_if(|&x| char::from_u32(x as u32).is_some_and(|y| y.is_ascii_digit())) {
            num = num * 10 + char::from_u32(a_digit_char as u32).unwrap().to_digit(10).unwrap();
        }
        num
    }

    // Apple's documentation says `[NSString compare:]` raises
    // `NSInvalidArgumentException` if `other` is nil, but real-world iPhone
    // OS apps (e.g. Angry Birds Crystal init path — HyperHLE log shows
    // `assertion 'left != right' failed; left: (null), right: (null)`)
    // pass nil and rely on a soft failure. touchHLE doesn't implement
    // Objective-C exceptions, so the closest "documented" behaviour is to
    // treat the non-nil receiver as ordered after nil instead of crashing
    // the emulator. (`isEqualToString:` in this file already follows the
    // same lenient convention.)
    if other == nil {
        log!(
            "Warning: [NSString {:?} compare:nil options:{:#x}] — returning \
             NSOrderedDescending instead of raising NSInvalidArgumentException.",
            this,
            mask
        );
        return NSOrderedDescending;
    }
    let mut a_iter = env.objc.borrow::<StringHostObject>(this).iter_code_units().peekable();
    let mut b_iter = env.objc.borrow::<StringHostObject>(other).iter_code_units().peekable();
    let mask = if mask == 0 { NSLiteralSearch } else { mask };
    match mask {
        NSCaseInsensitiveSearch => {
            loop {
                let a_next = a_iter.next();
                let b_next = b_iter.next();
                let (Some(a_unit), Some(b_unit)) = (a_next, b_next) else { return from_rust_ordering(a_next.cmp(&b_next)); };
                let (a_c, b_c) = match (char::from_u32(a_unit as u32), char::from_u32(b_unit as u32)) {
                    (Some(a), Some(b)) => (a, b),
                    _ => {
                        // One of the code units is a UTF-16 surrogate half
                        // (`char::from_u32` rejects U+D800..=U+DFFF). Fall
                        // back to byte-order comparison on the raw u16s
                        // rather than panicking the host.
                        log!(
                            "Warning: NSString compare: unpaired surrogate(s) at U+{:04X}/U+{:04X}; falling back to code-unit compare.",
                            a_unit,
                            b_unit
                        );
                        let ord = a_unit.cmp(&b_unit);
                        if ord != std::cmp::Ordering::Equal { return from_rust_ordering(ord); }
                        continue;
                    }
                };

                let insensitive_order = a_c.to_lowercase().cmp(b_c.to_lowercase());
                if insensitive_order != std::cmp::Ordering::Equal { return from_rust_ordering(insensitive_order); }
            }
        },
        NSLiteralSearch => from_rust_ordering(a_iter.cmp(b_iter)),
        NSNumericSearch => {
            loop {
                let a_next = a_iter.next();
                let b_next = b_iter.next();
                let (Some(a_unit), Some(b_unit)) = (a_next, b_next) else { return from_rust_ordering(a_next.cmp(&b_next)); };
                let (a_c, b_c) = match (char::from_u32(a_unit as u32), char::from_u32(b_unit as u32)) {
                    (Some(a), Some(b)) => (a, b),
                    _ => {
                        log!(
                            "Warning: NSString compare (numeric): unpaired surrogate(s) at U+{:04X}/U+{:04X}; falling back to code-unit compare.",
                            a_unit,
                            b_unit
                        );
                        let ord = a_unit.cmp(&b_unit);
                        if ord != std::cmp::Ordering::Equal { return from_rust_ordering(ord); }
                        continue;
                    }
                };
                if a_c.is_ascii_digit() && b_c.is_ascii_digit() {
                    let a_int = ascii_number(&mut a_iter, a_c);
                    let b_int = ascii_number(&mut b_iter, b_c);

                    let numeric_order = a_int.cmp(&b_int);
                    if numeric_order != std::cmp::Ordering::Equal { return from_rust_ordering(numeric_order); }
                } else {
                    let char_order = a_c.cmp(&b_c);
                    if char_order != std::cmp::Ordering::Equal { return from_rust_ordering(char_order); }
                }
            }
        },
        _ => {
            println!("Warning: compare:options: unhandled mask {}, falling back to literal search", mask);
            from_rust_ordering(a_iter.cmp(b_iter))
        }
    }
}

- (NSComparisonResult)localizedCaseInsensitiveCompare:(id)other {
    assert!(to_rust_string(env, this).is_ascii());
    assert!(to_rust_string(env, other).is_ascii());
    msg![env; this compare:other options:NSCaseInsensitiveSearch]
}

- (id)copyWithZone:(NSZonePtr)_zone { retain(env, this) }

- (id)mutableCopyWithZone:(NSZonePtr)_zone {
    let str_mut: id = msg_class![env; NSMutableString alloc];
    let str_mut: id = msg![env; str_mut init];
    () = msg![env; str_mut setString:this];
    str_mut
}


- (bool)getCString:(MutPtr<u8>)buffer maxLength:(NSUInteger)buffer_size encoding:(NSStringEncoding)encoding {
    get_bytes_buffer_inner(env, this, buffer, buffer_size, encoding, true)
}

- (())getCString:(MutPtr<u8>)buffer maxLength:(NSUInteger)max_length {
    // Two-argument variant: encoding defaults to the default C-string encoding.
    let encoding: NSStringEncoding = msg_class![env; NSString defaultCStringEncoding];
    let _: bool = msg![env; this getCString:buffer maxLength:max_length encoding:encoding];
}

- (())getCString:(MutPtr<u8>)buffer {
    let encoding: NSStringEncoding = msg_class![env; NSString defaultCStringEncoding];
    let length = (u32::MAX - buffer.to_bits()).min(NSMaximumStringLength);
    let res: bool = msg![env; this getCString:buffer maxLength:length encoding:encoding];
    assert!(res);
}

// -[NSString getBytes:maxLength:usedLength:encoding:options:range:remainingRange:]
// Apple: https://developer.apple.com/documentation/foundation/nsstring/1408564-getbytes
//
// Writes a representation of the receiver, encoded with `encoding`, into the
// memory at `buffer` (at most `max_buffer_count` bytes). The substring covered
// is given by `range`, expressed in UTF-16 code units. On return:
//   * usedLength receives the number of bytes actually written (if non-NULL).
//   * leftover receives the portion of `range` that did not fit
//     (if non-NULL). leftover.length == 0 means the whole range fit.
// Returns YES iff at least one full character fit; NO if `range` is non-empty
// but nothing could be encoded.
//
// Notes on our implementation:
//   * We encode the entire receiver up-front via `bytes_for_encoding`, then
//     slice based on `range` measured in UTF-16 code units. This matches what
//     guest code actually relies on for the small subset of `range` values it
//     ever passes (typically `{0, length}`).
//   * The `options` mask is best-effort: bit 0 (`NSStringEncodingConversionAllowLossy`)
//     is accepted; bit 1 (`NSStringEncodingConversionExternalRepresentation`)
//     is currently ignored.
- (bool)getBytes:(MutVoidPtr)buffer
       maxLength:(NSUInteger)max_buffer_count
      usedLength:(MutPtr<NSUInteger>)used_length
        encoding:(NSStringEncoding)encoding
         options:(NSUInteger)_options
           range:(NSRange)range
  remainingRange:(MutPtr<NSRange>)leftover {
    // Re-encode the whole receiver, then slice by the requested UTF-16 range.
    let full_bytes = bytes_for_encoding(env, this, encoding);

    // Map the UTF-16 range to a byte range over `full_bytes`.
    let prefix_units = range.location as usize;
    let suffix_start_unit = prefix_units + range.length as usize;

    // Bytes consumed to cover the first `prefix_units` UTF-16 code units.
    let prefix_bytes = code_units_consumed_for_bytes(env, this, encoding, full_bytes.len()) as usize;
    // Walk forward to find byte offsets corresponding to the unit boundaries.
    // To keep the implementation straightforward, recompute the byte/unit map
    // by re-encoding character-by-character. For most NSString instances this
    // is cheap (the strings in question are tiny localization keys).
    let rust_string = to_rust_string(env, this);
    let mut units_seen: usize = 0;
    let mut byte_offset_start: usize = 0;
    let mut byte_offset_end: usize = full_bytes.len();
    let mut have_start = prefix_units == 0;
    for ch in rust_string.chars() {
        let unit_step = ch.len_utf16();
        let byte_step = match encoding {
            NSUTF16LittleEndianStringEncoding
            | NSUTF16BigEndianStringEncoding
            | NSUTF16StringEncoding
            | NSUnicodeStringEncoding => unit_step * 2,
            NSUTF32LittleEndianStringEncoding
            | NSUTF32BigEndianStringEncoding
            | NSUTF32StringEncoding => 4,
            NSShiftJISStringEncoding => {
                let mut buf = [0u8; 4];
                let temp = ch.encode_utf8(&mut buf);
                let (cow, _, _) = SHIFT_JIS.encode(temp);
                cow.len()
            }
            _ => ch.len_utf8(),
        };
        if !have_start {
            byte_offset_start += byte_step;
            if units_seen + unit_step >= prefix_units {
                have_start = true;
            }
        }
        units_seen += unit_step;
        if units_seen >= suffix_start_unit {
            byte_offset_end = byte_offset_start
                .saturating_add(byte_step)
                .max(byte_offset_start);
            // We've consumed enough to cover the range; record where the
            // "end" pointer landed (the next character starts here).
            byte_offset_end = (byte_offset_start
                + (full_bytes.len().saturating_sub(byte_offset_start)))
                .min(full_bytes.len());
            break;
        }
    }
    let _ = prefix_bytes; // not used directly; kept for clarity / future use.

    let slice_end = byte_offset_end.min(full_bytes.len());
    let slice_start = byte_offset_start.min(slice_end);
    let slice = &full_bytes[slice_start..slice_end];

    let copy_len = (slice.len()).min(max_buffer_count as usize);
    if copy_len > 0 && !buffer.is_null() {
        env.mem
            .bytes_at_mut(buffer.cast::<u8>(), copy_len as u32)
            .copy_from_slice(&slice[..copy_len]);
    }

    if !used_length.is_null() {
        env.mem.write(used_length, copy_len as NSUInteger);
    }

    if !leftover.is_null() {
        // How many UTF-16 code units did the bytes we actually wrote cover?
        // Use the same incremental walk so the math agrees with what we
        // wrote out above.
        let mut consumed_bytes = 0usize;
        let mut consumed_units = 0usize;
        for ch in rust_string[..].chars().skip_while(|_| false) {
            let unit_step = ch.len_utf16();
            let byte_step = match encoding {
                NSUTF16LittleEndianStringEncoding
                | NSUTF16BigEndianStringEncoding
                | NSUTF16StringEncoding
                | NSUnicodeStringEncoding => unit_step * 2,
                NSUTF32LittleEndianStringEncoding
                | NSUTF32BigEndianStringEncoding
                | NSUTF32StringEncoding => 4,
                NSShiftJISStringEncoding => {
                    let mut buf = [0u8; 4];
                    let temp = ch.encode_utf8(&mut buf);
                    let (cow, _, _) = SHIFT_JIS.encode(temp);
                    cow.len()
                }
                _ => ch.len_utf8(),
            };
            if consumed_bytes + byte_step > copy_len {
                break;
            }
            consumed_bytes += byte_step;
            consumed_units += unit_step;
        }
        let consumed_units = consumed_units as NSUInteger;
        let new_loc = range.location.saturating_add(consumed_units);
        let new_len = range.length.saturating_sub(consumed_units);
        env.mem.write(
            leftover,
            NSRange {
                location: new_loc,
                length: new_len,
            },
        );
    }

    // Apple returns NO only when the requested range is non-empty but nothing
    // could be encoded. Empty range -> trivially YES.
    range.length == 0 || copy_len > 0
}

- (id)componentsSeparatedByString:(id)separator {
    if separator == nil {
        let res = ns_array::from_vec(env, vec![this]);
        return autorelease(env, res);
    }

    let mut main_iter = env.objc.borrow::<StringHostObject>(this).iter_code_units();
    let sep_iter = env.objc.borrow::<StringHostObject>(separator).iter_code_units();
    if sep_iter.clone().next().is_none() {
        let res = ns_array::from_vec(env, vec![this]);
        return autorelease(env, res);
    }

    let mut components = Vec::<Utf16String>::new();
    let mut current_component: Utf16String = Vec::new();
    loop {
        if let Some(new_main_iter) = main_iter.strip_prefix(&sep_iter, false) {
            components.push(std::mem::take(&mut current_component));
            main_iter = new_main_iter;
        } else {
            match main_iter.next() {
                Some(cur) => current_component.push(cur),
                None => break,
            }
        }
    }
    components.push(current_component);
    let class = env.objc.get_known_class("_touchHLE_NSString", &mut env.mem);
    let component_ns_strings: Vec<id> = components.drain(..).map(|utf16| {
        let host_object = Box::new(StringHostObject::Utf16(utf16));
        env.objc.alloc_object(class, host_object, &mut env.mem)
    }).collect();
    let array = ns_array::from_vec(env, component_ns_strings);
    autorelease(env, array)
}

- (())getCharacters:(MutPtr<unichar>)buffer range:(NSRange)range {
    let ranged = msg![env; this substringWithRange:range];
    msg![env; ranged getCharacters:buffer]
}

- (())getCharacters:(MutPtr<unichar>)buffer {
    let host_object = env.objc.borrow_mut::<StringHostObject>(this);
    let (utf16, did_convert) = host_object.convert_to_utf16_inplace();
    if did_convert { println!("[{:?} getCharacters:{:?}]: converted string to UTF-16", this, buffer); }

    let len: GuestUSize = guest_size_of::<unichar>() * utf16.len() as GuestUSize;
    let tmp_vec: Vec<u8> = utf16.iter().flat_map(|c| u16::to_le_bytes(*c)).collect();
    _ = env.mem.bytes_at_mut(buffer.cast(), len).write(tmp_vec.as_slice()).unwrap();
}

- (ConstPtr<u8>)cStringUsingEncoding:(NSStringEncoding)encoding {
    let string = to_rust_string(env, this);
    let bytes: Vec<u8> = match encoding {
        NSASCIIStringEncoding | NSMacOSRomanStringEncoding | NSISOLatin1StringEncoding | NSNextStepLatinStringEncoding => {
            string.as_bytes().to_vec()
        },
        NSUTF8StringEncoding | NSUTF32LittleEndianStringEncoding | NSUTF32StringEncoding | NSUTF32BigEndianStringEncoding => {
            string.as_bytes().to_vec()
        },
        NSUTF16LittleEndianStringEncoding | NSUnicodeStringEncoding => string.encode_utf16().flat_map(u16::to_le_bytes).collect(),
        NSUTF16BigEndianStringEncoding => string.encode_utf16().flat_map(u16::to_be_bytes).collect(),
        NSShiftJISStringEncoding => {
            let (cow, _, _) = SHIFT_JIS.encode(&string);
            cow.into_owned()
        },
        _ => {
            println!("Warning: cStringUsingEncoding requested with unknown encoding: {}, falling back to UTF-8", encoding);
            string.as_bytes().to_vec()
        }
    };
    let null_size: GuestUSize = match encoding {
        NSUTF16LittleEndianStringEncoding | NSUnicodeStringEncoding | NSUTF16BigEndianStringEncoding => 2,
        NSUTF32LittleEndianStringEncoding | NSUTF32BigEndianStringEncoding | NSUTF32StringEncoding => 4,
        _ => 1,
    };
    let bytes_size = bytes.len() as GuestUSize;
    let total_size: GuestUSize = bytes_size + null_size;
    let c_string: MutPtr<u8> = env.mem.alloc(total_size).cast();

    _ = env.mem.bytes_at_mut(c_string, bytes_size).write(&bytes).unwrap();
    for i in 0..null_size {
        env.mem.write(c_string + bytes_size + i, b'\0');
    }

    let _: id = msg_class![env; NSData dataWithBytesNoCopy:(c_string.cast_void()) length:total_size];
    c_string.cast_const()
}

- (ConstPtr<u8>)cString { msg![env; this UTF8String] }

- (ConstPtr<u8>)UTF8String { msg![env; this cStringUsingEncoding:NSUTF8StringEncoding] }

- (id)substringToIndex:(NSUInteger)to {
    let cap = (to as usize).min(1024);
    let mut res_utf16: Utf16String = Vec::with_capacity(cap);
    for_each_code_unit(env, this, |idx, c| { if idx < to { res_utf16.push(c); } });
    let res = msg_class![env; _touchHLE_NSString alloc];
    *env.objc.borrow_mut(res) = StringHostObject::Utf16(res_utf16);
    autorelease(env, res)
}

- (id)substringFromIndex:(NSUInteger)from {
    let mut res_utf16: Utf16String = Vec::new();
    for_each_code_unit(env, this, |idx, c| { if idx >= from { res_utf16.push(c); } });
    let res = msg_class![env; _touchHLE_NSString alloc];
    *env.objc.borrow_mut(res) = StringHostObject::Utf16(res_utf16);
    autorelease(env, res)
}

// Apple: Returns a new string formed from the receiver by either removing
// characters from the end, or by appending as many occurrences as necessary
// of a given pad string starting at a given index.
// https://developer.apple.com/documentation/foundation/nsstring/1416085-stringbypaddingtolength
- (id)stringByPaddingToLength:(NSUInteger)new_length
                   withString:(id)pad_string
              startingAtIndex:(NSUInteger)pad_index {
    let current_length: NSUInteger = msg![env; this length];
    if new_length <= current_length {
        return msg![env; this substringToIndex:new_length];
    }
    if pad_string == nil {
        // Apple raises NSInvalidArgumentException; soft-fail to original.
        log!("Warning: [NSString stringByPaddingToLength:withString:nil startingAtIndex:] called.");
        let copy: id = msg![env; this copy];
        return autorelease(env, copy);
    }
    let pad_length: NSUInteger = msg![env; pad_string length];
    if pad_length == 0 {
        // Apple raises NSInvalidArgumentException for empty pad string.
        log!("Warning: [NSString stringByPaddingToLength:withString:@\"\" startingAtIndex:] called.");
        let copy: id = msg![env; this copy];
        return autorelease(env, copy);
    }
    let safe_pad_index = pad_index % pad_length;
    let mut res_utf16: Utf16String = Vec::with_capacity(new_length as usize);
    for_each_code_unit(env, this, |_idx, c| { res_utf16.push(c); });
    let mut cursor = safe_pad_index;
    while (res_utf16.len() as NSUInteger) < new_length {
        let c: u16 = msg![env; pad_string characterAtIndex:cursor];
        res_utf16.push(c);
        cursor += 1;
        if cursor >= pad_length { cursor = 0; }
    }
    let res = msg_class![env; _touchHLE_NSString alloc];
    *env.objc.borrow_mut(res) = StringHostObject::Utf16(res_utf16);
    autorelease(env, res)
}

- (id)stringByTrimmingCharactersInSet:(id)set {
    let initial_length: NSUInteger = msg![env; this length];
    let mut res_start: NSUInteger = 0;
    let mut res_end = initial_length;
    while res_start < initial_length {
        let c: u16 = msg![env; this characterAtIndex:res_start];
        if msg![env; set characterIsMember:c] { res_start += 1; } else { break; }
    }
    while res_end > res_start {
        let c: u16 = msg![env; this characterAtIndex:(res_end - 1)];
        if msg![env; set characterIsMember:c] { res_end -= 1; } else { break; }
    }
    assert!(res_end >= res_start);
    let res_length = res_end - res_start;
    if res_length == initial_length {
        let ret = msg![env; this copy];
        autorelease(env, ret)
    } else {
        let range = NSRange{ location: res_start, length: res_length };
        let string: id = msg![env; this substringWithRange:range];
        string
    }
}

- (id)stringByReplacingOccurrencesOfString:(id)target withString:(id)replacement {
    let length: NSUInteger = msg![env; this length];
    let range = NSRange { location: 0, length };
    msg![env; this stringByReplacingOccurrencesOfString:target withString:replacement options:0u32 range:range]
}

- (id)stringByReplacingOccurrencesOfString:(id)target withString:(id)replacement options:(NSStringCompareOptions)options range:(NSRange)range {
    let loc = range.location;
    let len = range.length;
    let left: id = msg![env; this substringToIndex:loc];
    let middle: id = msg![env; this substringWithRange:range];
    let right: id = msg![env; this substringFromIndex:(loc + len)];
    let new_middle: id = string_by_replacing_occurrences_inner(env, middle, target, replacement, options);
    let res: id = msg![env; left stringByAppendingString:new_middle];
    msg![env; res stringByAppendingString:right]
}

- (id)stringByAppendingString:(id)other {
    // ЧЕСТНЫЙ ФИКС: Вместо жесткого assert, который убивает эмулятор,
    // эмулируем обработку исключения NSInvalidArgumentException.
    if other == nil {
        log!("Warning: [NSString stringByAppendingString:nil] called. This would throw NSInvalidArgumentException on iOS. Returning original string to prevent crash.");
        return this;
    }

    let this_len: NSUInteger = msg![env; this length];
    let other_len: NSUInteger = msg![env; other length];
    let mut new_utf16 = Vec::with_capacity((this_len + other_len) as usize);
    for_each_code_unit(env, this, |_idx, c| { new_utf16.push(c); });
    for_each_code_unit(env, other, |_idx, c| { new_utf16.push(c); });
    let class = env.objc.get_known_class("_touchHLE_NSString", &mut env.mem);
    let host_object = Box::new(StringHostObject::Utf16(new_utf16));
    env.objc.alloc_object(class, host_object, &mut env.mem)
}

- (id)stringByAppendingFormat:(id)format, ...args {
    let new_string = with_format(env, format,  args.start());
    let new_string = from_rust_string(env, new_string);
    let new_string = msg![env; this stringByAppendingString:new_string];
    autorelease(env, new_string)
}

- (id)stringByDeletingLastPathComponent {
    let string = to_rust_string(env, this);
    let (res, _) = path_algorithms::split_last_path_component(&string);
    let new_string = from_rust_string(env, String::from(res));
    autorelease(env, new_string)
}

- (id)lastPathComponent {
    let string = to_rust_string(env, this);
    let (_, res) = path_algorithms::split_last_path_component(&string);
    let new_string = from_rust_string(env, String::from(res));
    autorelease(env, new_string)
}

- (id)pathComponents {
    let string = to_rust_string(env, this);
    let vec = path_algorithms::split_path_components(&string);
    let vec = vec.iter().map(|component| from_rust_string(env, component.to_string())).collect();
    let array = ns_array::from_vec(env, vec);
    autorelease(env, array)
}

- (id)stringByDeletingPathExtension {
    let string = to_rust_string(env, this);
    let (res, _) = path_algorithms::split_path_extension(&string);
    let new_string = from_rust_string(env, String::from(res));
    autorelease(env, new_string)
}

- (id)pathExtension {
    let string = to_rust_string(env, this);
    let (_, res) = path_algorithms::split_path_extension(&string);
    let new_string = from_rust_string(env, String::from(res));
    autorelease(env, new_string)
}

- (ConstPtr<u8>)fileSystemRepresentation {
    let file_manager: id = msg_class![env; NSFileManager defaultManager];
    msg![env; file_manager fileSystemRepresentationWithPath:this]
}

- (bool)getFileSystemRepresentation:(MutPtr<u8>)buffer
                          maxLength:(NSUInteger)max_length {
    // Apple docs (NSString — "Working with Paths"):
    // "Returns a Boolean value indicating whether the receiver can fit in
    //  `maxLength` bytes, in the file-system representation. The buffer is
    //  filled with the C-string in a format suitable for use with file-
    //  system calls. Returns NO if `maxLength` would be exceeded (the
    //  buffer contents are unspecified in that case)."
    //
    // On Darwin the file system representation is UTF-8 normalized to HFS+
    // canonical form (NFD-ish). We don't perform Unicode normalization
    // because our backing fs already operates on raw UTF-8, matching
    // NSFileManager's `-fileSystemRepresentationWithPath:` above.
    if buffer.is_null() {
        return false;
    }
    let bytes = to_rust_string(env, this).into_owned().into_bytes();
    // `maxLength` includes the room required for the terminating NUL, per
    // the documented behavior of related getCString:maxLength: methods.
    if (bytes.len() as u64) + 1 > max_length as u64 {
        log_dbg!(
            "-[NSString getFileSystemRepresentation:maxLength:]: \
             string of {} bytes does not fit in buffer of {} bytes; \
             returning NO without writing.",
            bytes.len(),
            max_length,
        );
        return false;
    }
    for (i, byte) in bytes.iter().enumerate() {
        env.mem.write(buffer + i as GuestUSize, *byte);
    }
    env.mem.write(buffer + bytes.len() as GuestUSize, 0u8);
    true
}

// Pragmatic compatibility shim — NOT in Apple's documented NSString API.
//
// A number of iPhone OS 2.x / 3.x applications shipped a small NSString
// category (often as part of utility libraries like BBFramework, an
// in-house "NSString+Path" helper, or copy-pasted ASIHTTPRequest code)
// that forwards `fileExistsAtPath:` to NSFileManager. When the binary's
// __objc_selrefs / __objc_methname section contains non-UTF-8 entries
// — typical for partially-decrypted IPAs — `register_bin_categories`
// skips the entry and the app then spams the runtime with thousands of
// "_touchHLE_NSString does not respond to selector fileExistsAtPath:"
// warnings while silently getting the wrong answer.
//
// Implementing the same forward as a host method on NSString gives the
// correct semantics (file existence at `path`) and eliminates the log
// flood. If the app's own category registers successfully, it takes
// precedence over this implementation (per Apple's documented category
// override behavior) so we don't change observable behavior for
// correctly-loaded apps.
- (bool)fileExistsAtPath:(id)path { // NSString *
    let file_manager: id = msg_class![env; NSFileManager defaultManager];
    msg![env; file_manager fileExistsAtPath:path]
}

- (id)stringByAddingPercentEscapesUsingEncoding:(NSStringEncoding)encoding {
    let bytes = bytes_for_percent_escaping(env, this, encoding);
    let mut escaped = String::with_capacity(bytes.len());
    for byte in bytes.iter() {
        if byte.is_ascii_alphanumeric()
            || b"-_.~".contains(byte)
            || b"!*'();:@&=+$,/?%#[]".contains(byte)
        {
            escaped.push(*byte as char);
        } else {
            use std::fmt::Write;
            write!(&mut escaped, "%{:02X}", byte).unwrap();
        }
    }
    let new: id = from_rust_string(env, escaped);
    autorelease(env, new)
}

- (id)stringByReplacingPercentEscapesUsingEncoding:(NSStringEncoding)encoding {
    let source = to_rust_string(env, this);
    let mut bytes = Vec::with_capacity(source.len());
    let source_bytes = source.as_bytes();
    let mut i = 0;
    while i < source_bytes.len() {
        if source_bytes[i] == b'%' && i + 2 < source_bytes.len() {
            let hi = (source_bytes[i + 1] as char).to_digit(16);
            let lo = (source_bytes[i + 2] as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                bytes.push(((hi << 4) | lo) as u8);
                i += 3;
                continue;
            }
        }
        bytes.push(source_bytes[i]);
        i += 1;
    }
    let host_object = StringHostObject::decode(Cow::Owned(bytes), encoding);
    let class = env.objc.get_known_class("_touchHLE_NSString", &mut env.mem);
    let new = env.objc.alloc_object(class, Box::new(host_object), &mut env.mem);
    autorelease(env, new)
}

- (id)stringByAppendingPathComponent:(id)component {
    let base_str = to_rust_string(env, this);
    let component_str = to_rust_string(env, component);
    let res = path_algorithms::string_by_appending_path_component(&base_str, &component_str);
    let new_string = from_rust_string(env, res);
    autorelease(env, new_string)
}

- (id)stringByAppendingPathExtension:(id)extension {
    let mut combined = to_rust_string(env, this).into_owned();
    let extension_string = to_rust_string(env, extension);
    if !extension_string.is_empty(){
        combined.push('.');
        combined.push_str(&extension_string);
    }
    let new_string = from_rust_string(env, combined);
    autorelease(env, new_string)
}

- (id)stringByExpandingTildeInPath {
    let path = to_rust_string(env, this);
    let new_path_str = if let Some(new_path) = path.strip_prefix('~') {
        let within_home_dir = new_path.split_once('/').map(|x| x.1).unwrap_or("");
        let guest_path = env.fs.home_directory().join(within_home_dir);
        let resolved = fs::resolve_path(&guest_path, None);
        format!("/{}", resolved.join("/"))
    } else {
        path.to_string()
    };
    let new_string = from_rust_string(env, new_path_str);
    autorelease(env, new_string)
}

- (id)stringByStandardizingPath {
    let expanded: id = msg![env; this stringByExpandingTildeInPath];
    let path = to_rust_string(env, expanded);
    assert!(!path.starts_with("/private"));
    assert!(!path.starts_with("/var/automount"));
    assert!(!path.contains("//"));
    assert!(!path.contains("/./"));
    let path = path_algorithms::trim_trailing_slashes(&path);
    let new_path_str = if path.starts_with('/') {
        assert!(!path.starts_with("/.."));
        let resolved = fs::resolve_path(GuestPath::new(path), None);
        let new_path = format!("/{}", resolved.join("/"));
        assert!(!new_path.contains(".."));
        new_path
    } else {
        String::from(path)
    };
    let new_string = from_rust_string(env, new_path_str);
    autorelease(env, new_string)
}

- (id)stringsByAppendingPaths:(id)paths {
    let count: NSUInteger = msg![env; paths count];
    let mut_arr: id = msg_class![env; NSMutableArray new];
    for i in 0..count {
        let path: id = msg![env; paths objectAtIndex:i];
        let new: id = msg![env; this stringByAppendingPathComponent:path];
        () = msg![env; mut_arr addObject:new];
    }
    let arr = msg![env; mut_arr copy];
    release(env, mut_arr);
    autorelease(env, arr)
}

- (CGSize)sizeWithFont:(id)font {
    let text = to_rust_string(env, this);
    ui_font::size_with_font(env, font, &text, None)
}

- (CGSize)sizeWithFont:(id)font forWidth:(CGFloat)width lineBreakMode:(UILineBreakMode)line_break_mode {
    let text = to_rust_string(env, this);
    let size = CGSize { width, height: 99999.0 };
    ui_font::size_with_font(env, font, &text, Some((size, line_break_mode)))
}

- (CGSize)sizeWithFont:(id)font constrainedToSize:(CGSize)size {
    msg![env; this sizeWithFont:font constrainedToSize:size lineBreakMode:UILineBreakModeWordWrap]
}

- (CGSize)sizeWithFont:(id)font constrainedToSize:(CGSize)size lineBreakMode:(UILineBreakMode)line_break_mode {
    let text = to_rust_string(env, this);
    ui_font::size_with_font(env, font, &text, Some((size, line_break_mode)))
}

- (CGSize)drawAtPoint:(CGPoint)point withFont:(id)font {
    let text = to_rust_string(env, this);
    ui_font::draw_at_point(env, font, &text, point, None)
}

- (CGSize)drawAtPoint:(CGPoint)point forWidth:(CGFloat)width withFont:(id)font lineBreakMode:(UILineBreakMode)line_break_mode {
    let text = to_rust_string(env, this);
    ui_font::draw_at_point(env, font, &text, point, Some((width, line_break_mode)))
}

- (CGSize)drawAtPoint:(CGPoint)point
            forWidth:(CGFloat)width
             withFont:(id)font
             fontSize:(CGFloat)font_size
        lineBreakMode:(UILineBreakMode)line_break_mode
   baselineAdjustment:(NSInteger)baseline_adjustment {
    // Apple's UIStringDrawing.h: deprecated in iOS 7 but valid for the
    // iPhone OS 2.x / 3.x applications touchHLE targets. The method draws
    // the receiver into the current graphics context starting at `point`,
    // bounded to `width`, with the font rescaled toward `fontSize` (never
    // larger than the supplied font's pointSize) and using the given
    // `lineBreakMode` / `baselineAdjustment` heuristics.
    //
    // Our `ui_font::draw_at_point` already handles the constrain-to-width
    // path, so we just derive a sized copy of the font via
    // `-[UIFont fontWithSize:]` (matching what UILabel does internally) and
    // forward. The baselineAdjustment values
    // (UIBaselineAdjustmentAlignBaselines=0, AlignCenters=1, None=2) shift
    // the rendered baseline within the line box; since our renderer always
    // pins to the line's baseline we honor the dominant case (0) directly
    // and log the other two for visibility instead of silently misrendering.
    if baseline_adjustment != 0 {
        log_dbg!(
            "-[NSString drawAtPoint:forWidth:withFont:fontSize:\
             lineBreakMode:baselineAdjustment:]: baseline adjustment {} \
             not yet differentiated from default (0); rendering with \
             baseline alignment.",
            baseline_adjustment,
        );
    }
    let scaled_font: id = msg![env; font fontWithSize:font_size];
    let text = to_rust_string(env, this);
    ui_font::draw_at_point(env, scaled_font, &text, point, Some((width, line_break_mode)))
}

- (CGSize)drawInRect:(CGRect)rect withFont:(id)font {
    msg![env; this drawInRect:rect withFont:font lineBreakMode:UILineBreakModeWordWrap alignment:UITextAlignmentLeft]
}

- (CGSize)drawInRect:(CGRect)rect withFont:(id)font lineBreakMode:(UILineBreakMode)line_break_mode {
    msg![env; this drawInRect:rect withFont:font lineBreakMode:line_break_mode alignment:UITextAlignmentLeft]
}

- (CGSize)drawInRect:(CGRect)rect withFont:(id)font lineBreakMode:(UILineBreakMode)line_break_mode alignment:(UITextAlignment)align {
    let text = to_rust_string(env, this);
    ui_font::draw_in_rect(env, font, &text, rect, line_break_mode, align)
}

- (bool)writeToFile:(id)path atomically:(bool)use_aux_file {
    let encoding: NSStringEncoding = msg_class![env; NSString defaultCStringEncoding];
    let error: MutPtr<id> = Ptr::null();
    msg![env; this writeToFile:path atomically:use_aux_file encoding:encoding error:error]
}

- (bool)writeToFile:(id)path atomically:(bool)use_aux_file encoding:(NSStringEncoding)encoding error:(MutPtr<id>)error {
    let string = to_rust_string(env, this);
    let bytes: Vec<u8> = match encoding {
        NSUTF16StringEncoding | NSUTF16LittleEndianStringEncoding => string.encode_utf16().flat_map(u16::to_le_bytes).collect(),
        NSUTF16BigEndianStringEncoding => string.encode_utf16().flat_map(u16::to_be_bytes).collect(),
        NSUTF32LittleEndianStringEncoding => string.chars().flat_map(|c| (c as u32).to_le_bytes()).collect(),
        NSUTF32BigEndianStringEncoding | NSUTF32StringEncoding => string.chars().flat_map(|c| (c as u32).to_be_bytes()).collect(),
        _ => string.as_bytes().to_vec(),
    };
    let length: NSUInteger = bytes.len().try_into().unwrap();
    let buf_ptr: MutPtr<u8> = env.mem.alloc(length as u32).cast();
    env.mem.bytes_at_mut(buf_ptr, length as u32).copy_from_slice(&bytes);
    let data: id = msg_class![env; NSData dataWithBytesNoCopy:(buf_ptr.cast_void()) length:length];
    let success: bool = msg![env; data writeToFile:path atomically:use_aux_file];
    success
}

- (f32)floatValue { float_value_common(env, this) }

- (f64)doubleValue { float_value_common(env, this) }

- (NSInteger)integerValue { msg![env; this intValue] }

- (i32)intValue {
    let st = to_rust_string(env, this);
    let st = st.trim_start_matches(|c: char| c.is_ascii_whitespace());
    let (sign, rest) = match st.strip_prefix('-') {
        Some(r) => (-1i64, r),
        None    => (1i64, st.strip_prefix('+').unwrap_or(st)),
    };
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    let magnitude: i64 = digits.parse().unwrap_or(0);
    (sign * magnitude).clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

- (id)lowercaseString {
    let str = to_rust_string(env, this).to_lowercase();
    let res = from_rust_string(env, str);
    autorelease(env, res)
}

- (id)uppercaseString {
    let str = to_rust_string(env, this).to_uppercase();
    let res = from_rust_string(env, str);
    autorelease(env, res)
}

- (id)capitalizedString {
    // Per Apple docs: "first character of each word changed to its
    // corresponding uppercase value, and all remaining characters set to
    // their corresponding lowercase values." Words are delimited by
    // whitespace; we treat ASCII whitespace as the delimiter set, matching
    // Cocoa's `[NSCharacterSet whitespaceAndNewlineCharacterSet]`.
    let src = to_rust_string(env, this);
    let mut out = String::with_capacity(src.len());
    let mut start_of_word = true;
    for c in src.chars() {
        if c.is_whitespace() {
            out.push(c);
            start_of_word = true;
        } else if start_of_word {
            for u in c.to_uppercase() {
                out.push(u);
            }
            start_of_word = false;
        } else {
            for u in c.to_lowercase() {
                out.push(u);
            }
        }
    }
    let res = from_rust_string(env, out);
    autorelease(env, res)
}

// MARK: - Property list parsing

- (id)propertyListFromStringsFileFormat {
    // Parses the .strings file format: "key" = "value"; pairs separated by
    // newlines, with optional C-style /* ... */ comments.
    // Returns an NSDictionary with string keys and string values.
    let src = to_rust_string(env, this);
    let dict: id = msg_class![env; NSMutableDictionary dictionary];

    let chars: Vec<char> = src.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        // Skip whitespace and newlines
        if chars[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }

        // Skip C-style comments /* ... */
        if i + 1 < len && chars[i] == '/' && chars[i + 1] == '*' {
            i += 2;
            while i + 1 < len && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            if i + 1 < len {
                i += 2; // skip */
            }
            continue;
        }

        // Skip single-line comments //
        if i + 1 < len && chars[i] == '/' && chars[i + 1] == '/' {
            i += 2;
            while i < len && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }

        // Parse a key — either a quoted string or an unquoted token
        let key = if chars[i] == '"' {
            i += 1; // skip opening quote
            let mut s = String::new();
            while i < len && chars[i] != '"' {
                if chars[i] == '\\' && i + 1 < len {
                    i += 1;
                    match chars[i] {
                        'n' => s.push('\n'),
                        't' => s.push('\t'),
                        'r' => s.push('\r'),
                        '\\' => s.push('\\'),
                        '"' => s.push('"'),
                        'U' | 'u' => {
                            // \Uxxxx unicode escape
                            i += 1;
                            let mut hex = String::new();
                            while i < len && hex.len() < 4 && chars[i].is_ascii_hexdigit() {
                                hex.push(chars[i]);
                                i += 1;
                            }
                            if let Ok(code) = u32::from_str_radix(&hex, 16) {
                                if let Some(c) = char::from_u32(code) {
                                    s.push(c);
                                }
                            }
                            continue; // i already advanced past hex digits
                        }
                        other => {
                            s.push('\\');
                            s.push(other);
                        }
                    }
                } else {
                    s.push(chars[i]);
                }
                i += 1;
            }
            if i < len { i += 1; } // skip closing quote
            s
        } else {
            // Unquoted key — read until = or whitespace
            let mut s = String::new();
            while i < len && chars[i] != '=' && !chars[i].is_ascii_whitespace() && chars[i] != ';' {
                s.push(chars[i]);
                i += 1;
            }
            s
        };

        if key.is_empty() {
            i += 1;
            continue;
        }

        // Skip whitespace
        while i < len && chars[i].is_ascii_whitespace() {
            i += 1;
        }

        // Expect '='
        if i < len && chars[i] == '=' {
            i += 1;
        } else {
            // Malformed — skip to next semicolon or newline
            while i < len && chars[i] != ';' && chars[i] != '\n' {
                i += 1;
            }
            if i < len { i += 1; }
            continue;
        }

        // Skip whitespace
        while i < len && chars[i].is_ascii_whitespace() {
            i += 1;
        }

        // Parse the value — either quoted or unquoted
        let value = if i < len && chars[i] == '"' {
            i += 1; // skip opening quote
            let mut s = String::new();
            while i < len && chars[i] != '"' {
                if chars[i] == '\\' && i + 1 < len {
                    i += 1;
                    match chars[i] {
                        'n' => s.push('\n'),
                        't' => s.push('\t'),
                        'r' => s.push('\r'),
                        '\\' => s.push('\\'),
                        '"' => s.push('"'),
                        'U' | 'u' => {
                            i += 1;
                            let mut hex = String::new();
                            while i < len && hex.len() < 4 && chars[i].is_ascii_hexdigit() {
                                hex.push(chars[i]);
                                i += 1;
                            }
                            if let Ok(code) = u32::from_str_radix(&hex, 16) {
                                if let Some(c) = char::from_u32(code) {
                                    s.push(c);
                                }
                            }
                            continue;
                        }
                        other => {
                            s.push('\\');
                            s.push(other);
                        }
                    }
                } else {
                    s.push(chars[i]);
                }
                i += 1;
            }
            if i < len { i += 1; } // skip closing quote
            s
        } else {
            // Unquoted value — read until ; or newline
            let mut s = String::new();
            while i < len && chars[i] != ';' && chars[i] != '\n' {
                s.push(chars[i]);
                i += 1;
            }
            s.trim_end().to_string()
        };

        // Skip whitespace and semicolon
        while i < len && (chars[i].is_ascii_whitespace() || chars[i] == ';') {
            i += 1;
        }

        // Insert key-value pair into dictionary
        let key_ns = from_rust_string(env, key);
        let val_ns = from_rust_string(env, value);
        let _: () = msg![env; dict setObject:val_ns forKey:key_ns];
    }

    dict
}

@end

@implementation NSMutableString: NSString

+ (id)allocWithZone:(NSZonePtr)zone {
    assert!(this == env.objc.get_known_class("NSMutableString", &mut env.mem));
    msg_class![env; _touchHLE_NSMutableString allocWithZone:zone]
}

+ (bool)supportsSecureCoding { true }

+ (id)stringWithCapacity:(NSUInteger)capacity {
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithCapacity:capacity];
    autorelease(env, new)
}

- (id)copyWithZone:(NSZonePtr)_zone {
    let new: id = msg_class![env; NSString alloc];
    msg![env; new initWithString:this]
}

- (id)mergeWithPrevious:(id)previous {
    println!(
        "NSMutableString mergeWithPrevious: previous={:?} — returning self",
        previous
    );
    this
}

- (())appendString:(id)a_string {
    let new: id = msg![env; this stringByAppendingString:a_string];
    () = msg![env; this setString:new];
}

- (())insertString:(id)a_string atIndex:(NSUInteger)loc {
    let left: id = msg![env; this substringToIndex:loc];
    let right: id = msg![env; this substringFromIndex:loc];
    let mid: id = msg![env; left stringByAppendingString:a_string];
    let res: id = msg![env; mid stringByAppendingString:right];
    () = msg![env; this setString:res];
}

- (())replaceCharactersInRange:(NSRange)range withString:(id)a_string {
    let loc = range.location;
    let len = range.length;
    let left: id = msg![env; this substringToIndex:loc];
    let right: id = msg![env; this substringFromIndex:(loc + len)];
    let mid: id = msg![env; left stringByAppendingString:a_string];
    let res: id = msg![env; mid stringByAppendingString:right];
    () = msg![env; this setString:res];
}

- (())deleteCharactersInRange:(NSRange)range {
    let location = range.location;
    let length = range.length;
    let left: id = if location == 0 { get_static_str(env, "") } else {
        let left_range = NSRange { location: 0, length: location };
        msg![env; this substringWithRange:left_range]
    };
    let idx_after_removal = location + length;
    let lenght_str: NSUInteger = msg![env; this length];
    let right: id = if idx_after_removal == lenght_str { get_static_str(env, "") } else {
        let right_range = NSRange { location: idx_after_removal, length: lenght_str - idx_after_removal };
        msg![env; this substringWithRange:right_range]
    };
    let res: id = msg![env; left stringByAppendingString:right];
    () = msg![env; this setString:res];
}

- (())setString:(id)a_string {
    // Убираем assert_ne!(a_string, nil);
    if a_string == nil {
        log!("Warning: [NSMutableString setString:nil] called. This would throw NSInvalidArgumentException on iOS. Ignoring.");
        return;
    }
    let length: NSUInteger = msg![env; this length];
    let range = NSRange { location: 0, length };
    () = msg![env; this replaceCharactersInRange:range withString:a_string];
}

- (())appendFormat:(id)format, ...args {
    // Apple raises NSInvalidArgumentException when `format` is nil; mirror the
    // lenient behaviour used elsewhere and no-op to keep the guest alive.
    if format == nil {
        log!("Warning: [NSMutableString appendFormat:nil] called. This would throw NSInvalidArgumentException on iOS. Ignoring.");
        return;
    }
    let formatted = with_format(env, format, args.start());
    let ns = from_rust_string(env, formatted);
    () = msg![env; this appendString:ns];
    release(env, ns);
}

- (NSUInteger)replaceOccurrencesOfString:(id)target
                              withString:(id)replacement
                                 options:(NSStringCompareOptions)options
                                   range:(NSRange)search_range {
    if target == nil || replacement == nil {
        return 0;
    }
    let target_len: NSUInteger = msg![env; target length];
    if target_len == 0 {
        return 0;
    }
    let replacement_len: NSUInteger = msg![env; replacement length];
    let mut count: NSUInteger = 0;
    let mut pos = search_range.location;
    let mut end = search_range.location + search_range.length;

    loop {
        if pos > end || end.saturating_sub(pos) < target_len {
            break;
        }
        let remaining = NSRange { location: pos, length: end - pos };
        let found: NSRange = msg![env; this rangeOfString:target
                                                  options:options
                                                    range:remaining];
        if found.location == NSNotFound as NSUInteger {
            break;
        }
        let found_loc = found.location;
        () = msg![env; this replaceCharactersInRange:found withString:replacement];
        count += 1;
        pos = found_loc + replacement_len;
        // Adjust end for the length difference
        end = end - target_len + replacement_len;
    }
    count
}

@end

@implementation _touchHLE_NSString: NSString

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(StringHostObject::Utf8(Cow::Borrowed("")));
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

+ (bool)supportsSecureCoding { true }

- (id)initWithCoder:(id)coder {
    let class: Class = msg![env; coder class];
    let nib_archive_class: Class = msg_class![env; _touchHLE_NIBArchiveDecoder class];
    let new_str = if env.objc.class_is_subclass_of(class, nib_archive_class) {
        _nib_archive_decoder::decode_current_string(env, coder)
    } else {
        println!("Warning: _touchHLE_NSString initWithCoder: unsupported coder class, returning empty string");
        get_static_str(env, "")
    };
    release(env, this);
    new_str
}

- (id)initWithData:(id)data encoding:(NSStringEncoding)encoding {
    if data == nil {
        release(env, this);
        return nil;
    }
    let bytes: ConstVoidPtr = msg![env; data bytes];
    if bytes.is_null() {
        release(env, this);
        return nil;
    }
    let bytes_u8: ConstPtr<u8> = bytes.cast();
    let length: NSUInteger = msg![env; data length];
    let new = msg![env; this initWithBytes:bytes_u8 length:length encoding:encoding];
    println!("initWithData:encoding: {}", to_rust_string(env, new));
    new
}

- (id)initWithFormat:(id)format, ...args {
    init_with_format_inner(env, this, format, args.start())
}

- (id)initWithFormat:(id)format arguments:(VaList)args {
    init_with_format_inner(env, this, format, args)
}

- (id)initWithBytes:(ConstPtr<u8>)bytes length:(NSUInteger)len encoding:(NSStringEncoding)encoding {
    if bytes.is_null() {
        release(env, this);
        return nil;
    }
    let slice = env.mem.bytes_at(bytes, len);
    let host_object = StringHostObject::decode(Cow::Borrowed(slice), encoding);
    *env.objc.borrow_mut(this) = host_object;
    this
}

- (id)initWithBytesNoCopy:(MutPtr<u8>)bytes
                   length:(NSUInteger)len
                 encoding:(NSStringEncoding)encoding
             freeWhenDone:(bool)_free {
    msg![env; this initWithBytes:(bytes.cast_const()) length:len encoding:encoding]
}

- (id)initWithCharacters:(ConstPtr<unichar>)characters length:(NSUInteger)len {
    assert!(!characters.is_null());
    let num_bytes = len * 2;
    msg![env; this initWithBytes:(characters.cast::<u8>()) length:num_bytes encoding:NSUTF16StringEncoding]
}

- (id)initWithString:(id)string {
    let mut code_units = Vec::new();
    for_each_code_unit(env, string, |_, c| code_units.push(c));
    *env.objc.borrow_mut(this) = StringHostObject::Utf16(code_units);
    this
}

- (id)initWithContentsOfFile:(id)path {
    if path == nil {
        release(env, this);
        return nil;
    }
    let path_str = to_rust_string(env, path);
    let bytes = match env.fs.read(GuestPath::new(&path_str)) {
        Ok(b) => b,
        Err(_) => {
            println!("WARNING: File not found: {}, returning nil", path_str);
            release(env, this);
            return nil;
        }
    };
    let len = bytes.len();
    let encoding = if len > 1 && (bytes[..2] == [0xFE, 0xFF] || bytes[..2] == [0xFF, 0xFE]) {
        NSUTF16StringEncoding
    } else if len > 2 && bytes[..3] == [0xEF, 0xBB, 0xBF] {
        NSUTF8StringEncoding
    } else {
        msg_class![env; NSString defaultCStringEncoding]
    };
    let host_object = StringHostObject::decode(Cow::Owned(bytes), encoding);
    *env.objc.borrow_mut(this) = host_object;
    this
}

- (id)initWithContentsOfFile:(id)path encoding:(NSStringEncoding)encoding error:(MutPtr<id>)error {
    if path == nil {
        release(env, this);
        return nil;
    }
    let path_str = to_rust_string(env, path);
    let bytes = match env.fs.read(GuestPath::new(&path_str)) {
        Ok(b) => b,
        Err(_) => {
            println!("WARNING: File not found: {}, returning nil", path_str);
            if !error.is_null() {
                env.mem.write(error, nil);
            }
            release(env, this);
            return nil;
        }
    };
    let host_object = StringHostObject::decode(Cow::Owned(bytes), encoding);
    *env.objc.borrow_mut(this) = host_object;
    this
}

- (id)initWithContentsOfFile:(id)path usedEncoding:(MutPtr<NSUInteger>)enc error:(MutPtr<id>)error {
    if path == nil {
        release(env, this);
        return nil;
    }
    let path_str = to_rust_string(env, path);
    let bytes = match env.fs.read(GuestPath::new(&path_str)) {
        Ok(b) => b,
        Err(_) => {
            println!("WARNING: File not found: {}, returning nil", path_str);
            if !error.is_null() {
                env.mem.write(error, nil);
            }
            release(env, this);
            return nil;
        }
    };
    let len = bytes.len();
    let encoding = if len > 1 && (bytes[..2] == [0xFE, 0xFF] || bytes[..2] == [0xFF, 0xFE]) {
        NSUTF16StringEncoding
    } else if len > 2 && bytes[..3] == [0xEF, 0xBB, 0xBF] {
        NSUTF8StringEncoding
    } else {
        msg_class![env; NSString defaultCStringEncoding]
    };
    if !enc.is_null() {
        env.mem.write(enc, encoding);
    }
    let host_object = StringHostObject::decode(Cow::Owned(bytes), encoding);
    *env.objc.borrow_mut(this) = host_object;
    this
}

// NSString URL-based initializers. Per Apple's Foundation docs
// (https://developer.apple.com/documentation/foundation/nsstring),
// these methods load the contents of the resource at the given URL.
// We currently only support file URLs; we extract the path and reuse
// the file-based implementation. For non-file URLs we fail gracefully
// instead of letting the message dispatcher fall through to a stub.

- (id)initWithContentsOfURL:(id)url {
    if url == nil {
        release(env, this);
        return nil;
    }
    let path: id = msg![env; url path];
    if path == nil {
        release(env, this);
        return nil;
    }
    msg![env; this initWithContentsOfFile:path]
}

- (id)initWithContentsOfURL:(id)url encoding:(NSStringEncoding)encoding error:(MutPtr<id>)error {
    if url == nil {
        if !error.is_null() {
            env.mem.write(error, nil);
        }
        release(env, this);
        return nil;
    }
    let path: id = msg![env; url path];
    if path == nil {
        if !error.is_null() {
            env.mem.write(error, nil);
        }
        release(env, this);
        return nil;
    }
    msg![env; this initWithContentsOfFile:path encoding:encoding error:error]
}

- (id)initWithContentsOfURL:(id)url usedEncoding:(MutPtr<NSUInteger>)enc error:(MutPtr<id>)error {
    if url == nil {
        if !error.is_null() {
            env.mem.write(error, nil);
        }
        release(env, this);
        return nil;
    }
    let path: id = msg![env; url path];
    if path == nil {
        if !error.is_null() {
            env.mem.write(error, nil);
        }
        release(env, this);
        return nil;
    }
    msg![env; this initWithContentsOfFile:path usedEncoding:enc error:error]
}

- (id)systemUptime {
    nil
}

- (id)tick_audio {
    nil
}

- (id)load_sound_files {
    nil
}

- (id)CGImage {
    nil
}

- (NSUInteger)lengthOfBytesUsingEncoding:(NSUInteger)encoding {
    let s = ns_string::to_rust_string(env, this);
    match encoding {
        NSUTF8StringEncoding => s.len() as NSUInteger,
        NSASCIIStringEncoding => s.bytes().filter(|b| b.is_ascii()).count() as NSUInteger,
        NSUTF16StringEncoding | NSUTF16BigEndianStringEncoding | NSUTF16LittleEndianStringEncoding => {
            s.chars().map(|c| if (c as u32) <= 0xFFFF { 2usize } else { 4 }).sum::<usize>() as NSUInteger
        }
        NSUTF32StringEncoding | NSUTF32BigEndianStringEncoding | NSUTF32LittleEndianStringEncoding => {
            (s.chars().count() * 4) as NSUInteger
        }
        NSISOLatin1StringEncoding | NSWindowsCP1252StringEncoding => {
            s.chars().filter(|c| (*c as u32) <= 0xFF).count() as NSUInteger
        }
        NSShiftJISStringEncoding => {
            let (cow, _, _) = SHIFT_JIS.encode(&s);
            cow.len() as NSUInteger
        }
        _ => {
            println!("NSString lengthOfBytesUsingEncoding: unknown encoding {}, falling back to UTF-8", encoding);
            s.len() as NSUInteger
        }
    }
}

- (NSUInteger)maximumLengthOfBytesUsingEncoding:(NSUInteger)encoding {
    let s = ns_string::to_rust_string(env, this);
    match encoding {
        NSUTF8StringEncoding => (s.chars().count() * 4) as NSUInteger,
        NSUTF16StringEncoding | NSUTF16BigEndianStringEncoding | NSUTF16LittleEndianStringEncoding => (s.chars().count() * 4) as NSUInteger,
        NSUTF32StringEncoding | NSUTF32BigEndianStringEncoding | NSUTF32LittleEndianStringEncoding => (s.chars().count() * 4) as NSUInteger,
        NSShiftJISStringEncoding => (s.chars().count() * 2) as NSUInteger,
        _ => msg![env; this lengthOfBytesUsingEncoding:encoding]
    }
}

- (id)stringByReplacingCharactersInRange:(NSRange)range withString:(id)replacement {
    let string = to_rust_string(env, this);
    let repl   = to_rust_string(env, replacement);
    let mut char_indices = string.char_indices();
    let start_byte = if range.location == 0 { 0 } else {
        char_indices.nth(range.location as usize - 1).map(|(i, c): (usize, char)| i + c.len_utf8()).unwrap_or(string.len())
    };
    let mut remaining = string[start_byte..].char_indices();
    let end_byte = if range.length == 0 { start_byte } else {
        remaining.nth(range.length as usize - 1).map(|(i, c): (usize, char)| start_byte + i + c.len_utf8()).unwrap_or(string.len())
    };
    let mut result = String::with_capacity(string.len() - (end_byte - start_byte) + repl.len());
    result.push_str(&string[..start_byte]);
    result.push_str(&repl);
    result.push_str(&string[end_byte..]);
    let ns = from_rust_string(env, result);
    autorelease(env, ns)
}

- (())encodeWithCoder:(id)coder {
    let class: Class = msg![env; coder class];
    let keyed_arch_class: Class = msg_class![env; NSKeyedArchiver class];
    if env.objc.class_is_subclass_of(class, keyed_arch_class) {
        let host = env.objc.borrow::<StringHostObject>(this);
        let rust_str = match host {
            StringHostObject::Utf8(s) => s.to_string(),
            StringHostObject::Utf16(s) => String::from_utf16_lossy(s).to_string(),
        };
        let content = from_rust_string(env, rust_str);
        let key = from_rust_string(env, "NS.string".to_string());
        () = msg![env; coder encodeObject:content forKey:key];
        release(env, content);
        release(env, key);
    } else {
        println!("Warning: _touchHLE_NSString encodeWithCoder: unsupported coder class, skipping");
    }
}

- (bool)isAbsolutePath {
    let path = to_rust_string(env, this);
    path.starts_with('/') || path.starts_with('~')
}


- (bool)boolValue {
    let string = to_rust_string(env, this);
    let string = string.trim_start_matches(|c: char| c.is_ascii_whitespace() || c == '-' || c == '+' || c == '0');
    let matching_values = "YyTt123456789";
    string.chars().next().map(|c| matching_values.contains(c)).unwrap_or(false)
}

- (id)dataUsingEncoding:(NSStringEncoding)encoding allowLossyConversion:(bool)lossy {
    data_using_encoding_lossy_inner(env, this, encoding, lossy)
}

- (id)componentsSeparatedByCharactersInSet:(id)cset {
    let string = {
        let host_object = env.objc.borrow_mut::<StringHostObject>(this);
        let (orig_string, did_convert) = host_object.convert_to_utf16_inplace();
        if did_convert { println!("[{:?} componentsSeparatedByCharactersInSet]: converted string to UTF-16", this); }
        orig_string.clone()
    };
    let substrings: Vec<&[u16]> = { string.split(|&c| msg![env; cset characterIsMember:c]).collect() };
    let substrings: Vec<id> = substrings.into_iter().map(|substr| from_u16_vec(env, substr.to_vec())).collect();
    let res = ns_array::from_vec(env, substrings);
    autorelease(env, res)
}

- (id)substringWithRange:(NSRange)range {
    let host_object = env.objc.borrow_mut::<StringHostObject>(this);
    let (orig_string, did_convert) = host_object.convert_to_utf16_inplace();
    if did_convert { println!("[{:?} substringWithRange]: converted string to UTF-16", this); }

    let start = range.location as usize;
    let end = start.saturating_add(range.length as usize);

    if start > orig_string.len() || end > orig_string.len() {
        println!("WARNING: substringWithRange: range {start}..{end} out of bounds (len {})", orig_string.len());
        let res = from_u16_vec(env, Vec::new());
        return autorelease(env, res);
    }

    let host_string = orig_string[start..end].to_vec();
    let res = from_u16_vec(env, host_string);
    autorelease(env, res)
}

- (NSRange)lineRangeForRange:(NSRange)range {
    let host_object = env.objc.borrow_mut::<StringHostObject>(this);
    let (orig_string, did_convert) = host_object.convert_to_utf16_inplace();
    if did_convert { println!("[{:?} lineRangeForRange]: converted string to UTF-16", this); }
    let (start, end, _) = line_range_helper(orig_string, range, true, true);
    NSRange { location: start, length: end - start }
}

- (())applyToValue:(id)value forKey:(id)key ofObject:(id)object {
    if object == nil {
        println!("NSString applyToValue:forKey:ofObject: — object is nil, ignored");
        return;
    }
    let effective_key: id = if key == nil { this } else { key };
    let _: () = msg![env; object setValue:value forKey:effective_key];
}

- (id)mergeWithPrevious:(id)_previous {
    this
}

- (())getLineStart:(MutPtr<NSUInteger>)start_ptr end:(MutPtr<NSUInteger>)end_ptr contentsEnd:(MutPtr<NSUInteger>)contents_end_ptr forRange:(NSRange)range {
    let host_object = env.objc.borrow_mut::<StringHostObject>(this);
    let (orig_string, did_convert) = host_object.convert_to_utf16_inplace();
    if did_convert { println!("[{:?} getLineStart]: converted string to UTF-16", this); }
    let get_start = !start_ptr.is_null();
    let get_end = !end_ptr.is_null() || !contents_end_ptr.is_null();
    let (start, end, contents_end) = line_range_helper(orig_string, range, get_start, get_end);
    if !start_ptr.is_null() { env.mem.write(start_ptr, start); }
    if !end_ptr.is_null() { env.mem.write(end_ptr, end); }
    if !contents_end_ptr.is_null() { env.mem.write(contents_end_ptr, contents_end); }
}
@end

@implementation _touchHLE_NSString_Static: _touchHLE_NSString

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(StringHostObject::Utf8(Cow::Borrowed("")));
    env.objc.alloc_static_object(this, host_object, &mut env.mem)
}

- (())layoutSubviews {}
- (id) retain { this }
- (()) release {}
- (id) autorelease { this }

@end

@implementation _touchHLE_NSString_CFConstantString_UTF8: _touchHLE_NSString_Static

- (ConstPtr<u8>)UTF8String {
    let cfstringStruct { bytes, .. } = env.mem.read(this.cast());
    bytes
}

- (id)stringByReplacingCharactersInRange:(NSRange)range withString:(id)replacement {
    let string = to_rust_string(env, this);
    let repl   = to_rust_string(env, replacement);
    let mut char_indices = string.char_indices();
    let start_byte = if range.location == 0 { 0 } else {
        char_indices.nth(range.location as usize - 1).map(|(i, c): (usize, char)| i + c.len_utf8()).unwrap_or(string.len())
    };
    let mut remaining = string[start_byte..].char_indices();
    let end_byte = if range.length == 0 { start_byte } else {
        remaining.nth(range.length as usize - 1).map(|(i, c): (usize, char)| start_byte + i + c.len_utf8()).unwrap_or(string.len())
    };
    let mut result = String::with_capacity(string.len() - (end_byte - start_byte) + repl.len());
    result.push_str(&string[..start_byte]);
    result.push_str(&repl);
    result.push_str(&string[end_byte..]);
    let ns = from_rust_string(env, result);
    autorelease(env, ns)
}

- (())encodeWithCoder:(id)coder {
    let class: Class = msg![env; coder class];
    let keyed_arch_class: Class = msg_class![env; NSKeyedArchiver class];
    if env.objc.class_is_subclass_of(class, keyed_arch_class) {
        let host = env.objc.borrow::<StringHostObject>(this);
        let rust_str = match host {
            StringHostObject::Utf8(s) => s.to_string(),
            StringHostObject::Utf16(s) => String::from_utf16_lossy(s).to_string(),
        };
        let content = from_rust_string(env, rust_str);
        let key = from_rust_string(env, "NS.string".to_string());
        () = msg![env; coder encodeObject:content forKey:key];
        release(env, content);
        release(env, key);
    } else {
        println!("Warning: _touchHLE_NSString_CFConstantString_UTF8 encodeWithCoder: unsupported coder class, skipping");
    }
}

- (())applyToValue:(id)value forKey:(id)key ofObject:(id)object {
    if object == nil {
        println!("NSString applyToValue:forKey:ofObject: — object is nil, ignored");
        return;
    }
    let effective_key: id = if key == nil { this } else { key };
    let _: () = msg![env; object setValue:value forKey:effective_key];
}

// =========================================================================
// MARK: - mergeWithPrevious
// =========================================================================

- (id)mergeWithPrevious:(id)_previous {
    this
}

@end

@implementation _touchHLE_NSString_CFConstantString_UTF16: _touchHLE_NSString_Static
@end

@implementation _touchHLE_NSMutableString: NSMutableString

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(StringHostObject::Utf8(Cow::Borrowed("")));
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

- (id)initWithCapacity:(NSUInteger)_capacity { msg![env; this init] }

- (id)initWithCoder:(id)coder {
    // NIB archives store some UI strings (placeholder text, default
    // contents of UITextField, IASK setting titles, etc.) as
    // NSMutableString instances. Without this method, NIB decoding warns
    // "does not respond to selector initWithCoder:" and the property
    // becomes nil, which on Minecraft PE shows up as empty Create World
    // text fields and a non-functional keyboard.
    let class: Class = msg![env; coder class];
    let nib_archive_class: Class = msg_class![env; _touchHLE_NIBArchiveDecoder class];
    if env.objc.class_is_subclass_of(class, nib_archive_class) {
        let decoded = _nib_archive_decoder::decode_current_string(env, coder);
        if decoded != nil {
            () = msg![env; this setString:decoded];
            release(env, decoded);
        }
    } else {
        println!("Warning: _touchHLE_NSMutableString initWithCoder: unsupported coder class, returning empty string");
    }
    this
}

- (id)initWithBytes:(ConstPtr<u8>)bytes length:(NSUInteger)len encoding:(NSStringEncoding)encoding {
    let slice = env.mem.bytes_at(bytes, len);
    let host_object = StringHostObject::decode(Cow::Borrowed(slice), encoding);
    *env.objc.borrow_mut(this) = host_object;
    this
}

- (id)initWithBytesNoCopy:(MutPtr<u8>)bytes
                   length:(NSUInteger)len
                 encoding:(NSStringEncoding)encoding
             freeWhenDone:(bool)_free {
    msg![env; this initWithBytes:(bytes.cast_const()) length:len encoding:encoding]
}

- (id)initWithFormat:(id)format, ...args {
    init_with_format_inner(env, this, format, args.start())
}

- (id)initWithFormat:(id)format arguments:(VaList)args {
    init_with_format_inner(env, this, format, args)
}

- (id)initWithString:(id)string {
    () = msg![env; this setString:string];
    this
}

- (id)dataUsingEncoding:(NSStringEncoding)encoding allowLossyConversion:(bool)lossy {
    data_using_encoding_lossy_inner(env, this, encoding, lossy)
}

- (())appendFormat:(id)format, ...args {
    // Apple raises NSInvalidArgumentException when `format` is nil; mirror the
    // lenient behaviour used elsewhere and no-op to keep the guest alive.
    if format == nil {
        log!("Warning: [NSMutableString appendFormat:nil] called. This would throw NSInvalidArgumentException on iOS. Ignoring.");
        return;
    }
    let formatted = with_format(env, format, args.start());
    let ns = from_rust_string(env, formatted);
    () = msg![env; this appendString:ns];
    release(env, ns);
}

- (())setString:(id)a_string {
    // Apple raises NSInvalidArgumentException when `a_string` is nil; the real
    // runtime never aborts the process, so log and ignore instead of asserting
    // (this previously crashed e.g. Reckless Getaway with `left != right`).
    if a_string == nil {
        log!("Warning: [NSMutableString setString:nil] called. This would throw NSInvalidArgumentException on iOS. Ignoring.");
        return;
    }
    let str = to_rust_string(env, a_string);
    let host_object = StringHostObject::Utf8(str);
    *env.objc.borrow_mut(this) = host_object;
}

- (id)substringWithRange:(NSRange)range {
    let host_object = env.objc.borrow_mut::<StringHostObject>(this);
    let (orig_string, did_convert) = host_object.convert_to_utf16_inplace();
    if did_convert { println!("[{:?} substringWithRange]: converted string to UTF-16", this); }

    let start = range.location as usize;
    let end = start.saturating_add(range.length as usize);

    if start > orig_string.len() || end > orig_string.len() {
        println!("WARNING: substringWithRange: range {start}..{end} out of bounds (len {})", orig_string.len());
        let res = from_u16_vec(env, Vec::new());
        return autorelease(env, res);
    }

    let host_string = orig_string[start..end].to_vec();
    let res = from_u16_vec(env, host_string);
    autorelease(env, res)
}

@end

};

fn init_with_format_inner(env: &mut Environment, this: id, format: id, args: VaList) -> id {
    let res = with_format(env, format, args);
    *env.objc.borrow_mut::<StringHostObject>(this) = StringHostObject::Utf8(res.into());
    this
}

fn data_using_encoding_lossy_inner(
    env: &mut Environment,
    this: id,
    encoding: NSStringEncoding,
    lossy: bool,
) -> id {
    if lossy {
        println!(
            "Warning: lossy conversion requested for '{}'",
            to_rust_string(env, this)
        );
    }
    let string = to_rust_string(env, this);

    let bytes: Vec<u8> = match encoding {
        NSUTF16LittleEndianStringEncoding => {
            string.encode_utf16().flat_map(u16::to_le_bytes).collect()
        }
        NSUTF16BigEndianStringEncoding => {
            string.encode_utf16().flat_map(u16::to_be_bytes).collect()
        }
        NSUTF16StringEncoding => string.encode_utf16().flat_map(u16::to_le_bytes).collect(),
        NSUTF32LittleEndianStringEncoding => string
            .chars()
            .flat_map(|c| (c as u32).to_le_bytes())
            .collect(),
        NSUTF32BigEndianStringEncoding | NSUTF32StringEncoding => string
            .chars()
            .flat_map(|c| (c as u32).to_be_bytes())
            .collect(),
        NSShiftJISStringEncoding => {
            let (cow, _, _) = encoding_rs::SHIFT_JIS.encode(&string);
            cow.into_owned()
        }
        NSASCIIStringEncoding | NSISOLatin1StringEncoding | NSMacOSRomanStringEncoding => {
            if lossy {
                string
                    .chars()
                    .map(|c| if (c as u32) <= 0xFF { c as u8 } else { b'?' })
                    .collect()
            } else {
                if string.chars().any(|c| (c as u32) > 0xFF) {
                    return nil;
                }
                string.chars().map(|c| c as u8).collect()
            }
        }
        NSUTF8StringEncoding | _ => string.as_bytes().to_vec(),
    };

    let length: NSUInteger = bytes.len().try_into().unwrap();
    let alloc_size = if length > 0 { length } else { 1 };
    let buf_ptr: MutPtr<u8> = env.mem.alloc(alloc_size as u32).cast();

    if length > 0 {
        env.mem
            .bytes_at_mut(buf_ptr, length as u32)
            .copy_from_slice(&bytes);
    }

    msg_class![env; NSData dataWithBytesNoCopy:(buf_ptr.cast_void()) length:length]
}

pub fn register_constant_strings(bin: &MachO, mem: &mut Mem, objc: &mut ObjC) {
    let Some(cfstrings) = bin.get_section("__cfstring") else {
        return;
    };
    assert!(cfstrings.size % guest_size_of::<cfstringStruct>() == 0);
    let base: ConstPtr<cfstringStruct> = Ptr::from_bits(cfstrings.addr);
    for i in 0..(cfstrings.size / guest_size_of::<cfstringStruct>()) {
        let cfstr_ptr = base + i;
        let cfstringStruct {
            _isa,
            flags,
            bytes,
            length,
        } = mem.read(cfstr_ptr);
        let (host_object, class_name) = if flags == 0x7C8 {
            let decoded = String::from_utf8_lossy(mem.bytes_at(bytes, length)).into_owned();
            (
                StringHostObject::Utf8(Cow::Owned(decoded)),
                "_touchHLE_NSString_CFConstantString_UTF8",
            )
        } else if flags == 0x7D0 {
            let decoded = mem
                .bytes_at(bytes, length * 2)
                .chunks(2)
                .map(|chunk| u16::from_le_bytes(chunk.try_into().unwrap()))
                .collect();
            (
                StringHostObject::Utf16(decoded),
                "_touchHLE_NSString_CFConstantString_UTF16",
            )
        } else {
            // The constant string flags field encodes the underlying encoding.
            // We support 0x7C8 (UTF-8) and 0x7D0 (UTF-16LE). Anything else is
            // a brand-new variant we have not seen in iPhoneOS 2/3 binaries;
            // skip the constant rather than panic the host. The CFString
            // contents will then look empty to the guest, which is closer to
            // how a real device behaves under unknown flag values.
            log!(
                "Warning: register_constant_strings: unknown CFTypeID flags {:#x} at {:?}; \
                 skipping constant string entry.",
                flags,
                cfstr_ptr
            );
            continue;
        };

        objc.register_static_object(cfstr_ptr.cast().cast_mut(), Box::new(host_object));
        let new_isa = objc.get_known_class(class_name, mem);
        mem.write(cfstr_ptr.cast().cast_mut(), new_isa);
    }
}

pub fn get_static_str(env: &mut Environment, from: &'static str) -> id {
    if let Some(&existing) = State::get(env).static_str_pool.get(from) {
        existing
    } else {
        let new = msg_class![env; _touchHLE_NSString_Static alloc];
        *env.objc.borrow_mut(new) = StringHostObject::Utf8(Cow::Borrowed(from));
        State::get(env).static_str_pool.insert(from, new);
        new
    }
}

pub fn from_rust_string(env: &mut Environment, from: String) -> id {
    let string: id = msg_class![env; _touchHLE_NSString alloc];
    let host_object: &mut StringHostObject = env.objc.borrow_mut(string);
    *host_object = StringHostObject::Utf8(Cow::Owned(from));
    string
}

pub fn mutable_from_rust_string(env: &mut Environment, from: String) -> id {
    let string: id = msg_class![env; _touchHLE_NSMutableString alloc];
    let host_object: &mut StringHostObject = env.objc.borrow_mut(string);
    *host_object = StringHostObject::Utf8(Cow::Owned(from));
    string
}

pub fn from_u16_vec(env: &mut Environment, from: Vec<u16>) -> id {
    let string: id = msg_class![env; _touchHLE_NSString alloc];
    let host_object: &mut StringHostObject = env.objc.borrow_mut(string);
    *host_object = StringHostObject::Utf16(from);
    string
}

pub fn to_rust_string(env: &mut Environment, string: id) -> Cow<'static, str> {
    if string == nil {
        return Cow::Borrowed("");
    }
    env.objc
        .borrow_mut::<StringHostObject>(string)
        .to_utf8()
        .unwrap()
}

/// Returns the encoding in which `string`'s underlying code units can be
/// retrieved without conversion.
///
/// This mirrors `CFStringGetFastestEncoding` and Cocoa's
/// `-[NSString fastestEncoding]`: an `NSString` stored as UTF-16 reports
/// `NSUnicodeStringEncoding`, a pure-ASCII UTF-8 string reports
/// `NSASCIIStringEncoding`, and any other UTF-8 string reports
/// `NSUTF8StringEncoding`. `nil` is treated as the empty string (ASCII).
pub fn fastest_encoding(env: &mut Environment, string: id) -> NSStringEncoding {
    if string == nil {
        return NSASCIIStringEncoding;
    }
    match env.objc.borrow::<StringHostObject>(string) {
        StringHostObject::Utf8(s) => {
            if s.is_ascii() {
                NSASCIIStringEncoding
            } else {
                NSUTF8StringEncoding
            }
        }
        StringHostObject::Utf16(_) => NSUnicodeStringEncoding,
    }
}

/// Returns the smallest encoding that can losslessly represent `string`.
///
/// This mirrors `CFStringGetSmallestEncoding` and `-[NSString smallestEncoding]`:
/// pure-ASCII content reports `NSASCIIStringEncoding`, otherwise we report
/// `NSUTF8StringEncoding` because every Unicode scalar value fits in UTF-8.
pub fn smallest_encoding(env: &mut Environment, string: id) -> NSStringEncoding {
    if string == nil {
        return NSASCIIStringEncoding;
    }
    let host = env.objc.borrow::<StringHostObject>(string);
    let is_ascii = match host {
        StringHostObject::Utf8(s) => s.is_ascii(),
        StringHostObject::Utf16(v) => v.iter().all(|&c| c <= 0x7F),
    };
    if is_ascii {
        NSASCIIStringEncoding
    } else {
        NSUTF8StringEncoding
    }
}

pub fn for_each_code_unit<F>(env: &mut Environment, string: id, mut f: F)
where
    F: FnMut(NSUInteger, u16),
{
    if string == nil {
        return;
    }
    let mut idx: NSUInteger = 0;
    env.objc
        .borrow::<StringHostObject>(string)
        .iter_code_units()
        .for_each(|c| {
            f(idx, c);
            idx += 1;
        });
}

fn is_match_at_position<F: Fn(u16, u16) -> bool>(
    env: &mut Environment,
    the_string: id,
    search_string: id,
    start: NSUInteger,
    len: NSUInteger,
    len_search: NSUInteger,
    compare_fn: F,
) -> bool {
    (0..len_search).all(|j| {
        let curr: NSUInteger = start + j;
        if curr < len {
            let a_c: u16 = msg![env; the_string characterAtIndex:curr];
            let b_c: u16 = msg![env; search_string characterAtIndex:j];
            compare_fn(a_c, b_c)
        } else {
            false
        }
    })
}

fn float_value_common<F: std::str::FromStr + Default>(env: &mut Environment, string: id) -> F {
    let st = to_rust_string(env, string);
    let st = st.trim_start();
    let mut cutoff = st.len();
    for (i, c) in st.char_indices() {
        if !c.is_ascii_digit() && c != '.' && c != '+' && c != '-' {
            cutoff = i;
            break;
        }
    }
    st[..cutoff].parse().unwrap_or(Default::default())
}

fn line_range_helper(
    string: &Utf16String,
    range: NSRange,
    get_start: bool,
    get_end: bool,
) -> (NSUInteger, NSUInteger, NSUInteger) {
    let NSRange {
        location: r_start,
        length,
    } = range;
    let r_end: usize = r_start.checked_add(length).unwrap().try_into().unwrap();
    let r_start: usize = r_start.try_into().unwrap();
    let str_len = string.len();
    assert!(r_end <= str_len, "Range out of bounds!");

    let mut start_pos: usize = 0;
    if get_start {
        start_pos = r_start;
        while start_pos > 0 {
            let c: u16 = string[start_pos - 1];
            match c {
                0x000A | 0x0085 | 0x2028 | 0x2029 => break,
                0x000D => {
                    if start_pos == r_start && start_pos < str_len {
                        let after_cr: u16 = string[start_pos];
                        if after_cr == 0x000A {
                            start_pos -= 1;
                            continue;
                        }
                    }
                    break;
                }
                _ => {}
            }
            start_pos -= 1;
        }
    }

    let mut end_pos = 0;
    let mut cend_pos = 0;
    if get_end {
        cend_pos = if length > 0 { r_end - 1 } else { r_start };
        while cend_pos < str_len {
            let c: u16 = string[cend_pos];
            match c {
                0x0085 | 0x2028 | 0x2029 => {
                    end_pos = cend_pos + 1;
                    break;
                }
                0x000A => {
                    if cend_pos > 0 && string[cend_pos - 1] == 0x000D {
                        cend_pos -= 1;
                        end_pos = cend_pos + 2;
                    } else {
                        end_pos = cend_pos + 1;
                    }
                    break;
                }
                0x000D => {
                    if cend_pos < str_len - 1 {
                        let after_cr: u16 = string[cend_pos + 1];
                        if after_cr == 0x000A {
                            end_pos = cend_pos + 2;
                            break;
                        }
                    }
                    end_pos = cend_pos + 1;
                    break;
                }
                _ => {}
            }
            cend_pos += 1;
        }
        if cend_pos == str_len {
            end_pos = cend_pos
        }
    }
    (
        start_pos.try_into().unwrap(),
        end_pos.try_into().unwrap(),
        cend_pos.try_into().unwrap(),
    )
}

#[cfg(test)]
mod ns_string_tests {
    use super::*;
    #[test]
    fn linerange_tests() {
        let range = |x, y| NSRange {
            location: x,
            length: y,
        };
        let str1: Utf16String = "abcd\nab".encode_utf16().collect();
        assert!(line_range_helper(&str1, range(5, 1), true, true) == (5, 7, 7));
        assert!(line_range_helper(&str1, range(4, 1), true, true) == (0, 5, 4));
        let str2: Utf16String = "abc\r".encode_utf16().collect();
        assert!(line_range_helper(&str2, range(4, 0), true, true) == (4, 4, 4));
        assert!(line_range_helper(&str2, range(3, 1), true, true) == (0, 4, 3));
        let str3: Utf16String = "abc\r\nab".encode_utf16().collect();
        assert!(line_range_helper(&str3, range(4, 0), true, true) == (0, 5, 3));
        assert!(line_range_helper(&str3, range(4, 1), true, true) == (0, 5, 3));
        assert!(line_range_helper(&str3, range(6, 1), true, true) == (5, 7, 7));
        assert!(line_range_helper(&str3, range(4, 2), true, true) == (0, 7, 7));
        let str4: Utf16String = "\r\n".encode_utf16().collect();
        assert!(line_range_helper(&str4, range(1, 0), true, true) == (0, 2, 0));
        assert!(line_range_helper(&str4, range(1, 1), true, true) == (0, 2, 0));
        assert!(line_range_helper(&str4, range(0, 0), true, true) == (0, 2, 0));
        let str5: Utf16String = "abcd\na\n".encode_utf16().collect();
        assert!(line_range_helper(&str5, range(6, 1), true, true) == (5, 7, 6));
        assert!(line_range_helper(&str5, range(4, 1), true, true) == (0, 5, 4));
    }
}

fn bytes_for_encoding(env: &mut Environment, str: id, encoding: NSStringEncoding) -> Vec<u8> {
    let string = to_rust_string(env, str);
    match encoding {
        NSASCIIStringEncoding
        | NSMacOSRomanStringEncoding
        | NSISOLatin1StringEncoding
        | NSNextStepLatinStringEncoding => string.as_bytes().to_vec(),
        NSUTF8StringEncoding | NSWindowsCP1252StringEncoding => string.as_bytes().to_vec(),
        NSUTF16LittleEndianStringEncoding | NSUTF16StringEncoding | NSUnicodeStringEncoding => {
            string.encode_utf16().flat_map(u16::to_le_bytes).collect()
        }
        NSUTF16BigEndianStringEncoding => {
            string.encode_utf16().flat_map(u16::to_be_bytes).collect()
        }
        NSUTF32LittleEndianStringEncoding => string
            .chars()
            .flat_map(|c| (c as u32).to_le_bytes())
            .collect(),
        NSUTF32BigEndianStringEncoding | NSUTF32StringEncoding => string
            .chars()
            .flat_map(|c| (c as u32).to_be_bytes())
            .collect(),
        NSShiftJISStringEncoding => {
            let (cow, _, _) = SHIFT_JIS.encode(&string);
            cow.into_owned()
        }
        _ => {
            log!(
                "Warning: NSString byte conversion requested with unknown encoding: {}; using UTF-8",
                encoding
            );
            string.as_bytes().to_vec()
        }
    }
}

fn bytes_for_percent_escaping(
    env: &mut Environment,
    str: id,
    encoding: NSStringEncoding,
) -> Vec<u8> {
    bytes_for_encoding(env, str, encoding)
}

fn code_units_consumed_for_bytes(
    env: &mut Environment,
    str: id,
    encoding: NSStringEncoding,
    byte_count: usize,
) -> NSUInteger {
    match encoding {
        NSUTF16LittleEndianStringEncoding
        | NSUTF16BigEndianStringEncoding
        | NSUTF16StringEncoding
        | NSUnicodeStringEncoding => (byte_count / 2) as NSUInteger,
        NSUTF32LittleEndianStringEncoding
        | NSUTF32BigEndianStringEncoding
        | NSUTF32StringEncoding => {
            let string = to_rust_string(env, str);
            let mut consumed_bytes = 0usize;
            let mut consumed_units = 0usize;
            for ch in string.chars() {
                let needed = 4usize;
                if consumed_bytes + needed > byte_count {
                    break;
                }
                consumed_bytes += needed;
                consumed_units += ch.len_utf16();
            }
            consumed_units as NSUInteger
        }
        _ => {
            let string = to_rust_string(env, str);
            let mut consumed_bytes = 0usize;
            let mut consumed_units = 0usize;
            for ch in string.chars() {
                let needed = match encoding {
                    NSShiftJISStringEncoding => {
                        let mut buf = [0u8; 4];
                        let temp = ch.encode_utf8(&mut buf);
                        let (cow, _, _) = SHIFT_JIS.encode(temp);
                        cow.len()
                    }
                    _ => ch.len_utf8(),
                };
                if consumed_bytes + needed > byte_count {
                    break;
                }
                consumed_bytes += needed;
                consumed_units += ch.len_utf16();
            }
            consumed_units as NSUInteger
        }
    }
}

pub fn get_bytes_buffer_inner(
    env: &mut Environment,
    str: id,
    buffer: MutPtr<u8>,
    buffer_size: NSUInteger,
    encoding: NSStringEncoding,
    include_null_terminator: bool,
) -> bool {
    let mut bytes = bytes_for_encoding(env, str, encoding);

    if include_null_terminator {
        match encoding {
            NSUTF16LittleEndianStringEncoding
            | NSUTF16BigEndianStringEncoding
            | NSUTF16StringEncoding
            | NSUnicodeStringEncoding => {
                bytes.push(0);
                bytes.push(0);
            }
            NSUTF32LittleEndianStringEncoding
            | NSUTF32BigEndianStringEncoding
            | NSUTF32StringEncoding => {
                bytes.push(0);
                bytes.push(0);
                bytes.push(0);
                bytes.push(0);
            }
            _ => {
                bytes.push(0);
            }
        }
    }

    let bytes_len: NSUInteger = bytes.len().try_into().unwrap();
    if buffer_size < bytes_len {
        return false;
    }

    let dest = env.mem.bytes_at_mut(buffer, buffer_size);
    dest[..bytes.len()].copy_from_slice(&bytes);

    true
}

fn string_by_replacing_occurrences_inner(
    env: &mut Environment,
    source: id,
    target: id,
    replacement: id,
    options: NSStringCompareOptions,
) -> id {
    if source == nil {
        return nil;
    }
    if target == nil || replacement == nil {
        let res = msg![env; source copy];
        return autorelease(env, res);
    }
    let mut main_iter = env
        .objc
        .borrow::<StringHostObject>(source)
        .iter_code_units();
    let target_iter = env
        .objc
        .borrow::<StringHostObject>(target)
        .iter_code_units();
    let replacement_iter = env
        .objc
        .borrow::<StringHostObject>(replacement)
        .iter_code_units();
    if target_iter.clone().next().is_none() {
        let res = msg![env; source copy];
        return autorelease(env, res);
    }
    let case_insensitive = match options {
        0 => false,
        NSCaseInsensitiveSearch => true,
        _ => {
            println!(
                "Warning: unhandled options {}, falling back to case-sensitive",
                options
            );
            false
        }
    };
    let mut result: Utf16String = Vec::new();
    loop {
        if let Some(new_main_iter) = main_iter.strip_prefix(&target_iter, case_insensitive) {
            result.extend(replacement_iter.clone());
            main_iter = new_main_iter;
        } else {
            match main_iter.next() {
                Some(cur) => result.push(cur),
                None => break,
            }
        }
    }
    let result_ns_string = msg_class![env; _touchHLE_NSString alloc];
    *env.objc.borrow_mut(result_ns_string) = StringHostObject::Utf16(result);
    autorelease(env, result_ns_string)
}

fn size_with_font_min_font_size_actual_font_size_for_width_line_break_mode(
    env: &mut Environment,
    this: id,
    font: id,
    min_font_size: CGFloat,
    actual_font_size: MutPtr<CGFloat>,
    for_width: CGFloat,
    _line_break_mode: UILineBreakMode,
) -> CGSize {
    if font == nil {
        return CGSize {
            width: 0.0,
            height: 0.0,
        };
    }
    let unconstrained_size: CGSize = msg![env; this sizeWithFont:font];
    let orig_point_size: CGFloat = msg![env; font pointSize];
    let mut final_point_size = orig_point_size;
    let mut final_size = unconstrained_size;
    if unconstrained_size.width > for_width && for_width > 0.0 {
        let scale = for_width / unconstrained_size.width;
        final_point_size = (orig_point_size * scale).max(min_font_size);
        let actual_scale = final_point_size / orig_point_size;
        final_size.width = (unconstrained_size.width * actual_scale).min(for_width);
        final_size.height = unconstrained_size.height * actual_scale;
    }
    if !actual_font_size.is_null() {
        env.mem.write(actual_font_size, final_point_size);
    }
    final_size
}

pub fn CFStringGetCharactersPtr(env: &mut Environment, the_string: id) -> ConstPtr<unichar> {
    if the_string == nil {
        return Ptr::null();
    }
    let class: Class = msg![env; the_string class];
    let constant_utf16_class = env
        .objc
        .get_known_class("_touchHLE_NSString_CFConstantString_UTF16", &mut env.mem);
    if class == constant_utf16_class {
        let cfstr: cfstringStruct = env.mem.read(the_string.cast());
        cfstr.bytes.cast()
    } else {
        Ptr::null()
    }
}
