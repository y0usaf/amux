mod actions;
pub mod cordis;
mod keymap;
mod keys;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::app::glyphs::GlyphStyle;
use crate::util::app_config_dir;

pub use actions::{action_spec, ActionSpec, AppAction, ACTION_SPECS};
pub use keymap::{KeyChordState, Keymap, KeymapMatch};
pub use keys::{KeyModifiers, KeyStroke, KeyToken, NamedKeyToken};

pub const LAYOUT_TERMINAL_WIDTH_PERCENT_DEFAULT: u8 = 100;
/// Both bars take the same share of the terminal, so the left sidebar and the
/// in-Pi right rail read as a matched pair at any terminal size.
pub const PANEL_WIDTH_PERCENT_DEFAULT: u8 = 22;
pub const PANEL_WIDTH_MIN_COLS: u16 = 24;
pub const PANEL_WIDTH_MAX_COLS: u16 = 80;

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
    pub panel_width_percent: Option<u8>,
    pub right_rail_width: Option<u16>,
    pub tui_mode: Option<String>,
    pub tui_terminal_width_percent: Option<u8>,
    pub tui_sidebar_width_percent: Option<u8>,
    pub ascii: Option<bool>,
    /// Per-symbol overrides for the in-Pi rail glyph table, e.g.
    /// `symbols: { overrides: { "rail.ok": "OK" } }`. Showed as env on the
    /// pi process; unicode/ascii preset base still comes from `ascii`.
    pub symbols: Option<SymbolsConfig>,
    #[serde(default)]
    pub keybinds: BTreeMap<String, ConfigKeybind>,
}

/// Rail symbol overrides addressed by canonical "rail.*" keys. Values layer
/// on top of the unicode/ascii preset selected by the `ascii` flag.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolsConfig {
    #[serde(default)]
    pub overrides: BTreeMap<String, String>,
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
        if path.exists() {
            let content =
                fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
            let mut config: Self = serde_json::from_str(&content)
                .with_context(|| format!("parsing {}", path.display()))?;
            config.normalize();
            return Ok(config);
        }
        // No user `config.json: the default config comes from the compiled
        // `config.wasm` mounted on the cordis-rs core kernel at startup
        // ([[principle:no-privileged-path]]). Unmounting reverts it.
        let kernel = cordis::ConfigKernel::mount()?;
        kernel.to_app_config()
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

    /// Shared width share for both bars. Per-bar keys override it.
    pub fn panel_width_percent(&self) -> u8 {
        match self.panel_width_percent {
            Some(percent) if validate_panel_width_percent(percent) => percent,
            Some(percent) => {
                log::warn!("invalid panel_width_percent={percent} (need 5..=40); using default");
                PANEL_WIDTH_PERCENT_DEFAULT
            }
            None => PANEL_WIDTH_PERCENT_DEFAULT,
        }
    }

    /// Glyph rendering style: Unicode by default, plain ASCII when the config
    /// flag `ascii` is explicitly enabled.
    pub fn glyph_style(&self) -> GlyphStyle {
        if self.ascii == Some(true) {
            GlyphStyle::Ascii
        } else {
            GlyphStyle::Unicode
        }
    }

    /// Rail symbol overrides configured under `symbols.overrides`, if any.
    pub fn symbols_overrides(&self) -> Option<&BTreeMap<String, String>> {
        self.symbols.as_ref().map(|symbols| &symbols.overrides)
    }

    /// Rail width in PTY cells sent to the sidechannel extension, derived from
    /// the harness terminal width so both bars land on the same column count.
    /// `0` disables the rail; invalid values fall back to the shared share.
    pub fn right_rail_columns(&self, total_cols: u16) -> u16 {
        match self.right_rail_width {
            Some(width) if validate_right_rail_width(width) => width,
            Some(width) => {
                log::warn!(
                    "invalid right_rail_width={width} (need 0 or 24..=80); using the shared panel width"
                );
                panel_columns(total_cols, self.panel_width_percent())
            }
            None => panel_columns(total_cols, self.panel_width_percent()),
        }
    }

    /// Pi TUI mode forwarded as `--tui-mode <mode>` on launch. `None`/`regular`
    /// are the default (flag omitted); `fullscreen` is pi's experimental mode.
    pub fn pi_tui_mode(&self) -> Option<&str> {
        match self.tui_mode.as_deref() {
            Some("fullscreen") => Some("fullscreen"),
            Some("regular") | None => None,
            Some(other) => {
                log::warn!(
                    "invalid tui_mode={other:?} (need \"regular\" or \"fullscreen\"); ignoring"
                );
                None
            }
        }
    }

    fn layout_sidebar_width(&self) -> LayoutSidebarWidth {
        if let Some(width) = self.sidebar_width {
            if validate_layout_sidebar_width(width) {
                return LayoutSidebarWidth::Columns(width);
            }
            log::warn!(
                "invalid sidebar_width={width} (need 8..=120); using the shared panel width"
            );
        } else if let Some(percent) = self
            .sidebar_width_percent
            .or(self.tui_sidebar_width_percent)
        {
            if validate_panel_width_percent(percent) {
                return LayoutSidebarWidth::Percent(percent);
            }
            log::warn!(
                "invalid sidebar_width_percent={percent} (need 5..=40); using the shared panel width"
            );
        }

        LayoutSidebarWidth::Percent(self.panel_width_percent())
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

pub fn validate_right_rail_width(width: u16) -> bool {
    width == 0 || (24..=80).contains(&width)
}

/// Shared by `panel_width_percent` and the legacy per-bar percent keys.
pub fn validate_panel_width_percent(percent: u8) -> bool {
    (5..=40).contains(&percent)
}

/// Percent to cells for either bar. One function, so the sidebar and the rail
/// can never round or clamp differently.
pub fn panel_columns(total_cols: u16, percent: u8) -> u16 {
    let cells = (u32::from(total_cols) * u32::from(percent) + 50) / 100;
    (cells as u16).clamp(PANEL_WIDTH_MIN_COLS, PANEL_WIDTH_MAX_COLS)
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
