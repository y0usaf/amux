use crate::render::Color;
use crate::terminal::TerminalSelectionRange;

use super::backend::{ChromeView, FrameModel, StatusNoteKind};
use super::cell_surface::{
    display_cell_width, render_cell_scrollbar, truncate_to_cells, CellSurface,
};
use super::glyphs::GlyphSet;
use super::layout::{sidebar_content_rect, CellLayout, CellRect};
use super::sidebar::{
    crown_jewel_glyph, sidebar_status_glyph, SidebarRow, SidebarRowKind, SidebarStatusKind,
    SidebarViewportItem, SIDEBAR_ANIMATION_FRAME_MS,
};
use super::terminal_view::for_each_terminal_screen_cell;
use super::theme::{self, DerivedTheme, Role};
const STATUSLINE_MODE_LABEL_WIDTH: i32 = 9;
const STATUS_NOTE_OK_FG: Color = Color::rgb(0, 0, 0);
const STATUS_NOTE_ERROR_FG: Color = Color::rgb(255, 255, 255);

const SIDEBAR_SELECTOR_GLYPH: &str = "> ";

#[derive(Clone, Copy)]
pub(super) struct ScenePalette {
    pub(super) fg: Color,
    pub(super) bg: Color,
    pub(super) statusbar_bg: Color,
    pub(super) statusbar_fg: Color,
    pub(super) sidebar_bg: Color,
    pub(super) border: Color,
    pub(super) muted: Color,
    pub(super) heading: Color,
    pub(super) term_fg: Color,
    pub(super) term_bg: Color,
    pub(super) term_selection_fg: Color,
    pub(super) term_selection_bg: Color,
    pub(super) accent: Color,
    pub(super) accent_2: Color,

    pub(super) ansi: [Color; 16],

    pub(super) running: Color,
    pub(super) success: Color,
    pub(super) success_subtle: Color,
    pub(super) warning: Color,
    pub(super) error: Color,
    pub(super) monochrome: bool,

    pub(super) glyphs: GlyphSet,
}

impl ScenePalette {
    pub(super) fn themed(theme: DerivedTheme) -> Self {
        Self {
            fg: theme.text,
            bg: theme.term_bg,
            statusbar_fg: theme.status_fg,
            statusbar_bg: theme.status_bg,
            sidebar_bg: theme.sidebar_bg,
            border: theme.border,
            muted: theme.muted,
            heading: theme.heading,
            term_fg: theme.term_fg,
            term_bg: theme.term_bg,
            term_selection_fg: theme.term_selection_fg,
            term_selection_bg: theme.term_selection_bg,
            accent: theme.accent,
            accent_2: theme.accent_2,

            ansi: theme.ansi,

            running: theme.running,
            success: theme.success,
            success_subtle: theme.success,
            warning: theme.warning,
            error: theme.error,
            monochrome: false,
            glyphs: GlyphSet::unicode(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TerminalCursorMode {
    Cell,
    Hardware,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum HarnessMode {
    Normal,
    Command,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StatusbarState {
    Normal,
    Command,
    Ok,
    Error,
}

impl StatusbarState {
    fn resolve(mode: HarnessMode, chrome: &ChromeView) -> Self {
        if matches!(mode, HarnessMode::Command) {
            return Self::Command;
        }
        match chrome.status_kind {
            Some(StatusNoteKind::Ok) => Self::Ok,
            Some(StatusNoteKind::Error) => Self::Error,
            None => Self::Normal,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct HardwareCursor {
    pub(super) col: i32,
    pub(super) row: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TerminalSceneLayout {
    pub(super) card: CellRect,
    pub(super) terminal: CellRect,
    pub(super) scrollbar_col: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct HarnessSceneLayout {
    pub(super) statusbar: CellRect,
    pub(super) sidebar_panel: Option<CellRect>,
    pub(super) sidebar_content: CellRect,
    pub(super) terminal: TerminalSceneLayout,
}

pub(super) fn harness_scene_layout(layout: &CellLayout) -> HarnessSceneLayout {
    let scrollbar_col = if layout.terminal_card.cols > 0 {
        layout.terminal_card.col + layout.terminal_card.cols - 1
    } else {
        layout.terminal.col + layout.terminal.cols
    }
    .max(layout.terminal.col);

    HarnessSceneLayout {
        statusbar: layout.statusbar,
        sidebar_panel: (layout.sidebar.cols > 0 && layout.sidebar.rows > 0)
            .then_some(layout.sidebar),
        sidebar_content: sidebar_content_rect(layout.sidebar),
        terminal: TerminalSceneLayout {
            card: layout.terminal_card,
            terminal: layout.terminal,
            scrollbar_col,
        },
    }
}

pub(super) fn render_harness_scene(
    surface: &mut CellSurface,
    layout: HarnessSceneLayout,
    frame_model: &FrameModel<'_>,
    hovered_sidebar_row: Option<usize>,
    palette: &ScenePalette,
    cursor_mode: TerminalCursorMode,
    mode: HarnessMode,
    now_ms: u64,
) -> Option<HardwareCursor> {
    if let Some(sidebar_panel) = layout.sidebar_panel {
        render_sidebar(
            surface,
            sidebar_panel,
            layout.sidebar_content,
            &frame_model.sidebar_rows,
            &frame_model.sidebar_viewport,
            hovered_sidebar_row,
            palette,
            now_ms,
        );
    }
    let cursor = render_terminal(
        surface,
        layout.terminal,
        frame_model.terminal_screen,
        frame_model.terminal_selection,
        frame_model.terminal_max_scrollback,
        palette,
        cursor_mode,
    );
    render_statusbar(
        surface,
        layout.statusbar,
        layout.sidebar_panel,
        &frame_model.chrome,
        palette,
        mode,
    );
    cursor
}

pub(super) fn render_statusbar(
    surface: &mut CellSurface,
    panel: CellRect,
    sidebar_panel: Option<CellRect>,
    chrome: &ChromeView,
    palette: &ScenePalette,
    mode: HarnessMode,
) {
    if panel.cols <= 0 || panel.rows <= 0 {
        return;
    }

    let state = StatusbarState::resolve(mode, chrome);
    let statusline = CellRect::new(panel.col, panel.row, panel.cols, 1);
    let bg = statusbar_bg(state, &chrome.status, palette);
    let fg = statusbar_fg(state, palette);
    surface.fill_rect(statusline, fg, bg);

    let row = statusline.row;
    let main_col =
        render_statusbar_sidebar_segment(surface, statusline, sidebar_panel, palette, state, bg);
    let main_cols = (panel.col + panel.cols - main_col).max(0);
    if main_cols <= 0 {
        return;
    }

    let right = truncate_to_cells(&statusline_right_text(chrome, state), main_cols as usize);
    let right_cols = display_cell_width(&right) as i32;
    let left_max_cols = if right_cols > 0 {
        main_cols.saturating_sub(right_cols + 1)
    } else {
        main_cols
    };
    let left = truncate_to_cells(&statusline_left_text(chrome), left_max_cols as usize);
    let center = truncate_to_cells(&statusline_center_text(chrome), main_cols as usize);
    let left_cols = display_cell_width(&left) as i32;
    let center_cols = display_cell_width(&center) as i32;

    if left_cols > 0 {
        surface.put_text(main_col, row, left_cols, fg, bg, &left);
    }

    if right_cols > 0 {
        let right_fg = statusbar_right_fg(state, &chrome.status, palette);
        if matches!(state, StatusbarState::Ok | StatusbarState::Error) {
            surface.put_text_bold(
                main_col + main_cols - right_cols,
                row,
                right_cols,
                right_fg,
                bg,
                &right,
            );
        } else {
            surface.put_text(
                main_col + main_cols - right_cols,
                row,
                right_cols,
                right_fg,
                bg,
                &right,
            );
        }
    }

    if center_cols > 0 && center_cols <= main_cols {
        let centered_col = main_col + (main_cols - center_cols) / 2;
        let left_limit_col = main_col + left_cols + i32::from(left_cols > 0);
        let right_limit_col = main_col + main_cols - right_cols - i32::from(right_cols > 0);
        if centered_col >= left_limit_col && centered_col + center_cols <= right_limit_col {
            surface.put_text(centered_col, row, center_cols, fg, bg, &center);
        }
    }
}

fn statusbar_fg(state: StatusbarState, palette: &ScenePalette) -> Color {
    match state {
        StatusbarState::Command => palette.border,
        StatusbarState::Ok => STATUS_NOTE_OK_FG,
        StatusbarState::Error => STATUS_NOTE_ERROR_FG,
        StatusbarState::Normal => palette.statusbar_fg,
    }
}

fn statusbar_bg(state: StatusbarState, status: &str, palette: &ScenePalette) -> Color {
    match state {
        StatusbarState::Command => palette.warning,
        StatusbarState::Ok => palette.success_subtle,
        StatusbarState::Error => palette.error,
        StatusbarState::Normal if statusline_status_is_error(status) => palette.error,
        StatusbarState::Normal => palette.statusbar_bg,
    }
}

fn statusbar_right_fg(state: StatusbarState, status: &str, palette: &ScenePalette) -> Color {
    match state {
        StatusbarState::Ok => STATUS_NOTE_OK_FG,
        StatusbarState::Error => STATUS_NOTE_ERROR_FG,
        StatusbarState::Command => statusbar_fg(state, palette),
        StatusbarState::Normal => statusline_status_color(status, palette),
    }
}

fn render_statusbar_sidebar_segment(
    surface: &mut CellSurface,
    panel: CellRect,
    sidebar_panel: Option<CellRect>,
    palette: &ScenePalette,
    state: StatusbarState,
    status_bg: Color,
) -> i32 {
    let row = panel.row;
    let fg = statusbar_fg(state, palette);
    let Some(sidebar_panel) = sidebar_panel else {
        let mode_label = statusline_mode_label(state);
        surface.put_text_bold(
            panel.col,
            row,
            STATUSLINE_MODE_LABEL_WIDTH,
            fg,
            status_bg,
            &mode_label,
        );
        if let Some(rect) = statusbar_new_project_rect(panel, None) {
            let plus_fg = if matches!(state, StatusbarState::Command) {
                fg
            } else {
                palette.accent
            };
            surface.put_text(rect.col, rect.row, rect.cols, plus_fg, status_bg, "✚");
            return rect.col + rect.cols + 1;
        }
        return STATUSLINE_MODE_LABEL_WIDTH.min(panel.cols);
    };

    let sidebar_cols = sidebar_panel.cols.min(panel.cols).max(0);
    if sidebar_cols <= 0 {
        return panel.col;
    }

    let mode_label = statusline_mode_label(state);
    surface.put_text_bold(
        panel.col,
        row,
        STATUSLINE_MODE_LABEL_WIDTH.min(sidebar_cols),
        fg,
        status_bg,
        &mode_label,
    );

    let plus_rect = statusbar_new_project_rect(panel, Some(sidebar_panel));
    let separator_col = panel.col + sidebar_cols - 1;

    if let Some(rect) = plus_rect {
        let plus_fg = if matches!(state, StatusbarState::Command) {
            fg
        } else {
            palette.accent
        };
        surface.put_text(rect.col, rect.row, rect.cols, plus_fg, status_bg, "✚");
    }

    surface.set_cell(
        separator_col,
        row,
        theme::TRANSPARENT,
        status_bg,
        palette.glyphs.status_separator,
        false,
    );
    panel.col + sidebar_cols
}

#[allow(clippy::too_many_arguments)]
fn render_sidebar_scrollbar(
    surface: &mut CellSurface,
    col: i32,
    row: i32,
    rows: i32,
    visible_items: usize,
    total_items: usize,
    scroll_from_top: usize,
    track_fg: Color,
    track_bg: Color,
    thumb_fg: Color,
    glyphs: &GlyphSet,
) {
    if rows <= 0 {
        return;
    }

    for offset in 0..rows {
        surface.set_cell(
            col,
            row + offset,
            track_fg,
            track_bg,
            glyphs.scrollbar_track,
            false,
        );
    }

    if visible_items == 0 || total_items <= visible_items {
        return;
    }

    let thumb_rows =
        (((rows as i64 * visible_items as i64) / total_items as i64).max(1) as i32).min(rows);
    let max_scroll = total_items.saturating_sub(visible_items).max(1);
    let thumb_row = row
        + (((rows - thumb_rows).max(0) as i64 * scroll_from_top as i64) / max_scroll as i64) as i32;
    for offset in 0..thumb_rows {
        surface.set_cell(
            col,
            thumb_row + offset,
            thumb_fg,
            track_bg,
            glyphs.scrollbar_thumb,
            false,
        );
    }
}

pub(super) fn statusbar_new_project_rect(
    panel: CellRect,
    sidebar_panel: Option<CellRect>,
) -> Option<CellRect> {
    if panel.cols <= 0 || panel.rows <= 0 {
        return None;
    }

    let row = panel.row;
    let plus_col = sidebar_panel
        .map(|sidebar_panel| panel.col + sidebar_panel.cols.min(panel.cols).max(0) - 3)
        .unwrap_or(panel.col + STATUSLINE_MODE_LABEL_WIDTH + 1);
    let limit_col = sidebar_panel
        .map(|sidebar_panel| panel.col + sidebar_panel.cols.min(panel.cols).max(0) - 1)
        .unwrap_or(panel.col + panel.cols);

    (plus_col >= panel.col + STATUSLINE_MODE_LABEL_WIDTH && plus_col < limit_col)
        .then_some(CellRect::new(plus_col, row, 1, 1))
}

pub(super) fn statusline_mode_label(state: StatusbarState) -> String {
    let text = match state {
        StatusbarState::Command => "COMMAND",
        StatusbarState::Ok => "OKAY!",
        StatusbarState::Error => "ERROR!",
        StatusbarState::Normal => "NORMAL",
    };
    format!(" {:<7} ", text)
}

fn statusline_left_text(chrome: &ChromeView) -> String {
    format!(" {} ", chrome.project)
}

fn statusline_center_text(chrome: &ChromeView) -> String {
    if chrome.session.is_empty() {
        String::new()
    } else {
        format!(" {} ", chrome.session)
    }
}

fn statusline_right_text(chrome: &ChromeView, state: StatusbarState) -> String {
    if chrome.status.is_empty() {
        return String::new();
    }
    match state {
        StatusbarState::Ok => format!(" OKAY! {} ", chrome.status),
        StatusbarState::Error => format!(" ERROR! {} ", chrome.status),
        _ => format!(
            " {} {} ",
            statusline_status_symbol(&chrome.status),
            chrome.status
        ),
    }
}

fn statusline_status_symbol(status: &str) -> &'static str {
    match status {
        "thinking" => "⋯",
        "outputting" => "⟡",
        "running" | "launching" => "●",
        "error" | "interrupted" => "×",
        "exited" => "◌",
        _ if status.starts_with("tool") => "⚙",
        _ if status.contains("queued") => "▶",
        _ => "•",
    }
}

fn statusline_status_color(status: &str, palette: &ScenePalette) -> Color {
    match status {
        "thinking" | "outputting" | "running" | "launching" => palette.running,
        "error" | "interrupted" => palette.error,
        "exited" => palette.warning,
        _ if status.starts_with("tool") => palette.running,
        _ if status.contains("queued") => palette.warning,
        _ if statusline_status_is_error(status) => palette.statusbar_fg,
        _ => palette.accent_2,
    }
}

fn statusline_status_is_error(status: &str) -> bool {
    let status = status.to_ascii_lowercase();
    status == "error"
        || status.contains("error")
        || status.contains("unknown command")
        || status.contains("not found")
        || status.contains("usage:")
}

#[allow(clippy::too_many_arguments)]
pub(super) fn render_sidebar(
    surface: &mut CellSurface,
    panel: CellRect,
    mut content: CellRect,
    rows: &[SidebarRow],
    viewport: &[SidebarViewportItem],
    hovered_row_index: Option<usize>,
    palette: &ScenePalette,
    now_ms: u64,
) {
    surface.fill_rect(panel, palette.fg, palette.sidebar_bg);

    let visible_rows = content.rows.max(0) as usize;
    if visible_rows == 0 || content.cols <= 0 {
        return;
    }

    if content.cols > 1 {
        let scrollbar_col = content.col + content.cols - 1;
        render_sidebar_scrollbar(
            surface,
            scrollbar_col,
            content.row,
            content.rows,
            visible_rows,
            rows.len(),
            viewport_scroll_from_rows(viewport),
            theme::TRANSPARENT,
            palette.sidebar_bg,
            palette.accent_2,
            &palette.glyphs,
        );
        content.cols -= 1;
    }

    for item in viewport {
        let Some(row) = rows.get(item.row_index) else {
            continue;
        };
        if item.visible_row >= visible_rows || content.cols <= 0 {
            continue;
        }
        let row_y = content.row + item.visible_row as i32;
        let selected = row.inverted;
        let hovered = hovered_row_index == Some(item.row_index) && !selected;

        let sticky = item.sticky;
        let (row_fg, row_bg, reverse) = sidebar_row_style(row, selected, hovered, sticky, palette);
        surface.fill_rect(
            CellRect {
                col: content.col,
                row: row_y,
                cols: content.cols,
                rows: 1,
            },
            row_fg,
            row_bg,
        );

        match row.kind {
            SidebarRowKind::Label => {}
            SidebarRowKind::Project(_) => {
                render_sidebar_project_header(
                    surface,
                    CellRect::new(content.col, row_y, content.cols, 1),
                    &row.text,
                    row_bg,
                    row.status,
                    now_ms,
                    reverse,
                    selected,
                    hovered,
                    sticky,
                    row.current,
                    palette,
                );
            }

            SidebarRowKind::Session { .. } => {
                let status = row.status.map(|status| {
                    (
                        sidebar_status_glyph(&palette.glyphs, status, now_ms),
                        animated_sidebar_status_color(status, palette, now_ms, item.visible_row),
                    )
                });
                let inner_col = content.col + 1;
                let inner_cols = content.cols.saturating_sub(2);
                if inner_cols <= 0 {
                    continue;
                }
                let branch = if row.selector || hovered {
                    SIDEBAR_SELECTOR_GLYPH
                } else {
                    "  "
                };
                let indent_cols = inner_cols.min(display_cell_width(branch) as i32);
                let reserved_cells = status
                    .map(|(glyph, _)| display_cell_width(glyph) as i32 + 2)
                    .unwrap_or(0);
                let text_cols = inner_cols
                    .saturating_sub(indent_cols)
                    .saturating_sub(reserved_cells);
                let value = truncate_to_cells(&row.text, text_cols as usize);
                let branch_fg = if hovered {
                    palette.accent_2
                } else if selected {
                    palette.accent
                } else {
                    palette.border
                };
                surface.put_text_styled(
                    inner_col,
                    row_y,
                    indent_cols,
                    branch_fg,
                    row_bg,
                    branch,
                    reverse,
                );
                if matches!(
                    row.status,
                    Some(SidebarStatusKind::Active | SidebarStatusKind::Queued)
                ) {
                    render_sidebar_gradient_text(
                        surface,
                        CellRect::new(inner_col + indent_cols, row_y, text_cols, 1),
                        &value,
                        row_bg,
                        reverse,
                        palette.accent,
                        palette.accent_2,
                        row.status,
                        now_ms,
                        item.visible_row,
                        palette,
                    );
                } else {
                    let title_fg = if matches!(
                        row.status,
                        Some(
                            SidebarStatusKind::Interrupted
                                | SidebarStatusKind::Notification
                                | SidebarStatusKind::Input
                        )
                    ) {
                        sidebar_status_color_for_palette(row.status.unwrap(), palette)
                    } else {
                        row_fg
                    };
                    surface.put_text_styled(
                        inner_col + indent_cols,
                        row_y,
                        text_cols,
                        title_fg,
                        row_bg,
                        &value,
                        reverse,
                    );
                }

                if let Some((glyph, color)) = status {
                    let glyph_cols = display_cell_width(glyph) as i32;
                    let glyph_col = inner_col + inner_cols.saturating_sub(glyph_cols + 1);
                    surface.put_text_styled(
                        glyph_col, row_y, glyph_cols, color, row_bg, glyph, reverse,
                    );
                }
            }
        }
    }
}

fn render_sidebar_gradient_text(
    surface: &mut CellSurface,
    rect: CellRect,
    value: &str,
    bg: Color,
    reverse: bool,
    from: Color,
    to: Color,
    status: Option<SidebarStatusKind>,
    now_ms: u64,
    phase_slot: usize,
    palette: &ScenePalette,
) {
    if rect.cols <= 0 || value.is_empty() {
        return;
    }

    let value_cols = display_cell_width(value) as i32;
    let denom = value_cols.saturating_sub(1).max(1) as u16;

    let mut cursor = rect.col;
    for ch in value.chars() {
        if cursor >= rect.col + rect.cols {
            break;
        }
        let width = display_cell_width(&ch.to_string()) as i32;
        if cursor + width > rect.col + rect.cols {
            break;
        }
        let mix = (((cursor - rect.col) as u16 * 255) / denom) as u8;
        let mut fg = theme::fade_toward(from, to, mix);
        if let Some(SidebarStatusKind::Active | SidebarStatusKind::Queued) = status {
            let slot = phase_slot + (cursor - rect.col).max(0) as usize;
            let phase = ((now_ms / SIDEBAR_ANIMATION_FRAME_MS) as usize + slot) % 24;
            let intensity = if phase < 12 { phase } else { 24 - phase };
            let status_color = sidebar_status_color_for_palette(status.unwrap(), palette);
            fg = theme::fade_toward(fg, status_color, (intensity * 21).min(255) as u8);
        }
        let mut buf = [0; 4];
        surface.put_cell_span_styled(
            cursor,
            rect.row,
            width.max(1),
            ch.encode_utf8(&mut buf),
            fg,
            bg,
            false,
            reverse,
        );
        cursor += width.max(1);
    }
}

#[allow(clippy::too_many_arguments)]
fn render_sidebar_project_header(
    surface: &mut CellSurface,
    rect: CellRect,
    label: &str,
    bg: Color,
    status: Option<SidebarStatusKind>,
    now_ms: u64,
    reverse: bool,
    selected: bool,
    hovered: bool,
    sticky: bool,
    current: bool,
    palette: &ScenePalette,
) {
    if rect.cols <= 0 {
        return;
    }

    let status_color = status.map(|status| sidebar_status_color_for_palette(status, palette));
    // Flat rail-style header shared with the in-Pi right rail: state lives in
    // the jewel + title, the trailing rule stays border gray.
    let rule_fg = if palette.monochrome {
        palette.fg
    } else {
        palette.border
    };
    let jewel_fg = if palette.monochrome {
        palette.fg
    } else if selected {
        status_color.unwrap_or(palette.accent)
    } else {
        match status_color {
            Some(status_color) => status_color,
            None if current => palette.statusbar_fg,
            None => palette.border,
        }
    };
    let title_fg = if palette.monochrome {
        palette.fg
    } else if selected {
        palette.heading
    } else if let Some(status_color) = status_color {
        status_color
    } else if hovered || sticky {
        palette.fg
    } else {
        palette.accent
    };
    let jewel = crown_jewel_glyph(&palette.glyphs, status, now_ms);

    // Header layout: `{rule} {title} {jewel} ` (4 fixed cells + rule >= 1) --
    // the mirror of the rail's `| {jewel} {title} {rule}` panelHeader in
    // pi-extension/render.js, so both jewels sit against the Pi window.
    if rect.cols < 7 {
        let value = truncate_to_cells(&format!(" {label} "), rect.cols as usize);
        surface.put_text_bold_styled(rect.col, rect.row, rect.cols, title_fg, bg, &value, reverse);
        return;
    }
    let title_max = (rect.cols - 6).max(1) as usize;
    let value = truncate_to_cells(label, title_max);
    let value_cols = display_cell_width(&value) as i32;

    let mut cursor = rect.col;
    let lead_cols = (rect.cols - value_cols - 4).max(0);
    let lead = format!("{} ", palette.glyphs.header_rule.repeat(lead_cols as usize));
    surface.put_text_styled(cursor, rect.row, lead_cols + 1, rule_fg, bg, &lead, reverse);
    cursor += lead_cols + 1;
    surface.put_text_bold_styled(cursor, rect.row, value_cols, title_fg, bg, &value, reverse);
    cursor += value_cols;
    surface.put_text_styled(cursor, rect.row, 1, rule_fg, bg, " ", reverse);
    cursor += 1;
    surface.put_text_styled(cursor, rect.row, 1, jewel_fg, bg, jewel, reverse);
    cursor += 1;
    let trailing_cols = (rect.col + rect.cols - cursor).max(0);
    surface.put_text_styled(
        cursor,
        rect.row,
        trailing_cols,
        rule_fg,
        bg,
        &" ".repeat(trailing_cols as usize),
        reverse,
    );
}

fn animated_sidebar_status_color(
    status: SidebarStatusKind,
    palette: &ScenePalette,
    now_ms: u64,
    slot: usize,
) -> Color {
    let base = sidebar_status_color_for_palette(status, palette);
    match status {
        SidebarStatusKind::Interrupted
        | SidebarStatusKind::Notification
        | SidebarStatusKind::Input => base,
        SidebarStatusKind::Active | SidebarStatusKind::Queued => {
            let phase = ((now_ms / SIDEBAR_ANIMATION_FRAME_MS) as usize + slot) % 24;
            let intensity = if phase < 12 { phase } else { 24 - phase };
            let peak = match status {
                SidebarStatusKind::Active => palette.accent_2,
                SidebarStatusKind::Queued => palette.accent,
                SidebarStatusKind::Interrupted
                | SidebarStatusKind::Notification
                | SidebarStatusKind::Input => base,
            };
            theme::fade_toward(base, peak, (intensity * 10).min(180) as u8)
        }
    }
}

fn sidebar_status_color_for_palette(status: SidebarStatusKind, palette: &ScenePalette) -> Color {
    match status {
        SidebarStatusKind::Active => palette.running,
        SidebarStatusKind::Queued => palette.warning,
        SidebarStatusKind::Interrupted => palette.error,
        SidebarStatusKind::Notification => palette.success,
        SidebarStatusKind::Input => palette.accent_2,
    }
}

fn viewport_scroll_from_rows(viewport: &[SidebarViewportItem]) -> usize {
    viewport
        .iter()
        .find(|item| !item.sticky)
        .map(|item| item.row_index)
        .unwrap_or(0)
}

fn sidebar_row_style(
    row: &SidebarRow,
    selected: bool,
    hovered: bool,
    sticky: bool,
    palette: &ScenePalette,
) -> (Color, Color, bool) {
    if palette.monochrome {
        return (palette.fg, palette.bg, selected);
    }

    let fg = palette_color(row.fg, palette);
    let base_bg = row.bg.unwrap_or(palette.sidebar_bg);
    if selected {
        // Crush-style: bright fg, no bg highlight; selection is shown via the
        // accent indicator drawn separately on the leftmost cell of the row.
        return (palette.statusbar_fg, base_bg, false);
    }
    if hovered {
        // Hover only nudges fg, no bg highlight.
        return (palette.accent, base_bg, false);
    }
    if sticky {
        return (palette.accent, base_bg, false);
    }

    (fg, base_bg, false)
}

fn palette_color(role: Role, palette: &ScenePalette) -> Color {
    match role {
        Role::Text => palette.fg,
        Role::Muted => palette.muted,
        Role::Heading => palette.heading,
        Role::Accent => palette.accent,
        Role::Accent2 => palette.accent_2,
        Role::Border => palette.border,
        Role::Surface => palette.bg,
        Role::SurfaceRaised => palette.bg,
        Role::SidebarBg => palette.sidebar_bg,
        Role::StatusbarFg => palette.statusbar_fg,
        Role::StatusbarBg => palette.statusbar_bg,
        Role::Running => palette.running,
        Role::Success => palette.success,
        Role::Warning => palette.warning,
        Role::Error => palette.error,
    }
}
pub(super) fn render_terminal(
    surface: &mut CellSurface,
    layout: TerminalSceneLayout,
    screen: Option<&vt100::Screen>,
    selection: Option<TerminalSelectionRange>,
    max_scrollback: usize,
    palette: &ScenePalette,
    cursor_mode: TerminalCursorMode,
) -> Option<HardwareCursor> {
    surface.fill_rect(layout.card, palette.term_fg, palette.term_bg);
    if layout.terminal.rows <= 0 || layout.terminal.cols <= 0 {
        return None;
    }
    let Some(screen) = screen else {
        return None;
    };

    blit_terminal_screen(
        surface,
        layout.terminal,
        screen,
        selection,
        palette,
        cursor_mode == TerminalCursorMode::Cell,
    );
    render_terminal_scrollback(surface, layout, screen, max_scrollback, palette);

    match cursor_mode {
        TerminalCursorMode::Hardware => hardware_cursor_for_screen(layout.terminal, screen),
        TerminalCursorMode::Cell => None,
    }
}

fn blit_terminal_screen(
    surface: &mut CellSurface,
    rect: CellRect,
    screen: &vt100::Screen,
    selection: Option<TerminalSelectionRange>,
    palette: &ScenePalette,
    draw_cursor: bool,
) {
    for_each_terminal_screen_cell(
        screen,
        rect.rows.max(0) as u16,
        rect.cols.max(0) as u16,
        selection,
        palette.term_fg,
        palette.term_bg,
        palette.term_selection_fg,
        palette.term_selection_bg,
        &palette.ansi,
        draw_cursor,
        |cell| {
            surface.put_cell_span_terminal(
                rect.col + i32::from(cell.col),
                rect.row + i32::from(cell.row),
                i32::from(cell.span),
                cell.text,
                cell.fg,
                cell.bg,
                cell.bold,
                cell.underline,
                false,
            );
        },
    );
}

fn render_terminal_scrollback(
    surface: &mut CellSurface,
    layout: TerminalSceneLayout,
    screen: &vt100::Screen,
    max_scroll: usize,
    palette: &ScenePalette,
) {
    let visible_rows = usize::from(screen.size().0);
    if visible_rows == 0 || max_scroll == 0 || layout.terminal.rows <= 0 {
        return;
    }

    render_cell_scrollbar(
        surface,
        layout.scrollbar_col,
        layout.terminal.row,
        layout.terminal.rows,
        visible_rows,
        visible_rows.saturating_add(max_scroll),
        max_scroll.saturating_sub(screen.scrollback()),
        palette.border,
        palette.term_bg,
        palette.glyphs.scrollbar_track,
        palette.accent_2,
        palette.glyphs.scrollbar_thumb,
    );
}

fn hardware_cursor_for_screen(rect: CellRect, screen: &vt100::Screen) -> Option<HardwareCursor> {
    if screen.scrollback() != 0 || screen.hide_cursor() || rect.cols <= 0 || rect.rows <= 0 {
        return None;
    }

    let (cursor_row, cursor_col) = screen.cursor_position();
    let col = rect.col + i32::from(cursor_col).min(rect.cols.saturating_sub(1));
    let row = rect.row + i32::from(cursor_row).min(rect.rows.saturating_sub(1));
    Some(HardwareCursor { col, row })
}
