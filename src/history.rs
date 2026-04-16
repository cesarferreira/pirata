use std::{
    collections::{HashMap, HashSet},
    fs as stdfs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::model::{DownloaderKind, TrackedDownload};

#[derive(Debug, Clone)]
pub struct DownloadHistory {
    path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DownloadHistoryEntry {
    pub info_hash: String,
    pub name: String,
    pub target_path: PathBuf,
    pub downloader: DownloaderKind,
    pub added_at_epoch_secs: u64,
    pub completed_at_epoch_secs: Option<u64>,
}

impl DownloadHistory {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub async fn load(&self) -> Result<Vec<DownloadHistoryEntry>> {
        self.load_blocking()
    }

    pub fn load_blocking(&self) -> Result<Vec<DownloadHistoryEntry>> {
        let Ok(contents) = stdfs::read_to_string(&self.path) else {
            return Ok(Vec::new());
        };
        Ok(serde_json::from_str(&contents)?)
    }

    pub async fn load_visible(&self) -> Result<Vec<DownloadHistoryEntry>> {
        self.load_visible_blocking()
    }

    pub fn load_visible_blocking(&self) -> Result<Vec<DownloadHistoryEntry>> {
        let entries = self.load_blocking()?;
        let mut visible = Vec::with_capacity(entries.len());
        for entry in entries {
            if !entry.is_completed() || path_exists(&entry.target_path) {
                visible.push(entry);
            }
        }
        self.save_blocking(&visible)?;
        Ok(visible)
    }

    pub async fn upsert(&self, entry: DownloadHistoryEntry) -> Result<()> {
        self.upsert_blocking(entry)
    }

    pub fn upsert_blocking(&self, entry: DownloadHistoryEntry) -> Result<()> {
        let mut entries = self.load_blocking()?;
        if let Some(existing) = entries
            .iter_mut()
            .find(|existing| existing.info_hash == entry.info_hash)
        {
            *existing = entry;
        } else {
            entries.push(entry);
        }
        self.save_blocking(&entries)
    }

    pub async fn save(&self, entries: &[DownloadHistoryEntry]) -> Result<()> {
        self.save_blocking(entries)
    }

    pub fn save_blocking(&self, entries: &[DownloadHistoryEntry]) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            stdfs::create_dir_all(parent)?;
        }
        stdfs::write(&self.path, serde_json::to_string_pretty(entries)?)?;
        Ok(())
    }
}

impl DownloadHistoryEntry {
    pub fn from_tracked_download(download: &TrackedDownload, now: u64) -> Self {
        Self {
            info_hash: download.info_hash.clone(),
            name: download.name.clone(),
            target_path: download.target_path.clone(),
            downloader: download.downloader,
            added_at_epoch_secs: now,
            completed_at_epoch_secs: download.completed.then_some(now),
        }
    }

    pub fn is_completed(&self) -> bool {
        self.completed_at_epoch_secs.is_some()
    }

    fn into_tracked_download(self) -> TrackedDownload {
        TrackedDownload {
            info_hash: self.info_hash,
            name: self.name,
            target_path: self.target_path,
            downloader: self.downloader,
            percent_done: 100,
            completed: true,
        }
    }
}

pub fn merge_tracked_downloads(
    history: Vec<DownloadHistoryEntry>,
    live: Vec<TrackedDownload>,
    now: u64,
) -> (Vec<DownloadHistoryEntry>, Vec<TrackedDownload>) {
    let mut history_by_hash: HashMap<String, DownloadHistoryEntry> = history
        .into_iter()
        .map(|entry| (entry.info_hash.clone(), entry))
        .collect();
    let live_hashes: HashSet<String> = live.iter().map(|item| item.info_hash.clone()).collect();

    for item in &live {
        if let Some(entry) = history_by_hash.get_mut(&item.info_hash) {
            if item.completed && entry.completed_at_epoch_secs.is_none() {
                entry.completed_at_epoch_secs = Some(now);
            }
            if entry.target_path.as_os_str().is_empty() {
                entry.target_path = item.target_path.clone();
            }
        }
    }

    let mut tracked = live;
    for entry in history_by_hash.values().cloned() {
        if entry.is_completed() && !live_hashes.contains(&entry.info_hash) {
            tracked.push(entry.into_tracked_download());
        }
    }

    tracked.sort_by(|left, right| {
        left.completed
            .cmp(&right.completed)
            .then(left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });

    let mut updated_history: Vec<DownloadHistoryEntry> = history_by_hash.into_values().collect();
    updated_history.sort_by(|left, right| left.info_hash.cmp(&right.info_hash));

    (updated_history, tracked)
}

pub fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn path_exists(path: &Path) -> bool {
    path.exists()
}

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use crate::model::{DownloaderKind, TrackedDownload};

    use super::{DownloadHistory, DownloadHistoryEntry, merge_tracked_downloads};

    fn unique_path(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("pirata-{label}-{nanos}"))
    }

    #[tokio::test]
    async fn persists_and_reloads_history_entries() {
        let root = unique_path("history");
        let history = DownloadHistory::new(root.join("download-history.json"));
        let entry = DownloadHistoryEntry {
            info_hash: "hash-1".into(),
            name: "ubuntu.iso".into(),
            target_path: root.join("downloads/ubuntu.iso"),
            downloader: DownloaderKind::Transmission,
            added_at_epoch_secs: 10,
            completed_at_epoch_secs: Some(20),
        };

        history
            .upsert(entry.clone())
            .await
            .expect("write history entry");
        let loaded = history.load().await.expect("load history");

        assert_eq!(loaded, vec![entry]);
    }

    #[tokio::test]
    async fn prunes_completed_entries_whose_target_is_missing() {
        let root = unique_path("prune");
        std::fs::create_dir_all(root.join("downloads")).expect("create downloads dir");
        let existing = root.join("downloads/existing");
        std::fs::create_dir_all(&existing).expect("create existing target");

        let history = DownloadHistory::new(root.join("download-history.json"));
        history
            .upsert(DownloadHistoryEntry {
                info_hash: "keep".into(),
                name: "keep".into(),
                target_path: existing.clone(),
                downloader: DownloaderKind::Transmission,
                added_at_epoch_secs: 1,
                completed_at_epoch_secs: Some(2),
            })
            .await
            .expect("write keep entry");
        history
            .upsert(DownloadHistoryEntry {
                info_hash: "drop".into(),
                name: "drop".into(),
                target_path: root.join("downloads/missing"),
                downloader: DownloaderKind::Transmission,
                added_at_epoch_secs: 3,
                completed_at_epoch_secs: Some(4),
            })
            .await
            .expect("write drop entry");

        let loaded = history
            .load_visible()
            .await
            .expect("load pruned history entries");

        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].info_hash, "keep");
    }

    #[test]
    fn merge_prefers_live_items_and_keeps_completed_history() {
        let history = vec![
            DownloadHistoryEntry {
                info_hash: "active".into(),
                name: "active item".into(),
                target_path: PathBuf::from("/downloads/active"),
                downloader: DownloaderKind::Transmission,
                added_at_epoch_secs: 10,
                completed_at_epoch_secs: None,
            },
            DownloadHistoryEntry {
                info_hash: "done".into(),
                name: "done item".into(),
                target_path: PathBuf::from("/downloads/done"),
                downloader: DownloaderKind::Transmission,
                added_at_epoch_secs: 20,
                completed_at_epoch_secs: Some(30),
            },
        ];
        let live = vec![TrackedDownload {
            info_hash: "active".into(),
            name: "active item".into(),
            target_path: PathBuf::from("/downloads/active"),
            downloader: DownloaderKind::Transmission,
            percent_done: 42,
            completed: false,
        }];

        let (updated_history, tracked) = merge_tracked_downloads(history, live, 40);

        assert_eq!(tracked.len(), 2);
        assert_eq!(tracked[0].info_hash, "active");
        assert_eq!(tracked[0].percent_done, 42);
        assert_eq!(tracked[1].info_hash, "done");
        assert!(tracked[1].completed);
        assert_eq!(
            updated_history
                .iter()
                .find(|entry| entry.info_hash == "done")
                .and_then(|entry| entry.completed_at_epoch_secs),
            Some(30)
        );
    }
}
