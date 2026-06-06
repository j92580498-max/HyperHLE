/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Text layout and font rasterization abstraction.
//!
//! This is implemented using the [rusttype] library. All usage of that library
//! should be confined to this module.
//!
//! TODO: Less terrible text layout. RustType doesn't do text layout so this
//! code has its own, not particularly good implementation. We might want to
//! switch to something like cosmic-text in future, but that has a _lot_ more
//! dependencies.

use crate::frameworks::core_graphics::cg_affine_transform::CGAffineTransform;
use crate::frameworks::core_graphics::{CGPoint, CGRect, CGSize};
use crate::paths;
use rusttype::{vector, GlyphId, Point, Scale};
use std::io::Read;

mod arabic;

pub struct Font {
    /// `None` only for the phantom-default `Font` returned via
    /// [`Default`] when the objc runtime needs a placeholder for a
    /// missing/wrong-typed host object. Real `Font`s constructed via
    /// the `mono_regular()` / `from_file()` / etc. helpers always set
    /// this to `Some(...)`.
    font: Option<rusttype::Font<'static>>,
    /// Raw font file bytes — needed for TrueType table access (CGFontCopyTableForTag).
    raw_data: Option<Vec<u8>>,
}

impl Default for Font {
    fn default() -> Self {
        Font {
            font: None,
            raw_data: None,
        }
    }
}

impl Font {
    /// Panic with a clear message if someone tries to use a phantom/default
    /// `Font` (one without an actual `rusttype::Font` loaded) — this
    /// indicates a programming bug elsewhere, since the only path that
    /// produces such a `Font` is the `objc::ObjC::borrow` fallback used
    /// for missing/wrong-typed host objects.
    fn rt(&self) -> &rusttype::Font<'static> {
        self.font.as_ref().expect(
            "tried to use methods on an uninitialised Font \
             (likely a phantom-fallback host object)",
        )
    }
}

pub enum TextAlignment {
    Left,
    Center,
    Right,
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum WrapMode {
    Word,
    Char,
}

fn scale(font_size: f32) -> Scale {
    // iPhone OS's interpretation of font size is slightly different, reason
    // unknown. This is not the same as the Windows pt vs Mac pt issue.
    // This scale factor has been eyeball'd, it's not exact.
    Scale::uniform(font_size * 1.125)
}

/// Helper for [Font::draw], used for the `draw_glyph` callback.
pub struct RasterGlyph<'a> {
    origin: (f32, f32),
    dimensions: (i32, i32),
    pixels: &'a [f32],
}
impl RasterGlyph<'_> {
    /// Get the x and y co-ordinates the glyph should be drawn at.
    pub fn origin(&self) -> (f32, f32) {
        self.origin
    }
    /// Get the dimensions, in pixels, of the glyph.
    pub fn dimensions(&self) -> (i32, i32) {
        self.dimensions
    }
    /// Get the coverage at the given co-ordinates within the glyph.
    pub fn pixel_at(&self, coords: (i32, i32)) -> f32 {
        let (width, height) = self.dimensions;
        if (0..width).contains(&coords.0) && (0..height).contains(&coords.1) {
            self.pixels[coords.1 as usize * width as usize + coords.0 as usize]
        } else {
            0.0 // safety in case of rounding errors
        }
    }
}

impl Font {
    pub fn glyph_id_for_char(&self, c: u16) -> GlyphId {
        self.rt().glyph(char::from_u32(c as u32).unwrap()).id()
    }

    /// Returns the total number of glyphs in the font.
    pub fn glyph_count(&self) -> u32 {
        self.rt().glyph_count() as u32
    }

    /// Returns the units-per-em value from the font. rusttype normalizes to
    /// 1.0 scale = 1 em, so we read it from the raw 'head' table if available,
    /// otherwise default to 2048.
    pub fn units_per_em(&self) -> u16 {
        if let Some(data) = self.table_data(0x68656164 /* 'head' */) {
            if data.len() >= 20 {
                // unitsPerEm is at offset 18 in the 'head' table (big-endian u16)
                let upm = u16::from_be_bytes([data[18], data[19]]);
                if upm > 0 {
                    return upm;
                }
            }
        }
        2048
    }

    /// Returns the advance width (in design units) for a glyph.
    pub fn glyph_advance(&self, glyph_id: GlyphId) -> i32 {
        let upm = self.units_per_em() as f32;
        let g = self.rt().glyph(glyph_id).scaled(Scale::uniform(upm));
        g.h_metrics().advance_width.round() as i32
    }

    /// Returns the left side bearing (in design units) for a glyph.
    pub fn glyph_left_side_bearing(&self, glyph_id: GlyphId) -> i32 {
        let upm = self.units_per_em() as f32;
        let g = self.rt().glyph(glyph_id).scaled(Scale::uniform(upm));
        g.h_metrics().left_side_bearing.round() as i32
    }

    /// Returns the bounding box of a glyph in design units.
    /// Returns (x_min, y_min, width, height). If the glyph has no outline,
    /// returns a zero rect.
    pub fn glyph_bbox(&self, glyph_id: GlyphId) -> (f32, f32, f32, f32) {
        let upm = self.units_per_em() as f32;
        let g = self.rt().glyph(glyph_id)
            .scaled(Scale::uniform(upm))
            .positioned(Point { x: 0.0, y: 0.0 });
        match g.pixel_bounding_box() {
            Some(bb) => {
                let x = bb.min.x as f32;
                let y = bb.min.y as f32;
                let w = (bb.max.x - bb.min.x) as f32;
                let h = (bb.max.y - bb.min.y) as f32;
                (x, y, w, h)
            }
            None => (0.0, 0.0, 0.0, 0.0),
        }
    }

    /// Returns raw bytes of the font data (for table access).
    pub fn raw_data(&self) -> Option<&[u8]> {
        self.raw_data.as_deref()
    }

    /// Parse and return the raw data for a specific TrueType/OpenType table
    /// identified by its 4-byte tag (e.g. b"head" → 0x68656164).
    pub fn table_data(&self, tag: u32) -> Option<Vec<u8>> {
        let data = self.raw_data.as_ref()?;
        parse_table_from_raw(data, tag)
    }

    /// Returns a list of all table tags present in the font file.
    pub fn table_tags(&self) -> Vec<u32> {
        let Some(data) = self.raw_data.as_ref() else {
            return Vec::new();
        };
        parse_table_tags_from_raw(data)
    }

    /// Look up a glyph by its PostScript name. Returns `None` if the 'post'
    /// table is unavailable or the name is not found.
    pub fn glyph_for_name(&self, name: &str) -> Option<GlyphId> {
        // Parse the 'post' table to build a name→glyph mapping.
        let post_data = self.table_data(0x706F7374 /* 'post' */)?;
        glyph_id_from_post_table(&post_data, name, self.glyph_count())
    }

    /// Returns the PostScript name for a glyph index, or None.
    pub fn glyph_name(&self, glyph_id: GlyphId) -> Option<String> {
        let post_data = self.table_data(0x706F7374 /* 'post' */)?;
        glyph_name_from_post_table(&post_data, glyph_id.0 as u32, self.glyph_count())
    }

    fn from_resource_file(filename: &str) -> Font {
        let mut bytes = Vec::new();
        let path = format!("{}/{}", paths::FONTS_DIR, filename);
        if let Err(e) = paths::ResourceFile::open(&path)
            .and_then(|mut f| f.get().read_to_end(&mut bytes).map_err(|e| e.to_string()))
        {
            panic!(
                "Couldn't read bundled font file {path:?}: {e}. Perhaps the directory is missing?"
            );
        }

        let raw_copy = bytes.clone();
        let Some(font) = rusttype::Font::try_from_vec(bytes) else {
            panic!("Couldn't parse bundled font file {path:?}. This probably means the file is corrupt. Try re-downloading it.");
        };

        Font {
            font: Some(font),
            raw_data: Some(raw_copy),
        }
    }

    pub fn from_vec(bytes: Vec<u8>) -> Option<Font> {
        let raw_copy = bytes.clone();
        let Some(font) = rusttype::Font::try_from_vec(bytes) else {
            log!(
                "Warning: Font::from_vec: couldn't parse {} bytes of font data; \
                 returning None (the data is likely not a supported font format).",
                raw_copy.len()
            );
            return None;
        };

        Some(Font {
            font: Some(font),
            raw_data: Some(raw_copy),
        })
    }

    pub fn mono_regular() -> Font {
        Self::from_resource_file("LiberationMono-Regular.ttf")
    }
    pub fn mono_bold() -> Font {
        Self::from_resource_file("LiberationMono-Bold.ttf")
    }
    pub fn mono_bold_italic() -> Font {
        Self::from_resource_file("LiberationMono-BoldItalic.ttf")
    }
    pub fn mono_italic() -> Font {
        Self::from_resource_file("LiberationMono-Italic.ttf")
    }
    pub fn sans_regular() -> Font {
        Self::from_resource_file("LiberationSans-Regular.ttf")
    }
    pub fn sans_bold() -> Font {
        Self::from_resource_file("LiberationSans-Bold.ttf")
    }
    pub fn sans_bold_italic() -> Font {
        Self::from_resource_file("LiberationSans-BoldItalic.ttf")
    }
    pub fn sans_italic() -> Font {
        Self::from_resource_file("LiberationSans-Italic.ttf")
    }
    pub fn serif_regular() -> Font {
        Self::from_resource_file("LiberationSerif-Regular.ttf")
    }
    pub fn serif_bold() -> Font {
        Self::from_resource_file("LiberationSerif-Bold.ttf")
    }
    pub fn serif_bold_italic() -> Font {
        Self::from_resource_file("LiberationSerif-BoldItalic.ttf")
    }
    pub fn serif_italic() -> Font {
        Self::from_resource_file("LiberationSerif-Italic.ttf")
    }
    pub fn sans_regular_ja() -> Font {
        Self::from_resource_file("NotoSansJP-Regular.otf")
    }
    pub fn sans_bold_ja() -> Font {
        Self::from_resource_file("NotoSansJP-Bold.otf")
    }
    pub fn sans_regular_ar() -> Font {
        Self::from_resource_file("NotoSansArabic-Regular.ttf")
    }
    pub fn sans_bold_ar() -> Font {
        Self::from_resource_file("NotoSansArabic-Bold.ttf")
    }

    pub fn ascent(&self, font_size: f32) -> f32 {
        let v_metrics = self.rt().v_metrics(scale(font_size));
        v_metrics.ascent
    }
    pub fn descent(&self, font_size: f32) -> f32 {
        let v_metrics = self.rt().v_metrics(scale(font_size));
        v_metrics.descent
    }

    pub fn line_gap(&self, font_size: f32) -> f32 {
        let v_metrics = self.rt().v_metrics(scale(font_size));
        v_metrics.line_gap
    }

    fn line_height_and_gap(&self, font_size: f32) -> (f32, f32) {
        let v_metrics = self.rt().v_metrics(scale(font_size));
        (v_metrics.ascent - v_metrics.descent, v_metrics.line_gap)
    }

    /// Calculate the width of a line. This does not handle newlines!
    fn calculate_line_width(&self, font_size: f32, line: &str) -> f32 {
        let mut line_x_min: f32 = 0.0;
        let mut line_x_max: f32 = 0.0;

        // Apply Arabic shaping/reordering so the measured width matches what
        // will actually be drawn (see [arabic::shape_line_for_display]).
        let line = arabic::shape_line_for_display(line);

        for glyph in self.rt().layout(&line, scale(font_size), Default::default()) {
            let position = glyph.position();
            let h_metrics = glyph.unpositioned().h_metrics();

            // This method used to use pixel_bounding_box() for metrics, but
            // now uses h_metrics() in order to support whitespace characters.
            // This definition of character width was chosen because it gave
            // similar results to the old implementation, not because it's
            // optimal; maybe it could be improved.
            let glyph_x_min = position.x.min(position.x + h_metrics.left_side_bearing);
            let glyph_x_max = position.x + h_metrics.advance_width;

            line_x_min = line_x_min.min(glyph_x_min);
            line_x_min = line_x_min.min(glyph_x_max);
            line_x_max = line_x_max.max(glyph_x_min);
            line_x_max = line_x_max.max(glyph_x_max);
        }

        // This rounding is also to emulate pixel_bounding_box(), same caveat
        // applies.
        line_x_max.ceil() - line_x_min.floor()
    }

    /// Break text into lines with known widths.
    pub fn break_lines<'a>(
        &self,
        font_size: f32,
        text: &'a str,
        wrap: Option<(f32, WrapMode)>,
    ) -> Vec<(f32, &'a str)> {
        let mut lines = Vec::new();

        for line in text.lines() {
            let Some((wrap_width, wrap_mode)) = wrap else {
                lines.push((self.calculate_line_width(font_size, line), line));
                continue;
            };

            let unwrapped_line = line;

            // Find points at which the line could be wrapped
            let mut wrap_points = Vec::new();
            match wrap_mode {
                WrapMode::Word => {
                    let mut word_start = 0;

                    loop {
                        let Some(i) = line[word_start..].find(|c: char| c.is_whitespace()) else {
                            break;
                        };
                        let word_end = word_start + i;
                        // Include any additional whitespace in the word,
                        // so that the next word begins with non-whitespace.
                        let Some(i) = line[word_end..].find(|c: char| !c.is_whitespace()) else {
                            break;
                        };
                        wrap_points.push(word_end + i);
                        word_start = word_end + i;
                    }
                }
                WrapMode::Char => {
                    let mut char_end = 1;
                    while char_end < line.len() {
                        if line.is_char_boundary(char_end) {
                            wrap_points.push(char_end);
                        }
                        char_end += 1;
                    }
                }
            };
            wrap_points.push(line.len());

            let mut next_wrap_point_idx = 0;
            let mut line_start = 0;

            fn trim_wrapped_line(wrap_mode: WrapMode, line: &str) -> &str {
                // Spaces before a word wrap point are ignored for
                // wrapping purposes.
                if wrap_mode == WrapMode::Word {
                    line.trim_end()
                } else {
                    line
                }
            }

            while next_wrap_point_idx < wrap_points.len() {
                // Find optimal line wrapping by binary search.
                // `binary_search_by` returns Err when there's no exactly
                // matching line length, which is usually going to be the case.
                let wrap_search_result =
                    wrap_points[next_wrap_point_idx..].binary_search_by(|&wrap_point| {
                        let line = &line[line_start..wrap_point];
                        let line_width = self
                            .calculate_line_width(font_size, trim_wrapped_line(wrap_mode, line));
                        line_width.partial_cmp(&wrap_width).unwrap()
                    });
                let wrap_point_idx = match wrap_search_result {
                    Ok(i) => next_wrap_point_idx + i,
                    Err(i @ 1..) => next_wrap_point_idx + (i - 1),
                    _ => {
                        // The span between the current wrap point and the next
                        // wrap point is wider than the wrap width. In practice,
                        // this means a word too big to fit on-screen.
                        if matches!(wrap_mode, WrapMode::Word) {
                            // Try to break the word.
                            let word_end = wrap_points[next_wrap_point_idx];
                            let word = &line[line_start..word_end];
                            let broken_words = self.break_lines(
                                font_size,
                                word,
                                Some((wrap_width, WrapMode::Char)),
                            );
                            lines.extend(broken_words);
                            next_wrap_point_idx += 1;
                            line_start = word_end;
                            continue;
                        }
                        // It can't be helped: truncate.
                        next_wrap_point_idx
                    }
                };
                let line_end = wrap_points[wrap_point_idx];
                let line = &line[line_start..line_end];

                let trimmed_line = if line_end != unwrapped_line.len() {
                    // Whitespace at the end of a line must only be ignored if
                    // that line break came from word wrapping.
                    trim_wrapped_line(wrap_mode, line)
                } else {
                    line
                };

                lines.push((
                    self.calculate_line_width(font_size, trimmed_line),
                    trimmed_line,
                ));

                next_wrap_point_idx = wrap_point_idx + 1;
                line_start = line_end;
            }
        }

        lines
    }

    /// Calculate the on-screen width and height of text with a given font size.
    pub fn calculate_text_size(
        &self,
        font_size: f32,
        text: &str,
        wrap: Option<(f32, WrapMode)>,
    ) -> (f32, f32) {
        let lines = self.break_lines(font_size, text, wrap);

        let width = lines
            .iter()
            .fold(0f32, |widest, &(line_width, _line)| widest.max(line_width));
        let (line_height, line_gap) = self.line_height_and_gap(font_size);
        let height =
            line_height * (lines.len() as f32) + line_gap * (lines.len().saturating_sub(1) as f32);

        (width, height)
    }

    /// Draw text. Calls the provided callback for each glyph that is to be
    /// drawn. Assumes y starts at the top-left corner and points downwards.
    /// Used by UIKit for font rendering.
    pub fn draw<F: FnMut(RasterGlyph)>(
        &self,
        font_size: f32,
        text: &str,
        origin: (f32, f32),
        wrap: Option<(f32, WrapMode)>,
        alignment: TextAlignment,
        mut draw_glyph: F,
    ) {
        let lines = self.break_lines(font_size, text, wrap);

        let ascent = self.rt().v_metrics(scale(font_size)).ascent;
        let (line_height, line_gap) = self.line_height_and_gap(font_size);

        // RustType requires a "draw pixel" callback that will be called for
        // each pixel in the glyph's bounding box, in left-to-right
        // top-to-bottom order. This is unfortunately incompatible with
        // touchHLE's code which needs to be able to sample the pixels in any
        // order in order to support rotation. This is worked around by creating
        // a temporary bitmap for the glyph, and then the caller of this
        // function can provide a "draw glyph" callback that can do whatever it
        // wants with this bitmap.
        // TODO: Do we need to increase the font size when scale transforms are
        //       used, to avoid blurry text?
        let mut glyph_bitmap: Vec<f32> = Vec::new();

        for (line_idx, (line_width, line_text)) in lines.iter().enumerate() {
            let line_x_offset = match alignment {
                TextAlignment::Left => 0.0,
                TextAlignment::Center => -line_width / 2.0,
                TextAlignment::Right => -line_width,
            };

            let baseline = origin.1 + ascent + line_idx as f32 * (line_gap + line_height);

            // Reshape/reorder Arabic text into visual order before layout.
            let shaped_line = arabic::shape_line_for_display(line_text);

            for glyph in self.rt().layout(
                &shaped_line,
                scale(font_size),
                Point {
                    x: origin.0 + line_x_offset,
                    y: baseline,
                },
            ) {
                let Some(glyph_bounds) = glyph.pixel_bounding_box() else {
                    continue;
                };

                // TODO: Refactor this method to support y clipping too.
                // It's not mandatory since the caller can do it, but it would
                // be more efficient.
                if let Some((wrap_width, _)) = wrap {
                    if glyph_bounds.min.x as f32 > origin.0 + wrap_width {
                        // Avoid wasting effort on glyphs that are entirely
                        // clipped. Partial clipping is the responsibility of
                        // the draw_glyph implementation.
                        continue;
                    }
                }

                let glyph_bitmap_bounds = (
                    glyph_bounds.width() as usize,
                    glyph_bounds.height() as usize,
                );
                glyph_bitmap.clear();
                glyph_bitmap.resize(glyph_bitmap_bounds.0 * glyph_bitmap_bounds.1, 0.0);

                glyph.draw(|x, y, coverage| {
                    glyph_bitmap[y as usize * glyph_bitmap_bounds.0 + x as usize] = coverage;
                });

                let raster_glyph = RasterGlyph {
                    origin: (glyph_bounds.min.x as f32, glyph_bounds.min.y as f32),
                    dimensions: (glyph_bitmap_bounds.0 as i32, glyph_bitmap_bounds.1 as i32),
                    pixels: &glyph_bitmap,
                };

                draw_glyph(raster_glyph);
            }
        }
    }

    /// Draw glyphs. Similar to [Self::draw], but uses raw glyph ids instead of
    /// text and doesn't account for line breaks or text alignment (those
    /// should be handled by the caller). Used by CoreGraphics for font
    /// rendering.
    /// TODO: unify with [Self::draw]. Note: y sense is different! If you
    /// plan to do that refactoring, make sure that there are no visual
    /// regressions in the GUI tests for CGFont/CGGlyph!
    pub fn draw_glyphs<I, F>(
        &self,
        font_size: f32,
        glyphs: I,
        origin: (f32, f32),
        text_transform: CGAffineTransform,
        mut draw_glyph: F,
    ) where
        I: IntoIterator<Item = GlyphId>,
        F: FnMut(RasterGlyph),
    {
        // TODO: avoid creating a tmp bitmap
        let mut tmp_glyph_bitmap: Vec<f32> = Vec::new();
        // Cf. comment in the [Self::draw] function.
        let mut glyph_bitmap: Vec<f32> = Vec::new();

        let inverted_text_transform = text_transform.invert();

        // rusttype only supports scaling, so we render each glyph at a
        // resolution matched to the effective scale of the text
        // transform and then resample to apply rotation/mirror/etc.
        // TODO: apply x and y scales independently?
        let unit_x = text_transform.apply_to_size(CGSize {
            width: 1.0,
            height: 0.0,
        });
        let unit_y = text_transform.apply_to_size(CGSize {
            width: 0.0,
            height: 1.0,
        });
        let tmp_font_scale = unit_x
            .width
            .hypot(unit_x.height)
            .max(unit_y.width.hypot(unit_y.height))
            .max(1.0);

        let start = Point { x: 0.0, y: 0.0 };
        // This code is adapted from documentation of [rusttype::Font::layout].
        let iter = self.rt().glyphs_for(glyphs.into_iter())
            .scan((None, 0.0), |(last, x), g| {
                let g = g.scaled(scale(font_size * tmp_font_scale));
                if let Some(last) = last {
                    *x += self.rt().pair_kerning(
                        scale(font_size * tmp_font_scale),
                        *last,
                        g.id(),
                    );
                }
                let w = g.h_metrics().advance_width;
                let next = g.positioned(start + vector(*x, 0.0));
                *last = Some(next.id());
                *x += w;
                Some(next)
            });
        for glyph in iter {
            let Some(glyph_bounds) = glyph.pixel_bounding_box() else {
                continue;
            };
            let rect = CGRect {
                origin: CGPoint {
                    x: glyph_bounds.min.x as f32 / tmp_font_scale,
                    // Note: this is because y is inverted!
                    y: -glyph_bounds.max.y as f32 / tmp_font_scale,
                },
                size: CGSize {
                    width: glyph_bounds.width() as f32 / tmp_font_scale,
                    height: glyph_bounds.height() as f32 / tmp_font_scale,
                },
            };

            let transformed = text_transform.apply_to_rect(rect);
            assert!(rect.size.width >= 0.0 && rect.size.height >= 0.0);
            log_dbg!(
                "draw_glyphs: glyph {:?}, bounds {:?}, rect {:?}, transformed {:?}",
                glyph,
                glyph_bounds,
                rect,
                transformed
            );

            let x_offset = (origin.0 + transformed.origin.x).round() as i32;
            let y_offset = (origin.1 + transformed.origin.y).round() as i32;

            let tmp_glyph_bitmap_bounds = (
                glyph_bounds.width() as usize,
                glyph_bounds.height() as usize,
            );
            log_dbg!("tmp_glyph_bitmap_bounds {:?}", tmp_glyph_bitmap_bounds);
            tmp_glyph_bitmap.clear();
            tmp_glyph_bitmap.resize(tmp_glyph_bitmap_bounds.0 * tmp_glyph_bitmap_bounds.1, 0.0);

            glyph.draw(|x, y, coverage| {
                // Note: need to fill the bitmap in the reverse y order to
                // account for y sense
                tmp_glyph_bitmap[(tmp_glyph_bitmap_bounds.1 - 1 - y as usize)
                    * tmp_glyph_bitmap_bounds.0
                    + x as usize] = coverage;
            });

            // Ceil so fractional pixels at the right/bottom edge aren't
            // clipped.
            let glyph_bitmap_bounds = (
                transformed.size.width.ceil() as usize,
                transformed.size.height.ceil() as usize,
            );
            log_dbg!(
                "x_offset {}, y_offset {}, size {:?}",
                x_offset,
                y_offset,
                glyph_bitmap_bounds
            );
            glyph_bitmap.clear();
            glyph_bitmap.resize(glyph_bitmap_bounds.0 * glyph_bitmap_bounds.1, 0.0);

            for y in 0..glyph_bitmap_bounds.1 {
                for x in 0..glyph_bitmap_bounds.0 {
                    let point = CGPoint {
                        x: transformed.origin.x + x as f32 + 0.5,
                        y: transformed.origin.y + y as f32 + 0.5,
                    };
                    let orig = inverted_text_transform.apply_to_point(point);
                    let orig_x = (orig.x * tmp_font_scale - glyph_bounds.min.x as f32) as i32;
                    let orig_y = (orig.y * tmp_font_scale + glyph_bounds.max.y as f32) as i32;
                    log_dbg!("({}, {}) -> ({}, {})", x, y, orig_x, orig_y);
                    if orig_x >= 0
                        && orig_y >= 0
                        && orig_x < tmp_glyph_bitmap_bounds.0 as i32
                        && orig_y < tmp_glyph_bitmap_bounds.1 as i32
                    {
                        let idx_orig =
                            (orig_y * tmp_glyph_bitmap_bounds.0 as i32 + orig_x) as usize;
                        glyph_bitmap[y * glyph_bitmap_bounds.0 + x] = tmp_glyph_bitmap[idx_orig];
                    }
                }
            }

            let raster_glyph = RasterGlyph {
                origin: (x_offset as f32, y_offset as f32),
                dimensions: (glyph_bitmap_bounds.0 as _, glyph_bitmap_bounds.1 as _),
                pixels: &glyph_bitmap,
            };

            draw_glyph(raster_glyph);
        }
    }

    /// Draw glyphs at specified positions. Each glyph is placed at its
    /// corresponding position (x, y). Used by CGContextShowGlyphsAtPositions.
    /// The y sense matches CoreGraphics (y growing upward in user space).
    pub fn draw_glyphs_at_positions<F>(
        &self,
        font_size: f32,
        glyphs: &[GlyphId],
        positions: &[(f32, f32)],
        origin: (f32, f32),
        mut draw_glyph: F,
    ) where
        F: FnMut(RasterGlyph),
    {
        let mut glyph_bitmap: Vec<f32> = Vec::new();

        for (glyph_id, pos) in glyphs.iter().zip(positions.iter()) {
            let x = origin.0 + pos.0;
            let y = origin.1 + pos.1;

            let g = self.rt().glyph(*glyph_id)
                .scaled(scale(font_size))
                .positioned(Point { x, y: 0.0 });

            let Some(glyph_bounds) = g.pixel_bounding_box() else {
                continue;
            };

            let x_offset = glyph_bounds.min.x;
            let y_offset = (y.round() as i32) - glyph_bounds.max.y;

            let glyph_bitmap_bounds = (
                glyph_bounds.width() as usize,
                glyph_bounds.height() as usize,
            );
            glyph_bitmap.clear();
            glyph_bitmap.resize(glyph_bitmap_bounds.0 * glyph_bitmap_bounds.1, 0.0);

            g.draw(|x, y, coverage| {
                glyph_bitmap[(glyph_bitmap_bounds.1 - 1 - y as usize) * glyph_bitmap_bounds.0
                    + x as usize] = coverage;
            });

            let raster_glyph = RasterGlyph {
                origin: (x_offset as f32, y_offset as f32),
                dimensions: (glyph_bitmap_bounds.0 as _, glyph_bitmap_bounds.1 as _),
                pixels: &glyph_bitmap,
            };

            draw_glyph(raster_glyph);
        }
    }

    /// Draw glyphs with explicit advances. Each glyph is placed sequentially
    /// with the given advance (dx, dy) offsets. Used by
    /// CGContextShowGlyphsWithAdvances.
    pub fn draw_glyphs_with_advances<F>(
        &self,
        font_size: f32,
        glyphs: &[GlyphId],
        advances: &[(f32, f32)],
        origin: (f32, f32),
        mut draw_glyph: F,
    ) where
        F: FnMut(RasterGlyph),
    {
        let mut glyph_bitmap: Vec<f32> = Vec::new();
        let mut current_x = origin.0;
        let mut current_y = origin.1;

        for (i, glyph_id) in glyphs.iter().enumerate() {
            let g = self.rt().glyph(*glyph_id)
                .scaled(scale(font_size))
                .positioned(Point {
                    x: current_x,
                    y: 0.0,
                });

            let Some(glyph_bounds) = g.pixel_bounding_box() else {
                // Even if the glyph has no outline, advance the pen.
                if i < advances.len() {
                    current_x += advances[i].0;
                    current_y += advances[i].1;
                }
                continue;
            };

            let x_offset = glyph_bounds.min.x;
            let y_offset = (current_y.round() as i32) - glyph_bounds.max.y;

            let glyph_bitmap_bounds = (
                glyph_bounds.width() as usize,
                glyph_bounds.height() as usize,
            );
            glyph_bitmap.clear();
            glyph_bitmap.resize(glyph_bitmap_bounds.0 * glyph_bitmap_bounds.1, 0.0);

            g.draw(|x, y, coverage| {
                glyph_bitmap[(glyph_bitmap_bounds.1 - 1 - y as usize) * glyph_bitmap_bounds.0
                    + x as usize] = coverage;
            });

            let raster_glyph = RasterGlyph {
                origin: (x_offset as f32, y_offset as f32),
                dimensions: (glyph_bitmap_bounds.0 as _, glyph_bitmap_bounds.1 as _),
                pixels: &glyph_bitmap,
            };

            draw_glyph(raster_glyph);

            if i < advances.len() {
                current_x += advances[i].0;
                current_y += advances[i].1;
            }
        }
    }
}

// =============================================================================
// MARK: - TrueType/OpenType table parsing helpers
// =============================================================================

/// Parse the table directory from raw font file bytes and return all table tags.
fn parse_table_tags_from_raw(data: &[u8]) -> Vec<u32> {
    if data.len() < 12 {
        return Vec::new();
    }

    // Check for TTC (TrueType Collection) — we only handle the first font.
    let offset_to_dir = if &data[0..4] == b"ttcf" {
        if data.len() < 16 {
            return Vec::new();
        }
        // offsetTable[0] is at byte 12 in TTC header
        u32::from_be_bytes([data[12], data[13], data[14], data[15]]) as usize
    } else {
        0
    };

    if data.len() < offset_to_dir + 12 {
        return Vec::new();
    }

    let num_tables =
        u16::from_be_bytes([data[offset_to_dir + 4], data[offset_to_dir + 5]]) as usize;

    let table_dir_start = offset_to_dir + 12;
    let mut tags = Vec::with_capacity(num_tables);

    for i in 0..num_tables {
        let entry_offset = table_dir_start + i * 16;
        if data.len() < entry_offset + 4 {
            break;
        }
        let tag = u32::from_be_bytes([
            data[entry_offset],
            data[entry_offset + 1],
            data[entry_offset + 2],
            data[entry_offset + 3],
        ]);
        tags.push(tag);
    }

    tags
}

/// Parse a specific table from raw font file bytes by its 4-byte tag.
fn parse_table_from_raw(data: &[u8], tag: u32) -> Option<Vec<u8>> {
    if data.len() < 12 {
        return None;
    }

    let offset_to_dir = if &data[0..4] == b"ttcf" {
        if data.len() < 16 {
            return None;
        }
        u32::from_be_bytes([data[12], data[13], data[14], data[15]]) as usize
    } else {
        0
    };

    if data.len() < offset_to_dir + 12 {
        return None;
    }

    let num_tables =
        u16::from_be_bytes([data[offset_to_dir + 4], data[offset_to_dir + 5]]) as usize;

    let table_dir_start = offset_to_dir + 12;

    for i in 0..num_tables {
        let entry_offset = table_dir_start + i * 16;
        if data.len() < entry_offset + 16 {
            break;
        }
        let entry_tag = u32::from_be_bytes([
            data[entry_offset],
            data[entry_offset + 1],
            data[entry_offset + 2],
            data[entry_offset + 3],
        ]);
        if entry_tag == tag {
            let offset = u32::from_be_bytes([
                data[entry_offset + 8],
                data[entry_offset + 9],
                data[entry_offset + 10],
                data[entry_offset + 11],
            ]) as usize;
            let length = u32::from_be_bytes([
                data[entry_offset + 12],
                data[entry_offset + 13],
                data[entry_offset + 14],
                data[entry_offset + 15],
            ]) as usize;
            if offset + length <= data.len() {
                return Some(data[offset..offset + length].to_vec());
            }
            return None;
        }
    }

    None
}

// Standard PostScript glyph names for glyph indices 0..257 (Macintosh
// standard ordering) used by 'post' table format 1.0 and 2.0.
const STANDARD_MAC_GLYPH_NAMES: &[&str] = &[
    ".notdef", ".null", "nonmarkingreturn", "space", "exclam", "quotedbl",
    "numbersign", "dollar", "percent", "ampersand", "quotesingle", "parenleft",
    "parenright", "asterisk", "plus", "comma", "hyphen", "period", "slash",
    "zero", "one", "two", "three", "four", "five", "six", "seven", "eight",
    "nine", "colon", "semicolon", "less", "equal", "greater", "question", "at",
    "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N",
    "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z",
    "bracketleft", "backslash", "bracketright", "asciicircum", "underscore",
    "grave", "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l",
    "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z",
    "braceleft", "bar", "braceright", "asciitilde", "Adieresis", "Aring",
    "Ccedilla", "Eacute", "Ntilde", "Odieresis", "Udieresis", "aacute",
    "agrave", "acircumflex", "adieresis", "atilde", "aring", "ccedilla",
    "eacute", "egrave", "ecircumflex", "edieresis", "iacute", "igrave",
    "icircumflex", "idieresis", "ntilde", "oacute", "ograve", "ocircumflex",
    "odieresis", "otilde", "uacute", "ugrave", "ucircumflex", "udieresis",
    "dagger", "degree", "cent", "sterling", "section", "bullet", "paragraph",
    "germandbls", "registered", "copyright", "trademark", "acute", "dieresis",
    "notequal", "AE", "Oslash", "infinity", "plusminus", "lessequal",
    "greaterequal", "yen", "mu", "partialdiff", "summation", "product", "pi",
    "integral", "ordfeminine", "ordmasculine", "Omega", "ae", "oslash",
    "questiondown", "exclamdown", "logicalnot", "radical", "florin",
    "approxequal", "Delta", "guillemotleft", "guillemotright", "ellipsis",
    "nonbreakingspace", "Agrave", "Atilde", "Otilde", "OE", "oe", "endash",
    "emdash", "quotedblleft", "quotedblright", "quoteleft", "quoteright",
    "divide", "lozenge", "ydieresis", "Ydieresis", "fraction", "currency",
    "guilsinglleft", "guilsinglright", "fi", "fl", "daggerdbl", "periodcentered",
    "quotesinglbase", "quotedblbase", "perthousand", "Acircumflex",
    "Ecircumflex", "Aacute", "Edieresis", "Egrave", "Iacute", "Icircumflex",
    "Idieresis", "Igrave", "Oacute", "Ocircumflex", "apple", "Ograve",
    "Uacute", "Ucircumflex", "Ugrave", "dotlessi", "circumflex", "tilde",
    "macron", "breve", "dotaccent", "ring", "cedilla", "hungarumlaut",
    "ogonek", "caron", "Lslash", "lslash", "Scaron", "scaron", "Zcaron",
    "zcaron", "brokenbar", "Eth", "eth", "Yacute", "yacute", "Thorn",
    "thorn", "minus", "multiply", "onesuperior", "twosuperior", "threesuperior",
    "onehalf", "onequarter", "threequarters", "franc", "Gbreve", "gbreve",
    "Idotaccent", "Scedilla", "scedilla", "Cacute", "cacute", "Ccaron",
    "ccaron", "dcroat",
];

/// Look up a glyph index from a 'post' table by PostScript name.
fn glyph_id_from_post_table(post_data: &[u8], name: &str, _glyph_count: u32) -> Option<GlyphId> {
    if post_data.len() < 32 {
        return None;
    }

    let format = u32::from_be_bytes([post_data[0], post_data[1], post_data[2], post_data[3]]);

    match format {
        // Format 1.0: standard 258 Macintosh glyphs
        0x00010000 => {
            for (i, &std_name) in STANDARD_MAC_GLYPH_NAMES.iter().enumerate() {
                if std_name == name {
                    return Some(GlyphId(i as u16));
                }
            }
            None
        }
        // Format 2.0: custom glyph names
        0x00020000 => {
            if post_data.len() < 34 {
                return None;
            }
            let num_glyphs =
                u16::from_be_bytes([post_data[32], post_data[33]]) as usize;
            let indices_start = 34;
            let indices_end = indices_start + num_glyphs * 2;
            if post_data.len() < indices_end {
                return None;
            }

            // Parse the string table (Pascal strings after the index array)
            let mut extra_names: Vec<&str> = Vec::new();
            let mut pos = indices_end;
            while pos < post_data.len() {
                let len = post_data[pos] as usize;
                pos += 1;
                if pos + len > post_data.len() {
                    break;
                }
                let s = std::str::from_utf8(&post_data[pos..pos + len]).unwrap_or("");
                extra_names.push(s);
                pos += len;
            }

            for glyph_idx in 0..num_glyphs {
                let idx_offset = indices_start + glyph_idx * 2;
                let name_index = u16::from_be_bytes([
                    post_data[idx_offset],
                    post_data[idx_offset + 1],
                ]) as usize;

                let glyph_name = if name_index < 258 {
                    STANDARD_MAC_GLYPH_NAMES
                        .get(name_index)
                        .copied()
                        .unwrap_or("")
                } else {
                    let extra_idx = name_index - 258;
                    extra_names.get(extra_idx).copied().unwrap_or("")
                };

                if glyph_name == name {
                    return Some(GlyphId(glyph_idx as u16));
                }
            }
            None
        }
        _ => None,
    }
}

/// Get the PostScript name of a glyph from the 'post' table.
fn glyph_name_from_post_table(
    post_data: &[u8],
    glyph_index: u32,
    _glyph_count: u32,
) -> Option<String> {
    if post_data.len() < 32 {
        return None;
    }

    let format = u32::from_be_bytes([post_data[0], post_data[1], post_data[2], post_data[3]]);

    match format {
        // Format 1.0
        0x00010000 => STANDARD_MAC_GLYPH_NAMES
            .get(glyph_index as usize)
            .map(|s| s.to_string()),
        // Format 2.0
        0x00020000 => {
            if post_data.len() < 34 {
                return None;
            }
            let num_glyphs =
                u16::from_be_bytes([post_data[32], post_data[33]]) as usize;
            if glyph_index as usize >= num_glyphs {
                return None;
            }
            let indices_start = 34;
            let indices_end = indices_start + num_glyphs * 2;
            if post_data.len() < indices_end {
                return None;
            }

            let idx_offset = indices_start + glyph_index as usize * 2;
            let name_index = u16::from_be_bytes([
                post_data[idx_offset],
                post_data[idx_offset + 1],
            ]) as usize;

            if name_index < 258 {
                return STANDARD_MAC_GLYPH_NAMES
                    .get(name_index)
                    .map(|s| s.to_string());
            }

            // Parse extra names
            let extra_idx = name_index - 258;
            let mut pos = indices_end;
            let mut current_extra = 0usize;
            while pos < post_data.len() {
                let len = post_data[pos] as usize;
                pos += 1;
                if pos + len > post_data.len() {
                    break;
                }
                if current_extra == extra_idx {
                    return std::str::from_utf8(&post_data[pos..pos + len])
                        .ok()
                        .map(|s| s.to_string());
                }
                pos += len;
                current_extra += 1;
            }
            None
        }
        _ => None,
    }
}
