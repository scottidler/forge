use eyre::{Context, Result, eyre};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ForgeConfigWrapper {
    pub forge: ForgeConfig,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ForgeConfig {
    pub version: String,
    pub home: String,
    pub store: String,
    #[serde(default)]
    pub pipelines: HashMap<String, String>,
    #[serde(default)]
    pub fabric: FabricConfig,
    #[serde(default)]
    pub global_references: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct FabricConfig {
    #[serde(default = "default_fabric_binary")]
    pub binary: String,
    #[serde(default)]
    pub model: String,
}

fn default_fabric_binary() -> String {
    "fabric".to_string()
}

impl ForgeConfig {
    /// Resolve the forge home directory (expands ~/ and resolves to absolute path)
    pub fn home_dir(&self) -> Result<PathBuf> {
        let expanded = shellexpand::tilde(&self.home);
        let path = PathBuf::from(expanded.as_ref());
        Ok(path)
    }

    /// Resolve the artifact store directory
    pub fn store_dir(&self) -> Result<PathBuf> {
        let expanded = shellexpand::tilde(&self.store);
        let path = PathBuf::from(expanded.as_ref());
        Ok(path)
    }

    /// Resolve a pipeline definition path relative to forge home
    pub fn pipeline_path(&self, name: &str) -> Result<PathBuf> {
        let rel = self
            .pipelines
            .get(name)
            .ok_or_else(|| eyre!("unknown pipeline: {}", name))?;
        let home = self.home_dir()?;
        Ok(home.join(rel))
    }

    /// Resolve a reference path relative to forge home
    pub fn reference_path(&self, rel: &str) -> Result<PathBuf> {
        let home = self.home_dir()?;
        Ok(home.join(rel))
    }

    /// Load configuration with fallback chain
    pub fn load(config_path: Option<&PathBuf>) -> Result<Self> {
        if let Some(path) = config_path {
            return Self::load_from_file(path).context(format!("failed to load config from {}", path.display()));
        }

        // Try FORGE_HOME env var
        if let Ok(forge_home) = std::env::var("FORGE_HOME") {
            let path = PathBuf::from(&forge_home).join("forge.yml");
            if path.exists() {
                match Self::load_from_file(&path) {
                    Ok(config) => return Ok(config),
                    Err(e) => {
                        log::warn!("failed to load config from {}: {}", path.display(), e);
                    }
                }
            }
        }

        // Try ~/.config/forge/forge.yml
        if let Some(config_dir) = dirs::config_dir() {
            let path = config_dir.join("forge").join("forge.yml");
            if path.exists() {
                match Self::load_from_file(&path) {
                    Ok(config) => return Ok(config),
                    Err(e) => {
                        log::warn!("failed to load config from {}: {}", path.display(), e);
                    }
                }
            }
        }

        // Try ./forge.yml
        let fallback = PathBuf::from("forge.yml");
        if fallback.exists() {
            return Self::load_from_file(&fallback).context("failed to load forge.yml from cwd");
        }

        Err(eyre!(
            "no forge.yml found; set FORGE_HOME or create ~/.config/forge/forge.yml"
        ))
    }

    fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = fs::read_to_string(&path).context("failed to read config file")?;
        let wrapper: ForgeConfigWrapper = serde_yaml::from_str(&content).context("failed to parse config file")?;
        log::info!("loaded config from: {}", path.as_ref().display());
        Ok(wrapper.forge)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn sample_config_yaml() -> &'static str {
        r#"forge:
  version: "1"
  home: /tmp/forge-test
  store: /tmp/forge-store
  pipelines:
    techspec: pipelines/techspec.yml
  fabric:
    binary: fabric
    model: ""
  global_references:
    - references/voice.md
"#
    }

    #[test]
    fn test_load_from_file() {
        let mut tmp = NamedTempFile::new().expect("failed to create temp file");
        write!(tmp, "{}", sample_config_yaml()).expect("failed to write");
        let config = ForgeConfig::load_from_file(tmp.path()).expect("failed to load");
        assert_eq!(config.version, "1");
        assert_eq!(config.home, "/tmp/forge-test");
        assert_eq!(config.pipelines.len(), 1);
        assert_eq!(config.global_references.len(), 1);
    }

    #[test]
    fn test_home_dir() {
        let mut tmp = NamedTempFile::new().expect("failed to create temp file");
        write!(tmp, "{}", sample_config_yaml()).expect("failed to write");
        let config = ForgeConfig::load_from_file(tmp.path()).expect("failed to load");
        let home = config.home_dir().expect("failed to resolve home");
        assert_eq!(home, PathBuf::from("/tmp/forge-test"));
    }

    #[test]
    fn test_pipeline_path() {
        let mut tmp = NamedTempFile::new().expect("failed to create temp file");
        write!(tmp, "{}", sample_config_yaml()).expect("failed to write");
        let config = ForgeConfig::load_from_file(tmp.path()).expect("failed to load");
        let path = config.pipeline_path("techspec").expect("failed to resolve");
        assert_eq!(path, PathBuf::from("/tmp/forge-test/pipelines/techspec.yml"));
    }

    #[test]
    fn test_pipeline_path_unknown() {
        let mut tmp = NamedTempFile::new().expect("failed to create temp file");
        write!(tmp, "{}", sample_config_yaml()).expect("failed to write");
        let config = ForgeConfig::load_from_file(tmp.path()).expect("failed to load");
        assert!(config.pipeline_path("nonexistent").is_err());
    }

    #[test]
    fn test_reference_path() {
        let mut tmp = NamedTempFile::new().expect("failed to create temp file");
        write!(tmp, "{}", sample_config_yaml()).expect("failed to write");
        let config = ForgeConfig::load_from_file(tmp.path()).expect("failed to load");
        let path = config.reference_path("references/voice.md").expect("failed to resolve");
        assert_eq!(path, PathBuf::from("/tmp/forge-test/references/voice.md"));
    }
}
