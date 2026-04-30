use crate::render::Color;

const DEFAULT: Color = Color::rgba(0, 0, 0, 0);

// CharmTone Pantera palette, mirrored from Crush's curated dark theme.
const CHARPLE: Color = Color::rgb(0x6b, 0x50, 0xff);
const DOLLY: Color = Color::rgb(0xff, 0x60, 0xff);
const BLUSH: Color = Color::rgb(0xff, 0x84, 0xff);
const BOK: Color = Color::rgb(0x68, 0xff, 0xd6);
const SRIRACHA: Color = Color::rgb(0xeb, 0x42, 0x68);
const CORAL: Color = Color::rgb(0xff, 0x57, 0x7d);
const MALIBU: Color = Color::rgb(0x00, 0xa4, 0xff);
const ANCHOVY: Color = Color::rgb(0x71, 0x9a, 0xfc);
const SARDINE: Color = Color::rgb(0x4f, 0xbe, 0xfe);
const GUAC: Color = Color::rgb(0x12, 0xc7, 0x8f);
const JULEP: Color = Color::rgb(0x00, 0xff, 0xb2);
const MUSTARD: Color = Color::rgb(0xf5, 0xef, 0x34);
const CITRON: Color = Color::rgb(0xe8, 0xff, 0x27);
const PEPPER: Color = Color::rgb(0x20, 0x1f, 0x26);
const BBQ: Color = Color::rgb(0x2d, 0x2c, 0x35);
const CHARCOAL: Color = Color::rgb(0x3a, 0x39, 0x43);
const IRON: Color = Color::rgb(0x4d, 0x4c, 0x57);
const SQUID: Color = Color::rgb(0x85, 0x83, 0x92);
const SMOKE: Color = Color::rgb(0xbf, 0xbc, 0xc8);
const ASH: Color = Color::rgb(0xdf, 0xdb, 0xdd);
const BUTTER: Color = Color::rgb(0xff, 0xfa, 0xf1);

pub(super) const TEXT: Color = ASH;
pub(super) const MUTED: Color = SQUID;
pub(super) const HEADING: Color = SMOKE;
pub(super) const ACCENT: Color = BOK;
pub(super) const ACCENT_2: Color = DOLLY;
pub(super) const SURFACE: Color = PEPPER;
pub(super) const SURFACE_RAISED: Color = BBQ;
pub(super) const SIDEBAR_BG: Color = DEFAULT;
pub(super) const STATUS_FG: Color = BUTTER;
pub(super) const STATUS_BG: Color = CHARPLE;
pub(super) const BORDER: Color = CHARCOAL;
pub(super) const RUNNING: Color = CITRON;
pub(super) const WARNING: Color = MUSTARD;
pub(super) const ERROR: Color = SRIRACHA;
const CURSOR: Color = DOLLY;
pub(super) const TERM_FG: Color = TEXT;
const TERM_SELECTION_BG: Color = CHARPLE;
const TERM_SELECTION_FG: Color = BUTTER;

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
    pub(super) status_fg: Color,
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
    pub(super) fn from_terminal_palette(_palette: TerminalPalette) -> Self {
        // Crush-style chrome is intentionally hardcoded; keep backgrounds as
        // terminal defaults so transparency / host background still show through.
        Self::fallback()
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
            status_fg: STATUS_FG,
            status_bg: STATUS_BG,
            border: BORDER,
            running: RUNNING,
            warning: WARNING,
            error: ERROR,
            term_fg: TERM_FG,
            term_bg: DEFAULT,
            ansi: charm_ansi_palette(),
        }
    }
}

const fn charm_ansi_palette() -> [Color; 16] {
    [
        PEPPER, SRIRACHA, JULEP, MUSTARD, MALIBU, DOLLY, BOK, SMOKE, IRON, CORAL, GUAC, CITRON,
        ANCHOVY, BLUSH, SARDINE, ASH,
    ]
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

#[cfg(test)]
mod tests {
    use super::{
        ansi_index_to_color, brighten, fade_toward, screen_cell_colors, DerivedTheme,
        TerminalPalette, ACCENT, ACCENT_2, BORDER, CURSOR, DEFAULT, ERROR, MUTED, RUNNING,
        STATUS_BG, STATUS_FG, TERM_FG, TERM_SELECTION_BG, TERM_SELECTION_FG, TEXT, WARNING,
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
    fn fallback_theme_matches_crush_charmtone_roles() {
        let theme = DerivedTheme::fallback();

        assert_eq!(theme.text, TEXT);
        assert_eq!(theme.muted, MUTED);
        assert_eq!(theme.accent, ACCENT);
        assert_eq!(theme.accent_2, ACCENT_2);
        assert_eq!(theme.status_fg, STATUS_FG);
        assert_eq!(theme.status_bg, STATUS_BG);
        assert_eq!(theme.border, BORDER);
        assert_eq!(theme.running, RUNNING);
        assert_eq!(theme.warning, WARNING);
        assert_eq!(theme.error, ERROR);
        assert_eq!(theme.term_fg, TERM_FG);
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
    fn derived_theme_keeps_crush_chrome_and_terminal_default_background() {
        let palette = TerminalPalette {
            fg: Color::rgb(0xd8, 0xd8, 0xd8),
            bg: Color::rgb(0x10, 0x14, 0x20),
            ansi: std::array::from_fn(|index| ansi_index_to_color(index as u8)),
        };
        let theme = DerivedTheme::from_terminal_palette(palette);

        assert_eq!(theme.text, TEXT);
        assert_eq!(theme.accent, ACCENT);
        assert_eq!(theme.status_fg, STATUS_FG);
        assert_eq!(theme.status_bg, STATUS_BG);
        assert_eq!(theme.term_bg, DEFAULT);
        assert_eq!(theme.ansi, DerivedTheme::fallback().ansi);
    }
}
