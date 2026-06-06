/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! The `NSArray` class cluster, including `NSMutableArray`.

use super::ns_enumerator::{fast_enumeration_helper, NSFastEnumerationState};
use super::ns_property_list_serialization::{
    deserialize_plist_from_file, NSPropertyListBinaryFormat_v1_0,
};
use super::{
    _nib_archive_decoder, ns_keyed_archiver, ns_keyed_unarchiver, ns_string, ns_url,
    NSComparisonResult, NSNotFound, NSRange, NSUInteger,
};
use crate::abi::{CallFromHost, GuestFunction};
use crate::frameworks::foundation::ns_string::from_rust_string;
use crate::fs::GuestPath;
use crate::libc::stdlib::qsort::qsort_generic;
use crate::mem::{ConstPtr, MutPtr, MutVoidPtr, Ptr};
use crate::objc::{
    autorelease, id, msg, msg_class, msg_send, nil, objc_classes, release, retain, Class,
    ClassExports, HostObject, NSZonePtr, SEL,
};
use crate::Environment;

#[derive(Default)]
struct ObjectEnumeratorHostObject {
    /// the enumerated collection, NSArray *
    array: id,
    /// an iterator
    iterator: std::vec::IntoIter<id>,
}
impl HostObject for ObjectEnumeratorHostObject {}

/// Belongs to _touchHLE_NSArray
#[derive(Debug, Default)]
pub(super) struct ArrayHostObject {
    pub(super) array: Vec<id>,
}
impl HostObject for ArrayHostObject {}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

// NSArray is an abstract class. A subclass must provide:
// - (NSUInteger)count;
// - (id)objectAtIndex:(NSUInteger)index;
// We can pick whichever subclass we want for the various alloc methods.
// For the time being, that will always be _touchHLE_NSArray.
@implementation NSArray: NSObject

+ (id)allocWithZone:(NSZonePtr)zone {
    // NSArray might be subclassed by something which needs allocWithZone:
    // to have the normal behaviour. We don't currently support that; warn and
    // fall back to the bridged subclass instead of crashing the host.
    let ns_array_class = env.objc.get_known_class("NSArray", &mut env.mem);
    if this != ns_array_class {
        log!(
            "Warning: +[NSArray allocWithZone:] called on subclass {:?}; \
             treating as NSArray.",
            this
        );
    }
    msg_class![env; _touchHLE_NSArray allocWithZone:zone]
}

+ (id)array {
    let array: id = msg![env; this new];
    autorelease(env, array)
}

+ (id)arrayWithArray:(id)other { // NSArray*
    let array: id = msg![env; this alloc];
    let array: id = msg![env; array initWithArray:other];
    autorelease(env, array)
}

// These probably comes from some category related to plists.
+ (id)arrayWithContentsOfFile:(id)path { // NSString*
    let array: id = msg![env; this alloc];
    let array: id = msg![env; array initWithContentsOfFile:path];
    autorelease(env, array)
}
+ (id)arrayWithContentsOfURL:(id)url { // NSURL*
    let array: id = msg![env; this alloc];
    let array: id = msg![env; array initWithContentsOfURL:url];
    autorelease(env, array)
}

+ (id)arrayWithObject:(id)object {
    retain(env, object);
    let objects = vec![object];
    let array = from_vec(env, objects);
    autorelease(env, array)
}
+ (id)arrayWithObjects:(id)first_obj, ...args {
    let objects = if first_obj == nil {
        vec![]
    } else {
        retain(env, first_obj);
        let mut objects = vec![first_obj];
        let mut varargs = args.start();
        loop {
            let next_arg: id = varargs.next(env);
            if next_arg.is_null() {
                break;
            }
            retain(env, next_arg);
            objects.push(next_arg);
        }
        objects
    };
    let array = from_vec(env, objects);
    autorelease(env, array)
}
+ (id)arrayWithObjects:(ConstPtr<id>)objects_ptr count:(NSUInteger)count {
    let array: id = msg![env; this alloc];
    let array: id = msg![env; array initWithObjects:objects_ptr count:count];
    autorelease(env, array)
}

// These probably comes from some category related to plists.
- (id)initWithContentsOfFile:(id)path { // NSString*
    release(env, this);
    let path = ns_string::to_rust_string(env, path);
    deserialize_plist_from_file(
        env,
        GuestPath::new(&path),
        /* array_expected: */ true,
    )
}
- (id)initWithContentsOfURL:(id)url { // NSURL*
    release(env, this);
    let path = ns_url::to_rust_path(env, url);
    deserialize_plist_from_file(env, &path, /* array_expected: */ true)
}

- (bool)writeToFile:(id)path // NSString*
         atomically:(bool)atomically {
    let error_desc: MutPtr<id> = Ptr::null();
    let data: id = msg_class![env; NSPropertyListSerialization
            dataFromPropertyList:this
                          format:NSPropertyListBinaryFormat_v1_0
                errorDescription:error_desc];
    let res = msg![env; data writeToFile:path atomically:atomically];
    log_dbg!(
        "[(NSArray *){:?} writeToFile:{:?} atomically:{}] -> {}",
        this,
        ns_string::to_rust_string(env, path),
        atomically,
        res
    );
    res
}

// NSCopying implementation
- (id)copyWithZone:(NSZonePtr)_zone {
    retain(env, this)
}

- (NSUInteger)indexOfObject:(id)object {
    let count: NSUInteger = msg![env; this count];
    for i in 0..count {
        let curr_object: id = msg![env; this objectAtIndex:i];
        let equal: bool = msg![env; object isEqual:curr_object];
        if equal {
            return i;
        }
    }
    NSNotFound as NSUInteger
}
- (bool)containsObject:(id)object {
    let idx: NSUInteger = msg![env; this indexOfObject:object];
    idx != NSNotFound as NSUInteger
}

- (id)firstObject {
    let size: NSUInteger = msg![env; this count];
    if size == 0 {
        return nil;
    }
    msg![env; this objectAtIndex:0u32]
}

- (id)lastObject {
    let size: NSUInteger = msg![env; this count];
    if size == 0 {
        return nil;
    }
    msg![env; this objectAtIndex:(size - 1)]
}

- (id)componentsJoinedByString:(id)str { // NSString *
    let res: id = msg_class![env; NSMutableString new];
    let count: NSUInteger = msg![env; this count];
    if count == 0 {
        autorelease(env, res);
        return res;
    }
    for i in 0..count {
        let curr_object: id = msg![env; this objectAtIndex:i];
        let curr_desc: id = msg![env; curr_object description];
        () = msg![env; res appendString:curr_desc];
        if i != count-1 {
            () = msg![env; res appendString:str];
        }
    }
    let res_imm = msg![env; res copy];
    release(env, res);
    autorelease(env, res_imm)
}

- (id)sortedArrayUsingFunction:(GuestFunction)comparator
                       context:(MutVoidPtr)context {
    let array = msg![env; this mutableCopy];
    () = msg![env; array sortUsingFunction:comparator context:context];
    let array_imm = msg![env; array copy];
    release(env, array);
    autorelease(env, array_imm)
}

- (id)sortedArrayUsingDescriptors:(id)descriptors { // NSArray* of NSSortDescriptor*
    let array = msg![env; this mutableCopy];
    () = msg![env; array sortUsingDescriptors:descriptors];
    let array_imm = msg![env; array copy];
    release(env, array);
    autorelease(env, array_imm)
}

// Add to NSArray @implementation:

- (id)objectsAtIndexes:(id)index_set { // NSIndexSet*
    let count: NSUInteger = msg![env; index_set count];
    let mut result = Vec::with_capacity(count as usize);
    let total: NSUInteger = msg![env; this count];
    for i in 0..total {
        let contains: bool = msg![env; index_set containsIndex:i];
        if contains {
            let obj: id = msg![env; this objectAtIndex:i];
            retain(env, obj);
            result.push(obj);
        }
    }
    let arr = from_vec(env, result);
    autorelease(env, arr)
}

- (id)firstObjectCommonWithArray:(id)other { // NSArray*
    let count: NSUInteger = msg![env; this count];
    for i in 0..count {
        let obj: id = msg![env; this objectAtIndex:i];
        let contains: bool = msg![env; other containsObject:obj];
        if contains {
            return obj;
        }
    }
    nil
}

- (bool)isEqual:(id)other {
    if this == other {
        return true;
    }
    // Per Apple's docs, `-[NSArray isEqual:]` returns YES when `other` is an
    // NSArray with equal contents. Without this override the NSObject default
    // (pointer identity) is used, so two distinct-but-equal arrays — e.g. an
    // array and its unarchived copy — compare unequal, which breaks
    // `NSDictionary`/`NSArray` equality after a round-trip.
    let class: Class = msg_class![env; NSArray class];
    if !msg![env; other isKindOfClass:class] {
        return false;
    }
    msg![env; this isEqualToArray:other]
}

- (bool)isEqualToArray:(id)other { // NSArray*
    let count: NSUInteger = msg![env; this count];
    let other_count: NSUInteger = msg![env; other count];
    if count != other_count {
        return false;
    }
    for i in 0..count {
        let a: id = msg![env; this objectAtIndex:i];
        let b: id = msg![env; other objectAtIndex:i];
        let equal: bool = msg![env; a isEqual:b];
        if !equal {
            return false;
        }
    }
    true
}

- (NSUInteger)indexOfObject:(id)object inRange:(NSRange)range {
    for i in range.location..(range.location + range.length) {
        let curr: id = msg![env; this objectAtIndex:i];
        let equal: bool = msg![env; object isEqual:curr];
        if equal {
            return i;
        }
    }
    NSNotFound as NSUInteger
}

- (NSUInteger)indexOfObjectIdenticalTo:(id)object {
    let count: NSUInteger = msg![env; this count];
    for i in 0..count {
        let curr: id = msg![env; this objectAtIndex:i];
        if curr == object {
            return i;
        }
    }
    NSNotFound as NSUInteger
}

- (NSUInteger)indexOfObjectIdenticalTo:(id)object inRange:(NSRange)range {
    for i in range.location..(range.location + range.length) {
        let curr: id = msg![env; this objectAtIndex:i];
        if curr == object {
            return i;
        }
    }
    NSNotFound as NSUInteger
}

- (id)arrayByAddingObject:(id)object {
    let count: NSUInteger = msg![env; this count];
    let mut objects = Vec::with_capacity(count as usize + 1);
    for i in 0..count {
        let obj: id = msg![env; this objectAtIndex:i];
        retain(env, obj);
        objects.push(obj);
    }
    retain(env, object);
    objects.push(object);
    let arr = from_vec(env, objects);
    autorelease(env, arr)
}

- (id)arrayByAddingObjectsFromArray:(id)other { // NSArray*
    let count: NSUInteger = msg![env; this count];
    let other_count: NSUInteger = msg![env; other count];
    let mut objects = Vec::with_capacity((count + other_count) as usize);
    for i in 0..count {
        let obj: id = msg![env; this objectAtIndex:i];
        retain(env, obj);
        objects.push(obj);
    }
    for i in 0..other_count {
        let obj: id = msg![env; other objectAtIndex:i];
        retain(env, obj);
        objects.push(obj);
    }
    let arr = from_vec(env, objects);
    autorelease(env, arr)
}

- (id)subarrayWithRange:(NSRange)range {
    let mut objects = Vec::with_capacity(range.length as usize);
    for i in range.location..(range.location + range.length) {
        let obj: id = msg![env; this objectAtIndex:i];
        retain(env, obj);
        objects.push(obj);
    }
    let arr = from_vec(env, objects);
    autorelease(env, arr)
}

- (id)filteredArrayUsingPredicate:(id)predicate { // NSPredicate*
    let count: NSUInteger = msg![env; this count];
    let mut result = Vec::new();
    for i in 0..count {
        let obj: id = msg![env; this objectAtIndex:i];
        let matches: bool = msg![env; predicate evaluateWithObject:obj];
        if matches {
            retain(env, obj);
            result.push(obj);
        }
    }
    let arr = from_vec(env, result);
    autorelease(env, arr)
}

- (())makeObjectsPerformSelector:(SEL)sel {
    let count: NSUInteger = msg![env; this count];
    for i in 0..count {
        let obj: id = msg![env; this objectAtIndex:i];
        let _: id = msg![env; obj performSelector:sel];
    }
}

- (())encodeWithCoder:(id)coder {
    let class: Class = msg![env; coder class];
    let keyed_arch_class: Class = msg_class![env; NSKeyedArchiver class];

    if env.objc.class_is_subclass_of(class, keyed_arch_class) {
        let array = env.objc.borrow::<ArrayHostObject>(this).array.clone();
        // NSKeyedArchiver stores an array's contents as an inline array of UID
        // references under "NS.objects" (Apple's format, which is what our
        // decoder reads back). See `encode_objects_as_uid_array`.
        ns_keyed_archiver::encode_objects_as_uid_array(env, coder, "NS.objects", &array);
    } else {
        log!(
            "Warning: NSArray encodeWithCoder: unsupported coder class, skipping"
        );
    }
}

- (())makeObjectsPerformSelector:(SEL)sel withObject:(id)arg {
    let count: NSUInteger = msg![env; this count];
    for i in 0..count {
        let obj: id = msg![env; this objectAtIndex:i];
        let _: id = msg![env; obj performSelector:sel withObject:arg];
    }
}

// Apple's
// <https://developer.apple.com/documentation/foundation/nsarray/1415846-enumerateobjectsusingblock>:
// iterates the receiver and, for each element, calls the supplied
// `void (^)(id obj, NSUInteger idx, BOOL *stop)` block in order. The
// block can write `*stop = YES;` to break out of the enumeration. The
// `BOOL *stop` argument is *always* a pointer to a freshly-zeroed
// `BOOL` provided by the caller — never a re-used storage location.
- (())enumerateObjectsUsingBlock:(MutVoidPtr)block {
    let opts: NSUInteger = 0;
    () = msg![env; this enumerateObjectsWithOptions:opts usingBlock:block];
}

// Apple's
// <https://developer.apple.com/documentation/foundation/nsarray/1415349-enumerateobjectswithoptions>.
// `NSEnumerationOptions` is a bitmask:
//   NSEnumerationConcurrent = 1 << 0
//   NSEnumerationReverse    = 1 << 1
// We deliberately ignore `NSEnumerationConcurrent` (we always enumerate
// serially, which is allowed: the docs say concurrent enumeration is
// merely *available* on platforms that support it).
- (())enumerateObjectsWithOptions:(NSUInteger)opts usingBlock:(MutVoidPtr)block {
    const NS_ENUMERATION_REVERSE: NSUInteger = 1 << 1;
    if block.is_null() {
        return;
    }
    let invoke_ptr_addr: MutPtr<u32> =
        Ptr::from_bits(block.to_bits() + 12);
    let invoke_addr: u32 = env.mem.read(invoke_ptr_addr);
    if invoke_addr == 0 {
        log!(
            "Warning: enumerateObjectsWithOptions:usingBlock: block at {:?} \
             has NULL invoke pointer; skipping.",
            block
        );
        return;
    }
    let invoke = GuestFunction::from_addr_with_thumb_bit(invoke_addr);
    let block_arg: crate::mem::ConstVoidPtr = block.cast_const();
    // `BOOL` on iOS is one byte. We allocate a 4-byte slot because the
    // ARMv7 ABI passes / returns small values widened to a word, and
    // returning the unused tail to the heap costs nothing.
    let stop_ptr: MutPtr<u8> = env.mem.alloc(4).cast();
    env.mem.write(stop_ptr, 0u8);

    let count: NSUInteger = msg![env; this count];
    let reverse = (opts & NS_ENUMERATION_REVERSE) != 0;
    let mut i: NSUInteger = 0;
    while i < count {
        let idx: NSUInteger = if reverse { count - 1 - i } else { i };
        let obj: id = msg![env; this objectAtIndex:idx];
        <GuestFunction as CallFromHost<(), (crate::mem::ConstVoidPtr, id, NSUInteger, MutPtr<u8>)>>::call_from_host(
            &invoke, env, (block_arg, obj, idx, stop_ptr),
        );
        if env.mem.read(stop_ptr) != 0 {
            break;
        }
        i += 1;
    }
    env.mem.free(stop_ptr.cast());
}

- (id)valueForKey:(id)key { // NSString*
    let count: NSUInteger = msg![env; this count];
    let mut result = Vec::with_capacity(count as usize);
    for i in 0..count {
        let obj: id = msg![env; this objectAtIndex:i];
        let val: id = msg![env; obj valueForKey:key];
        retain(env, val);
        result.push(val);
    }
    let arr = from_vec(env, result);
    autorelease(env, arr)
}

- (())setValue:(id)value forKey:(id)key {
    let count: NSUInteger = msg![env; this count];
    for i in 0..count {
        let obj: id = msg![env; this objectAtIndex:i];
        () = msg![env; obj setValue:value forKey:key];
    }
}

@end

// NSMutableArray is an abstract class. A subclass must provide everything
// NSArray provides, plus:
// - (void)insertObject:(id)object atIndex:(NSUInteger)index;
// - (void)removeObjectAtIndex:(NSUInteger)index;
// - (void)addObject:(id)object;
// - (void)removeLastObject
// - (void)replaceObjectAtIndex:(NSUInteger)index withObject:(id)object;
// Note that it inherits from NSArray, so we must ensure we override any default
// methods that would be inappropriate for mutability.
@implementation NSMutableArray: NSArray

+ (id)allocWithZone:(NSZonePtr)zone {
    // NSMutableArray might be subclassed by something which needs
    // allocWithZone: to have the normal behaviour. Warn and fall back to the
    // bridged subclass instead of panicking.
    let mutable_class = env.objc.get_known_class("NSMutableArray", &mut env.mem);
    if this != mutable_class {
        log!(
            "Warning: +[NSMutableArray allocWithZone:] called on subclass {:?}; \
             treating as NSMutableArray.",
            this
        );
    }
    msg_class![env; _touchHLE_NSMutableArray allocWithZone:zone]
}

+ (id)arrayWithCapacity:(NSUInteger)capacity {
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithCapacity:capacity];
    autorelease(env, new)
}

+ (id)arrayWithArray:(id)array {
    let new: id = msg![env; this alloc];
    () = msg![env; new addObjectsFromArray:array];
    autorelease(env, new)
}

+ (id)arrayWithObjects:(id)first_obj, ...args {
    let objects = if first_obj == nil {
        vec![]
    } else {
        retain(env, first_obj);
        let mut objects = vec![first_obj];
        let mut varargs = args.start();
        loop {
            let next_arg: id = varargs.next(env);
            if next_arg.is_null() {
                break;
            }
            retain(env, next_arg);
            objects.push(next_arg);
        }
        objects
    };
    let array = mutable_from_vec(env, objects);
    autorelease(env, array)
}

// These probably comes from some category related to plists.
- (id)initWithContentsOfFile:(id)path { // NSString*
    release(env, this);
    let path = ns_string::to_rust_string(env, path);
    let tmp = deserialize_plist_from_file(
        env,
        GuestPath::new(&path),
        /* array_expected: */ true,
    );
    if tmp == nil {
        return nil;
    }
    // We should respect mutability of the top most container!
    let res = msg_class![env; NSMutableArray alloc];
    let res = msg![env; res initWithArray:tmp];
    release(env, tmp);
    res
}
- (id)initWithContentsOfURL:(id)url { // NSURL*
    release(env, this);
    let path = ns_url::to_rust_path(env, url);
    let tmp = deserialize_plist_from_file(env, &path, /* array_expected: */ true);
    if tmp == nil {
        return nil;
    }
    // We should respect mutability of the top most container!
    let res = msg_class![env; NSMutableArray alloc];
    let res = msg![env; res initWithArray:tmp];
    release(env, tmp);
    res
}

- (())addObjectsFromArray:(id)other { // NSArray*
    let enumerator: id = msg![env; other objectEnumerator];
    loop {
        let next: id = msg![env; enumerator nextObject];
        if next == nil {
            break;
        }
        () = msg![env; this addObject:next];
    }
}

// NSCopying implementation
- (id)copyWithZone:(NSZonePtr)_zone {
    let other: id = msg_class![env; NSArray alloc];
    let other: id = msg![env; other initWithArray:this];
    other
}

- (())removeObjectsInArray:(id)other { // NSArray*
    let count: NSUInteger = msg![env; other count];
    for i in 0..count {
        let obj: id = msg![env; other objectAtIndex:i];
        () = msg![env; this removeObject:obj];
    }
}

- (())removeObjectsInRange:(NSRange)range {
    // Remove in reverse order to preserve indices.
    let end = range.location + range.length;
    let mut i = end;
    while i > range.location {
        i -= 1;
        () = msg![env; this removeObjectAtIndex:i];
    }
}

- (())removeObjectIdenticalTo:(id)object {
    let count: NSUInteger = msg![env; this count];
    let mut indices = Vec::new();
    for i in 0..count {
        let curr: id = msg![env; this objectAtIndex:i];
        if curr == object {
            indices.push(i);
        }
    }
    for i in indices.into_iter().rev() {
        () = msg![env; this removeObjectAtIndex:i];
    }
}

- (())exchangeObjectAtIndex:(NSUInteger)idx1 withObjectAtIndex:(NSUInteger)idx2 {
    env.objc
        .borrow_mut::<ArrayHostObject>(this)
        .array
        .swap(idx1 as usize, idx2 as usize);
}

- (())setArray:(id)other { // NSArray*
    () = msg![env; this removeAllObjects];
    () = msg![env; this addObjectsFromArray:other];
}

- (())filterUsingPredicate:(id)predicate { // NSPredicate*
    let count: NSUInteger = msg![env; this count];
    let mut to_remove = Vec::new();
    for i in 0..count {
        let obj: id = msg![env; this objectAtIndex:i];
        let matches: bool = msg![env; predicate evaluateWithObject:obj];
        if !matches {
            to_remove.push(i);
        }
    }
    for i in to_remove.into_iter().rev() {
        () = msg![env; this removeObjectAtIndex:i];
    }
}

- (())insertObjects:(id)objects atIndexes:(id)indexes { // NSArray*, NSIndexSet*
    let count: NSUInteger = msg![env; objects count];
    let total_idx: NSUInteger = msg![env; indexes count];
    if count != total_idx {
        log!("Warning: insertObjects:atIndexes: count mismatch, ignoring");
        return;
    }
    // Collect sorted indices and insert in ascending order.
    let arr_count: NSUInteger = msg![env; this count];
    let mut pairs: Vec<(NSUInteger, id)> = Vec::new();
    let mut obj_i: NSUInteger = 0;
    for idx in 0..arr_count + count {
        let contains: bool = msg![env; indexes containsIndex:idx];
        if contains {
            let obj: id = msg![env; objects objectAtIndex:obj_i];
            pairs.push((idx, obj));
            obj_i += 1;
        }
    }
    // Insert in reverse so earlier insertions don't shift later indices.
    for (idx, obj) in pairs.into_iter().rev() {
        () = msg![env; this insertObject:obj atIndex:idx];
    }
}

@end

// Our private subclass that is the single implementation of NSArray for the
// time being.
@implementation _touchHLE_NSArray: NSArray

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(ArrayHostObject {
        array: Vec::new(),
    });
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

// NSCoding implementation
- (id)initWithCoder:(id)coder {
    let class: Class = msg![env; coder class];
    let keyed_unarch_class: Class = msg_class![env; NSKeyedUnarchiver class];
    let nib_archive_class: Class = msg_class![env; _touchHLE_NIBArchiveDecoder class];
    let objects = if env.objc.class_is_subclass_of(class, keyed_unarch_class) {
    // It seems that every NSArray item in an NSKeyedArchiver plist looks like:
    // {
    //   "$class" => (uid of NSArray class goes here),
    //   "NS.objects" => [
    //     // objects here
    //   ]
    // }
    // Presumably we need to call a `decodeFooBarForKey:` method on the NSCoder
    // here, passing in an NSString for "NS.objects". There is no method for
    // arrays though (maybe it's `decodeObjectForKey:`), and in any case
    // allocating an NSString here would be inconvenient, so let's just take a
    // shortcut.
    ns_keyed_unarchiver::decode_current_array(env, coder)
    } else if env.objc.class_is_subclass_of(class, nib_archive_class) {
        _nib_archive_decoder::decode_current_array(env, coder)
    } else {
        log!(
            "Warning: -[_touchHLE_NSArray initWithCoder:] unsupported coder class {:?}; \
             returning empty array.",
            class
        );
        Vec::new()
    };
    let host_object: &mut ArrayHostObject = env.objc.borrow_mut(this);
    if !host_object.array.is_empty() {
        log!(
            "Warning: -[_touchHLE_NSArray initWithCoder:] called on an already-populated array; \
             releasing existing contents first."
        );
        let prev = std::mem::take(&mut host_object.array);
        for obj in prev {
            release(env, obj);
        }
    }
    env.objc.borrow_mut::<ArrayHostObject>(this).array = objects;
    // objects are already retained
    this
}

- (id)initWithArray:(id)array { // NSArray*
    let mut objects = Vec::new();
    let enumerator: id = msg![env; array objectEnumerator];
    loop {
        let next: id = msg![env; enumerator nextObject];
        if next == nil {
            break;
        }
        objects.push(next);
        retain(env, next);
    }
    env.objc.borrow_mut::<ArrayHostObject>(this).array = objects;
    this
}

- (id)initWithObjects:(id)firstObj, ...args {
    retain(env, firstObj);
    let mut objects = vec![firstObj];
    let mut varargs = args.start();
    loop {
        let next_arg: id = varargs.next(env);
        if next_arg.is_null() {
            break;
        }
        retain(env, next_arg);
        objects.push(next_arg);
    }
    env.objc.borrow_mut::<ArrayHostObject>(this).array = objects;
    this
}

- (id)initWithObjects:(ConstPtr<id>)objects_ptr count:(NSUInteger)count {
    let mut objects = Vec::new();
    for i in 0..count {
        let obj: id = env.mem.read(objects_ptr + i);
        retain(env, obj);
        objects.push(obj);
    }
    env.objc.borrow_mut::<ArrayHostObject>(this).array = objects;
    this
}

- (())dealloc {
    let host_object: &mut ArrayHostObject = env.objc.borrow_mut(this);
    let array = std::mem::take(&mut host_object.array);
    for object in array {
        release(env, object);
    }

    env.objc.dealloc_object(this, &mut env.mem)
}

// NSMutableCopying implementation
- (id)mutableCopyWithZone:(NSZonePtr)_zone {
    mutable_copy_inner(env, this)
}

- (id)objectEnumerator { // NSEnumerator*
    object_enumerator_inner(env, this)
}
- (id)reverseObjectEnumerator { // NSEnumerator*
    reverse_object_enumerator_inner(env, this)
}

// NSFastEnumeration implementation
- (NSUInteger)countByEnumeratingWithState:(MutPtr<NSFastEnumerationState>)state
                                  objects:(MutPtr<id>)stackbuf
                                    count:(NSUInteger)len {
    let count: NSUInteger = msg![env; this count];
    fast_enumeration_helper(env, this, |env, idx| {
        if idx < count {
            msg![env; this objectAtIndex:idx]
        } else {
            nil
        }
    }, state, stackbuf, len)
}

// TODO: more init methods, etc

- (NSUInteger)count {
    env.objc.borrow::<ArrayHostObject>(this).array.len().try_into().unwrap()
}
- (id)objectAtIndex:(NSUInteger)index {
    let len = env.objc.borrow::<ArrayHostObject>(this).array.len();
    if index as usize >= len {
        log!("Warning: NSArray objectAtIndex: index out of bounds (index {}, len {})", index, len);
        return nil;
    }
    env.objc.borrow::<ArrayHostObject>(this).array[index as usize]
}
// Modern Objective-C subscripting bridge:
//   id obj = array[idx];   // compiled to objectAtIndexedSubscript:
// Defined on NSArray (iOS 6+ ObjC literals); semantics are identical to
// `objectAtIndex:`, including out-of-bounds behaviour. We delegate via
// `msg![]` rather than reaching into the host object so that subclasses
// (e.g. CFArray, KVO-aware mutable subclasses) get the right behaviour.
// <https://developer.apple.com/documentation/foundation/nsarray/1410519-objectatindexedsubscript>
- (id)objectAtIndexedSubscript:(NSUInteger)index {
    msg![env; this objectAtIndex:index]
}

- (id)description {
    build_description(env, this)
}

- (())addObject:(id)object {
    retain(env, object);
    env.objc.borrow_mut::<ArrayHostObject>(this).array.push(object);
}

- (id)subarrayWithRange:(NSRange)range {
    let mut tmp = Vec::new();
    tmp.extend_from_slice(
        &env.objc.borrow::<ArrayHostObject>(this).array[range.location as usize..(range.location + range.length) as usize]
    );
    for &obj in &tmp {
        retain(env, obj);
    }
    let res = from_vec(env, tmp);
    autorelease(env, res)
}

- (id)sortedArrayUsingSelector:(SEL)comparator {
    let new = msg![env; this mutableCopy];
    () = msg![env; new sortUsingSelector:comparator];
    autorelease(env, new)
}

// `- (NSArray *)sortedArrayUsingComparator:(NSComparator)cmptr` —
// per Apple's [NSArray Reference](https://developer.apple.com/documentation/foundation/nsarray/1411124-sortedarrayusingcomparator):
// returns a new sorted array using the given NSComparator block. We
// implement it by copying the receiver into a mutable array and
// delegating to `-sortUsingComparator:` on the mutable copy, then
// returning an autoreleased copy. This matches `-sortedArrayUsingSelector:`
// above.
- (id)sortedArrayUsingComparator:(id)comparator {
    let new = msg![env; this mutableCopy];
    () = msg![env; new sortUsingComparator:comparator];
    autorelease(env, new)
}

@end

// Special variant for use by CFArray with NULL callbacks: objects aren't
// necessarily Objective-C objects and won't be retained/released.
@implementation _touchHLE_NSArray_non_retaining: _touchHLE_NSArray

- (())dealloc {
    env.objc.dealloc_object(this, &mut env.mem)
}

@end

@implementation _touchHLE_NSArray_ObjectEnumerator: NSEnumerator

- (id)nextObject {
    let host_obj = env.objc.borrow_mut::<ObjectEnumeratorHostObject>(this);
    host_obj.iterator.next().map_or(nil, |o| o)
}

- (())dealloc {
    let host_obj = env.objc.borrow::<ObjectEnumeratorHostObject>(this);
    release(env, host_obj.array);
    env.objc.dealloc_object(this, &mut env.mem)
}

@end

// Our private subclass that is the single implementation of NSMutableArray for
// the time being.
@implementation _touchHLE_NSMutableArray: NSMutableArray

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(ArrayHostObject {
        array: Vec::new(),
    });
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

- (id)initWithCapacity:(NSUInteger)capacity {
    env.objc.borrow_mut::<ArrayHostObject>(this).array.reserve(capacity as usize);
    this
}

- (id)initWithArray:(id)array { // NSArray*
    let mut objects = Vec::new();
    let enumerator: id = msg![env; array objectEnumerator];
    loop {
        let next: id = msg![env; enumerator nextObject];
        if next == nil {
            break;
        }
        objects.push(next);
        retain(env, next);
    }
    env.objc.borrow_mut::<ArrayHostObject>(this).array = objects;
    this
}

- (id)initWithObjects:(id)firstObj, ...args {
    retain(env, firstObj);
    let mut objects = vec![firstObj];
    let mut varargs = args.start();
    loop {
        let next_arg: id = varargs.next(env);
        if next_arg.is_null() {
            break;
        }
        retain(env, next_arg);
        objects.push(next_arg);
    }
    env.objc.borrow_mut::<ArrayHostObject>(this).array = objects;
    this
}

- (id)initWithObjects:(ConstPtr<id>)objects_ptr count:(NSUInteger)count {
    let mut objects = Vec::with_capacity(count as usize);
    for i in 0..count {
        let obj: id = env.mem.read(objects_ptr + i);
        retain(env, obj);
        objects.push(obj);
    }
    env.objc.borrow_mut::<ArrayHostObject>(this).array = objects;
    this
}

// NSCoding implementation
- (id)initWithCoder:(id)coder {
    let class: Class = msg![env; coder class];
    let keyed_unarch_class: Class = msg_class![env; NSKeyedUnarchiver class];
    let nib_archive_class: Class = msg_class![env; _touchHLE_NIBArchiveDecoder class];

    let objects = if env.objc.class_is_subclass_of(class, keyed_unarch_class) {
        ns_keyed_unarchiver::decode_current_array(env, coder)
    } else if env.objc.class_is_subclass_of(class, nib_archive_class) {
        _nib_archive_decoder::decode_current_array(env, coder)
    } else {
        log!(
            "Warning: -[_touchHLE_NSMutableArray initWithCoder:] unsupported coder class {:?}; \
             returning empty array.",
            class
        );
        Vec::new()
    };
    let host_object: &mut ArrayHostObject = env.objc.borrow_mut(this);
    if !host_object.array.is_empty() {
        log!(
            "Warning: -[_touchHLE_NSMutableArray initWithCoder:] called on an already-populated array; \
             releasing existing contents first."
        );
        let prev = std::mem::take(&mut host_object.array);
        for obj in prev {
            release(env, obj);
        }
    }
    env.objc.borrow_mut::<ArrayHostObject>(this).array = objects;
    // objects are already retained
    this
}

// NSCopying implementation
- (id)copyWithZone:(NSZonePtr)_zone {
    let arr: id = msg_class![env; NSArray alloc];
    let array = env.objc.borrow::<ArrayHostObject>(this).array.clone();
    for &object in &array {
        retain(env, object);
    }
    env.objc.borrow_mut::<ArrayHostObject>(arr).array = array;
    arr
}

// NSMutableCopying implementation
- (id)mutableCopyWithZone:(NSZonePtr)_zone {
    mutable_copy_inner(env, this)
}

- (())dealloc {
    let host_object: &mut ArrayHostObject = env.objc.borrow_mut(this);
    let array = std::mem::take(&mut host_object.array);

    for object in array {
        release(env, object);
    }

    env.objc.dealloc_object(this, &mut env.mem)
}

- (())makeObjectsPerformSelector:(SEL)sel {
    let count: NSUInteger = msg![env; this count];
    for idx in 0..count {
        let obj: id = msg![env; this objectAtIndex:idx];
        let _: id = msg![env; obj performSelector:sel];
    }
}

- (id)objectEnumerator { // NSEnumerator*
    object_enumerator_inner(env, this)
}
- (id)reverseObjectEnumerator { // NSEnumerator*
    reverse_object_enumerator_inner(env, this)
}

- (())sortUsingFunction:(GuestFunction)comparator
                context:(MutVoidPtr)context {
    let host_object: &mut ArrayHostObject = env.objc.borrow_mut(this);
    let mut array = std::mem::take(&mut host_object.array);
    let len = array.len().try_into().unwrap();
    let mut user_data = (env, &mut array);
    qsort_generic(
        &mut user_data,
        len,
        &mut |(env, array), l, r| {
            let (l, r): (usize, usize) = (l.try_into().unwrap(), r.try_into().unwrap());
            comparator.call_from_host(env, (array[l], array[r], context))
        },
        &mut |(_, array), l, r| {
            let (l, r): (usize, usize) = (l.try_into().unwrap(), r.try_into().unwrap());
            array.swap(l, r);
        },
    );
    let (env, _) = user_data;
    env.objc.borrow_mut::<ArrayHostObject>(this).array = array;
}

- (())sortUsingSelector:(SEL)comparator {
    let host_object: &mut ArrayHostObject = env.objc.borrow_mut(this);
    let mut array = std::mem::take(&mut host_object.array);
    let len = array.len().try_into().unwrap();
    let mut user_data = (env, &mut array);
    qsort_generic(
        &mut user_data,
        len,
        &mut |(env, array), l, r| {
            let (l, r): (usize, usize) = (l.try_into().unwrap(), r.try_into().unwrap());
            let res: NSComparisonResult = msg_send(env, (array[l], comparator, array[r]));
            res
        },
        &mut |(_, array), l, r| {
            let (l, r): (usize, usize) = (l.try_into().unwrap(), r.try_into().unwrap());
            array.swap(l, r);
        },
    );
    let (env, _) = user_data;
    env.objc.borrow_mut::<ArrayHostObject>(this).array = array;
}

// `- (void)sortUsingComparator:(NSComparator)cmptr` —
// per Apple's [NSMutableArray Reference](https://developer.apple.com/documentation/foundation/nsmutablearray/1413612-sortusingcomparator):
// sorts the receiver in place using the supplied NSComparator block.
// `NSComparator` is `^NSComparisonResult(id obj1, id obj2)`. An ObjC
// block on 32-bit iOS is laid out as:
//     struct Block_layout {
//         void *isa;            // word 0
//         int flags;            // word 1
//         int reserved;         // word 2
//         void (*invoke)(...);  // word 3  <-- the function pointer
//         struct Block_descriptor_1 *descriptor; // word 4
//         /* captured variables follow */
//     };
// We therefore invoke the block by calling `block->invoke(block, obj1,
// obj2)` with the standard ARM AAPCS calling convention. A nil block is
// treated as "leave the array in its current order", mirroring how
// Apple's runtime aborts with a NULL block call — touchHLE just logs
// and returns to keep the guest alive.
- (())sortUsingComparator:(id)block {
    if block == nil {
        log!("Warning: -[NSMutableArray sortUsingComparator:] called with nil block; leaving array unsorted");
        return;
    }
    let invoke_ptr: u32 = env.mem.read(block.cast::<u32>() + 3u32);
    if invoke_ptr == 0 {
        log!("Warning: -[NSMutableArray sortUsingComparator:] block {:?} has NULL invoke pointer; leaving array unsorted", block);
        return;
    }
    let invoke = GuestFunction::from_addr_with_thumb_bit(invoke_ptr);

    let host_object: &mut ArrayHostObject = env.objc.borrow_mut(this);
    let mut array = std::mem::take(&mut host_object.array);
    let len = array.len().try_into().unwrap();
    let mut user_data = (env, &mut array);
    qsort_generic(
        &mut user_data,
        len,
        &mut |(env, array), l, r| {
            let (l, r): (usize, usize) = (l.try_into().unwrap(), r.try_into().unwrap());
            let res: NSComparisonResult = invoke.call_from_host(env, (block, array[l], array[r]));
            res
        },
        &mut |(_, array), l, r| {
            let (l, r): (usize, usize) = (l.try_into().unwrap(), r.try_into().unwrap());
            array.swap(l, r);
        },
    );
    let (env, _) = user_data;
    env.objc.borrow_mut::<ArrayHostObject>(this).array = array;
}

- (())sortUsingDescriptors:(id)descriptors { // NSArray* of NSSortDescriptor*
    super::ns_sort_descriptor::sort_with_descriptors(env, this, descriptors);
}

// NSFastEnumeration implementation
- (NSUInteger)countByEnumeratingWithState:(MutPtr<NSFastEnumerationState>)state
                                  objects:(MutPtr<id>)stackbuf
                                    count:(NSUInteger)len {
    // TODO: check that array wasn't mutated!
    let count: NSUInteger = msg![env; this count];
    fast_enumeration_helper(env, this, |env, idx| {
        if idx < count {
            msg![env; this objectAtIndex:idx]
        } else {
            nil
        }
    }, state, stackbuf, len)
}

- (NSUInteger)count {
    env.objc.borrow::<ArrayHostObject>(this).array.len().try_into().unwrap()
}

- (id)objectAtIndex:(NSUInteger)index {
    let len = env.objc.borrow::<ArrayHostObject>(this).array.len();
    if index as usize >= len {
        log!("Warning: NSMutableArray objectAtIndex: index out of bounds (index {}, len {})", index, len);
        return nil;
    }
    env.objc.borrow::<ArrayHostObject>(this).array[index as usize]
}

- (id)description {
    build_description(env, this)
}

// TODO: more mutation methods

- (())insertObject:(id)object
           atIndex:(NSUInteger)index {
    let len = env.objc.borrow::<ArrayHostObject>(this).array.len();
    if index as usize > len {
        log!("Warning: NSMutableArray insertObject:atIndex: index out of bounds");
        return;
    }
    retain(env, object);
    env.objc.borrow_mut::<ArrayHostObject>(this).array.insert(index as usize, object);
}

- (())addObject:(id)object {
    retain(env, object);
    env.objc.borrow_mut::<ArrayHostObject>(this).array.push(object);
}

- (())removeObject:(id)object {
    let mut to_remove = Vec::new();
    let count: NSUInteger = msg![env; this count];
    for i in 0..count {
        let curr_object: id = msg![env; this objectAtIndex:i];
        let equal: bool = msg![env; object isEqual:curr_object];
        if equal {
            to_remove.push(i);
        }
    }
    // TODO: runtime here is O(n^2), it could be O(n) instead
    for i in to_remove {
        () = msg![env; this removeObjectAtIndex:i];
    }
}

- (())removeObjectAtIndex:(NSUInteger)index {
    let len = env.objc.borrow::<ArrayHostObject>(this).array.len();
    if index as usize >= len {
        log!("Warning: NSMutableArray removeObjectAtIndex: index out of bounds");
        return;
    }
    let object = env.objc.borrow_mut::<ArrayHostObject>(this).array.remove(index as usize);
    release(env, object)
}

- (())replaceObjectAtIndex:(NSUInteger)index withObject:(id)obj {
    let len = env.objc.borrow::<ArrayHostObject>(this).array.len();
    if index as usize >= len {
        log!("Warning: NSMutableArray replaceObjectAtIndex:withObject: index out of bounds");
        return;
    }
    retain(env, obj);
    let object = std::mem::replace(&mut env.objc.borrow_mut::<ArrayHostObject>(this).array[index as usize], obj);
    release(env, object);
}

// Modern Objective-C subscripting bridge for mutable arrays:
//   array[idx] = obj;     // compiled to setObject:atIndexedSubscript:
// `index` may equal `count`, in which case the object is appended (matches
// Apple's documented behaviour). Out-of-range indices fall back to a
// warning rather than panicking the host so that broken guests don't crash
// the whole emulator. Setting `nil` is treated like
// `removeObjectAtIndex:`, mirroring real NSMutableArray semantics.
// <https://developer.apple.com/documentation/foundation/nsmutablearray/1416687-setobject>
- (())setObject:(id)obj atIndexedSubscript:(NSUInteger)index {
    let len = env.objc.borrow::<ArrayHostObject>(this).array.len() as NSUInteger;
    if obj == nil {
        if (index as usize) < len as usize {
            () = msg![env; this removeObjectAtIndex:index];
        } else {
            log!(
                "Warning: NSMutableArray setObject:atIndexedSubscript: nil at out-of-range index {}",
                index
            );
        }
        return;
    }
    if index == len {
        () = msg![env; this addObject:obj];
    } else if index < len {
        () = msg![env; this replaceObjectAtIndex:index withObject:obj];
    } else {
        log!(
            "Warning: NSMutableArray setObject:atIndexedSubscript: index {} > count {}",
            index, len
        );
    }
}

- (())removeLastObject {
    let object_opt = env.objc.borrow_mut::<ArrayHostObject>(this).array.pop();
    if let Some(object) = object_opt {
        release(env, object)
    } else {
        log!("Warning: NSMutableArray removeLastObject: array is empty");
    }
}

- (())removeAllObjects {
    let host_object: &mut ArrayHostObject = env.objc.borrow_mut(this);
    let array = std::mem::take(&mut host_object.array);
    for object in array {
        release(env, object);
    }

    env.objc.borrow_mut::<ArrayHostObject>(this).array = Vec::new()
}

// Apple docs: Returns an array that lists the receiving array's elements in
// ascending order, as determined by the comparison method specified by a given
// selector. The new array contains references to the receiving array's
// elements, not copies of them.
- (id)sortedArrayUsingSelector:(SEL)comparator {
    let new = msg![env; this mutableCopy];
    () = msg![env; new sortUsingSelector:comparator];
    autorelease(env, new)
}

// Apple docs: Returns an array that lists the receiving array's elements in
// ascending order as defined by the comparison function comparator.
- (id)sortedArrayUsingFunction:(GuestFunction)comparator
                       context:(MutVoidPtr)context {
    let new = msg![env; this mutableCopy];
    () = msg![env; new sortUsingFunction:comparator context:context];
    autorelease(env, new)
}

@end

// Special variant for use by CFArray with NULL callbacks: objects aren't
// necessarily Objective-C objects and won't be retained/released.
@implementation _touchHLE_NSMutableArray_non_retaining: _touchHLE_NSMutableArray

- (())dealloc {
    env.objc.dealloc_object(this, &mut env.mem)
}

- (())addObject:(id)object {
    env.objc.borrow_mut::<ArrayHostObject>(this).array.push(object);
}

- (())removeObjectAtIndex:(NSUInteger)index {
    let len = env.objc.borrow::<ArrayHostObject>(this).array.len();
    if index as usize >= len {
        log!("Warning: NSMutableArray_non_retaining removeObjectAtIndex: index out of bounds");
        return;
    }
    env.objc.borrow_mut::<ArrayHostObject>(this).array.remove(index as usize);
}

- (())removeLastObject {
    let popped = env.objc.borrow_mut::<ArrayHostObject>(this).array.pop();
    if popped.is_none() {
        log!("Warning: NSMutableArray_non_retaining removeLastObject: array is empty");
    }
}

@end

};
/// Shortcut for host code, roughly equivalent to
/// `[[NSArray alloc] initWithObjects:count]` but without copying.
/// The elements should already be "retained by" the `Vec`.
pub fn from_vec(env: &mut Environment, objects: Vec<id>) -> id {
    let array: id = msg_class![env; NSArray alloc];
    env.objc.borrow_mut::<ArrayHostObject>(array).array = objects;
    array
}

/// Shortcut for host code, roughly equivalent to
/// `[[NSMutableArray alloc] initWithObjects:count]` but without copying.
/// The elements should already be "retained by" the `Vec`.
pub fn mutable_from_vec(env: &mut Environment, objects: Vec<id>) -> id {
    let array: id = msg_class![env; NSMutableArray alloc];
    env.objc.borrow_mut::<ArrayHostObject>(array).array = objects;
    array
}

/// A helper to build a description NSString
/// for a NSArray or a NSMutableArray.
fn build_description(env: &mut Environment, arr: id) -> id {
    // According to docs, this description should be formatted as property list.
    // But by the same docs, it's meant to be used for debugging purposes only.
    let desc: id = msg_class![env; NSMutableString new];
    let prefix: id = ns_string::from_rust_string(env, "(\n".to_string());
    () = msg![env; desc appendString:prefix];
    release(env, prefix);
    let values: Vec<id> = env.objc.borrow_mut::<ArrayHostObject>(arr).array.clone();
    for value in values {
        let value_desc: id = msg![env; value description];
        // TODO: respect nesting and padding
        let format = format!("\t{},\n", ns_string::to_rust_string(env, value_desc));
        let format = ns_string::from_rust_string(env, format);
        () = msg![env; desc appendString:format];
        release(env, format);
    }
    let suffix: id = ns_string::from_rust_string(env, ")".to_string());
    () = msg![env; desc appendString:suffix];
    release(env, suffix);
    let desc_imm = msg![env; desc copy];
    release(env, desc);
    autorelease(env, desc_imm)
}

/// A shared objectEnumerator helper method.
fn object_enumerator_inner(env: &mut Environment, arr: id) -> id {
    let array_host_object: &mut ArrayHostObject = env.objc.borrow_mut(arr);
    let vec = array_host_object.array.to_vec();
    object_enumerator_inner_helper(env, arr, vec)
}

/// A shared reverseObjectEnumerator helper method.
fn reverse_object_enumerator_inner(env: &mut Environment, arr: id) -> id {
    let array_host_object: &mut ArrayHostObject = env.objc.borrow_mut(arr);
    // TODO: avoid copying?
    let vec = array_host_object
        .array
        .iter()
        .rev()
        .cloned()
        .collect::<Vec<_>>();
    object_enumerator_inner_helper(env, arr, vec)
}

fn object_enumerator_inner_helper(env: &mut Environment, arr: id, vec: Vec<id>) -> id {
    let host_object = Box::new(ObjectEnumeratorHostObject {
        array: arr,
        iterator: vec.into_iter(),
    });
    retain(env, arr);
    let class = env
        .objc
        .get_known_class("_touchHLE_NSArray_ObjectEnumerator", &mut env.mem);
    let enumerator = env.objc.alloc_object(class, host_object, &mut env.mem);
    autorelease(env, enumerator)
}

fn mutable_copy_inner(env: &mut Environment, arr: id) -> id {
    let mut_arr: id = msg_class![env; NSMutableArray alloc];
    let array = env.objc.borrow::<ArrayHostObject>(arr).array.clone();
    for &object in &array {
        retain(env, object);
    }
    env.objc.borrow_mut::<ArrayHostObject>(mut_arr).array = array;
    mut_arr
}
