use anyhow::Result;
use async_trait::async_trait;

use crate::model::Torrent;

pub mod aria2;
pub mod system;
pub mod transmission;

#[async_trait]
pub trait Downloader: Send + Sync {
    async fn add_magnet(&self, magnet: &str) -> Result<()>;

    async fn add_torrent(&self, torrent: &Torrent) -> Result<()> {
        self.add_magnet(&torrent.resolved_magnet()).await
    }
}
