//! Pi theme roles and resolved colours.
use crate::render::Color;

pub(super) const TRANSPARENT: Color = Color::rgba(0, 0, 0, 0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Role {
    Text,
    Muted,
    Heading,
    Accent,
    Accent2,
    Border,
    Surface,
    SurfaceRaised,
    SidebarBg,
    StatusbarFg,
    StatusbarBg,
    Running,
    Success,
    Warning,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct DerivedTheme {
    pub(super) text: Color,
    pub(super) muted: Color,
    pub(super) heading: Color,
    pub(super) accent: Color,
    pub(super) accent_2: Color,
    pub(super) border: Color,
    pub(super) surface: Color,
    pub(super) surface_raised: Color,
    pub(super) sidebar_bg: Color,
    pub(super) status_fg: Color,
    pub(super) status_bg: Color,
    pub(super) running: Color,
    pub(super) success: Color,
    pub(super) warning: Color,
    pub(super) error: Color,
    pub(super) term_fg: Color,
    pub(super) term_bg: Color,
    pub(super) cursor: Color,
    pub(super) term_selection_bg: Color,
    pub(super) term_selection_fg: Color,
    pub(super) ansi: [Color; 16],
}
impl DerivedTheme {
    pub(super) fn from_roles(r: [Color; 15]) -> Self {
        Self {
            text: r[0],
            muted: r[1],
            heading: r[2],
            accent: r[3],
            accent_2: r[4],
            border: r[5],
            surface: r[6],
            surface_raised: r[7],
            sidebar_bg: r[8],
            status_fg: r[9],
            status_bg: r[10],
            running: r[11],
            success: r[12],
            warning: r[13],
            error: r[14],
            term_fg: TRANSPARENT,
            term_bg: TRANSPARENT,
            cursor: r[3],
            term_selection_bg: r[7],
            term_selection_fg: r[0],
            ansi: std::array::from_fn(|i| Color::ansi_index(i as u8)),
        }
    }
    pub(super) fn fallback() -> Self {
        Self::from_roles(crate::app::theme::pi_defaults::dark())
    }
}

pub(super) fn screen_cell_colors(
    cell: &vt100::Cell,
    cursor_here: bool,
    selected: bool,
    default_fg: Color,
    default_bg: Color,
    selection_fg: Color,
    selection_bg: Color,
    ansi_palette: &[Color; 16],
) -> (Color, Color) {
    let mut fg = terminal_color(cell.fgcolor(), default_fg, ansi_palette);
    let mut bg = terminal_color(cell.bgcolor(), default_bg, ansi_palette);
    if cell.inverse() {
        std::mem::swap(&mut fg, &mut bg)
    }
    // The renderer has no dim attribute, so this approximates dim by fading its colour.
    if cell.dim() {
        fg = fade_toward(fg, bg, 110)
    }
    if selected {
        return (selection_fg, selection_bg);
    }
    if cursor_here {
        return (Color::rgb(9, 12, 18), Color::ansi_index(0));
    }
    (fg, bg)
}
fn terminal_color(c: vt100::Color, d: Color, a: &[Color; 16]) -> Color {
    match c {
        vt100::Color::Default => d,
        vt100::Color::Idx(i) if i < 16 => a[i as usize],
        vt100::Color::Idx(i) => ansi_index_to_color(i),
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
            let x = idx - 16;
            (
                cube_component(x / 36),
                cube_component((x % 36) / 6),
                cube_component(x % 6),
            )
        }
        232..=255 => {
            let s = 8 + (idx - 232) * 10;
            (s, s, s)
        }
    };
    Color::rgb(r, g, b)
}
const fn cube_component(c: u8) -> u8 {
    if c == 0 {
        0
    } else {
        55 + c * 40
    }
}
fn ansi_index_to_color(i: u8) -> Color {
    ansi_index_to_color_const(i)
}
pub(super) fn brighten(c: Color, n: u8) -> Color {
    if c.ansi_index_value().is_some() {
        return c;
    }
    let (r, g, b) = c.rgb_components();
    Color::rgb(
        r.saturating_add(n),
        g.saturating_add(n),
        b.saturating_add(n),
    )
}
pub(super) fn fade_toward(c: Color, t: Color, m: u8) -> Color {
    if c.ansi_index_value().is_some() || t.ansi_index_value().is_some() {
        return if m < 128 { c } else { t };
    }
    let (r1, g1, b1) = c.rgb_components();
    let (r2, g2, b2) = t.rgb_components();
    let f = |a: u8, b: u8| ((u16::from(a) * u16::from(255 - m) + u16::from(b) * u16::from(m)) / 255) as u8;
    Color::rgb(f(r1, r2), f(g1, g2), f(b1, b2))
}

pub(super) mod pi_defaults;
