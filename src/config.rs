use std::path::PathBuf;
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
        let path = path_override.unwrap_or_else(default_config_path);
        match fs::read_to_string(&path).await {
            Ok(contents) => toml::from_str(&contents)
                .with_context(|| format!("failed to parse config at {}", path.display())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error).with_context(|| format!("failed to read {}", path.display())),
        }
    }

    pub async fn save(&self, path_override: Option<PathBuf>) -> Result<PathBuf> {
        let path = path_override.unwrap_or_else(default_config_path);
        let contents =
            toml::to_string_pretty(self).context("failed to serialize configuration")?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .await
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        fs::write(&path, contents)
            .await
            .with_context(|| format!("failed to write {}", path.display()))?;
        Ok(path)
    }

    pub fn cache_dir(&self) -> Result<PathBuf> {
        let dirs = ProjectDirs::from("dev", "pirate", "pirate-ctl")
            .context("unable to determine cache directory")?;
        Ok(dirs.cache_dir().to_path_buf())
    }

    pub fn cache_ttl(&self) -> Duration {
        Duration::from_secs(self.cache.ttl_minutes.saturating_mul(60))
    }
}

pub fn default_config_path() -> PathBuf {
    let Some(base_dirs) = BaseDirs::new() else {
        return PathBuf::from(".config/pirate-ctl/config.toml");
    };
    base_dirs.home_dir().join(".config/pirate-ctl/config.toml")
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

#[cfg(test)]
mod tests {
    use super::AppConfig;
    use crate::model::DownloaderKind;

    #[test]
    fn defaults_to_transmission_downloader() {
        let config = AppConfig::default();

        assert_eq!(config.defaults.downloader, DownloaderKind::Transmission);
    }
}
