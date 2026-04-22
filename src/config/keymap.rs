use std::collections::BTreeMap;

use super::{
    actions::{action_spec, action_spec_by_name},
    resolved_sequences, AppAction, AppConfig, KeyStroke, ACTION_SPECS,
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeyChordState {
    pending: Vec<KeyStroke>,
}

impl KeyChordState {
    pub fn clear(&mut self) {
        self.pending.clear();
    }

    pub fn pending(&self) -> &[KeyStroke] {
        &self.pending
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeymapMatch {
    NoMatch,
    Pending,
    Triggered(AppAction),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeymapHint {
    pub stroke: KeyStroke,
    pub action: Option<AppAction>,
    pub has_children: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Keymap {
    root: KeymapNode,
}

#[derive(Clone, Debug, Default)]
struct KeymapNode {
    action: Option<AppAction>,
    children: BTreeMap<KeyStroke, KeymapNode>,
}

fn sequence_text(sequence: &[KeyStroke]) -> String {
    sequence
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" ")
}

impl Keymap {
    pub fn from_config(config: &AppConfig) -> Self {
        for action_name in config.keybinds.keys() {
            if action_spec_by_name(action_name).is_none() {
                log::warn!("unknown keybind action in config: {action_name}");
            }
        }

        let mut keymap = Self::default();
        for spec in ACTION_SPECS {
            for sequence in resolved_sequences(config, spec) {
                keymap.insert(spec.action, sequence);
            }
        }
        keymap
    }

    pub fn advance(&self, state: &mut KeyChordState, stroke: KeyStroke) -> KeymapMatch {
        let mut next = state.pending.clone();
        next.push(stroke.clone());

        match self.evaluate(&next) {
            KeymapMatch::NoMatch if !state.pending.is_empty() => {
                state.clear();
                self.advance_fresh(state, stroke)
            }
            outcome => self.finish_advance(state, next, outcome),
        }
    }

    pub fn hints_for_prefix(&self, prefix: &[KeyStroke]) -> Vec<KeymapHint> {
        let Some(node) = self.node(prefix) else {
            return vec![];
        };

        node.children
            .iter()
            .map(|(stroke, child)| KeymapHint {
                stroke: stroke.clone(),
                action: child.action,
                has_children: !child.children.is_empty(),
            })
            .collect()
    }

    fn insert(&mut self, action: AppAction, sequence: Vec<KeyStroke>) {
        let mut node = &mut self.root;

        for (index, stroke) in sequence.iter().enumerate() {
            let child = node.children.entry(stroke.clone()).or_default();
            let is_last = index + 1 == sequence.len();

            if !is_last {
                if let Some(prefix_action) = child.action {
                    log::warn!(
                        "ignoring ambiguous keybind {} for {}: prefix already bound to {}",
                        sequence_text(&sequence),
                        action_spec(action).name,
                        action_spec(prefix_action).name
                    );
                    return;
                }
                node = child;
                continue;
            }

            if !child.children.is_empty() {
                log::warn!(
                    "dropping ambiguous descendant keybinds under {} for {}",
                    sequence_text(&sequence),
                    action_spec(action).name
                );
                child.children.clear();
            }
            if let Some(existing_action) = child.action {
                if existing_action != action {
                    log::warn!(
                        "ignoring duplicate keybind {} for {}: already bound to {}",
                        sequence_text(&sequence),
                        action_spec(action).name,
                        action_spec(existing_action).name
                    );
                    return;
                }
            }
            child.action = Some(action);
            return;
        }
    }

    fn advance_fresh(&self, state: &mut KeyChordState, stroke: KeyStroke) -> KeymapMatch {
        let next = vec![stroke];
        let outcome = self.evaluate(&next);
        self.finish_advance(state, next, outcome)
    }

    fn finish_advance(
        &self,
        state: &mut KeyChordState,
        next: Vec<KeyStroke>,
        outcome: KeymapMatch,
    ) -> KeymapMatch {
        match outcome {
            KeymapMatch::NoMatch => {
                state.clear();
                KeymapMatch::NoMatch
            }
            KeymapMatch::Pending => {
                state.pending = next;
                KeymapMatch::Pending
            }
            KeymapMatch::Triggered(action) => {
                state.clear();
                KeymapMatch::Triggered(action)
            }
        }
    }

    fn evaluate(&self, sequence: &[KeyStroke]) -> KeymapMatch {
        let Some(node) = self.node(sequence) else {
            return KeymapMatch::NoMatch;
        };

        match (node.action, node.children.is_empty()) {
            (Some(action), true) => KeymapMatch::Triggered(action),
            (Some(_), false) | (None, false) => KeymapMatch::Pending,
            (None, true) => KeymapMatch::NoMatch,
        }
    }

    fn node(&self, sequence: &[KeyStroke]) -> Option<&KeymapNode> {
        let mut node = &self.root;
        for stroke in sequence {
            node = node.children.get(stroke)?;
        }
        Some(node)
    }
}

#[cfg(test)]
mod tests {
    use super::{AppAction, AppConfig, KeyChordState, KeyStroke, Keymap, KeymapHint, KeymapMatch};
    use crate::config::ConfigKeybind;

    fn stroke(text: &str) -> KeyStroke {
        KeyStroke::parse(text).unwrap()
    }

    #[test]
    fn from_config_uses_custom_multi_stroke_binding_and_hints_expose_children() {
        let mut config = AppConfig::default();
        config.keybinds.insert(
            "zoom_in".to_string(),
            ConfigKeybind::Single("ctrl+k ctrl+plus".to_string()),
        );

        let keymap = Keymap::from_config(&config);
        let root_hints = keymap.hints_for_prefix(&[]);

        assert!(root_hints.contains(&KeymapHint {
            stroke: stroke("ctrl+k"),
            action: None,
            has_children: true,
        }));
        assert_eq!(
            keymap.hints_for_prefix(&[stroke("ctrl+k")]),
            vec![KeymapHint {
                stroke: stroke("ctrl+plus"),
                action: Some(AppAction::ZoomIn),
                has_children: false,
            }]
        );
    }

    #[test]
    fn advance_falls_back_to_fresh_match_after_bad_pending_prefix() {
        let mut config = AppConfig::default();
        config.keybinds.insert(
            "zoom_in".to_string(),
            ConfigKeybind::Single("ctrl+k ctrl+plus".to_string()),
        );
        config.keybinds.insert(
            "zoom_out".to_string(),
            ConfigKeybind::Single("ctrl+minus".to_string()),
        );

        let keymap = Keymap::from_config(&config);
        let mut state = KeyChordState::default();

        assert_eq!(
            keymap.advance(&mut state, stroke("ctrl+k")),
            KeymapMatch::Pending
        );
        assert_eq!(state.pending(), &[stroke("ctrl+k")]);

        assert_eq!(
            keymap.advance(&mut state, stroke("ctrl+minus")),
            KeymapMatch::Triggered(AppAction::ZoomOut)
        );
        assert!(state.pending().is_empty());
    }

    #[test]
    fn triggered_sequences_clear_pending_state() {
        let mut config = AppConfig::default();
        config.keybinds.insert(
            "zoom_in".to_string(),
            ConfigKeybind::Single("ctrl+k ctrl+plus".to_string()),
        );

        let keymap = Keymap::from_config(&config);
        let mut state = KeyChordState::default();

        assert_eq!(
            keymap.advance(&mut state, stroke("ctrl+k")),
            KeymapMatch::Pending
        );
        assert_eq!(
            keymap.advance(&mut state, stroke("ctrl+plus")),
            KeymapMatch::Triggered(AppAction::ZoomIn)
        );
        assert!(state.pending().is_empty());
    }

    #[test]
    fn invalid_override_falls_back_to_default_bindings() {
        let mut config = AppConfig::default();
        config.keybinds.insert(
            "zoom_out".to_string(),
            ConfigKeybind::Single("definitely-not-a-key".to_string()),
        );

        let keymap = Keymap::from_config(&config);
        let mut state = KeyChordState::default();

        assert_eq!(
            keymap.advance(&mut state, stroke("ctrl+minus")),
            KeymapMatch::Triggered(AppAction::ZoomOut)
        );
    }

    #[test]
    fn partial_override_replaces_defaults_with_only_valid_configured_sequences() {
        let mut config = AppConfig::default();
        config.keybinds.insert(
            "paste_clipboard".to_string(),
            ConfigKeybind::Multiple(vec![
                "bad-binding".to_string(),
                " shift+insert ".to_string(),
            ]),
        );

        let keymap = Keymap::from_config(&config);
        let mut state = KeyChordState::default();

        assert_eq!(
            keymap.advance(&mut state, stroke("shift+insert")),
            KeymapMatch::Triggered(AppAction::PasteClipboard)
        );
        assert_eq!(
            keymap.advance(&mut state, stroke("ctrl+v")),
            KeymapMatch::NoMatch
        );
    }

    #[test]
    fn shorter_binding_prunes_existing_longer_descendants() {
        let mut keymap = Keymap::default();
        keymap.insert(
            AppAction::OpenProjectPicker,
            vec![stroke("ctrl+p"), stroke("o")],
        );
        keymap.insert(AppAction::NewSession, vec![stroke("ctrl+p")]);

        let mut state = KeyChordState::default();
        assert_eq!(
            keymap.advance(&mut state, stroke("ctrl+p")),
            KeymapMatch::Triggered(AppAction::NewSession)
        );
        assert!(keymap.hints_for_prefix(&[stroke("ctrl+p")]).is_empty());
        assert_eq!(
            keymap.advance(&mut state, stroke("o")),
            KeymapMatch::NoMatch
        );
    }

    #[test]
    fn longer_binding_is_ignored_when_prefix_already_triggers_action() {
        let mut keymap = Keymap::default();
        keymap.insert(AppAction::NewSession, vec![stroke("ctrl+p")]);
        keymap.insert(
            AppAction::OpenProjectPicker,
            vec![stroke("ctrl+p"), stroke("o")],
        );

        let mut state = KeyChordState::default();
        assert_eq!(
            keymap.advance(&mut state, stroke("ctrl+p")),
            KeymapMatch::Triggered(AppAction::NewSession)
        );
        assert!(keymap.hints_for_prefix(&[stroke("ctrl+p")]).is_empty());
    }

    #[test]
    fn duplicate_exact_binding_keeps_first_action() {
        let mut keymap = Keymap::default();
        keymap.insert(AppAction::OpenProjectPicker, vec![stroke("ctrl+p")]);
        keymap.insert(AppAction::NewSession, vec![stroke("ctrl+p")]);

        let mut state = KeyChordState::default();
        assert_eq!(
            keymap.advance(&mut state, stroke("ctrl+p")),
            KeymapMatch::Triggered(AppAction::OpenProjectPicker)
        );
    }
}
