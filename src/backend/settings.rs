use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Context;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fal_key: Option<String>,
}

impl UserSettings {
    pub fn load_default() -> anyhow::Result<Self> {
        Self::load(settings_path()?)
    }

    pub fn save_default(&self) -> anyhow::Result<PathBuf> {
        let path = settings_path()?;
        self.save(&path)?;
        Ok(path)
    }

    pub fn load(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(Self::default());
        }

        let json = fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        serde_json::from_str(&json).with_context(|| format!("failed to parse {}", path.display()))
    }

    pub fn save(&self, path: impl AsRef<Path>) -> anyhow::Result<()> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        let json = serde_json::to_string_pretty(self)?;
        fs::write(path, json).with_context(|| format!("failed to write {}", path.display()))
    }

    pub fn fal_key_or_env(&self) -> Option<String> {
        self.fal_key
            .as_ref()
            .filter(|key| !key.trim().is_empty())
            .cloned()
            .or_else(|| std::env::var("FAL_KEY").ok())
    }
}

pub fn settings_path() -> anyhow::Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(".cashewai").join("settings.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_json() {
        let path =
            std::env::temp_dir().join(format!("cashew-settings-{}.json", std::process::id()));
        let settings = UserSettings {
            fal_key: Some("test-key".to_string()),
        };

        settings.save(&path).unwrap();
        let loaded = UserSettings::load(&path).unwrap();
        let _ = fs::remove_file(path);

        assert_eq!(loaded, settings);
    }

    #[test]
    fn missing_settings_file_loads_defaults() {
        let path = std::env::temp_dir().join(format!(
            "cashew-missing-settings-{}.json",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);

        assert_eq!(UserSettings::load(path).unwrap(), UserSettings::default());
    }
}
