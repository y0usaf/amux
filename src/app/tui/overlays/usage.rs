use crate::app::cell_surface::{draw_box, render_cell_scrollbar, truncate_to_cells, CellSurface};
use crate::app::layout::CellRect as Rect;
use crate::app::theme::{self, DerivedTheme};
use crate::pi::{self, PiUsageDay, PiUsageReport, PiUsageTotals};

use super::super::raw::terminal_size;
use super::dialog::render_dialog_title_line;

const USAGE_MODEL_COLS: usize = 28;
const USAGE_FOOTER_HINT: &str = "↑/↓/j/k · r reload · q/Esc";

#[derive(Clone, Debug)]
pub(in crate::app::tui) struct UsageOverlayState {
    pub(in crate::app::tui) report: PiUsageReport,
    pub(in crate::app::tui) scroll: usize,
}

impl UsageOverlayState {
    pub(in crate::app::tui) fn load() -> Self {
        Self {
            report: pi::load_usage_report(),
            scroll: 0,
        }
    }

    pub(in crate::app::tui) fn reload(&mut self) {
        self.report = pi::load_usage_report();
        self.scroll = 0;
    }

    pub(in crate::app::tui) fn clamp_scroll(&mut self, visible_rows: usize) {
        let lines = self.report.days.len();
        self.scroll = self.scroll.min(lines.saturating_sub(visible_rows));
    }
}

pub(in crate::app::tui) fn usage_overlay_visible_rows_for_terminal() -> usize {
    let (cols, rows) = terminal_size();
    usage_overlay_list_rows(usage_overlay_rect(i32::from(cols), i32::from(rows)))
}

fn usage_overlay_rect(cols: i32, rows: i32) -> Rect {
    let cols = cols.max(1);
    let rows = rows.max(1);
    let width = cols.clamp(1, 118);
    let height = rows.clamp(1, 34);
    Rect::new((cols - width) / 2, (rows - height) / 2, width, height)
}

fn usage_overlay_list_rows(rect: Rect) -> usize {
    rect.rows.saturating_sub(6) as usize
}

#[cfg(test)]
/// Scrollable daily rows; the total row stays fixed in the overlay.
pub(in crate::app::tui) fn usage_overlay_lines(report: &PiUsageReport) -> Vec<String> {
    report.days.iter().map(usage_day_line).collect()
}

pub(in crate::app::tui) fn render_usage_overlay(
    surface: &mut CellSurface,
    usage: &mut UsageOverlayState,
    theme: &DerivedTheme,
) {
    let rect = usage_overlay_rect(surface.cols, surface.rows);
    draw_box(
        surface,
        rect,
        theme.text,
        theme.surface_raised,
        theme.status_bg,
    );
    if rect.cols <= 2 || rect.rows <= 2 {
        return;
    }

    let inner = rect.inset_edges(2, 1, 2, 1);
    surface.fill_rect(inner, theme.text, theme.surface);

    let count = format!(" {} ", usage.report.entries);
    render_dialog_title_line(
        surface, inner.row, inner.col, inner.cols, " USAGE ", &count, theme,
    );

    let header_row = inner.row + 1;
    let total_row = inner.row + 2;
    let list_row = inner.row + 3;
    let footer_row = inner.row + inner.rows - 1;
    let list_rows = (footer_row - list_row).max(0) as usize;
    let list_width = (inner.cols - 1).max(0);

    let header_line = usage_header_line();
    surface.put_text(
        inner.col,
        header_row,
        inner.cols,
        theme::brighten(theme.muted, 24),
        theme.surface,
        &header_line,
    );

    let total_line = usage_total_line(&usage.report.totals);
    surface.put_text_bold(
        inner.col,
        total_row,
        inner.cols,
        theme.text,
        theme.surface,
        &total_line,
    );

    usage.clamp_scroll(list_rows);

    for (row_offset, day) in usage
        .report
        .days
        .iter()
        .skip(usage.scroll)
        .take(list_rows)
        .enumerate()
    {
        let row = list_row + row_offset as i32;
        let line = usage_day_line(day);
        surface.put_text(
            inner.col,
            row,
            list_width,
            theme::brighten(theme.muted, 32),
            theme.surface,
            &truncate_to_cells(&line, list_width as usize),
        );
    }
    render_cell_scrollbar(
        surface,
        inner.col + inner.cols - 1,
        list_row,
        list_rows as i32,
        list_rows,
        usage.report.days.len(),
        usage.scroll,
        theme.border,
        theme.surface,
        "╎",
        theme.accent_2,
        "┃",
    );

    surface.put_text(
        inner.col,
        footer_row,
        inner.cols,
        theme.muted,
        theme.surface,
        &truncate_to_cells(USAGE_FOOTER_HINT, inner.cols.max(0) as usize),
    );
}

fn usage_header_line() -> String {
    format!(
        "{:<10} {:<28} {:>10} {:>10} {:>10} {:>10} {:>10} {:>9}",
        "DATE", "MODELS", "INPUT", "OUTPUT", "CACHE+", "CACHE↺", "TOTAL", "COST"
    )
}

fn usage_total_line(totals: &PiUsageTotals) -> String {
    usage_row_line("TOTAL", "", totals)
}

fn usage_day_line(day: &PiUsageDay) -> String {
    usage_row_line(
        &day.date,
        &format_models_display(&day.models_used),
        &day.totals,
    )
}

fn usage_row_line(label: &str, models: &str, totals: &PiUsageTotals) -> String {
    format!(
        "{:<10} {:<28} {:>10} {:>10} {:>10} {:>10} {:>10} {:>9}",
        label,
        truncate_to_cells(models, USAGE_MODEL_COLS),
        format_u64(totals.input_tokens),
        format_u64(totals.output_tokens),
        format_u64(totals.cache_creation_tokens),
        format_u64(totals.cache_read_tokens),
        format_u64(totals.total_tokens()),
        format_currency(totals.total_cost)
    )
}

fn format_models_display(models: &[String]) -> String {
    let mut models = models
        .iter()
        .map(|model| format_model_name(model))
        .collect::<Vec<_>>();
    models.sort();
    models.dedup();
    models.join(", ")
}

fn format_model_name(model: &str) -> String {
    if let Some(rest) = model.strip_prefix("[pi] ") {
        return format!("[pi] {}", format_model_name(rest));
    }

    if let Some(rest) = model.strip_prefix("anthropic/") {
        return format_model_name(rest);
    }

    let Some(rest) = model.strip_prefix("claude-") else {
        return model.to_string();
    };

    let mut parts = rest.split('-').collect::<Vec<_>>();
    if parts
        .last()
        .is_some_and(|part| part.len() == 8 && part.chars().all(|ch| ch.is_ascii_digit()))
    {
        parts.pop();
    }
    parts.join("-")
}

fn format_u64(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

fn format_currency(amount: f64) -> String {
    format!("${amount:.2}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pi::{PiUsageDay, PiUsageTotals};

    #[test]
    fn formats_model_names_like_pi_usage() {
        assert_eq!(format_model_name("[pi] claude-opus-4-5"), "[pi] opus-4-5");
        assert_eq!(
            format_model_name("[pi] claude-sonnet-4-20250514"),
            "[pi] sonnet-4"
        );
        assert_eq!(
            format_model_name("[pi] anthropic/claude-opus-4.5"),
            "[pi] opus-4.5"
        );
    }

    #[test]
    fn usage_rows_include_totals_and_cost() {
        let report = PiUsageReport {
            days: vec![PiUsageDay {
                date: "2026-01-02".into(),
                totals: PiUsageTotals {
                    input_tokens: 7,
                    output_tokens: 8,
                    cache_creation_tokens: 1,
                    cache_read_tokens: 2,
                    total_cost: 0.01,
                },
                models_used: vec!["[pi] claude-sonnet-4".into()],
                model_breakdowns: Vec::new(),
            }],
            totals: PiUsageTotals {
                input_tokens: 1000,
                output_tokens: 50,
                cache_creation_tokens: 20,
                cache_read_tokens: 10,
                total_cost: 0.055,
            },
            files_scanned: 1,
            entries: 1,
            skipped_duplicates: 0,
        };

        let daily_rows = usage_overlay_lines(&report);

        assert_eq!(daily_rows.len(), 1);
        assert!(daily_rows[0].contains("2026-01-02"));
        assert!(daily_rows[0].contains("[pi] sonnet-4"));
        assert!(daily_rows[0].contains("18"));
        assert!(daily_rows[0].contains("$0.01"));

        let total_row = usage_total_line(&report.totals);

        assert!(total_row.contains("TOTAL"));
        assert!(total_row.contains("1,080"));
        assert!(total_row.contains("$0.06"));
    }
}
