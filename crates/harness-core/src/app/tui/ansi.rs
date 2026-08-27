use std::io::{self, Write};

use crate::render::Color;

use super::super::cell_surface::{Cell, CellSurface};
use super::super::scene::HardwareCursor;

pub(super) const DEFAULT_FG: Color = Color::rgba(0, 0, 0, 0);
pub(super) const DEFAULT_BG: Color = Color::rgba(0, 0, 0, 0);

pub(super) struct AnsiRenderer {
    previous: Option<CellSurface>,
    scratch: String,
}

impl Default for AnsiRenderer {
    fn default() -> Self {
        Self {
            previous: None,
            scratch: String::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct AnsiStyle {
    fg: Color,
    bg: Color,
    bold: bool,
    underline: bool,
    reverse: bool,
}

impl AnsiStyle {
    fn from_cell(cell: &Cell) -> Self {
        Self {
            fg: cell.fg,
            bg: cell.bg,
            bold: cell.bold,
            underline: cell.underline,
            reverse: cell.reverse,
        }
    }
}

impl AnsiRenderer {
    pub(super) fn render(
        &mut self,
        stdout: &mut io::Stdout,
        surface: CellSurface,
        hardware_cursor: Option<HardwareCursor>,
    ) -> io::Result<CellSurface> {
        write!(stdout, "[?25l")?;
        match self.previous.as_ref() {
            Some(previous) if previous.cols == surface.cols && previous.rows == surface.rows => {
                render_diff(stdout, previous, &surface, &mut self.scratch)?;
            }
            _ => render_full(stdout, &surface)?,
        }
        render_hardware_cursor(stdout, hardware_cursor)?;
        let reusable = self.previous.replace(surface).unwrap_or_default();
        stdout.flush()?;
        Ok(reusable)
    }
}

fn render_full(stdout: &mut io::Stdout, surface: &CellSurface) -> io::Result<()> {
    write!(stdout, "[H")?;
    for row in 0..surface.rows {
        write!(stdout, "[{};1H", row + 1)?;
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
            stdout.write_all(cell.text.as_bytes())?;
        }
        write!(stdout, "[0m")?;
    }
    Ok(())
}

fn render_diff(
    stdout: &mut io::Stdout,
    previous: &CellSurface,
    surface: &CellSurface,
    scratch: &mut String,
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
            scratch.clear();
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
                scratch.push_str(&cell.text);
                col += 1;
            }

            if !scratch.is_empty() {
                write!(stdout, "[{};{}H", row + 1, start_col + 1)?;
                write_style(stdout, style)?;
                stdout.write_all(scratch.as_bytes())?;
            }
        }
    }
    write!(stdout, "[0m")
}

fn render_hardware_cursor(
    stdout: &mut io::Stdout,
    hardware_cursor: Option<HardwareCursor>,
) -> io::Result<()> {
    if let Some(cursor) = hardware_cursor {
        write!(stdout, "[{};{}H[?25h", cursor.row + 1, cursor.col + 1)
    } else {
        write!(stdout, "[?25l")
    }
}

fn write_style(stdout: &mut io::Stdout, style: AnsiStyle) -> io::Result<()> {
    write!(stdout, "[0m")?;
    write_fg(stdout, style.fg)?;
    write_bg(stdout, style.bg)?;
    if style.bold {
        write!(stdout, "[1m")?;
    }
    if style.underline {
        write!(stdout, "[4m")?;
    }
    if style.reverse {
        write!(stdout, "[7m")?;
    }
    Ok(())
}

fn write_fg(stdout: &mut io::Stdout, color: Color) -> io::Result<()> {
    if color == DEFAULT_FG {
        return write!(stdout, "[39m");
    }
    if let Some(index) = color.ansi_index_value() {
        return write_index_fg(stdout, index);
    }
    let (r, g, b) = color.rgb_components();
    write!(stdout, "[38;2;{r};{g};{b}m")
}

fn write_bg(stdout: &mut io::Stdout, color: Color) -> io::Result<()> {
    if color == DEFAULT_BG {
        return write!(stdout, "[49m");
    }
    if let Some(index) = color.ansi_index_value() {
        return write_index_bg(stdout, index);
    }
    let (r, g, b) = color.rgb_components();
    write!(stdout, "[48;2;{r};{g};{b}m")
}

fn write_index_fg(stdout: &mut io::Stdout, index: u8) -> io::Result<()> {
    match index {
        0..=7 => write!(stdout, "[{}m", 30 + index),
        8..=15 => write!(stdout, "[{}m", 90 + index - 8),
        _ => write!(stdout, "[38;5;{index}m"),
    }
}

fn write_index_bg(stdout: &mut io::Stdout, index: u8) -> io::Result<()> {
    match index {
        0..=7 => write!(stdout, "[{}m", 40 + index),
        8..=15 => write!(stdout, "[{}m", 100 + index - 8),
        _ => write!(stdout, "[48;5;{index}m"),
    }
}

#[cfg(test)]
fn ansi_index_fg(index: u8) -> String {
    match index {
        0..=7 => format!("[{}m", 30 + index),
        8..=15 => format!("[{}m", 90 + index - 8),
        _ => format!("[38;5;{index}m"),
    }
}

#[cfg(test)]
fn ansi_index_bg(index: u8) -> String {
    match index {
        0..=7 => format!("[{}m", 40 + index),
        8..=15 => format!("[{}m", 100 + index - 8),
        _ => format!("[48;5;{index}m"),
    }
}

#[cfg(test)]
mod tests {
    use super::{ansi_index_bg, ansi_index_fg};

    #[test]
    fn low_ansi_indexes_use_classic_sgr_sequences() {
        assert_eq!(ansi_index_fg(4), "[34m");
        assert_eq!(ansi_index_fg(12), "[94m");
        assert_eq!(ansi_index_bg(4), "[44m");
        assert_eq!(ansi_index_bg(12), "[104m");
    }

    #[test]
    fn high_ansi_indexes_use_256_color_sequences() {
        assert_eq!(ansi_index_fg(42), "[38;5;42m");
        assert_eq!(ansi_index_bg(42), "[48;5;42m");
    }
}
