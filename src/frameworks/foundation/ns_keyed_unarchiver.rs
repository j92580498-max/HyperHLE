/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `NSKeyedUnarchiver` and deserialization of its object graph format.
//!
//! Resources:
//! - You can get a good intuitive grasp of how the format works just by staring
//!   at a pretty-print of a simple nib file from something that can parse
//!   plists, e.g. `plutil -p` or `println!("{:#?}", plist::Value::...);`.
//! - Apple's [Archives and Serializations Programming Guide](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/Archiving/Articles/archives.html)

use super::ns_string::{from_rust_string, get_static_str, to_rust_string};
use crate::dyld::{ConstantExports, HostConstant};
use crate::frameworks::core_graphics::{CGPoint, CGRect, CGSize};
use crate::frameworks::foundation::{NSInteger, NSUInteger};
use crate::frameworks::uikit::ui_geometry::{
    CGPointFromString, CGRectFromString, CGSizeFromString,
};
use crate::mem::{ConstPtr, ConstVoidPtr, GuestUSize, MutPtr, MutVoidPtr};
use crate::objc::{
    autorelease, id, msg, msg_class, nil, objc_classes, release, retain, ClassExports, HostObject,
    NSZonePtr,
};
use crate::Environment;
use plist::{Dictionary, Uid, Value};
use std::io::Cursor;

pub const NSKeyedArchiveRootObjectKey: &str = "root";

pub const CONSTANTS: ConstantExports = &[(
    "_NSKeyedArchiveRootObjectKey",
    HostConstant::NSString(NSKeyedArchiveRootObjectKey),
)];

#[derive(Default)]
struct NSKeyedUnarchiverHostObject {
    plist: Dictionary,
    current_key: Option<Uid>,
    /// linear map of Uid => id
    already_unarchived: Vec<Option<id>>,
    /// Something responding to NSKeyedUnarchiverDelegate
    delegate: id,
    /// Stores the buffers decoded by `decodeBytesForKey:returnedLength:`
    /// Instead of reusing the same buffer, we allocate different ones that get
    /// freed on dealloc. A similar behavior has been observed in real iOS.
    temporary_buffers: Vec<MutVoidPtr>,
}
impl HostObject for NSKeyedUnarchiverHostObject {}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation NSKeyedUnarchiver: NSCoder

+ (id)allocWithZone:(NSZonePtr)_zone { // struct _NSZone*
    let unarchiver = Box::new(NSKeyedUnarchiverHostObject {
        plist: Dictionary::new(),
        current_key: None,
        already_unarchived: Vec::new(),
        delegate: nil,
        temporary_buffers: Vec::new(),
    });
    env.objc.alloc_object(this, unarchiver, &mut env.mem)
}

+ (id)unarchiveObjectWithFile:(id)path { // NSString *
    let data: id = msg_class![env; NSData dataWithContentsOfFile:path];
    if data == nil {
        return nil;
    }
    msg![env; this unarchiveObjectWithData:data]
}

+ (id)unarchiveObjectWithData:(id)data { // NSData *
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initForReadingWithData:data];
    let root_key = get_static_str(env, NSKeyedArchiveRootObjectKey);
    let result: id = msg![env; new decodeObjectForKey:root_key];
    autorelease(env, result)
}

// TODO: other init methods.

- (id)initForReadingWithData:(id)data { // NSData *
    if data == nil {
        release(env, this);
        return nil;
    }

    let length: NSUInteger = msg![env; data length];
    let bytes: ConstVoidPtr = msg![env; data bytes];

    // 1. Честная проверка на пустые данные или null-указатель
    if length == 0 || bytes.is_null() {
        log!("Warning: [NSKeyedUnarchiver initForReadingWithData:] called with empty data. Returning nil.");
        release(env, this);
        return nil;
    }

    let slice = env.mem.bytes_at(bytes.cast(), length);

    // 2. Безопасный парсинг plist вместо жесткого .unwrap()
    let plist = match Value::from_reader(Cursor::new(slice)) {
        Ok(p) => p,
        Err(e) => {
            log!("Warning: [NSKeyedUnarchiver initForReadingWithData:] failed to parse plist: {:?}", e);
            release(env, this);
            return nil;
        }
    };

    let plist = match plist.into_dictionary() {
        Some(d) => d,
        None => {
            log!("Warning: [NSKeyedUnarchiver initForReadingWithData:] root is not a dictionary.");
            release(env, this);
            return nil;
        }
    };

    // 3. Безопасная проверка версии и типа архива
    if plist.get("$version").and_then(|v| v.as_unsigned_integer()) != Some(100000) {
        log!("Warning: [NSKeyedUnarchiver initForReadingWithData:] unsupported archiver version.");
        release(env, this);
        return nil;
    }

    if plist.get("$archiver").and_then(|v| v.as_string()) != Some("NSKeyedArchiver") {
        log!("Warning: [NSKeyedUnarchiver initForReadingWithData:] unsupported archiver type.");
        release(env, this);
        return nil;
    }

    let key_count = plist.get("$objects").and_then(|v| v.as_array()).map_or(0, |a| a.len());

    // 4. Инициализация объекта (borrow_mut вызывается только ПОСЛЕ всех
    // проверок)
    let host_obj = env.objc.borrow_mut::<NSKeyedUnarchiverHostObject>(this);
    assert!(host_obj.already_unarchived.is_empty());
    assert!(host_obj.current_key.is_none());
    assert!(host_obj.plist.is_empty());

    host_obj.already_unarchived = vec![None; key_count];
    host_obj.plist = plist;

    this
}

- (())dealloc {
    let host_obj = borrow_host_obj(env, this);
    let already_unarchived = std::mem::take(&mut host_obj.already_unarchived);
    let temporary_buffers = std::mem::take(&mut host_obj.temporary_buffers);

    for &object in already_unarchived.iter().flatten() {
        release(env, object);
    }

    for &buffer in temporary_buffers.iter() {
        env.mem.free(buffer);
    }

    env.objc.dealloc_object(this, &mut env.mem)
}

// TODO: implement calls to delegate methods
// weak/non-retaining
- (())setDelegate:(id)delegate { // id<NSKeyedUnarchiverDelegate>
    let host_object = env.objc.borrow_mut::<NSKeyedUnarchiverHostObject>(this);
    host_object.delegate = delegate;
}
- (id)delegate {
    env.objc.borrow::<NSKeyedUnarchiverHostObject>(this).delegate
}

// These methods drive most of the decoding. They get called in two cases:
// - By the code that initiates the unarchival, e.g. UINib, to retrieve
//   top-level objects.
// - By the object currently being unarchived, i.e. something that had
//   `initWithCoder:` called on it, to retrieve objects from its scope.
// They are all from the NSCoder abstract class and they return default values
// if the key is unknown.

- (bool)decodeBoolForKey:(id)key { // NSString *
    let Some(value) = get_value_to_decode_for_key(env, this, key) else { return false; };
    if let Some(b) = value.as_boolean() { return b; }
    if let Some(i) = value.as_signed_integer() { return i != 0; }
    if let Some(u) = value.as_unsigned_integer() { return u != 0; }
    log!("Warning: decodeBoolForKey: non-boolean value {:?}; returning false.", value);
    false
}

- (f64)decodeDoubleForKey:(id)key { // NSString *
    let Some(value) = get_value_to_decode_for_key(env, this, key) else { return 0.0; };
    if let Some(r) = value.as_real() { return r; }
    if let Some(i) = value.as_signed_integer() { return i as f64; }
    if let Some(u) = value.as_unsigned_integer() { return u as f64; }
    log!("Warning: decodeDoubleForKey: non-numeric value {:?}; returning 0.0.", value);
    0.0
}

- (f32)decodeFloatForKey:(id)key { // NSString *
    let Some(value) = get_value_to_decode_for_key(env, this, key) else { return 0.0; };
    if let Some(r) = value.as_real() { return r as f32; }
    if let Some(i) = value.as_signed_integer() { return i as f32; }
    if let Some(u) = value.as_unsigned_integer() { return u as f32; }
    log!("Warning: decodeFloatForKey: non-numeric value {:?}; returning 0.0.", value);
    0.0
}

- (NSInteger)decodeIntegerForKey:(id)key { // NSString *
    let Some(value) = get_value_to_decode_for_key(env, this, key) else { return 0; };
    let Some(i) = value.as_signed_integer() else {
        log!("Warning: decodeIntegerForKey: non-integer value {:?}; returning 0.", value);
        return 0;
    };
    // Clamp to NSInteger range instead of panicking.
    if i > NSInteger::MAX as i64 { return NSInteger::MAX; }
    if i < NSInteger::MIN as i64 { return NSInteger::MIN; }
    i as NSInteger
}

- (i32)decodeIntForKey:(id)key { // NSString *
    let Some(value) = get_value_to_decode_for_key(env, this, key) else { return 0; };
    let Some(i) = value.as_signed_integer() else {
        log!("Warning: decodeIntForKey: non-integer value {:?}; returning 0.", value);
        return 0;
    };
    if i > i32::MAX as i64 { return i32::MAX; }
    if i < i32::MIN as i64 { return i32::MIN; }
    i as i32
}

- (i32)decodeInt32ForKey:(id)key { // NSString *
    let Some(value) = get_value_to_decode_for_key(env, this, key) else { return 0; };
    let Some(i) = value.as_signed_integer() else {
        log!("Warning: decodeInt32ForKey: non-integer value {:?}; returning 0.", value);
        return 0;
    };
    if i > i32::MAX as i64 { return i32::MAX; }
    if i < i32::MIN as i64 { return i32::MIN; }
    i as i32
}

- (i64)decodeInt64ForKey:(id)key { // NSString *
    let Some(value) = get_value_to_decode_for_key(env, this, key) else { return 0; };
    let Some(i) = value.as_signed_integer() else {
        log!("Warning: decodeInt64ForKey: non-integer value {:?}; returning 0.", value);
        return 0;
    };
    i
}

- (id)decodeObjectForKey:(id)key { // NSString*
    let Some(next_uid) = get_value_to_decode_for_key(env, this, key) else {
        return nil;
    };
    let Some(next_uid) = next_uid.as_uid().copied() else {
        log!("Warning: decodeObjectForKey: value {:?} is not a UID; returning nil.", next_uid);
        return nil;
    };
    let object = unarchive_key(env, this, next_uid);

    // on behalf of the caller
    retain(env, object);
    autorelease(env, object)
}

- (ConstPtr<u8>)decodeBytesForKey:(id)key returnedLength:(MutPtr<NSUInteger>)length {
    if key == nil {
        env.mem.write(length, 0);
        return ConstPtr::null();
    }
    let Some(data) = get_value_to_decode_for_key(env, this, key)
        .and_then(|value| value.as_data())
        .map(|data| data.to_vec()) else {
            env.mem.write(length, 0);
            return ConstPtr::null();
    };
    let len: GuestUSize = match data.len().try_into() {
        Ok(l) => l,
        Err(_) => {
            log!("Warning: decodeBytesForKey: data of length {} exceeds u32; truncating.", data.len());
            GuestUSize::MAX
        }
    };
    let guest_bytes: MutVoidPtr = env.mem.alloc(len);
    env.objc.borrow_mut::<NSKeyedUnarchiverHostObject>(this)
        .temporary_buffers
        .push(guest_bytes);
    let copy_len = std::cmp::min(len as usize, data.len());
    env.mem
        .bytes_at_mut(guest_bytes.cast(), copy_len as GuestUSize)
        .copy_from_slice(&data[..copy_len]);
    env.mem.write(length, len);
    guest_bytes.cast().cast_const()
}

- (bool)containsValueForKey:(id)key { // NSString*
    if key == nil { return false; }
    get_value_to_decode_for_key(env, this, key).is_some()
}

// TODO: add more decode methods

// These come from a category in UIKit's UIGeometry.h
- (CGPoint)decodeCGPointForKey:(id)key { // NSString*
    let string: id = msg![env; this decodeObjectForKey:key];
    CGPointFromString(env, string)
}
- (CGSize)decodeCGSizeForKey:(id)key { // NSString*
    let string: id = msg![env; this decodeObjectForKey:key];
    CGSizeFromString(env, string)
}
- (CGRect)decodeCGRectForKey:(id)key { // NSString*
    let string: id = msg![env; this decodeObjectForKey:key];
    CGRectFromString(env, string)
}

@end

};

fn borrow_host_obj(env: &mut Environment, unarchiver: id) -> &mut NSKeyedUnarchiverHostObject {
    env.objc.borrow_mut(unarchiver)
}

fn get_value_to_decode_for_key(env: &mut Environment, unarchiver: id, key: id) -> Option<&Value> {
    if key == nil {
        return None;
    }
    let key = to_rust_string(env, key); // TODO: avoid copying string
    let host_obj = borrow_host_obj(env, unarchiver);
    let scope_value = match host_obj.current_key {
        Some(current_uid) => {
            let objects = host_obj.plist.get("$objects").and_then(|v| v.as_array())?;
            objects.get(current_uid.get() as usize)?
        }
        None => host_obj.plist.get("$top")?,
    };
    let scope = scope_value.as_dictionary()?;
    scope.get(&key)
}

/// The core of the implementation: unarchive something by its uid.
///
/// This is recursive in practice: the `initWithCoder:` messages sent by this
/// function will be received by objects which will then send
/// `decodeXXXWithKey:` messages back to the unarchiver, which will then call
/// this function (and so on).
///
/// The object returned is retained only by the archiver. Remember to retain and
/// possibly autorelease it as appropriate.
fn unarchive_key(env: &mut Environment, unarchiver: id, key: Uid) -> id {
    let host_obj = borrow_host_obj(env, unarchiver);
    let key_idx = key.get() as usize;
    if key_idx >= host_obj.already_unarchived.len() {
        log!(
            "Warning: unarchive_key: uid {} out of range (max {}); returning nil.",
            key.get(),
            host_obj.already_unarchived.len()
        );
        return nil;
    }
    if let Some(existing) = host_obj.already_unarchived[key_idx] {
        return existing;
    }

    let Some(objects) = host_obj.plist.get("$objects").and_then(|v| v.as_array()) else {
        log!("Warning: unarchive_key: $objects missing or not an array; returning nil.");
        return nil;
    };

    let Some(item) = objects.get(key_idx) else {
        log!(
            "Warning: unarchive_key: uid {} out of $objects range; returning nil.",
            key.get()
        );
        return nil;
    };
    let new_object = match item {
        // The most general kind of item: a dictionary that contains the info
        // needed to invoke `initWithCoder:` on a class implementing NSCoding.
        Value::Dictionary(dict) => {
            let Some(class_key) = dict.get("$class").and_then(|v| v.as_uid()).copied() else {
                log!(
                    "Warning: unarchive_key: missing $class for uid {}; returning nil.",
                    key.get()
                );
                return nil;
            };
            let class_key_idx = class_key.get() as usize;
            let class;
            if class_key_idx >= host_obj.already_unarchived.len() {
                log!(
                    "Warning: unarchive_key: class uid {} out of range; returning nil.",
                    class_key.get()
                );
                return nil;
            }
            if let Some(existing) = host_obj.already_unarchived[class_key_idx] {
                class = existing;
            } else {
                let Some(class_dict) = objects.get(class_key_idx) else {
                    log!("Warning: unarchive_key: class uid {} out of $objects range; returning nil.", class_key.get());
                    return nil;
                };
                let Some(class_dict) = class_dict.as_dictionary() else {
                    log!("Warning: unarchive_key: class entry at uid {} is not a dict; returning nil.", class_key.get());
                    return nil;
                };

                let Some(class_name) = class_dict.get("$classname").and_then(|v| v.as_string())
                else {
                    log!("Warning: unarchive_key: missing $classname for class uid {}; returning nil.", class_key.get());
                    return nil;
                };

                class = {
                    // get_known_class needs &mut ObjC, so we can't call it
                    // while holding a reference to the class name, since it
                    // is ultimately owned by ObjC via the host object
                    let class_name = class_name.to_string();
                    if class_name.is_empty() { eprintln!("DIAG_CALLER: ns_keyed_unarchiver classname empty"); }
                    env.objc.get_known_class(&class_name, &mut env.mem)
                };
                let host_obj = borrow_host_obj(env, unarchiver); // reborrow

                host_obj.already_unarchived[class_key_idx] = Some(class);
            };

            let host_obj = borrow_host_obj(env, unarchiver); // reborrow
            let old_current_key = host_obj.current_key;
            host_obj.current_key = Some(key);

            let new_object: id = msg![env; class alloc];
            let new_object: id = msg![env; new_object initWithCoder:unarchiver];

            let host_obj = borrow_host_obj(env, unarchiver); // reborrow
            host_obj.current_key = old_current_key;

            new_object
        }
        Value::String(s) => {
            let s = s.to_string();
            from_rust_string(env, s)
        }
        Value::Integer(int) => {
            #[allow(clippy::clone_on_copy)]
            let int = int.clone();
            // Similar logic to deserialize_plist()
            let number: id = msg_class![env; NSNumber alloc];
            // TODO: is this the correct order of preference? does it matter?
            if let Some(int64) = int.as_signed() {
                let longlong: i64 = int64;
                msg![env; number initWithLongLong:longlong]
            } else if let Some(uint64) = int.as_unsigned() {
                let ulonglong: u64 = uint64;
                msg![env; number initWithUnsignedLongLong:ulonglong]
            } else {
                // plist crate docs say this is unreachable, but if we ever
                // hit it just return a zero NSNumber rather than panicking.
                log!("Warning: unarchive_key: integer with no signed/unsigned representation; returning 0.");
                msg![env; number initWithInteger:(0 as NSInteger)]
            }
        }
        Value::Real(r) => {
            let r = *r;
            let number: id = msg_class![env; NSNumber alloc];
            msg![env; number initWithDouble:r]
        }
        Value::Boolean(b) => {
            let b = *b;
            let number: id = msg_class![env; NSNumber alloc];
            msg![env; number initWithBool:b]
        }
        Value::Data(data) => {
            let data = data.clone();
            let ns_data: id = msg_class![env; NSData alloc];
            let len: GuestUSize = match data.len().try_into() {
                Ok(l) => l,
                Err(_) => GuestUSize::MAX,
            };
            let bytes_ptr: MutVoidPtr = env.mem.alloc(len);
            let copy_len = std::cmp::min(len as usize, data.len());
            env.mem
                .bytes_at_mut(bytes_ptr.cast(), copy_len as GuestUSize)
                .copy_from_slice(&data[..copy_len]);
            let ns_len: NSUInteger = copy_len as NSUInteger;
            let bytes_const: ConstVoidPtr = bytes_ptr.cast_const();
            let result: id = msg![env; ns_data initWithBytes:bytes_const length:ns_len];
            env.mem.free(bytes_ptr);
            result
        }
        Value::Date(_) | Value::Array(_) | Value::Uid(_) | Value::Dictionary(_) => {
            log!(
                "Warning: unarchive_key: unhandled plist variant for uid {}; returning nil.",
                key.get()
            );
            nil
        }
        _ => {
            log!(
                "Warning: unarchive_key: unknown plist variant for uid {}; returning nil.",
                key.get()
            );
            nil
        }
    };

    let host_obj = borrow_host_obj(env, unarchiver); // reborrow
    host_obj.already_unarchived[key_idx] = Some(new_object);
    new_object
}

/// Shortcut for use by `[_touchHLE_NSArray initWithCoder:]`.
///
/// The objects are to be considered retained by the `Vec`.
pub fn decode_current_array(env: &mut Environment, unarchiver: id) -> Vec<id> {
    let keys = keys_for_key(env, unarchiver, "NS.objects");

    keys.into_iter()
        .map(|key| {
            let new_object = unarchive_key(env, unarchiver, key);
            // object is retained by the Vec
            retain(env, new_object)
        })
        .collect()
}

/// Shortcut for use by `[_touchHLE_NSMutableDictionary initWithCoder:]`.
///
/// Similar to `decode_current_array`, but for dictionaries.
/// The keys and objects are not retained!
pub fn decode_current_dict(env: &mut Environment, unarchiver: id) -> Vec<(id, id)> {
    let keys = keys_for_key(env, unarchiver, "NS.keys");
    let vals = keys_for_key(env, unarchiver, "NS.objects");
    log_dbg!("decode_current_dict: keys {:?}, vals {:?}", keys, vals);

    let keys: Vec<id> = keys
        .into_iter()
        .map(|key| unarchive_key(env, unarchiver, key))
        .collect();
    let vals: Vec<id> = vals
        .into_iter()
        .map(|val| unarchive_key(env, unarchiver, val))
        .collect();

    keys.into_iter().zip(vals).collect()
}

/// Shortcut for use by `[NSDate initWithCoder:]`.
pub fn decode_current_date(env: &mut Environment, unarchiver: id) -> id {
    let key = get_static_str(env, "NS.time");
    let timestamp = get_value_to_decode_for_key(env, unarchiver, key)
        .unwrap()
        .as_real()
        .unwrap();

    let date: id = msg_class![env; NSDate alloc];
    msg![env; date initWithTimeIntervalSinceReferenceDate:timestamp]
}

/// Shortcut for use by `[NSData initWithCoder:]`.
pub fn decode_current_data(env: &mut Environment, unarchiver: id, is_mutable: bool) -> id {
    let key = get_static_str(env, "NS.data");
    // TODO: avoid copying (twice!)
    let bytes = get_value_to_decode_for_key(env, unarchiver, key)
        .unwrap()
        .as_data()
        .unwrap()
        .to_vec();
    let len: GuestUSize = bytes.len().try_into().unwrap();
    let guest_bytes: MutVoidPtr = env.mem.alloc(len);
    env.mem
        .bytes_at_mut(guest_bytes.cast(), len)
        .copy_from_slice(bytes.as_slice());

    assert!(is_mutable); // TODO
    let data: id = msg_class![env; NSMutableData alloc];
    msg![env; data initWithBytesNoCopy:guest_bytes length:len freeWhenDone:true]
}

fn keys_for_key(env: &mut Environment, unarchiver: id, key: &str) -> Vec<Uid> {
    let host_obj = borrow_host_obj(env, unarchiver);
    let Some(objects) = host_obj.plist.get("$objects").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    let Some(current_key) = host_obj.current_key else {
        return Vec::new();
    };
    let idx = current_key.get() as usize;
    if idx >= objects.len() {
        return Vec::new();
    }
    let item = &objects[idx];
    let Some(dict) = item.as_dictionary() else {
        return Vec::new();
    };
    let Some(arr) = dict.get(key).and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|value| value.as_uid().copied())
        .collect()
}
