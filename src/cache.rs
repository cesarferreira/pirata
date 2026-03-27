use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use tokio::fs;

use crate::model::Torrent;

#[derive(Debug, Clone)]
pub struct SearchCache {
    dir: PathBuf,
    ttl: Duration,
}

#[derive(Debug, Serialize, Deserialize)]
struct CacheEntry {
    created_at_epoch_secs: u64,
    results: Vec<Torrent>,
}

impl SearchCache {
    pub fn new(dir: PathBuf, ttl: Duration) -> Self {
        Self { dir, ttl }
    }

    pub async fn get(&self, query: &str, limit: usize) -> Result<Option<Vec<Torrent>>> {
        let path = self.entry_path(query, limit);
        let Ok(contents) = fs::read_to_string(&path).await else {
            return Ok(None);
        };
        let entry: CacheEntry = serde_json::from_str(&contents)
            .with_context(|| format!("failed to parse cache entry {}", path.display()))?;
        if self.is_expired(entry.created_at_epoch_secs) {
            let _ = fs::remove_file(&path).await;
            return Ok(None);
        }
        Ok(Some(entry.results))
    }

    pub async fn put(&self, query: &str, limit: usize, results: &[Torrent]) -> Result<()> {
        ensure_dir(&self.dir).await?;
        let entry = CacheEntry {
            created_at_epoch_secs: now_epoch_secs(),
            results: results.to_vec(),
        };
        let payload = serde_json::to_string(&entry)?;
        fs::write(self.entry_path(query, limit), payload).await?;
        Ok(())
    }

    fn entry_path(&self, query: &str, limit: usize) -> PathBuf {
        let mut hasher = Sha1::new();
        hasher.update(query.as_bytes());
        hasher.update(limit.to_string().as_bytes());
        let digest = format!("{:x}", hasher.finalize());
        self.dir.join(format!("{digest}.json"))
    }

    fn is_expired(&self, created_at_epoch_secs: u64) -> bool {
        let expires_at = created_at_epoch_secs.saturating_add(self.ttl.as_secs());
        now_epoch_secs() > expires_at
    }
}

async fn ensure_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path).await?;
    Ok(())
}

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
