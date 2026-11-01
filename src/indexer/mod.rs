use anyhow::Result;
use async_trait::async_trait;

use crate::model::Torrent;

pub mod pirate_bay;

#[allow(clippy::double_must_use)] // async_trait adds must_use to generated Future methods.
#[async_trait]
pub trait Indexer: Send + Sync {
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<Torrent>>;
    async fn trending(&self, limit: usize) -> Result<Vec<Torrent>>;
    async fn info(&self, id: &str) -> Result<Torrent>;
}
