use crate::render::Color;
use crate::terminal::TerminalSelectionRange;

use super::backend::{ChromeView, FrameModel};
use super::cell_surface::{
    display_cell_width, render_cell_scrollbar, truncate_to_cells, CellSurface,
};
use super::layout::{sidebar_content_rect, CellLayout, CellRect};
use super::sidebar::{
    sidebar_status_glyph, SidebarRow, SidebarRowKind, SidebarStatusKind, SidebarViewportItem,
};
use super::terminal_view::{terminal_max_scrollback, terminal_screen_cells};
use super::theme::{self, DerivedTheme};
const SCROLLBAR_TRACK_GLYPH: &str = "│";
const SCROLLBAR_THUMB_GLYPH: &str = "┃";
const STATUS_SEPARATOR_GLYPH: &str = "|";

#[derive(Clone, Copy)]
pub(super) struct ScenePalette {
    pub(super) fg: Color,
    pub(super) bg: Color,
    pub(super) statusbar_bg: Color,
    pub(super) statusbar_fg: Color,
    pub(super) sidebar_bg: Color,
    pub(super) border: Color,
    pub(super) muted: Color,
    pub(super) term_fg: Color,
    pub(super) term_bg: Color,
    pub(super) accent: Color,
    pub(super) accent_2: Color,

    pub(super) ansi: [Color; 16],

    pub(super) running: Color,
    pub(super) warning: Color,
    pub(super) error: Color,
    pub(super) monochrome: bool,
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
            term_fg: theme.term_fg,
            term_bg: theme.term_bg,
            accent: theme.accent,
            accent_2: theme.accent_2,

            ansi: theme.ansi,

            running: theme.running,
            warning: theme.warning,
            error: theme.error,
            monochrome: false,
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
    fill_horizontal_gradient(
        surface,
        statusline,
        palette.statusbar_fg,
        palette.statusbar_bg,
        palette.accent_2,
    );

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
            palette.statusbar_fg,
            palette.statusbar_bg,
            &left,
        );
    }

    if right_cols > 0 {
        surface.put_text(
            main_col + main_cols - right_cols,
            row,
            right_cols,
            statusline_status_color(&chrome.status, palette),
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
                palette.statusbar_fg,
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
    let bg = commandbar_bg(palette);
    surface.fill_rect(
        CellRect::new(panel.col, row, panel.cols, 1),
        palette.muted,
        bg,
    );
    let right = footer_hint.unwrap_or_default();
    let right_cols = display_cell_width(right) as i32;
    if right_cols > 0 && right_cols <= panel.cols {
        surface.put_text(
            panel.col + panel.cols - right_cols,
            row,
            right_cols,
            palette.muted,
            bg,
            right,
        );
    }
}

fn commandbar_bg(palette: &ScenePalette) -> Color {
    palette.bg
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
            mode_fg(mode, palette),
            mode_bg(mode, palette),
            mode_label,
        );
        if let Some(rect) = statusbar_new_project_rect(panel, None, mode) {
            surface.put_text(
                rect.col,
                rect.row,
                rect.cols,
                palette.accent,
                theme::fade_toward(palette.statusbar_bg, palette.sidebar_bg, 60),
                "✚",
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
        mode_fg(mode, palette),
        mode_bg(mode, palette),
        mode_label,
    );

    let plus_rect = statusbar_new_project_rect(panel, Some(sidebar_panel), mode);
    let separator_col = panel.col + sidebar_cols - 1;
    let brand_start = panel.col + mode_cols + 1;
    let brand_end = plus_rect
        .map(|rect| rect.col.saturating_sub(1))
        .unwrap_or(separator_col);
    render_statusline_rule(
        surface,
        row,
        brand_start,
        brand_end.saturating_sub(brand_start),
        palette,
    );

    if let Some(rect) = plus_rect {
        surface.put_text(
            rect.col,
            rect.row,
            rect.cols,
            palette.accent,
            theme::fade_toward(palette.statusbar_bg, palette.sidebar_bg, 60),
            "✚",
        );
    }

    surface.set_cell(
        separator_col,
        row,
        palette.border,
        palette.statusbar_bg,
        STATUS_SEPARATOR_GLYPH,
        false,
    );
    panel.col + sidebar_cols
}

fn render_statusline_rule(
    surface: &mut CellSurface,
    row: i32,
    col: i32,
    cols: i32,
    palette: &ScenePalette,
) {
    if cols <= 0 {
        return;
    }
    for offset in 0..cols {
        surface.set_cell(
            col + offset,
            row,
            palette.border,
            palette.statusbar_bg,
            "|",
            false,
        );
    }
}

fn fill_horizontal_gradient(
    surface: &mut CellSurface,
    rect: CellRect,
    fg: Color,
    from: Color,
    to: Color,
) {
    if rect.cols <= 0 || rect.rows <= 0 {
        return;
    }
    let denom = rect.cols.saturating_sub(1).max(1) as u16;
    for col_offset in 0..rect.cols {
        let mix = ((col_offset as u16 * 255) / denom) as u8;
        let bg = theme::fade_toward(from, to, mix);
        for row in rect.row..(rect.row + rect.rows) {
            surface.set_cell(rect.col + col_offset, row, fg, bg, " ", false);
        }
    }
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

fn mode_fg(mode: HarnessMode, palette: &ScenePalette) -> Color {
    match mode {
        HarnessMode::Normal => palette.statusbar_bg,
        HarnessMode::Command => palette.statusbar_fg,
    }
}

fn mode_bg(mode: HarnessMode, palette: &ScenePalette) -> Color {
    match mode {
        HarnessMode::Normal => palette.border,
        HarnessMode::Command => palette.warning,
    }
}

fn statusline_left_text(chrome: &ChromeView) -> String {
    format!(" ◆ {} ", chrome.project)
}

fn statusline_center_text(chrome: &ChromeView) -> String {
    if chrome.session.is_empty() {
        String::new()
    } else {
        format!(" ◇ {} ", chrome.session)
    }
}

fn statusline_right_text(chrome: &ChromeView) -> String {
    if chrome.status.is_empty() {
        String::new()
    } else {
        format!(
            " {} {} ",
            statusline_status_symbol(&chrome.status),
            chrome.status
        )
    }
}

fn statusline_status_symbol(status: &str) -> &'static str {
    match status {
        "thinking" => "⋯",
        "outputting" => "⟡",
        "running" | "launching" => "●",
        "error" => "×",
        "exited" => "◌",
        _ if status.starts_with("tool") => "⚙",
        _ if status.contains("queued") => "▶",
        _ => "•",
    }
}

fn statusline_status_color(status: &str, palette: &ScenePalette) -> Color {
    match status {
        "thinking" | "outputting" | "running" | "launching" => palette.running,
        "error" => palette.error,
        "exited" => palette.warning,
        _ if status.starts_with("tool") => palette.running,
        _ if status.contains("queued") => palette.warning,
        _ => palette.accent_2,
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
            let row_offset = (row - panel.row).max(0) as u16;
            let denom = panel.rows.saturating_sub(1).max(1) as u16;
            let mix = ((row_offset * 255) / denom) as u8;
            let color = theme::fade_toward(palette.border, palette.accent_2, mix);
            surface.set_cell(separator_col, row, color, palette.sidebar_bg, "│", false);
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
            palette.accent_2,
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
        let selected = row.inverted;
        let hovered = hovered_row_index == Some(item.row_index) && !selected;
        let active = selected || hovered;
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
                render_sidebar_project_rule(
                    surface,
                    CellRect::new(content.col, row_y, content.cols, 1),
                    &row.text,
                    row_fg,
                    row_bg,
                    row.status,
                    now_ms,
                    reverse,
                    palette,
                );
            }
            SidebarRowKind::Session { .. } => {
                let status = row.status.map(|status| {
                    (
                        sidebar_status_glyph(status, now_ms),
                        sidebar_status_color_for_palette(status, palette),
                    )
                });
                let branch = if active { "▌ " } else { "  " };
                let indent_cols = content.cols.min(display_cell_width(branch) as i32);
                let reserved_cells = status
                    .map(|(glyph, _)| display_cell_width(glyph) as i32 + 1)
                    .unwrap_or(0);
                let text_cols = content
                    .cols
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
                    content.col,
                    row_y,
                    indent_cols,
                    branch_fg,
                    row_bg,
                    branch,
                    reverse,
                );
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

fn render_sidebar_project_rule(
    surface: &mut CellSurface,
    rect: CellRect,
    label: &str,
    label_fg: Color,
    bg: Color,
    status: Option<SidebarStatusKind>,
    now_ms: u64,
    reverse: bool,
    palette: &ScenePalette,
) {
    if rect.cols <= 0 {
        return;
    }

    let label_fg = if matches!(status, Some(SidebarStatusKind::Notification)) {
        sidebar_status_color_for_palette(SidebarStatusKind::Notification, palette)
    } else {
        label_fg
    };
    let max_label_cols = rect.cols.max(0) as usize;
    let decorated_label = format!("⧸ {} ⧸", label);
    let mut value = truncate_to_cells(&decorated_label, max_label_cols);
    let mut value_cols = display_cell_width(&value) as i32;

    if value_cols >= rect.cols {
        surface.put_text_styled(
            rect.col,
            rect.row,
            rect.cols,
            label_fg,
            bg,
            &truncate_to_cells(&decorated_label, max_label_cols),
            reverse,
        );
        return;
    }

    // Keep the rule symmetric: left `/` count always equals right `/` count.
    // If the available rule width is odd, absorb the extra cell into label padding.
    if (rect.cols - value_cols) % 2 != 0 && value_cols < rect.cols {
        value.push(' ');
        value_cols += 1;
    }

    let side_cols = (rect.cols - value_cols) / 2;
    let left_cols = side_cols;
    let right_cols = side_cols;
    let mut slash_index = 0usize;

    for offset in 0..left_cols {
        surface.set_cell(
            rect.col + offset,
            rect.row,
            sidebar_project_rule_color(status, slash_index, now_ms, palette),
            bg,
            "/",
            reverse,
        );
        slash_index += 1;
    }

    surface.put_text_styled(
        rect.col + left_cols,
        rect.row,
        value_cols,
        label_fg,
        bg,
        &value,
        reverse,
    );

    for offset in 0..right_cols {
        surface.set_cell(
            rect.col + left_cols + value_cols + offset,
            rect.row,
            sidebar_project_rule_color(status, slash_index, now_ms, palette),
            bg,
            "/",
            reverse,
        );
        slash_index += 1;
    }
}

fn sidebar_project_rule_color(
    status: Option<SidebarStatusKind>,
    slash_index: usize,
    now_ms: u64,
    palette: &ScenePalette,
) -> Color {
    let base = palette.muted;
    let Some(status) = status else {
        return base;
    };
    let status_color = sidebar_status_color_for_palette(status, palette);
    match status {
        SidebarStatusKind::Notification => status_color,
        SidebarStatusKind::Active | SidebarStatusKind::Queued => {
            let phase = ((now_ms / 80) as usize + slash_index) % 12;
            let intensity = if phase < 6 { phase } else { 12 - phase };
            theme::fade_toward(base, status_color, (intensity * 42).min(255) as u8)
        }
    }
}

fn sidebar_status_color_for_palette(status: SidebarStatusKind, palette: &ScenePalette) -> Color {
    match status {
        SidebarStatusKind::Active => palette.running,
        SidebarStatusKind::Queued => palette.warning,
        SidebarStatusKind::Notification => palette.accent,
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
    if selected {
        return (
            palette.statusbar_fg,
            row.bg.unwrap_or(palette.sidebar_bg),
            false,
        );
    }
    if hovered {
        return (
            palette.accent_2,
            row.bg.unwrap_or(palette.sidebar_bg),
            false,
        );
    }
    if sticky {
        return (palette.accent, palette.sidebar_bg, false);
    }

    (fg, row.bg.unwrap_or(palette.sidebar_bg), false)
}

fn palette_color(color: Color, palette: &ScenePalette) -> Color {
    if color == theme::TEXT {
        palette.fg
    } else if color == theme::HEADING {
        theme::HEADING
    } else if color == theme::MUTED {
        palette.muted
    } else if color == theme::ACCENT {
        palette.accent
    } else if color == theme::RUNNING {
        palette.running
    } else if color == theme::WARNING {
        palette.warning
    } else if color == theme::ERROR {
        palette.error
    } else {
        color
    }
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
        &palette.ansi,
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
        palette.accent_2,
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
