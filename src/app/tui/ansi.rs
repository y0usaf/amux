use std::io::{self, Write};

use crate::render::Color;

use super::super::cell_surface::{Cell, CellSurface};
use super::super::scene::HardwareCursor;

pub(super) const DEFAULT_FG: Color = Color::rgba(0, 0, 0, 0);
pub(super) const DEFAULT_BG: Color = Color::rgba(0, 0, 0, 0);

#[derive(Default)]
pub(super) struct AnsiRenderer {
    previous: Option<CellSurface>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct AnsiStyle {
    fg: Color,
    bg: Color,
    underline: bool,
    reverse: bool,
}

impl AnsiStyle {
    fn from_cell(cell: &Cell) -> Self {
        Self {
            fg: cell.fg,
            bg: cell.bg,
            underline: cell.underline,
            reverse: cell.reverse,
        }
    }
}

impl AnsiRenderer {
    pub(super) fn render(
        &mut self,
        stdout: &mut io::Stdout,
        surface: &CellSurface,
        hardware_cursor: Option<HardwareCursor>,
    ) -> io::Result<()> {
        write!(stdout, "\x1b[?25l")?;
        match self.previous.as_ref() {
            Some(previous) if previous.cols == surface.cols && previous.rows == surface.rows => {
                render_diff(stdout, previous, surface)?;
            }
            _ => render_full(stdout, surface)?,
        }
        render_hardware_cursor(stdout, hardware_cursor)?;
        self.previous = Some(surface.clone());
        stdout.flush()
    }
}

fn render_full(stdout: &mut io::Stdout, surface: &CellSurface) -> io::Result<()> {
    write!(stdout, "\x1b[H")?;
    for row in 0..surface.rows {
        write!(stdout, "\x1b[{};1H", row + 1)?;
        let mut current_style = None;
        for col in 0..surface.cols {
            let cell = &surface.cells[(row * surface.cols + col) as usize];
            if cell.continuation {
                continue;
            }
            let style = AnsiStyle::from_cell(cell);
            if current_style != Some(style) {
                write_style(stdout, style)?;
                current_style = Some(style);
            }
            write!(stdout, "{}", cell.text)?;
        }
        write!(stdout, "\x1b[0m")?;
    }
    Ok(())
}

fn render_diff(
    stdout: &mut io::Stdout,
    previous: &CellSurface,
    surface: &CellSurface,
) -> io::Result<()> {
    for row in 0..surface.rows {
        let mut col = 0;
        while col < surface.cols {
            let index = (row * surface.cols + col) as usize;
            let cell = &surface.cells[index];
            if previous.cells[index] == *cell || cell.continuation {
                col += 1;
                continue;
            }

            let style = AnsiStyle::from_cell(cell);
            let start_col = col;
            let mut text = String::new();
            while col < surface.cols {
                let index = (row * surface.cols + col) as usize;
                let cell = &surface.cells[index];
                if previous.cells[index] == *cell {
                    break;
                }
                if cell.continuation {
                    col += 1;
                    continue;
                }
                if AnsiStyle::from_cell(cell) != style {
                    break;
                }
                text.push_str(&cell.text);
                col += 1;
            }

            if !text.is_empty() {
                write!(stdout, "\x1b[{};{}H", row + 1, start_col + 1)?;
                write_style(stdout, style)?;
                write!(stdout, "{}", text)?;
            }
        }
    }
    write!(stdout, "\x1b[0m")
}

fn render_hardware_cursor(
    stdout: &mut io::Stdout,
    hardware_cursor: Option<HardwareCursor>,
) -> io::Result<()> {
    if let Some(cursor) = hardware_cursor {
        write!(
            stdout,
            "\x1b[{};{}H\x1b[?25h",
            cursor.row + 1,
            cursor.col + 1
        )
    } else {
        write!(stdout, "\x1b[?25l")
    }
}

fn write_style(stdout: &mut io::Stdout, style: AnsiStyle) -> io::Result<()> {
    write!(stdout, "\x1b[0m")?;
    write!(stdout, "{}{}", ansi_fg(style.fg), ansi_bg(style.bg))?;
    if style.underline {
        write!(stdout, "\x1b[4m")?;
    }
    if style.reverse {
        write!(stdout, "\x1b[7m")?;
    }
    Ok(())
}

fn ansi_fg(color: Color) -> String {
    if color == DEFAULT_FG {
        return "\x1b[39m".to_string();
    }
    if let Some(index) = color.ansi_index_value() {
        return ansi_index_fg(index);
    }
    let (r, g, b) = color.rgb_components();
    format!("\x1b[38;2;{r};{g};{b}m")
}

fn ansi_bg(color: Color) -> String {
    if color == DEFAULT_BG {
        return "\x1b[49m".to_string();
    }
    if let Some(index) = color.ansi_index_value() {
        return ansi_index_bg(index);
    }
    let (r, g, b) = color.rgb_components();
    format!("\x1b[48;2;{r};{g};{b}m")
}

fn ansi_index_fg(index: u8) -> String {
    match index {
        0..=7 => format!("\x1b[{}m", 30 + index),
        8..=15 => format!("\x1b[{}m", 90 + index - 8),
        _ => format!("\x1b[38;5;{index}m"),
    }
}

fn ansi_index_bg(index: u8) -> String {
    match index {
        0..=7 => format!("\x1b[{}m", 40 + index),
        8..=15 => format!("\x1b[{}m", 100 + index - 8),
        _ => format!("\x1b[48;5;{index}m"),
    }
}

#[cfg(test)]
mod tests {
    use super::{ansi_index_bg, ansi_index_fg};

    #[test]
    fn low_ansi_indexes_use_classic_sgr_sequences() {
        assert_eq!(ansi_index_fg(4), "\x1b[34m");
        assert_eq!(ansi_index_fg(12), "\x1b[94m");
        assert_eq!(ansi_index_bg(4), "\x1b[44m");
        assert_eq!(ansi_index_bg(12), "\x1b[104m");
    }

    #[test]
    fn high_ansi_indexes_use_256_color_sequences() {
        assert_eq!(ansi_index_fg(42), "\x1b[38;5;42m");
        assert_eq!(ansi_index_bg(42), "\x1b[48;5;42m");
    }
}
