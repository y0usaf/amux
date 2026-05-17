// The theme registry intentionally exposes the full Charmtone role set even
// when current render code only consumes a subset; new sites are expected to
// reach for the typed roles directly rather than hard-coding hexes.
#![allow(dead_code)]

//! Theme registry + derived theme tables.
//!
//! Currently ships a single Charmtone-derived theme (Pantera). Role-tag
//! constants (`TEXT`, `MUTED`, …) are stable sentinels used by the
//! scene/sidebar code to look up a palette colour at render time.

use crate::render::Color;

pub(super) mod charmtone;
mod pantera;

pub(super) const TRANSPARENT: Color = Color::rgba(0, 0, 0, 0);

// Role-tag sentinels. The actual rendered colour is resolved through
// `palette_color()` against the active `ScenePalette`.
pub(super) const TEXT: Color = charmtone::ASH;
pub(super) const MUTED: Color = charmtone::SQUID;
pub(super) const HEADING: Color = charmtone::SMOKE;
pub(super) const ACCENT: Color = charmtone::BOK;
pub(super) const ACCENT_2: Color = charmtone::DOLLY;
pub(super) const RUNNING: Color = charmtone::CITRON;
pub(super) const WARNING: Color = charmtone::MUSTARD;
pub(super) const ERROR: Color = charmtone::SRIRACHA;
pub(super) const SUCCESS: Color = charmtone::JULEP;
pub(super) const SUCCESS_SUBTLE: Color = charmtone::GUAC;
pub(super) const STATUS_FG: Color = charmtone::BUTTER;
pub(super) const STATUS_BG: Color = charmtone::CHARPLE;
pub(super) const BORDER: Color = charmtone::CHARCOAL;
pub(super) const SURFACE: Color = charmtone::PEPPER;
pub(super) const SURFACE_RAISED: Color = charmtone::BBQ;
pub(super) const TERM_FG: Color = TEXT;
const CURSOR: Color = charmtone::DOLLY;
const TERM_SELECTION_BG: Color = charmtone::CHARPLE;
const TERM_SELECTION_FG: Color = charmtone::BUTTER;

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
            bg: TRANSPARENT,
            ansi: std::array::from_fn(|index| Color::ansi_index(index as u8)),
        }
    }
}

/// Fully resolved theme table.
///
/// Field set is a superset of Crush's `quickStyleOpts` roles plus a handful
/// of legacy aliases used by the existing render code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct DerivedTheme {
    // Crush role vocabulary.
    pub(super) primary: Color,
    pub(super) secondary: Color,
    pub(super) accent: Color,
    pub(super) keyword: Color,
    pub(super) on_primary: Color,

    pub(super) fg_base: Color,
    pub(super) fg_subtle: Color,
    pub(super) fg_more_subtle: Color,
    pub(super) fg_most_subtle: Color,

    pub(super) bg_base: Color,
    pub(super) bg_least_visible: Color,
    pub(super) bg_less_visible: Color,
    pub(super) bg_most_visible: Color,

    pub(super) separator: Color,

    pub(super) destructive: Color,
    pub(super) error: Color,
    pub(super) warning: Color,
    pub(super) warning_subtle: Color,
    pub(super) busy: Color,
    pub(super) info: Color,
    pub(super) info_more_subtle: Color,
    pub(super) info_most_subtle: Color,
    pub(super) success: Color,
    pub(super) success_more_subtle: Color,
    pub(super) success_most_subtle: Color,

    // Legacy aliases retained for the current scene/overlay code.
    pub(super) text: Color,
    pub(super) muted: Color,
    pub(super) accent_2: Color,
    pub(super) surface: Color,
    pub(super) surface_raised: Color,
    pub(super) sidebar_bg: Color,
    pub(super) status_fg: Color,
    pub(super) status_bg: Color,
    pub(super) border: Color,
    pub(super) running: Color,
    pub(super) success_subtle: Color,
    pub(super) term_fg: Color,
    pub(super) term_bg: Color,

    pub(super) ansi: [Color; 16],
}

impl DerivedTheme {
    pub(super) fn fallback() -> Self {
        pantera::theme()
    }

    /// Crush-style chrome is intentionally hardcoded; the terminal palette is
    /// ignored so transparency / host background still show through. Kept
    /// here so callers can re-resolve the theme on terminal palette events.
    pub(super) fn from_terminal_palette(_palette: TerminalPalette) -> Self {
        Self::fallback()
    }
}
pub(super) fn default_ansi_palette() -> [Color; 16] {
    [
        charmtone::PEPPER,
        charmtone::SRIRACHA,
        charmtone::JULEP,
        charmtone::MUSTARD,
        charmtone::MALIBU,
        charmtone::DOLLY,
        charmtone::BOK,
        charmtone::SMOKE,
        charmtone::IRON,
        charmtone::CORAL,
        charmtone::GUAC,
        charmtone::CITRON,
        charmtone::ANCHOVY,
        charmtone::BLUSH,
        charmtone::SARDINE,
        charmtone::ASH,
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
        ansi_index_to_color, brighten, default_ansi_palette, fade_toward, screen_cell_colors,
        DerivedTheme, TerminalPalette, ACCENT, ACCENT_2, BORDER, CURSOR, ERROR, MUTED, RUNNING,
        STATUS_BG, STATUS_FG, SUCCESS, SUCCESS_SUBTLE, TERM_FG, TERM_SELECTION_BG,
        TERM_SELECTION_FG, TEXT, TRANSPARENT, WARNING,
    };

    use super::charmtone as ct;
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
    fn pantera_theme_matches_charmtone_role_mapping() {
        let theme = DerivedTheme::fallback();

        // Brand
        assert_eq!(theme.primary, ct::CHARPLE);
        assert_eq!(theme.secondary, ct::DOLLY);
        assert_eq!(theme.accent, ct::BOK);
        assert_eq!(theme.keyword, ct::BLUSH);
        assert_eq!(theme.on_primary, ct::BUTTER);

        // Foreground tiers
        assert_eq!(theme.fg_base, ct::ASH);
        assert_eq!(theme.fg_subtle, ct::SMOKE);
        assert_eq!(theme.fg_more_subtle, ct::SQUID);
        assert_eq!(theme.fg_most_subtle, ct::OYSTER);

        // Background tiers
        assert_eq!(theme.bg_base, ct::PEPPER);
        assert_eq!(theme.bg_least_visible, ct::BBQ);
        assert_eq!(theme.bg_less_visible, ct::CHARCOAL);
        assert_eq!(theme.bg_most_visible, ct::IRON);

        assert_eq!(theme.separator, ct::CHARCOAL);

        // Status
        assert_eq!(theme.destructive, ct::CORAL);
        assert_eq!(theme.error, ct::SRIRACHA);
        assert_eq!(theme.warning, ct::MUSTARD);
        assert_eq!(theme.warning_subtle, ct::ZEST);
        assert_eq!(theme.busy, ct::CITRON);
        assert_eq!(theme.info, ct::MALIBU);
        assert_eq!(theme.info_more_subtle, ct::SARDINE);
        assert_eq!(theme.info_most_subtle, ct::DAMSON);
        assert_eq!(theme.success, ct::JULEP);
        assert_eq!(theme.success_more_subtle, ct::BOK);
        assert_eq!(theme.success_most_subtle, ct::GUAC);
    }

    #[test]
    fn legacy_role_aliases_match_pantera_constants() {
        let theme = DerivedTheme::fallback();

        assert_eq!(theme.text, TEXT);
        assert_eq!(theme.muted, MUTED);
        assert_eq!(theme.accent, ACCENT);
        assert_eq!(theme.accent_2, ACCENT_2);
        assert_eq!(theme.status_fg, STATUS_FG);
        assert_eq!(theme.status_bg, STATUS_BG);
        assert_eq!(theme.border, BORDER);
        assert_eq!(theme.running, RUNNING);
        assert_eq!(theme.success, SUCCESS);
        assert_eq!(theme.success_subtle, SUCCESS_SUBTLE);
        assert_eq!(theme.warning, WARNING);
        assert_eq!(theme.error, ERROR);
        assert_eq!(theme.term_fg, TERM_FG);
        assert_eq!(theme.sidebar_bg, TRANSPARENT);
        assert_eq!(theme.term_bg, TRANSPARENT);
        assert_eq!(theme.ansi, default_ansi_palette());
    }

    #[test]
    fn color_components_extract_rgb_channels() {
        assert_eq!(
            Color::rgb(0x12, 0x34, 0x56).rgb_components(),
            (0x12, 0x34, 0x56)
        );
    }

    #[test]
    fn negative_color_inverts_rgb_channels_and_leaves_ansi_values_alone() {
        assert_eq!(
            Color::rgb(0x12, 0x34, 0x56).negative(),
            Color::rgb(0xed, 0xcb, 0xa9)
        );
        assert_eq!(Color::ansi_index(12).negative(), Color::ansi_index(12));
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
    fn from_terminal_palette_keeps_pantera_chrome() {
        let palette = TerminalPalette {
            fg: Color::rgb(0xd8, 0xd8, 0xd8),
            bg: Color::rgb(0x10, 0x14, 0x20),
            ansi: std::array::from_fn(|index| ansi_index_to_color(index as u8)),
        };
        assert_eq!(
            DerivedTheme::from_terminal_palette(palette),
            DerivedTheme::fallback()
        );
    }
}
