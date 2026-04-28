mod actions;
mod keymap;
mod keys;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::util::app_config_dir;

pub use actions::{action_spec, ActionSpec, AppAction, ACTION_SPECS};
pub use keymap::{KeyChordState, Keymap, KeymapMatch};
pub use keys::{KeyModifiers, KeyStroke, KeyToken, NamedKeyToken};

pub const LAYOUT_TERMINAL_WIDTH_PERCENT_DEFAULT: u8 = 50;
pub const LAYOUT_SIDEBAR_WIDTH_PERCENT_DEFAULT: u8 = 13;
pub const LAYOUT_BODY_HEIGHT_PERCENT_DEFAULT: u8 = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayoutWidthPercents {
    pub terminal: u8,
    pub sidebar: u8,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AppConfig {
    pub terminal_width_percent: Option<u8>,
    pub sidebar_width_percent: Option<u8>,
    pub body_height_percent: Option<u8>,
    pub tui_terminal_width_percent: Option<u8>,
    pub tui_sidebar_width_percent: Option<u8>,
    pub tui_body_height_percent: Option<u8>,
    #[serde(default)]
    pub keybinds: BTreeMap<String, ConfigKeybind>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ConfigKeybind {
    Single(String),
    Multiple(Vec<String>),
}

impl ConfigKeybind {
    fn normalize(&mut self) {
        match self {
            Self::Single(text) => *text = text.trim().to_string(),
            Self::Multiple(bindings) => {
                for binding in bindings.iter_mut() {
                    *binding = binding.trim().to_string();
                }
                bindings.retain(|binding| !binding.is_empty());
            }
        }
    }

    fn binding_strings(&self) -> Vec<String> {
        match self {
            Self::Single(text) => {
                let text = text.trim();
                if text.is_empty() {
                    vec![]
                } else {
                    vec![text.to_string()]
                }
            }
            Self::Multiple(bindings) => bindings
                .iter()
                .map(|binding| binding.trim())
                .filter(|binding| !binding.is_empty())
                .map(str::to_string)
                .collect(),
        }
    }
}

impl AppConfig {
    pub fn load_default() -> anyhow::Result<Self> {
        let path = default_config_path();
        if !path.exists() {
            return Ok(Self::default());
        }
        let content =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        let mut config: Self = serde_json::from_str(&content)
            .with_context(|| format!("parsing {}", path.display()))?;
        config.normalize();
        Ok(config)
    }

    pub fn save_default(&self) -> anyhow::Result<()> {
        let path = default_config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        let mut copy = self.clone();
        copy.normalize();
        let content = serde_json::to_string_pretty(&copy)?;
        fs::write(&path, content).with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }

    pub fn keymap(&self) -> Keymap {
        Keymap::from_config(self)
    }

    pub fn layout_width_percents(&self) -> LayoutWidthPercents {
        let widths = LayoutWidthPercents {
            terminal: self
                .terminal_width_percent
                .or(self.tui_terminal_width_percent)
                .unwrap_or(LAYOUT_TERMINAL_WIDTH_PERCENT_DEFAULT),
            sidebar: self
                .sidebar_width_percent
                .or(self.tui_sidebar_width_percent)
                .unwrap_or(LAYOUT_SIDEBAR_WIDTH_PERCENT_DEFAULT),
        };
        if validate_layout_width_percents(widths) {
            return widths;
        }

        log::warn!(
            "invalid layout width percents: terminal_width_percent={} sidebar_width_percent={} (need terminal > sidebar and terminal + sidebar*2 < 100); using defaults",
            widths.terminal,
            widths.sidebar,
        );
        LayoutWidthPercents {
            terminal: LAYOUT_TERMINAL_WIDTH_PERCENT_DEFAULT,
            sidebar: LAYOUT_SIDEBAR_WIDTH_PERCENT_DEFAULT,
        }
    }

    pub fn layout_body_height_percent(&self) -> u8 {
        let percent = self
            .body_height_percent
            .or(self.tui_body_height_percent)
            .unwrap_or(LAYOUT_BODY_HEIGHT_PERCENT_DEFAULT);
        if validate_layout_body_height_percent(percent) {
            return percent;
        }

        log::warn!(
            "invalid layout body height percent: body_height_percent={} (need 1..=100); using default",
            percent,
        );
        LAYOUT_BODY_HEIGHT_PERCENT_DEFAULT
    }

    fn normalize(&mut self) {
        for binding in self.keybinds.values_mut() {
            binding.normalize();
        }
    }
}

pub fn validate_layout_width_percents(widths: LayoutWidthPercents) -> bool {
    widths.sidebar > 0
        && widths.terminal > widths.sidebar
        && u16::from(widths.terminal) + u16::from(widths.sidebar) * 2 < 100
}

pub fn validate_layout_body_height_percent(percent: u8) -> bool {
    (1..=100).contains(&percent)
}

fn resolved_sequences(config: &AppConfig, spec: &ActionSpec) -> Vec<Vec<KeyStroke>> {
    if let Some(binding) = config.keybinds.get(spec.name) {
        let overrides = binding.binding_strings();
        if overrides.is_empty() {
            return vec![];
        }

        let sequences = parse_binding_list(spec.name, &overrides);
        if !sequences.is_empty() {
            return sequences;
        }

        log::warn!(
            "all configured keybinds invalid for {}; using defaults",
            spec.name
        );
    }

    parse_binding_list(spec.name, spec.defaults)
}

fn parse_binding_list<S>(action_name: &str, bindings: &[S]) -> Vec<Vec<KeyStroke>>
where
    S: AsRef<str>,
{
    bindings
        .iter()
        .filter_map(|binding| match parse_key_sequence(binding.as_ref()) {
            Ok(sequence) => Some(sequence),
            Err(error) => {
                log::warn!(
                    "invalid keybind for {}: {} ({error})",
                    action_name,
                    binding.as_ref()
                );
                None
            }
        })
        .collect()
}

fn parse_key_sequence(text: &str) -> Result<Vec<KeyStroke>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("empty key sequence".to_string());
    }

    text.split_whitespace().map(KeyStroke::parse).collect()
}

pub fn default_config_path() -> PathBuf {
    app_config_dir().join("config.json")
}
