/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0.
 * If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `NSIndexPath`.

use super::ns_string::from_rust_string;
use super::NSUInteger;
use crate::frameworks::foundation::NSInteger;
use crate::mem::{ConstPtr, MutPtr};
use crate::objc::{
    autorelease, id, msg, msg_class, nil, objc_classes, retain, ClassExports, HostObject,
    NSZonePtr,
};

#[derive(Default)]
struct NSIndexPathHostObject {
    /// Stored indexes (heap-allocated Rust vec).
    indexes: Vec<NSUInteger>,
}
impl HostObject for NSIndexPathHostObject {}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation NSIndexPath: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(NSIndexPathHostObject {
        indexes: Vec::new(),
    });
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

// MARK: - Constructors

// `indexPathWithIndex:` — single-component index path.
+ (id)indexPathWithIndex:(NSUInteger)index {
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithIndex:index];
    autorelease(env, new)
}

// `indexPathWithIndexes:length:` — from a C array of NSUInteger.
+ (id)indexPathWithIndexes:(ConstPtr<NSUInteger>)indexes
                   length:(NSUInteger)length {
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithIndexes:indexes length:length];
    autorelease(env, new)
}

// UIKit convenience: `indexPathForRow:inSection:`
+ (id)indexPathForRow:(NSUInteger)row inSection:(NSUInteger)section {
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithIndexes:(ConstPtr::<NSUInteger>::null()) length:0u32];
    {
        let host = env.objc.borrow_mut::<NSIndexPathHostObject>(new);
        host.indexes = vec![section, row];
    }
    autorelease(env, new)
}

// UIKit convenience: `indexPathForItem:inSection:` (collection view)
+ (id)indexPathForItem:(NSUInteger)item inSection:(NSUInteger)section {
    msg_class![env; NSIndexPath indexPathForRow:item inSection:section]
}

// MARK: - Init

- (id)initWithIndex:(NSUInteger)index {
    env.objc.borrow_mut::<NSIndexPathHostObject>(this).indexes = vec![index];
    this
}

- (id)initWithIndexes:(ConstPtr<NSUInteger>)indexes length:(NSUInteger)length {
    let mut vec = Vec::with_capacity(length as usize);
    for i in 0..length {
        vec.push(env.mem.read(indexes + i));
    }
    env.objc.borrow_mut::<NSIndexPathHostObject>(this).indexes = vec;
    this
}

// MARK: - Dealloc

- (())dealloc {
    env.objc.dealloc_object(this, &mut env.mem)
}

// MARK: - Length / indexes

- (NSUInteger)length {
    env.objc.borrow::<NSIndexPathHostObject>(this).indexes.len() as NSUInteger
}

- (NSUInteger)indexAtPosition:(NSUInteger)position {
    let host = env.objc.borrow::<NSIndexPathHostObject>(this);
    if (position as usize) < host.indexes.len() {
        host.indexes[position as usize]
    } else {
        // NSNotFound
        NSUInteger::MAX
    }
}

// Write all indexes into a caller-supplied C array.
- (())getIndexes:(MutPtr<NSUInteger>)indexes {
    let host = env.objc.borrow::<NSIndexPathHostObject>(this);
    for (i, &idx) in host.indexes.iter().enumerate() {
        env.mem.write(indexes + i as u32, idx);
    }
}

// Write a range of indexes into a caller-supplied C array.
- (())getIndexes:(MutPtr<NSUInteger>)indexes
           range:(NSUInteger)range_loc :(NSUInteger)range_len {
    let host = env.objc.borrow::<NSIndexPathHostObject>(this);
    let end = (range_loc as usize + range_len as usize).min(host.indexes.len());
    for i in (range_loc as usize)..end {
        env.mem.write(indexes + (i - range_loc as usize) as u32, host.indexes[i]);
    }
}

// MARK: - UIKit row / section / item convenience

- (NSUInteger)row {
    let host = env.objc.borrow::<NSIndexPathHostObject>(this);
    if host.indexes.len() >= 2 { host.indexes[1] } else { 0 }
}

- (NSUInteger)section {
    let host = env.objc.borrow::<NSIndexPathHostObject>(this);
    if !host.indexes.is_empty() { host.indexes[0] } else { 0 }
}

- (NSUInteger)item {
    // Collection view alias for row.
    msg![env; this row]
}

// MARK: - Path manipulation

- (id)indexPathByAddingIndex:(NSUInteger)index {
    let mut new_indexes = env.objc.borrow::<NSIndexPathHostObject>(this).indexes.clone();
    new_indexes.push(index);
    let _len = new_indexes.len() as NSUInteger;

    let new: id = msg_class![env; NSIndexPath alloc];
    env.objc.borrow_mut::<NSIndexPathHostObject>(new).indexes = new_indexes;
    autorelease(env, new)
}

- (id)indexPathByRemovingLastIndex {
    let mut new_indexes = env.objc.borrow::<NSIndexPathHostObject>(this).indexes.clone();
    new_indexes.pop();

    let new: id = msg_class![env; NSIndexPath alloc];
    env.objc.borrow_mut::<NSIndexPathHostObject>(new).indexes = new_indexes;
    autorelease(env, new)
}

// MARK: - Comparison

- (NSInteger)compare:(id)other { // NSIndexPath*
    if other == nil { return 1; }
    let a = env.objc.borrow::<NSIndexPathHostObject>(this).indexes.clone();
    let b = env.objc.borrow::<NSIndexPathHostObject>(other).indexes.clone();
    let min_len = a.len().min(b.len());
    for i in 0..min_len {
        if a[i] < b[i] { return -1; }
        if a[i] > b[i] { return  1; }
    }
    match a.len().cmp(&b.len()) {
        std::cmp::Ordering::Less    => -1,
        std::cmp::Ordering::Greater =>  1,
        std::cmp::Ordering::Equal   =>  0,
    }
}

- (bool)isEqual:(id)other {
    if this == other { return true; }
    if other == nil  { return false; }
    let a = env.objc.borrow::<NSIndexPathHostObject>(this).indexes.clone();
    let b = env.objc.borrow::<NSIndexPathHostObject>(other);
    a == b.indexes
}

- (NSUInteger)hash {
    // FNV-1a over the index sequence.
    let host = env.objc.borrow::<NSIndexPathHostObject>(this);
    let mut h: u32 = 2166136261;
    for &idx in &host.indexes {
        let bytes = idx.to_le_bytes();
        for b in bytes {
            h ^= b as u32;
            h = h.wrapping_mul(16777619);
        }
    }
    h as NSUInteger
}

// MARK: - NSCopying / NSMutableCopying

- (id)copyWithZone:(NSZonePtr)_zone {
    // NSIndexPath is immutable — return self retained.
    retain(env, this)
}

- (id)mutableCopyWithZone:(NSZonePtr)_zone {
    let new: id = msg_class![env; NSIndexPath alloc];
    let idxs = env.objc.borrow::<NSIndexPathHostObject>(this).indexes.clone();
    env.objc.borrow_mut::<NSIndexPathHostObject>(new).indexes = idxs;
    autorelease(env, new)
}

// MARK: - NSCoding

- (id)initWithCoder:(id)coder {
    let key = crate::frameworks::foundation::ns_string::get_static_str(env, "NSIndexPathData");
    let data: id = msg![env; coder decodeObjectForKey:key];
    if data != nil {
        let len: NSUInteger = msg![env; data length];
        let count = len as usize / std::mem::size_of::<NSUInteger>();
        let bytes_ptr: crate::mem::ConstVoidPtr = msg![env; data bytes];
        let mut indexes = Vec::with_capacity(count);
        for i in 0..count as u32 {
            let idx = env.mem.read(bytes_ptr.cast::<NSUInteger>() + i);
            indexes.push(idx);
        }
        env.objc.borrow_mut::<NSIndexPathHostObject>(this).indexes = indexes;
    }
    this
}

- (())encodeWithCoder:(id)coder {
    let host = env.objc.borrow::<NSIndexPathHostObject>(this);
    let byte_len = (host.indexes.len() * std::mem::size_of::<NSUInteger>()) as NSUInteger;
    let buf: crate::mem::MutVoidPtr = env.mem.alloc(byte_len as u32).cast();
    for (i, &idx) in host.indexes.iter().enumerate() {
        env.mem.write(buf.cast::<NSUInteger>() + i as u32, idx);
    }
    let data: id = msg_class![env; NSData dataWithBytesNoCopy:buf length:byte_len];
    let key = crate::frameworks::foundation::ns_string::get_static_str(env, "NSIndexPathData");
    () = msg![env; coder encodeObject:data forKey:key];
}

// MARK: - Description

- (id)description {
    let host = env.objc.borrow::<NSIndexPathHostObject>(this);
    let parts: Vec<String> = host.indexes.iter().map(|i| i.to_string()).collect();
    let s = format!("<NSIndexPath: [{}]>", parts.join(", "));
    let ns = from_rust_string(env, s);
    autorelease(env, ns)
}

@end

};
