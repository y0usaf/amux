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

pub const LAYOUT_TERMINAL_WIDTH_PERCENT_DEFAULT: u8 = 100;
pub const LAYOUT_SIDEBAR_WIDTH_DEFAULT: u16 = 36;
pub const LAYOUT_SIDEBAR_WIDTH_PERCENT_DEFAULT: u8 = 22;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutSidebarWidth {
    Columns(u16),
    Percent(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayoutWidths {
    pub sidebar: LayoutSidebarWidth,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AppConfig {
    pub terminal_width_percent: Option<u8>,
    pub sidebar_width: Option<u16>,
    pub sidebar_width_percent: Option<u8>,
    pub tui_terminal_width_percent: Option<u8>,
    pub tui_sidebar_width_percent: Option<u8>,
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

    pub fn action_binding_texts(&self, action: AppAction) -> Vec<String> {
        resolved_sequences(self, action_spec(action))
            .into_iter()
            .map(|sequence| {
                sequence
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect()
    }

    pub fn layout_widths(&self) -> LayoutWidths {
        LayoutWidths {
            sidebar: self.layout_sidebar_width(),
        }
    }

    fn layout_sidebar_width(&self) -> LayoutSidebarWidth {
        if let Some(width) = self.sidebar_width {
            if validate_layout_sidebar_width(width) {
                return LayoutSidebarWidth::Columns(width);
            }
            log::warn!(
                "invalid sidebar_width={} (need 8..=120); using default",
                width,
            );
            return LayoutSidebarWidth::Columns(LAYOUT_SIDEBAR_WIDTH_DEFAULT);
        }

        if let Some(percent) = self
            .sidebar_width_percent
            .or(self.tui_sidebar_width_percent)
        {
            if validate_layout_sidebar_width_percent(percent) {
                return LayoutSidebarWidth::Percent(percent);
            }
            log::warn!(
                "invalid sidebar_width_percent={} (need 1..=50); using default",
                percent,
            );
        }

        LayoutSidebarWidth::Columns(LAYOUT_SIDEBAR_WIDTH_DEFAULT)
    }

    fn normalize(&mut self) {
        for binding in self.keybinds.values_mut() {
            binding.normalize();
        }
    }
}

pub fn validate_layout_sidebar_width(width: u16) -> bool {
    (8..=120).contains(&width)
}

pub fn validate_layout_sidebar_width_percent(percent: u8) -> bool {
    (1..=50).contains(&percent)
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
