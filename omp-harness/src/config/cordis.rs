//! Config-as-WASM: omp-harness's default config is a compiled `.wasm` module
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
const DEFAULT_CONFIG_WASM: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/config.wasm"));

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

#[cfg(test)]
mod tests {
    use super::DEFAULT_CONFIG_WASM;
    use super::ConfigKernel;

    /// Mirror of the cordis-rs `config_wasm` reference test: the embedded
    /// default config mounts at startup, serves its keys as strings, and —
    /// because the kernel owns inverses — unmounting reverts every key.
    #[test]
    fn config_kernel_loads_at_startup_and_reverts_on_unmount() {
        let mut kernel = ConfigKernel::mount().expect("mount default config");
        // Keys served by the WASM guest (core set 1 writes).
        assert_eq!(kernel.get("panel_width_percent").as_deref(), Some("22"));
        assert_eq!(kernel.get("keybinds").as_deref(), Some("{}"));

        let config = kernel.to_app_config().expect("app config from wasm");
        assert_eq!(config.panel_width_percent(), 22);

        // Unmount -> kernel replays inverses; no residue.
        kernel.unmount().expect("unmount config");
        assert!(!kernel.has("panel_width_percent"));
        assert!(!kernel.has("keybinds"));
    }

    /// A key present before the mount is restored, not clobbered — identical to
    /// the cordis-rs reference (`config_wasm_reverts_pre_existing_keys`).
    #[test]
    fn config_reverts_pre_existing_keys() {
        let mut ctx = cordis::Context::new();
        ctx.set("panel_width_percent", "33").expect("seed");
        let id = ctx.mount(DEFAULT_CONFIG_WASM).expect("mount config at startup");
        assert_eq!(
            ctx.get("panel_width_percent").as_deref(),
            Some("22"),
            "config overrides the (user) seed"
        );

        ctx.unmount(id).expect("unmount config");
        assert_eq!(
            ctx.get("panel_width_percent").as_deref(),
            Some("33"),
            "pre-existing key is restored on config unmount"
        );
    }
}
