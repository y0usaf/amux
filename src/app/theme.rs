use crate::render::Color;

pub(super) const TEXT: Color = ansi_index_to_color_const(7);
pub(super) const MUTED: Color = ansi_index_to_color_const(8);
pub(super) const ACCENT: Color = ansi_index_to_color_const(12);
pub(super) const ACCENT_2: Color = ansi_index_to_color_const(13);
pub(super) const SURFACE: Color = Color::rgb(0x16, 0x16, 0x18);
pub(super) const SURFACE_RAISED: Color = Color::rgb(0x20, 0x20, 0x24);
pub(super) const SIDEBAR_BG: Color = Color::rgba(0, 0, 0, 0);
pub(super) const STATUS_BG: Color = Color::rgb(0x24, 0x24, 0x28);
pub(super) const BORDER: Color = Color::rgb(0x3e, 0x3e, 0x46);
pub(super) const RUNNING: Color = ansi_index_to_color_const(10);
pub(super) const WARNING: Color = ansi_index_to_color_const(11);
pub(super) const ERROR: Color = ansi_index_to_color_const(9);
const CURSOR: Color = Color::rgb(188, 204, 255);
pub(super) const TERM_FG: Color = TEXT;
const TERM_SELECTION_BG: Color = Color::rgb(53, 92, 173);
const TERM_SELECTION_FG: Color = Color::rgb(245, 248, 255);
const SEMANTIC_CHROMA_FLOOR: u8 = 64;
const SEMANTIC_LUMA_DELTA_FLOOR: u8 = 70;
const SEMANTIC_DISTANCE_FLOOR: i16 = 48;
const WHITE: Color = Color::rgb(0xff, 0xff, 0xff);
const BLACK: Color = Color::rgb(0, 0, 0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TerminalPalette {
    pub(super) fg: Color,
    pub(super) bg: Color,
    pub(super) ansi: [Color; 16],
}

impl TerminalPalette {
    pub(super) fn fallback() -> Self {
        Self {
            fg: Color::ansi_index(7),
            bg: Color::rgba(0, 0, 0, 0),
            ansi: std::array::from_fn(|index| Color::ansi_index(index as u8)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct DerivedTheme {
    pub(super) text: Color,
    pub(super) muted: Color,
    pub(super) accent: Color,
    pub(super) accent_2: Color,
    pub(super) surface: Color,
    pub(super) surface_raised: Color,
    pub(super) sidebar_bg: Color,
    pub(super) status_bg: Color,
    pub(super) border: Color,
    pub(super) running: Color,
    pub(super) warning: Color,
    pub(super) error: Color,
    pub(super) term_fg: Color,
    pub(super) term_bg: Color,
    pub(super) ansi: [Color; 16],
}

impl DerivedTheme {
    pub(super) fn from_terminal_palette(palette: TerminalPalette) -> Self {
        let text = palette.fg;
        let bg = palette.bg;
        let muted = fade_toward(text, bg, 112);
        let warning = semantic_color(&[palette.ansi[11], palette.ansi[3], WARNING], bg);
        let error =
            separated_semantic_color(&[palette.ansi[9], palette.ansi[1], ERROR], bg, &[warning]);
        let accent = palette_accent(&palette.ansi, bg, &[warning, error]);
        let accent_2 = palette_accent(&palette.ansi, bg, &[warning, error, accent]);
        let running = accent_2;
        Self {
            text,
            muted,
            accent,
            accent_2,
            surface: fade_toward(bg, text, 12),
            surface_raised: fade_toward(bg, text, 22),
            sidebar_bg: SIDEBAR_BG,
            status_bg: fade_toward(bg, text, 28),
            border: fade_toward(text, bg, 150),
            running,
            warning,
            error,
            term_fg: text,
            term_bg: Color::rgba(0, 0, 0, 0),
            ansi: palette.ansi,
        }
    }

    pub(super) fn fallback() -> Self {
        let fallback_palette = TerminalPalette::fallback();
        let warning = Color::ansi_index(11);
        let error = Color::ansi_index(9);
        let accent = palette_accent(
            &fallback_palette.ansi,
            fallback_palette.bg,
            &[warning, error],
        );
        let accent_2 = palette_accent(
            &fallback_palette.ansi,
            fallback_palette.bg,
            &[warning, error, accent],
        );
        Self {
            text: Color::rgba(0, 0, 0, 0),
            muted: Color::ansi_index(8),
            accent,
            accent_2,
            surface: SURFACE,
            surface_raised: SURFACE_RAISED,
            sidebar_bg: SIDEBAR_BG,
            status_bg: STATUS_BG,
            border: BORDER,
            running: accent_2,
            warning,
            error,
            term_fg: Color::rgba(0, 0, 0, 0),
            term_bg: Color::rgba(0, 0, 0, 0),
            ansi: fallback_palette.ansi,
        }
    }
}

pub(super) fn screen_cell_colors(
    cell: &vt100::Cell,
    cursor_here: bool,
    selected: bool,
    default_fg: Color,
    default_bg: Color,
    ansi_palette: &[Color; 16],
) -> (Color, Color) {
    let mut fg = terminal_color(cell.fgcolor(), default_fg, ansi_palette);
    let mut bg = terminal_color(cell.bgcolor(), default_bg, ansi_palette);

    if cell.inverse() {
        std::mem::swap(&mut fg, &mut bg);
    }
    if cell.bold() {
        fg = brighten(fg, 18);
    }
    if cell.dim() {
        fg = fade_toward(fg, bg, 110);
    }
    if selected {
        return (TERM_SELECTION_FG, TERM_SELECTION_BG);
    }
    if cursor_here {
        return (Color::rgb(9, 12, 18), CURSOR);
    }

    (fg, bg)
}

fn terminal_color(color: vt100::Color, default: Color, ansi_palette: &[Color; 16]) -> Color {
    match color {
        vt100::Color::Default => default,
        vt100::Color::Idx(idx) if idx < 16 => ansi_palette[idx as usize],
        vt100::Color::Idx(idx) => ansi_index_to_color(idx),
        vt100::Color::Rgb(r, g, b) => Color::rgb(r, g, b),
    }
}

const fn ansi_index_to_color_const(idx: u8) -> Color {
    let (r, g, b) = match idx {
        0 => (0x1d, 0x23, 0x2f),
        1 => (0xf7, 0x76, 0x8e),
        2 => (0x6f, 0xc7, 0x88),
        3 => (0xf4, 0xd0, 0x6b),
        4 => (0x7a, 0xa2, 0xf7),
        5 => (0xbb, 0x9a, 0xf7),
        6 => (0x7d, 0xd8, 0xe8),
        7 => (0xc8, 0xd3, 0xf5),
        8 => (0x44, 0x4b, 0x6a),
        9 => (0xff, 0x9e, 0xb0),
        10 => (0x8b, 0xde, 0xa3),
        11 => (0xfa, 0xe3, 0x8c),
        12 => (0x91, 0xb7, 0xff),
        13 => (0xce, 0xb6, 0xff),
        14 => (0x99, 0xe2, 0xf0),
        15 => (0xe5, 0xe9, 0xf0),
        16..=231 => {
            let index = idx - 16;
            let r = index / 36;
            let g = (index % 36) / 6;
            let b = index % 6;
            (cube_component(r), cube_component(g), cube_component(b))
        }
        232..=255 => {
            let shade = 8 + (idx - 232) * 10;
            (shade, shade, shade)
        }
    };
    Color::rgb(r, g, b)
}

const fn cube_component(component: u8) -> u8 {
    if component == 0 {
        0
    } else {
        55 + component * 40
    }
}

fn ansi_index_to_color(idx: u8) -> Color {
    ansi_index_to_color_const(idx)
}

pub(super) fn brighten(color: Color, amount: u8) -> Color {
    if color.ansi_index_value().is_some() {
        return color;
    }

    let (r, g, b) = color.rgb_components();
    Color::rgb(
        r.saturating_add(amount),
        g.saturating_add(amount),
        b.saturating_add(amount),
    )
}

pub(super) fn fade_toward(color: Color, target: Color, mix: u8) -> Color {
    if color.ansi_index_value().is_some() || target.ansi_index_value().is_some() {
        return if mix < 128 { color } else { target };
    }

    let (r1, g1, b1) = color.rgb_components();
    let (r2, g2, b2) = target.rgb_components();
    let blend = |a: u8, b: u8| -> u8 {
        let a = u16::from(a);
        let b = u16::from(b);
        let mix = u16::from(mix);
        (((a * (255 - mix)) + (b * mix)) / 255) as u8
    };
    Color::rgb(blend(r1, r2), blend(g1, g2), blend(b1, b2))
}

fn palette_accent(ansi: &[Color; 16], bg: Color, existing: &[Color]) -> Color {
    let mut best = None;
    let mut best_score = i32::MIN;

    for color in ansi.iter().copied().skip(1) {
        let candidate = adjust_semantic_color(color, bg);
        if !semantic_color_is_readable(candidate, bg) {
            continue;
        }
        let separated = existing.is_empty() || is_separated_from_all(candidate, existing);
        let separation_bonus = if separated { 800 } else { 0 };
        let nearest = existing
            .iter()
            .map(|other| rgb_distance_sq(candidate, *other))
            .min()
            .unwrap_or(semantic_distance_floor_sq() * 4)
            .min(16_384) as i32;
        let score = i32::from(rgb_chroma(candidate)) * 8
            + i32::from(luma_delta(candidate, bg)) * 4
            + nearest / 24
            + separation_bonus;

        if score > best_score {
            best_score = score;
            best = Some(candidate);
        }
    }

    best.unwrap_or_else(|| semantic_color(&[ACCENT_2, ACCENT], bg))
}
fn semantic_color(candidates: &[Color], bg: Color) -> Color {
    if let Some(indexed) = candidates
        .iter()
        .copied()
        .find(|color| color.ansi_index_value().is_some())
    {
        return indexed;
    }

    let first = candidates.first().copied().unwrap_or(ACCENT);
    candidates
        .iter()
        .copied()
        .map(|color| adjust_semantic_color(color, bg))
        .find(|color| semantic_color_is_readable(*color, bg))
        .unwrap_or_else(|| adjust_semantic_color(first, bg))
}

fn separated_semantic_color(candidates: &[Color], bg: Color, existing: &[Color]) -> Color {
    let color = semantic_color(candidates, bg);
    if is_separated_from_all(color, existing) {
        return color;
    }

    for target in candidates.iter().copied().skip(1) {
        let target = semantic_color(&[target], bg);
        for mix in [64, 96, 128, 160, 192, 224, 255] {
            let candidate = semantic_color(&[fade_toward(color, target, mix)], bg);
            if is_separated_from_all(candidate, existing) {
                return candidate;
            }
        }
    }

    color
}

fn adjust_semantic_color(color: Color, bg: Color) -> Color {
    let color = ensure_min_chroma(color);
    let color = ensure_min_luma_delta(color, bg);
    let color = ensure_min_chroma(color);
    ensure_min_luma_delta(color, bg)
}

fn semantic_color_is_readable(color: Color, bg: Color) -> bool {
    rgb_chroma(color) >= SEMANTIC_CHROMA_FLOOR && luma_delta(color, bg) >= SEMANTIC_LUMA_DELTA_FLOOR
}

fn ensure_min_chroma(color: Color) -> Color {
    if rgb_chroma(color) >= SEMANTIC_CHROMA_FLOOR {
        return color;
    }

    for factor in [2, 3, 4, 5, 6, 7, 8] {
        let candidate = boost_chroma(color, factor);
        if rgb_chroma(candidate) >= SEMANTIC_CHROMA_FLOOR {
            return candidate;
        }
    }

    color
}

fn boost_chroma(color: Color, factor: i16) -> Color {
    let (r, g, b) = color.rgb_components();
    let gray = i16::from(perceived_luma(color));
    Color::rgb(
        scale_channel_from_gray(r, gray, factor),
        scale_channel_from_gray(g, gray, factor),
        scale_channel_from_gray(b, gray, factor),
    )
}

fn scale_channel_from_gray(channel: u8, gray: i16, factor: i16) -> u8 {
    (gray + (i16::from(channel) - gray) * factor).clamp(0, 255) as u8
}

fn ensure_min_luma_delta(color: Color, bg: Color) -> Color {
    if luma_delta(color, bg) >= SEMANTIC_LUMA_DELTA_FLOOR {
        return color;
    }

    let target = if perceived_luma(bg) < 128 {
        WHITE
    } else {
        BLACK
    };
    for mix in [32, 64, 96, 128, 160, 192, 224, 255] {
        let candidate = fade_toward(color, target, mix);
        if luma_delta(candidate, bg) >= SEMANTIC_LUMA_DELTA_FLOOR {
            return candidate;
        }
    }

    target
}

fn is_separated_from_all(color: Color, existing: &[Color]) -> bool {
    existing
        .iter()
        .all(|other| rgb_distance_sq(color, *other) >= semantic_distance_floor_sq())
}

fn color_for_scoring(color: Color) -> Color {
    color
        .ansi_index_value()
        .map(ansi_index_to_color_const)
        .unwrap_or(color)
}

fn rgb_chroma(color: Color) -> u8 {
    let (r, g, b) = color_for_scoring(color).rgb_components();
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    max - min
}

fn perceived_luma(color: Color) -> u8 {
    let (r, g, b) = color_for_scoring(color).rgb_components();
    ((u32::from(r) * 299 + u32::from(g) * 587 + u32::from(b) * 114) / 1000) as u8
}

fn luma_delta(a: Color, b: Color) -> u8 {
    perceived_luma(a).abs_diff(perceived_luma(b))
}

fn rgb_distance_sq(a: Color, b: Color) -> u32 {
    let (ar, ag, ab) = color_for_scoring(a).rgb_components();
    let (br, bg, bb) = color_for_scoring(b).rgb_components();
    let dr = i32::from(ar) - i32::from(br);
    let dg = i32::from(ag) - i32::from(bg);
    let db = i32::from(ab) - i32::from(bb);
    (dr * dr + dg * dg + db * db) as u32
}

fn semantic_distance_floor_sq() -> u32 {
    let floor = i32::from(SEMANTIC_DISTANCE_FLOOR);
    (floor * floor) as u32
}

#[cfg(test)]
mod tests {
    use super::{
        ansi_index_to_color, brighten, fade_toward, luma_delta, rgb_chroma, rgb_distance_sq,
        screen_cell_colors, semantic_distance_floor_sq, DerivedTheme, TerminalPalette, CURSOR,
        SEMANTIC_CHROMA_FLOOR, SEMANTIC_LUMA_DELTA_FLOOR, TERM_FG, TERM_SELECTION_BG,
        TERM_SELECTION_FG, TEXT,
    };
    use crate::render::Color;
    fn cell_from_bytes(bytes: &[u8]) -> vt100::Cell {
        let mut parser = vt100::Parser::new(1, 8, 0);
        parser.process(bytes);
        parser.screen().cell(0, 0).unwrap().clone()
    }

    #[test]
    fn ansi_index_to_color_handles_base_cube_and_grayscale_ranges() {
        assert_eq!(ansi_index_to_color(1), Color::rgb(0xf7, 0x76, 0x8e));
        assert_eq!(ansi_index_to_color(16), Color::rgb(0, 0, 0));
        assert_eq!(ansi_index_to_color(21), Color::rgb(0, 0, 255));
        assert_eq!(ansi_index_to_color(51), Color::rgb(0, 255, 255));
        assert_eq!(ansi_index_to_color(231), Color::rgb(255, 255, 255));
        assert_eq!(ansi_index_to_color(232), Color::rgb(8, 8, 8));
        assert_eq!(ansi_index_to_color(255), Color::rgb(238, 238, 238));
    }

    #[test]
    fn brighten_and_fade_toward_adjust_channels_as_expected() {
        assert_eq!(
            brighten(Color::rgb(250, 1, 240), 20),
            Color::rgb(255, 21, 255)
        );
        assert_eq!(
            fade_toward(Color::rgb(10, 20, 30), Color::rgb(110, 120, 130), 0),
            Color::rgb(10, 20, 30)
        );
        assert_eq!(
            fade_toward(Color::rgb(10, 20, 30), Color::rgb(110, 120, 130), 255),
            Color::rgb(110, 120, 130)
        );
    }

    #[test]
    fn indexed_colors_survive_brighten_and_fade_without_becoming_rgb_blue() {
        let indexed = Color::ansi_index(12);
        let target = Color::rgb(100, 120, 140);

        assert_eq!(brighten(indexed, 20), indexed);
        assert_eq!(fade_toward(indexed, target, 64), indexed);
        assert_eq!(fade_toward(indexed, target, 192), target);
        assert_eq!(fade_toward(target, indexed, 192), indexed);
    }

    #[test]
    fn fallback_theme_uses_palette_accents_without_green_active_default() {
        let theme = DerivedTheme::fallback();
        let default_fg = Color::rgba(0, 0, 0, 0);

        assert_eq!(theme.text, default_fg);
        assert_eq!(theme.muted.ansi_index_value(), Some(8));
        assert!(theme.accent.ansi_index_value().is_some());
        assert!(theme.accent_2.ansi_index_value().is_some());
        assert_ne!(theme.running.ansi_index_value(), Some(10));
        assert_eq!(theme.warning.ansi_index_value(), Some(11));
        assert_eq!(theme.error.ansi_index_value(), Some(9));
        assert_eq!(theme.term_fg, default_fg);
    }

    #[test]
    fn color_components_extract_rgb_channels() {
        assert_eq!(
            Color::rgb(0x12, 0x34, 0x56).rgb_components(),
            (0x12, 0x34, 0x56)
        );
    }

    #[test]
    fn terminal_cell_colors_apply_selection_and_cursor_after_style_processing() {
        let bold_dim_inverse = cell_from_bytes(b"\x1b[1;2;7mX");
        let term_bg = Color::rgb(10, 13, 18);
        let ansi = TerminalPalette::fallback().ansi;
        let selected = screen_cell_colors(&bold_dim_inverse, false, true, TERM_FG, term_bg, &ansi);
        let cursor = screen_cell_colors(&bold_dim_inverse, true, false, TERM_FG, term_bg, &ansi);

        assert_eq!(selected, (TERM_SELECTION_FG, TERM_SELECTION_BG));
        assert_eq!(cursor, (Color::rgb(9, 12, 18), CURSOR));
    }

    #[test]
    fn terminal_cell_colors_apply_inverse_bold_and_dim_for_unselected_cells() {
        let plain = cell_from_bytes(b"X");
        let styled = cell_from_bytes(b"\x1b[31;44;1;2;7mX");
        let ansi = TerminalPalette::fallback().ansi;

        assert_eq!(
            screen_cell_colors(&plain, false, false, TERM_FG, Color::rgb(10, 13, 18), &ansi),
            (TERM_FG, Color::rgb(10, 13, 18))
        );
        assert_eq!(
            screen_cell_colors(
                &styled,
                false,
                false,
                TERM_FG,
                Color::rgb(10, 13, 18),
                &ansi
            ),
            (Color::ansi_index(4), Color::ansi_index(1))
        );
    }

    #[test]
    fn derived_theme_uses_scored_terminal_palette_accents() {
        let palette = TerminalPalette {
            fg: TEXT,
            bg: Color::rgb(0x10, 0x14, 0x20),
            ansi: std::array::from_fn(|index| ansi_index_to_color(index as u8)),
        };
        let theme = DerivedTheme::from_terminal_palette(palette);

        assert_ne!(theme.accent, palette.fg);
        assert_ne!(theme.accent, palette.ansi[10]);
        assert_ne!(theme.running, palette.ansi[10]);
        assert_eq!(theme.warning, palette.ansi[11]);
        assert_eq!(theme.error, palette.ansi[9]);
    }

    #[test]
    fn derived_theme_can_pick_accent_from_any_palette_slot() {
        let custom_accent = Color::rgb(0xff, 0x80, 0x20);
        let mut palette = TerminalPalette {
            fg: Color::rgb(0xd8, 0xd8, 0xd8),
            bg: Color::rgb(0x10, 0x10, 0x10),
            ansi: [Color::rgb(0x78, 0x78, 0x78); 16],
        };
        palette.ansi[2] = custom_accent;

        let theme = DerivedTheme::from_terminal_palette(palette);

        assert_eq!(theme.accent, custom_accent);
    }

    #[test]
    fn derived_theme_boosts_only_warning_and_error_for_monochrome_terminal_colors() {
        let gray = Color::rgb(0x78, 0x78, 0x78);
        let palette = TerminalPalette {
            fg: Color::rgb(0xd8, 0xd8, 0xd8),
            bg: Color::rgb(0x10, 0x10, 0x10),
            ansi: [gray; 16],
        };
        let theme = DerivedTheme::from_terminal_palette(palette);

        for color in [theme.accent, theme.running, theme.warning, theme.error] {
            assert!(rgb_chroma(color) >= SEMANTIC_CHROMA_FLOOR);
            assert!(luma_delta(color, palette.bg) >= SEMANTIC_LUMA_DELTA_FLOOR);
        }
        assert!(rgb_distance_sq(theme.warning, theme.error) >= semantic_distance_floor_sq());
    }
}
