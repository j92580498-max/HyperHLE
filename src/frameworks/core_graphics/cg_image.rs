/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `CGImage.h`

use super::cg_color_space::{
    kCGColorSpaceGenericRGB, CGColorSpaceCreateWithName, CGColorSpaceGetModel, CGColorSpaceRef,
};
use super::cg_data_provider::{self, CGDataProviderRef};
use super::CGFloat;
use crate::dyld::{export_c_func, ConstantExports, FunctionExports, HostConstant};
use crate::frameworks::core_foundation::{CFRelease, CFRetain, CFTypeRef};
use crate::frameworks::foundation::ns_string;
use crate::image::Image;
use crate::mem::{ConstPtr, GuestUSize};
use crate::objc::{autorelease, id, nil, objc_classes, ClassExports, HostObject, ObjC};
use crate::Environment;

pub type CGImageAlphaInfo = u32;
pub const kCGImageAlphaNone: CGImageAlphaInfo = 0;
pub const kCGImageAlphaPremultipliedLast: CGImageAlphaInfo = 1;
pub const kCGImageAlphaPremultipliedFirst: CGImageAlphaInfo = 2;
pub const kCGImageAlphaLast: CGImageAlphaInfo = 3;
pub const kCGImageAlphaFirst: CGImageAlphaInfo = 4;
pub const kCGImageAlphaNoneSkipLast: CGImageAlphaInfo = 5;
pub const kCGImageAlphaNoneSkipFirst: CGImageAlphaInfo = 6;
pub const kCGImageAlphaOnly: CGImageAlphaInfo = 7;

pub type CGImageSourceRef = CFTypeRef;
pub type CGImageSourceStatus = i32;
pub const kCGImageStatusUnexpectedEOF:   CGImageSourceStatus = -5;
pub const kCGImageStatusInvalidData:     CGImageSourceStatus = -4;
pub const kCGImageStatusUnknownType:     CGImageSourceStatus = -3;
pub const kCGImageStatusReadingHeader:   CGImageSourceStatus = -2;
pub const kCGImageStatusIncomplete:      CGImageSourceStatus = -1;
pub const kCGImageStatusComplete:        CGImageSourceStatus =  0;

pub type CGImageByteOrderInfo = u32;
pub const kCGImageByteOrderMask: CGImageByteOrderInfo = 0x7000;
pub const kCGImageByteOrderDefault: CGImageByteOrderInfo = 0 << 12;
#[allow(dead_code)]
pub const kCGImageByteOrder16Little: CGImageByteOrderInfo = 1 << 12;
#[allow(dead_code)]
pub const kCGImageByteOrder32Little: CGImageByteOrderInfo = 2 << 12;
#[allow(dead_code)]
pub const kCGImageByteOrder16Big: CGImageByteOrderInfo = 3 << 12;
pub const kCGImageByteOrder32Big: CGImageByteOrderInfo = 4 << 12;

pub type CGBitmapInfo = u32;
pub const kCGBitmapAlphaInfoMask: CGBitmapInfo = 0x1F;
pub const kCGBitmapByteOrderMask: CGBitmapInfo = kCGImageByteOrderMask;

pub const CLASSES: ClassExports = objc_classes! {
(env, this, _cmd);

@implementation _touchHLE_CGImage: NSObject

- (id)systemUptime {
    nil
}

- (id)tick_audio {
    nil
}

@end
};

#[derive(Default)]
struct CGImageHostObject {
    image: Image,
}
impl HostObject for CGImageHostObject {}

pub type CGImageRef = CFTypeRef;

pub fn CGImageRelease(env: &mut Environment, c: CGImageRef) {
    if !c.is_null() {
        CFRelease(env, c);
    }
}

pub fn CGImageRetain(env: &mut Environment, c: CGImageRef) -> CGImageRef {
    if !c.is_null() {
        CFRetain(env, c)
    } else {
        c
    }
}

pub fn from_image(env: &mut Environment, image: Image) -> CGImageRef {
    let host_obj = Box::new(CGImageHostObject { image });
    let class = env.objc.get_known_class("_touchHLE_CGImage", &mut env.mem);
    env.objc.alloc_object(class, host_obj, &mut env.mem)
}

pub fn borrow_image(objc: &ObjC, image: CGImageRef) -> &Image {
    // ВНИМАНИЕ: Если здесь передан null, эмулятор упадет.
    // Но CoreGraphics функции ниже теперь защищены.
    &objc.borrow::<CGImageHostObject>(image).image
}

pub fn borrow_image_mut(objc: &mut ObjC, image: CGImageRef) -> &mut Image {
    &mut objc.borrow_mut::<CGImageHostObject>(image).image
}

fn CGImageCreateCopyWithColorSpace(
    env: &mut Environment,
    image: CGImageRef,
    color_space: CGColorSpaceRef,
) -> CGImageRef {
    if image.is_null() {
        return nil;
    }

    let image_color_space = CGImageGetColorSpace(env, image);
    if image_color_space.is_null() {
        return nil;
    }

    assert_eq!(
        CGColorSpaceGetModel(env, image_color_space),
        CGColorSpaceGetModel(env, color_space)
    );

    let new_image = env.objc.borrow::<CGImageHostObject>(image).image.clone();
    from_image(env, new_image)
}

fn CGImageCreateWithPNGDataProvider(
    env: &mut Environment,
    source: CGDataProviderRef,
    decode: ConstPtr<CGFloat>,
    _should_interpolate: bool,
    _intent: i32,
) -> CGImageRef {
    if source.is_null() {
        return nil;
    }
    assert!(decode.is_null());

    let bytes = cg_data_provider::borrow_bytes(env, source);
    let Ok(image) = Image::from_bytes(bytes) else {
        return nil;
    };

    from_image(env, image)
}

fn CGImageCreateWithJPEGDataProvider(
    env: &mut Environment,
    source: CGDataProviderRef,
    decode: ConstPtr<CGFloat>,
    _should_interpolate: bool,
    _intent: i32,
) -> CGImageRef {
    if source.is_null() {
        return nil;
    }
    assert!(decode.is_null());

    let bytes = cg_data_provider::borrow_bytes(env, source);
    let Ok(image) = Image::from_bytes(bytes) else {
        return nil;
    };

    from_image(env, image)
}

fn CGImageGetAlphaInfo(_env: &mut Environment, image: CGImageRef) -> CGImageAlphaInfo {
    if image.is_null() {
        return kCGImageAlphaNone;
    }
    kCGImageAlphaPremultipliedLast
}

fn CGImageGetColorSpace(env: &mut Environment, image: CGImageRef) -> CGColorSpaceRef {
    if image.is_null() {
        return nil;
    }
    let srgb_name = ns_string::get_static_str(env, kCGColorSpaceGenericRGB);
    CGColorSpaceCreateWithName(env, srgb_name)
}

pub fn CGImageGetWidth(env: &mut Environment, image: CGImageRef) -> GuestUSize {
    if image.is_null() {
        return 0;
    }
    let (width, _height) = env
        .objc
        .borrow::<CGImageHostObject>(image)
        .image
        .dimensions();
    width
}

pub fn CGImageGetHeight(env: &mut Environment, image: CGImageRef) -> GuestUSize {
    if image.is_null() {
        return 0;
    }
    let (_width, height) = env
        .objc
        .borrow::<CGImageHostObject>(image)
        .image
        .dimensions();
    height
}

fn CGImageGetBitsPerPixel(_env: &mut Environment, image: CGImageRef) -> GuestUSize {
    if image.is_null() {
        return 0;
    }
    32
}

fn CGImageGetBytesPerRow(env: &mut Environment, image: CGImageRef) -> GuestUSize {
    if image.is_null() {
        return 0;
    }
    let (width, _height) = env
        .objc
        .borrow::<CGImageHostObject>(image)
        .image
        .dimensions();
    width * 4
}

fn CGImageGetDataProvider(env: &mut Environment, image: CGImageRef) -> CGDataProviderRef {
    if image.is_null() {
        return nil;
    }
    let cg_data_provider = cg_data_provider::from_cg_image(env, image);
    autorelease(env, cg_data_provider)
}

fn CGImageGetBitsPerComponent(_: &mut Environment, image: CGImageRef) -> GuestUSize {
    if image.is_null() {
        return 0;
    }
    8
}

/// Copy of an existing CGImage — just clone the underlying Image.
fn CGImageCreateCopy(env: &mut Environment, image: CGImageRef) -> CGImageRef {
    if image.is_null() {
        return nil;
    }
    let new_image = env.objc.borrow::<CGImageHostObject>(image).image.clone();
    from_image(env, new_image)
}

/// Crop to a sub-rectangle. CGRect is in pixels (no coordinate transform here).
fn CGImageCreateWithImageInRect(
    env: &mut Environment,
    image: CGImageRef,
    rect: super::CGRect,
) -> CGImageRef {
    if image.is_null() {
        return nil;
    }

    let (img_w, img_h) = env
        .objc
        .borrow::<CGImageHostObject>(image)
        .image
        .dimensions();

    // Clamp rect to image bounds.
    let x = (rect.origin.x as u32).min(img_w);
    let y = (rect.origin.y as u32).min(img_h);
    let width = (rect.size.width as u32).min(img_w.saturating_sub(x));
    let height = (rect.size.height as u32).min(img_h.saturating_sub(y));

    if width == 0 || height == 0 {
        return nil;
    }

    let src_pixels = env.objc.borrow::<CGImageHostObject>(image).image.pixels();

    // Copy the sub-region row by row (RGBA — 4 bytes per pixel).
    let mut dst = vec![0u8; (width * height * 4) as usize];
    for row in 0..height {
        let src_start = ((y + row) * img_w + x) as usize * 4;
        let dst_start = (row * width) as usize * 4;
        dst[dst_start..dst_start + width as usize * 4]
            .copy_from_slice(&src_pixels[src_start..src_start + width as usize * 4]);
    }

    let new_image = Image::from_pixels(width, height, dst);
    from_image(env, new_image)
}

/// Create an image masked by another image. We don't apply the mask —
/// return a copy of the source so apps that read back pixels get something.
fn CGImageCreateWithMask(
    env: &mut Environment,
    image: CGImageRef,
    _mask: CGImageRef,
) -> CGImageRef {
    log!("CGImageCreateWithMask: mask not applied (stubbed)");
    CGImageCreateCopy(env, image)
}

/// Invert-mask stub — just copy.
fn CGImageCreateMaskWithImageMask(env: &mut Environment, mask_image: CGImageRef) -> CGImageRef {
    log!("CGImageCreateMaskWithImageMask: stubbed");
    CGImageCreateCopy(env, mask_image)
}

// MARK: - Additional accessors

fn CGImageGetBitmapInfo(_env: &mut Environment, image: CGImageRef) -> CGBitmapInfo {
    if image.is_null() {
        return 0;
    }
    // Report premultiplied-last RGBA, big-endian 32-bit — matches our Image
    // format.
    kCGImageAlphaPremultipliedLast | kCGImageByteOrder32Big
}

/// Decode array — we always use the default (nil / identity mapping).
fn CGImageGetDecode(_env: &mut Environment, _image: CGImageRef) -> ConstPtr<CGFloat> {
    ConstPtr::null()
}

fn CGImageGetShouldInterpolate(_env: &mut Environment, image: CGImageRef) -> bool {
    if image.is_null() {
        return false;
    }
    true
}

/// CGColorRenderingIntent — 0 = kCGRenderingIntentDefault.
fn CGImageGetRenderingIntent(_env: &mut Environment, _image: CGImageRef) -> i32 {
    0
}

fn CGImageIsMask(_env: &mut Environment, _image: CGImageRef) -> bool {
    false
}

/// `CGImageCreate` — create a bitmap image from raw pixel data provided by a
/// `CGDataProvider`.  Only the RGBA/RGBX 8-bit-per-component layout that our
/// `Image` type uses internally is fully supported; other configurations are
/// converted to that format on a best-effort basis so that apps which call this
/// function to compose textures at runtime can proceed.
fn CGImageCreate(
    env: &mut Environment,
    width: GuestUSize,
    height: GuestUSize,
    bits_per_component: GuestUSize,
    bits_per_pixel: GuestUSize,
    bytes_per_row: GuestUSize,
    _space: CGColorSpaceRef,
    bitmap_info: CGBitmapInfo,
    provider: CGDataProviderRef,
    _decode: ConstPtr<CGFloat>,
    _should_interpolate: bool,
    _intent: i32,
) -> CGImageRef {
    if provider.is_null() || width == 0 || height == 0 {
        return nil;
    }

    let src = cg_data_provider::borrow_bytes(env, provider);

    let alpha_info = bitmap_info & kCGBitmapAlphaInfoMask;
    let byte_order = bitmap_info & kCGBitmapByteOrderMask;

    let expected_len = bytes_per_row as usize * height as usize;
    if src.len() < expected_len {
        log!(
            "CGImageCreate: provider has {} bytes but {}×{} at {} bpr needs {}",
            src.len(), width, height, bytes_per_row, expected_len,
        );
        return nil;
    }

    let mut dst = vec![0u8; (width * height * 4) as usize];

    let src_components = (bits_per_pixel / bits_per_component) as usize;
    let src_bytes_per_pixel = (bits_per_pixel / 8) as usize;

    for row in 0..height as usize {
        for col in 0..width as usize {
            let si = row * bytes_per_row as usize + col * src_bytes_per_pixel;
            let di = (row * width as usize + col) * 4;

            if bits_per_component == 8 && src_components >= 3 {
                let (r, g, b, a) = match (alpha_info, byte_order) {
                    (kCGImageAlphaPremultipliedFirst, _)
                    | (kCGImageAlphaFirst, _)
                    | (kCGImageAlphaNoneSkipFirst, _) => {
                        let a = if alpha_info == kCGImageAlphaNoneSkipFirst { 255 } else { src[si] };
                        (src[si + 1], src[si + 2], src[si + 3], a)
                    }
                    (kCGImageAlphaPremultipliedLast, _)
                    | (kCGImageAlphaLast, _) => {
                        (src[si], src[si + 1], src[si + 2], src[si + 3])
                    }
                    (kCGImageAlphaNoneSkipLast, _) => {
                        (src[si], src[si + 1], src[si + 2], 255)
                    }
                    _ => {
                        if src_components >= 4 {
                            (src[si], src[si + 1], src[si + 2], src[si + 3])
                        } else {
                            (src[si], src[si + 1], src[si + 2], 255)
                        }
                    }
                };
                dst[di] = r;
                dst[di + 1] = g;
                dst[di + 2] = b;
                dst[di + 3] = a;
            } else if bits_per_component == 8 && src_components == 1 {
                let v = src[si];
                dst[di] = v;
                dst[di + 1] = v;
                dst[di + 2] = v;
                dst[di + 3] = 255;
            } else {
                dst[di] = 0;
                dst[di + 1] = 0;
                dst[di + 2] = 0;
                dst[di + 3] = 255;
            }
        }
    }

    let image = Image::from_pixels(width, height, dst);
    from_image(env, image)
}

// =========================================================================
// MARK: - New creation functions
// =========================================================================

/// `CGImageCreateWithBitmapContext` — not a real public API but some apps
/// call an internal variant. We treat it the same as CGImageCreate with
/// premultiplied-last RGBA.
fn CGImageCreateWithBitmapData(
   env: &mut Environment,
   width: GuestUSize,
   height: GuestUSize,
   data: crate::mem::ConstPtr<u8>,
   bytes_per_row: GuestUSize,
   bitmap_info: CGBitmapInfo,
) -> CGImageRef {
   if data.is_null() || width == 0 || height == 0 { return nil; }
   let len = bytes_per_row as usize * height as usize;
   let src = env.mem.bytes_at(data, len as u32);
   let mut dst = vec![0u8; (width * height * 4) as usize];
   let alpha_info = bitmap_info & kCGBitmapAlphaInfoMask;
   let bpp = if alpha_info == kCGImageAlphaNone { 3usize } else { 4 };
   for row in 0..height as usize {
       for col in 0..width as usize {
           let si = row * bytes_per_row as usize + col * bpp;
           let di = (row * width as usize + col) * 4;
           if bpp == 4 {
               let (r, g, b, a) = match alpha_info {
                   kCGImageAlphaPremultipliedFirst | kCGImageAlphaFirst
                   | kCGImageAlphaNoneSkipFirst =>
                       (src[si+1], src[si+2], src[si+3], if alpha_info == kCGImageAlphaNoneSkipFirst { 255 } else { src[si] }),
                   _ => (src[si], src[si+1], src[si+2], if alpha_info == kCGImageAlphaNoneSkipLast { 255 } else { src[si+3] }),
               };
               dst[di] = r; dst[di+1] = g; dst[di+2] = b; dst[di+3] = a;
           } else {
               dst[di] = src[si]; dst[di+1] = src[si+1]; dst[di+2] = src[si+2]; dst[di+3] = 255;
           }
       }
   }
   from_image(env, Image::from_pixels(width, height, dst))
}

/// Flip an image vertically (used by some bitmap context reads).
fn CGImageCreateFlippedVertically(env: &mut Environment, image: CGImageRef) -> CGImageRef {
   if image.is_null() { return nil; }
   let (w, h) = env.objc.borrow::<CGImageHostObject>(image).image.dimensions();
   let src = env.objc.borrow::<CGImageHostObject>(image).image.pixels().to_vec();
   let mut dst = vec![0u8; src.len()];
   for row in 0..h as usize {
       let src_row = (h as usize - 1 - row) * w as usize * 4;
       let dst_row = row * w as usize * 4;
       dst[dst_row..dst_row + w as usize * 4]
           .copy_from_slice(&src[src_row..src_row + w as usize * 4]);
   }
   from_image(env, Image::from_pixels(w, h, dst))
}

/// Scale an image to a new size using nearest-neighbour sampling.
fn CGImageCreateScaled(
   env: &mut Environment,
   image: CGImageRef,
   new_width: GuestUSize,
   new_height: GuestUSize,
) -> CGImageRef {
   if image.is_null() || new_width == 0 || new_height == 0 { return nil; }
   let (src_w, src_h) = env.objc.borrow::<CGImageHostObject>(image).image.dimensions();
   let src = env.objc.borrow::<CGImageHostObject>(image).image.pixels().to_vec();
   let mut dst = vec![0u8; (new_width * new_height * 4) as usize];
   for dy in 0..new_height as usize {
       let sy = (dy * src_h as usize) / new_height as usize;
       for dx in 0..new_width as usize {
           let sx = (dx * src_w as usize) / new_width as usize;
           let si = (sy * src_w as usize + sx) * 4;
           let di = (dy * new_width as usize + dx) * 4;
           dst[di..di+4].copy_from_slice(&src[si..si+4]);
       }
   }
   from_image(env, Image::from_pixels(new_width, new_height, dst))
}

// =========================================================================
// MARK: - New accessor functions
// =========================================================================

/// `CFTypeID CGImageGetTypeID()` — returns a stable non-zero type ID.
fn CGImageGetTypeID(_env: &mut Environment) -> u32 {
   // Any stable non-zero value is fine — apps use this for type checking.
   0x0000_BABE
}

/// `CGImageRef CGImageCreateWithImageInRect` — already exists, this is the
/// `CGImageGetUTType` variant that returns the UTI string.
fn CGImageGetUTType(_env: &mut Environment, _image: CGImageRef) -> crate::objc::id {
   // Return nil — we don't track the original file type.
   nil
}

fn CGImageGetPixelFormatInfo(_env: &mut Environment, image: CGImageRef) -> u32 {
   if image.is_null() { return 0; }
   0 // kCGImagePixelFormatPacked
}

/// Read raw RGBA pixels from a CGImage into a guest buffer.
/// `out_data` must be at least `width * height * 4` bytes.
fn CGImageReadPixels(
   env: &mut Environment,
   image: CGImageRef,
   out_data: crate::mem::MutPtr<u8>,
) {
   if image.is_null() || out_data.is_null() { return; }
   let pixels = env.objc.borrow::<CGImageHostObject>(image).image.pixels().to_vec();
   let dst = env.mem.bytes_at_mut(out_data, pixels.len() as u32);
   dst.copy_from_slice(&pixels);
}

/// Premultiply alpha on a copy of the image.
fn CGImageCreatePremultiplied(env: &mut Environment, image: CGImageRef) -> CGImageRef {
   if image.is_null() { return nil; }
   let (w, h) = env.objc.borrow::<CGImageHostObject>(image).image.dimensions();
   let mut pixels = env.objc.borrow::<CGImageHostObject>(image).image.pixels().to_vec();
   for px in pixels.chunks_exact_mut(4) {
       let a = px[3] as u32;
       px[0] = ((px[0] as u32 * a + 127) / 255) as u8;
       px[1] = ((px[1] as u32 * a + 127) / 255) as u8;
       px[2] = ((px[2] as u32 * a + 127) / 255) as u8;
   }
   from_image(env, Image::from_pixels(w, h, pixels))
}

/// Un-premultiply alpha on a copy of the image.
fn CGImageCreateUnpremultiplied(env: &mut Environment, image: CGImageRef) -> CGImageRef {
   if image.is_null() { return nil; }
   let (w, h) = env.objc.borrow::<CGImageHostObject>(image).image.dimensions();
   let mut pixels = env.objc.borrow::<CGImageHostObject>(image).image.pixels().to_vec();
   for px in pixels.chunks_exact_mut(4) {
       let a = px[3] as u32;
       if a > 0 {
           px[0] = ((px[0] as u32 * 255 + a / 2) / a).min(255) as u8;
           px[1] = ((px[1] as u32 * 255 + a / 2) / a).min(255) as u8;
           px[2] = ((px[2] as u32 * 255 + a / 2) / a).min(255) as u8;
       }
   }
   from_image(env, Image::from_pixels(w, h, pixels))
}

fn CGImageSourceCreateWithData(
   env: &mut Environment,
   data: CFTypeRef,
   _options: CFTypeRef,
) -> CGImageSourceRef {
   // Try to decode the data as an image and wrap it in a CGImageSource-like
   // object. We reuse CGImageRef as the source ref since we only support
   // single-frame sources.
   if data.is_null() { return nil; }
   // data is an NSData* / CFDataRef — get its bytes.
   let bytes_ptr: crate::mem::ConstPtr<u8> = crate::objc::msg![env; data bytes];
   let length: crate::frameworks::foundation::NSUInteger = crate::objc::msg![env; data length];
   if bytes_ptr.is_null() || length == 0 { return nil; }
   let bytes = env.mem.bytes_at(bytes_ptr, length);
   match crate::image::Image::from_bytes(bytes) {
       Ok(image) => {
           let img_ref = from_image(env, image);
           // Retain to match "Create" rule.
           crate::frameworks::core_foundation::CFRetain(env, img_ref)
       }
       Err(_) => {
           log_dbg!("CGImageSourceCreateWithData: failed to decode image");
           nil
       }
   }
}

fn CGImageSourceCreateWithURL(
   env: &mut Environment,
   url: CFTypeRef,
   _options: CFTypeRef,
) -> CGImageSourceRef {
   // Delegate to NSData loading then create from data.
   if url.is_null() { return nil; }
   let data: crate::objc::id = crate::objc::msg_class![env; NSData dataWithContentsOfURL:url];
   if data == nil { return nil; }
   CGImageSourceCreateWithData(env, data, nil)
}

fn CGImageSourceGetCount(_env: &mut Environment, source: CGImageSourceRef) -> u32 {
   if source.is_null() { 0 } else { 1 }
}

fn CGImageSourceGetStatus(_env: &mut Environment, source: CGImageSourceRef) -> CGImageSourceStatus {
   if source.is_null() { kCGImageStatusInvalidData } else { kCGImageStatusComplete }
}

fn CGImageSourceGetStatusAtIndex(
   _env: &mut Environment,
   source: CGImageSourceRef,
   _index: u32,
) -> CGImageSourceStatus {
   if source.is_null() { kCGImageStatusInvalidData } else { kCGImageStatusComplete }
}

fn CGImageSourceCreateImageAtIndex(
   env: &mut Environment,
   source: CGImageSourceRef,
   index: u32,
   _options: CFTypeRef,
) -> CGImageRef {
   if source.is_null() || index != 0 { return nil; }
   // source IS the CGImageRef (see CGImageSourceCreateWithData).
   crate::frameworks::core_foundation::CFRetain(env, source)
}

fn CGImageSourceCopyPropertiesAtIndex(
   _env: &mut Environment,
   _source: CGImageSourceRef,
   _index: u32,
   _options: CFTypeRef,
) -> CFTypeRef {
   // Return nil — we don't track image metadata.
   nil
}

fn CGImageSourceCopyProperties(
   _env: &mut Environment,
   _source: CGImageSourceRef,
   _options: CFTypeRef,
) -> CFTypeRef {
   nil
}

fn CGImageSourceGetTypeID(_env: &mut Environment) -> u32 {
   0x0000_CAFE
}

fn CGImageSourceCreateThumbnailAtIndex(
   env: &mut Environment,
   source: CGImageSourceRef,
   index: u32,
   _options: CFTypeRef,
) -> CGImageRef {
   // Return the full image as the thumbnail — no downscaling.
   CGImageSourceCreateImageAtIndex(env, source, index, nil)
}

pub const FUNCTIONS: FunctionExports = &[
    // Existing entries ...
    export_c_func!(CGImageCreate(_, _, _, _, _, _, _, _, _, _, _)),
    export_c_func!(CGImageRelease(_)),
    export_c_func!(CGImageRetain(_)),
    export_c_func!(CGImageCreateCopyWithColorSpace(_, _)),
    export_c_func!(CGImageCreateWithPNGDataProvider(_, _, _, _)),
    export_c_func!(CGImageCreateWithJPEGDataProvider(_, _, _, _)),
    export_c_func!(CGImageCreateWithImageInRect(_, _)),
    export_c_func!(CGImageCreateWithMask(_, _)),
    export_c_func!(CGImageCreateMaskWithImageMask(_)),
    export_c_func!(CGImageCreateCopy(_)),
    export_c_func!(CGImageGetAlphaInfo(_)),
    export_c_func!(CGImageGetBitmapInfo(_)),
    export_c_func!(CGImageGetColorSpace(_)),
    export_c_func!(CGImageGetWidth(_)),
    export_c_func!(CGImageGetHeight(_)),
    export_c_func!(CGImageGetBitsPerPixel(_)),
    export_c_func!(CGImageGetBitsPerComponent(_)),
    export_c_func!(CGImageGetBytesPerRow(_)),
    export_c_func!(CGImageGetDataProvider(_)),
    export_c_func!(CGImageGetDecode(_)),
    export_c_func!(CGImageGetShouldInterpolate(_)),
    export_c_func!(CGImageGetRenderingIntent(_)),
    export_c_func!(CGImageIsMask(_)),
    // New entries
    export_c_func!(CGImageGetTypeID()),
    export_c_func!(CGImageGetUTType(_)),
    export_c_func!(CGImageGetPixelFormatInfo(_)),
    export_c_func!(CGImageCreateWithBitmapData(_, _, _, _, _)),
    export_c_func!(CGImageCreateFlippedVertically(_)),
    export_c_func!(CGImageCreateScaled(_, _, _)),
    export_c_func!(CGImageReadPixels(_, _)),
    export_c_func!(CGImageCreatePremultiplied(_)),
    export_c_func!(CGImageCreateUnpremultiplied(_)),
    // CGImageSource (ImageIO.framework)
    export_c_func!(CGImageSourceGetTypeID()),
    export_c_func!(CGImageSourceCreateWithData(_, _)),
    export_c_func!(CGImageSourceCreateWithURL(_, _)),
    export_c_func!(CGImageSourceGetCount(_)),
    export_c_func!(CGImageSourceGetStatus(_)),
    export_c_func!(CGImageSourceGetStatusAtIndex(_, _)),
    export_c_func!(CGImageSourceCreateImageAtIndex(_, _, _)),
    export_c_func!(CGImageSourceCopyPropertiesAtIndex(_, _, _)),
    export_c_func!(CGImageSourceCopyProperties(_, _)),
    export_c_func!(CGImageSourceCreateThumbnailAtIndex(_, _, _)),
];

/// `CGImageProperty*` / `kCGImageDestination*` / `kCGImageSource*` are
/// `extern const CFStringRef` keys published by `ImageIO.framework`.
/// touchHLE has no live ImageIO pipeline, but apps embed references to
/// these symbols in their non-lazy symbol pointer table — usually to
/// read EXIF / GIF / PNG / JPEG metadata back from images they decoded
/// elsewhere — so we publish them as CFString placeholders matching
/// Apple's literal keys. With these in place dictionary-key identity
/// comparisons (e.g. `dict[(__bridge id)kCGImagePropertyOrientation]`)
/// continue to work, even if the dictionary itself is empty.
/// <https://developer.apple.com/documentation/imageio/image_properties>
pub const CONSTANTS: ConstantExports = &[
    (
        "_kCGImagePropertyPixelWidth",
        HostConstant::NSString("PixelWidth"),
    ),
    (
        "_kCGImagePropertyPixelHeight",
        HostConstant::NSString("PixelHeight"),
    ),
    (
        "_kCGImagePropertyOrientation",
        HostConstant::NSString("Orientation"),
    ),
    (
        "_kCGImagePropertyDPIWidth",
        HostConstant::NSString("DPIWidth"),
    ),
    (
        "_kCGImagePropertyDPIHeight",
        HostConstant::NSString("DPIHeight"),
    ),
    (
        "_kCGImagePropertyDepth",
        HostConstant::NSString("Depth"),
    ),
    (
        "_kCGImagePropertyColorModel",
        HostConstant::NSString("ColorModel"),
    ),
    (
        "_kCGImagePropertyHasAlpha",
        HostConstant::NSString("HasAlpha"),
    ),
    (
        "_kCGImagePropertyIsFloat",
        HostConstant::NSString("IsFloat"),
    ),
    (
        "_kCGImagePropertyIsIndexed",
        HostConstant::NSString("IsIndexed"),
    ),
    (
        "_kCGImagePropertyProfileName",
        HostConstant::NSString("ProfileName"),
    ),
    (
        "_kCGImagePropertyFileSize",
        HostConstant::NSString("FileSize"),
    ),
    // Format-specific subdictionary keys.
    (
        "_kCGImagePropertyJFIFDictionary",
        HostConstant::NSString("{JFIF}"),
    ),
    (
        "_kCGImagePropertyJFIFIsProgressive",
        HostConstant::NSString("IsProgressive"),
    ),
    (
        "_kCGImagePropertyJFIFVersion",
        HostConstant::NSString("JFIFVersion"),
    ),
    (
        "_kCGImagePropertyJFIFXDensity",
        HostConstant::NSString("XDensity"),
    ),
    (
        "_kCGImagePropertyJFIFYDensity",
        HostConstant::NSString("YDensity"),
    ),
    (
        "_kCGImagePropertyJFIFDensityUnit",
        HostConstant::NSString("DensityUnit"),
    ),
    (
        "_kCGImagePropertyGIFDictionary",
        HostConstant::NSString("{GIF}"),
    ),
    (
        "_kCGImagePropertyGIFLoopCount",
        HostConstant::NSString("LoopCount"),
    ),
    (
        "_kCGImagePropertyGIFDelayTime",
        HostConstant::NSString("DelayTime"),
    ),
    (
        "_kCGImagePropertyGIFUnclampedDelayTime",
        HostConstant::NSString("UnclampedDelayTime"),
    ),
    (
        "_kCGImagePropertyGIFHasGlobalColorMap",
        HostConstant::NSString("HasGlobalColorMap"),
    ),
    (
        "_kCGImagePropertyGIFImageColorMap",
        HostConstant::NSString("ImageColorMap"),
    ),
    (
        "_kCGImagePropertyPNGDictionary",
        HostConstant::NSString("{PNG}"),
    ),
    (
        "_kCGImagePropertyPNGInterlaceType",
        HostConstant::NSString("InterlaceType"),
    ),
    (
        "_kCGImagePropertyPNGGamma",
        HostConstant::NSString("Gamma"),
    ),
    (
        "_kCGImagePropertyPNGsRGBIntent",
        HostConstant::NSString("sRGBIntent"),
    ),
    (
        "_kCGImagePropertyPNGChromaticities",
        HostConstant::NSString("Chromaticities"),
    ),
    // Parent metadata dictionaries. Apple documents these as
    // `extern const CFStringRef` keys whose string values match
    // dictionary names embedded in image containers (EXIF, TIFF, etc.).
    // <https://developer.apple.com/documentation/imageio/image_properties>
    (
        "_kCGImagePropertyExifDictionary",
        HostConstant::NSString("{Exif}"),
    ),
    (
        "_kCGImagePropertyTIFFDictionary",
        HostConstant::NSString("{TIFF}"),
    ),
    (
        "_kCGImagePropertyIPTCDictionary",
        HostConstant::NSString("{IPTC}"),
    ),
    (
        "_kCGImagePropertyGPSDictionary",
        HostConstant::NSString("{GPS}"),
    ),
    (
        "_kCGImagePropertyRawDictionary",
        HostConstant::NSString("{RAW}"),
    ),
    (
        "_kCGImagePropertyCIFFDictionary",
        HostConstant::NSString("{CIFF}"),
    ),
    (
        "_kCGImagePropertyExifAuxDictionary",
        HostConstant::NSString("{ExifAux}"),
    ),
    (
        "_kCGImagePropertyMakerCanonDictionary",
        HostConstant::NSString("{MakerCanon}"),
    ),
    (
        "_kCGImagePropertyMakerNikonDictionary",
        HostConstant::NSString("{MakerNikon}"),
    ),
    (
        "_kCGImagePropertyMakerMinoltaDictionary",
        HostConstant::NSString("{MakerMinolta}"),
    ),
    (
        "_kCGImagePropertyMakerFujiDictionary",
        HostConstant::NSString("{MakerFuji}"),
    ),
    (
        "_kCGImagePropertyMakerOlympusDictionary",
        HostConstant::NSString("{MakerOlympus}"),
    ),
    (
        "_kCGImagePropertyMakerPentaxDictionary",
        HostConstant::NSString("{MakerPentax}"),
    ),
    (
        "_kCGImageProperty8BIMDictionary",
        HostConstant::NSString("{8BIM}"),
    ),
    (
        "_kCGImagePropertyDNGDictionary",
        HostConstant::NSString("{DNG}"),
    ),
    // Common EXIF subkeys. These are accessed via the {Exif}
    // sub-dictionary; we expose them as literal CFString constants so
    // guest comparisons work whether or not the dictionary is populated.
    (
        "_kCGImagePropertyExifExposureTime",
        HostConstant::NSString("ExposureTime"),
    ),
    (
        "_kCGImagePropertyExifApertureValue",
        HostConstant::NSString("ApertureValue"),
    ),
    (
        "_kCGImagePropertyExifShutterSpeedValue",
        HostConstant::NSString("ShutterSpeedValue"),
    ),
    (
        "_kCGImagePropertyExifFNumber",
        HostConstant::NSString("FNumber"),
    ),
    (
        "_kCGImagePropertyExifISOSpeedRatings",
        HostConstant::NSString("ISOSpeedRatings"),
    ),
    (
        "_kCGImagePropertyExifDateTimeOriginal",
        HostConstant::NSString("DateTimeOriginal"),
    ),
    (
        "_kCGImagePropertyExifDateTimeDigitized",
        HostConstant::NSString("DateTimeDigitized"),
    ),
    (
        "_kCGImagePropertyExifPixelXDimension",
        HostConstant::NSString("PixelXDimension"),
    ),
    (
        "_kCGImagePropertyExifPixelYDimension",
        HostConstant::NSString("PixelYDimension"),
    ),
    (
        "_kCGImagePropertyExifFocalLength",
        HostConstant::NSString("FocalLength"),
    ),
    (
        "_kCGImagePropertyExifFlash",
        HostConstant::NSString("Flash"),
    ),
    (
        "_kCGImagePropertyExifWhiteBalance",
        HostConstant::NSString("WhiteBalance"),
    ),
    (
        "_kCGImagePropertyExifColorSpace",
        HostConstant::NSString("ColorSpace"),
    ),
    (
        "_kCGImagePropertyExifSubjectArea",
        HostConstant::NSString("SubjectArea"),
    ),
    (
        "_kCGImagePropertyExifSubjectDistance",
        HostConstant::NSString("SubjectDistance"),
    ),
    (
        "_kCGImagePropertyExifFlashEnergy",
        HostConstant::NSString("FlashEnergy"),
    ),
    (
        "_kCGImagePropertyExifUserComment",
        HostConstant::NSString("UserComment"),
    ),
    (
        "_kCGImagePropertyExifBodySerialNumber",
        HostConstant::NSString("BodySerialNumber"),
    ),
    (
        "_kCGImagePropertyExifCameraOwnerName",
        HostConstant::NSString("CameraOwnerName"),
    ),
    (
        "_kCGImagePropertyExifLensModel",
        HostConstant::NSString("LensModel"),
    ),
    (
        "_kCGImagePropertyExifLensMake",
        HostConstant::NSString("LensMake"),
    ),
    (
        "_kCGImagePropertyExifLensSerialNumber",
        HostConstant::NSString("LensSerialNumber"),
    ),
    // ImageDestination options.
    (
        "_kCGImageDestinationLossyCompressionQuality",
        HostConstant::NSString("kCGImageDestinationLossyCompressionQuality"),
    ),
    (
        "_kCGImageDestinationBackgroundColor",
        HostConstant::NSString("kCGImageDestinationBackgroundColor"),
    ),
    // ImageSource options.
    (
        "_kCGImageSourceShouldCache",
        HostConstant::NSString("kCGImageSourceShouldCache"),
    ),
    (
        "_kCGImageSourceShouldAllowFloat",
        HostConstant::NSString("kCGImageSourceShouldAllowFloat"),
    ),
    (
        "_kCGImageSourceCreateThumbnailFromImageIfAbsent",
        HostConstant::NSString("kCGImageSourceCreateThumbnailFromImageIfAbsent"),
    ),
    (
        "_kCGImageSourceCreateThumbnailFromImageAlways",
        HostConstant::NSString("kCGImageSourceCreateThumbnailFromImageAlways"),
    ),
    (
        "_kCGImageSourceThumbnailMaxPixelSize",
        HostConstant::NSString("kCGImageSourceThumbnailMaxPixelSize"),
    ),
    (
        "_kCGImageSourceCreateThumbnailWithTransform",
        HostConstant::NSString("kCGImageSourceCreateThumbnailWithTransform"),
    ),
    (
        "_kCGImageSourceTypeIdentifierHint",
        HostConstant::NSString("kCGImageSourceTypeIdentifierHint"),
    ),
];
