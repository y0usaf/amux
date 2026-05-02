use crate::terminal::TerminalSelectionPoint;

use super::input::{MouseEvent, MouseEventKind, WheelDirection};
use super::raw::terminal_size;
use super::{TuiApp, TUI_WHEEL_LINES};
use crate::app::backend::terminal_selection_point_for_cell_rect;
use crate::app::layout::{compute_cell_layout, sidebar_content_rect, CellLayout};
use crate::app::scene::statusbar_new_project_rect;

impl TuiApp {
    pub(super) fn handle_scroll_key(&mut self, stroke: &crate::config::KeyStroke) -> bool {
        self.core.handle_terminal_scroll_key(stroke)
    }

    pub(super) fn handle_mouse_event(&mut self, event: MouseEvent) {
        let (cols, rows) = terminal_size();
        let layout = compute_cell_layout(cols, rows, self.core.config.layout_widths());

        match event.kind {
            MouseEventKind::Wheel(direction) => self.handle_mouse_wheel(event, direction, &layout),
            MouseEventKind::LeftPress => self.handle_left_mouse_press(event, &layout),
            MouseEventKind::LeftDrag => self.handle_left_mouse_drag(event, &layout),
            MouseEventKind::LeftRelease => self.handle_left_mouse_release(),
            MouseEventKind::Other => {}
        }
    }

    pub(super) fn handle_mouse_wheel(
        &mut self,
        event: MouseEvent,
        direction: WheelDirection,
        layout: &CellLayout,
    ) {
        let delta = match direction {
            WheelDirection::Up => TUI_WHEEL_LINES,
            WheelDirection::Down => -TUI_WHEEL_LINES,
        };

        if layout.sidebar.contains_cell(event.col, event.row) {
            let visible_rows = sidebar_content_rect(layout.sidebar).rows.max(0) as usize;
            let row_count = self.core.sidebar_rows().len();
            self.core
                .scroll_sidebar_from_wheel(delta, visible_rows, row_count);
            return;
        }

        if layout.terminal_card.contains_cell(event.col, event.row) {
            self.core.scroll_terminal_by_lines(delta);
        }
    }

    pub(super) fn handle_left_mouse_press(&mut self, event: MouseEvent, layout: &CellLayout) {
        self.core.set_terminal_selection_in_progress(false);
        if self.handle_statusbar_click(event, layout) {
            return;
        }
        if self.handle_sidebar_click(event, layout) {
            return;
        }

        let point = self.terminal_selection_point_for_mouse(event, layout);
        self.core.clear_or_begin_terminal_selection(point);
    }

    pub(super) fn handle_left_mouse_drag(&mut self, event: MouseEvent, layout: &CellLayout) {
        let Some(point) = self.terminal_selection_point_for_mouse(event, layout) else {
            return;
        };
        self.core.update_terminal_selection(point);
    }

    pub(super) fn handle_left_mouse_release(&mut self) {
        self.core.finish_terminal_selection();
    }

    pub(super) fn handle_statusbar_click(
        &mut self,
        event: MouseEvent,
        layout: &CellLayout,
    ) -> bool {
        if !layout.statusbar.contains_cell(event.col, event.row) {
            return false;
        }
        let sidebar_panel =
            (layout.sidebar.cols > 0 && layout.sidebar.rows > 0).then_some(layout.sidebar);
        if statusbar_new_project_rect(layout.statusbar, sidebar_panel)
            .is_some_and(|rect| rect.contains_cell(event.col, event.row))
        {
            self.start_command_line("open ");
        }
        true
    }

    pub(super) fn handle_sidebar_click(&mut self, event: MouseEvent, layout: &CellLayout) -> bool {
        let content = sidebar_content_rect(layout.sidebar);
        if !content.contains_cell(event.col, event.row) {
            return false;
        }
        let visible_rows = content.rows.max(0) as usize;
        let visible_row = (event.row - content.row).max(0) as usize;
        let rows = self.core.sidebar_rows();
        let Some(row_index) =
            self.core
                .sidebar_row_index_at_visible_row(&rows, visible_rows, visible_row)
        else {
            return true;
        };
        let Some(row_kind) = rows.get(row_index).map(|row| row.kind.clone()) else {
            return true;
        };
        self.core.activate_sidebar_row(&row_kind);
        true
    }

    pub(super) fn terminal_selection_point_for_mouse(
        &self,
        event: MouseEvent,
        layout: &CellLayout,
    ) -> Option<TerminalSelectionPoint> {
        let (rows, cols) = self
            .core
            .current_terminal()
            .map(|terminal| terminal.screen().size())
            .unwrap_or((
                layout.terminal.rows.max(1) as u16,
                layout.terminal.cols.max(1) as u16,
            ));
        terminal_selection_point_for_cell_rect(layout.terminal, rows, cols, event.col, event.row)
    }
}
