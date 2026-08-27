use super::super::cell_surface::display_cell_width;
use super::super::layout::CellLayout;
use super::super::scene::{statusline_mode_label, StatusbarState};

#[derive(Clone, Debug)]
pub(super) struct CommandLineState {
    pub(super) input: String,
    cursor: usize,
}

impl CommandLineState {
    pub(super) fn with_input(input: impl Into<String>) -> Self {
        let input = input.into();
        let cursor = input.len();
        Self { input, cursor }
    }

    pub(super) fn insert_str(&mut self, text: &str) {
        self.input.insert_str(self.cursor, text);
        self.cursor += text.len();
    }

    pub(super) fn backspace(&mut self) -> bool {
        let Some(previous) = previous_char_boundary(&self.input, self.cursor) else {
            return false;
        };
        self.input.drain(previous..self.cursor);
        self.cursor = previous;
        true
    }

    pub(super) fn delete(&mut self) -> bool {
        let Some(next) = next_char_boundary(&self.input, self.cursor) else {
            return false;
        };
        self.input.drain(self.cursor..next);
        true
    }

    pub(super) fn delete_word_back(&mut self) -> bool {
        let original = self.cursor;
        while self
            .input
            .get(..self.cursor)
            .and_then(|text| text.chars().next_back())
            .is_some_and(char::is_whitespace)
        {
            self.backspace();
        }
        while self
            .input
            .get(..self.cursor)
            .and_then(|text| text.chars().next_back())
            .is_some_and(|ch| !ch.is_whitespace())
        {
            self.backspace();
        }
        self.cursor != original
    }

    pub(super) fn clear(&mut self) {
        self.input.clear();
        self.cursor = 0;
    }

    pub(super) fn move_left(&mut self) -> bool {
        let Some(previous) = previous_char_boundary(&self.input, self.cursor) else {
            return false;
        };
        self.cursor = previous;
        true
    }

    pub(super) fn move_right(&mut self) -> bool {
        let Some(next) = next_char_boundary(&self.input, self.cursor) else {
            return false;
        };
        self.cursor = next;
        true
    }

    pub(super) fn move_home(&mut self) -> bool {
        let changed = self.cursor != 0;
        self.cursor = 0;
        changed
    }

    pub(super) fn move_end(&mut self) -> bool {
        let changed = self.cursor != self.input.len();
        self.cursor = self.input.len();
        changed
    }

    pub(super) fn visible_text_and_cursor_col(&self, max_cols: usize) -> (String, i32) {
        let max_cols = max_cols.max(1);
        let full = format!(":{}", self.input);
        let cursor_byte = 1 + self.cursor;
        let full_cells = display_cell_width(&full);
        let cursor_cells = display_cell_width(&full[..cursor_byte]);
        if full_cells <= max_cols {
            return (full, cursor_cells.min(max_cols.saturating_sub(1)) as i32);
        }

        let marker = "…";
        let marker_cells = display_cell_width(marker);
        if max_cols <= marker_cells {
            return (marker.to_string(), 0);
        }

        let left_overflow = cursor_cells > max_cols.saturating_sub(marker_cells);
        let start_byte = if left_overflow {
            byte_index_at_cell_width(
                &full,
                cursor_cells.saturating_sub(max_cols.saturating_sub(marker_cells)),
            )
        } else {
            0
        };
        let available_cols = if start_byte > 0 {
            max_cols.saturating_sub(marker_cells)
        } else {
            max_cols
        };
        let mut visible = String::new();
        if start_byte > 0 {
            visible.push_str(marker);
        }
        visible.push_str(&prefix_to_cells(&full[start_byte..], available_cols));

        let cursor_col = (if start_byte > 0 { marker_cells } else { 0 })
            + display_cell_width(&full[start_byte..cursor_byte]).min(available_cols);
        (visible, cursor_col.min(max_cols.saturating_sub(1)) as i32)
    }
}

fn previous_char_boundary(value: &str, index: usize) -> Option<usize> {
    if index == 0 {
        return None;
    }
    value[..index]
        .char_indices()
        .next_back()
        .map(|(index, _)| index)
}

fn next_char_boundary(value: &str, index: usize) -> Option<usize> {
    if index >= value.len() {
        return None;
    }
    value[index..]
        .char_indices()
        .nth(1)
        .map(|(offset, _)| index + offset)
        .or(Some(value.len()))
}

fn byte_index_at_cell_width(value: &str, target_cells: usize) -> usize {
    if target_cells == 0 {
        return 0;
    }
    let mut cells = 0;
    for (index, ch) in value.char_indices() {
        if cells >= target_cells {
            return index;
        }
        cells += command_char_width(ch);
    }
    value.len()
}

fn prefix_to_cells(value: &str, max_cols: usize) -> String {
    let mut output = String::new();
    let mut cells = 0;
    for ch in value.chars() {
        let width = command_char_width(ch);
        if cells + width > max_cols {
            break;
        }
        output.push(ch);
        cells += width;
    }
    output
}

fn command_char_width(ch: char) -> usize {
    unicode_width::UnicodeWidthChar::width(ch)
        .unwrap_or(1)
        .max(1)
}

pub(super) fn command_line_start_col(layout: &CellLayout, surface_cols: i32) -> i32 {
    if layout.sidebar.cols > 0 {
        return layout.sidebar.cols.min(surface_cols).max(0);
    }

    display_cell_width(&statusline_mode_label(StatusbarState::Command))
        .min(surface_cols.max(0) as usize) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_line_backspace_updates_utf8_cursor() {
        let mut command_line = CommandLineState::with_input("open café");
        assert!(command_line.backspace());
        assert_eq!(command_line.input, "open caf");
        command_line.move_left();
        command_line.insert_str("é");
        assert_eq!(command_line.input, "open caéf");
    }
}
