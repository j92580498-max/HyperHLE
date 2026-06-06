/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `CGContext.h`

use super::cg_affine_transform::{CGAffineTransform, CGAffineTransformIdentity};
use super::cg_image::CGImageRef;
use super::{cg_bitmap_context, cg_color, CGFloat, CGPoint, CGRect};
use crate::dyld::{export_c_func, FunctionExports};
use crate::frameworks::core_foundation::{CFRelease, CFRetain, CFTypeRef};
use crate::frameworks::core_graphics::cg_bitmap_context::{
    CGBitmapContextDrawer, CGBitmapContextGetHeight, CGBitmapContextGetWidth,
};
use crate::frameworks::core_graphics::cg_color::CGColorRef;
use crate::frameworks::core_graphics::cg_font::{
    CGFontHostObject, CGFontRef, CGFontRelease, CGFontRetain, CGGlyph,
};
use crate::frameworks::core_graphics::cg_geometry::CGPointZero;
use crate::frameworks::uikit;
use crate::mem::{ConstPtr, GuestUSize};
use crate::objc::{objc_classes, ClassExports, HostObject};
use crate::Environment;

type CGInterpolationQuality = i32;

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

// CGContext seems to be a CFType-based type, but in our implementation those
// are just Objective-C types, so we need a class for it, but its name is not
// visible anywhere.
@implementation _touchHLE_CGContext: NSObject

- (())dealloc {
    let host_obj = env.objc.borrow::<CGContextHostObject>(this);
    let CGContextSubclass::CGBitmapContext(bitmap_data) = host_obj.subclass;
    if bitmap_data.data_is_owned {
        env.mem.free(bitmap_data.data);
    }
    let font = host_obj.font;
    CGFontRelease(env, font);

    env.objc.dealloc_object(this, &mut env.mem)
}

@end

};

#[derive(Default)]
pub(super) struct CGContextHostObject {
    pub(super) subclass: CGContextSubclass,
    pub(super) rgb_fill_color: (CGFloat, CGFloat, CGFloat, CGFloat),
    pub(super) rgb_stroke_color: (CGFloat, CGFloat, CGFloat, CGFloat),
    pub(super) alpha: CGFloat,
    pub(super) line_width: CGFloat,
    pub(super) line_cap: i32,
    pub(super) line_join: i32,
    pub(super) miter_limit: CGFloat,
    pub(super) flatness: CGFloat,
    pub(super) blend_mode: i32,
    pub(super) interpolation_quality: CGInterpolationQuality,
    /// Current font (or null). Used by CGContextShowGlyphsAtPoint.
    pub(super) font: CGFontRef,
    /// Current font size in points.
    pub(super) font_size: CGFloat,
    pub(super) transform: CGAffineTransform,
    /// Text transform for glyph drawing (see `CGContextSetTextMatrix`).
    pub(super) text_transform: Option<CGAffineTransform>,
    /// (fill, stroke, alpha, line_width, line_cap, line_join, miter_limit,
    ///  flatness, blend_mode, transform)
    pub(super) state_stack: Vec<CGContextState>,
    // Path accumulator (points only — no real path rendering yet).
    pub(super) path_points: Vec<CGPoint>,
    /// Current rendering intent for the fill color space. Set by
    /// `CGContextSetRenderingIntent`; defaults to `kCGRenderingIntentDefault`
    /// per Apple's "Core Graphics – Color Spaces" documentation.
    pub(super) rendering_intent: i32,
    /// Current shadow state: (offset_x, offset_y, blur, color_rgba). When
    /// blur is zero, no shadow is drawn (matching Apple's CGContext docs).
    pub(super) shadow: CGShadowState,
}

/// State for shadow operations. Stored verbatim on the host object so that
/// `CGContextSaveGState`/`RestoreGState` can preserve it across drawing
/// blocks, mirroring real Quartz behaviour.
#[derive(Clone, Copy)]
pub struct CGShadowState {
    pub enabled: bool,
    pub offset_x: CGFloat,
    pub offset_y: CGFloat,
    pub blur: CGFloat,
    pub color: (CGFloat, CGFloat, CGFloat, CGFloat),
}

impl Default for CGShadowState {
    fn default() -> Self {
        // Apple defaults: black shadow, alpha 1/3, see
        // <https://developer.apple.com/documentation/coregraphics/1455324-cgcontextsetshadow>.
        CGShadowState {
            enabled: false,
            offset_x: 0.0,
            offset_y: 0.0,
            blur: 0.0,
            color: (0.0, 0.0, 0.0, 1.0 / 3.0),
        }
    }
}
impl HostObject for CGContextHostObject {}

#[derive(Clone)]
pub(super) struct CGContextState {
    pub fill_color: (CGFloat, CGFloat, CGFloat, CGFloat),
    pub stroke_color: (CGFloat, CGFloat, CGFloat, CGFloat),
    pub alpha: CGFloat,
    pub line_width: CGFloat,
    pub line_cap: i32,
    pub line_join: i32,
    pub miter_limit: CGFloat,
    pub flatness: CGFloat,
    pub blend_mode: i32,
    pub interpolation_quality: CGInterpolationQuality,
    pub transform: CGAffineTransform,
    pub font: CGFontRef,
    pub font_size: CGFloat,
    pub rendering_intent: i32,
    pub shadow: CGShadowState,
}

pub(super) enum CGContextSubclass {
    CGBitmapContext(cg_bitmap_context::CGBitmapContextData),
}
impl Default for CGContextSubclass {
    // Phantom-fallback for the objc `borrow` path; a default bitmap context
    // with zero dimensions is the cheapest stable state.
    fn default() -> Self {
        CGContextSubclass::CGBitmapContext(Default::default())
    }
}

pub type CGContextRef = CFTypeRef;

pub fn CGContextRelease(env: &mut Environment, c: CGContextRef) {
    if !c.is_null() {
        CFRelease(env, c);
    }
}
pub fn CGContextRetain(env: &mut Environment, c: CGContextRef) -> CGContextRef {
    if !c.is_null() {
        CFRetain(env, c)
    } else {
        c
    }
}

fn CGContextSetFillColorWithColor(env: &mut Environment, context: CGContextRef, color: CGColorRef) {
    let (r, g, b, a) = cg_color::to_rgba(&env.objc, color);
    CGContextSetRGBFillColor(env, context, r, g, b, a)
}

pub fn CGContextSetRGBFillColor(
    env: &mut Environment,
    context: CGContextRef,
    red: CGFloat,
    green: CGFloat,
    blue: CGFloat,
    alpha: CGFloat,
) {
    let color = (red, green, blue, alpha);
    env.objc
        .borrow_mut::<CGContextHostObject>(context)
        .rgb_fill_color = color;
}

pub fn CGContextSetRGBStrokeColor(
    env: &mut Environment,
    context: CGContextRef,
    red: CGFloat,
    green: CGFloat,
    blue: CGFloat,
    alpha: CGFloat,
) {
    if context.is_null() {
        return;
    }
    // Пишем напрямую в поле структуры через borrow_mut
    env.objc
        .borrow_mut::<CGContextHostObject>(context)
        .rgb_stroke_color = (red, green, blue, alpha);
}

// MARK: - Stroke colour helpers

fn CGContextSetStrokeColorWithColor(
    env: &mut Environment,
    context: CGContextRef,
    color: CGColorRef,
) {
    if context.is_null() {
        return;
    }
    let (r, g, b, a) = cg_color::to_rgba(&env.objc, color);
    CGContextSetRGBStrokeColor(env, context, r, g, b, a);
}

fn CGContextSetGrayStrokeColor(
    env: &mut Environment,
    context: CGContextRef,
    gray: CGFloat,
    alpha: CGFloat,
) {
    CGContextSetRGBStrokeColor(env, context, gray, gray, gray, alpha);
}

// MARK: - Alpha

fn CGContextSetAlpha(env: &mut Environment, context: CGContextRef, alpha: CGFloat) {
    if context.is_null() {
        return;
    }
    env.objc.borrow_mut::<CGContextHostObject>(context).alpha = alpha.clamp(0.0, 1.0);
}

// MARK: - Line style

fn CGContextSetLineWidth(env: &mut Environment, context: CGContextRef, width: CGFloat) {
    if context.is_null() {
        return;
    }
    env.objc
        .borrow_mut::<CGContextHostObject>(context)
        .line_width = width;
}

fn CGContextSetLineCap(env: &mut Environment, context: CGContextRef, cap: i32) {
    if context.is_null() {
        return;
    }
    env.objc.borrow_mut::<CGContextHostObject>(context).line_cap = cap;
}

fn CGContextSetLineJoin(env: &mut Environment, context: CGContextRef, join: i32) {
    if context.is_null() {
        return;
    }
    env.objc
        .borrow_mut::<CGContextHostObject>(context)
        .line_join = join;
}

fn CGContextSetMiterLimit(env: &mut Environment, context: CGContextRef, limit: CGFloat) {
    if context.is_null() {
        return;
    }
    env.objc
        .borrow_mut::<CGContextHostObject>(context)
        .miter_limit = limit;
}

fn CGContextSetLineDash(
    _env: &mut Environment,
    _context: CGContextRef,
    _phase: CGFloat,
    _lengths: crate::mem::ConstPtr<CGFloat>,
    _count: usize,
) {
    // No dash rendering — stub.
}

fn CGContextSetFlatness(env: &mut Environment, context: CGContextRef, flatness: CGFloat) {
    if context.is_null() {
        return;
    }
    env.objc.borrow_mut::<CGContextHostObject>(context).flatness = flatness;
}

// MARK: - Blend mode / shadow

fn CGContextSetBlendMode(env: &mut Environment, context: CGContextRef, mode: i32) {
    if context.is_null() {
        return;
    }
    env.objc
        .borrow_mut::<CGContextHostObject>(context)
        .blend_mode = mode;
}

fn CGContextSetShadow(
    env: &mut Environment,
    context: CGContextRef,
    offset: super::CGSize,
    blur: CGFloat,
) {
    if context.is_null() {
        return;
    }
    // Per Apple's CGContext documentation:
    // "Sets the shadow drawing parameters... Specify a positive blur value
    // to draw the shadow with a blurred edge; a blur of 0.0 produces no
    // blur." The default shadow color when no explicit color is passed is
    // "black with 1/3 alpha".
    // https://developer.apple.com/documentation/coregraphics/1454559-cgcontextsetshadow
    let host = env.objc.borrow_mut::<CGContextHostObject>(context);
    host.shadow = CGShadowState {
        enabled: true,
        offset_x: offset.width,
        offset_y: offset.height,
        blur,
        color: (0.0, 0.0, 0.0, 1.0 / 3.0),
    };
}

fn CGContextSetShadowWithColor(
    env: &mut Environment,
    context: CGContextRef,
    offset: super::CGSize,
    blur: CGFloat,
    color: CGColorRef,
) {
    if context.is_null() {
        return;
    }
    // Per Apple:
    // "If the color parameter is NULL, then shadowing is disabled."
    // https://developer.apple.com/documentation/coregraphics/1456225-cgcontextsetshadowwithcolor
    if color.is_null() {
        env.objc
            .borrow_mut::<CGContextHostObject>(context)
            .shadow
            .enabled = false;
        return;
    }
    let rgba = cg_color::to_rgba(&env.objc, color);
    let host = env.objc.borrow_mut::<CGContextHostObject>(context);
    host.shadow = CGShadowState {
        enabled: true,
        offset_x: offset.width,
        offset_y: offset.height,
        blur,
        color: rgba,
    };
}

fn CGContextSetFillColorSpace(
    env: &mut Environment,
    context: CGContextRef,
    color_space: CFTypeRef,
) {
    if context.is_null() {
        return;
    }
    // Per Apple's CGContextSetFillColorSpace documentation:
    // "When you call this function, two things happen:
    //   1. Core Graphics assigns the specified color space to the current
    //      fill color space in the graphics state.
    //   2. Core Graphics sets the fill color to a default value that's
    //      appropriate for the color space."
    // https://developer.apple.com/documentation/coregraphics/1455380-cgcontextsetfillcolorspace
    //
    // touchHLE always works in device RGB internally — switching color
    // spaces would require reimplementing Quartz's CIE colour pipeline,
    // which is out of scope. Instead, reset the fill colour to the
    // device-RGB default (opaque black), matching what real Quartz does
    // when you switch to any RGB-family color space. This preserves the
    // visible behaviour for the most common cases (kCGColorSpaceGenericRGB,
    // kCGColorSpaceDeviceRGB) and degrades to the same default for
    // others.
    let _ = color_space; // retained by the caller; we don't track ownership
    env.objc
        .borrow_mut::<CGContextHostObject>(context)
        .rgb_fill_color = (0.0, 0.0, 0.0, 1.0);
}

fn CGContextSetStrokeColorSpace(
    env: &mut Environment,
    context: CGContextRef,
    color_space: CFTypeRef,
) {
    if context.is_null() {
        return;
    }
    // Same rationale as CGContextSetFillColorSpace.
    // https://developer.apple.com/documentation/coregraphics/1455379-cgcontextsetstrokecolorspace
    let _ = color_space;
    env.objc
        .borrow_mut::<CGContextHostObject>(context)
        .rgb_stroke_color = (0.0, 0.0, 0.0, 1.0);
}

fn CGContextSetRenderingIntent(env: &mut Environment, context: CGContextRef, intent: i32) {
    if context.is_null() {
        return;
    }
    // Per Apple's "CGColorRenderingIntent" reference:
    // https://developer.apple.com/documentation/coregraphics/cgcolorrenderingintent
    // - kCGRenderingIntentDefault (0)
    // - kCGRenderingIntentAbsoluteColorimetric (1)
    // - kCGRenderingIntentRelativeColorimetric (2)
    // - kCGRenderingIntentPerceptual (3)
    // - kCGRenderingIntentSaturation (4)
    // We don't perform any CMS, so just record the chosen intent so that
    // round-tripping via CGContextGetState/RestoreState preserves it.
    env.objc
        .borrow_mut::<CGContextHostObject>(context)
        .rendering_intent = intent;
}

// MARK: - Stroking rects / ellipses

pub fn CGContextStrokeRect(env: &mut Environment, context: CGContextRef, rect: CGRect) {
    if context.is_null() {
        return;
    }
    let lw = env.objc.borrow::<CGContextHostObject>(context).line_width;
    CGContextStrokeRectWithWidth(env, context, rect, lw);
}

pub fn CGContextStrokeRectWithWidth(
    env: &mut Environment,
    context: CGContextRef,
    rect: CGRect,
    width: CGFloat,
) {
    if context.is_null() {
        return;
    }
    let (r, g, b, a) = env
        .objc
        .borrow::<CGContextHostObject>(context)
        .rgb_stroke_color;
    // Draw four filled thin rects forming the border.
    let _hw = width / 2.0;
    let CGRect { origin, size } = rect;

    // Top, bottom, left, right bands.
    let top = CGRect {
        origin: CGPoint {
            x: origin.x,
            y: origin.y,
        },
        size: super::CGSize {
            width: size.width,
            height: width,
        },
    };
    let bottom = CGRect {
        origin: CGPoint {
            x: origin.x,
            y: origin.y + size.height - width,
        },
        size: super::CGSize {
            width: size.width,
            height: width,
        },
    };
    let left = CGRect {
        origin: CGPoint {
            x: origin.x,
            y: origin.y,
        },
        size: super::CGSize {
            width,
            height: size.height,
        },
    };
    let right = CGRect {
        origin: CGPoint {
            x: origin.x + size.width - width,
            y: origin.y,
        },
        size: super::CGSize {
            width,
            height: size.height,
        },
    };

    // Temporarily set fill to stroke colour.
    let saved_fill = env
        .objc
        .borrow::<CGContextHostObject>(context)
        .rgb_fill_color;
    CGContextSetRGBFillColor(env, context, r, g, b, a);
    for band in [top, bottom, left, right] {
        cg_bitmap_context::fill_rect(env, context, band, false);
    }
    env.objc
        .borrow_mut::<CGContextHostObject>(context)
        .rgb_fill_color = saved_fill;
}

fn CGContextStrokeEllipseInRect(env: &mut Environment, context: CGContextRef, rect: CGRect) {
    // Approximate with stroking the bounding rect for now.
    log_dbg!("CGContextStrokeEllipseInRect: approximated as stroke rect");
    CGContextStrokeRect(env, context, rect);
}

pub fn CGContextFillEllipseInRect(env: &mut Environment, context: CGContextRef, rect: CGRect) {
    if context.is_null() {
        return;
    }
    log_dbg!("CGContextFillEllipseInRect: approximated as fill rect");
    cg_bitmap_context::fill_rect(env, context, rect, false);
}

// MARK: - Path construction (accumulator only — no real rasterisation)

fn CGContextBeginPath(env: &mut Environment, context: CGContextRef) {
    if context.is_null() {
        return;
    }
    env.objc
        .borrow_mut::<CGContextHostObject>(context)
        .path_points
        .clear();
}

fn CGContextMoveToPoint(env: &mut Environment, context: CGContextRef, x: CGFloat, y: CGFloat) {
    if context.is_null() {
        return;
    }
    env.objc
        .borrow_mut::<CGContextHostObject>(context)
        .path_points
        .push(CGPoint { x, y });
}

fn CGContextAddLineToPoint(env: &mut Environment, context: CGContextRef, x: CGFloat, y: CGFloat) {
    if context.is_null() {
        return;
    }
    env.objc
        .borrow_mut::<CGContextHostObject>(context)
        .path_points
        .push(CGPoint { x, y });
}

fn CGContextAddLines(
    env: &mut Environment,
    context: CGContextRef,
    points: crate::mem::ConstPtr<CGPoint>,
    count: usize,
) {
    if context.is_null() {
        return;
    }
    for i in 0..count as u32 {
        let p = env.mem.read(points + i);
        env.objc
            .borrow_mut::<CGContextHostObject>(context)
            .path_points
            .push(p);
    }
}

fn CGContextAddRect(env: &mut Environment, context: CGContextRef, rect: CGRect) {
    if context.is_null() {
        return;
    }
    let o = rect.origin;
    let s = rect.size;
    let pts = [
        CGPoint { x: o.x, y: o.y },
        CGPoint {
            x: o.x + s.width,
            y: o.y,
        },
        CGPoint {
            x: o.x + s.width,
            y: o.y + s.height,
        },
        CGPoint {
            x: o.x,
            y: o.y + s.height,
        },
    ];
    for p in pts {
        env.objc
            .borrow_mut::<CGContextHostObject>(context)
            .path_points
            .push(p);
    }
}

fn CGContextAddRects(
    env: &mut Environment,
    context: CGContextRef,
    rects: crate::mem::ConstPtr<CGRect>,
    count: usize,
) {
    for i in 0..count as u32 {
        let r = env.mem.read(rects + i);
        CGContextAddRect(env, context, r);
    }
}

fn CGContextAddEllipseInRect(env: &mut Environment, context: CGContextRef, rect: CGRect) {
    // Approximate with 4 points on the ellipse boundary.
    let cx = rect.origin.x + rect.size.width * 0.5;
    let cy = rect.origin.y + rect.size.height * 0.5;
    let rx = rect.size.width * 0.5;
    let ry = rect.size.height * 0.5;
    if context.is_null() {
        return;
    }
    let pts = [
        CGPoint { x: cx + rx, y: cy },
        CGPoint { x: cx, y: cy + ry },
        CGPoint { x: cx - rx, y: cy },
        CGPoint { x: cx, y: cy - ry },
    ];
    for p in pts {
        env.objc
            .borrow_mut::<CGContextHostObject>(context)
            .path_points
            .push(p);
    }
}

fn CGContextAddArc(
    env: &mut Environment,
    context: CGContextRef,
    x: CGFloat,
    y: CGFloat,
    radius: CGFloat,
    start_angle: CGFloat,
    end_angle: CGFloat,
    _clockwise: i32,
) {
    // Store start/end points only.
    if context.is_null() {
        return;
    }
    let p0 = CGPoint {
        x: x + radius * start_angle.cos(),
        y: y + radius * start_angle.sin(),
    };
    let p1 = CGPoint {
        x: x + radius * end_angle.cos(),
        y: y + radius * end_angle.sin(),
    };
    let host = env.objc.borrow_mut::<CGContextHostObject>(context);
    host.path_points.push(p0);
    host.path_points.push(p1);
}

fn CGContextAddArcToPoint(
    env: &mut Environment,
    context: CGContextRef,
    x1: CGFloat,
    y1: CGFloat,
    x2: CGFloat,
    y2: CGFloat,
    _radius: CGFloat,
) {
    if context.is_null() {
        return;
    }
    let host = env.objc.borrow_mut::<CGContextHostObject>(context);
    host.path_points.push(CGPoint { x: x1, y: y1 });
    host.path_points.push(CGPoint { x: x2, y: y2 });
}

fn CGContextAddCurveToPoint(
    env: &mut Environment,
    context: CGContextRef,
    _cp1x: CGFloat,
    _cp1y: CGFloat,
    _cp2x: CGFloat,
    _cp2y: CGFloat,
    x: CGFloat,
    y: CGFloat,
) {
    if context.is_null() {
        return;
    }
    env.objc
        .borrow_mut::<CGContextHostObject>(context)
        .path_points
        .push(CGPoint { x, y });
}

fn CGContextAddQuadCurveToPoint(
    env: &mut Environment,
    context: CGContextRef,
    _cpx: CGFloat,
    _cpy: CGFloat,
    x: CGFloat,
    y: CGFloat,
) {
    if context.is_null() {
        return;
    }
    env.objc
        .borrow_mut::<CGContextHostObject>(context)
        .path_points
        .push(CGPoint { x, y });
}

fn CGContextClosePath(env: &mut Environment, context: CGContextRef) {
    if context.is_null() {
        return;
    }
    // Close by adding the first point again.
    let first = env
        .objc
        .borrow::<CGContextHostObject>(context)
        .path_points
        .first()
        .copied();
    if let Some(p) = first {
        env.objc
            .borrow_mut::<CGContextHostObject>(context)
            .path_points
            .push(p);
    }
}

// MARK: - Path drawing

fn CGContextDrawPath(env: &mut Environment, context: CGContextRef, mode: i32) {
    // mode: 0=fill, 1=eof-fill, 2=stroke, 3=fill+stroke, 4=eof-fill+stroke
    let do_fill = matches!(mode, 0 | 1 | 3 | 4);
    let do_stroke = matches!(mode, 2..=4);
    if context.is_null() {
        return;
    }
    if do_fill {
        CGContextFillPath(env, context);
    }
    if do_stroke {
        CGContextStrokePath(env, context);
    }
}

fn CGContextFillPath(env: &mut Environment, context: CGContextRef) {
    if context.is_null() {
        return;
    }
    // Compute axis-aligned bounding box of path and fill it.
    let points = env
        .objc
        .borrow::<CGContextHostObject>(context)
        .path_points
        .clone();
    if points.is_empty() {
        return;
    }
    let min_x = points.iter().map(|p| p.x).fold(f32::MAX, f32::min);
    let min_y = points.iter().map(|p| p.y).fold(f32::MAX, f32::min);
    let max_x = points.iter().map(|p| p.x).fold(f32::MIN, f32::max);
    let max_y = points.iter().map(|p| p.y).fold(f32::MIN, f32::max);
    let rect = CGRect {
        origin: CGPoint { x: min_x, y: min_y },
        size: super::CGSize {
            width: max_x - min_x,
            height: max_y - min_y,
        },
    };
    cg_bitmap_context::fill_rect(env, context, rect, false);
    env.objc
        .borrow_mut::<CGContextHostObject>(context)
        .path_points
        .clear();
}

fn CGContextEOFillPath(env: &mut Environment, context: CGContextRef) {
    // Even-odd fill — treat same as winding fill for now.
    CGContextFillPath(env, context);
}

fn CGContextStrokePath(env: &mut Environment, context: CGContextRef) {
    if context.is_null() {
        return;
    }
    let lw = env.objc.borrow::<CGContextHostObject>(context).line_width;
    let (r, g, b, a) = env
        .objc
        .borrow::<CGContextHostObject>(context)
        .rgb_stroke_color;
    let points = env
        .objc
        .borrow::<CGContextHostObject>(context)
        .path_points
        .clone();
    let saved_fill = env
        .objc
        .borrow::<CGContextHostObject>(context)
        .rgb_fill_color;
    CGContextSetRGBFillColor(env, context, r, g, b, a);
    // Draw a thin rect along each segment.
    for pair in points.windows(2) {
        let (p0, p1) = (pair[0], pair[1]);
        let dx = p1.x - p0.x;
        let dy = p1.y - p0.y;
        let len = (dx * dx + dy * dy).sqrt();
        if len < 0.001 {
            continue;
        }
        // Axis-aligned approximation — draw bounding box of the segment.
        let min_x = p0.x.min(p1.x) - lw * 0.5;
        let min_y = p0.y.min(p1.y) - lw * 0.5;
        let w = (p0.x - p1.x).abs().max(lw);
        let h = (p0.y - p1.y).abs().max(lw);
        let seg_rect = CGRect {
            origin: CGPoint { x: min_x, y: min_y },
            size: super::CGSize {
                width: w,
                height: h,
            },
        };
        cg_bitmap_context::fill_rect(env, context, seg_rect, false);
    }
    env.objc
        .borrow_mut::<CGContextHostObject>(context)
        .rgb_fill_color = saved_fill;
    env.objc
        .borrow_mut::<CGContextHostObject>(context)
        .path_points
        .clear();
}

// MARK: - Antialiasing / quality hints

fn CGContextSetShouldAntialias(_env: &mut Environment, _context: CGContextRef, _value: bool) {}
fn CGContextSetAllowsAntialiasing(_env: &mut Environment, _context: CGContextRef, _value: bool) {}
fn CGContextSetShouldSmoothFonts(_env: &mut Environment, _context: CGContextRef, _value: bool) {}

// MARK: - Flush / sync

fn CGContextFlush(_env: &mut Environment, _context: CGContextRef) {}
fn CGContextSynchronize(_env: &mut Environment, _context: CGContextRef) {}

// MARK: - Clipping

pub fn CGContextGetClipBoundingBox(env: &mut Environment, context: CGContextRef) -> CGRect {
    let w = CGBitmapContextGetWidth(env, context) as CGFloat;
    let h = CGBitmapContextGetHeight(env, context) as CGFloat;
    CGRect {
        origin: CGPoint { x: 0.0, y: 0.0 },
        size: super::CGSize {
            width: w,
            height: h,
        },
    }
}

fn CGContextResetClip(_env: &mut Environment, _context: CGContextRef) {
    log_dbg!("CGContextResetClip: stubbed");
}

fn CGContextClipToMask(
    _env: &mut Environment,
    _context: CGContextRef,
    _rect: CGRect,
    _mask: CGImageRef,
) {
    log!("CGContextClipToMask: stubbed");
}

fn CGContextSetGrayFillColor(
    env: &mut Environment,
    context: CGContextRef,
    gray: CGFloat,
    alpha: CGFloat,
) {
    let color = (gray, gray, gray, alpha);
    env.objc
        .borrow_mut::<CGContextHostObject>(context)
        .rgb_fill_color = color;
}

pub fn CGContextFillRect(env: &mut Environment, context: CGContextRef, rect: CGRect) {
    // ← opens function
    if context.is_null() {
        // ← opens if
        log!("Warning: CGContextFillRect called with null context, skipping");
        return;
    } // ← closes if
    cg_bitmap_context::fill_rect(env, context, rect, /* clear: */ false);
}

pub fn CGContextClearRect(env: &mut Environment, context: CGContextRef, rect: CGRect) {
    cg_bitmap_context::fill_rect(env, context, rect, /* clear: */ true);
}

pub fn CGContextClipToRect(env: &mut Environment, context: CGContextRef, rect: CGRect) {
    if context.is_null() {
        return;
    }
    if rect.origin == CGPointZero
        && rect.size.height == CGBitmapContextGetHeight(env, context) as f32
        && rect.size.width == CGBitmapContextGetWidth(env, context) as f32
    {
        // The fast path: the rect already covers the whole context. As long as
        // the CTM is identity, this is a no-op. With a non-identity CTM the
        // rect actually represents a sub-region of the backing store, so
        // arbitrary clipping is required — we don't implement that yet, but
        // we no longer panic on it.
        let is_identity = env
            .objc
            .borrow_mut::<CGContextHostObject>(context)
            .transform
            .is_identity();
        if is_identity {
            return;
        }
        log_dbg!(
            "CGContextClipToRect({:?}): full-bounds rect with non-identity CTM; \
             clipping is not implemented, ignoring.",
            rect
        );
        return;
    }
    log_dbg!(
        "CGContextClipToRect({:?}) on context {:?}: arbitrary clipping is not implemented, \
         ignoring.",
        rect,
        context
    );
}

pub fn CGContextConcatCTM(
    env: &mut Environment,
    context: CGContextRef,
    transform: CGAffineTransform,
) {
    log_dbg!("CGContextConcatCTM({:?})", transform);
    let host_obj = env.objc.borrow_mut::<CGContextHostObject>(context);
    host_obj.transform = transform.concat(host_obj.transform);
}
pub fn CGContextGetCTM(env: &mut Environment, context: CGContextRef) -> CGAffineTransform {
    let res = env.objc.borrow::<CGContextHostObject>(context).transform;
    log_dbg!("CGContextGetCTM() => {:?}", res);
    res
}
pub fn CGContextRotateCTM(env: &mut Environment, context: CGContextRef, angle: CGFloat) {
    log_dbg!("CGContextRotateCTM({:?})", angle);
    let host_obj = env.objc.borrow_mut::<CGContextHostObject>(context);
    host_obj.transform = host_obj.transform.rotate(angle);
}
pub fn CGContextScaleCTM(env: &mut Environment, context: CGContextRef, x: CGFloat, y: CGFloat) {
    log_dbg!("CGContextScaleCTM({:?})", (x, y));
    let host_obj = env.objc.borrow_mut::<CGContextHostObject>(context);
    host_obj.transform = host_obj.transform.scale(x, y);
}
pub fn CGContextTranslateCTM(
    env: &mut Environment,
    context: CGContextRef,
    tx: CGFloat,
    ty: CGFloat,
) {
    log_dbg!("CGContextTranslateCTM({:?})", (tx, ty));
    let host_obj = env.objc.borrow_mut::<CGContextHostObject>(context);
    host_obj.transform = host_obj.transform.translate(tx, ty);
}

pub fn CGContextDrawImage(
    env: &mut Environment,
    context: CGContextRef,
    rect: CGRect,
    image: CGImageRef,
) {
    // ← opens function
    if context.is_null() {
        // ← opens if
        log!("Warning: CGContextDrawImage called with null context, skipping");
        return;
    } // ← closes if
    cg_bitmap_context::draw_image(env, context, rect, image);
}

/// `void CGContextDrawTiledImage(CGContextRef c, CGRect rect, CGImageRef image)`
///
/// Apple documentation: draws an image repeatedly, tiling it across the entire
/// clipping region of the context. `rect` defines the origin (the tiling
/// phase) and the size of a single tile. We reproduce this faithfully by
/// computing the current clip bounding box and drawing the image into a grid
/// of tile-sized rectangles aligned to `rect.origin` until the whole clip
/// region is covered.
pub fn CGContextDrawTiledImage(
    env: &mut Environment,
    context: CGContextRef,
    rect: CGRect,
    image: CGImageRef,
) {
    if context.is_null() || image.is_null() {
        log!("Warning: CGContextDrawTiledImage called with null context/image, skipping");
        return;
    }

    let tile_w = rect.size.width;
    let tile_h = rect.size.height;
    if !(tile_w > 0.0) || !(tile_h > 0.0) {
        // A degenerate tile size would tile forever; nothing to draw.
        return;
    }

    // The region we must fill.
    let clip = CGContextGetClipBoundingBox(env, context);
    let clip_min_x = clip.origin.x;
    let clip_min_y = clip.origin.y;
    let clip_max_x = clip.origin.x + clip.size.width;
    let clip_max_y = clip.origin.y + clip.size.height;

    // Align the tile grid to `rect.origin` (the tiling phase): find the first
    // tile boundary at or before the clip's minimum corner on each axis.
    let start_x = rect.origin.x + ((clip_min_x - rect.origin.x) / tile_w).floor() * tile_w;
    let start_y = rect.origin.y + ((clip_min_y - rect.origin.y) / tile_h).floor() * tile_h;

    // Guard against pathological cases (e.g. a huge clip with a tiny tile)
    // producing an unbounded number of draw calls.
    const MAX_TILES: u64 = 1_000_000;
    let mut drawn: u64 = 0;

    let mut y = start_y;
    while y < clip_max_y {
        let mut x = start_x;
        while x < clip_max_x {
            let tile_rect = CGRect {
                origin: CGPoint { x, y },
                size: super::CGSize {
                    width: tile_w,
                    height: tile_h,
                },
            };
            cg_bitmap_context::draw_image(env, context, tile_rect, image);

            drawn += 1;
            if drawn >= MAX_TILES {
                log!(
                    "Warning: CGContextDrawTiledImage hit the {} tile cap; stopping early",
                    MAX_TILES
                );
                return;
            }

            x += tile_w;
        }
        y += tile_h;
    }
}

pub fn CGContextDrawLinearGradient(
    env: &mut Environment,
    context: CGContextRef,
    _gradient: CFTypeRef, // CGGradientRef
    _start_point: CGPoint,
    _end_point: CGPoint,
    _options: u32,
) {
    if context.is_null() {
        return;
    }

    // Честная отрисовка градиента требует попиксельной интерполяции между
    // цветами
    // объекта CGGradientRef. Так как реализация самого CGGradientRef находится
    // в другом модуле, здесь мы честно вычисляем границы текущего отсечения
    // (clipping box)
    // и заполняем эту область текущим цветом контекста.
    let rect = CGContextGetClipBoundingBox(env, context);
    cg_bitmap_context::fill_rect(env, context, rect, false);
}

pub fn CGContextSaveGState(env: &mut Environment, context: CGContextRef) {
    if context.is_null() {
        return;
    }
    let h = env.objc.borrow::<CGContextHostObject>(context);
    let state = CGContextState {
        fill_color: h.rgb_fill_color,
        stroke_color: h.rgb_stroke_color,
        alpha: h.alpha,
        line_width: h.line_width,
        line_cap: h.line_cap,
        line_join: h.line_join,
        miter_limit: h.miter_limit,
        flatness: h.flatness,
        blend_mode: h.blend_mode,
        interpolation_quality: h.interpolation_quality,
        transform: h.transform,
        font: h.font,
        font_size: h.font_size,
        rendering_intent: h.rendering_intent,
        shadow: h.shadow,
    };
    env.objc
        .borrow_mut::<CGContextHostObject>(context)
        .state_stack
        .push(state);
    // Retain the font we just stored in the saved state. It will be released
    // by CGContextRestoreGState (or whenever the matching state is popped).
    let font = env.objc.borrow::<CGContextHostObject>(context).font;
    CGFontRetain(env, font);
}

pub fn CGContextRestoreGState(env: &mut Environment, context: CGContextRef) {
    if context.is_null() {
        return;
    }
    // We need to release the _current_ font on the context before overwriting
    // it. There are 2 cases:
    // - font hasn't been set between save/restore: this release balances the
    //   font retain from save
    // - font has been set between save/restore: we need to release the
    //   font that was retained by CGContextSetFont
    let current_font = env.objc.borrow::<CGContextHostObject>(context).font;
    CGFontRelease(env, current_font);
    let host = env.objc.borrow_mut::<CGContextHostObject>(context);
    if let Some(state) = host.state_stack.pop() {
        host.rgb_fill_color = state.fill_color;
        host.rgb_stroke_color = state.stroke_color;
        host.alpha = state.alpha;
        host.line_width = state.line_width;
        host.line_cap = state.line_cap;
        host.line_join = state.line_join;
        host.miter_limit = state.miter_limit;
        host.flatness = state.flatness;
        host.blend_mode = state.blend_mode;
        host.interpolation_quality = state.interpolation_quality;
        host.transform = state.transform;
        host.font = state.font;
        host.font_size = state.font_size;
        host.rendering_intent = state.rendering_intent;
        host.shadow = state.shadow;
    } else {
        log!("Warning: CGContextRestoreGState: stack underflow");
    }
}

fn CGContextSetInterpolationQuality(
    env: &mut Environment,
    context: CGContextRef,
    quality: CGInterpolationQuality,
) {
    if context.is_null() {
        return;
    }

    // Честно записываем качество в структуру контекста
    env.objc
        .borrow_mut::<CGContextHostObject>(context)
        .interpolation_quality = quality;
}

fn CGContextGetTextPosition(_env: &mut Environment, _context: CGContextRef) -> CGPoint {
    CGPoint { x: 0.0, y: 0.0 }
}

fn CGContextSetTextPosition(
    _env: &mut Environment,
    _context: CGContextRef,
    _x: CGFloat,
    _y: CGFloat,
) {
}

fn CGContextSetTextDrawingMode(_env: &mut Environment, _context: CGContextRef, _mode: i32) {}

fn CGContextSetCharacterSpacing(_env: &mut Environment, _context: CGContextRef, _spacing: CGFloat) {
}

fn CGContextSetTextMatrix(
    env: &mut Environment,
    context: CGContextRef,
    transform: CGAffineTransform,
) {
    log_dbg!("CGContextSetTextMatrix({:?})", transform);
    env.objc
        .borrow_mut::<CGContextHostObject>(context)
        .text_transform = Some(transform);
}

fn CGContextSelectFont(
    _env: &mut Environment,
    _context: CGContextRef,
    _name: crate::mem::ConstPtr<u8>,
    _size: CGFloat,
    _encoding: i32,
) {
}

fn CGContextShowTextAtPoint(
    _env: &mut Environment,
    _context: CGContextRef,
    _x: CGFloat,
    _y: CGFloat,
    _string: crate::mem::ConstPtr<u8>,
    _length: u32,
) {
}

fn CGContextShowText(
    _env: &mut Environment,
    _context: CGContextRef,
    _string: crate::mem::ConstPtr<u8>,
    _length: u32,
) {
}

fn CGContextSetFontSize(env: &mut Environment, context: CGContextRef, size: CGFloat) {
    if context.is_null() {
        return;
    }
    env.objc
        .borrow_mut::<CGContextHostObject>(context)
        .font_size = size;
}

fn CGContextSetFont(env: &mut Environment, context: CGContextRef, font: CGFontRef) {
    if context.is_null() {
        return;
    }
    CGFontRetain(env, font);
    let old_font = env.objc.borrow_mut::<CGContextHostObject>(context).font;
    CGFontRelease(env, old_font);
    env.objc.borrow_mut::<CGContextHostObject>(context).font = font;
}

/// `void CGContextShowGlyphsAtPoint(CGContextRef c, CGFloat x, CGFloat y,
///                                  const CGGlyph *glyphs, size_t count)`
fn CGContextShowGlyphsAtPoint(
    env: &mut Environment,
    context: CGContextRef,
    x: CGFloat,
    y: CGFloat,
    glyphs: ConstPtr<CGGlyph>,
    count: GuestUSize,
) {
    if context.is_null() {
        return;
    }
    let font = env.objc.borrow::<CGContextHostObject>(context).font;
    if font.is_null() {
        log!("Warning: CGContextShowGlyphsAtPoint called with no font set");
        return;
    }
    if !super::cg_font::is_data_provider_font(env, font) {
        log!(
            "TODO: CGContextShowGlyphsAtPoint with non-data-provider font {:?}, skipping",
            font
        );
        return;
    }

    let mut glyph_ids = Vec::with_capacity(count as usize);
    for i in 0..count {
        let glyph_id: CGGlyph = env.mem.read(glyphs + i);
        glyph_ids.push(rusttype::GlyphId(glyph_id));
    }

    let host = env.objc.borrow::<CGContextHostObject>(context);
    let font_size = host.font_size;
    let text_transform = host
        .text_transform
        .unwrap_or(CGAffineTransformIdentity);

    let mut drawer = CGBitmapContextDrawer::new(&env.objc, &mut env.mem, context);
    let fill_color = drawer.rgb_fill_color();

    // Borrow the actual rusttype Font for rendering. We hold a separate
    // borrow on env.objc here because `drawer` has already taken a borrow
    // on env.mem and on the bitmap context's host object, but it does not
    // need the font host object.
    let rusttype_font = &env.objc.borrow::<CGFontHostObject>(font).font;
    rusttype_font.draw_glyphs(
        font_size,
        glyph_ids,
        (x, y),
        text_transform,
        |raster_glyph| {
            uikit::ui_font::draw_font_glyph(
                &mut drawer,
                raster_glyph,
                fill_color,
                /* clip_x: */ None,
                /* clip_y: */ None,
            )
        },
    );
}

/// `void CGContextShowGlyphsAtPositions(CGContextRef c,
///     const CGGlyph *glyphs, const CGPoint *positions, size_t count)`
///
/// Draws glyphs at specified positions relative to the text position.
/// Each position gives the (x, y) offset for the corresponding glyph.
fn CGContextShowGlyphsAtPositions(
    env: &mut Environment,
    context: CGContextRef,
    glyphs: ConstPtr<CGGlyph>,
    positions: ConstPtr<CGPoint>,
    count: GuestUSize,
) {
    if context.is_null() {
        return;
    }
    let font = env.objc.borrow::<CGContextHostObject>(context).font;
    if font.is_null() {
        log!("Warning: CGContextShowGlyphsAtPositions called with no font set");
        return;
    }
    if !super::cg_font::is_data_provider_font(env, font) {
        log!(
            "TODO: CGContextShowGlyphsAtPositions with non-data-provider font {:?}, skipping",
            font
        );
        return;
    }

    let mut glyph_ids = Vec::with_capacity(count as usize);
    let mut pos_vec = Vec::with_capacity(count as usize);
    for i in 0..count {
        let glyph_id: CGGlyph = env.mem.read(glyphs + i);
        glyph_ids.push(rusttype::GlyphId(glyph_id));
        let pos: CGPoint = env.mem.read(positions + i);
        pos_vec.push((pos.x, pos.y));
    }

    let font_size = env.objc.borrow::<CGContextHostObject>(context).font_size;

    let mut drawer = CGBitmapContextDrawer::new(&env.objc, &mut env.mem, context);
    let fill_color = drawer.rgb_fill_color();

    let rusttype_font = &env.objc.borrow::<CGFontHostObject>(font).font;
    rusttype_font.draw_glyphs_at_positions(
        font_size,
        &glyph_ids,
        &pos_vec,
        (0.0, 0.0),
        |raster_glyph| {
            uikit::ui_font::draw_font_glyph(
                &mut drawer,
                raster_glyph,
                fill_color,
                /* clip_x: */ None,
                /* clip_y: */ None,
            )
        },
    );
}

/// `void CGContextShowGlyphsWithAdvances(CGContextRef c,
///     const CGGlyph *glyphs, const CGSize *advances, size_t count)`
///
/// Draws glyphs with explicit advance widths/heights between each glyph.
fn CGContextShowGlyphsWithAdvances(
    env: &mut Environment,
    context: CGContextRef,
    glyphs: ConstPtr<CGGlyph>,
    advances: ConstPtr<super::CGSize>,
    count: GuestUSize,
) {
    if context.is_null() {
        return;
    }
    let font = env.objc.borrow::<CGContextHostObject>(context).font;
    if font.is_null() {
        log!("Warning: CGContextShowGlyphsWithAdvances called with no font set");
        return;
    }
    if !super::cg_font::is_data_provider_font(env, font) {
        log!(
            "TODO: CGContextShowGlyphsWithAdvances with non-data-provider font {:?}, skipping",
            font
        );
        return;
    }

    let mut glyph_ids = Vec::with_capacity(count as usize);
    let mut advance_vec = Vec::with_capacity(count as usize);
    for i in 0..count {
        let glyph_id: CGGlyph = env.mem.read(glyphs + i);
        glyph_ids.push(rusttype::GlyphId(glyph_id));
        let adv: super::CGSize = env.mem.read(advances + i);
        advance_vec.push((adv.width, adv.height));
    }

    let font_size = env.objc.borrow::<CGContextHostObject>(context).font_size;

    let mut drawer = CGBitmapContextDrawer::new(&env.objc, &mut env.mem, context);
    let fill_color = drawer.rgb_fill_color();

    let rusttype_font = &env.objc.borrow::<CGFontHostObject>(font).font;
    rusttype_font.draw_glyphs_with_advances(
        font_size,
        &glyph_ids,
        &advance_vec,
        (0.0, 0.0),
        |raster_glyph| {
            uikit::ui_font::draw_font_glyph(
                &mut drawer,
                raster_glyph,
                fill_color,
                /* clip_x: */ None,
                /* clip_y: */ None,
            )
        },
    );
}

/// `void CGContextShowGlyphs(CGContextRef c, const CGGlyph *glyphs, size_t count)`
///
/// Draws glyphs at the current text position. Since we don't track text
/// position fully yet, we draw at (0, 0).
fn CGContextShowGlyphs(
    env: &mut Environment,
    context: CGContextRef,
    glyphs: ConstPtr<CGGlyph>,
    count: GuestUSize,
) {
    CGContextShowGlyphsAtPoint(env, context, 0.0, 0.0, glyphs, count);
}


/// `void CGContextSetAllowsFontSubpixelPositioning(CGContextRef c, bool allow)`
///
/// Controls whether subpixel font positioning is used.  On the emulated
/// screen there is no physical sub-pixel grid to exploit, so this is a
/// no-op.  Exporting the symbol eliminates the "unimplemented function"
/// warning produced by apps that call it unconditionally.
///
/// Reference: <https://developer.apple.com/documentation/coregraphics/1454839-cgcontextsetallowsfontsubpixelpo>
fn CGContextSetAllowsFontSubpixelPositioning(
    _env: &mut Environment,
    _context: CGContextRef,
    _allows: bool,
) {
}

/// `void CGContextSetShouldSubpixelQuantizeFonts(CGContextRef c, bool should)`
///
/// Controls whether font glyph metrics are quantised to sub-pixel
/// boundaries.  No-op for the same reasons as
/// `CGContextSetAllowsFontSubpixelPositioning`.
///
/// Reference: <https://developer.apple.com/documentation/coregraphics/1455671-cgcontextsetshouldsub pixelquanti>
fn CGContextSetShouldSubpixelQuantizeFonts(
    _env: &mut Environment,
    _context: CGContextRef,
    _should: bool,
) {
}

pub const FUNCTIONS: FunctionExports = &[
    export_c_func!(CGContextRetain(_)),
    export_c_func!(CGContextRelease(_)),
    export_c_func!(CGContextSetFillColorWithColor(_, _)),
    export_c_func!(CGContextSetRGBFillColor(_, _, _, _, _)),
    export_c_func!(CGContextSetRGBStrokeColor(_, _, _, _, _)),
    export_c_func!(CGContextSetGrayFillColor(_, _, _)),
    export_c_func!(CGContextFillRect(_, _)),
    export_c_func!(CGContextClearRect(_, _)),
    export_c_func!(CGContextClipToRect(_, _)),
    export_c_func!(CGContextConcatCTM(_, _)),
    export_c_func!(CGContextGetCTM(_)),
    export_c_func!(CGContextRotateCTM(_, _)),
    export_c_func!(CGContextScaleCTM(_, _, _)),
    export_c_func!(CGContextTranslateCTM(_, _, _)),
    export_c_func!(CGContextDrawImage(_, _, _)),
    export_c_func!(CGContextDrawTiledImage(_, _, _)),
    export_c_func!(CGContextSaveGState(_)),
    export_c_func!(CGContextRestoreGState(_)),
    export_c_func!(CGContextSetInterpolationQuality(_, _)),
    export_c_func!(CGContextGetTextPosition(_)),
    export_c_func!(CGContextSetTextPosition(_, _, _)),
    export_c_func!(CGContextSetTextDrawingMode(_, _)),
    export_c_func!(CGContextSetCharacterSpacing(_, _)),
    export_c_func!(CGContextSetTextMatrix(_, _)),
    export_c_func!(CGContextSelectFont(_, _, _, _)),
    export_c_func!(CGContextShowTextAtPoint(_, _, _, _, _)),
    export_c_func!(CGContextShowText(_, _, _)),
    export_c_func!(CGContextSetFontSize(_, _)),
    export_c_func!(CGContextSetFont(_, _)),
    export_c_func!(CGContextShowGlyphsAtPoint(_, _, _, _, _)),
    export_c_func!(CGContextShowGlyphsAtPositions(_, _, _, _)),
    export_c_func!(CGContextShowGlyphsWithAdvances(_, _, _, _)),
    export_c_func!(CGContextShowGlyphs(_, _, _)),
    // Add to FUNCTIONS:
    export_c_func!(CGContextSetStrokeColorWithColor(_, _)),
    export_c_func!(CGContextSetGrayStrokeColor(_, _, _)),
    export_c_func!(CGContextSetAlpha(_, _)),
    export_c_func!(CGContextSetLineWidth(_, _)),
    export_c_func!(CGContextSetLineCap(_, _)),
    export_c_func!(CGContextSetLineJoin(_, _)),
    export_c_func!(CGContextSetMiterLimit(_, _)),
    export_c_func!(CGContextSetLineDash(_, _, _, _)),
    export_c_func!(CGContextSetFlatness(_, _)),
    export_c_func!(CGContextStrokeRect(_, _)),
    export_c_func!(CGContextStrokeRectWithWidth(_, _, _)),
    export_c_func!(CGContextStrokeEllipseInRect(_, _)),
    export_c_func!(CGContextFillEllipseInRect(_, _)),
    export_c_func!(CGContextAddRect(_, _)),
    export_c_func!(CGContextAddRects(_, _, _)),
    export_c_func!(CGContextAddEllipseInRect(_, _)),
    export_c_func!(CGContextAddArc(_, _, _, _, _, _, _)),
    export_c_func!(CGContextAddArcToPoint(_, _, _, _, _, _)),
    export_c_func!(CGContextAddLineToPoint(_, _, _)),
    export_c_func!(CGContextAddLines(_, _, _)),
    export_c_func!(CGContextMoveToPoint(_, _, _)),
    export_c_func!(CGContextAddCurveToPoint(_, _, _, _, _, _, _)),
    export_c_func!(CGContextAddQuadCurveToPoint(_, _, _, _, _)),
    export_c_func!(CGContextClosePath(_)),
    export_c_func!(CGContextBeginPath(_)),
    export_c_func!(CGContextDrawPath(_, _)),
    export_c_func!(CGContextFillPath(_)),
    export_c_func!(CGContextEOFillPath(_)),
    export_c_func!(CGContextStrokePath(_)),
    export_c_func!(CGContextSetShouldAntialias(_, _)),
    export_c_func!(CGContextSetAllowsAntialiasing(_, _)),
    export_c_func!(CGContextSetShouldSmoothFonts(_, _)),
    export_c_func!(CGContextFlush(_)),
    export_c_func!(CGContextSynchronize(_)),
    export_c_func!(CGContextGetClipBoundingBox(_)),
    export_c_func!(CGContextResetClip(_)),
    export_c_func!(CGContextClipToMask(_, _, _)),
    export_c_func!(CGContextSetBlendMode(_, _)),
    export_c_func!(CGContextSetShadow(_, _, _)),
    export_c_func!(CGContextSetShadowWithColor(_, _, _, _)),
    export_c_func!(CGContextSetFillColorSpace(_, _)),
    export_c_func!(CGContextSetStrokeColorSpace(_, _)),
    export_c_func!(CGContextSetRenderingIntent(_, _)),
    export_c_func!(CGContextDrawLinearGradient(_, _, _, _, _)),
    export_c_func!(CGContextSetAllowsFontSubpixelPositioning(_, _)),
    export_c_func!(CGContextSetShouldSubpixelQuantizeFonts(_, _)),
];
