use std::collections::BTreeMap;

use super::{
    actions::action_spec_by_name, resolved_sequences, AppAction, AppConfig, KeyStroke, ACTION_SPECS,
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
        for stroke in sequence {
            node = node.children.entry(stroke).or_default();
        }
        node.action = Some(action);
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
