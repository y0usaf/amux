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
pub use keymap::{KeyChordState, Keymap, KeymapHint, KeymapMatch};
pub use keys::{KeyModifiers, KeyStroke, KeyToken, NamedKeyToken};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AppConfig {
    pub ui_scale: Option<f32>,
    pub font_family: Option<String>,
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

    pub fn font_family(&self) -> Option<&str> {
        self.font_family.as_deref()
    }

    pub fn keymap(&self) -> Keymap {
        Keymap::from_config(self)
    }

    fn normalize(&mut self) {
        self.font_family = self
            .font_family
            .as_deref()
            .map(str::trim)
            .filter(|family| !family.is_empty())
            .map(str::to_owned);
        for binding in self.keybinds.values_mut() {
            binding.normalize();
        }
    }
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

    let sequence: Result<Vec<_>, _> = text.split_whitespace().map(KeyStroke::parse).collect();
    let sequence = sequence?;
    if sequence.is_empty() {
        return Err("empty key sequence".to_string());
    }
    Ok(sequence)
}

pub fn default_config_path() -> PathBuf {
    app_config_dir().join("config.json")
}
