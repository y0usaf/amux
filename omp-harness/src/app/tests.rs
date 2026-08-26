use std::path::PathBuf;

use super::backend::ChromeView;
use crate::omp::{PiSessionStage, PiSidecarSnapshot};
use crate::state::Session;

use super::{
    cell_surface::CellSurface,
    glyphs::GlyphSet,
    layout::CellRect,
    scene::{render_sidebar, render_statusbar, HarnessMode, ScenePalette},
    sidebar::{self, SidebarRow, SidebarRowKind, SidebarStatusKind, SidebarViewportItem},
    sidecar_sync,
    theme::{self, DerivedTheme},
    workspace,
};

fn snapshot(stage: PiSessionStage, queued: bool) -> PiSidecarSnapshot {
    PiSidecarSnapshot {
        kind: Some("snapshot".into()),
        session_id: "pi-session-1".into(),
        harness_session_id: Some("local-session-1".into()),
        session_file: Some(PathBuf::from("/tmp/pi-session-1.jsonl")),
        session_name: Some("Test session".into()),
        stage,
        queued,
        interrupted: false,
        tool_name: None,
        ts_ms: 1,
    }
}

// Mirrored header `──────────── Project ✦ ` on a 24-cell sidebar whose last
// column is the scrollbar: 23 content cells - 7 title cells - 4 fixed cells
// leaves a 12-cell rule, so "Project" occupies cells 13..=19 and the jewel
// lands on cell 21, two cells from the Pi window.
const PROJECT_TITLE_FIRST_CELL: usize = 13;
const PROJECT_TITLE_LAST_CELL: usize = 19;
const PROJECT_JEWEL_CELL: usize = 21;
const PROJECT_RULE_CELL: usize = 0;

fn render_project_title(status: Option<SidebarStatusKind>) -> (ScenePalette, CellSurface) {
    render_project_title_with_current(status, false)
}

fn render_project_title_with_current(
    status: Option<SidebarStatusKind>,
    current: bool,
) -> (ScenePalette, CellSurface) {
    let palette = ScenePalette::themed(DerivedTheme::fallback());
    let mut surface = CellSurface::new(24, 3, palette.fg, palette.bg);
    let rows = [SidebarRow {
        kind: SidebarRowKind::Project(0),
        text: "Project".into(),
        fg: theme::Role::Heading,
        bg: None,
        inverted: false,
        selector: false,
        current,
        status,
    }];
    let viewport = [SidebarViewportItem {
        row_index: 0,
        visible_row: 0,
        sticky: false,
    }];

    render_sidebar(
        &mut surface,
        CellRect::new(0, 0, 24, 3),
        CellRect::new(0, 0, 24, 1),
        &rows,
        &viewport,
        None,
        &palette,
        0,
    );

    (palette, surface)
}

#[test]
fn idle_snapshot_does_not_bind_empty_draft() {
    let session = Session::new_draft();
    assert!(!sidecar_sync::should_bind_sidecar_session(
        &session,
        &snapshot(PiSessionStage::Idle, false)
    ));
}

#[test]
fn active_snapshot_binds_new_session() {
    let session = Session::new_draft();
    assert!(sidecar_sync::should_bind_sidecar_session(
        &session,
        &snapshot(PiSessionStage::Thinking, false)
    ));
}

#[test]
fn queued_snapshot_binds_new_session() {
    let session = Session::new_draft();
    assert!(sidecar_sync::should_bind_sidecar_session(
        &session,
        &snapshot(PiSessionStage::Idle, true)
    ));
}

#[test]
fn unmaterialized_sidecar_session_does_not_reorder() {
    assert_eq!(
        sidecar_sync::sidecar_order_update(false, true, false, false),
        sidecar_sync::SidecarOrderUpdate::None
    );
    assert_eq!(
        sidecar_sync::sidecar_order_update(true, false, false, false),
        sidecar_sync::SidecarOrderUpdate::None
    );
}

#[test]
fn sidecar_session_promotes_once_materialized() {
    assert_eq!(
        sidecar_sync::sidecar_order_update(false, true, false, true),
        sidecar_sync::SidecarOrderUpdate::Promote
    );
    assert_eq!(
        sidecar_sync::sidecar_order_update(true, true, false, true),
        sidecar_sync::SidecarOrderUpdate::Promote
    );
    assert_eq!(
        sidecar_sync::sidecar_order_update(true, false, false, true),
        sidecar_sync::SidecarOrderUpdate::Promote
    );
    assert_eq!(
        sidecar_sync::sidecar_order_update(true, false, true, true),
        sidecar_sync::SidecarOrderUpdate::Touch
    );
}

#[test]
fn sidebar_status_prefers_active_over_notification() {
    let mut session = Session::new_draft();
    session.runtime.running = true;
    session.runtime.unread = true;

    assert_eq!(
        sidebar::session_sidebar_status(&session),
        Some(sidebar::SidebarStatusKind::Active)
    );
}

#[test]
fn notification_status_uses_static_glyph() {
    let glyphs = GlyphSet::unicode();
    assert_eq!(
        sidebar::sidebar_status_glyph(&glyphs, sidebar::SidebarStatusKind::Notification, 0),
        sidebar::SIDEBAR_NOTIFICATION_GLYPH
    );
}

#[test]
fn interrupted_status_uses_notification_glyph() {
    let glyphs = GlyphSet::unicode();
    assert_eq!(
        sidebar::sidebar_status_glyph(&glyphs, sidebar::SidebarStatusKind::Interrupted, 0),
        sidebar::SIDEBAR_NOTIFICATION_GLYPH
    );
}

#[test]
fn sidebar_status_prefers_queued_over_notification() {
    let mut session = Session::new_draft();
    session.runtime.queued = true;
    session.runtime.unread = true;

    assert_eq!(
        sidebar::session_sidebar_status(&session),
        Some(sidebar::SidebarStatusKind::Queued)
    );
}

#[test]
fn notification_status_tints_session_title() {
    let palette = ScenePalette::themed(DerivedTheme::fallback());
    let mut surface = CellSurface::new(24, 3, palette.fg, palette.bg);
    let rows = [SidebarRow {
        kind: SidebarRowKind::Session {
            project_index: 0,
            session_index: 0,
        },
        text: "Unread session".into(),
        fg: theme::Role::Text,
        bg: None,
        inverted: false,
        selector: false,
        current: false,
        status: Some(SidebarStatusKind::Notification),
    }];
    let viewport = [SidebarViewportItem {
        row_index: 0,
        visible_row: 0,
        sticky: false,
    }];

    render_sidebar(
        &mut surface,
        CellRect::new(0, 0, 24, 3),
        CellRect::new(0, 0, 24, 1),
        &rows,
        &viewport,
        None,
        &palette,
        0,
    );

    let title_cell = &surface.cells[3];
    assert_eq!(title_cell.text, "U");
    assert_eq!(title_cell.fg, palette.success);
}

#[test]
fn interrupted_status_tints_session_title_sriracha() {
    let palette = ScenePalette::themed(DerivedTheme::fallback());
    let mut surface = CellSurface::new(24, 3, palette.fg, palette.bg);
    let rows = [SidebarRow {
        kind: SidebarRowKind::Session {
            project_index: 0,
            session_index: 0,
        },
        text: "Interrupted session".into(),
        fg: theme::Role::Text,
        bg: None,
        inverted: false,
        selector: false,
        current: false,
        status: Some(SidebarStatusKind::Interrupted),
    }];
    let viewport = [SidebarViewportItem {
        row_index: 0,
        visible_row: 0,
        sticky: false,
    }];

    render_sidebar(
        &mut surface,
        CellRect::new(0, 0, 24, 3),
        CellRect::new(0, 0, 24, 1),
        &rows,
        &viewport,
        None,
        &palette,
        0,
    );

    let title_cell = &surface.cells[3];
    assert_eq!(title_cell.text, "I");
    assert_eq!(title_cell.fg, palette.error);
}

#[test]
fn project_title_uses_accent_text_without_status() {
    let (palette, surface) = render_project_title(None);
    let idle_title_fg = palette.accent;

    let first_title_cell = &surface.cells[PROJECT_TITLE_FIRST_CELL];
    let last_title_cell = &surface.cells[PROJECT_TITLE_LAST_CELL];
    assert_eq!(first_title_cell.text, "P");
    assert_eq!(first_title_cell.fg, idle_title_fg);
    assert_eq!(last_title_cell.text, "t");
    assert_eq!(last_title_cell.fg, idle_title_fg);
}

#[test]
fn active_project_title_uses_running_color() {
    let (palette, surface) = render_project_title(Some(SidebarStatusKind::Active));

    let first_title_cell = &surface.cells[PROJECT_TITLE_FIRST_CELL];
    let last_title_cell = &surface.cells[PROJECT_TITLE_LAST_CELL];
    assert_eq!(first_title_cell.text, "P");
    assert_eq!(first_title_cell.fg, palette.running);
    assert_eq!(last_title_cell.text, "t");
    assert_eq!(last_title_cell.fg, palette.running);
}

#[test]
fn completed_project_title_uses_success_green() {
    let (palette, surface) = render_project_title(Some(SidebarStatusKind::Notification));

    let first_title_cell = &surface.cells[PROJECT_TITLE_FIRST_CELL];
    let last_title_cell = &surface.cells[PROJECT_TITLE_LAST_CELL];
    assert_eq!(first_title_cell.text, "P");
    assert_eq!(first_title_cell.fg, palette.success);
    assert_eq!(last_title_cell.text, "t");
    assert_eq!(last_title_cell.fg, palette.success);
}

#[test]
fn interrupted_project_title_uses_sriracha() {
    let (palette, surface) = render_project_title(Some(SidebarStatusKind::Interrupted));

    let first_title_cell = &surface.cells[PROJECT_TITLE_FIRST_CELL];
    let last_title_cell = &surface.cells[PROJECT_TITLE_LAST_CELL];
    assert_eq!(first_title_cell.text, "P");
    assert_eq!(first_title_cell.fg, palette.error);
    assert_eq!(last_title_cell.text, "t");
    assert_eq!(last_title_cell.fg, palette.error);
}

#[test]
fn current_project_header_jewel_uses_statusbar_white_without_status() {
    let (palette, surface) = render_project_title_with_current(None, true);

    // Mirrored header `─── title ✦ `: the leading rule stays border gray and
    // the jewel, pinned to the Pi-facing edge, carries the "current" state.
    let jewel_cell = &surface.cells[PROJECT_JEWEL_CELL];
    assert_eq!(jewel_cell.text, "✦");
    assert_eq!(jewel_cell.fg, palette.statusbar_fg);
    let rule_cell = &surface.cells[PROJECT_RULE_CELL];
    assert_eq!(rule_cell.text, "─");
    assert_eq!(rule_cell.fg, palette.border);
}

#[test]
fn sidebar_divider_colors_only_the_glyph() {
    let palette = ScenePalette::themed(DerivedTheme::fallback());
    let mut surface = CellSurface::new(6, 2, palette.fg, palette.bg);
    let rows = [SidebarRow {
        kind: SidebarRowKind::Label,
        text: "sidebar".into(),
        fg: theme::Role::Text,
        bg: None,
        inverted: false,
        selector: false,
        current: false,
        status: None,
    }];
    let viewport = [SidebarViewportItem {
        row_index: 0,
        visible_row: 0,
        sticky: false,
    }];

    render_sidebar(
        &mut surface,
        CellRect::new(0, 0, 6, 2),
        CellRect::new(0, 0, 6, 1),
        &rows,
        &viewport,
        None,
        &palette,
        0,
    );

    let divider_cell = &surface.cells[5];
    assert_eq!(divider_cell.text, "│");
    assert_eq!(divider_cell.fg, theme::TRANSPARENT);
    assert_eq!(divider_cell.bg, palette.sidebar_bg);
    assert!(!divider_cell.reverse);
}

#[test]
fn statusbar_sidebar_separator_colors_only_the_glyph() {
    let palette = ScenePalette::themed(DerivedTheme::fallback());
    let mut surface = CellSurface::new(12, 1, palette.fg, palette.bg);
    let chrome = ChromeView {
        project: "project".into(),
        status: String::new(),
        status_kind: None,
        session: String::new(),
    };

    render_statusbar(
        &mut surface,
        CellRect::new(0, 0, 12, 1),
        Some(CellRect::new(0, 0, 4, 1)),
        &chrome,
        &palette,
        HarnessMode::Normal,
    );

    let separator_cell = &surface.cells[3];
    assert_eq!(separator_cell.text, "│");
    assert_eq!(separator_cell.fg, theme::TRANSPARENT);
    assert_eq!(separator_cell.bg, palette.statusbar_bg);
    assert!(!separator_cell.reverse);
}

#[test]
fn statusbar_sidebar_segment_drops_decorative_rule() {
    let palette = ScenePalette::themed(DerivedTheme::fallback());
    let mut surface = CellSurface::new(20, 1, palette.fg, palette.bg);
    let chrome = ChromeView {
        project: "project".into(),
        status: String::new(),
        status_kind: None,
        session: String::new(),
    };

    render_statusbar(
        &mut surface,
        CellRect::new(0, 0, 20, 1),
        Some(CellRect::new(0, 0, 14, 1)),
        &chrome,
        &palette,
        HarnessMode::Normal,
    );

    assert!(surface.cells.iter().all(|cell| cell.text != "╱"));
    assert_eq!(surface.cells[9].text, " ");
    assert_eq!(surface.cells[10].text, " ");
}

#[test]
fn active_project_header_jewel_uses_running_color_not_focus_white() {
    let (palette, surface) =
        render_project_title_with_current(Some(SidebarStatusKind::Active), true);

    // The leading rule stays border gray; the jewel signals the running state.
    let jewel_cell = &surface.cells[PROJECT_JEWEL_CELL];
    assert_eq!(jewel_cell.fg, palette.running);
    assert_ne!(jewel_cell.fg, palette.statusbar_fg);
    let rule_cell = &surface.cells[PROJECT_RULE_CELL];
    assert_eq!(rule_cell.text, "─");
    assert_eq!(rule_cell.fg, palette.border);
}

#[test]
fn selected_empty_project_draws_accent_crown_jewel() {
    let palette = ScenePalette::themed(DerivedTheme::fallback());
    let mut surface = CellSurface::new(24, 3, palette.fg, palette.bg);
    let rows = [SidebarRow {
        kind: SidebarRowKind::Project(0),
        text: "PROJECT".into(),
        fg: theme::Role::Text,
        bg: None,
        inverted: true,
        selector: true,
        current: true,
        status: None,
    }];
    let viewport = [SidebarViewportItem {
        row_index: 0,
        visible_row: 0,
        sticky: false,
    }];

    render_sidebar(
        &mut surface,
        CellRect::new(0, 0, 24, 3),
        CellRect::new(0, 0, 24, 1),
        &rows,
        &viewport,
        None,
        &palette,
        0,
    );

    assert_eq!(surface.cells[PROJECT_JEWEL_CELL].text, "✦");
    assert_eq!(surface.cells[PROJECT_JEWEL_CELL].fg, palette.accent);
}

#[test]
fn spinner_glyph_advances_with_frame_interval() {
    let glyphs = GlyphSet::unicode();
    let first = sidebar::sidebar_status_glyph(&glyphs, sidebar::SidebarStatusKind::Active, 0);
    let second = sidebar::sidebar_status_glyph(
        &glyphs,
        sidebar::SidebarStatusKind::Active,
        sidebar::SIDEBAR_SPINNER_FRAME_MS,
    );

    assert_eq!(first, sidebar::SIDEBAR_SPINNER_FRAMES[0]);
    assert_eq!(second, sidebar::SIDEBAR_SPINNER_FRAMES[1]);
}

#[test]
fn materialized_session_always_binds_even_when_idle() {
    let mut session = Session::new_draft();
    session.session_file = Some(PathBuf::from("/tmp/pi-session-1.jsonl"));

    assert!(sidecar_sync::should_bind_sidecar_session(
        &session,
        &snapshot(PiSessionStage::Idle, false)
    ));
}

#[test]
fn sidecar_order_update_only_touches_after_trackable_running_session_finishes() {
    assert_eq!(
        sidecar_sync::sidecar_order_update(true, false, true, false),
        sidecar_sync::SidecarOrderUpdate::None
    );
    assert_eq!(
        sidecar_sync::sidecar_order_update(false, false, true, true),
        sidecar_sync::SidecarOrderUpdate::None
    );
}

#[test]
fn project_path_normalization_preserves_order() {
    let paths = workspace::normalize_unique_project_paths(vec![
        PathBuf::from("/tmp/project-b"),
        PathBuf::from("/tmp/project-a"),
        PathBuf::from("/tmp/project-b"),
    ]);

    assert_eq!(
        paths,
        vec![
            PathBuf::from("/tmp/project-b"),
            PathBuf::from("/tmp/project-a")
        ]
    );
}

#[test]
fn ascii_glyph_set_uses_plain_box_and_spinner() {
    let glyphs = GlyphSet::ascii();
    assert_eq!(glyphs.spinner, &["-", "\\", "|", "/"]);
    assert_eq!(glyphs.box_.h, "-");
    assert_eq!(glyphs.box_.v, "|");
    assert_eq!(glyphs.box_.tl, "+");
    assert_eq!(glyphs.box_.tr, "+");
    assert_eq!(glyphs.box_.bl, "+");
    assert_eq!(glyphs.box_.br, "+");
    assert_eq!(glyphs.scrollbar_track, "|");
    assert_eq!(glyphs.scrollbar_thumb, "|");
}
