use crate::render::Color;
use crate::terminal::TerminalSelectionRange;

use super::backend::{ChromeView, FrameModel};
use super::cell_surface::{
    centered_cell_offset, display_cell_width, draw_box, render_cell_scrollbar, truncate_to_cells,
    CellSurface,
};
use super::layout::{CellLayout, CellRect};
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
    pub(super) topbar_bg: Color,
    pub(super) sidebar_bg: Color,
    pub(super) terminal_card_bg: Color,
    pub(super) border: Color,
    pub(super) muted: Color,
    pub(super) accent: Color,
    pub(super) term_fg: Color,
    pub(super) term_bg: Color,
    pub(super) monochrome: bool,
}

impl ScenePalette {
    pub(super) fn monochrome(fg: Color, bg: Color, term_fg: Color, term_bg: Color) -> Self {
        Self {
            fg,
            bg,
            topbar_bg: bg,
            sidebar_bg: bg,
            terminal_card_bg: bg,
            border: fg,
            muted: fg,
            accent: fg,
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
    pub(super) topbar_panel: CellRect,
    pub(super) topbar_content: CellRect,
    pub(super) sidebar_panel: Option<CellRect>,
    pub(super) sidebar_content: CellRect,
    pub(super) terminal: TerminalSceneLayout,
}

pub(super) fn harness_scene_layout(layout: &CellLayout) -> HarnessSceneLayout {
    let scrollbar_col = (layout.terminal.col + layout.terminal.cols)
        .min(layout.terminal_card.col + layout.terminal_card.cols - 2)
        .max(layout.terminal.col);

    HarnessSceneLayout {
        topbar_panel: layout.topbar,
        topbar_content: layout.topbar.inset(1, 1),
        sidebar_panel: (layout.sidebar.cols > 0 && layout.sidebar.rows > 0)
            .then_some(layout.sidebar),
        sidebar_content: layout.sidebar.inset(1, 1),
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
    topbar_status_fg: Color,
    palette: &ScenePalette,
    cursor_mode: TerminalCursorMode,
    footer_hint: Option<&str>,
    now_ms: u64,
) -> Option<HardwareCursor> {
    render_topbar(
        surface,
        layout.topbar_panel,
        layout.topbar_content,
        &frame_model.chrome,
        topbar_status_fg,
        palette,
    );
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
    render_terminal(
        surface,
        layout.terminal,
        &frame_model.terminal_screen,
        frame_model.terminal_selection,
        palette,
        cursor_mode,
        footer_hint,
    )
}

pub(super) fn render_topbar(
    surface: &mut CellSurface,
    panel: CellRect,
    content: CellRect,
    chrome: &ChromeView,
    status_fg: Color,
    palette: &ScenePalette,
) {
    draw_box(
        surface,
        panel,
        palette.fg,
        palette.topbar_bg,
        palette.border,
    );
    if content.cols <= 0 || content.rows <= 0 {
        return;
    }

    let rows = [
        (chrome.project.as_str(), palette.muted),
        (chrome.status.as_str(), status_fg),
        (chrome.session.as_str(), palette.accent),
    ];
    for (offset, (value, fg)) in rows.into_iter().enumerate() {
        if offset as i32 >= content.rows {
            break;
        }
        let value = truncate_to_cells(value, content.cols.max(0) as usize);
        let col = content.col + centered_cell_offset(content.cols, &value);
        surface.put_text(
            col,
            content.row + offset as i32,
            content.cols,
            fg,
            palette.topbar_bg,
            &value,
        );
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
    draw_box(
        surface,
        panel,
        palette.fg,
        palette.sidebar_bg,
        palette.border,
    );
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
            SidebarRowKind::ActionOpenProject | SidebarRowKind::Project(_) => {
                let value = truncate_to_cells(&row.text, content.cols as usize);
                let col = content.col + centered_cell_offset(content.cols, &value);
                surface.put_text_styled(col, row_y, content.cols, row_fg, row_bg, &value, reverse);
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
                let reserved_cells = status
                    .map(|(glyph, _)| display_cell_width(glyph) as i32 + 1)
                    .unwrap_or(0);
                let text_cols = content.cols.saturating_sub(reserved_cells);
                let value = truncate_to_cells(&row.text, text_cols as usize);
                surface.put_text_styled(
                    content.col,
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
    footer_hint: Option<&str>,
) -> Option<HardwareCursor> {
    draw_box(
        surface,
        layout.card,
        palette.fg,
        palette.terminal_card_bg,
        palette.border,
    );
    surface.fill_rect(layout.terminal, palette.term_fg, palette.term_bg);
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

    if let Some(help) = footer_hint {
        if layout.card.cols > display_cell_width(help) as i32 + 2 {
            surface.put_text(
                layout.card.col + layout.card.cols - display_cell_width(help) as i32 - 1,
                layout.card.row + layout.card.rows - 1,
                display_cell_width(help) as i32,
                palette.fg,
                palette.terminal_card_bg,
                help,
            );
        }
    }

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
        palette.terminal_card_bg,
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
