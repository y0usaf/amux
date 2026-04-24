#[path = "terminal/controller.rs"]
mod controller;
#[path = "terminal/input.rs"]
mod input;
#[path = "terminal/process.rs"]
mod process;
#[path = "terminal/selection.rs"]
mod selection;

pub use controller::{TerminalController, TerminalStatus};
pub use process::TerminalTarget;
pub(crate) use selection::terminal_selection_span;
pub use selection::{TerminalSelectionPoint, TerminalSelectionRange};

#[cfg(test)]
mod tests {
    use super::{
        controller::disconnected_terminal_status, process::targets_share_process,
        terminal_selection_span, TerminalSelectionPoint, TerminalStatus, TerminalTarget,
    };
    use crate::terminal::selection::TerminalSelection;
    use std::path::PathBuf;

    fn target(session_file: Option<&str>) -> TerminalTarget {
        TerminalTarget {
            pi_binary: Some("pi".into()),
            sidecar_extension_path: Some(PathBuf::from("/tmp/sidecar.js")),
            sidecar_socket_path: PathBuf::from("/tmp/pi.sock"),
            harness_session_id: "local-session-1".into(),
            cwd: PathBuf::from("/tmp/project"),
            session_file: session_file.map(PathBuf::from),
        }
    }

    #[test]
    fn session_materialization_reuses_existing_process() {
        assert!(targets_share_process(
            Some(&target(None)),
            Some(&target(Some("/tmp/session.jsonl"))),
        ));
    }

    #[test]
    fn disconnected_terminal_status_promotes_active_states_to_error() {
        assert!(matches!(
            disconnected_terminal_status(&TerminalStatus::Launching),
            Some(TerminalStatus::Error(ref error)) if error == "terminal disconnected"
        ));
        assert!(matches!(
            disconnected_terminal_status(&TerminalStatus::Running),
            Some(TerminalStatus::Error(ref error)) if error == "terminal disconnected"
        ));
    }

    #[test]
    fn disconnected_terminal_status_preserves_terminal_end_states() {
        assert!(disconnected_terminal_status(&TerminalStatus::Empty).is_some());
        assert!(disconnected_terminal_status(&TerminalStatus::Exited("0".into())).is_none());
        assert!(disconnected_terminal_status(&TerminalStatus::Error("boom".into())).is_none());
    }

    #[test]
    fn process_identity_change_forces_restart() {
        let current = target(Some("/tmp/session-a.jsonl"));
        let mut next = target(Some("/tmp/session-b.jsonl"));
        next.cwd = PathBuf::from("/tmp/other-project");

        assert!(!targets_share_process(Some(&current), Some(&next)));
    }

    #[test]
    fn terminal_selection_normalizes_anchor_and_focus() {
        let mut selection = TerminalSelection::default();
        selection.set(TerminalSelectionPoint { row: 4, col: 7 });
        selection.update_focus(TerminalSelectionPoint { row: 2, col: 3 });

        let normalized = selection.normalized().expect("normalized selection");
        assert_eq!(normalized.start, TerminalSelectionPoint { row: 2, col: 3 });
        assert_eq!(normalized.end, TerminalSelectionPoint { row: 4, col: 7 });
    }

    #[test]
    fn terminal_selection_span_handles_single_and_multi_row_ranges() {
        let mut selection = TerminalSelection::default();
        selection.set(TerminalSelectionPoint { row: 1, col: 2 });
        selection.update_focus(TerminalSelectionPoint { row: 3, col: 8 });
        let range = selection.normalized();

        assert_eq!(terminal_selection_span(range, 0, 10), None);
        assert_eq!(terminal_selection_span(range, 1, 10), Some((2, 8)));
        assert_eq!(terminal_selection_span(range, 2, 10), Some((0, 10)));
        assert_eq!(terminal_selection_span(range, 3, 10), Some((0, 8)));
        assert_eq!(terminal_selection_span(range, 4, 10), None);

        selection.update_focus(TerminalSelectionPoint { row: 3, col: 80 });
        let range = selection.normalized();
        assert_eq!(terminal_selection_span(range, 1, 10), Some((2, 8)));
        assert_eq!(terminal_selection_span(range, 3, 10), Some((0, 10)));
    }

    #[test]
    fn target_without_session_file_still_reuses_same_process() {
        let current = target(Some("/tmp/session-a.jsonl"));
        let next = target(None);

        assert!(targets_share_process(Some(&current), Some(&next)));
    }

    #[test]
    fn different_materialized_session_files_force_restart() {
        let current = target(Some("/tmp/session-a.jsonl"));
        let next = target(Some("/tmp/session-b.jsonl"));

        assert!(!targets_share_process(Some(&current), Some(&next)));
    }

    #[test]
    fn target_change_in_harness_session_forces_restart() {
        let current = target(Some("/tmp/session.jsonl"));
        let mut next = target(Some("/tmp/session.jsonl"));
        next.harness_session_id = "local-session-2".into();

        assert!(!targets_share_process(Some(&current), Some(&next)));
    }

    #[test]
    fn single_point_selection_normalizes_to_none() {
        let mut selection = TerminalSelection::default();
        selection.set(TerminalSelectionPoint { row: 2, col: 4 });

        assert_eq!(selection.normalized(), None);
    }

    #[test]
    fn selection_focus_updates_without_anchor_still_has_no_range() {
        let mut selection = TerminalSelection::default();
        selection.update_focus(TerminalSelectionPoint { row: 2, col: 4 });

        assert_eq!(selection.normalized(), None);
    }
}
