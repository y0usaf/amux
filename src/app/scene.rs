use crate::render::Color;
use crate::terminal::TerminalSelectionRange;

use super::backend::{ChromeView, FrameModel};
use super::cell_surface::{
    display_cell_width, render_cell_scrollbar, truncate_to_cells, CellSurface,
};
use super::layout::{sidebar_content_rect, CellLayout, CellRect};
use super::sidebar::{
    sidebar_status_color, sidebar_status_glyph, SidebarRow, SidebarRowKind, SidebarViewportItem,
};
use super::terminal_view::{terminal_max_scrollback, terminal_screen_cells};

const SCROLLBAR_TRACK_GLYPH: &str = "▕";
const SCROLLBAR_THUMB_GLYPH: &str = "▐";

#[derive(Clone, Copy)]
pub(super) struct ScenePalette {
    pub(super) fg: Color,
    pub(super) bg: Color,
    pub(super) statusbar_bg: Color,
    pub(super) sidebar_bg: Color,
    pub(super) border: Color,
    pub(super) muted: Color,
    pub(super) term_fg: Color,
    pub(super) term_bg: Color,
    pub(super) monochrome: bool,
}

impl ScenePalette {
    pub(super) fn monochrome(fg: Color, bg: Color, term_fg: Color, term_bg: Color) -> Self {
        Self {
            fg,
            bg,
            statusbar_bg: bg,
            sidebar_bg: bg,
            border: fg,
            muted: fg,
            term_fg,
            term_bg,
            monochrome: true,
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

#[allow(clippy::too_many_arguments)]
pub(super) fn render_harness_scene(
    surface: &mut CellSurface,
    layout: HarnessSceneLayout,
    frame_model: &FrameModel,
    hovered_sidebar_row: Option<usize>,
    palette: &ScenePalette,
    cursor_mode: TerminalCursorMode,
    mode: HarnessMode,
    footer_hint: Option<&str>,
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
        &frame_model.terminal_screen,
        frame_model.terminal_selection,
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
        footer_hint,
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
    footer_hint: Option<&str>,
) {
    if panel.cols <= 0 || panel.rows <= 0 {
        return;
    }

    let statusline = CellRect::new(panel.col, panel.row, panel.cols, 1);
    surface.fill_rect(statusline, palette.fg, palette.statusbar_bg);
    if panel.rows > 1 {
        surface.fill_rect(
            CellRect::new(panel.col, panel.row + 1, panel.cols, panel.rows - 1),
            palette.fg,
            palette.bg,
        );
    }

    let row = statusline.row;
    let main_col =
        render_statusbar_sidebar_segment(surface, statusline, sidebar_panel, palette, mode);
    let main_cols = (panel.col + panel.cols - main_col).max(0);
    if main_cols <= 0 {
        return;
    }

    let right = truncate_to_cells(&statusline_right_text(chrome), main_cols as usize);
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
        surface.put_text(
            main_col,
            row,
            left_cols,
            palette.fg,
            palette.statusbar_bg,
            &left,
        );
    }

    if right_cols > 0 {
        surface.put_text(
            main_col + main_cols - right_cols,
            row,
            right_cols,
            palette.fg,
            palette.statusbar_bg,
            &right,
        );
    }

    if center_cols > 0 && center_cols <= main_cols {
        let centered_col = main_col + (main_cols - center_cols) / 2;
        let left_limit_col = main_col + left_cols + i32::from(left_cols > 0);
        let right_limit_col = main_col + main_cols - right_cols - i32::from(right_cols > 0);
        if centered_col >= left_limit_col && centered_col + center_cols <= right_limit_col {
            surface.put_text(
                centered_col,
                row,
                center_cols,
                palette.fg,
                palette.statusbar_bg,
                &center,
            );
        }
    }

    render_commandbar(surface, panel, palette, footer_hint);
}

fn render_commandbar(
    surface: &mut CellSurface,
    panel: CellRect,
    palette: &ScenePalette,
    footer_hint: Option<&str>,
) {
    if panel.rows < 2 {
        return;
    }

    let row = panel.row + panel.rows - 1;
    let right = footer_hint.unwrap_or_default();
    let right_cols = display_cell_width(right) as i32;
    if right_cols > 0 && right_cols <= panel.cols {
        surface.put_text(
            panel.col + panel.cols - right_cols,
            row,
            right_cols,
            palette.muted,
            palette.bg,
            right,
        );
    }
}

fn render_statusbar_sidebar_segment(
    surface: &mut CellSurface,
    panel: CellRect,
    sidebar_panel: Option<CellRect>,
    palette: &ScenePalette,
    mode: HarnessMode,
) -> i32 {
    let row = panel.row;
    let Some(sidebar_panel) = sidebar_panel else {
        let mode_label = statusline_mode_label(mode);
        let mode_cols = display_cell_width(mode_label) as i32;
        surface.put_text(
            panel.col,
            row,
            mode_cols,
            palette.fg,
            palette.statusbar_bg,
            mode_label,
        );
        if let Some(rect) = statusbar_new_project_rect(panel, None, mode) {
            surface.put_text(
                rect.col,
                rect.row,
                rect.cols,
                palette.fg,
                palette.statusbar_bg,
                "+",
            );
            return rect.col + rect.cols + 1;
        }
        return mode_cols.min(panel.cols);
    };

    let sidebar_cols = sidebar_panel.cols.min(panel.cols).max(0);
    if sidebar_cols <= 0 {
        return panel.col;
    }

    let mode_label = statusline_mode_label(mode);
    let mode_cols = display_cell_width(mode_label) as i32;
    let mode_cols = mode_cols.min(sidebar_cols);
    surface.put_text(
        panel.col,
        row,
        mode_cols,
        palette.fg,
        palette.statusbar_bg,
        mode_label,
    );

    if let Some(rect) = statusbar_new_project_rect(panel, Some(sidebar_panel), mode) {
        surface.put_text(
            rect.col,
            rect.row,
            rect.cols,
            palette.fg,
            palette.statusbar_bg,
            "+",
        );
    }

    let separator_col = panel.col + sidebar_cols - 1;
    surface.set_cell(
        separator_col,
        row,
        palette.border,
        palette.statusbar_bg,
        "│",
        false,
    );
    panel.col + sidebar_cols
}

pub(super) fn statusbar_new_project_rect(
    panel: CellRect,
    sidebar_panel: Option<CellRect>,
    mode: HarnessMode,
) -> Option<CellRect> {
    if panel.cols <= 0 || panel.rows <= 0 {
        return None;
    }

    let row = panel.row;
    let mode_cols = display_cell_width(statusline_mode_label(mode)) as i32;
    let plus_col = sidebar_panel
        .map(|sidebar_panel| panel.col + sidebar_panel.cols.min(panel.cols).max(0) - 3)
        .unwrap_or(panel.col + mode_cols + 1);
    let limit_col = sidebar_panel
        .map(|sidebar_panel| panel.col + sidebar_panel.cols.min(panel.cols).max(0) - 1)
        .unwrap_or(panel.col + panel.cols);

    (plus_col >= panel.col + mode_cols && plus_col < limit_col)
        .then_some(CellRect::new(plus_col, row, 1, 1))
}

pub(super) fn statusline_mode_label(mode: HarnessMode) -> &'static str {
    match mode {
        HarnessMode::Normal => " NORMAL ",
        HarnessMode::Command => " COMMAND ",
    }
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

fn statusline_right_text(chrome: &ChromeView) -> String {
    if chrome.status.is_empty() {
        String::new()
    } else {
        format!(" {} ", chrome.status)
    }
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
    if panel.cols > 0 && panel.rows > 0 {
        let separator_col = panel.col + panel.cols - 1;
        for row in panel.row..(panel.row + panel.rows) {
            surface.set_cell(
                separator_col,
                row,
                palette.border,
                palette.sidebar_bg,
                "│",
                false,
            );
        }
    }
    let visible_rows = content.rows.max(0) as usize;
    if visible_rows == 0 || content.cols <= 0 {
        return;
    }

    let shows_scrollbar = rows.len() > visible_rows;
    if shows_scrollbar && content.cols > 1 {
        let scrollbar_col = content.col + content.cols - 1;
        render_cell_scrollbar(
            surface,
            scrollbar_col,
            content.row,
            content.rows,
            visible_rows,
            rows.len(),
            viewport_scroll_from_rows(viewport),
            palette.border,
            palette.sidebar_bg,
            SCROLLBAR_TRACK_GLYPH,
            palette.muted,
            SCROLLBAR_THUMB_GLYPH,
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
        let active = row.inverted || hovered_row_index == Some(item.row_index);
        let (row_fg, row_bg, reverse) = sidebar_row_style(row, active, palette);
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
        if reverse {
            surface.set_reverse_rect(
                CellRect {
                    col: content.col,
                    row: row_y,
                    cols: content.cols,
                    rows: 1,
                },
                true,
            );
        }

        match row.kind {
            SidebarRowKind::Label => {}
            SidebarRowKind::Project(_) => {
                let value = truncate_to_cells(&row.text, content.cols as usize);
                surface.put_text_styled(
                    content.col,
                    row_y,
                    content.cols,
                    row_fg,
                    row_bg,
                    &value,
                    reverse,
                );
            }
            SidebarRowKind::Session { .. } => {
                let status = row.status.map(|status| {
                    (
                        sidebar_status_glyph(status, now_ms),
                        if palette.monochrome || active {
                            row_fg
                        } else {
                            sidebar_status_color(status)
                        },
                    )
                });
                let indent_cols = content.cols.min(2);
                let reserved_cells = status
                    .map(|(glyph, _)| display_cell_width(glyph) as i32 + 1)
                    .unwrap_or(0);
                let text_cols = content
                    .cols
                    .saturating_sub(indent_cols)
                    .saturating_sub(reserved_cells);
                let value = truncate_to_cells(&row.text, text_cols as usize);
                surface.put_text_styled(
                    content.col + indent_cols,
                    row_y,
                    text_cols,
                    row_fg,
                    row_bg,
                    &value,
                    reverse,
                );

                if let Some((glyph, color)) = status {
                    let glyph_cols = display_cell_width(glyph) as i32;
                    let glyph_col = content.col + content.cols.saturating_sub(glyph_cols);
                    surface.put_text_styled(
                        glyph_col, row_y, glyph_cols, color, row_bg, glyph, reverse,
                    );
                }
            }
        }
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
    active: bool,
    palette: &ScenePalette,
) -> (Color, Color, bool) {
    if palette.monochrome {
        return (palette.fg, palette.bg, active);
    }

    if active {
        return (row.bg.unwrap_or(palette.sidebar_bg), row.fg, false);
    }

    (row.fg, row.bg.unwrap_or(palette.sidebar_bg), false)
}

pub(super) fn render_terminal(
    surface: &mut CellSurface,
    layout: TerminalSceneLayout,
    screen: &vt100::Screen,
    selection: Option<TerminalSelectionRange>,
    palette: &ScenePalette,
    cursor_mode: TerminalCursorMode,
) -> Option<HardwareCursor> {
    surface.fill_rect(layout.card, palette.term_fg, palette.term_bg);
    if layout.terminal.rows <= 0 || layout.terminal.cols <= 0 {
        return None;
    }

    blit_terminal_screen(
        surface,
        layout.terminal,
        screen,
        selection,
        palette,
        cursor_mode == TerminalCursorMode::Cell,
    );
    render_terminal_scrollback(surface, layout, screen, palette);

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
    for cell in terminal_screen_cells(
        screen,
        rect.rows.max(0) as u16,
        rect.cols.max(0) as u16,
        selection,
        palette.term_fg,
        palette.term_bg,
        draw_cursor,
    ) {
        surface.put_cell_span(
            rect.col + i32::from(cell.col),
            rect.row + i32::from(cell.row),
            i32::from(cell.span),
            &cell.text,
            cell.fg,
            cell.bg,
            cell.underline,
        );
    }
}

fn render_terminal_scrollback(
    surface: &mut CellSurface,
    layout: TerminalSceneLayout,
    screen: &vt100::Screen,
    palette: &ScenePalette,
) {
    let visible_rows = usize::from(screen.size().0);
    let max_scroll = terminal_max_scrollback(screen);
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
        SCROLLBAR_TRACK_GLYPH,
        palette.muted,
        SCROLLBAR_THUMB_GLYPH,
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
