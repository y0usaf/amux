use std::path::PathBuf;

use crate::pi::{PiSessionStage, PiSidecarSnapshot};
use crate::state::Session;

use super::{
    cell_surface::CellSurface,
    layout::CellRect,
    scene::{render_sidebar, ScenePalette},
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
        tool_name: None,
        ts_ms: 1,
    }
}

const PROJECT_TITLE_FIRST_CELL: usize = 7;
const PROJECT_TITLE_LAST_CELL: usize = 15;

fn render_project_title(status: Option<SidebarStatusKind>) -> (ScenePalette, CellSurface) {
    let palette = ScenePalette::themed(DerivedTheme::fallback());
    let mut surface = CellSurface::new(24, 3, palette.fg, palette.bg);
    let rows = [SidebarRow {
        kind: SidebarRowKind::Project(0),
        text: "Project".into(),
        fg: theme::HEADING,
        bg: None,
        inverted: false,
        selector: false,
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
    assert_eq!(
        sidebar::sidebar_status_glyph(sidebar::SidebarStatusKind::Notification, 0),
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
        fg: theme::TEXT,
        bg: None,
        inverted: false,
        selector: false,
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

    let title_cell = &surface.cells[2];
    assert_eq!(title_cell.text, "U");
    assert_eq!(title_cell.fg, palette.success);
}

#[test]
fn project_title_uses_lifted_statusbar_purple_without_status() {
    let (palette, surface) = render_project_title(None);
    let idle_title_fg = theme::brighten(palette.statusbar_bg, 36);

    let first_title_cell = &surface.cells[PROJECT_TITLE_FIRST_CELL];
    let last_title_cell = &surface.cells[PROJECT_TITLE_LAST_CELL];
    assert_eq!(first_title_cell.text, "[");
    assert_eq!(first_title_cell.fg, idle_title_fg);
    assert_eq!(last_title_cell.text, "]");
    assert_eq!(last_title_cell.fg, idle_title_fg);
}

#[test]
fn active_project_title_uses_gradient() {
    let (palette, surface) = render_project_title(Some(SidebarStatusKind::Active));

    let first_title_cell = &surface.cells[PROJECT_TITLE_FIRST_CELL];
    let last_title_cell = &surface.cells[PROJECT_TITLE_LAST_CELL];
    assert_eq!(first_title_cell.text, "[");
    assert_eq!(first_title_cell.fg, palette.accent);
    assert_eq!(last_title_cell.text, "]");
    assert_ne!(last_title_cell.fg, first_title_cell.fg);
}

#[test]
fn completed_project_title_uses_success_green() {
    let (palette, surface) = render_project_title(Some(SidebarStatusKind::Notification));

    let first_title_cell = &surface.cells[PROJECT_TITLE_FIRST_CELL];
    let last_title_cell = &surface.cells[PROJECT_TITLE_LAST_CELL];
    assert_eq!(first_title_cell.text, "[");
    assert_eq!(first_title_cell.fg, palette.success);
    assert_eq!(last_title_cell.text, "]");
    assert_eq!(last_title_cell.fg, palette.success);
}

#[test]
fn selected_empty_project_draws_sidebar_selector() {
    let palette = ScenePalette::themed(DerivedTheme::fallback());
    let mut surface = CellSurface::new(24, 3, palette.fg, palette.bg);
    let rows = [SidebarRow {
        kind: SidebarRowKind::Project(0),
        text: "PROJECT".into(),
        fg: theme::TEXT,
        bg: None,
        inverted: true,
        selector: true,
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

    assert_eq!(surface.cells[0].text, ">");
    assert_eq!(surface.cells[0].fg, palette.accent);
}

#[test]
fn spinner_glyph_advances_with_frame_interval() {
    let first = sidebar::sidebar_status_glyph(sidebar::SidebarStatusKind::Active, 0);
    let second = sidebar::sidebar_status_glyph(
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
