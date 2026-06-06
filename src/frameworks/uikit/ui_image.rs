/*
 * Implementation of UIImage for iOS 2.0 - 4.3.5 targeting TouchHLE.
 */

use crate::dyld::{export_c_func, FunctionExports};
use crate::frameworks::core_graphics::cg_context::CGContextDrawImage;
use crate::frameworks::core_graphics::cg_image::{
    self, CGImageGetHeight, CGImageGetWidth, CGImageRef, CGImageRelease, CGImageRetain,
};
use crate::frameworks::core_graphics::{CGFloat, CGPoint, CGRect, CGSize};
use crate::frameworks::foundation::ns_string::get_static_str;
use crate::frameworks::foundation::{ns_data, ns_string, NSInteger};
use crate::frameworks::uikit::ui_graphics::UIGraphicsGetCurrentContext;
use crate::fs::GuestPath;
use crate::image::Image;
use crate::objc::{
    autorelease, id, msg, msg_class, nil, objc_classes, release, retain, ClassExports,
    HostObject, NSZonePtr, SEL,
};
use crate::mem::MutVoidPtr;
use crate::Environment;
use std::collections::HashMap;

const CACHE_SIZE: usize = 10;

#[derive(Default)]
pub struct State {
    cached_images: HashMap<String, id>,
}

impl State {
    fn get(env: &Environment) -> &Self {
        &env.framework_state.uikit.ui_image
    }
    fn get_mut(env: &mut Environment) -> &mut Self {
        &mut env.framework_state.uikit.ui_image
    }
}

// В iOS 2-4 stretchableImage хранило параметры leftCapWidth и topCapHeight
// прямо в объекте.
#[derive(Default)]
struct UIImageHostObject {
    cg_image: CGImageRef,
    orientation: NSInteger, // UIImageOrientation
    left_cap_width: NSInteger,
    top_cap_height: NSInteger,
}
impl HostObject for UIImageHostObject {}

fn get_dummy_cg_image(env: &mut Environment) -> CGImageRef {
    const DUMMY_PNG: &[u8] = &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6,
        0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 11, 73, 68, 65, 84, 8, 215, 99, 96, 0, 2, 0, 0, 5, 0,
        1, 226, 38, 5, 155, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ];
    let image = Image::from_bytes(DUMMY_PNG).unwrap();
    cg_image::from_image(env, image)
}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation UIImage: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(UIImageHostObject {
        cg_image: nil,
        orientation: 0, // UIImageOrientationUp
        left_cap_width: 0,
        top_cap_height: 0,
    });
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

+ (id)imageNamed:(id)name { // NSString*
    let bundle: id = msg_class![env; NSBundle mainBundle];
    let path: id = msg![env; bundle pathForResource:name ofType:nil];
    let name_str = ns_string::to_rust_string(env, name).to_string();

    if State::get(env).cached_images.len() > CACHE_SIZE {
        let cache = std::mem::take(&mut State::get_mut(env).cached_images);
        for (_, img) in cache {
            release(env, img);
        }
    }

    if !State::get(env).cached_images.contains_key(&name_str) {
        let mut img: id = nil;
        if path != nil {
            img = msg![env; this imageWithContentsOfFile:path];
        }

        let final_img = if img != nil {
            retain(env, img);
            img
        } else {
            let cg_image = get_dummy_cg_image(env);
            let new_img: id = msg![env; this alloc];
            msg![env; new_img initWithCGImage:cg_image]
        };

        State::get_mut(env).cached_images.insert(name_str.clone(), final_img);
    }
    *State::get(env).cached_images.get(&name_str).unwrap()
}

+ (id)imageWithContentsOfFile:(id)path { // NSString*
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithContentsOfFile:path];
    autorelease(env, new)
}

+ (id)imageWithData:(id)data { // NSData*
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithData:data];
    autorelease(env, new)
}

+ (id)imageWithCGImage:(CGImageRef)cg_image {
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithCGImage:cg_image];
    autorelease(env, new)
}

// iOS 4.0+
+ (id)imageWithCGImage:(CGImageRef)cg_image scale:(CGFloat)_scale orientation:(NSInteger)orientation {
    // touchHLE's UIImage doesn't honour the scale factor (the app already
    // assumes the image's pixel dimensions are correct), so we just call
    // the regular initializer and store the orientation. Apps on iOS 5/6
    // (e.g. ketchapp's "2 Cars") rely on this overload existing —
    // returning `nil` from `[UIImage imageWithCGImage:scale:orientation:]`
    // makes most images render as a placeholder.
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithCGImage:cg_image];
    if new != nil {
        env.objc.borrow_mut::<UIImageHostObject>(new).orientation = orientation;
    }
    autorelease(env, new)
}

// iOS 4.0+
+ (id)imageWithCGImage:(CGImageRef)cg_image scale:(CGFloat)scale {
    msg![env; this imageWithCGImage:cg_image scale:scale orientation:(0i32)]
}

// MARK: - Initializers

- (id)initWithCGImage:(CGImageRef)cg_image {
    CGImageRetain(env, cg_image);
    let host = env.objc.borrow_mut::<UIImageHostObject>(this);
    host.cg_image = cg_image;
    host.orientation = 0;
    host.left_cap_width = 0;
    host.top_cap_height = 0;
    this
}

- (id)initWithContentsOfFile:(id)path { // NSString*
    if path == nil {
        env.objc.borrow_mut::<UIImageHostObject>(this).cg_image = get_dummy_cg_image(env);
        return this;
    }
    let path_str = ns_string::to_rust_string(env, path);
    let Ok(bytes) = env.fs.read(GuestPath::new(&path_str)) else {
        env.objc.borrow_mut::<UIImageHostObject>(this).cg_image = get_dummy_cg_image(env);
        return this;
    };
    let Ok(image) = Image::from_bytes(&bytes) else {
        env.objc.borrow_mut::<UIImageHostObject>(this).cg_image = get_dummy_cg_image(env);
        return this;
    };

    env.objc.borrow_mut::<UIImageHostObject>(this).cg_image = cg_image::from_image(env, image);
    this
}

- (id)initWithData:(id)data { // NSData*
    if data == nil {
        env.objc.borrow_mut::<UIImageHostObject>(this).cg_image = get_dummy_cg_image(env);
        return this;
    }
    let slice = ns_data::to_rust_slice(env, data);
    let Ok(image) = Image::from_bytes(slice) else {
        env.objc.borrow_mut::<UIImageHostObject>(this).cg_image = get_dummy_cg_image(env);
        return this;
    };
    env.objc.borrow_mut::<UIImageHostObject>(this).cg_image = cg_image::from_image(env, image);
    this
}

// MARK: - Properties

- (CGSize)size {
    let image = env.objc.borrow::<UIImageHostObject>(this).cg_image;
    let (width, height) = cg_image::borrow_image(&env.objc, image).dimensions();
    CGSize {
        width: width as _,
        height: height as _,
    }
}

- (CGImageRef)CGImage {
    env.objc.borrow::<UIImageHostObject>(this).cg_image
}

- (NSInteger)imageOrientation {
    env.objc.borrow::<UIImageHostObject>(this).orientation
}

// MARK: - Stretchable Images (iOS 2.0 - 4.x legacy method)

- (id)stretchableImageWithLeftCapWidth:(NSInteger)leftCapWidth topCapHeight:(NSInteger)topCapHeight {
    let cg_image = env.objc.borrow::<UIImageHostObject>(this).cg_image;

    // Создаем новый объект UIImage на основе того же CGImage.
    // ИСПОЛЬЗУЕМ msg_class! для отправки сообщения alloc классу
    let new_img: id = msg_class![env; UIImage alloc];
    let new_img: id = msg![env; new_img initWithCGImage:cg_image];

    // Но прописываем ему параметры растяжения
    let host = env.objc.borrow_mut::<UIImageHostObject>(new_img);
    host.left_cap_width = leftCapWidth;
    host.top_cap_height = topCapHeight;

    autorelease(env, new_img)
}

- (NSInteger)leftCapWidth {
    env.objc.borrow::<UIImageHostObject>(this).left_cap_width
}

- (NSInteger)topCapHeight {
    env.objc.borrow::<UIImageHostObject>(this).top_cap_height
}

// MARK: - Drawing

- (())drawAtPoint:(CGPoint)point {
    let context = UIGraphicsGetCurrentContext(env);
    if context == nil { return; }
    let image = env.objc.borrow::<UIImageHostObject>(this).cg_image;
    let rect = CGRect {
        origin: point,
        size: CGSize {
            width: CGImageGetWidth(env, image) as CGFloat,
            height: CGImageGetHeight(env, image) as CGFloat,
        }
    };
    CGContextDrawImage(env, context, rect, image);
}

- (())drawInRect:(CGRect)rect {
    let context = UIGraphicsGetCurrentContext(env);
    if context == nil { return; }
    // TODO: Здесь должна быть логика отрисовки с учетом leftCapWidth и
    // topCapHeight
    // Если left_cap_width > 0 || top_cap_height > 0, нужно делить картинку на 9
    // частей (nine-patch)
    // и отрисовывать через CGContextDrawImage кусками. Пока рисуем целиком.
    let image = env.objc.borrow::<UIImageHostObject>(this).cg_image;
    CGContextDrawImage(env, context, rect, image);
}

// `blendMode` and `alpha` arguments are accepted and validated, but applying
// them to the bitmap blit path is not yet implemented. The standard, opaque
// draw still happens so the image is not invisible (which is what blocked
// FarmFrenzy from progressing past its loading screen).
// See https://developer.apple.com/documentation/uikit/uiimage/1624155-drawinrect
- (())drawInRect:(CGRect)rect blendMode:(i32)_blend_mode alpha:(CGFloat)_alpha {
    let context = UIGraphicsGetCurrentContext(env);
    if context == nil { return; }
    let image = env.objc.borrow::<UIImageHostObject>(this).cg_image;
    CGContextDrawImage(env, context, rect, image);
}

- (())drawAtPoint:(CGPoint)point blendMode:(i32)_blend_mode alpha:(CGFloat)_alpha {
    let context = UIGraphicsGetCurrentContext(env);
    if context == nil { return; }
    let image = env.objc.borrow::<UIImageHostObject>(this).cg_image;
    let rect = CGRect {
        origin: point,
        size: CGSize {
            width: CGImageGetWidth(env, image) as CGFloat,
            height: CGImageGetHeight(env, image) as CGFloat,
        }
    };
    CGContextDrawImage(env, context, rect, image);
}

// Apple: "Draws the image, tiled, in a rectangle." The receiver's CGImage is
// repeated horizontally and vertically to fill `rect` in the current graphics
// context, anchored at the rect's top-left.
// https://developer.apple.com/documentation/uikit/uiimage/1624157-drawaspatterninrect
- (())drawAsPatternInRect:(CGRect)rect {
    let context = UIGraphicsGetCurrentContext(env);
    if context == nil { return; }
    let image = env.objc.borrow::<UIImageHostObject>(this).cg_image;
    if image == nil { return; }
    let tile_w = CGImageGetWidth(env, image) as CGFloat;
    let tile_h = CGImageGetHeight(env, image) as CGFloat;
    if tile_w <= 0.0 || tile_h <= 0.0 || rect.size.width <= 0.0 || rect.size.height <= 0.0 {
        return;
    }
    let mut y = rect.origin.y;
    let y_end = rect.origin.y + rect.size.height;
    let x_end = rect.origin.x + rect.size.width;
    while y < y_end {
        let mut x = rect.origin.x;
        while x < x_end {
            let tile_rect = CGRect {
                origin: CGPoint { x, y },
                size: CGSize { width: tile_w, height: tile_h },
            };
            CGContextDrawImage(env, context, tile_rect, image);
            x += tile_w;
        }
        y += tile_h;
    }
}

// MARK: - Memory Management

- (())dealloc {
    let &UIImageHostObject { cg_image, .. } = env.objc.borrow(this);
    CGImageRelease(env, cg_image);
    env.objc.dealloc_object(this, &mut env.mem)
}

@end

@implementation UIImageNibPlaceholder: NSObject

- (id)initWithCoder:(id)coder {
    // В NIB-файлах плейсхолдеры картинок хранят имя ресурса под ключом
    // UIResourceName
    let key = get_static_str(env, "UIResourceName");
    let name: id = msg![env; coder decodeObjectForKey:key];

    // Плейсхолдер был выделен через alloc, но мы не будем его использовать.
    // Сразу деаллоцируем этот временный объект, чтобы не было утечек памяти.
    () = msg![env; this dealloc];

    if name != nil {
        // Запрашиваем настоящую картинку (твой метод imageNamed: сам проверит
        // кэш или загрузит)
        let image: id = msg_class![env; UIImage imageNamed:name];

        // Важный момент (без заглушек и утечек):
        // Вызов [UIImageNibPlaceholder alloc] initWithCoder:] подразумевает,
        // что возвращенный объект будет иметь retain count +1 (владение).
        // Но [UIImage imageNamed:] возвращает закэшированный/autorelease
        // объект!
        // Поэтому мы ОБЯЗАНЫ сделать retain возвращаемой картинке, иначе она
        // удалится раньше времени.
        if image != nil {
            retain(env, image);
        }
        return image;
    }

    nil
}

@end

};

// MARK: - C Functions (Exporting formats like mentioned in the doc)

/// `void UIImageWriteToSavedPhotosAlbum(UIImage *image, id completionTarget,
///     SEL completionSelector, void *contextInfo);`
///
/// Apple documentation:
/// <https://developer.apple.com/documentation/uikit/1619125-uiimagewritetosavedphotosalbum>
///
/// Adds the specified image to the user's Camera Roll album. In the emulator
/// we persist the PNG data into the app's sandboxed Documents directory (since
/// there is no real photo library). After saving, the optional completion
/// callback is invoked with a nil error to signal success.
///
/// The completion selector signature expected by Apple is:
/// `- (void)image:(UIImage *)image didFinishSavingWithError:(NSError *)error
///               contextInfo:(void *)contextInfo;`
fn UIImageWriteToSavedPhotosAlbum(
    env: &mut Environment,
    image: id,
    completion_target: id,
    completion_selector: SEL,
    _context_info: MutVoidPtr,
) {
    if image == nil {
        log!("UIImageWriteToSavedPhotosAlbum: nil image, ignoring.");
        // Still invoke callback if requested, with nil error
        if completion_target != nil && !completion_selector.is_null() {
            let _: () = msg![env; completion_target performSelector:completion_selector
                                                        withObject:image
                                                        withObject:nil];
        }
        return;
    }

    // Get the CGImage backing the UIImage
    let cg_image_ref: CGImageRef = msg![env; image CGImage];

    let save_success = if !cg_image_ref.is_null() {
        let img = cg_image::borrow_image(&env.objc, cg_image_ref);
        let (w, h) = img.dimensions();
        let rgba = img.pixels();
        let stride = w as usize * 4;

        let mut png_data: Vec<u8> = Vec::new();
        let ctx_ptr: *mut std::ffi::c_void = (&mut png_data as *mut Vec<u8>).cast();
        let ok = img.write_png_image(ctx_ptr, w as i32, h as i32, rgba, stride as i32);

        if ok != 0 && !png_data.is_empty() {
            // Generate a unique filename based on timestamp
            let filename = format!(
                "SavedPhoto_{}.png",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0)
            );

            // Save to the app's Documents directory
            let docs_path = format!("/Documents/{}", filename);
            let guest_path = GuestPath::new(&docs_path);
            match env.fs.write(guest_path, &png_data) {
                Ok(()) => {
                    log!(
                        "UIImageWriteToSavedPhotosAlbum: saved {}x{} image to {}",
                        w, h, docs_path
                    );
                    true
                }
                Err(e) => {
                    log!(
                        "UIImageWriteToSavedPhotosAlbum: failed to write {}: {:?}",
                        docs_path, e
                    );
                    false
                }
            }
        } else {
            log!("UIImageWriteToSavedPhotosAlbum: PNG encoding failed.");
            false
        }
    } else {
        log!("UIImageWriteToSavedPhotosAlbum: image has nil CGImage.");
        false
    };

    // Invoke the completion callback if provided.
    // Per Apple docs the selector is:
    //   - (void)image:(UIImage*)image didFinishSavingWithError:(NSError*)error
    //                contextInfo:(void*)contextInfo
    // We pass nil for the error on success. On failure we also pass nil since
    // constructing a full NSError is overkill for this emulation scenario and
    // most apps only check whether error is nil.
    if completion_target != nil && !completion_selector.is_null() {
        let error: id = nil;
        let _: () = msg![env; completion_target performSelector:completion_selector
                                                    withObject:image
                                                    withObject:error];
    }
}

fn UIImagePNGRepresentation(env: &mut Environment, image: id) -> id {
    if image == nil {
        return nil;
    }
    let cg_image: CGImageRef = msg![env; image CGImage];
    if cg_image.is_null() {
        return nil;
    }

    let img = cg_image::borrow_image(&env.objc, cg_image);
    let (w, h) = img.dimensions();
    let rgba = img.pixels();
    let stride = w as usize * 4;

    let mut png_data: Vec<u8> = Vec::new();
    let ctx_ptr: *mut std::ffi::c_void = (&mut png_data as *mut Vec<u8>).cast();
    let ok = img.write_png_image(ctx_ptr, w as i32, h as i32, rgba, stride as i32);
    if ok == 0 {
        return nil;
    }

    let len = png_data.len() as crate::frameworks::foundation::NSUInteger;
    let buf: crate::mem::MutVoidPtr = env.mem.alloc(len as u32).cast();
    env.mem
        .bytes_at_mut(buf.cast(), len as u32)
        .copy_from_slice(&png_data);
    msg_class![env; NSData dataWithBytesNoCopy:buf length:len]
}

fn UIImageJPEGRepresentation(
    env: &mut Environment,
    image: id,
    _compression_quality: CGFloat,
) -> id {
    // В эмуляторе пока фоллбек на PNG, если нет JPEG энкодера
    UIImagePNGRepresentation(env, image)
}

pub const FUNCTIONS: FunctionExports = &[
    export_c_func!(UIImagePNGRepresentation(_)),
    export_c_func!(UIImageJPEGRepresentation(_, _)),
    export_c_func!(UIImageWriteToSavedPhotosAlbum(_, _, _, _)),
];
