/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `CFUUID` — Core Foundation UUID type.

use crate::dyld::{export_c_func, FunctionExports};
use crate::frameworks::core_foundation::cf_allocator::CFAllocatorRef;
use crate::frameworks::core_foundation::cf_string::CFStringRef;
use crate::frameworks::core_foundation::{CFRelease, CFRetain, CFTypeRef};
use crate::frameworks::core_graphics::{CGPoint, CGRect, CGSize};
use crate::frameworks::foundation::ns_string;
use crate::mem::SafeRead;
use crate::objc::{autorelease, nil, objc_classes, ClassExports, HostObject};
use crate::Environment;

pub type CFUUIDRef = CFTypeRef;

/// 16-byte UUID stored as raw bytes.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
#[repr(C, packed)]
pub struct CFUUIDBytes {
    pub byte0: u8,
    pub byte1: u8,
    pub byte2: u8,
    pub byte3: u8,
    pub byte4: u8,
    pub byte5: u8,
    pub byte6: u8,
    pub byte7: u8,
    pub byte8: u8,
    pub byte9: u8,
    pub byte10: u8,
    pub byte11: u8,
    pub byte12: u8,
    pub byte13: u8,
    pub byte14: u8,
    pub byte15: u8,
}
unsafe impl SafeRead for CFUUIDBytes {}

#[derive(Default)]
struct CFUUIDHostObject {
    bytes: CFUUIDBytes,
}
impl HostObject for CFUUIDHostObject {}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation _touchHLE_CFUUID: NSObject
- (())dealloc {
    env.objc.dealloc_object(this, &mut env.mem)
}
@end

};

// MARK: - Internal helpers

fn alloc_uuid(env: &mut Environment, bytes: CFUUIDBytes) -> CFUUIDRef {
    let class = env.objc.get_known_class("_touchHLE_CFUUID", &mut env.mem);
    env.objc
        .alloc_object(class, Box::new(CFUUIDHostObject { bytes }), &mut env.mem)
}

/// Generate a pseudo-random UUID using a simple LCG seeded from the system
/// time. Not cryptographically secure, but sufficient for game use-cases.
fn generate_uuid_bytes(_env: &mut Environment) -> CFUUIDBytes {
    use std::time::{SystemTime, UNIX_EPOCH};
    // Use a static counter combined with time to reduce collisions within
    // a single session.
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let count = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
        ^ (count
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407));

    // Fill 16 bytes from two LCG steps.
    let a = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    let b = a
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);

    let mut raw = [0u8; 16];
    raw[..8].copy_from_slice(&a.to_le_bytes());
    raw[8..].copy_from_slice(&b.to_le_bytes());

    // Set version 4 (random) and variant bits.
    raw[6] = (raw[6] & 0x0f) | 0x40; // version 4
    raw[8] = (raw[8] & 0x3f) | 0x80; // variant 10xx

    CFUUIDBytes {
        byte0: raw[0],
        byte1: raw[1],
        byte2: raw[2],
        byte3: raw[3],
        byte4: raw[4],
        byte5: raw[5],
        byte6: raw[6],
        byte7: raw[7],
        byte8: raw[8],
        byte9: raw[9],
        byte10: raw[10],
        byte11: raw[11],
        byte12: raw[12],
        byte13: raw[13],
        byte14: raw[14],
        byte15: raw[15],
    }
}

fn bytes_to_string(b: &CFUUIDBytes) -> String {
    format!(
        "{:02X}{:02X}{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
        b.byte0,  b.byte1,  b.byte2,  b.byte3,
        b.byte4,  b.byte5,
        b.byte6,  b.byte7,
        b.byte8,  b.byte9,
        b.byte10, b.byte11, b.byte12, b.byte13, b.byte14, b.byte15,
    )
}

fn parse_uuid_string(s: &str) -> Option<CFUUIDBytes> {
    // Accept "XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX" (with or without braces).
    let s = s.trim().trim_start_matches('{').trim_end_matches('}');
    let hex: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if hex.len() != 32 {
        return None;
    }
    let mut raw = [0u8; 16];
    for i in 0..16 {
        raw[i] = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(CFUUIDBytes {
        byte0: raw[0],
        byte1: raw[1],
        byte2: raw[2],
        byte3: raw[3],
        byte4: raw[4],
        byte5: raw[5],
        byte6: raw[6],
        byte7: raw[7],
        byte8: raw[8],
        byte9: raw[9],
        byte10: raw[10],
        byte11: raw[11],
        byte12: raw[12],
        byte13: raw[13],
        byte14: raw[14],
        byte15: raw[15],
    })
}

// MARK: - Lifecycle

pub fn CFUUIDRetain(env: &mut Environment, uuid: CFUUIDRef) -> CFUUIDRef {
    if !uuid.is_null() {
        CFRetain(env, uuid)
    } else {
        uuid
    }
}

pub fn CFUUIDRelease(env: &mut Environment, uuid: CFUUIDRef) {
    if !uuid.is_null() {
        CFRelease(env, uuid);
    }
}

// MARK: - Constructors

fn CFUUIDCreate(env: &mut Environment, _allocator: CFAllocatorRef) -> CFUUIDRef {
    let bytes = generate_uuid_bytes(env);
    log_dbg!("CFUUIDCreate -> {}", bytes_to_string(&bytes));
    alloc_uuid(env, bytes)
}

// Replace both functions:

fn CFUUIDCreateWithBytes(
    env: &mut Environment,
    _allocator: CFAllocatorRef,
    bytes: crate::mem::ConstPtr<CFUUIDBytes>,
) -> CFUUIDRef {
    let b = env.mem.read(bytes);
    alloc_uuid(env, b)
}

fn CFUUIDCreateFromString(
    env: &mut Environment,
    _allocator: CFAllocatorRef,
    uuid_string: CFStringRef,
) -> CFUUIDRef {
    if uuid_string.is_null() {
        return nil;
    }
    let s = ns_string::to_rust_string(env, uuid_string).into_owned();
    match parse_uuid_string(&s) {
        Some(bytes) => alloc_uuid(env, bytes),
        None => {
            log!("CFUUIDCreateFromString: invalid UUID string {:?}", s);
            nil
        }
    }
}

fn CFUUIDCreateFromUUIDBytes(
    env: &mut Environment,
    _allocator: CFAllocatorRef,
    w0: u32,
    w1: u32,
    w2: u32,
    w3: u32,
) -> CFUUIDRef {
    let mut raw = [0u8; 16];
    raw[0..4].copy_from_slice(&w0.to_le_bytes());
    raw[4..8].copy_from_slice(&w1.to_le_bytes());
    raw[8..12].copy_from_slice(&w2.to_le_bytes());
    raw[12..16].copy_from_slice(&w3.to_le_bytes());

    let bytes = CFUUIDBytes {
        byte0: raw[0],
        byte1: raw[1],
        byte2: raw[2],
        byte3: raw[3],
        byte4: raw[4],
        byte5: raw[5],
        byte6: raw[6],
        byte7: raw[7],
        byte8: raw[8],
        byte9: raw[9],
        byte10: raw[10],
        byte11: raw[11],
        byte12: raw[12],
        byte13: raw[13],
        byte14: raw[14],
        byte15: raw[15],
    };

    alloc_uuid(env, bytes)
}

fn CFUUIDCreateString(
    env: &mut Environment,
    _allocator: CFAllocatorRef,
    uuid: CFUUIDRef,
) -> CFStringRef {
    if uuid.is_null() {
        return nil;
    }
    let bytes = env.objc.borrow::<CFUUIDHostObject>(uuid);
    let s = bytes_to_string(&bytes.bytes);
    let ns = ns_string::from_rust_string(env, s);
    // CFCreateString returns +1; autorelease so callers that CFRelease it
    // and callers that don't both work safely.
    autorelease(env, ns)
}

// 2. Return a CGRect (16 bytes) which already implements GuestRet
fn CFUUIDGetUUIDBytes(env: &mut Environment, uuid: CFUUIDRef) -> CGRect {
    let b = if uuid.is_null() {
        CFUUIDBytes {
            byte0: 0,
            byte1: 0,
            byte2: 0,
            byte3: 0,
            byte4: 0,
            byte5: 0,
            byte6: 0,
            byte7: 0,
            byte8: 0,
            byte9: 0,
            byte10: 0,
            byte11: 0,
            byte12: 0,
            byte13: 0,
            byte14: 0,
            byte15: 0,
        }
    } else {
        env.objc.borrow::<CFUUIDHostObject>(uuid).bytes
    };

    // Pack the 16 bytes into four 32-bit words
    let w0 = u32::from_le_bytes([b.byte0, b.byte1, b.byte2, b.byte3]);
    let w1 = u32::from_le_bytes([b.byte4, b.byte5, b.byte6, b.byte7]);
    let w2 = u32::from_le_bytes([b.byte8, b.byte9, b.byte10, b.byte11]);
    let w3 = u32::from_le_bytes([b.byte12, b.byte13, b.byte14, b.byte15]);

    // Disguise it as a CGRect to satisfy the GuestRet trait
    CGRect {
        origin: CGPoint {
            x: f32::from_bits(w0),
            y: f32::from_bits(w1),
        },
        size: CGSize {
            width: f32::from_bits(w2),
            height: f32::from_bits(w3),
        },
    }
}

fn CFUUIDGetConstantUUIDWithBytes(
    env: &mut Environment,
    _allocator: CFAllocatorRef,
    bytes: crate::mem::ConstPtr<CFUUIDBytes>,
) -> CFUUIDRef {
    let b = env.mem.read(bytes);
    alloc_uuid(env, b)
}

pub const FUNCTIONS: FunctionExports = &[
    export_c_func!(CFUUIDRetain(_)),
    export_c_func!(CFUUIDRelease(_)),
    export_c_func!(CFUUIDCreate(_)),
    export_c_func!(CFUUIDCreateWithBytes(_, _)),
    export_c_func!(CFUUIDCreateFromString(_, _)),
    export_c_func!(CFUUIDCreateFromUUIDBytes(_, _, _, _, _)),
    export_c_func!(CFUUIDCreateString(_, _)),
    export_c_func!(CFUUIDGetUUIDBytes(_)),
    export_c_func!(CFUUIDGetConstantUUIDWithBytes(_, _)),
];
