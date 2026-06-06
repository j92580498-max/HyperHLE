/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `CGDataProvider.h`

use super::cg_image::{self, CGImageRef, CGImageRelease, CGImageRetain};
use crate::abi::{CallFromHost, GuestFunction};
use crate::dyld::FunctionExports;
use crate::export_c_func;
use crate::frameworks::core_foundation::cf_allocator::kCFAllocatorDefault;
use crate::frameworks::core_foundation::cf_data::{
    CFDataCreate, CFDataGetBytePtr, CFDataGetLength, CFDataRef,
};
use crate::frameworks::core_foundation::cf_url::CFURLRef;
use crate::frameworks::core_foundation::{CFRelease, CFRetain, CFTypeRef};
use crate::frameworks::foundation::ns_string::to_rust_string;
use crate::frameworks::foundation::NSUInteger;
use crate::fs::GuestPath;
use crate::mem::{ConstPtr, ConstVoidPtr, GuestUSize, MutVoidPtr};
use crate::objc::{id, msg, msg_class, nil, objc_classes, ClassExports, HostObject};
use crate::Environment;

pub type CGDataProviderRef = CFTypeRef;

/// `(*void)(void *info, const void *data, size_t size)`
type CGDataProviderReleaseDataCallback = GuestFunction;

// A CGDataProvider is supposed to be a collection of callbacks used for
// accessing data, but at least for now, we instead only support some specific
// use-cases.

enum CGDataProviderHostObject {
    DataWithSize {
        data: ConstVoidPtr,
        size: GuestUSize,
        /// User-provided pointer passed to release callback.
        info: MutVoidPtr,
        release_callback: CGDataProviderReleaseDataCallback,
    },
    /// Created via CGDataProviderCreateDirect. The release callback
    /// signature is `void (*releaseInfo)(void *info)` — only takes info.
    Direct {
        data: ConstVoidPtr,
        size: GuestUSize,
        info: MutVoidPtr,
        release_info_callback: GuestFunction,
    },
    // TODO: Maybe we should store image data in guest memory so we don't
    // need a special variant for this.
    CGImage(CGImageRef),
    CFData(CFDataRef),
}
impl Default for CGDataProviderHostObject {
    // Phantom-fallback value; a `CFData(nil)` variant is the cheapest "no
    // data" form and doesn't reference any guest memory.
    fn default() -> Self {
        CGDataProviderHostObject::CFData(nil)
    }
}
impl HostObject for CGDataProviderHostObject {}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

// CGDataProvider is a CFType-based type, but in our implementation those
// are just Objective-C types, so we need a class for it, but its name is not
// visible anywhere.
@implementation _touchHLE_CGDataProvider: NSObject

- (())dealloc {
    match *env.objc.borrow(this) {
        CGDataProviderHostObject::DataWithSize {
            info,
            data,
            size,
            release_callback,
        } => {
            if !release_callback.to_ptr().is_null() {
                let args: (MutVoidPtr, ConstVoidPtr, GuestUSize) = (info, data, size);
                log_dbg!(
                    "Freeing {:?}, calling release callback {:?} with {:?}",
                    this,
                    release_callback,
                    args,
                );
                () = release_callback.call_from_host(env, args);
            }
        },
        CGDataProviderHostObject::Direct {
            info,
            release_info_callback,
            ..
        } => {
            if !release_info_callback.to_ptr().is_null() {
                log_dbg!(
                    "Freeing Direct provider {:?}, calling releaseInfo {:?} with info={:?}",
                    this,
                    release_info_callback,
                    info,
                );
                () = release_info_callback.call_from_host(env, (info,));
            }
        },
        CGDataProviderHostObject::CGImage(cg_image) => CGImageRelease(env, cg_image),
        CGDataProviderHostObject::CFData(cf_data) => CFRelease(env, cf_data),
    }
    env.objc.dealloc_object(this, &mut env.mem)
}

@end

};

pub fn CGDataProviderRelease(env: &mut Environment, c: CGDataProviderRef) {
    if !c.is_null() {
        CFRelease(env, c);
    }
}
pub fn CGDataProviderRetain(env: &mut Environment, c: CGDataProviderRef) -> CGDataProviderRef {
    if !c.is_null() {
        CFRetain(env, c)
    } else {
        c
    }
}

fn CGDataProviderCreateWithData(
    env: &mut Environment,
    info: MutVoidPtr,
    data: ConstVoidPtr,
    size: GuestUSize,
    release_callback: CGDataProviderReleaseDataCallback,
) -> CGDataProviderRef {
    let class = env
        .objc
        .get_known_class("_touchHLE_CGDataProvider", &mut env.mem);
    env.objc.alloc_object(
        class,
        Box::new(CGDataProviderHostObject::DataWithSize {
            info,
            data,
            size,
            release_callback,
        }),
        &mut env.mem,
    )
}

#[allow(rustdoc::broken_intra_doc_links)] // https://github.com/rust-lang/rust/issues/83049
/// This is for use by [super::cg_image::CGImageGetDataProvider].
pub(super) fn from_cg_image(env: &mut Environment, cg_image: CGImageRef) -> CGDataProviderRef {
    CGImageRetain(env, cg_image);
    let class = env
        .objc
        .get_known_class("_touchHLE_CGDataProvider", &mut env.mem);
    env.objc.alloc_object(
        class,
        Box::new(CGDataProviderHostObject::CGImage(cg_image)),
        &mut env.mem,
    )
}

/// Generic interface for host code.
pub(super) fn borrow_bytes(env: &mut Environment, provider: CGDataProviderRef) -> &[u8] {
    match *env.objc.borrow(provider) {
        CGDataProviderHostObject::DataWithSize { data, size, .. } => {
            env.mem.bytes_at(data.cast(), size)
        }
        CGDataProviderHostObject::Direct { data, size, .. } => {
            env.mem.bytes_at(data.cast(), size)
        }
        CGDataProviderHostObject::CGImage(cg_image) => {
            cg_image::borrow_image(&env.objc, cg_image).pixels()
        }
        CGDataProviderHostObject::CFData(cf_data) => {
            let data = CFDataGetBytePtr(env, cf_data);
            let size = CFDataGetLength(env, cf_data);
            env.mem.bytes_at(data, size.try_into().unwrap())
        }
    }
}

fn CGDataProviderCopyData(env: &mut Environment, provider: CGDataProviderRef) -> CFDataRef {
    match *env.objc.borrow(provider) {
        CGDataProviderHostObject::DataWithSize { data, size, .. } => CFDataCreate(
            env,
            kCFAllocatorDefault,
            data.cast(),
            size.try_into().unwrap(),
        ),
        CGDataProviderHostObject::Direct { data, size, .. } => CFDataCreate(
            env,
            kCFAllocatorDefault,
            data.cast(),
            size.try_into().unwrap(),
        ),
        CGDataProviderHostObject::CGImage(cg_image) => {
            let bytes = cg_image::borrow_image(&env.objc, cg_image).pixels();

            let len: NSUInteger = bytes.len().try_into().unwrap();
            let alloc = env.mem.alloc(len);
            env.mem
                .bytes_at_mut(alloc.cast(), len)
                .copy_from_slice(bytes);

            // TODO: it would be cleaner to use CFDataCreateWithBytesNoCopy, but
            // that's a bit more tricky.
            let ns_data: id = msg_class![env; NSData alloc];
            msg![env; ns_data initWithBytesNoCopy:alloc length:len]
        }
        CGDataProviderHostObject::CFData(cf_data) => {
            let data = CFDataGetBytePtr(env, cf_data);
            let size = CFDataGetLength(env, cf_data);
            CFDataCreate(env, kCFAllocatorDefault, data.cast(), size)
        }
    }
}

fn CGDataProviderCreateWithURL(env: &mut Environment, url: CFURLRef) -> CGDataProviderRef {
    assert!(msg![env; url isFileURL]); // TODO
    let path: id = msg![env; url path];
    log_dbg!(
        "CGDataProviderCreateWithURL url path {}",
        to_rust_string(env, path)
    );
    let data: id = msg_class![env; NSData dataWithContentsOfFile:path];
    CGDataProviderCreateWithCFData(env, data)
}

fn CGDataProviderCreateWithCFData(env: &mut Environment, data: CFDataRef) -> CGDataProviderRef {
    CFRetain(env, data);
    let class = env
        .objc
        .get_known_class("_touchHLE_CGDataProvider", &mut env.mem);
    env.objc.alloc_object(
        class,
        Box::new(CGDataProviderHostObject::CFData(data)),
        &mut env.mem,
    )
}

fn CGDataProviderCreateWithFilename(
    env: &mut Environment,
    filename: crate::mem::ConstPtr<u8>,
) -> CGDataProviderRef {
    let path_str = env.mem.cstr_at_utf8(filename).unwrap_or("").to_string();
    log_dbg!("CGDataProviderCreateWithFilename: {}", path_str);
    let Ok(bytes) = env.fs.read(GuestPath::new(&path_str)) else {
        log!(
            "Warning: CGDataProviderCreateWithFilename: couldn't read {:?}",
            path_str
        );
        return nil; // <- was std::ptr::null()
    };
    let len: GuestUSize = bytes.len().try_into().unwrap();
    let buf = env.mem.alloc(len);
    env.mem
        .bytes_at_mut(buf.cast(), len)
        .copy_from_slice(&bytes);

    CGDataProviderCreateWithData(
        env,
        MutVoidPtr::null(),
        buf.cast_const().cast(),
        len,
        GuestFunction::null_ptr(), // <- was GuestFunction::from_ptr(...)
    )
}

fn CGDataProviderGetInfo(_env: &mut Environment, _provider: CGDataProviderRef) -> MutVoidPtr {
    // Real API returns the `info` pointer passed at creation time.
    // We don't expose it publicly; return null as a safe stub.
    MutVoidPtr::null()
}

fn CGDataProviderGetSize(env: &mut Environment, provider: CGDataProviderRef) -> u64 {
    match *env.objc.borrow(provider) {
        CGDataProviderHostObject::DataWithSize { size, .. } => size as u64,
        CGDataProviderHostObject::Direct { size, .. } => size as u64,
        CGDataProviderHostObject::CGImage(cg_image) => {
            cg_image::borrow_image(&env.objc, cg_image).pixels().len() as u64
        }
        CGDataProviderHostObject::CFData(cf_data) => CFDataGetLength(env, cf_data) as u64,
    }
}

fn CGDataProviderCreateSequential(
    env: &mut Environment,
    info: MutVoidPtr,
    callbacks: ConstVoidPtr,
) -> CGDataProviderRef {
    // CGDataProviderSequentialCallbacks struct layout (32-bit ARM):
    //   offset 0: version (u32)
    //   offset 4: getBytes  — size_t (*)(void *info, void *buffer, size_t count)
    //   offset 8: skipForward — off_t (*)(void *info, off_t count)
    //   offset 12: rewind   — void (*)(void *info)
    //   offset 16: releaseInfo — void (*)(void *info)
    //
    // Strategy: call getBytes in a loop to read the full data into a
    // host-side Vec, then wrap it in a DataWithSize provider.  This
    // works for the common case where the data source is finite (e.g.
    // an image file being decoded by Core Graphics).

    if callbacks.is_null() {
        log!("Warning: CGDataProviderCreateSequential: null callbacks, returning null");
        return nil;
    }

    let cb_base = callbacks.to_bits();

    let get_bytes_addr: u32 = env.mem.read(ConstPtr::<u32>::from_bits(cb_base + 4));
    let release_info_addr: u32 = env.mem.read(ConstPtr::<u32>::from_bits(cb_base + 16));

    if get_bytes_addr == 0 {
        log!("Warning: CGDataProviderCreateSequential: no getBytes callback, returning null");
        return nil;
    }

    let get_bytes = GuestFunction::from_addr_with_thumb_bit(get_bytes_addr);

    // Allocate a temporary guest buffer for reading chunks.
    const CHUNK_SIZE: GuestUSize = 16384;
    let tmp_buf: MutVoidPtr = env.mem.alloc(CHUNK_SIZE).cast();

    let mut all_data: Vec<u8> = Vec::new();

    loop {
        let bytes_read: GuestUSize = get_bytes.call_from_host(env, (info, tmp_buf, CHUNK_SIZE));
        if bytes_read == 0 {
            break;
        }
        let slice = env.mem.bytes_at(tmp_buf.cast::<u8>().cast_const(), bytes_read);
        all_data.extend_from_slice(slice);
        if bytes_read < CHUNK_SIZE {
            break;
        }
    }

    // Free the temporary buffer.
    env.mem.free(tmp_buf.cast());

    // Call releaseInfo if provided.
    if release_info_addr != 0 {
        let release_info = GuestFunction::from_addr_with_thumb_bit(release_info_addr);
        () = release_info.call_from_host(env, (info,));
    }

    if all_data.is_empty() {
        log_dbg!("CGDataProviderCreateSequential: read 0 bytes from callbacks");
    } else {
        log_dbg!(
            "CGDataProviderCreateSequential: read {} bytes from callbacks",
            all_data.len()
        );
    }

    // Copy the data into guest memory and wrap as a provider.
    let len: GuestUSize = all_data.len().try_into().unwrap();
    let guest_buf: MutVoidPtr = env.mem.alloc(len.max(1)).cast();
    if len > 0 {
        env.mem
            .bytes_at_mut(guest_buf.cast::<u8>(), len)
            .copy_from_slice(&all_data);
    }

    let class = env
        .objc
        .get_known_class("_touchHLE_CGDataProvider", &mut env.mem);
    env.objc.alloc_object(
        class,
        Box::new(CGDataProviderHostObject::DataWithSize {
            info: MutVoidPtr::null(),
            data: guest_buf.cast_const(),
            size: len,
            release_callback: GuestFunction::null_ptr(),
        }),
        &mut env.mem,
    )
}

fn CGDataProviderCreateDirect(
    env: &mut Environment,
    info: MutVoidPtr,
    size: i64,
    callbacks: ConstVoidPtr,
) -> CGDataProviderRef {
    // CGDataProviderDirectCallbacks struct layout (32-bit ARM):
    //   offset 0: version (u32)
    //   offset 4: getBytePointer (function pointer)
    //   offset 8: releaseBytePointer (function pointer)
    //   offset 12: getBytesAtPosition (function pointer)
    //   offset 16: releaseInfo (function pointer)
    //
    // Strategy: if getBytePointer is non-NULL, call it to get a direct
    // pointer to the data, then create a provider wrapping that pointer.
    // If only getBytesAtPosition is available, allocate a buffer and
    // read the full data into it.

    if callbacks.is_null() || size <= 0 {
        log!("Warning: CGDataProviderCreateDirect: null callbacks or invalid size ({}), returning null", size);
        return nil;
    }

    let size_u: GuestUSize = size as GuestUSize;
    let cb_base = callbacks.to_bits();

    let get_byte_pointer_addr: u32 = env.mem.read(ConstPtr::<u32>::from_bits(cb_base + 4));
    let get_bytes_at_position_addr: u32 = env.mem.read(ConstPtr::<u32>::from_bits(cb_base + 12));
    let release_info_addr: u32 = env.mem.read(ConstPtr::<u32>::from_bits(cb_base + 16));

    let data_ptr: ConstVoidPtr = if get_byte_pointer_addr != 0 {
        // Call getBytePointer(info) to get direct data pointer
        let get_byte_pointer = GuestFunction::from_addr_with_thumb_bit(get_byte_pointer_addr);
        let ptr: MutVoidPtr = get_byte_pointer.call_from_host(env, (info,));
        ptr.cast_const()
    } else if get_bytes_at_position_addr != 0 {
        // Allocate buffer and read data via getBytesAtPosition(info, buffer, position, count)
        // Note: position is off_t (i64 on Darwin ARM32), passed in r2:r3 register pair
        let buf: MutVoidPtr = env.mem.alloc(size_u).cast();
        let get_bytes = GuestFunction::from_addr_with_thumb_bit(get_bytes_at_position_addr);
        let _bytes_read: GuestUSize = get_bytes.call_from_host(env, (info, buf, 0i64, size_u));
        buf.cast_const()
    } else {
        log!("Warning: CGDataProviderCreateDirect: no data access callback available, returning null");
        return nil;
    };

    if data_ptr.is_null() {
        log!("Warning: CGDataProviderCreateDirect: data callback returned NULL, returning null");
        return nil;
    }

    // Build a release callback wrapper: we call releaseInfo(info) on dealloc.
    let release_callback = if release_info_addr != 0 {
        GuestFunction::from_addr_with_thumb_bit(release_info_addr)
    } else {
        GuestFunction::null_ptr()
    };

    log_dbg!(
        "CGDataProviderCreateDirect: info={:?}, size={}, data={:?}",
        info, size, data_ptr
    );

    let class = env
        .objc
        .get_known_class("_touchHLE_CGDataProvider", &mut env.mem);
    env.objc.alloc_object(
        class,
        Box::new(CGDataProviderHostObject::Direct {
            info,
            data: data_ptr,
            size: size_u,
            release_info_callback: release_callback,
        }),
        &mut env.mem,
    )
}

pub const FUNCTIONS: FunctionExports = &[
    export_c_func!(CGDataProviderRetain(_)),
    export_c_func!(CGDataProviderRelease(_)),
    export_c_func!(CGDataProviderCreateWithData(_, _, _, _)),
    export_c_func!(CGDataProviderCopyData(_)),
    export_c_func!(CGDataProviderCreateWithURL(_)),
    export_c_func!(CGDataProviderCreateWithCFData(_)),
    export_c_func!(CGDataProviderCreateWithFilename(_)),
    export_c_func!(CGDataProviderGetInfo(_)),
    export_c_func!(CGDataProviderGetSize(_)),
    export_c_func!(CGDataProviderCreateSequential(_, _)),
    export_c_func!(CGDataProviderCreateDirect(_, _, _)),
];
