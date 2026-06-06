/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! The `NSCharacterSet` class cluster, including `NSMutableCharacterSet`.

use super::{ns_string, unichar};
use crate::frameworks::foundation::NSRange;
use crate::objc::{
    autorelease, id, msg, msg_class, nil, objc_classes, retain, ClassExports, HostObject,
    NSZonePtr,
};
use std::collections::HashSet;

// Unicode General Category Zs and CHARACTER TABULATION (U+0009).
const WHITESPACE_CHARACTERS: [char; 18] = [
    '\u{0020}', '\u{00A0}', '\u{1680}', '\u{2000}', '\u{2001}', '\u{2002}', '\u{2003}', '\u{2004}',
    '\u{2005}', '\u{2006}', '\u{2007}', '\u{2008}', '\u{2009}', '\u{200A}', '\u{202F}', '\u{205F}',
    '\u{3000}', '\u{0009}',
];
// The newline characters (U+000A - U+000D, U+0085, U+2028, and U+2029).
const NEWLINE_CHARACTERS: [char; 7] = [
    '\u{000A}', '\u{000B}', '\u{000C}', '\u{000D}', '\u{0085}', '\u{2028}', '\u{2029}',
];

// =========================================================================
// MARK: - Helpers for building sets from Unicode ranges
// =========================================================================

fn ascii_range_set(ranges: &[(u32, u32)]) -> HashSet<unichar> {
    let mut set = HashSet::new();
    for &(lo, hi) in ranges {
        for cp in lo..=hi {
            if let Ok(uc) = unichar::try_from(cp as u16) {
                set.insert(uc);
            }
        }
    }
    set
}

fn unicode_range_set(ranges: &[(u32, u32)]) -> HashSet<unichar> {
    let mut set = HashSet::new();
    for &(lo, hi) in ranges {
        for cp in lo..=hi {
            if cp <= 0xFFFF {
                if let Ok(uc) = unichar::try_from(cp as u16) {
                    set.insert(uc);
                }
            }
        }
    }
    set
}

// =========================================================================
// MARK: - Host object
// =========================================================================

#[derive(Default)]
struct CharacterSetHostObject {
    set: HashSet<unichar>,
    inverted: bool,
}
impl HostObject for CharacterSetHostObject {}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

// =========================================================================
// MARK: - NSCharacterSet
// =========================================================================

@implementation NSCharacterSet: NSObject

+ (id)allocWithZone:(NSZonePtr)zone {
    assert!(this == env.objc.get_known_class("NSCharacterSet", &mut env.mem));
    msg_class![env; _touchHLE_NSCharacterSet allocWithZone:zone]
}

// MARK: Standard character sets

+ (id)characterSetWithCharactersInString:(id)string { // NSString*
    let mut set = HashSet::new();
    ns_string::for_each_code_unit(env, string, |_idx, c| { set.insert(c); });
    let new: id = msg![env; this alloc];
    env.objc.borrow_mut::<CharacterSetHostObject>(new).set = set;
    autorelease(env, new)
}

+ (id)characterSetWithRange:(NSRange)range {
    // NSRange here is { location: first_codepoint, length: count }
    let mut set = HashSet::new();
    for cp in range.location..range.location + range.length {
        if cp <= 0xFFFF {
            if let Ok(uc) = unichar::try_from(cp as u16) {
                set.insert(uc);
            }
        }
    }
    let new: id = msg![env; this alloc];
    env.objc.borrow_mut::<CharacterSetHostObject>(new).set = set;
    autorelease(env, new)
}

+ (id)alphanumericCharacterSet {
    // A-Z, a-z, 0-9 plus Unicode letters and digits (approximated to BMP).
    let mut set = ascii_range_set(&[
        (b'A' as u32, b'Z' as u32),
        (b'a' as u32, b'z' as u32),
        (b'0' as u32, b'9' as u32),
    ]);
    // Add common Unicode letter ranges (Latin Extended, etc.)
    for s in unicode_range_set(&[
        (0x00C0, 0x00FF), // Latin Extended-A/B
        (0x0100, 0x017F),
        (0x0180, 0x024F),
        (0x0370, 0x03FF), // Greek
        (0x0400, 0x04FF), // Cyrillic
        (0x4E00, 0x9FFF), // CJK Unified Ideographs (subset)
    ]) {
        set.insert(s);
    }
    let new: id = msg![env; this alloc];
    env.objc.borrow_mut::<CharacterSetHostObject>(new).set = set;
    autorelease(env, new)
}

+ (id)letterCharacterSet {
    let mut set = ascii_range_set(&[
        (b'A' as u32, b'Z' as u32),
        (b'a' as u32, b'z' as u32),
    ]);
    for s in unicode_range_set(&[
        (0x00C0, 0x00FF),
        (0x0100, 0x017F),
        (0x0180, 0x024F),
        (0x0370, 0x03FF),
        (0x0400, 0x04FF),
    ]) {
        set.insert(s);
    }
    let new: id = msg![env; this alloc];
    env.objc.borrow_mut::<CharacterSetHostObject>(new).set = set;
    autorelease(env, new)
}

+ (id)lowercaseLetterCharacterSet {
    let mut set = ascii_range_set(&[(b'a' as u32, b'z' as u32)]);
    for s in unicode_range_set(&[
        (0x00DF, 0x00F6), (0x00F8, 0x00FF),
        (0x0101, 0x012F), (0x0131, 0x0131),
    ]) {
        set.insert(s);
    }
    let new: id = msg![env; this alloc];
    env.objc.borrow_mut::<CharacterSetHostObject>(new).set = set;
    autorelease(env, new)
}

+ (id)uppercaseLetterCharacterSet {
    let mut set = ascii_range_set(&[(b'A' as u32, b'Z' as u32)]);
    for s in unicode_range_set(&[
        (0x00C0, 0x00D6), (0x00D8, 0x00DE),
        (0x0100, 0x012E), (0x0130, 0x0130),
    ]) {
        set.insert(s);
    }
    let new: id = msg![env; this alloc];
    env.objc.borrow_mut::<CharacterSetHostObject>(new).set = set;
    autorelease(env, new)
}

+ (id)decimalDigitCharacterSet {
    let set = ascii_range_set(&[(b'0' as u32, b'9' as u32)]);
    let new: id = msg![env; this alloc];
    env.objc.borrow_mut::<CharacterSetHostObject>(new).set = set;
    autorelease(env, new)
}

+ (id)newlineCharacterSet {
    let set = HashSet::from(NEWLINE_CHARACTERS.map(|c| unichar::try_from(c as u16).unwrap()));
    let new: id = msg![env; this alloc];
    env.objc.borrow_mut::<CharacterSetHostObject>(new).set = set;
    autorelease(env, new)
}

+ (id)whitespaceCharacterSet {
    let set = HashSet::from(WHITESPACE_CHARACTERS.map(|c| unichar::try_from(c as u16).unwrap()));
    let new: id = msg![env; this alloc];
    env.objc.borrow_mut::<CharacterSetHostObject>(new).set = set;
    autorelease(env, new)
}

+ (id)whitespaceAndNewlineCharacterSet {
    let set1: HashSet<unichar> = HashSet::from(NEWLINE_CHARACTERS.map(|c| unichar::try_from(c as u16).unwrap()));
    let set2: HashSet<unichar> = HashSet::from(WHITESPACE_CHARACTERS.map(|c| unichar::try_from(c as u16).unwrap()));
    let set = set1.union(&set2).copied().collect();
    let new: id = msg![env; this alloc];
    env.objc.borrow_mut::<CharacterSetHostObject>(new).set = set;
    autorelease(env, new)
}

+ (id)punctuationCharacterSet {
    let set = ascii_range_set(&[
        (0x0021, 0x002F), // !"#$%&'()*+,-./
        (0x003A, 0x0040), // :;<=>?@
        (0x005B, 0x0060), // [\]^_`
        (0x007B, 0x007E), // {|}~
    ]);
    let new: id = msg![env; this alloc];
    env.objc.borrow_mut::<CharacterSetHostObject>(new).set = set;
    autorelease(env, new)
}

+ (id)symbolCharacterSet {
    let set = unicode_range_set(&[
        (0x2000, 0x206F), // General Punctuation
        (0x2100, 0x214F), // Letterlike Symbols
        (0x2190, 0x21FF), // Arrows
        (0x2200, 0x22FF), // Mathematical Operators
        (0x2300, 0x23FF), // Miscellaneous Technical
        (0x25A0, 0x25FF), // Geometric Shapes
        (0x2600, 0x26FF), // Miscellaneous Symbols
        (0x2700, 0x27BF), // Dingbats
    ]);
    let new: id = msg![env; this alloc];
    env.objc.borrow_mut::<CharacterSetHostObject>(new).set = set;
    autorelease(env, new)
}

+ (id)controlCharacterSet {
    let mut set = ascii_range_set(&[
        (0x0000, 0x001F), // C0 controls
        (0x007F, 0x009F), // DEL + C1 controls
    ]);
    set.insert(unichar::try_from(0x007Fu16).unwrap()); // DEL
    let new: id = msg![env; this alloc];
    env.objc.borrow_mut::<CharacterSetHostObject>(new).set = set;
    autorelease(env, new)
}

+ (id)nonBaseCharacterSet {
    // Combining characters (approximate — marks and combining diacritics).
    let set = unicode_range_set(&[
        (0x0300, 0x036F), // Combining Diacritical Marks
        (0x1DC0, 0x1DFF), // Combining Diacritical Marks Supplement
        (0x20D0, 0x20FF), // Combining Diacritical Marks for Symbols
        (0xFE20, 0xFE2F), // Combining Half Marks
    ]);
    let new: id = msg![env; this alloc];
    env.objc.borrow_mut::<CharacterSetHostObject>(new).set = set;
    autorelease(env, new)
}

+ (id)decomposableCharacterSet {
    log!("TODO: [NSCharacterSet decomposableCharacterSet] — returning empty set");
    let new: id = msg![env; this alloc];
    autorelease(env, new)
}

+ (id)illegalCharacterSet {
    // Surrogates and non-characters.
    let set = unicode_range_set(&[
        (0xD800, 0xDFFF), // Surrogates
        (0xFDD0, 0xFDEF), // Non-characters
        (0xFFFE, 0xFFFF), // BOM / non-character
    ]);
    let new: id = msg![env; this alloc];
    env.objc.borrow_mut::<CharacterSetHostObject>(new).set = set;
    autorelease(env, new)
}

+ (id)URLHostAllowedCharacterSet {
    // RFC 3986 host characters.
    let mut set = ascii_range_set(&[
        (b'A' as u32, b'Z' as u32),
        (b'a' as u32, b'z' as u32),
        (b'0' as u32, b'9' as u32),
    ]);
    for ch in b"-._~!$&'()*+,;=[]:" {
        set.insert(unichar::try_from(*ch as u16).unwrap());
    }
    let new: id = msg![env; this alloc];
    env.objc.borrow_mut::<CharacterSetHostObject>(new).set = set;
    autorelease(env, new)
}

+ (id)URLPathAllowedCharacterSet {
    let mut set = ascii_range_set(&[
        (b'A' as u32, b'Z' as u32),
        (b'a' as u32, b'z' as u32),
        (b'0' as u32, b'9' as u32),
    ]);
    for ch in b"-._~!$&'()*+,;=:@/" {
        set.insert(unichar::try_from(*ch as u16).unwrap());
    }
    let new: id = msg![env; this alloc];
    env.objc.borrow_mut::<CharacterSetHostObject>(new).set = set;
    autorelease(env, new)
}

+ (id)URLQueryAllowedCharacterSet {
    let mut set = ascii_range_set(&[
        (b'A' as u32, b'Z' as u32),
        (b'a' as u32, b'z' as u32),
        (b'0' as u32, b'9' as u32),
    ]);
    for ch in b"-._~!$&'()*+,;=:@/?%" {
        set.insert(unichar::try_from(*ch as u16).unwrap());
    }
    let new: id = msg![env; this alloc];
    env.objc.borrow_mut::<CharacterSetHostObject>(new).set = set;
    autorelease(env, new)
}

+ (id)URLFragmentAllowedCharacterSet {
    let mut set = ascii_range_set(&[
        (b'A' as u32, b'Z' as u32),
        (b'a' as u32, b'z' as u32),
        (b'0' as u32, b'9' as u32),
    ]);
    for ch in b"-._~!$&'()*+,;=:@/?" {
        set.insert(unichar::try_from(*ch as u16).unwrap());
    }
    let new: id = msg![env; this alloc];
    env.objc.borrow_mut::<CharacterSetHostObject>(new).set = set;
    autorelease(env, new)
}

+ (id)URLUserAllowedCharacterSet {
    let mut set = ascii_range_set(&[
        (b'A' as u32, b'Z' as u32),
        (b'a' as u32, b'z' as u32),
        (b'0' as u32, b'9' as u32),
    ]);
    for ch in b"-._~!$&'()*+,;=" {
        set.insert(unichar::try_from(*ch as u16).unwrap());
    }
    let new: id = msg![env; this alloc];
    env.objc.borrow_mut::<CharacterSetHostObject>(new).set = set;
    autorelease(env, new)
}

+ (id)URLPasswordAllowedCharacterSet {
    msg![env; this URLUserAllowedCharacterSet]
}

// MARK: NSCopying / NSMutableCopying

- (id)copyWithZone:(NSZonePtr)_zone {
    retain(env, this)
}

- (id)mutableCopyWithZone:(NSZonePtr)_zone {
    let host = env.objc.borrow::<CharacterSetHostObject>(this);
    let new_host = Box::new(CharacterSetHostObject {
        set: host.set.clone(),
        inverted: host.inverted,
    });
    let class = env.objc.get_known_class("_touchHLE_NSMutableCharacterSet", &mut env.mem);
    let new = env.objc.alloc_object(class, new_host, &mut env.mem);
    autorelease(env, new)
}

@end

// =========================================================================
// MARK: - _touchHLE_NSCharacterSet
// =========================================================================

@implementation _touchHLE_NSCharacterSet: NSCharacterSet

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(CharacterSetHostObject {
        set: HashSet::new(),
        inverted: false,
    });
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

- (bool)characterIsMember:(unichar)code_unit {
    let host_object = env.objc.borrow::<CharacterSetHostObject>(this);
    host_object.set.contains(&code_unit) ^ host_object.inverted
}

- (bool)hasMemberInPlane:(u8)plane {
    if plane != 0 {
        // We only track BMP (plane 0) characters.
        return false;
    }
    let host = env.objc.borrow::<CharacterSetHostObject>(this);
    !host.set.is_empty() ^ host.inverted
}

- (bool)isSupersetOfSet:(id)other { // NSCharacterSet*
    // Check that every member of `other` is also a member of `this`.
    // We iterate over all BMP code points that `other` contains.
    // For performance we only test the other's explicit set members.
    let other_set: HashSet<unichar> = {
        let h = env.objc.borrow::<CharacterSetHostObject>(other);
        h.set.clone()
    };
    let other_inverted = env.objc.borrow::<CharacterSetHostObject>(other).inverted;
    let self_host = env.objc.borrow::<CharacterSetHostObject>(this);

    if other_inverted {
        // other is an inverted set — too large to iterate; log and return
        // false.
        log_dbg!("isSupersetOfSet: other is inverted, returning false (not supported)");
        return false;
    }
    for &cp in &other_set {
        let is_member = self_host.set.contains(&cp) ^ self_host.inverted;
        if !is_member {
            return false;
        }
    }
    true
}

- (bool)isEqual:(id)other {
    if this == other { return true; }
    if other == nil { return false; }
    // Check that other is also a character set before borrowing.
    let cs_class = env.objc.get_known_class("_touchHLE_NSCharacterSet", &mut env.mem);
    let mcs_class = env.objc.get_known_class("_touchHLE_NSMutableCharacterSet", &mut env.mem);
    let other_class: id = msg![env; other class];
    let is_cs: bool = msg![env; other_class isSubclassOfClass:cs_class];
    let is_mcs: bool = msg![env; other_class isSubclassOfClass:mcs_class];
    if !is_cs && !is_mcs {
        return false;
    }
    let a_set: HashSet<unichar>;
    let a_inv: bool;
    {
        let a = env.objc.borrow::<CharacterSetHostObject>(this);
        a_set = a.set.clone();
        a_inv = a.inverted;
    }
    let b = env.objc.borrow::<CharacterSetHostObject>(other);
    a_set == b.set && a_inv == b.inverted
}

- (id)invertedSet {
    let old = env.objc.borrow::<CharacterSetHostObject>(this);
    let new_host = Box::new(CharacterSetHostObject {
        set: old.set.clone(),
        inverted: !old.inverted,
    });
    let class = env.objc.get_known_class("_touchHLE_NSCharacterSet", &mut env.mem);
    let new = env.objc.alloc_object(class, new_host, &mut env.mem);
    autorelease(env, new)
}

- (id)mutableCopyWithZone:(NSZonePtr)_zone {
    let old = env.objc.borrow::<CharacterSetHostObject>(this);
    let new_host = Box::new(CharacterSetHostObject {
        set: old.set.clone(),
        inverted: old.inverted,
    });
    let class = env.objc.get_known_class("_touchHLE_NSMutableCharacterSet", &mut env.mem);
    let new = env.objc.alloc_object(class, new_host, &mut env.mem);
    autorelease(env, new)
}

@end

// =========================================================================
// MARK: - NSMutableCharacterSet
// =========================================================================

@implementation NSMutableCharacterSet: NSCharacterSet

+ (id)allocWithZone:(NSZonePtr)zone {
    assert!(this == env.objc.get_known_class("NSMutableCharacterSet", &mut env.mem));
    msg_class![env; _touchHLE_NSMutableCharacterSet allocWithZone:zone]
}

// NSMutableCopying — mutable copy is also mutable.
- (id)mutableCopyWithZone:(NSZonePtr)_zone {
    retain(env, this)
}

// Immutable copy.
- (id)copyWithZone:(NSZonePtr)_zone {
    let host = env.objc.borrow::<CharacterSetHostObject>(this);
    let new_host = Box::new(CharacterSetHostObject {
        set: host.set.clone(),
        inverted: host.inverted,
    });
    let class = env.objc.get_known_class("_touchHLE_NSCharacterSet", &mut env.mem);
    let new = env.objc.alloc_object(class, new_host, &mut env.mem);
    autorelease(env, new)
}

@end

// =========================================================================
// MARK: - _touchHLE_NSMutableCharacterSet
// =========================================================================

@implementation _touchHLE_NSMutableCharacterSet: NSMutableCharacterSet

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(CharacterSetHostObject {
        set: HashSet::new(),
        inverted: false,
    });
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

- (bool)characterIsMember:(unichar)code_unit {
    let host_object = env.objc.borrow::<CharacterSetHostObject>(this);
    host_object.set.contains(&code_unit) ^ host_object.inverted
}

// MARK: Mutation

- (())addCharactersInString:(id)string { // NSString*
    let mut chars = Vec::new();
    ns_string::for_each_code_unit(env, string, |_idx, c| chars.push(c));
    let host = env.objc.borrow_mut::<CharacterSetHostObject>(this);
    for c in chars { host.set.insert(c); }
}

- (())removeCharactersInString:(id)string { // NSString*
    let mut chars = Vec::new();
    ns_string::for_each_code_unit(env, string, |_idx, c| chars.push(c));
    let host = env.objc.borrow_mut::<CharacterSetHostObject>(this);
    for c in chars { host.set.remove(&c); }
}

- (())addCharactersInRange:(NSRange)range {
    let host = env.objc.borrow_mut::<CharacterSetHostObject>(this);
    for cp in range.location..range.location + range.length {
        if cp <= 0xFFFF {
            if let Ok(uc) = unichar::try_from(cp as u16) {
                host.set.insert(uc);
            }
        }
    }
}

- (())removeCharactersInRange:(NSRange)range {
    let host = env.objc.borrow_mut::<CharacterSetHostObject>(this);
    for cp in range.location..range.location + range.length {
        if cp <= 0xFFFF {
            if let Ok(uc) = unichar::try_from(cp as u16) {
                host.set.remove(&uc);
            }
        }
    }
}

- (())unionWithCharacterSet:(id)other { // NSCharacterSet*
    let other_chars: Vec<unichar> = {
        let h = env.objc.borrow::<CharacterSetHostObject>(other);
        h.set.iter().copied().collect()
    };
    let host = env.objc.borrow_mut::<CharacterSetHostObject>(this);
    for c in other_chars { host.set.insert(c); }
}

- (())intersectWithCharacterSet:(id)other { // NSCharacterSet*
    let other_set: HashSet<unichar> = {
        let h = env.objc.borrow::<CharacterSetHostObject>(other);
        h.set.clone()
    };
    let host = env.objc.borrow_mut::<CharacterSetHostObject>(this);
    host.set.retain(|c| other_set.contains(c));
}

- (())invert {
    let host = env.objc.borrow_mut::<CharacterSetHostObject>(this);
    host.inverted = !host.inverted;
}

- (id)invertedSet {
    let old = env.objc.borrow::<CharacterSetHostObject>(this);
    let new_host = Box::new(CharacterSetHostObject {
        set: old.set.clone(),
        inverted: !old.inverted,
    });
    let class = env.objc.get_known_class("_touchHLE_NSCharacterSet", &mut env.mem);
    let new = env.objc.alloc_object(class, new_host, &mut env.mem);
    autorelease(env, new)
}

- (bool)characterIsMemberOfSet:(unichar)code_unit {
    let host_object = env.objc.borrow::<CharacterSetHostObject>(this);
    host_object.set.contains(&code_unit) ^ host_object.inverted
}

@end

};
