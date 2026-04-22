use crate::render::font_backend::{
    find_font_with_glyph, fontconfig_fallback_matches, load_font_match, normalize_font_pattern,
    resolve_primary_font_match, FontMatch, LoadedFont,
};
use anyhow::{Context, Result};
use fontdue::{Font, Metrics};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug)]
pub struct RendererMetrics {
    pub cell_width: i32,
    pub cell_height: i32,
    pub baseline: i32,
    pub font_size: f32,
}

#[derive(Clone, Debug)]
pub(super) struct CachedGlyph {
    pub width: usize,
    pub height: usize,
    pub xmin: i32,
    pub ymin: i32,
    pub alpha: Vec<u8>,
}

pub struct TextRenderer {
    primary: LoadedFont,
    fallback_fonts: Vec<LoadedFont>,
    fallback_index_by_match: HashMap<FontMatch, usize>,
    pub metrics: RendererMetrics,
    glyphs: HashMap<char, CachedGlyph>,
    font_pattern: String,
}

fn truncate_with_ellipsis_impl(text: &str, max_cells: usize) -> String {
    if max_cells == 0 {
        return String::new();
    }

    let visible = text.split('\n').next().unwrap_or("");
    let total_cells: usize = visible.chars().map(|c| char_cell_width(c) as usize).sum();
    if total_cells <= max_cells {
        return visible.to_string();
    }
    if max_cells == 1 {
        return "…".to_string();
    }

    let mut used = 0usize;
    let mut end = 0usize;
    for (idx, ch) in visible.char_indices() {
        let w = char_cell_width(ch) as usize;
        if used + w > max_cells - 1 {
            break;
        }
        used += w;
        end = idx + ch.len_utf8();
    }
    format!("{}…", &visible[..end])
}

impl TextRenderer {
    pub fn load(font_size: f32) -> Result<Self> {
        Self::with_font_family(None, font_size)
    }

    pub fn with_font_family(font_family: Option<&str>, font_size: f32) -> Result<Self> {
        let requested_pattern = normalize_font_pattern(font_family);
        let (font_pattern, primary_match) = resolve_primary_font_match(&requested_pattern)
            .context("failed to find a usable system font")?;
        Self::from_primary_match(primary_match, font_pattern, font_size)
    }

    fn from_primary_match(
        primary_match: FontMatch,
        font_pattern: String,
        font_size: f32,
    ) -> Result<Self> {
        let primary = load_font_match(&primary_match)?;
        let metrics = compute_metrics(&primary.font, font_size);
        Ok(Self {
            primary,
            fallback_fonts: Vec::new(),
            fallback_index_by_match: HashMap::new(),
            metrics,
            glyphs: HashMap::new(),
            font_pattern,
        })
    }

    pub fn measure_text(&self, text: &str) -> i32 {
        let width = self.metrics.cell_width.max(1);
        let mut cells = 0i32;
        for ch in text.chars() {
            if ch == '\n' {
                break;
            }
            cells += char_cell_width(ch);
        }
        cells * width
    }

    pub fn fit_text<'a>(&self, text: &'a str, max_cells: usize) -> &'a str {
        if max_cells == 0 {
            return "";
        }
        let mut used = 0usize;
        let mut end = 0usize;
        for (idx, ch) in text.char_indices() {
            let w = char_cell_width(ch) as usize;
            if used + w > max_cells || ch == '\n' {
                break;
            }
            used += w;
            end = idx + ch.len_utf8();
        }
        &text[..end]
    }

    pub fn truncate_with_ellipsis(&self, text: &str, max_cells: usize) -> String {
        truncate_with_ellipsis_impl(text, max_cells)
    }

    pub(super) fn glyph(&mut self, ch: char) -> &CachedGlyph {
        if !self.glyphs.contains_key(&ch) {
            let glyph = self.rasterize_glyph(ch);
            self.glyphs.insert(ch, glyph);
        }
        self.glyphs.get(&ch).expect("glyph cached")
    }

    fn rasterize_glyph(&mut self, ch: char) -> CachedGlyph {
        let font_index = self.font_index_for_char(ch);
        let font = if font_index == 0 {
            &self.primary.font
        } else {
            &self.fallback_fonts[font_index - 1].font
        };
        let (metrics, alpha) = font.rasterize(ch, self.metrics.font_size);
        CachedGlyph {
            width: metrics.width,
            height: metrics.height,
            xmin: metrics.xmin,
            ymin: metrics.ymin,
            alpha,
        }
    }

    fn font_index_for_char(&mut self, ch: char) -> usize {
        if self.primary.font.has_glyph(ch) {
            return 0;
        }

        if let Some(index) = self
            .fallback_fonts
            .iter()
            .position(|loaded| loaded.font.has_glyph(ch))
        {
            return index + 1;
        }

        self.load_fallback_font_for_char(ch)
            .map(|index| index + 1)
            .unwrap_or(0)
    }

    fn load_fallback_font_for_char(&mut self, ch: char) -> Option<usize> {
        for font_match in fontconfig_fallback_matches(&self.font_pattern, ch) {
            if font_match == self.primary.match_info {
                continue;
            }
            if let Ok(index) = self.ensure_fallback_loaded(font_match) {
                if self.fallback_fonts[index].font.has_glyph(ch) {
                    return Some(index);
                }
            }
        }

        let font_match = find_font_with_glyph(ch)?;
        if font_match == self.primary.match_info {
            return None;
        }
        let index = self.ensure_fallback_loaded(font_match).ok()?;
        self.fallback_fonts[index]
            .font
            .has_glyph(ch)
            .then_some(index)
    }

    fn ensure_fallback_loaded(&mut self, font_match: FontMatch) -> Result<usize> {
        if let Some(&index) = self.fallback_index_by_match.get(&font_match) {
            return Ok(index);
        }

        let loaded = load_font_match(&font_match)?;
        let index = self.fallback_fonts.len();
        self.fallback_fonts.push(loaded);
        self.fallback_index_by_match.insert(font_match, index);
        Ok(index)
    }
}

fn compute_metrics(font: &Font, font_size: f32) -> RendererMetrics {
    let probe_chars = ['M', 'W', '0', '@', 'i', ' '];
    let mut cell_width = 0i32;
    let mut ascent = 0i32;
    let mut descent = 0i32;

    for ch in probe_chars {
        let metrics = font.metrics(ch, font_size);
        cell_width = cell_width.max(estimate_cell_width(&metrics));
        ascent = ascent.max((metrics.height as i32 + metrics.ymin).max(0));
        descent = descent.max((-metrics.ymin).max(0));
    }

    let line_metrics = font
        .horizontal_line_metrics(font_size)
        .unwrap_or(fontdue::LineMetrics {
            ascent: font_size * 0.8,
            descent: -font_size * 0.2,
            line_gap: font_size * 0.2,
            new_line_size: font_size,
        });

    ascent = ascent.max(line_metrics.ascent.ceil() as i32);
    descent = descent.max((-line_metrics.descent).ceil() as i32);

    let cell_height = (line_metrics.new_line_size.ceil() as i32)
        .max(ascent + descent)
        .max(1);
    let baseline = ascent.max(1);

    RendererMetrics {
        cell_width: cell_width.max(1),
        cell_height,
        baseline,
        font_size,
    }
}

fn estimate_cell_width(metrics: &Metrics) -> i32 {
    metrics
        .advance_width
        .ceil()
        .max((metrics.width as i32 + metrics.xmin.max(0)) as f32) as i32
}

pub(crate) fn blend_over(dst: u32, src: u32, coverage: u8) -> u32 {
    let sa = (((src >> 24) & 0xFF) * coverage as u32 + 127) / 255;
    if sa == 0 {
        return dst;
    }
    let inv = 255 - sa;

    let sr = (src >> 16) & 0xFF;
    let sg = (src >> 8) & 0xFF;
    let sb = src & 0xFF;

    let dr = (dst >> 16) & 0xFF;
    let dg = (dst >> 8) & 0xFF;
    let db = dst & 0xFF;
    let da = (dst >> 24) & 0xFF;

    let r = (sr * sa + dr * inv + 127) / 255;
    let g = (sg * sa + dg * inv + 127) / 255;
    let b = (sb * sa + db * inv + 127) / 255;
    let a = sa + (da * inv + 127) / 255;

    ((a & 0xFF) << 24) | ((r & 0xFF) << 16) | ((g & 0xFF) << 8) | (b & 0xFF)
}

pub(crate) fn char_cell_width(ch: char) -> i32 {
    unicode_width::UnicodeWidthChar::width(ch)
        .unwrap_or(1)
        .max(1) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blend_over_respects_zero_and_full_coverage() {
        assert_eq!(blend_over(0x11223344, 0xFFABCDEF, 0), 0x11223344);
        assert_eq!(blend_over(0x00000000, 0xFFABCDEF, 255), 0xFFABCDEF);
    }

    #[test]
    fn blend_over_scales_source_alpha_by_coverage() {
        assert_eq!(blend_over(0x00000000, 0x80FF0000, 128), 0x40_40_00_00);
    }

    #[test]
    fn estimate_cell_width_prefers_visible_bitmap_extent() {
        let metrics = Metrics {
            xmin: 3,
            ymin: 0,
            width: 8,
            height: 10,
            advance_width: 4.2,
            advance_height: 0.0,
            bounds: fontdue::OutlineBounds {
                xmin: 0.0,
                ymin: 0.0,
                width: 0.0,
                height: 0.0,
            },
        };

        assert_eq!(estimate_cell_width(&metrics), 11);
    }

    #[test]
    fn estimate_cell_width_respects_advance_width_floor() {
        let metrics = Metrics {
            xmin: -2,
            ymin: 0,
            width: 3,
            height: 10,
            advance_width: 6.1,
            advance_height: 0.0,
            bounds: fontdue::OutlineBounds {
                xmin: 0.0,
                ymin: 0.0,
                width: 0.0,
                height: 0.0,
            },
        };

        assert_eq!(estimate_cell_width(&metrics), 7);
    }

    #[test]
    fn char_cell_width_never_returns_less_than_one() {
        assert_eq!(char_cell_width('a'), 1);
        assert_eq!(char_cell_width('\0'), 1);
        assert_eq!(char_cell_width('界'), 2);
    }

    #[test]
    fn truncate_with_ellipsis_stops_at_newline_without_counting_following_lines() {
        assert_eq!(truncate_with_ellipsis_impl("abc\ndef", 3), "abc");
        assert_eq!(truncate_with_ellipsis_impl("abcd\nef", 3), "ab…");
    }
}
