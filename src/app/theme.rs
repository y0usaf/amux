use crate::render::Color;

pub(super) const TEXT: Color = ansi_index_to_color_const(7);
pub(super) const MUTED: Color = ansi_index_to_color_const(8);
pub(super) const ACCENT: Color = ansi_index_to_color_const(12);
pub(super) const ACCENT_2: Color = ansi_index_to_color_const(13);
pub(super) const SURFACE: Color = Color::rgb(0x12, 0x18, 0x24);
pub(super) const SURFACE_RAISED: Color = Color::rgb(0x18, 0x20, 0x31);
pub(super) const SIDEBAR_BG: Color = Color::rgba(0, 0, 0, 0);
pub(super) const STATUS_BG: Color = Color::rgb(0x24, 0x2d, 0x44);
pub(super) const BORDER: Color = Color::rgb(0x34, 0x3d, 0x59);
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
            fg: TEXT,
            bg: Color::rgba(0, 0, 0, 0),
            ansi: std::array::from_fn(|index| ansi_index_to_color(index as u8)),
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
        let accent = semantic_color(palette.ansi[12], ACCENT, bg);
        let accent_2 = separated_semantic_color(palette.ansi[13], ACCENT_2, bg, &[accent]);
        let running = separated_semantic_color(palette.ansi[10], RUNNING, bg, &[accent, accent_2]);
        let warning =
            separated_semantic_color(palette.ansi[11], WARNING, bg, &[accent, accent_2, running]);
        let error = separated_semantic_color(
            palette.ansi[9],
            ERROR,
            bg,
            &[accent, accent_2, running, warning],
        );
        Self {
            text,
            muted,
            accent,
            accent_2,
            surface: fade_toward(bg, accent, 18),
            surface_raised: fade_toward(bg, accent, 32),
            sidebar_bg: SIDEBAR_BG,
            status_bg: fade_toward(bg, accent, 46),
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
        Self {
            text: TEXT,
            muted: MUTED,
            accent: ACCENT,
            accent_2: ACCENT_2,
            surface: SURFACE,
            surface_raised: SURFACE_RAISED,
            sidebar_bg: SIDEBAR_BG,
            status_bg: STATUS_BG,
            border: BORDER,
            running: RUNNING,
            warning: WARNING,
            error: ERROR,
            term_fg: TERM_FG,
            term_bg: Color::rgba(0, 0, 0, 0),
            ansi: TerminalPalette::fallback().ansi,
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
    let (r, g, b) = color.rgb_components();
    Color::rgb(
        r.saturating_add(amount),
        g.saturating_add(amount),
        b.saturating_add(amount),
    )
}

pub(super) fn fade_toward(color: Color, target: Color, mix: u8) -> Color {
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

fn semantic_color(inherited: Color, fallback: Color, bg: Color) -> Color {
    let color = ensure_min_chroma(inherited, fallback);
    let color = ensure_min_luma_delta(color, bg);
    let color = ensure_min_chroma(color, fallback);
    ensure_min_luma_delta(color, bg)
}

fn separated_semantic_color(
    inherited: Color,
    fallback: Color,
    bg: Color,
    existing: &[Color],
) -> Color {
    let color = semantic_color(inherited, fallback, bg);
    if is_separated_from_all(color, existing) {
        return color;
    }

    for mix in [64, 96, 128, 160, 192, 224, 255] {
        let candidate = semantic_color(fade_toward(color, fallback, mix), fallback, bg);
        if is_separated_from_all(candidate, existing) {
            return candidate;
        }
    }

    color
}

fn ensure_min_chroma(color: Color, fallback: Color) -> Color {
    if rgb_chroma(color) >= SEMANTIC_CHROMA_FLOOR {
        return color;
    }

    for mix in [48, 80, 112, 144, 176, 208, 240, 255] {
        let candidate = fade_toward(color, fallback, mix);
        if rgb_chroma(candidate) >= SEMANTIC_CHROMA_FLOOR {
            return candidate;
        }
    }

    fallback
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

fn rgb_chroma(color: Color) -> u8 {
    let (r, g, b) = color.rgb_components();
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    max - min
}

fn perceived_luma(color: Color) -> u8 {
    let (r, g, b) = color.rgb_components();
    ((u32::from(r) * 299 + u32::from(g) * 587 + u32::from(b) * 114) / 1000) as u8
}

fn luma_delta(a: Color, b: Color) -> u8 {
    perceived_luma(a).abs_diff(perceived_luma(b))
}

fn rgb_distance_sq(a: Color, b: Color) -> u32 {
    let (ar, ag, ab) = a.rgb_components();
    let (br, bg, bb) = b.rgb_components();
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
        TERM_SELECTION_FG,
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
            (Color::rgb(175, 143, 201), Color::rgb(247, 118, 142))
        );
    }

    #[test]
    fn derived_theme_preserves_visible_distinct_terminal_semantic_colors() {
        let mut palette = TerminalPalette::fallback();
        palette.bg = Color::rgb(0x10, 0x14, 0x20);
        let theme = DerivedTheme::from_terminal_palette(palette);

        assert_eq!(theme.accent, palette.ansi[12]);
        assert_eq!(theme.accent_2, palette.ansi[13]);
        assert_eq!(theme.running, palette.ansi[10]);
        assert_eq!(theme.warning, palette.ansi[11]);
        assert_eq!(theme.error, palette.ansi[9]);
    }

    #[test]
    fn derived_theme_boosts_monochrome_terminal_semantic_colors() {
        let gray = Color::rgb(0x78, 0x78, 0x78);
        let palette = TerminalPalette {
            fg: Color::rgb(0xd8, 0xd8, 0xd8),
            bg: Color::rgb(0x10, 0x10, 0x10),
            ansi: [gray; 16],
        };
        let theme = DerivedTheme::from_terminal_palette(palette);
        let semantic = [
            theme.accent,
            theme.accent_2,
            theme.running,
            theme.warning,
            theme.error,
        ];

        for color in semantic {
            assert!(rgb_chroma(color) >= SEMANTIC_CHROMA_FLOOR);
            assert!(luma_delta(color, palette.bg) >= SEMANTIC_LUMA_DELTA_FLOOR);
        }
        for (index, color) in semantic.iter().enumerate() {
            for other in semantic.iter().skip(index + 1) {
                assert!(rgb_distance_sq(*color, *other) >= semantic_distance_floor_sq());
            }
        }
    }
}
