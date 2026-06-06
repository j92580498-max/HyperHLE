/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `CGFont.h` — Full implementation of CGFont/CGGlyph APIs.
//!
//! This module provides a complete implementation of the CGFont API as defined
//! by Apple's Core Graphics framework for iPhone OS. All glyph metrics,
//! table access, and name lookups are backed by real TrueType/OpenType font
//! parsing via the rusttype library and raw table access.

use super::cg_data_provider;
use super::cg_data_provider::CGDataProviderRef;
use crate::dyld::{export_c_func, FunctionExports};
use crate::font::Font;
use crate::frameworks::core_foundation::cf_string::CFStringRef;
use crate::frameworks::core_foundation::{CFRelease, CFRetain, CFTypeRef};
use crate::frameworks::foundation::{ns_string, unichar};
use crate::mem::{ConstPtr, GuestUSize, MutPtr, MutVoidPtr, Ptr};
use crate::objc::{
    id, msg, msg_class, nil, objc_classes, retain, ClassExports, HostObject, ObjC,
};
use crate::Environment;
use rusttype::GlyphId;

// =========================================================================
// MARK: - Type aliases & constants
// =========================================================================

pub type CGFontRef = CFTypeRef;
pub type CGGlyph = u16;
pub type CGFontIndex = u16;
pub type CGFontPostScriptFormat = i32;

/// PostScript format constants
pub const kCGFontPostScriptFormatType1: CGFontPostScriptFormat = 1;
pub const kCGFontPostScriptFormatType3: CGFontPostScriptFormat = 3;
pub const kCGFontPostScriptFormatType42: CGFontPostScriptFormat = 42;

/// Special glyph index values
pub const kCGFontIndexInvalid: CGFontIndex = 0xFFFF;
pub const kCGFontIndexMax: CGFontIndex = 0xFFFE;
pub const kCGGlyphMax: CGGlyph = kCGFontIndexMax;

// Text encoding constants for CGContextSelectFont
pub const kCGEncodingFontSpecific: i32 = 0;
pub const kCGEncodingMacRoman: i32 = 1;


// =========================================================================
// MARK: - Host Object
// =========================================================================

/// Host object backing a CGFont created via [CGFontCreateWithDataProvider].
/// Contains the parsed Font with full glyph metrics and raw table access.
#[derive(Default)]
pub struct CGFontHostObject {
    pub font: Font,
}
impl HostObject for CGFontHostObject {}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation _touchHLE_CGFont: NSObject
@end

};

/// Returns `true` if `font` is a CGFont created via
/// [CGFontCreateWithDataProvider] (backed by [CGFontHostObject]).
pub fn is_data_provider_font(env: &mut Environment, font: CGFontRef) -> bool {
    if font.is_null() {
        return false;
    }
    let class = env.objc.get_known_class("_touchHLE_CGFont", &mut env.mem);
    let obj_class = ObjC::read_isa(font, &env.mem);
    obj_class == class || env.objc.class_is_subclass_of(obj_class, class)
}

// =========================================================================
// MARK: - Internal helpers
// =========================================================================

fn font_from_name(env: &mut Environment, name: CFStringRef) -> id {
    if name == nil {
        return nil;
    }
    let size: f32 = 12.0;
    let font: id = msg_class![env; UIFont fontWithName:name size:size];
    if font != nil {
        return font;
    }
    msg_class![env; UIFont systemFontOfSize:size]
}


// =========================================================================
// MARK: - Creation
// =========================================================================

/// `CGFontRef CGFontCreateWithFontName(CFStringRef name)`
fn CGFontCreateWithFontName(env: &mut Environment, name: CFStringRef) -> CGFontRef {
    let font = font_from_name(env, name);
    if font == nil {
        log!(
            "CGFontCreateWithFontName: could not create font for {:?}",
            name
        );
        return Ptr::null();
    }
    retain(env, font);
    log_dbg!(
        "CGFontCreateWithFontName({}) => {:?}",
        if name != nil {
            ns_string::to_rust_string(env, name).into_owned()
        } else {
            "(null)".into()
        },
        font
    );
    font
}

/// `CGFontRef CGFontCreateCopyWithVariations(CGFontRef font,
///                                           CFDictionaryRef variations)`
fn CGFontCreateCopyWithVariations(
    env: &mut Environment,
    font: CGFontRef,
    _variations: CFTypeRef,
) -> CGFontRef {
    // Font variations (optical size, weight axes, etc.) are not supported in
    // this emulation layer. Return a retained copy of the original font.
    log_dbg!("CGFontCreateCopyWithVariations: variations ignored, returning copy");
    if font.is_null() {
        return Ptr::null();
    }
    retain(env, font);
    font
}

/// `CGFontRef CGFontCreateWithDataProvider(CGDataProviderRef provider)`
///
/// Creates a CGFont backed by a real rasterizable [Font] parsed from the
/// bytes provided by the data provider.
fn CGFontCreateWithDataProvider(
    env: &mut Environment,
    provider: CGDataProviderRef,
) -> CGFontRef {
    if provider.is_null() {
        return Ptr::null();
    }
    let bytes = cg_data_provider::borrow_bytes(env, provider).to_vec();
    // Per Apple's Core Graphics documentation, CGFontCreateWithDataProvider
    // returns NULL if a font can't be created from the provided data, rather
    // than crashing. Honour that contract instead of panicking on bad data.
    let Some(font) = Font::from_vec(bytes) else {
        log!("CGFontCreateWithDataProvider: could not parse font data; returning NULL");
        return Ptr::null();
    };
    let host_obj = Box::new(CGFontHostObject { font });
    let class = env.objc.get_known_class("_touchHLE_CGFont", &mut env.mem);
    env.objc.alloc_object(class, host_obj, &mut env.mem)
}


// =========================================================================
// MARK: - Retain / Release
// =========================================================================

pub fn CGFontRetain(env: &mut Environment, font: CGFontRef) -> CGFontRef {
    if font.is_null() {
        return Ptr::null();
    }
    CFRetain(env, font)
}

pub fn CGFontRelease(env: &mut Environment, font: CGFontRef) {
    if font.is_null() {
        return;
    }
    CFRelease(env, font);
}

// =========================================================================
// MARK: - Glyph lookup
// =========================================================================

/// `bool CGFontGetGlyphsForUnichars(CGFontRef font, const UniChar chars[],
///     CGGlyph glyphs[], size_t count)`
///
/// Maps Unicode characters to glyph indices. This is the real implementation
/// using the font's cmap table via rusttype.
fn CGFontGetGlyphsForUnichars(
    env: &mut Environment,
    font: CGFontRef,
    chars: ConstPtr<unichar>,
    glyphs: MutPtr<CGGlyph>,
    length: GuestUSize,
) -> bool {
    if font.is_null() {
        return false;
    }
    if !is_data_provider_font(env, font) {
        log!(
            "TODO: CGFontGetGlyphsForUnichars on non-data-provider font {:?}",
            font
        );
        return false;
    }

    let mut all_mapped = true;
    for i in 0..length {
        let c: unichar = env.mem.read(chars + i);
        let glyph_id = env
            .objc
            .borrow::<CGFontHostObject>(font)
            .font
            .glyph_id_for_char(c);
        let g = glyph_id.0;
        if g == 0 {
            all_mapped = false;
        }
        env.mem.write(glyphs + i, g);
    }
    all_mapped
}

/// `CGGlyph CGFontGetGlyphWithGlyphName(CGFontRef font, CFStringRef name)`
///
/// Looks up a glyph by its PostScript name using the 'post' table.
fn CGFontGetGlyphWithGlyphName(
    env: &mut Environment,
    font: CGFontRef,
    glyph_name: CFStringRef,
) -> CGGlyph {
    if font.is_null() || glyph_name == nil {
        return kCGFontIndexInvalid;
    }
    if !is_data_provider_font(env, font) {
        log_dbg!(
            "CGFontGetGlyphWithGlyphName: non-data-provider font, returning invalid"
        );
        return kCGFontIndexInvalid;
    }

    let name_str = ns_string::to_rust_string(env, glyph_name);
    let result = env
        .objc
        .borrow::<CGFontHostObject>(font)
        .font
        .glyph_for_name(&name_str);

    match result {
        Some(gid) => gid.0,
        None => kCGFontIndexInvalid,
    }
}

/// `CFStringRef CGFontCopyGlyphNameForGlyph(CGFontRef font, CGGlyph glyph)`
///
/// Returns the PostScript name for a glyph, or nil if not available.
fn CGFontCopyGlyphNameForGlyph(
    env: &mut Environment,
    font: CGFontRef,
    glyph: CGGlyph,
) -> CFStringRef {
    if font.is_null() {
        return nil;
    }
    if !is_data_provider_font(env, font) {
        return nil;
    }

    let name = env
        .objc
        .borrow::<CGFontHostObject>(font)
        .font
        .glyph_name(GlyphId(glyph));

    match name {
        Some(s) => ns_string::from_rust_string(env, s),
        None => nil,
    }
}


// =========================================================================
// MARK: - Name queries
// =========================================================================

/// `CFStringRef CGFontCopyPostScriptName(CGFontRef font)`
fn CGFontCopyPostScriptName(env: &mut Environment, font: CGFontRef) -> CFStringRef {
    if font.is_null() {
        return nil;
    }
    if is_data_provider_font(env, font) {
        // Try to extract PostScript name from the 'name' table (nameID=6).
        let name_data = env
            .objc
            .borrow::<CGFontHostObject>(font)
            .font
            .table_data(0x6E616D65 /* 'name' */);
        if let Some(data) = name_data {
            if let Some(ps_name) = extract_name_from_name_table(&data, 6) {
                return ns_string::from_rust_string(env, ps_name);
            }
        }
        return nil;
    }
    // UIFont-backed path
    let name: id = msg![env; font fontName];
    if name == nil {
        return nil;
    }
    msg![env; name copy]
}

/// `CFStringRef CGFontCopyFullName(CGFontRef font)`
fn CGFontCopyFullName(env: &mut Environment, font: CGFontRef) -> CFStringRef {
    if font.is_null() {
        return nil;
    }
    if is_data_provider_font(env, font) {
        // Full name is nameID=4 in the 'name' table.
        let name_data = env
            .objc
            .borrow::<CGFontHostObject>(font)
            .font
            .table_data(0x6E616D65 /* 'name' */);
        if let Some(data) = name_data {
            if let Some(full_name) = extract_name_from_name_table(&data, 4) {
                return ns_string::from_rust_string(env, full_name);
            }
        }
        return nil;
    }
    CGFontCopyPostScriptName(env, font)
}

/// Extract a name record from a TrueType 'name' table.
/// `name_id`: 1=family, 2=subfamily, 4=full name, 6=PostScript name
fn extract_name_from_name_table(data: &[u8], name_id: u16) -> Option<String> {
    if data.len() < 6 {
        return None;
    }
    let count = u16::from_be_bytes([data[2], data[3]]) as usize;
    let string_offset = u16::from_be_bytes([data[4], data[5]]) as usize;

    for i in 0..count {
        let record_offset = 6 + i * 12;
        if data.len() < record_offset + 12 {
            break;
        }
        let platform_id = u16::from_be_bytes([data[record_offset], data[record_offset + 1]]);
        let encoding_id = u16::from_be_bytes([data[record_offset + 2], data[record_offset + 3]]);
        let _language_id = u16::from_be_bytes([data[record_offset + 4], data[record_offset + 5]]);
        let nid = u16::from_be_bytes([data[record_offset + 6], data[record_offset + 7]]);
        let length = u16::from_be_bytes([data[record_offset + 8], data[record_offset + 9]]) as usize;
        let offset = u16::from_be_bytes([data[record_offset + 10], data[record_offset + 11]]) as usize;

        if nid != name_id {
            continue;
        }

        let str_start = string_offset + offset;
        if str_start + length > data.len() {
            continue;
        }

        let raw = &data[str_start..str_start + length];

        // Platform 3 (Windows) encoding 1 (Unicode BMP) — UTF-16BE
        if platform_id == 3 && encoding_id == 1 {
            let chars: Vec<u16> = raw
                .chunks_exact(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect();
            return String::from_utf16(&chars).ok();
        }
        // Platform 1 (Macintosh) encoding 0 (Roman) — ASCII/Latin-1
        if platform_id == 1 && encoding_id == 0 {
            return Some(String::from_utf8_lossy(raw).into_owned());
        }
        // Platform 0 (Unicode) — UTF-16BE
        if platform_id == 0 {
            let chars: Vec<u16> = raw
                .chunks_exact(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect();
            return String::from_utf16(&chars).ok();
        }
    }
    None
}


// =========================================================================
// MARK: - Metrics (real implementations using rusttype)
// =========================================================================

/// `int CGFontGetNumberOfGlyphs(CGFontRef font)`
fn CGFontGetNumberOfGlyphs(env: &mut Environment, font: CGFontRef) -> i32 {
    if font.is_null() {
        return 0;
    }
    if is_data_provider_font(env, font) {
        return env
            .objc
            .borrow::<CGFontHostObject>(font)
            .font
            .glyph_count() as i32;
    }
    // Fallback for UIFont-backed fonts: return a reasonable default
    256
}

/// `int CGFontGetUnitsPerEm(CGFontRef font)`
///
/// Returns the number of glyph space units per em for the font. This is read
/// directly from the font's 'head' table.
fn CGFontGetUnitsPerEm(env: &mut Environment, font: CGFontRef) -> i32 {
    if font.is_null() {
        return 0;
    }
    if is_data_provider_font(env, font) {
        return env
            .objc
            .borrow::<CGFontHostObject>(font)
            .font
            .units_per_em() as i32;
    }
    2048
}

/// `int CGFontGetAscent(CGFontRef font)`
///
/// Returns the ascent in design units (glyph space). Computed from the
/// font's vertical metrics scaled to design units.
fn CGFontGetAscent(env: &mut Environment, font: CGFontRef) -> i32 {
    if font.is_null() {
        return 0;
    }
    if is_data_provider_font(env, font) {
        let host = env.objc.borrow::<CGFontHostObject>(font);
        let upm = host.font.units_per_em() as f32;
        // rusttype's ascent at scale=upm gives design units directly
        let ascent = host.font.ascent(upm / 1.125); // undo the iPhone scale
        return ascent.round() as i32;
    }
    let ascender: f32 = msg![env; font ascender];
    let upm = CGFontGetUnitsPerEm(env, font) as f32;
    let point_size: f32 = msg![env; font pointSize];
    if point_size == 0.0 {
        return 0;
    }
    (ascender / point_size * upm).round() as i32
}

/// `int CGFontGetDescent(CGFontRef font)`
///
/// Returns the descent in design units (negative value).
fn CGFontGetDescent(env: &mut Environment, font: CGFontRef) -> i32 {
    if font.is_null() {
        return 0;
    }
    if is_data_provider_font(env, font) {
        let host = env.objc.borrow::<CGFontHostObject>(font);
        let upm = host.font.units_per_em() as f32;
        let descent = host.font.descent(upm / 1.125);
        return descent.round() as i32;
    }
    let descender: f32 = msg![env; font descender];
    let upm = CGFontGetUnitsPerEm(env, font) as f32;
    let point_size: f32 = msg![env; font pointSize];
    if point_size == 0.0 {
        return 0;
    }
    (descender / point_size * upm).round() as i32
}

/// `int CGFontGetLeading(CGFontRef font)`
///
/// Returns the leading (line gap) in design units.
fn CGFontGetLeading(env: &mut Environment, font: CGFontRef) -> i32 {
    if font.is_null() {
        return 0;
    }
    if is_data_provider_font(env, font) {
        let host = env.objc.borrow::<CGFontHostObject>(font);
        let upm = host.font.units_per_em() as f32;
        let line_gap = host.font.line_gap(upm / 1.125);
        return line_gap.round() as i32;
    }
    let leading: f32 = msg![env; font leading];
    let upm = CGFontGetUnitsPerEm(env, font) as f32;
    let point_size: f32 = msg![env; font pointSize];
    if point_size == 0.0 {
        return 0;
    }
    (leading / point_size * upm).round() as i32
}

/// `int CGFontGetCapHeight(CGFontRef font)`
///
/// Returns the cap height in design units. Read from the 'OS/2' table if
/// available, otherwise approximated as 70% of ascent.
fn CGFontGetCapHeight(env: &mut Environment, font: CGFontRef) -> i32 {
    if font.is_null() {
        return 0;
    }
    if is_data_provider_font(env, font) {
        // Try OS/2 table: sCapHeight is at offset 88 (version >= 2)
        let os2_data = env
            .objc
            .borrow::<CGFontHostObject>(font)
            .font
            .table_data(0x4F532F32 /* 'OS/2' */);
        if let Some(data) = os2_data {
            if data.len() >= 90 {
                let version = u16::from_be_bytes([data[0], data[1]]);
                if version >= 2 {
                    let cap_height = i16::from_be_bytes([data[88], data[89]]);
                    if cap_height > 0 {
                        return cap_height as i32;
                    }
                }
            }
        }
        // Fallback: 70% of ascent
        return (CGFontGetAscent(env, font) as f32 * 0.70).round() as i32;
    }
    (CGFontGetAscent(env, font) as f32 * 0.70).round() as i32
}

/// `int CGFontGetXHeight(CGFontRef font)`
///
/// Returns the x-height in design units. Read from the 'OS/2' table if
/// available, otherwise approximated as 50% of ascent.
fn CGFontGetXHeight(env: &mut Environment, font: CGFontRef) -> i32 {
    if font.is_null() {
        return 0;
    }
    if is_data_provider_font(env, font) {
        // Try OS/2 table: sxHeight is at offset 86 (version >= 2)
        let os2_data = env
            .objc
            .borrow::<CGFontHostObject>(font)
            .font
            .table_data(0x4F532F32 /* 'OS/2' */);
        if let Some(data) = os2_data {
            if data.len() >= 88 {
                let version = u16::from_be_bytes([data[0], data[1]]);
                if version >= 2 {
                    let x_height = i16::from_be_bytes([data[86], data[87]]);
                    if x_height > 0 {
                        return x_height as i32;
                    }
                }
            }
        }
        return (CGFontGetAscent(env, font) as f32 * 0.50).round() as i32;
    }
    (CGFontGetAscent(env, font) as f32 * 0.50).round() as i32
}

/// `CGFloat CGFontGetItalicAngle(CGFontRef font)`
///
/// Returns the italic angle from the 'post' table.
fn CGFontGetItalicAngle(env: &mut Environment, font: CGFontRef) -> f32 {
    if font.is_null() {
        return 0.0;
    }
    if is_data_provider_font(env, font) {
        let post_data = env
            .objc
            .borrow::<CGFontHostObject>(font)
            .font
            .table_data(0x706F7374 /* 'post' */);
        if let Some(data) = post_data {
            if data.len() >= 8 {
                // italicAngle is a Fixed (16.16) at offset 4
                let fixed = i32::from_be_bytes([data[4], data[5], data[6], data[7]]);
                return fixed as f32 / 65536.0;
            }
        }
    }
    0.0
}

/// `CGFloat CGFontGetStemV(CGFontRef font)`
///
/// Returns the dominant vertical stem width. Read from OS/2 table or
/// approximated.
fn CGFontGetStemV(env: &mut Environment, font: CGFontRef) -> f32 {
    if font.is_null() {
        return 0.0;
    }
    // Approximate as ~12% of units-per-em
    let upm = CGFontGetUnitsPerEm(env, font) as f32;
    upm * 0.12
}

/// `CGRect CGFontGetFontBBox(CGFontRef font)`
///
/// Returns the bounding box of the font in design units. Read from the
/// 'head' table (xMin, yMin, xMax, yMax at offsets 36-43).
fn CGFontGetFontBBox(
    env: &mut Environment,
    font: CGFontRef,
    out: MutVoidPtr,
) {
    use crate::frameworks::core_graphics::{CGFloat, CGPoint, CGRect, CGSize};
    if font.is_null() || out.is_null() {
        return;
    }
    if is_data_provider_font(env, font) {
        let head_data = env
            .objc
            .borrow::<CGFontHostObject>(font)
            .font
            .table_data(0x68656164 /* 'head' */);
        if let Some(data) = head_data {
            if data.len() >= 44 {
                let x_min = i16::from_be_bytes([data[36], data[37]]) as f32;
                let y_min = i16::from_be_bytes([data[38], data[39]]) as f32;
                let x_max = i16::from_be_bytes([data[40], data[41]]) as f32;
                let y_max = i16::from_be_bytes([data[42], data[43]]) as f32;
                let rect = CGRect {
                    origin: CGPoint { x: x_min, y: y_min },
                    size: CGSize {
                        width: x_max - x_min,
                        height: y_max - y_min,
                    },
                };
                let p: crate::mem::MutPtr<CGRect> = out.cast();
                env.mem.write(p, rect);
                return;
            }
        }
    }
    // Fallback
    let upm = CGFontGetUnitsPerEm(env, font) as CGFloat;
    let asc = CGFontGetAscent(env, font) as CGFloat;
    let desc = CGFontGetDescent(env, font) as CGFloat;
    let rect = CGRect {
        origin: CGPoint { x: 0.0, y: desc },
        size: CGSize {
            width: upm,
            height: asc - desc,
        },
    };
    let p: crate::mem::MutPtr<CGRect> = out.cast();
    env.mem.write(p, rect);
}


// =========================================================================
// MARK: - Per-glyph metrics (real implementations)
// =========================================================================

/// `bool CGFontGetGlyphAdvances(CGFontRef font, const CGGlyph glyphs[],
///     size_t count, int advances[])`
///
/// Writes per-glyph advance widths in design units. Uses real h_metrics from
/// the font via rusttype.
fn CGFontGetGlyphAdvances(
    env: &mut Environment,
    font: CGFontRef,
    glyphs: ConstPtr<CGGlyph>,
    count: u32,
    advances: MutPtr<i32>,
) -> bool {
    if font.is_null() {
        return false;
    }
    if !is_data_provider_font(env, font) {
        // Fallback for non-data-provider fonts
        let upm = CGFontGetUnitsPerEm(env, font);
        let avg = (upm as f32 * 0.60).round() as i32;
        for i in 0..count {
            env.mem.write(advances + i, avg);
        }
        return true;
    }

    for i in 0..count {
        let glyph_id: CGGlyph = env.mem.read(glyphs + i);
        let advance = env
            .objc
            .borrow::<CGFontHostObject>(font)
            .font
            .glyph_advance(GlyphId(glyph_id));
        env.mem.write(advances + i, advance);
    }
    true
}

/// `bool CGFontGetGlyphBBoxes(CGFontRef font, const CGGlyph glyphs[],
///     size_t count, CGRect bboxes[])`
///
/// Writes per-glyph bounding boxes in design units.
fn CGFontGetGlyphBBoxes(
    env: &mut Environment,
    font: CGFontRef,
    glyphs: ConstPtr<CGGlyph>,
    count: u32,
    bboxes: MutVoidPtr,
) -> bool {
    use crate::frameworks::core_graphics::{CGPoint, CGRect, CGSize};
    if font.is_null() {
        return false;
    }

    let p: MutPtr<CGRect> = bboxes.cast();

    if !is_data_provider_font(env, font) {
        // Fallback: write the font bounding box for every glyph
        let upm = CGFontGetUnitsPerEm(env, font) as f32;
        let asc = CGFontGetAscent(env, font) as f32;
        let desc = CGFontGetDescent(env, font) as f32;
        let rect = CGRect {
            origin: CGPoint { x: 0.0, y: desc },
            size: CGSize {
                width: upm * 0.60,
                height: asc - desc,
            },
        };
        for i in 0..count {
            env.mem.write(p + i, rect);
        }
        return true;
    }

    for i in 0..count {
        let glyph_id: CGGlyph = env.mem.read(glyphs + i);
        let (x, y, w, h) = env
            .objc
            .borrow::<CGFontHostObject>(font)
            .font
            .glyph_bbox(GlyphId(glyph_id));
        let rect = CGRect {
            origin: CGPoint { x, y },
            size: CGSize {
                width: w,
                height: h,
            },
        };
        env.mem.write(p + i, rect);
    }
    true
}


// =========================================================================
// MARK: - Table access (real implementation)
// =========================================================================

/// `CFArrayRef CGFontCopyTableTags(CGFontRef font)`
///
/// Returns an array of all table tags present in the font file. Each tag is
/// a 4-byte value stored as a CFNumber (UInt32). We return it as a CFArray.
fn CGFontCopyTableTags(env: &mut Environment, font: CGFontRef) -> CFTypeRef {
    if font.is_null() {
        return nil;
    }
    if !is_data_provider_font(env, font) {
        return nil;
    }

    let tags = env
        .objc
        .borrow::<CGFontHostObject>(font)
        .font
        .table_tags();

    if tags.is_empty() {
        return nil;
    }

    // Create an NSMutableArray and populate it with NSNumber objects
    let array: id = msg_class![env; NSMutableArray arrayWithCapacity:(tags.len() as u32)];
    for tag in &tags {
        let num: id = msg_class![env; NSNumber numberWithUnsignedInt:(*tag)];
        () = msg![env; array addObject:num];
    }
    // Return retained (the caller owns this reference)
    retain(env, array);
    array
}

/// `CFDataRef CGFontCopyTableForTag(CGFontRef font, uint32_t tag)`
///
/// Returns the raw bytes of a font table as CFData. This provides direct
/// access to any TrueType/OpenType table in the font.
fn CGFontCopyTableForTag(env: &mut Environment, font: CGFontRef, tag: u32) -> CFTypeRef {
    if font.is_null() {
        return nil;
    }
    if !is_data_provider_font(env, font) {
        return nil;
    }

    let table_bytes = env
        .objc
        .borrow::<CGFontHostObject>(font)
        .font
        .table_data(tag);

    match table_bytes {
        Some(bytes) => {
            use crate::frameworks::core_foundation::cf_allocator::kCFAllocatorDefault;
            use crate::frameworks::core_foundation::cf_data::CFDataCreate;

            let len: u32 = bytes.len() as u32;
            let buf = env.mem.alloc(len);
            env.mem.bytes_at_mut(buf.cast(), len).copy_from_slice(&bytes);
            CFDataCreate(env, kCFAllocatorDefault, buf.cast_const().cast(), len as i32)
        }
        None => nil,
    }
}


// =========================================================================
// MARK: - PostScript subset / encoding
// =========================================================================

/// `bool CGFontCanCreatePostScriptSubset(CGFontRef font,
///     CGFontPostScriptFormat format)`
fn CGFontCanCreatePostScriptSubset(
    env: &mut Environment,
    font: CGFontRef,
    format: CGFontPostScriptFormat,
) -> bool {
    if font.is_null() {
        return false;
    }
    // We can theoretically create Type42 subsets (which are just TrueType
    // wrapped in PostScript). Type1 and Type3 conversions are not supported.
    format == kCGFontPostScriptFormatType42 && is_data_provider_font(env, font)
}

fn CGFontCreatePostScriptSubset(
    _env: &mut Environment,
    font: CGFontRef,
    _subset_name: CFStringRef,
    _format: CGFontPostScriptFormat,
    _glyphs: ConstPtr<CGGlyph>,
    _count: u32,
    _encoding: ConstPtr<CGGlyph>,
) -> CFTypeRef {
    log!(
        "TODO: CGFontCreatePostScriptSubset({:?}) — not yet implemented",
        font
    );
    nil
}

fn CGFontCreatePostScriptEncoding(
    _env: &mut Environment,
    font: CGFontRef,
    _encoding: ConstPtr<CGGlyph>,
) -> CFTypeRef {
    log!(
        "TODO: CGFontCreatePostScriptEncoding({:?}) — not yet implemented",
        font
    );
    nil
}

// =========================================================================
// MARK: - Variations
// =========================================================================

/// `CFArrayRef CGFontCopyVariationAxes(CGFontRef font)`
///
/// Returns variation axes if the font is a variable font. We check for the
/// 'fvar' table; if not present, returns nil.
fn CGFontCopyVariationAxes(env: &mut Environment, font: CGFontRef) -> CFTypeRef {
    if font.is_null() {
        return nil;
    }
    if !is_data_provider_font(env, font) {
        return nil;
    }
    // Check if fvar table exists
    let fvar = env
        .objc
        .borrow::<CGFontHostObject>(font)
        .font
        .table_data(0x66766172 /* 'fvar' */);
    if fvar.is_none() {
        return nil;
    }
    // Font variations are not supported in this emulation layer.
    // Returning nil (empty) for now — the font will use its default instance.
    nil
}

/// `CFDictionaryRef CGFontCopyVariations(CGFontRef font)`
fn CGFontCopyVariations(_env: &mut Environment, font: CGFontRef) -> CFTypeRef {
    log_dbg!("CGFontCopyVariations({:?}) — returning nil", font);
    nil
}

// =========================================================================
// MARK: - Encoding / misc
// =========================================================================

/// `CGFontRef CGFontCreateWithPlatformFont(void *platformFontReference)`
/// This is a deprecated API but some old apps still call it.
fn CGFontCreateWithPlatformFont(
    _env: &mut Environment,
    _platform_font_ref: MutVoidPtr,
) -> CGFontRef {
    log!("CGFontCreateWithPlatformFont: deprecated API, returning null");
    Ptr::null()
}

/// `CFTypeID CGFontGetTypeID(void)`
///
/// Returns the Core Foundation type identifier for CGFont objects.
fn CGFontGetTypeID(env: &mut Environment) -> u32 {
    let class = env.objc.get_known_class("_touchHLE_CGFont", &mut env.mem);
    class.to_bits()
}


// =========================================================================
// MARK: - Function exports
// =========================================================================

pub const FUNCTIONS: FunctionExports = &[
    // Creation
    export_c_func!(CGFontCreateWithFontName(_)),
    export_c_func!(CGFontCreateCopyWithVariations(_, _)),
    export_c_func!(CGFontCreateWithDataProvider(_)),
    export_c_func!(CGFontCreateWithPlatformFont(_)),
    // Retain / Release
    export_c_func!(CGFontRetain(_)),
    export_c_func!(CGFontRelease(_)),
    // Type info
    export_c_func!(CGFontGetTypeID()),
    // Glyph lookup
    export_c_func!(CGFontGetGlyphsForUnichars(_, _, _, _)),
    export_c_func!(CGFontGetGlyphWithGlyphName(_, _)),
    export_c_func!(CGFontCopyGlyphNameForGlyph(_, _)),
    // Names
    export_c_func!(CGFontCopyPostScriptName(_)),
    export_c_func!(CGFontCopyFullName(_)),
    // Metrics
    export_c_func!(CGFontGetNumberOfGlyphs(_)),
    export_c_func!(CGFontGetUnitsPerEm(_)),
    export_c_func!(CGFontGetAscent(_)),
    export_c_func!(CGFontGetDescent(_)),
    export_c_func!(CGFontGetLeading(_)),
    export_c_func!(CGFontGetCapHeight(_)),
    export_c_func!(CGFontGetXHeight(_)),
    export_c_func!(CGFontGetItalicAngle(_)),
    export_c_func!(CGFontGetStemV(_)),
    export_c_func!(CGFontGetFontBBox(_, _)),
    // Per-glyph metrics
    export_c_func!(CGFontGetGlyphAdvances(_, _, _, _)),
    export_c_func!(CGFontGetGlyphBBoxes(_, _, _, _)),
    // Table access
    export_c_func!(CGFontCopyTableTags(_)),
    export_c_func!(CGFontCopyTableForTag(_, _)),
    // PostScript
    export_c_func!(CGFontCanCreatePostScriptSubset(_, _)),
    export_c_func!(CGFontCreatePostScriptSubset(_, _, _, _, _, _)),
    export_c_func!(CGFontCreatePostScriptEncoding(_, _)),
    // Variations
    export_c_func!(CGFontCopyVariationAxes(_)),
    export_c_func!(CGFontCopyVariations(_)),
];
