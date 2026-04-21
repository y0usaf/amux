use super::*;

pub(super) const FONT_SIZE: f32 = 15.0;
pub(super) const UI_SCALE_DEFAULT: f32 = 1.0;
const UI_SCALE_MIN: f32 = 0.75;
const UI_SCALE_MAX: f32 = 3.0;
pub(super) const UI_SCALE_STEP: f32 = 0.125;
pub(super) const BG: Color = Color::rgb(8, 10, 14);
pub(super) const SURFACE: Color = Color::rgb(14, 17, 23);
pub(super) const SURFACE_ALT: Color = Color::rgb(18, 22, 29);
pub(super) const BORDER: Color = Color::rgb(42, 48, 60);
pub(super) const TEXT: Color = Color::rgb(222, 227, 234);
pub(super) const MUTED: Color = Color::rgb(124, 132, 146);
pub(super) const ACCENT: Color = Color::rgb(128, 164, 255);
pub(super) const RUNNING: Color = Color::rgb(116, 214, 168);
pub(super) const WARNING: Color = Color::rgb(245, 196, 108);
const ERROR: Color = Color::rgb(244, 133, 133);
pub(super) const SELECTION: Color = Color::rgb(28, 34, 46);
const CURSOR: Color = Color::rgb(188, 204, 255);
pub(super) const TERM_BG: Color = Color::rgb(10, 13, 18);
const TERM_FG: Color = Color::rgb(214, 219, 227);
const TERM_SELECTION_BG: Color = Color::rgb(53, 92, 173);
const TERM_SELECTION_FG: Color = Color::rgb(245, 248, 255);

pub(super) fn clamp_ui_scale(ui_scale: f32) -> f32 {
    ui_scale.clamp(UI_SCALE_MIN, UI_SCALE_MAX)
}

pub(super) fn status_color(
    session: Option<&Session>,
    terminal_status: Option<&TerminalStatus>,
) -> Color {
    if let Some(session) = session {
        if session.runtime.running {
            return RUNNING;
        }
        if session.runtime.queued {
            return WARNING;
        }
    }
    match terminal_status {
        Some(TerminalStatus::Error(_)) => ERROR,
        Some(TerminalStatus::Exited(_)) => WARNING,
        Some(TerminalStatus::Launching | TerminalStatus::Running) => ACCENT,
        Some(TerminalStatus::Empty) | None => MUTED,
    }
}

pub(super) fn terminal_cell_colors(
    cell: &vt100::Cell,
    cursor_here: bool,
    selected: bool,
) -> (Color, Color) {
    let mut fg = terminal_color(cell.fgcolor(), TERM_FG);
    let mut bg = terminal_color(cell.bgcolor(), TERM_BG);

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

fn terminal_color(color: vt100::Color, default: Color) -> Color {
    match color {
        vt100::Color::Default => default,
        vt100::Color::Idx(idx) => ansi_index_to_color(idx),
        vt100::Color::Rgb(r, g, b) => Color::rgb(r, g, b),
    }
}

fn ansi_index_to_color(idx: u8) -> Color {
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
            let conv = |component: u8| {
                if component == 0 {
                    0
                } else {
                    55 + component * 40
                }
            };
            (conv(r), conv(g), conv(b))
        }
        232..=255 => {
            let shade = 8 + (idx - 232) * 10;
            (shade, shade, shade)
        }
    };
    Color::rgb(r, g, b)
}

fn brighten(color: Color, amount: u8) -> Color {
    let (r, g, b) = color_components(color);
    Color::rgb(
        r.saturating_add(amount),
        g.saturating_add(amount),
        b.saturating_add(amount),
    )
}

fn fade_toward(color: Color, target: Color, mix: u8) -> Color {
    let (r1, g1, b1) = color_components(color);
    let (r2, g2, b2) = color_components(target);
    let blend = |a: u8, b: u8| -> u8 {
        let a = u16::from(a);
        let b = u16::from(b);
        let mix = u16::from(mix);
        (((a * (255 - mix)) + (b * mix)) / 255) as u8
    };
    Color::rgb(blend(r1, r2), blend(g1, g2), blend(b1, b2))
}

fn color_components(color: Color) -> (u8, u8, u8) {
    let value = color.argb();
    (
        ((value >> 16) & 0xff) as u8,
        ((value >> 8) & 0xff) as u8,
        (value & 0xff) as u8,
    )
}
