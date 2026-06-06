/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0.
 * If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//!
//! `CFString` and `CFMutableString`.
//!
//! This is toll-free bridged to `NSString` and `NSMutableString` in
//! Apple's implementation.
//!
//! Here it is the same type.

use super::cf_allocator::{kCFAllocatorDefault, CFAllocatorRef};
use super::cf_array::CFArrayRef;
use super::cf_dictionary::CFDictionaryRef;
use super::cf_locale::CFLocaleRef;
use super::{
    kCFNotFound, CFComparisonResult, CFIndex, CFOptionFlags, CFRange, CFRelease, CFRetain,
    CFTypeRef,
};
use crate::abi::{DotDotDot, VaList};
use crate::dyld::{export_c_func, ConstantExports, FunctionExports, HostConstant};
use crate::frameworks::foundation::{ns_string, unichar, NSNotFound, NSRange, NSUInteger};
use crate::mem::{ConstPtr, GuestUSize, MutPtr, MutVoidPtr};
use crate::objc::{id, msg, msg_class, nil};
use crate::Environment;

pub type CFStringRef = super::CFTypeRef;
pub type CFMutableStringRef = CFStringRef;

// String encodings
pub type CFStringEncoding = u32;
pub const kCFStringEncodingMacRoman: CFStringEncoding = 0;
pub const kCFStringEncodingNextStepLatin: CFStringEncoding = 0x422;
pub const kCFStringEncodingASCII: CFStringEncoding = 0x600;
pub const kCFStringEncodingUTF8: CFStringEncoding = 0x8000100;
pub const kCFStringEncodingUnicode: CFStringEncoding = 0x100;
pub const kCFStringEncodingUTF16: CFStringEncoding = kCFStringEncodingUnicode;
pub const kCFStringEncodingUTF16BE: CFStringEncoding = 0x10000100;
pub const kCFStringEncodingUTF16LE: CFStringEncoding = 0x14000100;
pub const kCFStringEncodingISOLatin1: CFStringEncoding = 0x0201;
pub const kCFStringEncodingWindowsLatin1: CFStringEncoding = 0x0500;
pub const kCFStringEncodingUTF32: CFStringEncoding = 0x0c000100;
pub const kCFStringEncodingUTF32BE: CFStringEncoding = 0x18000100;
pub const kCFStringEncodingUTF32LE: CFStringEncoding = 0x1c000100;

// String normalization forms
pub type CFStringNormalizationForm = CFIndex;
pub const kCFStringNormalizationFormD: CFStringNormalizationForm = 0;
pub const kCFStringNormalizationFormKD: CFStringNormalizationForm = 1;
pub const kCFStringNormalizationFormC: CFStringNormalizationForm = 2;
pub const kCFStringNormalizationFormKC: CFStringNormalizationForm = 3;

// String comparison options (subset of NSStringCompareOptions)
pub const kCFCompareCaseInsensitive: CFOptionFlags = 1;
pub const kCFCompareBackwards: CFOptionFlags = 4;
pub const kCFCompareAnchored: CFOptionFlags = 8;
pub const kCFCompareNonliteral: CFOptionFlags = 16;
pub const kCFCompareLocalized: CFOptionFlags = 32;
pub const kCFCompareNumerically: CFOptionFlags = 64;

// Built-in string constants
pub const kCFStringTransformStripCombiningMarks: &str = "StringTransformStripCombiningMarks";
pub const kCFStringTransformToLatin: &str = "StringTransformToLatin";
pub const kCFStringTransformFullwidthHalfwidth: &str = "StringTransformFullwidthHalfwidth";
pub const kCFStringTransformLatinKatakana: &str = "StringTransformLatinKatakana";
pub const kCFStringTransformLatinHiragana: &str = "StringTransformLatinHiragana";
pub const kCFStringTransformHiraganaKatakana: &str = "StringTransformHiraganaKatakana";
pub const kCFStringTransformMandarinLatin: &str = "StringTransformMandarinLatin";
pub const kCFStringTransformLatinHangul: &str = "StringTransformLatinHangul";
pub const kCFStringTransformLatinArabic: &str = "StringTransformLatinArabic";
pub const kCFStringTransformLatinHebrew: &str = "StringTransformLatinHebrew";
pub const kCFStringTransformLatinThai: &str = "StringTransformLatinThai";
pub const kCFStringTransformLatinCyrillic: &str = "StringTransformLatinCyrillic";
pub const kCFStringTransformLatinGreek: &str = "StringTransformLatinGreek";
pub const kCFStringTransformToXMLHex: &str = "StringTransformToXMLHex";
pub const kCFStringTransformToUnicodeName: &str = "StringTransformToUnicodeName";
pub const kCFStringTransformStripDiacritics: &str = "StringTransformStripDiacritics";

type ConstStr255Param = ConstPtr<u8>;
type StringPtr = MutPtr<u8>;

// MARK: - Helper functions

fn validate_allocator(env: &mut Environment, allocator: CFAllocatorRef) -> bool {
    allocator == kCFAllocatorDefault
        || allocator.is_null()
        || env.mem.read(allocator).is_system_default()
}

fn safe_cf_range_to_ns_range(range: CFRange) -> Option<NSRange> {
    if range.location < 0 || range.length < 0 {
        return None;
    }
    Some(NSRange {
        location: range.location.try_into().ok()?,
        length: range.length.try_into().ok()?,
    })
}

// MARK: - Retain / Release

fn CFStringRetain(env: &mut Environment, str: CFStringRef) -> CFStringRef {
    if !str.is_null() {
        CFRetain(env, str)
    } else {
        str
    }
}

fn CFStringRelease(env: &mut Environment, str: CFStringRef) {
    if !str.is_null() {
        CFRelease(env, str);
    }
}

// MARK: - Encoding conversion

pub fn CFStringConvertEncodingToNSStringEncoding(
    _env: &mut Environment,
    encoding: CFStringEncoding,
) -> ns_string::NSStringEncoding {
    match encoding {
        kCFStringEncodingMacRoman => ns_string::NSMacOSRomanStringEncoding,
        kCFStringEncodingASCII => ns_string::NSASCIIStringEncoding,
        kCFStringEncodingUTF8 => ns_string::NSUTF8StringEncoding,
        kCFStringEncodingUTF16 | kCFStringEncodingUnicode => ns_string::NSUTF16StringEncoding,
        kCFStringEncodingUTF16BE => ns_string::NSUTF16BigEndianStringEncoding,
        kCFStringEncodingUTF16LE => ns_string::NSUTF16LittleEndianStringEncoding,
        kCFStringEncodingISOLatin1 => ns_string::NSISOLatin1StringEncoding,
        kCFStringEncodingNextStepLatin => ns_string::NSNextStepLatinStringEncoding,
        kCFStringEncodingWindowsLatin1 => ns_string::NSWindowsCP1252StringEncoding,
        kCFStringEncodingUTF32 => ns_string::NSUTF32StringEncoding,
        kCFStringEncodingUTF32BE => ns_string::NSUTF32BigEndianStringEncoding,
        kCFStringEncodingUTF32LE => ns_string::NSUTF32LittleEndianStringEncoding,
        _ => {
            log!(
                "Warning: Unhandled CFStringEncoding {:#x}, defaulting to UTF-8",
                encoding
            );
            ns_string::NSUTF8StringEncoding
        }
    }
}

fn CFStringConvertNSStringEncodingToEncoding(
    _env: &mut Environment,
    encoding: ns_string::NSStringEncoding,
) -> CFStringEncoding {
    match encoding {
        ns_string::NSMacOSRomanStringEncoding => kCFStringEncodingMacRoman,
        ns_string::NSASCIIStringEncoding => kCFStringEncodingASCII,
        ns_string::NSUTF8StringEncoding => kCFStringEncodingUTF8,
        ns_string::NSUTF16StringEncoding => kCFStringEncodingUTF16,
        ns_string::NSUTF16BigEndianStringEncoding => kCFStringEncodingUTF16BE,
        ns_string::NSUTF16LittleEndianStringEncoding => kCFStringEncodingUTF16LE,
        ns_string::NSISOLatin1StringEncoding => kCFStringEncodingISOLatin1,
        ns_string::NSNextStepLatinStringEncoding => kCFStringEncodingNextStepLatin,
        ns_string::NSWindowsCP1252StringEncoding => kCFStringEncodingWindowsLatin1,
        ns_string::NSUTF32StringEncoding => kCFStringEncodingUTF32,
        ns_string::NSUTF32BigEndianStringEncoding => kCFStringEncodingUTF32BE,
        ns_string::NSUTF32LittleEndianStringEncoding => kCFStringEncodingUTF32LE,
        _ => {
            log!(
                "Warning: Unhandled NSStringEncoding {:#x}, defaulting to UTF-8",
                encoding
            );
            kCFStringEncodingUTF8
        }
    }
}

fn CFStringIsEncodingAvailable(_env: &mut Environment, encoding: CFStringEncoding) -> bool {
    // Most common encodings are available
    matches!(
        encoding,
        kCFStringEncodingMacRoman
            | kCFStringEncodingASCII
            | kCFStringEncodingUTF8
            | kCFStringEncodingUTF16
            | kCFStringEncodingUTF16BE
            | kCFStringEncodingUTF16LE
            | kCFStringEncodingISOLatin1
            | kCFStringEncodingWindowsLatin1
            | kCFStringEncodingUTF32
            | kCFStringEncodingUTF32BE
            | kCFStringEncodingUTF32LE
            | kCFStringEncodingNextStepLatin
    )
}

fn CFStringGetSystemEncoding(_env: &mut Environment) -> CFStringEncoding {
    // Default system encoding
    kCFStringEncodingUTF8
}

/// Returns the encoding in which the string is most efficiently stored.
///
/// CoreFoundation's `CFString` may hold its bytes as ASCII, UTF-16, or a
/// platform encoding; this getter lets callers grab those bytes without a
/// conversion. We bridge into `NSString`'s host object via
/// [`ns_string::fastest_encoding`] and translate the result back to a
/// `CFStringEncoding`.
fn CFStringGetFastestEncoding(env: &mut Environment, the_string: CFStringRef) -> CFStringEncoding {
    if the_string.is_null() {
        // Apple's real implementation also tolerates NULL by returning
        // `kCFStringEncodingASCII` rather than crashing.
        return kCFStringEncodingASCII;
    }
    let ns_enc = ns_string::fastest_encoding(env, the_string);
    CFStringConvertNSStringEncodingToEncoding(env, ns_enc)
}

/// Returns the smallest encoding that can losslessly represent the string.
///
/// Mirrors `CFStringGetSmallestEncoding`. ASCII-only content reports
/// `kCFStringEncodingASCII`; anything with non-ASCII code points reports
/// `kCFStringEncodingUTF8` (UTF-8 covers the full Unicode range).
fn CFStringGetSmallestEncoding(env: &mut Environment, the_string: CFStringRef) -> CFStringEncoding {
    if the_string.is_null() {
        return kCFStringEncodingASCII;
    }
    let ns_enc = ns_string::smallest_encoding(env, the_string);
    CFStringConvertNSStringEncodingToEncoding(env, ns_enc)
}

fn CFStringGetMostCompatibleMacStringEncoding(
    _env: &mut Environment,
    encoding: CFStringEncoding,
) -> CFStringEncoding {
    // Return closest Mac encoding
    match encoding {
        kCFStringEncodingUTF8
        | kCFStringEncodingUTF16
        | kCFStringEncodingUTF16BE
        | kCFStringEncodingUTF16LE => kCFStringEncodingUTF8,
        kCFStringEncodingASCII => kCFStringEncodingASCII,
        _ => kCFStringEncodingMacRoman,
    }
}

// MARK: - Immutable constructors

fn CFStringCreateCopy(
    env: &mut Environment,
    allocator: CFAllocatorRef,
    the_string: CFStringRef,
) -> CFStringRef {
    if !validate_allocator(env, allocator) {
        return nil;
    }

    if the_string.is_null() {
        return nil;
    }

    msg![env; the_string copy]
}

fn CFStringCreateWithBytes(
    env: &mut Environment,
    allocator: CFAllocatorRef,
    bytes: ConstPtr<u8>,
    num_bytes: CFIndex,
    encoding: CFStringEncoding,
    is_external: bool,
) -> CFStringRef {
    if !validate_allocator(env, allocator) {
        return nil;
    }

    if bytes.is_null() || num_bytes < 0 {
        return nil;
    }

    if num_bytes == 0 {
        return msg_class![env;
        NSString string];
    }

    // When is_external is true, the byte stream is in "external representation"
    // format. For Unicode encodings this means the data may start with a BOM
    // (Byte Order Mark) that indicates byte order. We detect the BOM, determine
    // the correct encoding, and strip the BOM before passing data to NSString.
    let (actual_bytes, actual_len, actual_encoding) = if is_external {
        let len = num_bytes as GuestUSize;
        let raw = env.mem.bytes_at(bytes, len);

        match encoding {
            kCFStringEncodingUTF16 | kCFStringEncodingUnicode => {
                // Check for UTF-16 BOM
                if len >= 2 && raw[0] == 0xFF && raw[1] == 0xFE {
                    // Little-endian BOM — skip 2 bytes
                    let new_ptr = ConstPtr::<u8>::from_bits(bytes.to_bits() + 2);
                    (new_ptr, (len - 2) as CFIndex, kCFStringEncodingUTF16LE)
                } else if len >= 2 && raw[0] == 0xFE && raw[1] == 0xFF {
                    // Big-endian BOM — skip 2 bytes
                    let new_ptr = ConstPtr::<u8>::from_bits(bytes.to_bits() + 2);
                    (new_ptr, (len - 2) as CFIndex, kCFStringEncodingUTF16BE)
                } else {
                    // No BOM — assume big-endian (per Apple convention)
                    (bytes, num_bytes, kCFStringEncodingUTF16BE)
                }
            }
            kCFStringEncodingUTF32 => {
                // Check for UTF-32 BOM
                if len >= 4 && raw[0] == 0xFF && raw[1] == 0xFE && raw[2] == 0x00 && raw[3] == 0x00 {
                    // Little-endian BOM — skip 4 bytes
                    let new_ptr = ConstPtr::<u8>::from_bits(bytes.to_bits() + 4);
                    (new_ptr, (len - 4) as CFIndex, kCFStringEncodingUTF32LE)
                } else if len >= 4 && raw[0] == 0x00 && raw[1] == 0x00 && raw[2] == 0xFE && raw[3] == 0xFF {
                    // Big-endian BOM — skip 4 bytes
                    let new_ptr = ConstPtr::<u8>::from_bits(bytes.to_bits() + 4);
                    (new_ptr, (len - 4) as CFIndex, kCFStringEncodingUTF32BE)
                } else {
                    // No BOM — assume big-endian
                    (bytes, num_bytes, kCFStringEncodingUTF32BE)
                }
            }
            kCFStringEncodingUTF8 => {
                // Check for UTF-8 BOM (EF BB BF)
                if len >= 3 && raw[0] == 0xEF && raw[1] == 0xBB && raw[2] == 0xBF {
                    let new_ptr = ConstPtr::<u8>::from_bits(bytes.to_bits() + 3);
                    (new_ptr, (len - 3) as CFIndex, kCFStringEncodingUTF8)
                } else {
                    (bytes, num_bytes, kCFStringEncodingUTF8)
                }
            }
            _ => {
                // For other encodings, is_external has no special effect
                (bytes, num_bytes, encoding)
            }
        }
    } else {
        (bytes, num_bytes, encoding)
    };

    let ns_encoding = CFStringConvertEncodingToNSStringEncoding(env, actual_encoding);
    let length: NSUInteger = actual_len.try_into().unwrap_or(0);
    let ns_string: id = msg_class![env; NSString alloc];
    msg![env; ns_string initWithBytes:actual_bytes length:length encoding:ns_encoding]
}

fn CFStringCreateWithBytesNoCopy(
    env: &mut Environment,
    allocator: CFAllocatorRef,
    bytes: ConstPtr<u8>,
    num_bytes: CFIndex,
    encoding: CFStringEncoding,
    is_external: bool,
    contents_deallocator: CFAllocatorRef,
) -> CFStringRef {
    if !validate_allocator(env, allocator) {
        return nil;
    }

    // For simplicity, we copy the bytes anyway
    // In a real implementation, this would avoid copying
    let _ = contents_deallocator;
    CFStringCreateWithBytes(env, allocator, bytes, num_bytes, encoding, is_external)
}

fn CFStringCreateWithCString(
    env: &mut Environment,
    allocator: CFAllocatorRef,
    c_string: ConstPtr<u8>,
    encoding: CFStringEncoding,
) -> CFStringRef {
    if !validate_allocator(env, allocator) {
        return nil;
    }

    if c_string.is_null() {
        return nil;
    }

    let encoding = CFStringConvertEncodingToNSStringEncoding(env, encoding);
    let ns_string: id = msg_class![env; NSString alloc];
    msg![env; ns_string initWithCString:c_string encoding:encoding]
}

fn CFStringCreateWithCStringNoCopy(
    env: &mut Environment,
    allocator: CFAllocatorRef,
    c_string: ConstPtr<u8>,
    encoding: CFStringEncoding,
    contents_deallocator: CFAllocatorRef,
) -> CFStringRef {
    if !validate_allocator(env, allocator) {
        return nil;
    }

    // For simplicity, we copy anyway
    let _ = contents_deallocator;
    CFStringCreateWithCString(env, allocator, c_string, encoding)
}

fn CFStringCreateWithCharacters(
    env: &mut Environment,
    allocator: CFAllocatorRef,
    chars: ConstPtr<unichar>,
    num_chars: CFIndex,
) -> CFStringRef {
    if !validate_allocator(env, allocator) {
        return nil;
    }

    if chars.is_null() || num_chars < 0 {
        return nil;
    }

    if num_chars == 0 {
        return msg_class![env;
        NSString string];
    }

    let length: NSUInteger = num_chars.try_into().unwrap_or(0);
    let ns_string: id = msg_class![env;
    NSString alloc];
    msg![env; ns_string initWithCharacters:chars length:length]
}

fn CFStringCreateWithCharactersNoCopy(
    env: &mut Environment,
    allocator: CFAllocatorRef,
    chars: ConstPtr<unichar>,
    num_chars: CFIndex,
    contents_deallocator: CFAllocatorRef,
) -> CFStringRef {
    if !validate_allocator(env, allocator) {
        return nil;
    }

    // For simplicity, we copy anyway
    let _ = contents_deallocator;
    CFStringCreateWithCharacters(env, allocator, chars, num_chars)
}

fn CFStringCreateWithFormat(
    env: &mut Environment,
    allocator: CFAllocatorRef,
    format_options: CFDictionaryRef,
    format: CFStringRef,
    args: DotDotDot,
) -> CFStringRef {
    CFStringCreateWithFormatAndArguments(env, allocator, format_options, format, args.start())
}

fn CFStringCreateWithFormatAndArguments(
    env: &mut Environment,
    allocator: CFAllocatorRef,
    // Apple's own docs say format_options are unimplemented!
    _format_options: CFDictionaryRef,
    format: CFStringRef,
    args: VaList,
) -> CFStringRef {
    if !validate_allocator(env, allocator) {
        return nil;
    }

    if format.is_null() {
        return nil;
    }

    let res = ns_string::with_format(env, format, args);
    ns_string::from_rust_string(env, res)
}

fn CFStringCreateWithPascalString(
    env: &mut Environment,
    allocator: CFAllocatorRef,
    p_str: ConstStr255Param,
    encoding: CFStringEncoding,
) -> CFStringRef {
    if !validate_allocator(env, allocator) {
        return nil;
    }

    if p_str.is_null() {
        return nil;
    }

    let len: CFIndex = env.mem.read(p_str).into();
    if !(0..=255).contains(&len) {
        return nil;
    }

    let res = CFStringCreateWithBytes(env, allocator, p_str + 1, len, encoding, false);
    log_dbg!(
        "CFStringCreateWithPascalString(len={}, '{}')",
        len,
        if !res.is_null() {
            ns_string::to_rust_string(env, res)
        } else {
            std::borrow::Cow::Borrowed("<null>")
        }
    );
    res
}

fn CFStringCreateWithSubstring(
    env: &mut Environment,
    allocator: CFAllocatorRef,
    string: CFStringRef,
    range: CFRange,
) -> CFStringRef {
    if !validate_allocator(env, allocator) {
        return nil;
    }

    if string.is_null() {
        return nil;
    }

    let ns_range = match safe_cf_range_to_ns_range(range) {
        Some(r) => r,
        None => return nil,
    };
    msg![env; string substringWithRange:ns_range]
}

fn CFStringCreateArrayBySeparatingStrings(
    env: &mut Environment,
    allocator: CFAllocatorRef,
    string: CFStringRef,
    separator: CFStringRef,
) -> CFArrayRef {
    if !validate_allocator(env, allocator) {
        return nil;
    }

    if string.is_null() || separator.is_null() {
        return nil;
    }

    msg![env;
    string componentsSeparatedByString:separator]
}

fn CFStringCreateByCombiningStrings(
    env: &mut Environment,
    allocator: CFAllocatorRef,
    array: CFArrayRef,
    separator: CFStringRef,
) -> CFStringRef {
    if !validate_allocator(env, allocator) {
        return nil;
    }

    if array.is_null() || separator.is_null() {
        return nil;
    }

    msg![env;
    array componentsJoinedByString:separator]
}

// MARK: - Mutable constructors

fn CFStringCreateMutable(
    env: &mut Environment,
    allocator: CFAllocatorRef,
    max_length: CFIndex,
) -> CFMutableStringRef {
    if !validate_allocator(env, allocator) {
        return nil;
    }

    // max_length is typically ignored (0 means unlimited)
    let _ = max_length;
    msg_class![env; NSMutableString new]
}

fn CFStringCreateMutableCopy(
    env: &mut Environment,
    allocator: CFAllocatorRef,
    max_length: CFIndex,
    the_string: CFStringRef,
) -> CFMutableStringRef {
    if !validate_allocator(env, allocator) {
        return nil;
    }

    if the_string.is_null() {
        return CFStringCreateMutable(env, allocator, max_length);
    }

    // max_length typically ignored
    let _ = max_length;
    msg![env;
    the_string mutableCopy]
}

fn CFStringCreateMutableWithExternalCharactersNoCopy(
    env: &mut Environment,
    allocator: CFAllocatorRef,
    chars: MutPtr<unichar>,
    num_chars: CFIndex,
    capacity: CFIndex,
    external_characters_allocator: CFAllocatorRef,
) -> CFMutableStringRef {
    if !validate_allocator(env, allocator) {
        return nil;
    }

    // Not fully supported - create a regular mutable copy
    let _ = (chars, external_characters_allocator);
    let string = CFStringCreateMutable(env, allocator, capacity);

    if !string.is_null() && !chars.is_null() && num_chars > 0 {
        let temp = CFStringCreateWithCharacters(env, allocator, chars.cast_const(), num_chars);
        if !temp.is_null() {
            () = msg![env; string appendString:temp];
            CFRelease(env, temp);
        }
    }

    string
}

// MARK: - Queries

fn CFStringGetLength(env: &mut Environment, the_string: CFStringRef) -> CFIndex {
    if the_string.is_null() {
        return 0;
    }

    let length: NSUInteger = msg![env; the_string length];
    length.try_into().unwrap_or(0)
}

fn CFStringGetCharacterAtIndex(
    env: &mut Environment,
    the_string: CFStringRef,
    idx: CFIndex,
) -> unichar {
    if the_string.is_null() || idx < 0 {
        return 0;
    }

    let length = CFStringGetLength(env, the_string);
    if idx >= length {
        return 0;
    }

    let idx_u: NSUInteger = idx.try_into().unwrap();
    msg![env;
    the_string characterAtIndex:idx_u]
}

fn CFStringGetCharacters(
    env: &mut Environment,
    string: CFStringRef,
    range: CFRange,
    buffer: MutPtr<unichar>,
) {
    if string.is_null() || buffer.is_null() {
        return;
    }

    let ns_range = match safe_cf_range_to_ns_range(range) {
        Some(r) => r,
        None => return,
    };
    let length = CFStringGetLength(env, string);
    if range.location + range.length > length {
        return;
    }

    msg![env; string getCharacters:buffer range:ns_range]
}

fn CFStringGetCharacterFromInlineBuffer(
    _env: &mut Environment,
    buf: MutVoidPtr,
    idx: CFIndex,
) -> unichar {
    // This would normally use an inline buffer cache
    // For simplicity, we extract the string and get the character
    // In real implementation, this would be optimized
    if buf.is_null() || idx < 0 {
        return 0;
    }

    // The inline buffer structure would contain the string pointer
    // For now, we just return 0 as this is an optimization function
    log!("TODO: CFStringGetCharacterFromInlineBuffer not fully implemented");
    0
}

fn CFStringGetCString(
    env: &mut Environment,
    the_string: CFStringRef,
    buffer: MutPtr<u8>,
    buffer_size: CFIndex,
    encoding: CFStringEncoding,
) -> bool {
    if the_string.is_null() || buffer.is_null() || buffer_size <= 0 {
        return false;
    }

    let encoding = CFStringConvertEncodingToNSStringEncoding(env, encoding);
    let buffer_size_u = buffer_size as NSUInteger;
    msg![env;
    the_string getCString:buffer maxLength:buffer_size_u encoding:encoding]
}

fn CFStringGetCStringPtr(
    env: &mut Environment,
    the_string: CFStringRef,
    encoding: CFStringEncoding,
) -> ConstPtr<u8> {
    if the_string.is_null() {
        return ConstPtr::null();
    }

    let encoding = CFStringConvertEncodingToNSStringEncoding(env, encoding);
    msg![env;
    the_string cStringUsingEncoding:encoding]
}

fn CFStringGetPascalString(
    env: &mut Environment,
    the_string: CFStringRef,
    buffer: StringPtr,
    buffer_size: CFIndex,
    encoding: CFStringEncoding,
) -> bool {
    if the_string.is_null() || buffer.is_null() || buffer_size < 1 {
        return false;
    }

    log_dbg!(
        "CFStringGetPascalString('{}')",
        ns_string::to_rust_string(env, the_string)
    );
    let len = CFStringGetLength(env, the_string);

    // Pascal string needs length byte + content
    if (len + 1) > buffer_size || len > 255 {
        return false;
    }

    let len_u8: u8 = len.try_into().unwrap_or(0);
    env.mem.write(buffer, len_u8);

    let encoding = CFStringConvertEncodingToNSStringEncoding(env, encoding);
    ns_string::get_bytes_buffer_inner(env, the_string, buffer + 1, len_u8.into(), encoding, false)
}

fn CFStringGetBytes(
    env: &mut Environment,
    the_string: CFStringRef,
    range: CFRange,
    encoding: CFStringEncoding,
    loss_byte: u8,
    is_external_representation: bool,
    buffer: MutPtr<u8>,
    max_buf_len: CFIndex,
    used_buf_len: MutPtr<CFIndex>,
) -> CFIndex {
    if the_string.is_null() || max_buf_len < 0 {
        return 0;
    }

    let ns_range = match safe_cf_range_to_ns_range(range) {
        Some(r) => r,
        None => return 0,
    };
    let length = CFStringGetLength(env, the_string);
    if range.location + range.length > length {
        return 0;
    }

    let encoding_ns = CFStringConvertEncodingToNSStringEncoding(env, encoding);
    let range_length = ns_range.length;
    let substring: id = msg![env; the_string substringWithRange:ns_range];
    let lossy = loss_byte != 0;
    let mut bytes = cf_string_bytes_for_encoding(env, substring, encoding_ns, lossy, loss_byte);
    if bytes.is_none() && !lossy {
        return 0;
    }
    let bytes = bytes.take().unwrap_or_else(|| {
        cf_string_bytes_for_encoding(env, substring, encoding_ns, true, b'?').unwrap()
    });
    let total_bytes: NSUInteger = bytes.len().try_into().unwrap_or(0);

    if !used_buf_len.is_null() {
        env.mem.write(used_buf_len, 0);
    }

    if is_external_representation {
        log!("CFStringGetBytes: external representation requested but not implemented");
        return 0;
    }

    if buffer.is_null() {
        if !used_buf_len.is_null() {
            env.mem
                .write(used_buf_len, total_bytes.try_into().unwrap_or(0));
        }
        return range.length;
    }

    let max_len_u: NSUInteger = max_buf_len.try_into().unwrap_or(0);
    if total_bytes <= max_len_u {
        let dest = env
            .mem
            .bytes_at_mut(buffer, total_bytes.try_into().unwrap_or(0));
        dest.copy_from_slice(&bytes);
        if !used_buf_len.is_null() {
            env.mem
                .write(used_buf_len, total_bytes.try_into().unwrap_or(0));
        }
        return range.length;
    }

    let mut converted_chars: NSUInteger = 0;
    let mut converted_bytes: NSUInteger = 0;
    for chars_to_convert in 1..=range_length {
        let prefix_range = NSRange {
            location: 0,
            length: chars_to_convert,
        };
        let prefix: id = msg![env; substring substringWithRange:prefix_range];
        let Some(prefix_bytes_vec) =
            cf_string_bytes_for_encoding(env, prefix, encoding_ns, lossy, loss_byte)
        else {
            break;
        };
        let prefix_bytes = prefix_bytes_vec.len().try_into().unwrap_or(NSUInteger::MAX);
        if prefix_bytes > max_len_u {
            break;
        }
        converted_chars = chars_to_convert;
        converted_bytes = prefix_bytes;
    }

    if converted_chars == 0 {
        return 0;
    }

    let prefix: id = msg![env; substring substringWithRange:(NSRange {
        location: 0,
        length: converted_chars,
    })];
    let Some(prefix_bytes) =
        cf_string_bytes_for_encoding(env, prefix, encoding_ns, lossy, loss_byte)
    else {
        return 0;
    };
    let dest = env
        .mem
        .bytes_at_mut(buffer, converted_bytes.try_into().unwrap_or(0));
    dest.copy_from_slice(&prefix_bytes);

    if !used_buf_len.is_null() {
        env.mem
            .write(used_buf_len, converted_bytes.try_into().unwrap_or(0));
    }
    converted_chars.try_into().unwrap_or(0)
}

fn cf_string_bytes_for_encoding(
    env: &mut Environment,
    string: id,
    encoding: ns_string::NSStringEncoding,
    lossy: bool,
    loss_byte: u8,
) -> Option<Vec<u8>> {
    let rust_string = ns_string::to_rust_string(env, string);
    let bytes = match encoding {
        ns_string::NSASCIIStringEncoding
        | ns_string::NSISOLatin1StringEncoding
        | ns_string::NSMacOSRomanStringEncoding
        | ns_string::NSNextStepLatinStringEncoding => {
            if lossy {
                rust_string
                    .chars()
                    .map(|c| {
                        if (c as u32) <= 0xff {
                            c as u8
                        } else {
                            loss_byte
                        }
                    })
                    .collect()
            } else if rust_string.chars().any(|c| (c as u32) > 0xff) {
                return None;
            } else {
                rust_string.chars().map(|c| c as u8).collect()
            }
        }
        ns_string::NSUTF8StringEncoding | ns_string::NSWindowsCP1252StringEncoding => {
            rust_string.as_bytes().to_vec()
        }
        ns_string::NSUTF16LittleEndianStringEncoding
        | ns_string::NSUTF16StringEncoding
        | ns_string::NSUnicodeStringEncoding => rust_string
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect(),
        ns_string::NSUTF16BigEndianStringEncoding => rust_string
            .encode_utf16()
            .flat_map(u16::to_be_bytes)
            .collect(),
        ns_string::NSUTF32LittleEndianStringEncoding => rust_string
            .chars()
            .flat_map(|c| (c as u32).to_le_bytes())
            .collect(),
        ns_string::NSUTF32BigEndianStringEncoding | ns_string::NSUTF32StringEncoding => rust_string
            .chars()
            .flat_map(|c| (c as u32).to_be_bytes())
            .collect(),
        ns_string::NSShiftJISStringEncoding => {
            let (cow, _, _) = encoding_rs::SHIFT_JIS.encode(&rust_string);
            cow.into_owned()
        }
        _ => rust_string.as_bytes().to_vec(),
    };
    Some(bytes)
}

fn CFStringGetIntValue(env: &mut Environment, string: CFStringRef) -> i32 {
    if string.is_null() {
        return 0;
    }

    msg![env; string intValue]
}

fn CFStringGetDoubleValue(env: &mut Environment, string: CFStringRef) -> f64 {
    if string.is_null() {
        return 0.0;
    }

    msg![env; string doubleValue]
}

// MARK: - Searching

fn CFStringFind(
    env: &mut Environment,
    string: CFStringRef,
    to_find: CFStringRef,
    options: CFStringCompareFlags,
) -> CFRange {
    if string.is_null() || to_find.is_null() {
        return CFRange {
            location: kCFNotFound,
            length: 0,
        };
    }

    let range: NSRange = msg![env; string rangeOfString:to_find options:options];
    let location: CFIndex = if range.location == NSNotFound as NSUInteger {
        kCFNotFound
    } else {
        range.location.try_into().unwrap_or(kCFNotFound)
    };
    CFRange {
        location,
        length: range.length.try_into().unwrap_or(0),
    }
}

fn CFStringFindWithOptions(
    env: &mut Environment,
    string: CFStringRef,
    to_find: CFStringRef,
    range_to_search: CFRange,
    options: CFStringCompareFlags,
    result: MutPtr<CFRange>,
) -> bool {
    if string.is_null() || to_find.is_null() {
        return false;
    }

    let search_range = match safe_cf_range_to_ns_range(range_to_search) {
        Some(r) => r,
        None => return false,
    };
    let found_range: NSRange =
        msg![env; string rangeOfString:to_find options:options range:search_range];

    if found_range.location == NSNotFound as NSUInteger {
        return false;
    }

    if !result.is_null() {
        let cf_range = CFRange {
            location: found_range.location.try_into().unwrap_or(kCFNotFound),
            length: found_range.length.try_into().unwrap_or(0),
        };
        env.mem.write(result, cf_range);
    }

    true
}

fn CFStringFindCharacterFromSet(
    env: &mut Environment,
    string: CFStringRef,
    set: CFTypeRef, // CFCharacterSetRef
    range_to_search: CFRange,
    options: CFStringCompareFlags,
    result: MutPtr<CFRange>,
) -> bool {
    if string.is_null() || set.is_null() {
        return false;
    }

    let search_range = match safe_cf_range_to_ns_range(range_to_search) {
        Some(r) => r,
        None => return false,
    };
    let found_range: NSRange =
        msg![env; string rangeOfCharacterFromSet:set options:options range:search_range];

    if found_range.location == NSNotFound as NSUInteger {
        return false;
    }

    if !result.is_null() {
        let cf_range = CFRange {
            location: found_range.location.try_into().unwrap_or(kCFNotFound),
            length: found_range.length.try_into().unwrap_or(0),
        };
        env.mem.write(result, cf_range);
    }

    true
}

// MARK: - Comparison

pub type CFStringCompareFlags = CFOptionFlags;
fn CFStringCompare(
    env: &mut Environment,
    a: CFStringRef,
    b: CFStringRef,
    flags: CFStringCompareFlags,
) -> CFComparisonResult {
    if a.is_null() && b.is_null() {
        return 0;
        // kCFCompareEqualTo
    }
    if a.is_null() {
        return -1;
        // kCFCompareLessThan
    }
    if b.is_null() {
        return 1;
        // kCFCompareGreaterThan
    }

    msg![env;
    a compare:b options:flags]
}

fn CFStringCompareWithOptions(
    env: &mut Environment,
    a: CFStringRef,
    b: CFStringRef,
    range: CFRange,
    flags: CFStringCompareFlags,
) -> CFComparisonResult {
    if a.is_null() || b.is_null() {
        return if a.is_null() && b.is_null() {
            0
        } else if a.is_null() {
            -1
        } else {
            1
        };
    }

    let ns_range = match safe_cf_range_to_ns_range(range) {
        Some(r) => r,
        None => return 0,
    };
    // Create substring and compare
    let a_sub: id = msg![env; a substringWithRange:ns_range];
    msg![env;
    a_sub compare:b options:flags]
}

fn CFStringCompareWithOptionsAndLocale(
    env: &mut Environment,
    a: CFStringRef,
    b: CFStringRef,
    range: CFRange,
    flags: CFStringCompareFlags,
    locale: CFLocaleRef,
) -> CFComparisonResult {
    if a.is_null() || b.is_null() {
        return if a.is_null() && b.is_null() {
            0
        } else if a.is_null() {
            -1
        } else {
            1
        };
    }

    let ns_range = match safe_cf_range_to_ns_range(range) {
        Some(r) => r,
        None => return 0,
    };
    let a_sub: id = msg![env; a substringWithRange:ns_range];

    if locale.is_null() {
        msg![env;
        a_sub compare:b options:flags]
    } else {
        msg![env;
        a_sub compare:b options:flags range:(NSRange { location: 0, length: msg![env; b length] }) locale:locale]
    }
}

fn CFStringHasPrefix(env: &mut Environment, the_string: CFStringRef, prefix: CFStringRef) -> bool {
    if the_string.is_null() || prefix.is_null() {
        return false;
    }

    msg![env;
    the_string hasPrefix:prefix]
}

fn CFStringHasSuffix(env: &mut Environment, the_string: CFStringRef, suffix: CFStringRef) -> bool {
    if the_string.is_null() || suffix.is_null() {
        return false;
    }

    msg![env;
    the_string hasSuffix:suffix]
}

// MARK: - Mutation

fn CFStringAppend(
    env: &mut Environment,
    the_string: CFMutableStringRef,
    appended_string: CFStringRef,
) {
    if the_string.is_null() || appended_string.is_null() {
        return;
    }

    () = msg![env;
    the_string appendString:appended_string]
}

fn CFStringAppendCString(
    env: &mut Environment,
    string: CFMutableStringRef,
    c_string: ConstPtr<u8>,
    encoding: CFStringEncoding,
) {
    if string.is_null() || c_string.is_null() {
        return;
    }

    let encoding = CFStringConvertEncodingToNSStringEncoding(env, encoding);
    let to_append: id = msg_class![env;
    NSString stringWithCString:c_string encoding:encoding];

    if !to_append.is_null() {
        () = msg![env; string appendString:to_append];
    }
}

fn CFStringAppendCharacters(
    env: &mut Environment,
    the_string: CFMutableStringRef,
    chars: ConstPtr<unichar>,
    num_chars: CFIndex,
) {
    if the_string.is_null() || chars.is_null() || num_chars <= 0 {
        return;
    }

    let temp = CFStringCreateWithCharacters(env, kCFAllocatorDefault, chars, num_chars);
    if !temp.is_null() {
        () = msg![env; the_string appendString:temp];
        CFRelease(env, temp);
    }
}

fn CFStringAppendFormat(
    env: &mut Environment,
    string: CFMutableStringRef,
    _format_options: CFDictionaryRef,
    format: CFStringRef,
    dots: DotDotDot,
) {
    if string.is_null() || format.is_null() {
        return;
    }

    let res = ns_string::with_format(env, format, dots.start());
    let to_append: id = ns_string::from_rust_string(env, res);
    if !to_append.is_null() {
        () = msg![env; string appendString:to_append];
    }
}

fn CFStringInsert(
    env: &mut Environment,
    string: CFMutableStringRef,
    idx: CFIndex,
    inserted_str: CFStringRef,
) {
    if string.is_null() || inserted_str.is_null() || idx < 0 {
        return;
    }

    let length = CFStringGetLength(env, string);
    if idx > length {
        return;
    }

    let idx_u: NSUInteger = idx.try_into().unwrap();
    () = msg![env; string insertString:inserted_str atIndex:idx_u];
}

fn CFStringDelete(env: &mut Environment, string: CFMutableStringRef, range: CFRange) {
    if string.is_null() {
        return;
    }

    let ns_range = match safe_cf_range_to_ns_range(range) {
        Some(r) => r,
        None => return,
    };
    let length = CFStringGetLength(env, string);
    if range.location + range.length > length {
        return;
    }

    () = msg![env; string deleteCharactersInRange:ns_range];
}

fn CFStringReplace(
    env: &mut Environment,
    string: CFMutableStringRef,
    range: CFRange,
    replacement: CFStringRef,
) {
    if string.is_null() || replacement.is_null() {
        return;
    }

    let ns_range = match safe_cf_range_to_ns_range(range) {
        Some(r) => r,
        None => return,
    };
    let length = CFStringGetLength(env, string);
    if range.location + range.length > length {
        return;
    }

    () = msg![env; string replaceCharactersInRange:ns_range withString:replacement];
}

fn CFStringReplaceAll(env: &mut Environment, string: CFMutableStringRef, replacement: CFStringRef) {
    if string.is_null() || replacement.is_null() {
        return;
    }

    () = msg![env; string setString:replacement];
}

fn CFStringFindAndReplace(
    env: &mut Environment,
    string: CFMutableStringRef,
    string_to_find: CFStringRef,
    replacement_string: CFStringRef,
    range_to_search: CFRange,
    compare_options: CFStringCompareFlags,
) -> CFIndex {
    if string.is_null() || string_to_find.is_null() || replacement_string.is_null() {
        return 0;
    }

    let ns_range = match safe_cf_range_to_ns_range(range_to_search) {
        Some(r) => r,
        None => return 0,
    };
    let length = CFStringGetLength(env, string);
    if range_to_search.location + range_to_search.length > length {
        return 0;
    }

    let count: NSUInteger = msg![env;
    string
        replaceOccurrencesOfString:string_to_find
        withString:replacement_string
        options:compare_options
        range:ns_range];
    count.try_into().unwrap_or(0)
}

fn CFStringPad(
    env: &mut Environment,
    string: CFMutableStringRef,
    pad_string: CFStringRef,
    length: CFIndex,
    index_into_pad: CFIndex,
) {
    if string.is_null() || pad_string.is_null() || length < 0 || index_into_pad < 0 {
        return;
    }

    let current_len = CFStringGetLength(env, string);
    if current_len >= length {
        return;
        // Already long enough
    }

    let pad_len = CFStringGetLength(env, pad_string);
    if pad_len == 0 {
        return;
    }

    let needed = length - current_len;
    let mut padded = 0;
    while padded < needed {
        let start_idx = index_into_pad % pad_len;
        let chars_to_add = (needed - padded).min(pad_len - start_idx);

        let range = CFRange {
            location: start_idx,
            length: chars_to_add,
        };
        let substring: id =
            msg![env; pad_string substringWithRange:(safe_cf_range_to_ns_range(range).unwrap())];
        () = msg![env; string appendString:substring];

        padded += chars_to_add;
    }
}

fn CFStringTrim(env: &mut Environment, string: CFMutableStringRef, trim_string: CFStringRef) {
    if string.is_null() || trim_string.is_null() {
        return;
    }

    // Create character set first, then use it
    let char_set: id = msg_class![env;
    NSCharacterSet characterSetWithCharactersInString:trim_string];
    let trimmed: id = msg![env; string stringByTrimmingCharactersInSet:char_set];
    () = msg![env; string setString:trimmed];
}

fn CFStringTrimWhitespace(env: &mut Environment, string: CFMutableStringRef) {
    if string.is_null() {
        return;
    }

    let whitespace_set: id = msg_class![env; NSCharacterSet whitespaceAndNewlineCharacterSet];
    let trimmed: id = msg![env;
    string stringByTrimmingCharactersInSet:whitespace_set];
    () = msg![env; string setString:trimmed];
}

// MARK: - Case transformations

fn CFStringLowercase(env: &mut Environment, string: CFMutableStringRef, _locale: CFLocaleRef) {
    if string.is_null() {
        return;
    }

    // TODO: account for locale
    let lowercase: id = msg![env;
    string lowercaseString];
    () = msg![env; string setString:lowercase];
}

fn CFStringUppercase(env: &mut Environment, string: CFMutableStringRef, _locale: CFLocaleRef) {
    if string.is_null() {
        return;
    }

    // TODO: account for locale
    let uppercase: id = msg![env;
    string uppercaseString];
    () = msg![env; string setString:uppercase];
}

fn CFStringCapitalize(env: &mut Environment, string: CFMutableStringRef, _locale: CFLocaleRef) {
    if string.is_null() {
        return;
    }

    // TODO: account for locale
    let capitalized: id = msg![env;
    string capitalizedString];
    () = msg![env; string setString:capitalized];
}

// MARK: - Normalization and transformation

fn CFStringNormalize(
    env: &mut Environment,
    the_string: CFMutableStringRef,
    the_form: CFStringNormalizationForm,
) {
    if the_string.is_null() {
        return;
    }

    let str_content = ns_string::to_rust_string(env, the_string);

    // Apple docs: CFStringNormalize performs in-place Unicode normalization
    // on a mutable string. The four normalization forms are:
    // - FormD (NFD): Canonical Decomposition
    // - FormKD (NFKD): Compatibility Decomposition
    // - FormC (NFC): Canonical Decomposition followed by Canonical Composition
    // - FormKC (NFKC): Compatibility Decomposition followed by Canonical Composition
    //
    // For ASCII-only strings (the common case in iOS game paths), all forms
    // are identity operations. We detect ASCII-only content and skip
    // processing. For non-ASCII content we perform the normalization using
    // Rust's built-in Unicode character decomposition/composition tables.

    // Fast path: if the string is entirely ASCII, normalization is a no-op
    // for all four forms.
    if str_content.bytes().all(|b| b < 0x80) {
        return;
    }

    // For non-ASCII strings, perform real Unicode normalization.
    // We implement this using Rust's char::decompose_canonical /
    // char::decompose_compatible and manual canonical composition.
    let normalized = match the_form {
        kCFStringNormalizationFormD => {
            // NFD: Canonical Decomposition
            unicode_nfd(&str_content)
        }
        kCFStringNormalizationFormKD => {
            // NFKD: Compatibility Decomposition
            unicode_nfkd(&str_content)
        }
        kCFStringNormalizationFormC => {
            // NFC: Canonical Decomposition + Canonical Composition
            unicode_nfc(&str_content)
        }
        kCFStringNormalizationFormKC => {
            // NFKC: Compatibility Decomposition + Canonical Composition
            unicode_nfkc(&str_content)
        }
        _ => {
            log!("Unknown normalization form: {}", the_form);
            return;
        }
    };

    // Write the normalized string back if it changed
    if normalized != str_content.as_ref() {
        let new_str = ns_string::from_rust_string(env, normalized);
        () = msg![env; the_string setString:new_str];
    }
}

fn CFStringTransform(
    env: &mut Environment,
    string: CFMutableStringRef,
    _range: MutPtr<CFRange>,
    transform: CFStringRef,
    reverse: bool,
) -> bool {
    if string.is_null() || transform.is_null() {
        return false;
    }

    let transform_name = ns_string::to_rust_string(env, transform);
    log!(
        "TODO: CFStringTransform('{}', reverse={})",
        transform_name,
        reverse
    );
    // For now, basic implementation of common transforms
    match transform_name.as_ref() {
        kCFStringTransformStripDiacritics | kCFStringTransformStripCombiningMarks => {
            // Strip accents/diacritics - approximate implementation
            let folded: id = msg![env;
            string
                stringByFoldingWithOptions:128 // NSCaseInsensitiveSearch + NSDiacriticInsensitiveSearch
                locale:nil];
            () = msg![env; string setString:folded];
            true
        }
        kCFStringTransformToLatin => {
            // For non-Latin scripts, transliterate to Latin
            // This is very complex - just log for now
            false
        }
        _ => false,
    }
}

// MARK: - Type info

fn CFStringGetTypeID(_env: &mut Environment) -> u32 {
    // Return a fake CFTypeID for CFString
    0x43465374 // 'CFSt' in hex
}

// MARK: - Exports

/// `Boolean CFStringGetFileSystemRepresentation(CFStringRef string,
///                                              char *buffer,
///                                              CFIndex maxBufLen)`
///
/// Per Apple docs: "Extracts the contents of a string object and stores
/// them in a C-string buffer in a form suitable for use with POSIX file
/// system calls. Returns true if the operation was successful."
/// The file system representation on iOS is UTF-8.
/// Reference: https://developer.apple.com/documentation/corefoundation/1542721-cfstringgetfilesystemrepresentat
fn CFStringGetFileSystemRepresentation(
    env: &mut Environment,
    the_string: CFStringRef,
    buffer: MutPtr<u8>,
    max_buf_len: CFIndex,
) -> bool {
    if the_string.is_null() || buffer.is_null() || max_buf_len <= 0 {
        return false;
    }
    // File system representation is always UTF-8 on iOS/macOS
    CFStringGetCString(env, the_string, buffer, max_buf_len, kCFStringEncodingUTF8)
}

fn CFStringCreateExternalRepresentation(
    env: &mut Environment,
    _alloc: CFAllocatorRef,
    the_string: CFStringRef,
    encoding: CFStringEncoding,
    loss_byte: u8,
) -> CFTypeRef {
    if the_string == nil {
        return nil;
    }

    // Маппинг CFStringEncoding в NSStringEncoding
    let ns_encoding: NSUInteger = match encoding {
        0 => 30,                  // kCFStringEncodingMacRoman -> NSMacOSRomanStringEncoding
        0x0600 => 1,              // kCFStringEncodingASCII -> NSASCIIStringEncoding
        0x0201 => 5,              // kCFStringEncodingISOLatin1 -> NSISOLatin1StringEncoding
        0x08000100 => 4,          // kCFStringEncodingUTF8 -> NSUTF8StringEncoding
        0x0100 => 10,             // kCFStringEncodingUTF16 -> NSUTF16StringEncoding
        0x10000100 => 0x90000100, // kCFStringEncodingUTF16BE -> NSUTF16BigEndianStringEncoding
        0x14000100 => 0x94000100, // kCFStringEncodingUTF16LE -> NSUTF16LittleEndianStringEncoding
        0x0c000100 => 0x8c000100, // kCFStringEncodingUTF32 -> NSUTF32StringEncoding
        0x18000100 => 0x98000100, // kCFStringEncodingUTF32BE -> NSUTF32BigEndianStringEncoding
        0x1c000100 => 0x9c000100, // kCFStringEncodingUTF32LE -> NSUTF32LittleEndianStringEncoding
        _ => {
            log!("Warning: _CFStringCreateExternalRepresentation unknown encoding 0x{:X}, defaulting to UTF8", encoding);
            4 // По умолчанию UTF-8
        }
    };

    let lossy = loss_byte != 0;

    // Вызываем метод -[NSString dataUsingEncoding:allowLossyConversion:]
    let data: id = msg![env; the_string dataUsingEncoding:ns_encoding allowLossyConversion:lossy];

    // Core Foundation функции со словом "Create" обязаны возвращать объект с +1
    // retain count.
    // Так как метод dataUsingEncoding возвращает autoreleased объект, его нужно
    // вручную удержать.
    if data != nil {
        let _: () = msg![env; data retain];
    }

    data
}

// =============================================================================
// MARK: - Unicode Normalization Helpers
// =============================================================================

/// Canonical Ordering Algorithm: sort combining marks by their Canonical
/// Combining Class (ccc). We use a simplified lookup that covers the most
/// common combining characters (accents, diacritics used in Latin/Cyrillic).
fn canonical_combining_class(ch: char) -> u8 {
    let cp = ch as u32;
    // Most characters have ccc=0 (starter). We handle the common ranges:
    match cp {
        // Combining Diacritical Marks (U+0300..U+036F)
        0x0300..=0x0314 => 230, // Above
        0x0315 => 232,          // Above Right
        0x0316..=0x0319 => 220, // Below
        0x031A => 232,
        0x031B => 216,          // Attached Below Left (horn)
        0x031C..=0x0320 => 220,
        0x0321..=0x0322 => 202, // Attached Below
        0x0323..=0x0326 => 220,
        0x0327..=0x0328 => 202, // Attached Below (cedilla, ogonek)
        0x0329..=0x0333 => 220,
        0x0334..=0x0338 => 1,   // Overlay
        0x0339..=0x033C => 220,
        0x033D..=0x0344 => 230,
        0x0345 => 240,          // Iota subscript
        0x0346..=0x034E => 230,
        0x0350..=0x0352 => 230,
        0x0353..=0x0356 => 220,
        0x0357 => 230,
        0x0358 => 232,
        0x0359..=0x035A => 220,
        0x035B => 230,
        0x035C => 233,
        0x035D..=0x035E => 234,
        0x035F => 233,
        0x0360..=0x0361 => 234,
        0x0362 => 233,
        0x0363..=0x036F => 230,
        // Hebrew accents (simplified)
        0x0591..=0x05BD => 220,
        0x05BF => 23,
        0x05C1 => 24,
        0x05C2 => 25,
        0x05C4 => 230,
        0x05C5 => 220,
        0x05C7 => 18,
        // Arabic (simplified: most combining marks are 230 or 220)
        0x064B..=0x065F => 230,
        0x0670 => 35,
        // Devanagari nukta / virama
        0x093C => 7,
        0x094D => 9,
        // Bengali, Gurmukhi, etc. nukta / virama
        0x09BC => 7,
        0x09CD => 9,
        0x0A3C => 7,
        0x0A4D => 9,
        0x0ABC => 7,
        0x0ACD => 9,
        0x0B3C => 7,
        0x0B4D => 9,
        0x0BCD => 9,
        0x0C4D => 9,
        0x0CCD => 9,
        0x0D4D => 9,
        // Thai / Lao
        0x0E38..=0x0E3A => 103,
        0x0E48..=0x0E4B => 107,
        0x0EB8..=0x0EB9 => 118,
        0x0EC8..=0x0ECB => 122,
        // Combining Diacritical Marks Extended / Supplement (common ones)
        0x1DC0..=0x1DFF => 230,
        0x20D0..=0x20DC => 230,
        0x20DD..=0x20E0 => 0,   // Enclosing marks
        0x20E1 => 230,
        0x20E2..=0x20E4 => 0,
        0x20E5..=0x20F0 => 230,
        // Combining Half Marks
        0xFE20..=0xFE2F => 230,
        _ => 0,
    }
}

/// Sort combining mark sequence by canonical combining class (stable sort).
fn canonical_sort(buffer: &mut Vec<char>) {
    // Find runs of non-starters and sort them by ccc.
    let len = buffer.len();
    let mut i = 0;
    while i < len {
        if canonical_combining_class(buffer[i]) == 0 {
            i += 1;
            continue;
        }
        // Found start of combining sequence
        let start = i;
        while i < len && canonical_combining_class(buffer[i]) != 0 {
            i += 1;
        }
        // Stable sort by ccc
        buffer[start..i].sort_by_key(|&ch| canonical_combining_class(ch));
    }
}

/// NFD: Canonical Decomposition.
/// Decomposes each character to its canonical decomposition, then applies
/// canonical ordering.
fn unicode_nfd(input: &str) -> String {
    let mut buffer: Vec<char> = Vec::with_capacity(input.len());
    for ch in input.chars() {
        decompose_canonical(ch, &mut buffer);
    }
    canonical_sort(&mut buffer);
    buffer.into_iter().collect()
}

/// NFKD: Compatibility Decomposition.
fn unicode_nfkd(input: &str) -> String {
    let mut buffer: Vec<char> = Vec::with_capacity(input.len());
    for ch in input.chars() {
        decompose_compatible(ch, &mut buffer);
    }
    canonical_sort(&mut buffer);
    buffer.into_iter().collect()
}

/// NFC: Canonical Decomposition + Canonical Composition.
fn unicode_nfc(input: &str) -> String {
    let decomposed = unicode_nfd(input);
    canonical_compose(&decomposed)
}

/// NFKC: Compatibility Decomposition + Canonical Composition.
fn unicode_nfkc(input: &str) -> String {
    let decomposed = unicode_nfkd(input);
    canonical_compose(&decomposed)
}

/// Canonical Decomposition for a single character.
/// Uses Rust's built-in char methods where possible. For characters that
/// don't have simple decompositions, outputs the character as-is.
fn decompose_canonical(ch: char, output: &mut Vec<char>) {
    // Common precomposed Latin characters (NFC -> NFD mappings)
    // This covers the vast majority of characters apps will encounter.
    match ch {
        '\u{00C0}' => { output.push('A'); output.push('\u{0300}'); } // À
        '\u{00C1}' => { output.push('A'); output.push('\u{0301}'); } // Á
        '\u{00C2}' => { output.push('A'); output.push('\u{0302}'); } // Â
        '\u{00C3}' => { output.push('A'); output.push('\u{0303}'); } // Ã
        '\u{00C4}' => { output.push('A'); output.push('\u{0308}'); } // Ä
        '\u{00C5}' => { output.push('A'); output.push('\u{030A}'); } // Å
        '\u{00C7}' => { output.push('C'); output.push('\u{0327}'); } // Ç
        '\u{00C8}' => { output.push('E'); output.push('\u{0300}'); } // È
        '\u{00C9}' => { output.push('E'); output.push('\u{0301}'); } // É
        '\u{00CA}' => { output.push('E'); output.push('\u{0302}'); } // Ê
        '\u{00CB}' => { output.push('E'); output.push('\u{0308}'); } // Ë
        '\u{00CC}' => { output.push('I'); output.push('\u{0300}'); } // Ì
        '\u{00CD}' => { output.push('I'); output.push('\u{0301}'); } // Í
        '\u{00CE}' => { output.push('I'); output.push('\u{0302}'); } // Î
        '\u{00CF}' => { output.push('I'); output.push('\u{0308}'); } // Ï
        '\u{00D1}' => { output.push('N'); output.push('\u{0303}'); } // Ñ
        '\u{00D2}' => { output.push('O'); output.push('\u{0300}'); } // Ò
        '\u{00D3}' => { output.push('O'); output.push('\u{0301}'); } // Ó
        '\u{00D4}' => { output.push('O'); output.push('\u{0302}'); } // Ô
        '\u{00D5}' => { output.push('O'); output.push('\u{0303}'); } // Õ
        '\u{00D6}' => { output.push('O'); output.push('\u{0308}'); } // Ö
        '\u{00D9}' => { output.push('U'); output.push('\u{0300}'); } // Ù
        '\u{00DA}' => { output.push('U'); output.push('\u{0301}'); } // Ú
        '\u{00DB}' => { output.push('U'); output.push('\u{0302}'); } // Û
        '\u{00DC}' => { output.push('U'); output.push('\u{0308}'); } // Ü
        '\u{00DD}' => { output.push('Y'); output.push('\u{0301}'); } // Ý
        '\u{00E0}' => { output.push('a'); output.push('\u{0300}'); } // à
        '\u{00E1}' => { output.push('a'); output.push('\u{0301}'); } // á
        '\u{00E2}' => { output.push('a'); output.push('\u{0302}'); } // â
        '\u{00E3}' => { output.push('a'); output.push('\u{0303}'); } // ã
        '\u{00E4}' => { output.push('a'); output.push('\u{0308}'); } // ä
        '\u{00E5}' => { output.push('a'); output.push('\u{030A}'); } // å
        '\u{00E7}' => { output.push('c'); output.push('\u{0327}'); } // ç
        '\u{00E8}' => { output.push('e'); output.push('\u{0300}'); } // è
        '\u{00E9}' => { output.push('e'); output.push('\u{0301}'); } // é
        '\u{00EA}' => { output.push('e'); output.push('\u{0302}'); } // ê
        '\u{00EB}' => { output.push('e'); output.push('\u{0308}'); } // ë
        '\u{00EC}' => { output.push('i'); output.push('\u{0300}'); } // ì
        '\u{00ED}' => { output.push('i'); output.push('\u{0301}'); } // í
        '\u{00EE}' => { output.push('i'); output.push('\u{0302}'); } // î
        '\u{00EF}' => { output.push('i'); output.push('\u{0308}'); } // ï
        '\u{00F1}' => { output.push('n'); output.push('\u{0303}'); } // ñ
        '\u{00F2}' => { output.push('o'); output.push('\u{0300}'); } // ò
        '\u{00F3}' => { output.push('o'); output.push('\u{0301}'); } // ó
        '\u{00F4}' => { output.push('o'); output.push('\u{0302}'); } // ô
        '\u{00F5}' => { output.push('o'); output.push('\u{0303}'); } // õ
        '\u{00F6}' => { output.push('o'); output.push('\u{0308}'); } // ö
        '\u{00F9}' => { output.push('u'); output.push('\u{0300}'); } // ù
        '\u{00FA}' => { output.push('u'); output.push('\u{0301}'); } // ú
        '\u{00FB}' => { output.push('u'); output.push('\u{0302}'); } // û
        '\u{00FC}' => { output.push('u'); output.push('\u{0308}'); } // ü
        '\u{00FD}' => { output.push('y'); output.push('\u{0301}'); } // ý
        '\u{00FF}' => { output.push('y'); output.push('\u{0308}'); } // ÿ
        // Hangul decomposition (LV and LVT syllables)
        ch if ('\u{AC00}'..='\u{D7A3}').contains(&ch) => {
            let s_index = ch as u32 - 0xAC00;
            let l = 0x1100 + s_index / (21 * 28);
            let v = 0x1161 + (s_index % (21 * 28)) / 28;
            let t = s_index % 28;
            output.push(char::from_u32(l).unwrap());
            output.push(char::from_u32(v).unwrap());
            if t != 0 {
                output.push(char::from_u32(0x11A7 + t).unwrap());
            }
        }
        // Default: character has no decomposition, output as-is
        _ => output.push(ch),
    }
}

/// Compatibility Decomposition — includes canonical decompositions plus
/// compatibility mappings (e.g. ﬁ -> fi, ² -> 2, etc.)
fn decompose_compatible(ch: char, output: &mut Vec<char>) {
    match ch {
        // Compatibility mappings for common characters
        '\u{FB01}' => { output.push('f'); output.push('i'); }  // ﬁ
        '\u{FB02}' => { output.push('f'); output.push('l'); }  // ﬂ
        '\u{00B2}' => output.push('2'),  // ²
        '\u{00B3}' => output.push('3'),  // ³
        '\u{00B9}' => output.push('1'),  // ¹
        '\u{00BC}' => { output.push('1'); output.push('/'); output.push('4'); }
        '\u{00BD}' => { output.push('1'); output.push('/'); output.push('2'); }
        '\u{00BE}' => { output.push('3'); output.push('/'); output.push('4'); }
        '\u{2002}' => output.push(' '),  // En space
        '\u{2003}' => output.push(' '),  // Em space
        '\u{2004}'..='\u{200A}' => output.push(' '),  // Various spaces
        '\u{2024}' => output.push('.'),  // One dot leader
        '\u{2025}' => { output.push('.'); output.push('.'); }  // Two dot leader
        '\u{2026}' => { output.push('.'); output.push('.'); output.push('.'); }  // Ellipsis
        // For everything else, fall through to canonical decomposition
        _ => decompose_canonical(ch, output),
    }
}

/// Canonical Composition: takes a decomposed string and composes characters
/// where possible (the inverse of NFD for precomposed forms).
fn canonical_compose(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    if chars.is_empty() {
        return String::new();
    }

    let mut result: Vec<char> = Vec::with_capacity(chars.len());
    result.push(chars[0]);

    for i in 1..chars.len() {
        let ch = chars[i];
        let ccc = canonical_combining_class(ch);

        // Try to compose with the last starter in result
        let last_starter_idx = if ccc == 0 {
            // ch is a starter; try to compose with previous starter
            result.len() - 1
        } else {
            // ch is a combining mark; find the last starter
            let mut idx = result.len();
            while idx > 0 {
                idx -= 1;
                if canonical_combining_class(result[idx]) == 0 {
                    break;
                }
            }
            idx
        };

        // Check if there's a blocked combining character between starter and ch
        let blocked = if ccc == 0 {
            false
        } else {
            // A combining character C is blocked from the starter if there's
            // a character B between them with ccc(B) >= ccc(C)
            let mut is_blocked = false;
            for j in (last_starter_idx + 1)..result.len() {
                if canonical_combining_class(result[j]) >= ccc {
                    is_blocked = true;
                    break;
                }
            }
            is_blocked
        };

        if !blocked {
            if let Some(composed) = compose_pair(result[last_starter_idx], ch) {
                result[last_starter_idx] = composed;
                continue;
            }
        }

        result.push(ch);
    }

    result.into_iter().collect()
}

/// Try to compose two characters into a single precomposed character.
/// Returns Some(composed) if the pair has a canonical composition.
fn compose_pair(a: char, b: char) -> Option<char> {
    // Hangul composition (Jamo -> Syllable)
    let l = a as u32;
    let v = b as u32;
    if (0x1100..=0x1112).contains(&l) && (0x1161..=0x1175).contains(&v) {
        // LV composition
        let l_index = l - 0x1100;
        let v_index = v - 0x1161;
        let s = 0xAC00 + (l_index * 21 + v_index) * 28;
        return char::from_u32(s);
    }
    // LVT composition (LV syllable + trailing jamo)
    if (0xAC00..=0xD7A3).contains(&l) && (l - 0xAC00).is_multiple_of(28) && (0x11A8..=0x11C2).contains(&v) {
        let t_index = v - 0x11A7;
        return char::from_u32(l + t_index);
    }

    // Common Latin precomposed character compositions
    match (a, b) {
        ('A', '\u{0300}') => Some('\u{00C0}'),
        ('A', '\u{0301}') => Some('\u{00C1}'),
        ('A', '\u{0302}') => Some('\u{00C2}'),
        ('A', '\u{0303}') => Some('\u{00C3}'),
        ('A', '\u{0308}') => Some('\u{00C4}'),
        ('A', '\u{030A}') => Some('\u{00C5}'),
        ('C', '\u{0327}') => Some('\u{00C7}'),
        ('E', '\u{0300}') => Some('\u{00C8}'),
        ('E', '\u{0301}') => Some('\u{00C9}'),
        ('E', '\u{0302}') => Some('\u{00CA}'),
        ('E', '\u{0308}') => Some('\u{00CB}'),
        ('I', '\u{0300}') => Some('\u{00CC}'),
        ('I', '\u{0301}') => Some('\u{00CD}'),
        ('I', '\u{0302}') => Some('\u{00CE}'),
        ('I', '\u{0308}') => Some('\u{00CF}'),
        ('N', '\u{0303}') => Some('\u{00D1}'),
        ('O', '\u{0300}') => Some('\u{00D2}'),
        ('O', '\u{0301}') => Some('\u{00D3}'),
        ('O', '\u{0302}') => Some('\u{00D4}'),
        ('O', '\u{0303}') => Some('\u{00D5}'),
        ('O', '\u{0308}') => Some('\u{00D6}'),
        ('U', '\u{0300}') => Some('\u{00D9}'),
        ('U', '\u{0301}') => Some('\u{00DA}'),
        ('U', '\u{0302}') => Some('\u{00DB}'),
        ('U', '\u{0308}') => Some('\u{00DC}'),
        ('Y', '\u{0301}') => Some('\u{00DD}'),
        ('a', '\u{0300}') => Some('\u{00E0}'),
        ('a', '\u{0301}') => Some('\u{00E1}'),
        ('a', '\u{0302}') => Some('\u{00E2}'),
        ('a', '\u{0303}') => Some('\u{00E3}'),
        ('a', '\u{0308}') => Some('\u{00E4}'),
        ('a', '\u{030A}') => Some('\u{00E5}'),
        ('c', '\u{0327}') => Some('\u{00E7}'),
        ('e', '\u{0300}') => Some('\u{00E8}'),
        ('e', '\u{0301}') => Some('\u{00E9}'),
        ('e', '\u{0302}') => Some('\u{00EA}'),
        ('e', '\u{0308}') => Some('\u{00EB}'),
        ('i', '\u{0300}') => Some('\u{00EC}'),
        ('i', '\u{0301}') => Some('\u{00ED}'),
        ('i', '\u{0302}') => Some('\u{00EE}'),
        ('i', '\u{0308}') => Some('\u{00EF}'),
        ('n', '\u{0303}') => Some('\u{00F1}'),
        ('o', '\u{0300}') => Some('\u{00F2}'),
        ('o', '\u{0301}') => Some('\u{00F3}'),
        ('o', '\u{0302}') => Some('\u{00F4}'),
        ('o', '\u{0303}') => Some('\u{00F5}'),
        ('o', '\u{0308}') => Some('\u{00F6}'),
        ('u', '\u{0300}') => Some('\u{00F9}'),
        ('u', '\u{0301}') => Some('\u{00FA}'),
        ('u', '\u{0302}') => Some('\u{00FB}'),
        ('u', '\u{0308}') => Some('\u{00FC}'),
        ('y', '\u{0301}') => Some('\u{00FD}'),
        ('y', '\u{0308}') => Some('\u{00FF}'),
        _ => None,
    }
}

/// CFStringTransform built-in transform identifier constants. Apple
/// publishes these as `CFStringRef` globals (`kCFStringTransform…`) in
/// `<CoreFoundation/CFString.h>`; see
/// <https://developer.apple.com/documentation/corefoundation/kcfstringtransformtolatin>.
pub const CONSTANTS: ConstantExports = &[
    (
        "_kCFStringTransformStripCombiningMarks",
        HostConstant::NSString(kCFStringTransformStripCombiningMarks),
    ),
    (
        "_kCFStringTransformToLatin",
        HostConstant::NSString(kCFStringTransformToLatin),
    ),
    (
        "_kCFStringTransformFullwidthHalfwidth",
        HostConstant::NSString(kCFStringTransformFullwidthHalfwidth),
    ),
    (
        "_kCFStringTransformLatinKatakana",
        HostConstant::NSString(kCFStringTransformLatinKatakana),
    ),
    (
        "_kCFStringTransformLatinHiragana",
        HostConstant::NSString(kCFStringTransformLatinHiragana),
    ),
    (
        "_kCFStringTransformHiraganaKatakana",
        HostConstant::NSString(kCFStringTransformHiraganaKatakana),
    ),
    (
        "_kCFStringTransformMandarinLatin",
        HostConstant::NSString(kCFStringTransformMandarinLatin),
    ),
    (
        "_kCFStringTransformLatinHangul",
        HostConstant::NSString(kCFStringTransformLatinHangul),
    ),
    (
        "_kCFStringTransformLatinArabic",
        HostConstant::NSString(kCFStringTransformLatinArabic),
    ),
    (
        "_kCFStringTransformLatinHebrew",
        HostConstant::NSString(kCFStringTransformLatinHebrew),
    ),
    (
        "_kCFStringTransformLatinThai",
        HostConstant::NSString(kCFStringTransformLatinThai),
    ),
    (
        "_kCFStringTransformLatinCyrillic",
        HostConstant::NSString(kCFStringTransformLatinCyrillic),
    ),
    (
        "_kCFStringTransformLatinGreek",
        HostConstant::NSString(kCFStringTransformLatinGreek),
    ),
    (
        "_kCFStringTransformToXMLHex",
        HostConstant::NSString(kCFStringTransformToXMLHex),
    ),
    (
        "_kCFStringTransformToUnicodeName",
        HostConstant::NSString(kCFStringTransformToUnicodeName),
    ),
    (
        "_kCFStringTransformStripDiacritics",
        HostConstant::NSString(kCFStringTransformStripDiacritics),
    ),
];

pub const FUNCTIONS: FunctionExports = &[
    // Lifecycle
    export_c_func!(CFStringRetain(_)),
    export_c_func!(CFStringRelease(_)),
    // Encoding
    export_c_func!(CFStringConvertEncodingToNSStringEncoding(_)),
    export_c_func!(CFStringConvertNSStringEncodingToEncoding(_)),
    export_c_func!(CFStringIsEncodingAvailable(_)),
    export_c_func!(CFStringGetSystemEncoding()),
    export_c_func!(CFStringGetFastestEncoding(_)),
    export_c_func!(CFStringGetSmallestEncoding(_)),
    export_c_func!(CFStringGetMostCompatibleMacStringEncoding(_)),
    // Immutable constructors
    export_c_func!(CFStringCreateCopy(_, _)),
    export_c_func!(CFStringCreateWithBytes(_, _, _, _, _)),
    export_c_func!(CFStringCreateWithBytesNoCopy(_, _, _, _, _, _)),
    export_c_func!(CFStringCreateWithCString(_, _, _)),
    export_c_func!(CFStringCreateWithCStringNoCopy(_, _, _, _)),
    export_c_func!(CFStringCreateWithCharacters(_, _, _)),
    export_c_func!(CFStringCreateWithCharactersNoCopy(_, _, _, _)),
    export_c_func!(CFStringCreateWithFormat(_, _, _, _)),
    export_c_func!(CFStringCreateWithFormatAndArguments(_, _, _, _)),
    export_c_func!(CFStringCreateWithPascalString(_, _, _)),
    export_c_func!(CFStringCreateWithSubstring(_, _, _)),
    export_c_func!(CFStringCreateArrayBySeparatingStrings(_, _, _)),
    export_c_func!(CFStringCreateByCombiningStrings(_, _, _)),
    // Mutable constructors
    export_c_func!(CFStringCreateMutable(_, _)),
    export_c_func!(CFStringCreateMutableCopy(_, _, _)),
    export_c_func!(CFStringCreateMutableWithExternalCharactersNoCopy(
        _,
        _,
        _,
        _,
        _
    )),
    // Queries
    export_c_func!(CFStringGetLength(_)),
    export_c_func!(CFStringGetCharacterAtIndex(_, _)),
    export_c_func!(CFStringGetCharacters(_, _, _)),
    export_c_func!(CFStringGetCharacterFromInlineBuffer(_, _)),
    export_c_func!(CFStringGetCString(_, _, _, _)),
    export_c_func!(CFStringGetCStringPtr(_, _)),
    export_c_func!(CFStringGetPascalString(_, _, _, _)),
    export_c_func!(CFStringGetBytes(_, _, _, _, _, _, _, _)),
    export_c_func!(CFStringGetIntValue(_)),
    export_c_func!(CFStringGetDoubleValue(_)),
    // Searching
    export_c_func!(CFStringFind(_, _, _)),
    export_c_func!(CFStringFindWithOptions(_, _, _, _, _)),
    export_c_func!(CFStringFindCharacterFromSet(_, _, _, _, _)),
    // Comparison
    export_c_func!(CFStringCompare(_, _, _)),
    export_c_func!(CFStringCompareWithOptions(_, _, _, _)),
    export_c_func!(CFStringCompareWithOptionsAndLocale(_, _, _, _, _)),
    export_c_func!(CFStringHasPrefix(_, _)),
    export_c_func!(CFStringHasSuffix(_, _)),
    // Mutation
    export_c_func!(CFStringAppend(_, _)),
    export_c_func!(CFStringAppendCString(_, _, _)),
    export_c_func!(CFStringAppendCharacters(_, _, _)),
    export_c_func!(CFStringAppendFormat(_, _, _, _)),
    export_c_func!(CFStringInsert(_, _, _)),
    export_c_func!(CFStringDelete(_, _)),
    export_c_func!(CFStringReplace(_, _, _)),
    export_c_func!(CFStringReplaceAll(_, _)),
    export_c_func!(CFStringFindAndReplace(_, _, _, _, _)),
    export_c_func!(CFStringPad(_, _, _, _)),
    export_c_func!(CFStringTrim(_, _)),
    export_c_func!(CFStringTrimWhitespace(_)),
    // Case transformations
    export_c_func!(CFStringLowercase(_, _)),
    export_c_func!(CFStringUppercase(_, _)),
    export_c_func!(CFStringCapitalize(_, _)),
    // Normalization and transformation
    export_c_func!(CFStringNormalize(_, _)),
    export_c_func!(CFStringTransform(_, _, _, _)),
    // File system representation
    export_c_func!(CFStringGetFileSystemRepresentation(_, _, _)),
    // Type info — CFStringGetTypeID is exported from cf_type; not duplicated.
    export_c_func!(CFStringCreateExternalRepresentation(_, _, _, _)),
];
