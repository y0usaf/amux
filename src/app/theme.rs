use crate::render::Color;
use crate::state::Session;
use crate::terminal::TerminalStatus;

pub(super) const FONT_SIZE: f32 = 15.0;
pub(super) const UI_SCALE_DEFAULT: f32 = 1.0;
const UI_SCALE_MIN: f32 = 0.75;
const UI_SCALE_MAX: f32 = 3.0;
pub(super) const UI_SCALE_STEP: f32 = 0.125;
pub(super) const BG: Color = Color::rgb(8, 10, 14);
pub(super) const SURFACE: Color = Color::rgb(14, 17, 23);
pub(super) const SURFACE_ALT: Color = Color::rgb(18, 22, 29);
pub(super) const BORDER: Color = Color::rgb(42, 48, 60);
pub(super) const TEXT: Color = ansi_index_to_color_const(7);
pub(super) const MUTED: Color = ansi_index_to_color_const(8);
pub(super) const ACCENT: Color = ansi_index_to_color_const(12);
pub(super) const RUNNING: Color = ansi_index_to_color_const(10);
pub(super) const WARNING: Color = ansi_index_to_color_const(11);
const ERROR: Color = ansi_index_to_color_const(9);
const CURSOR: Color = Color::rgb(188, 204, 255);
pub(super) const TERM_BG: Color = Color::rgb(10, 13, 18);
pub(super) const TERM_FG: Color = TEXT;
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

pub(super) fn screen_cell_colors(
    cell: &vt100::Cell,
    cursor_here: bool,
    selected: bool,
    default_fg: Color,
    default_bg: Color,
) -> (Color, Color) {
    let mut fg = terminal_color(cell.fgcolor(), default_fg);
    let mut bg = terminal_color(cell.bgcolor(), default_bg);

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

pub(super) fn theme_palette_index(color: Color) -> Option<u8> {
    if color == TEXT {
        Some(7)
    } else if color == MUTED {
        Some(8)
    } else if color == ERROR {
        Some(9)
    } else if color == RUNNING {
        Some(10)
    } else if color == WARNING {
        Some(11)
    } else if color == ACCENT {
        Some(12)
    } else {
        None
    }
}

fn terminal_color(color: vt100::Color, default: Color) -> Color {
    match color {
        vt100::Color::Default => default,
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

fn brighten(color: Color, amount: u8) -> Color {
    let (r, g, b) = color.rgb_components();
    Color::rgb(
        r.saturating_add(amount),
        g.saturating_add(amount),
        b.saturating_add(amount),
    )
}

fn fade_toward(color: Color, target: Color, mix: u8) -> Color {
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
        ansi_index_to_color, brighten, clamp_ui_scale, fade_toward, screen_cell_colors,
        status_color, ACCENT, CURSOR, ERROR, MUTED, RUNNING, TERM_BG, TERM_FG, TERM_SELECTION_BG,
        TERM_SELECTION_FG, UI_SCALE_MAX, UI_SCALE_MIN, WARNING,
    };
    use crate::render::Color;
    use crate::state::Session;
    use crate::terminal::TerminalStatus;

    fn session_with_runtime(running: bool, queued: bool) -> Session {
        let mut session = Session::new_draft();
        session.runtime.running = running;
        session.runtime.queued = queued;
        session
    }

    fn cell_from_bytes(bytes: &[u8]) -> vt100::Cell {
        let mut parser = vt100::Parser::new(1, 8, 0);
        parser.process(bytes);
        parser.screen().cell(0, 0).unwrap().clone()
    }

    #[test]
    fn clamp_ui_scale_respects_bounds_and_interior_values() {
        assert_eq!(clamp_ui_scale(UI_SCALE_MIN - 1.0), UI_SCALE_MIN);
        assert_eq!(clamp_ui_scale(1.5), 1.5);
        assert_eq!(clamp_ui_scale(UI_SCALE_MAX + 1.0), UI_SCALE_MAX);
    }

    #[test]
    fn status_color_prioritizes_session_runtime_over_terminal_status() {
        let running = session_with_runtime(true, false);
        let queued = session_with_runtime(false, true);

        assert_eq!(
            status_color(Some(&running), Some(&TerminalStatus::Error("boom".into()))),
            RUNNING
        );
        assert_eq!(
            status_color(Some(&queued), Some(&TerminalStatus::Running)),
            WARNING
        );
    }

    #[test]
    fn status_color_falls_back_to_terminal_status_when_session_is_idle() {
        let idle = session_with_runtime(false, false);

        assert_eq!(
            status_color(Some(&idle), Some(&TerminalStatus::Error("boom".into()))),
            ERROR
        );
        assert_eq!(
            status_color(None, Some(&TerminalStatus::Exited("0".into()))),
            WARNING
        );
        assert_eq!(status_color(None, Some(&TerminalStatus::Launching)), ACCENT);
        assert_eq!(status_color(None, Some(&TerminalStatus::Running)), ACCENT);
        assert_eq!(status_color(None, Some(&TerminalStatus::Empty)), MUTED);
        assert_eq!(status_color(None, None), MUTED);
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
        let selected = screen_cell_colors(&bold_dim_inverse, false, true, TERM_FG, TERM_BG);
        let cursor = screen_cell_colors(&bold_dim_inverse, true, false, TERM_FG, TERM_BG);

        assert_eq!(selected, (TERM_SELECTION_FG, TERM_SELECTION_BG));
        assert_eq!(cursor, (Color::rgb(9, 12, 18), CURSOR));
    }

    #[test]
    fn terminal_cell_colors_apply_inverse_bold_and_dim_for_unselected_cells() {
        let plain = cell_from_bytes(b"X");
        let styled = cell_from_bytes(b"\x1b[31;44;1;2;7mX");

        assert_eq!(
            screen_cell_colors(&plain, false, false, TERM_FG, TERM_BG),
            (TERM_FG, TERM_BG)
        );
        assert_eq!(
            screen_cell_colors(&styled, false, false, TERM_FG, TERM_BG),
            (Color::rgb(175, 143, 201), Color::rgb(247, 118, 142))
        );
    }
}
