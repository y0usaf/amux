//! Config-as-WASM: pi-harness's default config is a compiled `.wasm` module
//! (`config.wat`, compiled by `build.rs` into `OUT_DIR/config.wasm` and embedded
//! here) loaded at startup through the cordis-rs core kernel — function set 1
//! only (`ctx_set`/`ctx_remove`/`ctx_read`). There is no textual parser for the
//! default: the default IS data served by a WASM guest over the public ABI, so
//! builtins share the same boundary as any user config.wasm
//! ([[principle:no-privileged-path]]). Unmounting reverts the kernel's inverse
//! replay, leaving no residue ([[principle:spatiotemporal]]).
//!
//! A user `config.json` may still be provided to override these defaults: that
//! file is the imperative-shell read at runtime, layered on top of the
//! WASM-served default. Its removal is out of scope for this wave and would
//! drop a user-facing override mechanism ([[principle:least-code]]).

use anyhow::Result;

use super::AppConfig;

/// The compiled default config bytes, embedded at build time. This is exactly
/// what a user-provided `config.wasm` would be — same format, same ABI.
const DEFAULT_CONFIG_WASM: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/config.wasm"));

/// A config-as-WASM guest mounted at startup. It owns the cordis kernel context
/// so that unmounting (explicitly, or on drop) reverts every config key with no
/// residue.
pub struct ConfigKernel {
    ctx: cordis::Context,
    id: usize,
}

impl ConfigKernel {
    /// Mount the embedded default config on a fresh kernel. Call once at
    /// startup, before any consumer reads config.
    pub fn mount() -> Result<Self> {
        let mut ctx = cordis::Context::new();
        let id = ctx.mount(DEFAULT_CONFIG_WASM)?;
        Ok(Self { ctx, id })
    }

    pub fn get(&self, key: &str) -> Option<String> {
        self.ctx.get(key)
    }

    pub fn has(&self, key: &str) -> bool {
        self.ctx.has(key)
    }

    /// Revert the config (unmount the kernel's inverse replay). Drop does this
    /// too if you don't call it; calling it keeps the error visible.
    pub fn unmount(&mut self) -> Result<()> {
        Ok(self.ctx.unmount(self.id)?)
    }

    /// Default `AppConfig` derived from the WASM-served string keys. This is
    /// the default config: data served over the public ABI, not a text parser.
    pub fn to_app_config(&self) -> Result<AppConfig> {
        let mut config = AppConfig::default();
        if let Some(percent) = self.get("panel_width_percent") {
            config.panel_width_percent = Some(percent.parse()?);
        }
        if let Some(keybinds) = self.get("keybinds") {
            config.keybinds = serde_json::from_str(&keybinds)?;
        }
        Ok(config)
    }
}

impl Drop for ConfigKernel {
    fn drop(&mut self) {
        let _ = self.ctx.unmount(self.id);
    }
}
