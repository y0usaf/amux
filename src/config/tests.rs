use super::*;

#[test]
fn parses_chorded_key_sequence() {
    let sequence = parse_key_sequence("ctrl+p n").expect("sequence");
    assert_eq!(sequence.len(), 2);
    assert_eq!(sequence[0], KeyStroke::parse("ctrl+p").unwrap());
    assert_eq!(sequence[1], KeyStroke::parse("n").unwrap());
}

#[test]
fn defaults_use_ctrl_arrows_for_navigation() {
    let config = AppConfig::default();
    let keymap = config.keymap();
    let mut state = KeyChordState::default();

    assert_eq!(
        keymap.advance(&mut state, KeyStroke::parse("ctrl+left").unwrap()),
        KeymapMatch::Triggered(AppAction::PreviousProject)
    );
    assert_eq!(
        keymap.advance(&mut state, KeyStroke::parse("ctrl+down").unwrap()),
        KeymapMatch::Triggered(AppAction::NextSession)
    );
}

#[test]
fn defaults_bind_refresh_and_reload() {
    let config = AppConfig::default();
    let keymap = config.keymap();
    let mut state = KeyChordState::default();

    assert_eq!(
        keymap.advance(&mut state, KeyStroke::parse("ctrl+r").unwrap()),
        KeymapMatch::Triggered(AppAction::RefreshSession)
    );
    assert_eq!(
        keymap.advance(&mut state, KeyStroke::parse("ctrl+shift+r").unwrap()),
        KeymapMatch::Triggered(AppAction::RefreshAllSessions)
    );
}

#[test]
fn config_overrides_navigation_defaults() {
    let mut config = AppConfig::default();
    config.keybinds.insert(
        "project_prev".into(),
        ConfigKeybind::Single("ctrl+h".into()),
    );
    config.keybinds.insert(
        "project_next".into(),
        ConfigKeybind::Single("ctrl+l".into()),
    );
    config.keybinds.insert(
        "session_prev".into(),
        ConfigKeybind::Single("ctrl+k".into()),
    );
    config.keybinds.insert(
        "session_next".into(),
        ConfigKeybind::Single("ctrl+j".into()),
    );

    let keymap = config.keymap();
    let mut state = KeyChordState::default();

    assert_eq!(
        keymap.advance(&mut state, KeyStroke::parse("ctrl+h").unwrap()),
        KeymapMatch::Triggered(AppAction::PreviousProject)
    );
    assert_eq!(
        keymap.advance(&mut state, KeyStroke::parse("ctrl+left").unwrap()),
        KeymapMatch::NoMatch
    );
}

#[test]
fn chord_state_tracks_pending_prefixes() {
    let mut config = AppConfig::default();
    config.keybinds.insert(
        "new_session".into(),
        ConfigKeybind::Single("ctrl+p n".into()),
    );

    let keymap = config.keymap();
    let mut state = KeyChordState::default();

    assert_eq!(
        keymap.advance(&mut state, KeyStroke::parse("ctrl+p").unwrap()),
        KeymapMatch::Pending
    );
    assert_eq!(state.pending(), &[KeyStroke::parse("ctrl+p").unwrap()]);
    assert_eq!(
        keymap.advance(&mut state, KeyStroke::parse("n").unwrap()),
        KeymapMatch::Triggered(AppAction::NewSession)
    );
    assert!(state.pending().is_empty());
}

#[test]
fn invalid_override_falls_back_to_defaults() {
    let mut config = AppConfig::default();
    config.keybinds.insert(
        "project_prev".into(),
        ConfigKeybind::Single("ctrl+totally-invalid".into()),
    );

    let keymap = config.keymap();
    let mut state = KeyChordState::default();

    assert_eq!(
        keymap.advance(&mut state, KeyStroke::parse("ctrl+left").unwrap()),
        KeymapMatch::Triggered(AppAction::PreviousProject)
    );
}

#[test]
fn blank_multiple_override_disables_action_bindings() {
    let mut config = AppConfig::default();
    config.keybinds.insert(
        "project_prev".into(),
        ConfigKeybind::Multiple(vec![" ".into(), "".into()]),
    );

    let keymap = config.keymap();
    let mut state = KeyChordState::default();

    assert_eq!(
        keymap.advance(&mut state, KeyStroke::parse("ctrl+left").unwrap()),
        KeymapMatch::NoMatch
    );
}

#[test]
fn blank_single_override_disables_action_bindings() {
    let mut config = AppConfig::default();
    config
        .keybinds
        .insert("project_prev".into(), ConfigKeybind::Single("   ".into()));

    let keymap = config.keymap();
    let mut state = KeyChordState::default();

    assert_eq!(
        keymap.advance(&mut state, KeyStroke::parse("ctrl+left").unwrap()),
        KeymapMatch::NoMatch
    );
}

#[test]
fn conflicting_override_keeps_earlier_action_binding() {
    let mut config = AppConfig::default();
    config.keybinds.insert(
        "new_session".into(),
        ConfigKeybind::Single("ctrl+left".into()),
    );

    let keymap = config.keymap();
    let mut state = KeyChordState::default();

    assert_eq!(
        keymap.advance(&mut state, KeyStroke::parse("ctrl+left").unwrap()),
        KeymapMatch::Triggered(AppAction::PreviousProject)
    );
    assert_eq!(
        keymap.advance(&mut state, KeyStroke::parse("ctrl+n").unwrap()),
        KeymapMatch::NoMatch
    );
}

#[test]
fn whitespace_in_keybinds_is_normalized() {
    let mut config = AppConfig::default();
    config.keybinds.insert(
        "new_session".into(),
        ConfigKeybind::Multiple(vec!["  ctrl+p n  ".into(), "   ".into()]),
    );

    config.normalize();

    let keymap = config.keymap();
    let mut state = KeyChordState::default();
    assert_eq!(
        keymap.advance(&mut state, KeyStroke::parse("ctrl+p").unwrap()),
        KeymapMatch::Pending
    );
    assert_eq!(
        keymap.advance(&mut state, KeyStroke::parse("n").unwrap()),
        KeymapMatch::Triggered(AppAction::NewSession)
    );
}

#[test]
fn layout_widths_default_to_fixed_sidebar_columns() {
    assert_eq!(
        AppConfig::default().layout_widths(),
        LayoutWidths {
            sidebar: LayoutSidebarWidth::Columns(LAYOUT_SIDEBAR_WIDTH_DEFAULT),
        }
    );
    assert!(validate_layout_sidebar_width(8));
    assert!(validate_layout_sidebar_width(120));
    assert!(!validate_layout_sidebar_width(7));
    assert!(!validate_layout_sidebar_width(121));
}

#[test]
fn configured_sidebar_width_uses_fixed_columns() {
    let config = AppConfig {
        sidebar_width: Some(34),
        ..AppConfig::default()
    };

    assert_eq!(
        config.layout_widths(),
        LayoutWidths {
            sidebar: LayoutSidebarWidth::Columns(34),
        }
    );
}

#[test]
fn legacy_sidebar_width_percent_is_still_accepted() {
    let config = AppConfig {
        tui_sidebar_width_percent: Some(22),
        ..AppConfig::default()
    };

    assert_eq!(
        config.layout_widths(),
        LayoutWidths {
            sidebar: LayoutSidebarWidth::Percent(22),
        }
    );
    assert!(validate_layout_sidebar_width_percent(1));
    assert!(validate_layout_sidebar_width_percent(50));
    assert!(!validate_layout_sidebar_width_percent(0));
    assert!(!validate_layout_sidebar_width_percent(60));
}

#[test]
fn invalid_sidebar_width_falls_back_to_default_columns() {
    let config = AppConfig {
        sidebar_width: Some(121),
        ..AppConfig::default()
    };

    assert_eq!(
        config.layout_widths(),
        LayoutWidths {
            sidebar: LayoutSidebarWidth::Columns(LAYOUT_SIDEBAR_WIDTH_DEFAULT),
        }
    );
}

#[test]
fn parse_key_sequence_rejects_empty_input() {
    assert_eq!(parse_key_sequence("   "), Err("empty key sequence".into()));
}

#[test]
fn non_matching_second_stroke_falls_back_to_fresh_lookup() {
    let mut config = AppConfig::default();
    config.keybinds.insert(
        "new_session".into(),
        ConfigKeybind::Single("ctrl+p n".into()),
    );

    let keymap = config.keymap();
    let mut state = KeyChordState::default();
    assert_eq!(
        keymap.advance(&mut state, KeyStroke::parse("ctrl+p").unwrap()),
        KeymapMatch::Pending
    );
    assert_eq!(
        keymap.advance(&mut state, KeyStroke::parse("ctrl+left").unwrap()),
        KeymapMatch::Triggered(AppAction::PreviousProject)
    );
    assert!(state.pending().is_empty());
}
