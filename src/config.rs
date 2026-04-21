use std::fs;
use std::path::PathBuf;

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::util::app_config_dir;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AppConfig {
    pub ui_scale: Option<f32>,
    pub font_family: Option<String>,
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

    fn normalize(&mut self) {
        self.font_family = self
            .font_family
            .as_deref()
            .map(str::trim)
            .filter(|family| !family.is_empty())
            .map(str::to_owned);
    }
}

pub fn default_config_path() -> PathBuf {
    app_config_dir().join("config.json")
}
