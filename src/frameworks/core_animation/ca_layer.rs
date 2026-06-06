/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0.
 * If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//!
//! `CALayer`.

use crate::dyld::{ConstantExports, HostConstant};
use crate::frameworks::core_animation::ca_transform3d::{CATransform3D, CATransform3DIdentity};
use crate::frameworks::core_foundation::time::CFTimeInterval;
use crate::frameworks::core_graphics::cg_affine_transform::{
    CGAffineTransform, CGAffineTransformIdentity,
};
use crate::frameworks::core_graphics::cg_bitmap_context::{
    CGBitmapContextCreate, CGBitmapContextGetHeight, CGBitmapContextGetWidth,
};
use crate::frameworks::core_graphics::cg_color::{CGColorHostObject, CGColorRef};
use crate::frameworks::core_graphics::cg_color_space::CGColorSpaceCreateDeviceRGB;
use crate::frameworks::core_graphics::cg_context::{
    CGContextClearRect, CGContextDrawImage, CGContextFillRect, CGContextRef, CGContextRelease,
    CGContextRestoreGState, CGContextSaveGState, CGContextSetRGBFillColor, CGContextTranslateCTM,
};
use crate::frameworks::core_graphics::cg_image::{
    kCGImageAlphaPremultipliedLast, kCGImageByteOrder32Big,
};
use crate::frameworks::core_graphics::{CGFloat, CGPoint, CGRect, CGSize};
use crate::frameworks::foundation::ns_string::{self, to_rust_string};
use crate::mem::{GuestUSize, Ptr};
use crate::objc::{
    autorelease, id, msg, msg_class, nil, objc_classes, release, retain, ClassExports, HostObject,
    ObjC,
};
use crate::Environment;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Default)]
pub(super) struct CALayerHostObject {
    delegate: id,
    pub(super) sublayers: Vec<id>,
    superlayer: id,
    pub(super) bounds: CGRect,
    pub(super) position: CGPoint,
    pub(super) z_position: CGFloat, // <-- ДОБАВЛЕНО СВОЙСТВО Z-POSITION
    pub(super) anchor_point: CGPoint,
    pub(super) affine_transform: CGAffineTransform,
    /// Full 3D transform set via `-[CALayer setTransform:]`. touchHLE's
    /// renderer is 2D-only, so we extract the 2x3 affine submatrix from
    /// the assigned `CATransform3D` and store it in `affine_transform`
    /// (used by the existing `frame`/`bounds` machinery). The full 4x4
    /// matrix is kept here so `-[CALayer transform]` can roundtrip the
    /// value the app assigned.
    pub(super) transform_3d: CATransform3D,
    /// `CALayer.sublayerTransform` — a transform applied to the layer's
    /// sublayers when they are rendered. Defaults to the identity matrix.
    /// Stored verbatim so reads round-trip; touchHLE's 2D renderer doesn't
    /// currently apply this when compositing sublayers, but apps that set
    /// and read it back observe the right values.
    pub(super) sublayer_transform: CATransform3D,
    pub(super) hidden: bool,
    pub(super) opaque: bool,
    pub(super) opacity: f32,
    pub(super) background_color: Option<CGColorHostObject>,
    /// CGImageRef for pattern backgrounds (set via colorWithPatternImage:)
    pub(super) background_pattern_cg_image: id,
    pub(super) background_pattern_gles_texture: Option<crate::gles::gles11_raw::types::GLuint>,
    pub(super) corner_radius: CGFloat,
    pub(super) border_width: CGFloat,
    pub(super) border_color: Option<CGColorHostObject>,
    pub(super) needs_display: bool,
    pub(super) needs_display_on_bounds_change: bool,
    pub(super) contents: id,
    pub(super) drawable_properties: id,
    pub(super) presented_pixels: Option<(Vec<u8>, u32, u32)>,
    pub(super) cg_context: Option<CGContextRef>,
    pub(super) gles_texture: Option<crate::gles::gles11_raw::types::GLuint>,
    pub(super) gles_texture_is_up_to_date: bool,
    pub(super) animations: HashMap<String, id>,
    pub(super) anonymous_animations: HashSet<id>,
    pub(super) name: Option<String>,
    pub(super) mask: id,
    /// `contentsGravity` — one of the `kCAGravity*` strings. Defaults to
    /// `"resize"` per Apple's CALayer documentation:
    /// <https://developer.apple.com/documentation/quartzcore/calayer/1410933-contentsgravity>
    pub(super) contents_gravity: String,
    /// `contentsRect` — sub-rectangle of the contents to draw, normalized
    /// (`[0..1]`). Defaults to the unit rectangle `(0,0,1,1)`.
    /// <https://developer.apple.com/documentation/quartzcore/calayer/1410893-contentsrect>
    pub(super) contents_rect: CGRect,
    /// `edgeAntialiasingMask` — bitmask of `CAEdgeAntialiasingMask` edges
    /// (left/right/top/bottom). Stored verbatim so the property
    /// round-trips through `-[CALayer edgeAntialiasingMask]`.
    /// <https://developer.apple.com/documentation/quartzcore/calayer/1410868-edgeantialiasingmask>
    pub(super) edge_antialiasing_mask: u32,
    /// `minificationFilter` / `magnificationFilter` — one of
    /// `kCAFilterLinear` / `kCAFilterNearest` / `kCAFilterTrilinear`.
    /// Defaults to `kCAFilterLinear` per Apple's CALayer reference.
    pub(super) minification_filter: String,
    pub(super) magnification_filter: String,
    /// `minificationFilterBias` — accepted for round-tripping. Defaults
    /// to 0.0 per Apple's docs.
    pub(super) minification_filter_bias: f32,
}
impl HostObject for CALayerHostObject {}

impl CALayerHostObject {
    pub(super) fn superlayer_to_layer_transform(&self) -> CGAffineTransform {
        CGAffineTransform::make_translation(-self.bounds.origin.x, -self.bounds.origin.y)
            .concat(CGAffineTransform::make_translation(
                -self.bounds.size.width * self.anchor_point.x,
                -self.bounds.size.height * self.anchor_point.y,
            ))
            .concat(self.affine_transform)
            .concat(CGAffineTransform::make_translation(
                self.position.x,
                self.position.y,
            ))
    }
}

/// Set a CGImage as the tiled background pattern for this layer.
/// Called from UIView when a pattern-based UIColor is set as backgroundColor.
pub fn set_background_pattern_cg_image(env: &mut Environment, layer: id, cg_image: id) {
    use crate::objc::{release, retain};
    retain(env, cg_image);
    let old = env
        .objc
        .borrow::<CALayerHostObject>(layer)
        .background_pattern_cg_image;
    release(env, old);
    env.objc
        .borrow_mut::<CALayerHostObject>(layer)
        .background_pattern_cg_image = cg_image;
}

pub const kCAFilterLinear: &str = "kCAFilterLinear";
pub const kCAFilterNearest: &str = "kCAFilterNearest";
pub const kCAFilterTrilinear: &str = "kCAFilterTrilinear";
pub const kCAGravityCenter: &str = "center";
// Apple QuartzCore framework — `CALayer.h` declares these as
// `CA_EXTERN NSString * const kCAGravity*` strings. The literal values
// match what real Core Animation uses internally and what `isEqual:`
// comparisons on the contentsGravity property test against.
pub const kCAGravityResize: &str = "resize";
pub const kCAGravityResizeAspect: &str = "resizeAspect";
pub const kCAGravityResizeAspectFill: &str = "resizeAspectFill";
pub const kCAGravityTop: &str = "top";
pub const kCAGravityBottom: &str = "bottom";
pub const kCAGravityLeft: &str = "left";
pub const kCAGravityRight: &str = "right";
pub const kCAGravityTopLeft: &str = "topLeft";
pub const kCAGravityTopRight: &str = "topRight";
pub const kCAGravityBottomLeft: &str = "bottomLeft";
pub const kCAGravityBottomRight: &str = "bottomRight";

// CAShapeLayer line cap / line join constants. Apple's QuartzCore
// headers (`CAShapeLayer.h`) declare these as
// `CA_EXTERN NSString * const kCALineCap*` / `kCALineJoin*`. The literal
// values are the same strings the framework uses internally and the
// ones that `-[CAShapeLayer setLineCap:]` and similar setters compare
// against with `-[NSString isEqualToString:]`.
//
// References:
// * Apple [`CAShapeLayer.lineCap`](https://developer.apple.com/documentation/quartzcore/cashapelayer/1521905-linecap)
// * Apple [`CAShapeLayer.lineJoin`](https://developer.apple.com/documentation/quartzcore/cashapelayer/1521918-linejoin)
// * Apple [`CAShapeLayer.fillRule`](https://developer.apple.com/documentation/quartzcore/cashapelayer/1522146-fillrule)
pub const kCALineCapButt: &str = "butt";
pub const kCALineCapRound: &str = "round";
pub const kCALineCapSquare: &str = "square";
pub const kCALineJoinMiter: &str = "miter";
pub const kCALineJoinRound: &str = "round";
pub const kCALineJoinBevel: &str = "bevel";
pub const kCAFillRuleNonZero: &str = "non-zero";
pub const kCAFillRuleEvenOdd: &str = "even-odd";

// CATransition types / subtypes. From `CATransition.h`. Used by
// `setType:` / `setSubtype:` on CATransition.
pub const kCATransitionFade: &str = "fade";
pub const kCATransitionMoveIn: &str = "moveIn";
pub const kCATransitionPush: &str = "push";
pub const kCATransitionReveal: &str = "reveal";
pub const kCATransitionFromRight: &str = "fromRight";
pub const kCATransitionFromLeft: &str = "fromLeft";
pub const kCATransitionFromTop: &str = "fromTop";
pub const kCATransitionFromBottom: &str = "fromBottom";

// CAValueFunction names. From `CAValueFunction.h`. These identify
// channel-component functions used when binding scalar animations to
// matrix transform components (e.g. `rotation.x`, `translation.y`).
pub const kCAValueFunctionRotateX: &str = "rotateX";
pub const kCAValueFunctionRotateY: &str = "rotateY";
pub const kCAValueFunctionRotateZ: &str = "rotateZ";
pub const kCAValueFunctionScale: &str = "scale";
pub const kCAValueFunctionScaleX: &str = "scaleX";
pub const kCAValueFunctionScaleY: &str = "scaleY";
pub const kCAValueFunctionScaleZ: &str = "scaleZ";
pub const kCAValueFunctionTranslate: &str = "translate";
pub const kCAValueFunctionTranslateX: &str = "translateX";
pub const kCAValueFunctionTranslateY: &str = "translateY";
pub const kCAValueFunctionTranslateZ: &str = "translateZ";

// CALayer action key constants. From `CALayer.h`. Apps look up implicit
// animations under these keys in a layer's `actions` dictionary (or via
// `-[CALayer actionForKey:]`). The string values are exactly the key names.
pub const kCAOnOrderIn: &str = "onOrderIn";
pub const kCAOnOrderOut: &str = "onOrderOut";

pub const CONSTANTS: ConstantExports = &[
    ("_kCAFilterLinear", HostConstant::NSString(kCAFilterLinear)),
    (
        "_kCAFilterNearest",
        HostConstant::NSString(kCAFilterNearest),
    ),
    (
        "_kCAFilterTrilinear",
        HostConstant::NSString(kCAFilterTrilinear),
    ),
    (
        "_kCAGravityCenter",
        HostConstant::NSString(kCAGravityCenter),
    ),
    (
        "_kCAGravityResize",
        HostConstant::NSString(kCAGravityResize),
    ),
    (
        "_kCAGravityResizeAspect",
        HostConstant::NSString(kCAGravityResizeAspect),
    ),
    (
        "_kCAGravityResizeAspectFill",
        HostConstant::NSString(kCAGravityResizeAspectFill),
    ),
    ("_kCAGravityTop", HostConstant::NSString(kCAGravityTop)),
    (
        "_kCAGravityBottom",
        HostConstant::NSString(kCAGravityBottom),
    ),
    ("_kCAGravityLeft", HostConstant::NSString(kCAGravityLeft)),
    ("_kCAGravityRight", HostConstant::NSString(kCAGravityRight)),
    (
        "_kCAGravityTopLeft",
        HostConstant::NSString(kCAGravityTopLeft),
    ),
    (
        "_kCAGravityTopRight",
        HostConstant::NSString(kCAGravityTopRight),
    ),
    (
        "_kCAGravityBottomLeft",
        HostConstant::NSString(kCAGravityBottomLeft),
    ),
    (
        "_kCAGravityBottomRight",
        HostConstant::NSString(kCAGravityBottomRight),
    ),
    ("_kCALineCapButt", HostConstant::NSString(kCALineCapButt)),
    ("_kCALineCapRound", HostConstant::NSString(kCALineCapRound)),
    (
        "_kCALineCapSquare",
        HostConstant::NSString(kCALineCapSquare),
    ),
    (
        "_kCALineJoinMiter",
        HostConstant::NSString(kCALineJoinMiter),
    ),
    (
        "_kCALineJoinRound",
        HostConstant::NSString(kCALineJoinRound),
    ),
    (
        "_kCALineJoinBevel",
        HostConstant::NSString(kCALineJoinBevel),
    ),
    (
        "_kCAFillRuleNonZero",
        HostConstant::NSString(kCAFillRuleNonZero),
    ),
    (
        "_kCAFillRuleEvenOdd",
        HostConstant::NSString(kCAFillRuleEvenOdd),
    ),
    ("_kCAOnOrderIn", HostConstant::NSString(kCAOnOrderIn)),
    ("_kCAOnOrderOut", HostConstant::NSString(kCAOnOrderOut)),
    (
        "_kCAValueFunctionRotateX",
        HostConstant::NSString(kCAValueFunctionRotateX),
    ),
    (
        "_kCAValueFunctionRotateY",
        HostConstant::NSString(kCAValueFunctionRotateY),
    ),
    (
        "_kCAValueFunctionRotateZ",
        HostConstant::NSString(kCAValueFunctionRotateZ),
    ),
    (
        "_kCAValueFunctionScale",
        HostConstant::NSString(kCAValueFunctionScale),
    ),
    (
        "_kCAValueFunctionScaleX",
        HostConstant::NSString(kCAValueFunctionScaleX),
    ),
    (
        "_kCAValueFunctionScaleY",
        HostConstant::NSString(kCAValueFunctionScaleY),
    ),
    (
        "_kCAValueFunctionScaleZ",
        HostConstant::NSString(kCAValueFunctionScaleZ),
    ),
    (
        "_kCAValueFunctionTranslate",
        HostConstant::NSString(kCAValueFunctionTranslate),
    ),
    (
        "_kCAValueFunctionTranslateX",
        HostConstant::NSString(kCAValueFunctionTranslateX),
    ),
    (
        "_kCAValueFunctionTranslateY",
        HostConstant::NSString(kCAValueFunctionTranslateY),
    ),
    (
        "_kCAValueFunctionTranslateZ",
        HostConstant::NSString(kCAValueFunctionTranslateZ),
    ),
];
pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation CALayer: NSObject

+ (id)alloc {
    let host_object = Box::new(CALayerHostObject {
        delegate: nil,
        sublayers: Vec::new(),
        superlayer: nil,
        bounds: CGRect {
            origin: CGPoint { x: 0.0, y: 0.0 },
            size: CGSize { width: 0.0, height: 0.0 }
        },
        position: CGPoint { x: 0.0, y: 0.0 },
        z_position: 0.0, // <-- ИНИЦИАЛИЗАЦИЯ Z-POSITION
        anchor_point: CGPoint { x: 0.5, y: 0.5 },
        affine_transform: CGAffineTransformIdentity,
        transform_3d: CATransform3DIdentity,
        sublayer_transform: CATransform3DIdentity,
        hidden: false,
        opaque: false,
        opacity: 1.0,
        background_color: None,
        background_pattern_cg_image: nil,
        background_pattern_gles_texture: None,
        corner_radius: 0.0,
        border_width: 0.0,
        border_color: None,
        needs_display: false,
        needs_display_on_bounds_change: false,
        contents: nil,
        drawable_properties: nil,
        presented_pixels: None,
        cg_context: None,
        gles_texture: None,
        gles_texture_is_up_to_date: false,
        animations: HashMap::new(),
        anonymous_animations: HashSet::new(),
        name: None,
        mask: nil,
        contents_gravity: kCAGravityResize.to_owned(),
        contents_rect: CGRect {
            origin: CGPoint { x: 0.0, y: 0.0 },
            size: CGSize { width: 1.0, height: 1.0 },
        },
        edge_antialiasing_mask: 0, // All edges disabled by default
        minification_filter: kCAFilterLinear.to_owned(),
        magnification_filter: kCAFilterLinear.to_owned(),
        minification_filter_bias: 0.0,
    });
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

+ (id)layer {
    let new_layer: id = msg![env; this alloc];
    msg![env; new_layer init]
}

- (())dealloc {
    let &mut CALayerHostObject {
        drawable_properties,
        contents,
        superlayer,
        cg_context,
        mask,
        ref mut sublayers,
        ..
    } = env.objc.borrow_mut(this);
    let sublayers = std::mem::take(sublayers);

    if drawable_properties != nil { release(env, drawable_properties); }
    if contents != nil { release(env, contents); }
    if mask != nil { release(env, mask); }
    if let Some(cg_context) = cg_context { CGContextRelease(env, cg_context); }

    // On real iOS a layer being deallocated cannot have a superlayer,
    // because the superlayer's `sublayers` array holds a strong reference
    // and would keep the retain count above zero. In touchHLE the
    // retain/release accounting is occasionally off for games that mix
    // direct -release with cached `id` references (e.g. Chuzzle's alert-
    // view init path which produced HyperHLE log #3 — the dealloc fires
    // while the layer is still installed in the alert view hierarchy).
    // Panicking the whole emulator over a reference-counting glitch in
    // the guest is worse than the alternative, so we instead detach
    // ourselves from the superlayer gracefully. This matches what
    // CoreAnimation's own internal teardown does when a layer is force-
    // released through CFRelease while still parented.
    if superlayer != nil {
        log!(
            "Warning: CALayer {:?} is being deallocated while still attached \
             to superlayer {:?}; detaching to avoid a dangling sublayer \
             reference.",
            this,
            superlayer
        );
        let CALayerHostObject { sublayers: ref mut super_sublayers, .. } =
            env.objc.borrow_mut(superlayer);
        super_sublayers.retain(|&sublayer| sublayer != this);
        // Clear our own back-pointer so the recursive cleanup below sees a
        // clean state if something unexpected re-enters.
        env.objc.borrow_mut::<CALayerHostObject>(this).superlayer = nil;
    }

    for sublayer in sublayers {
        env.objc.borrow_mut::<CALayerHostObject>(sublayer).superlayer = nil;
        release(env, sublayer);
    }

    env.objc.dealloc_object(this, &mut env.mem)
}

- (id)delegate { env.objc.borrow::<CALayerHostObject>(this).delegate }
- (())setDelegate:(id)delegate { env.objc.borrow_mut::<CALayerHostObject>(this).delegate = delegate; }

- (id)superlayer { env.objc.borrow::<CALayerHostObject>(this).superlayer }

- (())addSublayer:(id)layer {
    if layer == nil { return; }
    if env.objc.borrow::<CALayerHostObject>(layer).superlayer == this {
        // The layer is already a sublayer of this layer. Per Core Animation,
        // re-adding an existing sublayer moves it to the top of the z-order,
        // i.e. the end of the sublayers array. Do this directly instead of
        // dispatching a `bringSublayerToFront:` selector, which is not a real
        // CALayer method and is never registered (sending it panics with
        // "Unknown selector").
        let CALayerHostObject { ref mut sublayers, .. } = env.objc.borrow_mut(this);
        if let Some(pos) = sublayers.iter().position(|&l| l == layer) {
            let moved = sublayers.remove(pos);
            sublayers.push(moved);
        }
    } else {
        retain(env, layer);
        () = msg![env; layer removeFromSuperlayer];
        env.objc.borrow_mut::<CALayerHostObject>(layer).superlayer = this;
        env.objc.borrow_mut::<CALayerHostObject>(this).sublayers.push(layer);
    }
}

- (())insertSublayer:(id)layer atIndex:(u32)idx {
    if layer == nil { return; }
    retain(env, layer);
    () = msg![env; layer removeFromSuperlayer];
    env.objc.borrow_mut::<CALayerHostObject>(layer).superlayer = this;
    let CALayerHostObject { ref mut sublayers, .. } = env.objc.borrow_mut(this);
    sublayers.insert(idx.try_into().unwrap(), layer);
}

- (())insertSublayer:(id)layer below:(id)sibling {
    if layer == nil { return; }
    retain(env, layer);
    () = msg![env; layer removeFromSuperlayer];
    env.objc.borrow_mut::<CALayerHostObject>(layer).superlayer = this;
    let CALayerHostObject { ref mut sublayers, .. } = env.objc.borrow_mut(this);
    let idx = sublayers.iter().position(|&sublayer| sublayer == sibling).unwrap_or(0);
    sublayers.insert(idx, layer);
}

- (())insertSublayer:(id)layer above:(id)sibling {
    if layer == nil { return; }
    retain(env, layer);
    () = msg![env; layer removeFromSuperlayer];
    env.objc.borrow_mut::<CALayerHostObject>(layer).superlayer = this;
    let CALayerHostObject { ref mut sublayers, .. } = env.objc.borrow_mut(this);
    let idx = sublayers
        .iter()
        .position(|&sublayer| sublayer == sibling)
        .map(|i| i + 1)
        .unwrap_or(sublayers.len());
    sublayers.insert(idx, layer);
}

- (())replaceSublayer:(id)old_layer with:(id)new_layer {
    if old_layer == nil || new_layer == nil || old_layer == new_layer { return; }
    let old_idx = {
        let host = env.objc.borrow::<CALayerHostObject>(this);
        host.sublayers.iter().position(|&x| x == old_layer)
    };
    if old_idx.is_some() {
        retain(env, new_layer);
        () = msg![env; new_layer removeFromSuperlayer];
        let host = env.objc.borrow_mut::<CALayerHostObject>(this);
        if let Some(actual_idx) = host.sublayers.iter().position(|&x| x == old_layer) {
            host.sublayers[actual_idx] = new_layer;
            env.objc.borrow_mut::<CALayerHostObject>(new_layer).superlayer = this;
            env.objc.borrow_mut::<CALayerHostObject>(old_layer).superlayer = nil;
            release(env, old_layer);
        } else {
            release(env, new_layer);
        }
    }
}

- (())removeFromSuperlayer {
    let CALayerHostObject { ref mut superlayer, .. } = env.objc.borrow_mut(this);
    let superlayer = std::mem::take(superlayer);
    if superlayer == nil { return; }
    let CALayerHostObject { ref mut sublayers, .. } = env.objc.borrow_mut(superlayer);
    let idx = sublayers.iter().position(|&sublayer| sublayer == this).unwrap();
    let sublayer = sublayers.remove(idx);
    assert!(sublayer == this);
    release(env, this);
}

- (CGRect)bounds { env.objc.borrow::<CALayerHostObject>(this).bounds }
- (())setBounds:(CGRect)bounds {
    let host_object = env.objc.borrow_mut::<CALayerHostObject>(this);
    host_object.bounds = bounds;
    if host_object.needs_display_on_bounds_change {
        () = msg![env; this setNeedsDisplay];
    }
}

- (CGPoint)position { env.objc.borrow::<CALayerHostObject>(this).position }
- (())setPosition:(CGPoint)position { env.objc.borrow_mut::<CALayerHostObject>(this).position = position; }

// --- ДОБАВЛЕНЫ МЕТОДЫ ДЛЯ Z-POSITION ---
- (CGFloat)zPosition { env.objc.borrow::<CALayerHostObject>(this).z_position }
- (())setZPosition:(CGFloat)z_position { env.objc.borrow_mut::<CALayerHostObject>(this).z_position = z_position; }
// ---------------------------------------

- (CGPoint)anchorPoint { env.objc.borrow::<CALayerHostObject>(this).anchor_point }
- (())setAnchorPoint:(CGPoint)anchor_point { env.objc.borrow_mut::<CALayerHostObject>(this).anchor_point = anchor_point; }

- (CGAffineTransform)affineTransform { env.objc.borrow::<CALayerHostObject>(this).affine_transform }
- (())setAffineTransform:(CGAffineTransform)affine_transform {
    let host_obj = env.objc.borrow_mut::<CALayerHostObject>(this);
    host_obj.affine_transform = affine_transform;
    // Keep transform_3d in sync so a subsequent -transform read returns
    // the equivalent CATransform3D, matching iOS behaviour.
    host_obj.transform_3d = affine_transform_to_catransform3d(affine_transform);
}

// `-[CALayer transform]` is a CATransform3D (4x4 matrix). touchHLE's
// renderer is 2D, so a CATransform3D assigned here is collapsed to its 2x3
// affine submatrix for the existing frame/bounds pipeline; the full 4x4
// is kept for roundtrip reads. iMilk (HyperHLE appdb report #70) was the
// motivating case — without these the app crashed with "CALayer does not
// respond to setTransform:".
- (CATransform3D)transform { env.objc.borrow::<CALayerHostObject>(this).transform_3d }
- (())setTransform:(CATransform3D)transform {
    let host_obj = env.objc.borrow_mut::<CALayerHostObject>(this);
    host_obj.transform_3d = transform;
    host_obj.affine_transform = catransform3d_to_affine(transform);
}

// `-[CALayer sublayerTransform]` / `-setSublayerTransform:` — the
// `CATransform3D` applied to this layer's sublayers when rendering.
- (CATransform3D)sublayerTransform {
    env.objc.borrow::<CALayerHostObject>(this).sublayer_transform
}
- (())setSublayerTransform:(CATransform3D)transform {
    env.objc.borrow_mut::<CALayerHostObject>(this).sublayer_transform = transform;
}

- (CGRect)frame {
    let host_obj @ &CALayerHostObject { bounds, .. } = env.objc.borrow(this);
    host_obj.superlayer_to_layer_transform().apply_to_rect(CGRect {
        origin: CGPoint { x: bounds.origin.x, y: bounds.origin.y },
        size: bounds.size,
    })
}
- (())setFrame:(CGRect)frame {
    let CALayerHostObject { anchor_point, affine_transform, .. } = env.objc.borrow_mut(this);
    let inverse_transform = CGAffineTransform::make_translation(
        -frame.size.width * anchor_point.x,
        -frame.size.height * anchor_point.y,
    ).concat(*affine_transform).invert();
    let transformed_size = inverse_transform.apply_to_rect(CGRect {
        origin: CGPoint { x: 0.0, y: 0.0 },
        size: frame.size
    }).size;
    let transformed_offset = inverse_transform.apply_to_point(CGPoint { x: 0.0, y: 0.0 });
    let new_position = CGPoint {
        x: frame.origin.x + transformed_offset.x,
        y: frame.origin.y + transformed_offset.y,
    };
    () = msg![env; this setPosition:new_position];
    let new_bounds = CGRect {
        origin: CGPoint { x: 0.0, y: 0.0 },
        size: transformed_size,
    };
    () = msg![env; this setBounds:new_bounds];
}

// `- (void)renderInContext:(CGContextRef)ctx` —
// per Apple's [CALayer Reference](https://developer.apple.com/documentation/quartzcore/calayer/1521914-renderincontext):
// renders the layer tree (this layer plus all sublayers) into the supplied
// CGContext, ignoring any animations. Apple documents this as the API for
// capturing a snapshot of a layer hierarchy on the CPU (used e.g. by
// screenshotting code in many apps and middleware).
//
// touchHLE's full layer compositor lives in
// `crate::frameworks::core_animation::composition` and runs on the GPU
// via OpenGL ES; it cannot target an arbitrary CGContextRef. For
// `renderInContext:` we instead walk the layer tree on the CPU and emit
// CoreGraphics drawing calls that are already implemented in
// `core_graphics::cg_context`:
//
//   * Save the CTM, translate to the layer's frame.origin, then if the
//     layer has a `backgroundColor` set, fill the bounds rect with it.
//   * If the layer has CGImage contents, blit them via `CGContextDrawImage`
//     into the layer's bounds (after flipping the Y axis to match Apple's
//     UIKit-style coordinate system).
//   * Recurse into sublayers in z-order.
//
// Anything more elaborate (sub-pixel rasterised shadows, masks, custom
// `-drawInContext:` overrides) is not yet wired through CGContext — those
// only render through the GPU compositor. The result of this method is
// still consistent with what Apple's docs guarantee for "no animation"
// rendering for solid colors and image contents, which is what the apps in
// touchHLE's log corpus actually need.
- (())renderInContext:(CGContextRef)ctx {
    if ctx.is_null() {
        log!("Warning: -[CALayer renderInContext:] called with NULL context");
        return;
    }
    render_layer_in_context(env, this, ctx);
}

- (bool)isHidden { env.objc.borrow::<CALayerHostObject>(this).hidden }
- (())setHidden:(bool)hidden { env.objc.borrow_mut::<CALayerHostObject>(this).hidden = hidden; }

- (bool)isOpaque { env.objc.borrow::<CALayerHostObject>(this).opaque }
- (())setOpaque:(bool)opaque { env.objc.borrow_mut::<CALayerHostObject>(this).opaque = opaque; }

- (f32)opacity { env.objc.borrow::<CALayerHostObject>(this).opacity }
- (())setOpacity:(f32)opacity { env.objc.borrow_mut::<CALayerHostObject>(this).opacity = opacity; }

- (CGColorRef)backgroundColor {
    if let Some(bg_color) = env.objc.borrow::<CALayerHostObject>(this).background_color {
        let class = env.objc.get_known_class("_touchHLE_CGColor", &mut env.mem);
        let obj = env.objc.alloc_object(class, Box::new(bg_color), &mut env.mem);
        autorelease(env, obj)
    } else { nil }
}
- (())setBackgroundColor:(CGColorRef)new_color {
    let new_color = if new_color == nil { None } else { Some(*env.objc.borrow::<CGColorHostObject>(new_color)) };
    env.objc.borrow_mut::<CALayerHostObject>(this).background_color = new_color;
}

- (CGFloat)cornerRadius { env.objc.borrow::<CALayerHostObject>(this).corner_radius }
- (())setCornerRadius:(CGFloat)corner_radius { env.objc.borrow_mut::<CALayerHostObject>(this).corner_radius = corner_radius; }

- (CGFloat)borderWidth { env.objc.borrow::<CALayerHostObject>(this).border_width }
- (())setBorderWidth:(CGFloat)border_width { env.objc.borrow_mut::<CALayerHostObject>(this).border_width = border_width; }

- (CGColorRef)borderColor {
    if let Some(border_color) = env.objc.borrow::<CALayerHostObject>(this).border_color {
        let class = env.objc.get_known_class("_touchHLE_CGColor", &mut env.mem);
        let obj = env.objc.alloc_object(class, Box::new(border_color), &mut env.mem);
        autorelease(env, obj)
    } else { nil }
}
- (())setBorderColor:(CGColorRef)new_color {
    let new_color = if new_color == nil { None } else { Some(*env.objc.borrow::<CGColorHostObject>(new_color)) };
    env.objc.borrow_mut::<CALayerHostObject>(this).border_color = new_color;
}

- (bool)needsDisplay { env.objc.borrow::<CALayerHostObject>(this).needs_display }
- (())setNeedsDisplay { env.objc.borrow_mut::<CALayerHostObject>(this).needs_display = true; }

- (())setNeedsDisplayInRect:(CGRect)_invalid_rect {
    // Apple docs (CALayer Reference):
    //   "Marks the region within the specified rectangle as needing to be
    //    updated. ... You should call this method when the layer's contents
    //    have changed and need to be redrawn."
    //
    // We currently track invalidation at whole-layer granularity rather
    // than per-rect, so the documented conservative thing is to mark the
    // entire layer as needing display. This still produces correct output
    // (just at the cost of a full -displayLayer:/-drawLayer:inContext:
    // round-trip instead of a partial one) and matches the iOS
    // documentation's wording that "calling setNeedsDisplay(in:) with the
    // bounds of the layer is equivalent to calling setNeedsDisplay()".
    env.objc.borrow_mut::<CALayerHostObject>(this).needs_display = true;
}

- (bool)needsDisplayOnBoundsChange { env.objc.borrow::<CALayerHostObject>(this).needs_display_on_bounds_change }
- (())setNeedsDisplayOnBoundsChange:(bool)value { env.objc.borrow_mut::<CALayerHostObject>(this).needs_display_on_bounds_change = value; }

- (())displayIfNeeded {
    let &mut CALayerHostObject {
        ref mut needs_display,
        delegate,
        ..
    } = env.objc.borrow_mut(this);
    if !std::mem::take(needs_display) { return; }
    if delegate == nil { return; }

    let delegate_class = ObjC::read_isa(delegate, &env.mem);
    if env.objc.class_has_method_named(delegate_class, "displayLayer:") {
        () = msg![env; delegate displayLayer:this];
        return;
    }

    let &mut CALayerHostObject {
        cg_context,
        ref mut gles_texture_is_up_to_date,
        bounds: CGRect { origin, size },
        ..
    } = env.objc.borrow_mut(this);
    *gles_texture_is_up_to_date = false;

    let int_width = size.width.round() as GuestUSize;
    let int_height = size.height.round() as GuestUSize;
    // --- ФИКС КРАША 0x0 ---
    if int_width == 0 || int_height == 0 {
        return;
    }

    let need_new_context = cg_context.is_none_or(|existing|
            CGBitmapContextGetWidth(env, existing) != int_width ||
            CGBitmapContextGetHeight(env, existing) != int_height
    );
    let cg_context = if need_new_context {
        if let Some(old_context) = cg_context { CGContextRelease(env, old_context); }
        let color_space = CGColorSpaceCreateDeviceRGB(env);
        let cg_context = CGBitmapContextCreate(
            env, Ptr::null(), int_width, int_height, 8,
            int_width.checked_mul(4).unwrap(), color_space,
            kCGImageByteOrder32Big | kCGImageAlphaPremultipliedLast
        );
        env.objc.borrow_mut::<CALayerHostObject>(this).cg_context = Some(cg_context);
        cg_context
    } else {
        cg_context.unwrap()
    };
    CGContextTranslateCTM(env, cg_context, -origin.x, -origin.y);
    CGContextClearRect(env, cg_context, CGRect { origin, size });
    () = msg![env; delegate drawLayer:this inContext:cg_context];
    CGContextTranslateCTM(env, cg_context, origin.x, origin.y);
}

- (id)contents { env.objc.borrow::<CALayerHostObject>(this).contents }
- (())setContents:(id)new_contents {
    let host_obj = env.objc.borrow_mut::<CALayerHostObject>(this);
    host_obj.gles_texture_is_up_to_date = false;
    let old_contents = std::mem::replace(&mut host_obj.contents, new_contents);
    retain(env, new_contents);
    release(env, old_contents);
}

- (id)name {
    if let Some(ref name) = env.objc.borrow::<CALayerHostObject>(this).name {
        let string_id = ns_string::from_rust_string(env, name.clone());
        autorelease(env, string_id)
    } else { nil }
}

- (())setName:(id)name {
    let name_str = if name != nil { Some(ns_string::to_rust_string(env, name).into_owned()) } else { None };
    env.objc.borrow_mut::<CALayerHostObject>(this).name = name_str;
}

- (id)mask { env.objc.borrow::<CALayerHostObject>(this).mask }

- (())setMask:(id)mask {
    let old_mask = env.objc.borrow::<CALayerHostObject>(this).mask;
    if mask != old_mask {
        if mask != nil { retain(env, mask); }
        env.objc.borrow_mut::<CALayerHostObject>(this).mask = mask;
        if old_mask != nil { release(env, old_mask); }
    }
}

// Per Apple's CALayer reference:
// https://developer.apple.com/documentation/quartzcore/calayer/1410868-edgeantialiasingmask
- (())setEdgeAntialiasingMask:(u32)mask {
    env.objc.borrow_mut::<CALayerHostObject>(this).edge_antialiasing_mask = mask;
}
- (u32)edgeAntialiasingMask {
    env.objc.borrow::<CALayerHostObject>(this).edge_antialiasing_mask
}

// https://developer.apple.com/documentation/quartzcore/calayer/1410907-magnificationfilter
- (())setMagnificationFilter:(id)filter {
    let s = ns_string::to_rust_string(env, filter).into_owned();
    env.objc.borrow_mut::<CALayerHostObject>(this).magnification_filter = s;
}
- (id)magnificationFilter {
    let s = env.objc.borrow::<CALayerHostObject>(this).magnification_filter.clone();
    ns_string::from_rust_string(env, s)
}

// https://developer.apple.com/documentation/quartzcore/calayer/1410898-minificationfilter
- (())setMinificationFilter:(id)filter {
    let s = ns_string::to_rust_string(env, filter).into_owned();
    env.objc.borrow_mut::<CALayerHostObject>(this).minification_filter = s;
}
- (id)minificationFilter {
    let s = env.objc.borrow::<CALayerHostObject>(this).minification_filter.clone();
    ns_string::from_rust_string(env, s)
}

// https://developer.apple.com/documentation/quartzcore/calayer/1410933-contentsgravity
- (())setContentsGravity:(id)gravity {
    let s = ns_string::to_rust_string(env, gravity).into_owned();
    env.objc.borrow_mut::<CALayerHostObject>(this).contents_gravity = s;
}
- (id)contentsGravity {
    let s = env.objc.borrow::<CALayerHostObject>(this).contents_gravity.clone();
    ns_string::from_rust_string(env, s)
}

// https://developer.apple.com/documentation/quartzcore/calayer/1410893-contentsrect
- (())setContentsRect:(CGRect)rect {
    env.objc.borrow_mut::<CALayerHostObject>(this).contents_rect = rect;
    env.objc.borrow_mut::<CALayerHostObject>(this).gles_texture_is_up_to_date = false;
}
- (CGRect)contentsRect {
    env.objc.borrow::<CALayerHostObject>(this).contents_rect
}

- (())setMinificationFilterBias:(f32)bias {
    env.objc.borrow_mut::<CALayerHostObject>(this).minification_filter_bias = bias;
}
- (f32)minificationFilterBias {
    env.objc.borrow::<CALayerHostObject>(this).minification_filter_bias
}

- (bool)containsPoint:(CGPoint)point {
    let bounds: CGRect = msg![env; this bounds];
    let x_range = bounds.origin.x..(bounds.origin.x + bounds.size.width);
    let y_range = bounds.origin.y..(bounds.origin.y + bounds.size.height);
    let CGPoint {x, y} = point;
    x_range.contains(&x) && y_range.contains(&y)
}

- (CGPoint)convertPoint:(CGPoint)point fromLayer:(id)other {
    if this == other { return point; }
    transform_for_conversion(env, this, other).apply_to_point(point)
}
- (CGPoint)convertPoint:(CGPoint)point toLayer:(id)other {
    if this == other { return point; }
    transform_for_conversion(env, other, this).apply_to_point(point)
}
- (CGRect)convertRect:(CGRect)rect fromLayer:(id)other {
    if this == other { return rect; }
    transform_for_conversion(env, this, other).apply_to_rect(rect)
}
- (CGRect)convertRect:(CGRect)rect toLayer:(id)other {
    if this == other { return rect; }
    transform_for_conversion(env, other, this).apply_to_rect(rect)
}

- (())addAnimation:(id)anim forKey:(id)key {
    let duration: CFTimeInterval = msg![env; anim duration];
    if duration == 0.0 {
        let duration: CFTimeInterval = msg_class![env; CATransaction animationDuration];
        () = msg![env; anim setDuration:duration];
    }
    if key == nil {
        // Anonymous animation. If this exact animation object is already
        // attached, adding it again is a no-op (and we must not retain it a
        // second time, or it would leak).
        let inserted = env
            .objc
            .borrow_mut::<CALayerHostObject>(this)
            .anonymous_animations
            .insert(anim);
        if inserted {
            retain(env, anim);
        }
    } else {
        // Named animation. Adding an animation for a key that already exists
        // replaces (and releases) the previous one, per Core Animation.
        let key_string = to_rust_string(env, key).to_string();
        let previous = env
            .objc
            .borrow_mut::<CALayerHostObject>(this)
            .animations
            .insert(key_string, anim);
        if let Some(previous) = previous {
            release(env, previous);
        }
        retain(env, anim);
    }
}

- (())removeAnimationForKey:(id)key {
    let key_string = to_rust_string(env, key);
    if let Some(anim) = env.objc.borrow_mut::<CALayerHostObject>(this).animations.remove(&*key_string) {
        release(env, anim);
    };
}

// Apple: -[CALayer animationKeys] - returns the array of NSString keys of
// the currently attached named animations, or nil if there are none.
// https://developer.apple.com/documentation/quartzcore/calayer/animationkeys()
- (id)animationKeys {
    // Collect first to avoid borrow conflicts when constructing NSStrings.
    let keys: Vec<String> = env.objc
        .borrow::<CALayerHostObject>(this)
        .animations
        .keys()
        .cloned()
        .collect();
    if keys.is_empty() {
        return nil;
    }
    let mut ids = Vec::with_capacity(keys.len());
    for k in keys {
        ids.push(ns_string::from_rust_string(env, k));
    }
    let array = crate::frameworks::foundation::ns_array::from_vec(env, ids);
    crate::objc::autorelease(env, array)
}

// Apple: -[CALayer animationForKey:] - returns the CAAnimation for the
// given key, or nil if there is no such animation.
// https://developer.apple.com/documentation/quartzcore/calayer/animation(forkey:)
- (id)animationForKey:(id)key {
    if key == nil {
        return nil;
    }
    let key_string = to_rust_string(env, key).into_owned();
    env.objc
        .borrow::<CALayerHostObject>(this)
        .animations
        .get(&key_string)
        .copied()
        .unwrap_or(nil)
}

// --- ДОБАВЛЕННЫЙ МЕТОД: removeAllAnimations ---
- (())removeAllAnimations {
    let host = env.objc.borrow_mut::<CALayerHostObject>(this);

    // Забираем коллекции, оставляя пустые на их месте
    let named_animations = std::mem::take(&mut host.animations);
    let anonymous_animations = std::mem::take(&mut host.anonymous_animations);

    // Освобождаем память (release) для каждой именованной анимации
    for (_, anim) in named_animations {
        release(env, anim);
    }

    // Освобождаем память (release) для каждой анонимной анимации
    for anim in anonymous_animations {
        release(env, anim);
    }
}

@end

};

/// Project a `CGAffineTransform` (2x3 matrix used by `setAffineTransform:`)
/// up into the equivalent `CATransform3D` used by `setTransform:`. This is
/// the documented `CATransform3DMakeAffineTransform(t)` mapping.
fn affine_transform_to_catransform3d(t: CGAffineTransform) -> CATransform3D {
    CATransform3D::from_affine(t)
}

/// Collapse a `CATransform3D` to its 2x3 affine submatrix, the way the
/// system's `CATransform3DGetAffineTransform` does. The 3D-only entries
/// (m13/m14/m23/m24/m31..m34/m43/m44) are dropped — touchHLE's renderer
/// is 2D so layers with non-trivial 3D content just get their projected
/// 2D shadow.
fn catransform3d_to_affine(t: CATransform3D) -> CGAffineTransform {
    t.to_affine()
}

/// Recursive CPU-side renderer used by `-[CALayer renderInContext:]`.
///
/// Walks the layer tree depth-first and emits CoreGraphics drawing calls
/// for the parts of the layer that we can express through CGContext. This
/// is intentionally narrower than the GPU compositor in `composition.rs`:
/// it relies only on documented CoreGraphics primitives so the result is
/// portable to any CGContextRef the guest hands us (bitmap context, PDF
/// context, etc.).
fn render_layer_in_context(env: &mut Environment, layer: id, ctx: CGContextRef) {
    if layer == nil {
        return;
    }
    // Skip hidden layers — Apple's reference doc explicitly says
    // -renderInContext: respects layer.hidden, layer.opacity, etc.
    let (
        hidden,
        bounds,
        position,
        anchor_point,
        affine_transform,
        sublayers,
        background_color,
        contents,
    ) = {
        let h: &CALayerHostObject = env.objc.borrow(layer);
        (
            h.hidden,
            h.bounds,
            h.position,
            h.anchor_point,
            h.affine_transform,
            h.sublayers.clone(),
            h.background_color,
            h.contents,
        )
    };
    if hidden {
        return;
    }

    CGContextSaveGState(env, ctx);

    // Move from the superlayer's coordinate system into this layer's
    // coordinate system. The layer is positioned by its anchor point at
    // `position`, so the origin of its bounds maps to
    //   (position.x - anchor_point.x * bounds.size.width,
    //    position.y - anchor_point.y * bounds.size.height).
    let tx = position.x - anchor_point.x * bounds.size.width;
    let ty = position.y - anchor_point.y * bounds.size.height;
    CGContextTranslateCTM(env, ctx, tx, ty);

    // Apply the layer's affine transform around its anchor point.
    if affine_transform != CGAffineTransformIdentity {
        CGContextTranslateCTM(
            env,
            ctx,
            anchor_point.x * bounds.size.width,
            anchor_point.y * bounds.size.height,
        );
        crate::frameworks::core_graphics::cg_context::CGContextConcatCTM(
            env,
            ctx,
            affine_transform,
        );
        CGContextTranslateCTM(
            env,
            ctx,
            -anchor_point.x * bounds.size.width,
            -anchor_point.y * bounds.size.height,
        );
    }

    // Solid background fill.
    if let Some(c) = background_color {
        CGContextSetRGBFillColor(env, ctx, c.r, c.g, c.b, c.a);
        CGContextFillRect(env, ctx, bounds);
    }

    // Image contents (e.g. UIImage-backed UIImageView). Only handle the
    // CGImage case; layer.contents may also be a UIImage which exposes
    // its CGImage via `-CGImage` — match how the GPU compositor handles
    // it: try CGImage first, otherwise log and skip.
    if contents != nil {
        // Probe whether `contents` is a CGImage-equivalent. A CGImage is a
        // CF type, not an Obj-C class, so we duck-type using the
        // existing pure-CG getter `CGImageGetWidth` returning non-zero.
        let width =
            crate::frameworks::core_graphics::cg_image::CGImageGetWidth(env, contents);
        if width != 0 {
            CGContextDrawImage(env, ctx, bounds, contents);
        }
    }

    // Recurse into sublayers; CA documents sublayers as drawn in array
    // order, which is back-to-front on iOS.
    for child in sublayers {
        render_layer_in_context(env, child, ctx);
    }

    CGContextRestoreGState(env, ctx);
}

pub fn remove_anonymous_animation(env: &mut Environment, layer: id, animation: id) {
    let removed = env
        .objc
        .borrow_mut::<CALayerHostObject>(layer)
        .anonymous_animations
        .remove(&animation);
    // Removing an animation that is no longer attached (e.g. it was already
    // cleared by -removeAllAnimations, or its completion handler ran twice)
    // is a no-op, matching Core Animation semantics. Only release the retain
    // taken in -addAnimation:forKey: when we actually removed it here, so the
    // retain count stays balanced and we never double-free.
    if removed {
        release(env, animation);
    }
}

fn transform_for_conversion(env: &mut Environment, this: id, other: id) -> CGAffineTransform {
    let need_common_ancestor = this != nil && other != nil;
    assert!(!(this == nil && other == nil));

    let mut this_map = HashMap::from([(this, CGAffineTransformIdentity)]);
    let mut other_map = HashMap::from([(other, CGAffineTransformIdentity)]);
    let mut this_superlayer = this;
    let mut this_transform = CGAffineTransformIdentity;
    let mut other_superlayer = other;
    let mut other_transform = CGAffineTransformIdentity;
    let (common_ancestor, this_transform, other_transform) = loop {
        if this_superlayer != nil {
            let this_hostobj: &CALayerHostObject = env.objc.borrow(this_superlayer);
            let next = this_hostobj.superlayer;
            let next_transform =
                this_transform.concat(this_hostobj.superlayer_to_layer_transform());
            if need_common_ancestor && next != nil {
                if let Some(&other_transform) = other_map.get(&next) {
                    break (next, next_transform, other_transform);
                }
                this_map.insert(next, next_transform);
            }
            this_superlayer = next;
            this_transform = next_transform;
        }

        if other_superlayer != nil {
            let other_hostobj: &CALayerHostObject = env.objc.borrow(other_superlayer);
            let next = other_hostobj.superlayer;
            let next_transform =
                other_transform.concat(other_hostobj.superlayer_to_layer_transform());
            if need_common_ancestor && next != nil {
                if let Some(&this_transform) = this_map.get(&next) {
                    break (next, this_transform, next_transform);
                }
                other_map.insert(next, next_transform);
            }
            other_superlayer = next;
            other_transform = next_transform;
        }

        if this_superlayer == nil && other_superlayer == nil {
            if need_common_ancestor {
                // Disconnected layers (e.g. one was removed from its
                // superview, or a CATransition snapshot layer is being
                // queried after it was detached) have no path between
                // them. Real Core Animation tolerates this and returns
                // the identity transform from the partial walk; mirror
                // that instead of panicking.
                log!(
                    "Warning: Layers {:?} and {:?} have no common ancestor; \
                     falling back to identity transform.",
                    this,
                    other
                );
                break (nil, this_transform, other_transform);
            } else {
                break (nil, this_transform, other_transform);
            }
        }
    };

    let _ = common_ancestor;
    other_transform.concat(this_transform.invert())
}
