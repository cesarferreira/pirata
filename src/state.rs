use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::model::Torrent;

const MAX_DETACHED_DOWNLOADS: usize = 50;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetachedDownloadRecord {
    pub torrent: Torrent,
    pub pid: u32,
    pub started_unix_secs: u64,
    pub download_dir: Option<String>,
}

impl DetachedDownloadRecord {
    pub fn key(&self) -> (u32, u64) {
        (self.pid, self.started_unix_secs)
    }
}

pub fn record_detached_download(
    torrent: &Torrent,
    pid: u32,
    download_dir: Option<String>,
) -> Result<()> {
    let path = detached_downloads_path()?;
    let mut records = load_detached_downloads_from_path(&path)?;
    records.push(DetachedDownloadRecord {
        torrent: torrent.clone(),
        pid,
        started_unix_secs: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        download_dir,
    });
    if records.len() > MAX_DETACHED_DOWNLOADS {
        let drain_len = records.len() - MAX_DETACHED_DOWNLOADS;
        records.drain(0..drain_len);
    }
    save_detached_downloads(&path, &records)
}

pub fn load_recent_detached_downloads(limit: usize) -> Result<Vec<DetachedDownloadRecord>> {
    let path = detached_downloads_path()?;
    let mut records = load_detached_downloads_from_path(&path)?;
    if records.len() > limit {
        records = records.split_off(records.len() - limit);
    }
    records.reverse();
    Ok(records)
}

fn detached_downloads_path() -> Result<PathBuf> {
    let dirs = ProjectDirs::from("dev", "pirate", "pirate-ctl")
        .context("unable to determine pirate-ctl state directory")?;
    Ok(dirs.cache_dir().join("detached-downloads.json"))
}

fn load_detached_downloads_from_path(path: &PathBuf) -> Result<Vec<DetachedDownloadRecord>> {
    match fs::read_to_string(path) {
        Ok(contents) => serde_json::from_str(&contents)
            .with_context(|| format!("failed to parse {}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error).with_context(|| format!("failed to read {}", path.display())),
    }
}

fn save_detached_downloads(path: &PathBuf, records: &[DetachedDownloadRecord]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let payload =
        serde_json::to_string_pretty(records).context("failed to serialize detached downloads")?;
    fs::write(path, payload).with_context(|| format!("failed to write {}", path.display()))
}
