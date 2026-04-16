use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use directories::{BaseDirs, ProjectDirs};
use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::model::{DownloaderKind, IndexerKind};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    #[serde(default)]
    pub defaults: DefaultsConfig,
    #[serde(default)]
    pub transmission: TransmissionConfig,
    #[serde(default)]
    pub cache: CacheConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefaultsConfig {
    #[serde(default = "default_indexer")]
    pub indexer: IndexerKind,
    #[serde(default = "default_downloader")]
    pub downloader: DownloaderKind,
    #[serde(default = "default_limit")]
    pub search_limit: usize,
}

impl Default for DefaultsConfig {
    fn default() -> Self {
        Self {
            indexer: default_indexer(),
            downloader: default_downloader(),
            search_limit: default_limit(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransmissionConfig {
    #[serde(default = "default_transmission_rpc_url")]
    pub rpc_url: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub download_dir: Option<String>,
}

impl Default for TransmissionConfig {
    fn default() -> Self {
        Self {
            rpc_url: default_transmission_rpc_url(),
            username: None,
            password: None,
            download_dir: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    #[serde(default = "default_cache_ttl_minutes")]
    pub ttl_minutes: u64,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            ttl_minutes: default_cache_ttl_minutes(),
        }
    }
}

impl AppConfig {
    pub async fn load(path_override: Option<PathBuf>) -> Result<Self> {
        if let Some(path) = path_override {
            return load_config_path(&path).await;
        }

        for path in default_config_candidates() {
            match load_config_path(&path).await {
                Ok(config) => return Ok(config),
                Err(error) if is_missing_file(&error) => continue,
                Err(error) => return Err(error),
            }
        }

        Ok(Self::default())
    }

    pub fn cache_dir(&self) -> Result<PathBuf> {
        let dirs = ProjectDirs::from("dev", "pirate", "pirata")
            .context("unable to determine cache directory")?;
        Ok(dirs.cache_dir().to_path_buf())
    }

    pub fn history_path(&self) -> Result<PathBuf> {
        let dirs = ProjectDirs::from("dev", "pirate", "pirata")
            .context("unable to determine history directory")?;
        Ok(dirs.data_local_dir().join("download-history.json"))
    }

    pub fn cache_ttl(&self) -> Duration {
        Duration::from_secs(self.cache.ttl_minutes.saturating_mul(60))
    }
}

pub fn default_config_path() -> PathBuf {
    default_config_candidates()
        .into_iter()
        .next()
        .unwrap_or_else(|| PathBuf::from(".config/pirata/config.toml"))
}

fn default_indexer() -> IndexerKind {
    IndexerKind::Piratebay
}

fn default_downloader() -> DownloaderKind {
    DownloaderKind::Transmission
}

fn default_limit() -> usize {
    20
}

fn default_transmission_rpc_url() -> String {
    "http://localhost:9091/transmission/rpc".to_string()
}

fn default_cache_ttl_minutes() -> u64 {
    5
}

async fn load_config_path(path: &Path) -> Result<AppConfig> {
    match fs::read_to_string(path).await {
        Ok(contents) => toml::from_str(&contents)
            .with_context(|| format!("failed to parse config at {}", path.display())),
        Err(error) => Err(error).with_context(|| format!("failed to read {}", path.display())),
    }
}

fn default_config_candidates() -> Vec<PathBuf> {
    let home_dir = BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf());
    config_path_candidates(home_dir.as_deref())
}

fn config_path_candidates(home_dir: Option<&Path>) -> Vec<PathBuf> {
    match home_dir {
        Some(base) => vec![
            base.join(".config/pirata/config.toml"),
            base.join(".config/pirate-ctl/config.toml"),
        ],
        None => vec![
            PathBuf::from(".config/pirata/config.toml"),
            PathBuf::from(".config/pirate-ctl/config.toml"),
        ],
    }
}

fn is_missing_file(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<std::io::Error>()
        .is_some_and(|inner| inner.kind() == std::io::ErrorKind::NotFound)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::config_path_candidates;

    #[test]
    fn prefers_pirata_config_and_keeps_legacy_fallback() {
        let candidates = config_path_candidates(Some(Path::new("/tmp/home")));
        assert_eq!(
            candidates[0],
            Path::new("/tmp/home/.config/pirata/config.toml")
        );
        assert_eq!(
            candidates[1],
            Path::new("/tmp/home/.config/pirate-ctl/config.toml")
        );
    }

    #[test]
    fn uses_relative_candidates_when_home_is_unavailable() {
        let candidates = config_path_candidates(None);
        assert_eq!(candidates[0], Path::new(".config/pirata/config.toml"));
        assert_eq!(candidates[1], Path::new(".config/pirate-ctl/config.toml"));
    }
}
