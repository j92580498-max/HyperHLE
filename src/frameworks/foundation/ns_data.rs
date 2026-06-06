/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, you can obtain one at https://mozilla.org/MPL/2.0/.
 */

use super::ns_string::to_rust_string;
use super::NSUInteger;
use crate::frameworks::foundation::ns_keyed_unarchiver::decode_current_data;
use crate::frameworks::foundation::NSRange;
use crate::fs::GuestPath;
use crate::mem::{ConstPtr, ConstVoidPtr, MutPtr, MutVoidPtr, Ptr};
use crate::objc::{
    autorelease, id, msg, nil, objc_classes, release, retain, ClassExports, HostObject, NSZonePtr,
};
use crate::{msg_class, Environment};

#[derive(Default)]
pub(super) struct NSDataHostObject {
    pub(super) bytes: MutVoidPtr,
    pub(super) length: NSUInteger,
    pub(super) free_when_done: bool,
}

impl HostObject for NSDataHostObject {}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation NSData: NSObject

// =========================================================================
// MARK: - Allocation
// =========================================================================

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(NSDataHostObject {
        bytes: Ptr::null(),
        length: 0,
        free_when_done: true,
    });
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

// =========================================================================
// MARK: - Class convenience constructors
// =========================================================================

+ (id)data {
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new init];
    autorelease(env, new)
}

+ (id)dataWithBytesNoCopy:(MutVoidPtr)bytes length:(NSUInteger)length {
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithBytesNoCopy:bytes length:length];
    autorelease(env, new)
}

+ (id)dataWithBytesNoCopy:(MutVoidPtr)bytes
                   length:(NSUInteger)length
             freeWhenDone:(bool)free_when_done {
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithBytesNoCopy:bytes
                                             length:length
                                       freeWhenDone:free_when_done];
    autorelease(env, new)
}

+ (id)dataWithBytes:(ConstVoidPtr)bytes length:(NSUInteger)length {
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithBytes:bytes length:length];
    autorelease(env, new)
}

+ (id)dataWithContentsOfFile:(id)path {
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithContentsOfFile:path];
    autorelease(env, new)
}

+ (id)dataWithContentsOfFile:(id)path
                     options:(NSUInteger)options
                       error:(MutVoidPtr)error {
    let _ = options;
    let _ = error;
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithContentsOfFile:path];
    autorelease(env, new)
}

+ (id)dataWithContentsOfMappedFile:(id)path {
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithContentsOfMappedFile:path];
    autorelease(env, new)
}

+ (id)dataWithContentsOfURL:(id)url {
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithContentsOfURL:url];
    autorelease(env, new)
}

+ (id)dataWithContentsOfURL:(id)url
                    options:(NSUInteger)options
                      error:(MutVoidPtr)error {
    let _ = options;
    let _ = error;
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithContentsOfURL:url];
    autorelease(env, new)
}

+ (id)dataWithData:(id)data {
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithData:data];
    autorelease(env, new)
}

// =========================================================================
// MARK: - Instance initializers
// =========================================================================

- (id)init {
    // Zero-length empty data — bytes stays null, length stays 0.
    this
}

- (id)initWithBytesNoCopy:(MutVoidPtr)bytes length:(NSUInteger)length {
    msg![env; this initWithBytesNoCopy:bytes length:length freeWhenDone:true]
}

- (id)initWithBytesNoCopy:(MutVoidPtr)bytes
                   length:(NSUInteger)length
             freeWhenDone:(bool)free_when_done {
    let host_object = env.objc.borrow_mut::<NSDataHostObject>(this);
    assert!(host_object.bytes.is_null() && host_object.length == 0);
    host_object.bytes = bytes;
    host_object.length = length;
    host_object.free_when_done = free_when_done;
    this
}

- (id)initWithBytes:(ConstVoidPtr)bytes length:(NSUInteger)length {
    let host_object = env.objc.borrow_mut::<NSDataHostObject>(this);
    assert!(host_object.bytes.is_null() && host_object.length == 0);
    if length == 0 {
        return this;
    }
    let alloc = env.mem.alloc(length);
    env.mem.memmove(alloc, bytes, length);
    host_object.bytes = alloc;
    host_object.length = length;
    this
}

- (id)initWithData:(id)data {
    let bytes: ConstVoidPtr = msg![env; data bytes];
    let length: NSUInteger = msg![env; data length];
    msg![env; this initWithBytes:bytes length:length]
}

- (id)initWithContentsOfURL:(id)url {
    if url == nil {
        release(env, this);
        return nil;
    }
    let path: id = msg![env; url path];
    msg![env; this initWithContentsOfFile:path]
}

- (id)initWithContentsOfURL:(id)url
                    options:(NSUInteger)_options
                      error:(MutVoidPtr)_error {
    msg![env; this initWithContentsOfURL:url]
}

- (id)initWithContentsOfFile:(id)path {
    if path == nil {
        release(env, this);
        return nil;
    }
    let path_str = to_rust_string(env, path);
    let Ok(bytes) = env.fs.read(GuestPath::new(&path_str)) else {
        log_dbg!("NSData: Failed to read file at {:?}", path_str);
        release(env, this);
        return nil;
    };
    let size: NSUInteger = bytes.len().try_into().unwrap();
    if size == 0 {
        return this;
    }
    let alloc = env.mem.alloc(size);
    let casted_alloc: MutPtr<u8> = alloc.cast();
    let slice = env.mem.bytes_at_mut(casted_alloc, size);
    slice.copy_from_slice(&bytes);
    let host_object = env.objc.borrow_mut::<NSDataHostObject>(this);
    host_object.bytes = alloc;
    host_object.length = size;
    this
}

- (id)initWithContentsOfFile:(id)path
                     options:(NSUInteger)_options
                       error:(MutVoidPtr)_error {
    msg![env; this initWithContentsOfFile:path]
}

- (id)initWithContentsOfMappedFile:(id)path {
    msg![env; this initWithContentsOfFile:path]
}

- (id)initWithCoder:(id)coder {
    release(env, this);
    decode_current_data(env, coder, true)
}

// =========================================================================
// MARK: - Dealloc / copy
// =========================================================================

- (())dealloc {
    let &NSDataHostObject { bytes, free_when_done, .. } = env.objc.borrow(this);
    if !bytes.is_null() && free_when_done {
        env.mem.free(bytes);
    }
    env.objc.dealloc_object(this, &mut env.mem)
}

- (id)copyWithZone:(NSZonePtr)_zone {
    retain(env, this)
}

- (id)mutableCopyWithZone:(NSZonePtr)_zone {
    let bytes: ConstVoidPtr = msg![env; this bytes];
    let length: NSUInteger = msg![env; this length];
    let new: id = msg_class![env; NSMutableData alloc];
    msg![env; new initWithBytes:bytes length:length]
}

// =========================================================================
// MARK: - Accessors
// =========================================================================

- (ConstVoidPtr)bytes {
    env.objc.borrow::<NSDataHostObject>(this).bytes.cast_const()
}

- (NSUInteger)length {
    env.objc.borrow::<NSDataHostObject>(this).length
}

// =========================================================================
// MARK: - Byte extraction
// =========================================================================

- (())getBytes:(MutVoidPtr)buffer {
    let host_object = env.objc.borrow::<NSDataHostObject>(this);
    if host_object.length > 0 && !host_object.bytes.is_null() {
        env.mem.memmove(buffer, host_object.bytes.cast_const(), host_object.length);
    }
}

- (())getBytes:(MutVoidPtr)buffer length:(NSUInteger)length {
    let host_object = env.objc.borrow::<NSDataHostObject>(this);
    let to_copy = length.min(host_object.length);
    if to_copy > 0 && !host_object.bytes.is_null() {
        env.mem.memmove(buffer, host_object.bytes.cast_const(), to_copy);
    }
}

- (())getBytes:(MutVoidPtr)buffer range:(NSRange)range {
    let host_object = env.objc.borrow::<NSDataHostObject>(this);
    let loc = range.location;
    let len = range.length;
    if len == 0 {
        return;
    }
    if loc + len <= host_object.length && !host_object.bytes.is_null() {
        let src: ConstVoidPtr = (host_object.bytes.cast_const().cast::<u8>() + loc).cast_void();
        env.mem.memmove(buffer, src, len);
    } else {
        log_dbg!(
            "Warning: NSData getBytes:range: out of bounds \
             (location={}, length={}, data_length={})",
            loc, len, host_object.length
        );
    }
}

// =========================================================================
// MARK: - Subdatas / searching
// =========================================================================

- (id)subdataWithRange:(NSRange)range {
    let loc = range.location;
    let len = range.length;
    let host_object = env.objc.borrow::<NSDataHostObject>(this);
    if loc + len > host_object.length {
        log_dbg!(
            "Warning: NSData subdataWithRange: out of bounds \
             (location={}, length={}, data_length={})",
            loc, len, host_object.length
        );
        // Return empty data rather than crashing.
        return msg_class![env; NSData data];
    }
    if len == 0 {
        return msg_class![env; NSData data];
    }
    let src: ConstVoidPtr = (host_object.bytes.cast_const().cast::<u8>() + loc).cast_void();
    let new: id = msg_class![env; NSData alloc];
    msg![env; new initWithBytes:src length:len]
}

- (NSRange)rangeOfData:(id)data_to_find
               options:(NSUInteger)_options
                 range:(NSRange)search_range {
    let not_found = NSRange { location: NSUInteger::MAX, length: 0 };

    let haystack = env.objc.borrow::<NSDataHostObject>(this);
    let needle   = env.objc.borrow::<NSDataHostObject>(data_to_find);

    let h_len = haystack.length;
    let n_len = needle.length;

    if n_len == 0 || n_len > h_len {
        return not_found;
    }

    let search_start = search_range.location;
    let search_end   = (search_range.location + search_range.length).min(h_len);

    if search_start >= search_end || n_len > search_end - search_start {
        return not_found;
    }

    let h_ptr: ConstPtr<u8> = haystack.bytes.cast_const().cast();
    let n_ptr: ConstPtr<u8> = needle.bytes.cast_const().cast();
    let h_slice = env.mem.bytes_at(h_ptr, h_len);
    let n_slice = env.mem.bytes_at(n_ptr, n_len);

    let search_slice = &h_slice[(search_start as usize)..(search_end as usize)];
    for i in 0..=(search_slice.len().saturating_sub(n_len as usize)) {
        if search_slice[i..i + (n_len as usize)] == *n_slice {
            return NSRange { location: search_start + (i as u32), length: n_len };
        }
    }
    not_found
}

// =========================================================================
// MARK: - Equality / hashing
// =========================================================================

- (bool)isEqualToData:(id)other {
    let a = to_rust_slice(env, this).to_owned();
    let b = to_rust_slice(env, other);
    a == b
}

- (bool)isEqual:(id)other {
    if this == other {
        return true;
    }
    if other == nil {
        return false;
    }
    // Check if other responds to isEqualToData: (i.e. is an NSData).
    msg![env; this isEqualToData:other]
}

// =========================================================================
// MARK: - Writing
// =========================================================================

- (bool)writeToFile:(id)path atomically:(bool)_use_aux_file {
    let file = to_rust_string(env, path);
    let host_object = env.objc.borrow::<NSDataHostObject>(this);
    let slice: &[u8] = if host_object.length == 0 || host_object.bytes.is_null() {
        &[]
    } else {
        let casted_ptr: ConstPtr<u8> = host_object.bytes.cast_const().cast();
        env.mem.bytes_at(casted_ptr, host_object.length)
    };
    env.fs.write(GuestPath::new(&file), slice).is_ok()
}

- (bool)writeToFile:(id)path
            options:(NSUInteger)_options
              error:(MutVoidPtr)error {
    let success: bool = msg![env; this writeToFile:path atomically:false];
    if !success {
        log!("Warning: NSData writeToFile:options:error: failed");
    }
    if !error.is_null() {
        let error_ptr: MutPtr<id> = error.cast();
        env.mem.write(error_ptr, nil);
    }
    success
}

- (bool)writeToURL:(id)url atomically:(bool)atomically {
    if url == nil {
        return false;
    }
    let path: id = msg![env; url path];
    msg![env; this writeToFile:path atomically:atomically]
}

- (bool)writeToURL:(id)url
           options:(NSUInteger)options
             error:(MutVoidPtr)error {
    if url == nil {
        if !error.is_null() {
            let error_ptr: MutPtr<id> = error.cast();
            env.mem.write(error_ptr, nil);
        }
        return false;
    }
    let path: id = msg![env; url path];
    msg![env; this writeToFile:path options:options error:error]
}

// =========================================================================
// MARK: - Description / debug
// =========================================================================

- (id)description {
    // Returns a hex-encoded string like Apple's implementation: <deadbeef ...>
    let host_object = env.objc.borrow::<NSDataHostObject>(this);
    let length = host_object.length;
    if length == 0 || host_object.bytes.is_null() {
        let s = "<>";
        return msg_class![env; NSString stringWithUTF8String:(env.mem.alloc_and_write_cstr(s.as_bytes()))];
    }
    let ptr: ConstPtr<u8> = host_object.bytes.cast_const().cast();
    let bytes = env.mem.bytes_at(ptr, length);
    let mut hex = String::with_capacity((2 + length * 2 + (length / 4)) as usize);
    hex.push('<');
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && i % 4 == 0 {
            hex.push(' ');
        }
        use std::fmt::Write;
        let _ = write!(hex, "{:02x}", b);
    }
    hex.push('>');
    let cstr = env.mem.alloc_and_write_cstr(hex.as_bytes());
    msg_class![env; NSString stringWithUTF8String:cstr]
}

@end

// =========================================================================
// MARK: - NSMutableData
// =========================================================================

@implementation NSMutableData: NSData

+ (id)data {
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new init];
    autorelease(env, new)
}

+ (id)dataWithCapacity:(NSUInteger)capacity {
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithCapacity:capacity];
    autorelease(env, new)
}

+ (id)dataWithLength:(NSUInteger)length {
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithLength:length];
    autorelease(env, new)
}

- (id)initWithCapacity:(NSUInteger)capacity {
    // Pre-allocate but leave length at 0.
    if capacity > 0 {
        let alloc = env.mem.alloc(capacity);
        let host_object = env.objc.borrow_mut::<NSDataHostObject>(this);
        host_object.bytes = alloc;
        // length intentionally remains 0.
    }
    this
}

- (id)initWithLength:(NSUInteger)length {
    if length > 0 {
        let alloc = env.mem.alloc(length);
        env.mem.bytes_at_mut(alloc.cast(), length).fill(0);
        let host_object = env.objc.borrow_mut::<NSDataHostObject>(this);
        host_object.bytes = alloc;
        host_object.length = length;
    }
    this
}

// copyWithZone: for NSMutableData must return a *mutable* copy.
- (id)mutableCopyWithZone:(NSZonePtr)_zone {
    let bytes: ConstVoidPtr = msg![env; this bytes];
    let length: NSUInteger = msg![env; this length];
    let new: id = msg_class![env; NSMutableData alloc];
    msg![env; new initWithBytes:bytes length:length]
}

- (MutVoidPtr)mutableBytes {
    env.objc.borrow_mut::<NSDataHostObject>(this).bytes
}

- (())setLength:(NSUInteger)length {
    let host_object = env.objc.borrow_mut::<NSDataHostObject>(this);
    if host_object.length == length {
        return;
    }
    if length == 0 {
        if !host_object.bytes.is_null() && host_object.free_when_done {
            env.mem.free(host_object.bytes);
        }
        host_object.bytes = Ptr::null();
        host_object.length = 0;
        return;
    }
    if host_object.bytes.is_null() {
        let alloc = env.mem.alloc(length);
        env.mem.bytes_at_mut(alloc.cast(), length).fill(0);
        host_object.bytes = alloc;
        host_object.length = length;
        host_object.free_when_done = true;
    } else {
        let old_len = host_object.length;
        let alloc = env.mem.realloc(host_object.bytes, length);
        if length > old_len {
            let diff = length - old_len;
            let offset_ptr: MutPtr<u8> = alloc.cast() + old_len;
            env.mem.bytes_at_mut(offset_ptr, diff).fill(0);
        }
        host_object.bytes = alloc;
        host_object.length = length;
        host_object.free_when_done = true;
    }
}

- (())increaseLengthBy:(NSUInteger)extra_length {
    if extra_length == 0 {
        return;
    }
    let current_length: NSUInteger = msg![env; this length];
    () = msg![env; this setLength:(current_length + extra_length)];
}

- (())appendBytes:(ConstVoidPtr)bytes length:(NSUInteger)length {
    if length == 0 {
        return;
    }
    let old_length: NSUInteger = msg![env; this length];
    let new_length = old_length + length;
    () = msg![env; this setLength:new_length];
    let host_object = env.objc.borrow::<NSDataHostObject>(this);
    let dest: MutVoidPtr = (host_object.bytes.cast::<u8>() + old_length).cast_void();
    env.mem.memmove(dest, bytes, length);
}

- (())appendData:(id)other {
    let bytes: ConstVoidPtr = msg![env; other bytes];
    let length: NSUInteger = msg![env; other length];
    () = msg![env; this appendBytes:bytes length:length];
}

- (())replaceBytesInRange:(NSRange)range withBytes:(ConstVoidPtr)replacement_bytes {
    let loc = range.location;
    let len = range.length;
    let current_length: NSUInteger = msg![env; this length];
    if loc + len > current_length {
        log_dbg!(
            "Warning: NSMutableData replaceBytesInRange: out of bounds \
             (location={}, length={}, data_length={})",
            loc, len, current_length
        );
        return;
    }
    if len == 0 {
        return;
    }
    let host_object = env.objc.borrow::<NSDataHostObject>(this);
    let dest: MutVoidPtr = (host_object.bytes.cast::<u8>() + loc).cast_void();
    env.mem.memmove(dest, replacement_bytes, len);
}

- (())replaceBytesInRange:(NSRange)range
               withBytes:(ConstVoidPtr)replacement_bytes
                  length:(NSUInteger)replacement_length {
    let loc       = range.location;
    let old_len   = range.length;
    let cur_len: NSUInteger = msg![env; this length];

    if loc + old_len > cur_len {
        log_dbg!(
            "Warning: NSMutableData replaceBytesInRange:withBytes:length: out of bounds \
             (location={}, length={}, data_length={})",
            loc, old_len, cur_len
        );
        return;
    }

    let new_total = cur_len - old_len + replacement_length;
    // Shift tail bytes to make (or close) the gap.
    if old_len != replacement_length && loc + old_len < cur_len {
        let tail_len = cur_len - (loc + old_len);
        let src: ConstVoidPtr = {
            let h = env.objc.borrow::<NSDataHostObject>(this);
            (h.bytes.cast_const().cast::<u8>() + (loc + old_len)).cast_void()
        };
        // Resize first so the destination is valid.
        () = msg![env; this setLength:new_total];
        let dest: MutVoidPtr = {
            let h = env.objc.borrow::<NSDataHostObject>(this);
            (h.bytes.cast::<u8>() + (loc + replacement_length)).cast_void()
        };
        env.mem.memmove(dest, src, tail_len);
    } else {
        () = msg![env; this setLength:new_total];
    }

    if replacement_length > 0 {
        let host_object = env.objc.borrow::<NSDataHostObject>(this);
        let dest: MutVoidPtr = (host_object.bytes.cast::<u8>() + loc).cast_void();
        env.mem.memmove(dest, replacement_bytes, replacement_length);
    }
}

- (())resetBytesInRange:(NSRange)range {
    let loc = range.location;
    let len = range.length;
    let current_length: NSUInteger = msg![env; this length];
    if len == 0 {
        return;
    }
    if loc + len > current_length {
        log_dbg!(
            "Warning: NSMutableData resetBytesInRange: out of bounds \
             (location={}, length={}, data_length={})",
            loc, len, current_length
        );
        return;
    }
    let host_object = env.objc.borrow::<NSDataHostObject>(this);
    let dest: MutPtr<u8> = host_object.bytes.cast() + loc;
    env.mem.bytes_at_mut(dest, len).fill(0);
}

- (())setData:(id)data {
    let bytes: ConstVoidPtr = msg![env; data bytes];
    let length: NSUInteger = msg![env; data length];
    () = msg![env; this setLength:0];
    () = msg![env; this appendBytes:bytes length:length];
}

@end

};

pub fn to_rust_slice(env: &mut Environment, data: id) -> &[u8] {
    let borrowed_data = env.objc.borrow::<NSDataHostObject>(data);
    if borrowed_data.length == 0 || borrowed_data.bytes.is_null() {
        return &[];
    }
    let casted_ptr: ConstPtr<u8> = borrowed_data.bytes.cast_const().cast();
    env.mem.bytes_at(casted_ptr, borrowed_data.length)
}
