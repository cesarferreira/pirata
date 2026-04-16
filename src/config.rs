use std::fmt;
use std::fs as stdfs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use directories::{BaseDirs, ProjectDirs, UserDirs};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::fs;

use crate::model::{DownloaderKind, IndexerKind};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    #[serde(default)]
    pub defaults: DefaultsConfig,
    #[serde(default)]
    pub aria2: Aria2Config,
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
pub struct Aria2Config {
    pub download_dir: Option<String>,
}

impl Default for Aria2Config {
    fn default() -> Self {
        Self {
            download_dir: default_user_download_dir(),
        }
    }
}

impl Aria2Config {
    pub fn download_target_display(&self) -> String {
        self.download_dir
            .clone()
            .or_else(default_user_download_dir)
            .unwrap_or_else(|| "current working directory".to_string())
    }

    pub fn download_dir_path(&self) -> Option<PathBuf> {
        self.download_dir
            .clone()
            .or_else(default_user_download_dir)
            .map(PathBuf::from)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransmissionConfig {
    #[serde(default = "default_transmission_client")]
    pub client: TransmissionClient,
    #[serde(default = "default_transmission_rpc_url")]
    pub rpc_url: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub download_dir: Option<String>,
}

impl Default for TransmissionConfig {
    fn default() -> Self {
        Self {
            client: default_transmission_client(),
            rpc_url: default_transmission_rpc_url(),
            username: None,
            password: None,
            download_dir: None,
        }
    }
}

impl TransmissionConfig {
    pub fn download_target_display(&self) -> String {
        self.download_dir
            .clone()
            .or_else(transmission_default_download_dir)
            .unwrap_or_else(|| "Transmission default download directory".to_string())
    }

    pub fn download_dir_path(&self) -> Option<PathBuf> {
        self.download_dir
            .clone()
            .or_else(transmission_default_download_dir)
            .map(PathBuf::from)
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

    pub async fn save(&self, path_override: Option<PathBuf>) -> Result<PathBuf> {
        let path = path_override.unwrap_or_else(default_config_path);
        let contents = toml::to_string_pretty(self).context("failed to serialize configuration")?;
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
        let dirs = ProjectDirs::from("dev", "pirate", "pirata")
            .context("unable to determine cache directory")?;
        Ok(dirs.cache_dir().to_path_buf())
    }

    pub fn history_path(&self) -> Result<PathBuf> {
        Ok(self.cache_dir()?.join("download-history.json"))
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
    DownloaderKind::Aria2
}

fn default_limit() -> usize {
    20
}

fn default_transmission_rpc_url() -> String {
    "http://localhost:9091/transmission/rpc".to_string()
}

fn default_transmission_client() -> TransmissionClient {
    TransmissionClient::Cli
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

fn transmission_default_download_dir() -> Option<String> {
    transmission_settings_candidates()
        .into_iter()
        .find_map(|path| read_transmission_download_dir(&path))
}

fn default_user_download_dir() -> Option<String> {
    UserDirs::new().and_then(|dirs| dirs.download_dir().map(|path| path.display().to_string()))
}

fn transmission_settings_candidates() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Some(base_dirs) = BaseDirs::new() {
        let home = base_dirs.home_dir();
        paths.push(
            home.join("Library")
                .join("Application Support")
                .join("Transmission")
                .join("settings.json"),
        );
        paths.push(
            home.join(".config")
                .join("transmission-daemon")
                .join("settings.json"),
        );
        paths.push(
            home.join(".config")
                .join("transmission")
                .join("settings.json"),
        );
    }

    paths
}

fn read_transmission_download_dir(path: &PathBuf) -> Option<String> {
    let contents = stdfs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(&contents).ok()?;
    value
        .get("download-dir")
        .and_then(Value::as_str)
        .map(str::to_owned)
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TransmissionClient {
    Cli,
    Rpc,
    Auto,
}

impl fmt::Display for TransmissionClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cli => write!(f, "cli"),
            Self::Rpc => write!(f, "rpc"),
            Self::Auto => write!(f, "auto"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{AppConfig, config_path_candidates};
    use crate::model::DownloaderKind;

    #[test]
    fn defaults_to_aria2_downloader() {
        let config = AppConfig::default();

        assert_eq!(config.defaults.downloader, DownloaderKind::Aria2);
    }

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
}
