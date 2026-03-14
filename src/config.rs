use eyre::{Context, Result, eyre};
use log::debug;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
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
    pub pipelines: Vec<String>,
    #[serde(default)]
    pub global_references: Vec<String>,
    #[serde(default)]
    pub log_level: Option<String>,
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

    /// Resolve a pipeline directory entry to an absolute path
    fn resolve_pipeline_dir(&self, dir: &str) -> Result<PathBuf> {
        let expanded = shellexpand::tilde(dir);
        let path = Path::new(expanded.as_ref());
        if path.is_absolute() {
            Ok(path.to_path_buf())
        } else {
            Ok(self.home_dir()?.join(path))
        }
    }

    /// Resolve a pipeline definition path by scanning pipeline directories
    pub fn pipeline_path(&self, name: &str) -> Result<PathBuf> {
        debug!("pipeline_path: name={}", name);
        let filename = format!("{}.yml", name);
        for dir in &self.pipelines {
            let dir_path = self.resolve_pipeline_dir(dir)?;
            let candidate = dir_path.join(&filename);
            if candidate.exists() {
                return Ok(candidate);
            }
        }
        Err(eyre!("unknown pipeline: {} (searched: {:?})", name, self.pipelines))
    }

    /// List all discovered pipelines as (name, path) pairs
    pub fn list_pipelines(&self) -> Result<Vec<(String, PathBuf)>> {
        debug!("list_pipelines: dirs={:?}", self.pipelines);
        let mut seen = HashSet::new();
        let mut result = Vec::new();
        for dir in &self.pipelines {
            let dir_path = self.resolve_pipeline_dir(dir)?;
            if !dir_path.is_dir() {
                log::warn!("pipeline directory not found: {}", dir_path.display());
                continue;
            }
            for entry in fs::read_dir(&dir_path)? {
                let entry = entry?;
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "yml")
                    && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
                    && seen.insert(stem.to_string())
                {
                    result.push((stem.to_string(), path));
                }
            }
        }
        result.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(result)
    }

    /// Resolve a reference path relative to forge home
    pub fn reference_path(&self, rel: &str) -> Result<PathBuf> {
        let home = self.home_dir()?;
        Ok(home.join(rel))
    }

    /// Load configuration with fallback chain
    pub fn load(config_path: Option<&PathBuf>) -> Result<Self> {
        debug!("load: config_path={:?}", config_path);
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
        debug!("load_from_file: path={}", path.as_ref().display());
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
    use tempfile::{NamedTempFile, TempDir};

    fn sample_config_yaml() -> &'static str {
        r#"forge:
  version: "1"
  home: /tmp/forge-test
  store: /tmp/forge-store
  pipelines:
    - pipelines/
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
        assert_eq!(config.pipelines[0], "pipelines/");
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
        let dir = TempDir::new().expect("failed to create temp dir");
        let pipelines_dir = dir.path().join("pipelines");
        fs::create_dir_all(&pipelines_dir).expect("failed to create dir");
        fs::write(pipelines_dir.join("techspec.yml"), "dummy").expect("failed to write");

        let config = ForgeConfig {
            version: "1".to_string(),
            home: dir.path().to_string_lossy().to_string(),
            store: "/tmp/store".to_string(),
            pipelines: vec!["pipelines/".to_string()],
            global_references: vec![],
            log_level: None,
        };
        let path = config.pipeline_path("techspec").expect("failed to resolve");
        assert_eq!(path, pipelines_dir.join("techspec.yml"));
    }

    #[test]
    fn test_pipeline_path_unknown() {
        let dir = TempDir::new().expect("failed to create temp dir");
        let pipelines_dir = dir.path().join("pipelines");
        fs::create_dir_all(&pipelines_dir).expect("failed to create dir");

        let config = ForgeConfig {
            version: "1".to_string(),
            home: dir.path().to_string_lossy().to_string(),
            store: "/tmp/store".to_string(),
            pipelines: vec!["pipelines/".to_string()],
            global_references: vec![],
            log_level: None,
        };
        assert!(config.pipeline_path("nonexistent").is_err());
    }

    #[test]
    fn test_pipeline_path_multiple_dirs() {
        let dir = TempDir::new().expect("failed to create temp dir");
        let dir1 = dir.path().join("local");
        let dir2 = dir.path().join("shared");
        fs::create_dir_all(&dir1).expect("failed to create dir1");
        fs::create_dir_all(&dir2).expect("failed to create dir2");
        // techspec in dir1, research in dir2
        fs::write(dir1.join("techspec.yml"), "dummy").expect("failed to write");
        fs::write(dir2.join("research.yml"), "dummy").expect("failed to write");

        let config = ForgeConfig {
            version: "1".to_string(),
            home: dir.path().to_string_lossy().to_string(),
            store: "/tmp/store".to_string(),
            pipelines: vec!["local/".to_string(), "shared/".to_string()],
            global_references: vec![],
            log_level: None,
        };
        assert!(config.pipeline_path("techspec").is_ok());
        assert!(config.pipeline_path("research").is_ok());
    }

    #[test]
    fn test_pipeline_path_shadowing() {
        let dir = TempDir::new().expect("failed to create temp dir");
        let dir1 = dir.path().join("local");
        let dir2 = dir.path().join("shared");
        fs::create_dir_all(&dir1).expect("failed to create dir1");
        fs::create_dir_all(&dir2).expect("failed to create dir2");
        // Same name in both -- first directory wins
        fs::write(dir1.join("techspec.yml"), "local").expect("failed to write");
        fs::write(dir2.join("techspec.yml"), "shared").expect("failed to write");

        let config = ForgeConfig {
            version: "1".to_string(),
            home: dir.path().to_string_lossy().to_string(),
            store: "/tmp/store".to_string(),
            pipelines: vec!["local/".to_string(), "shared/".to_string()],
            global_references: vec![],
            log_level: None,
        };
        let path = config.pipeline_path("techspec").expect("failed to resolve");
        assert_eq!(path, dir1.join("techspec.yml"));
    }

    #[test]
    fn test_list_pipelines() {
        let dir = TempDir::new().expect("failed to create temp dir");
        let pipelines_dir = dir.path().join("pipelines");
        fs::create_dir_all(&pipelines_dir).expect("failed to create dir");
        fs::write(pipelines_dir.join("techspec.yml"), "dummy").expect("failed to write");
        fs::write(pipelines_dir.join("research.yml"), "dummy").expect("failed to write");
        fs::write(pipelines_dir.join("not-yaml.txt"), "dummy").expect("failed to write");

        let config = ForgeConfig {
            version: "1".to_string(),
            home: dir.path().to_string_lossy().to_string(),
            store: "/tmp/store".to_string(),
            pipelines: vec!["pipelines/".to_string()],
            global_references: vec![],
            log_level: None,
        };
        let list = config.list_pipelines().expect("failed to list");
        assert_eq!(list.len(), 2);
        // Sorted alphabetically
        assert_eq!(list[0].0, "research");
        assert_eq!(list[1].0, "techspec");
    }

    #[test]
    fn test_list_pipelines_missing_dir() {
        let config = ForgeConfig {
            version: "1".to_string(),
            home: "/tmp/nonexistent".to_string(),
            store: "/tmp/store".to_string(),
            pipelines: vec!["pipelines/".to_string()],
            global_references: vec![],
            log_level: None,
        };
        let list = config.list_pipelines().expect("failed to list");
        assert!(list.is_empty());
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
