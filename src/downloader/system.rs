use anyhow::Result;
use async_trait::async_trait;

use crate::downloader::Downloader;

#[derive(Debug, Default)]
pub struct SystemDownloader;

#[async_trait]
impl Downloader for SystemDownloader {
    async fn add_magnet(&self, magnet: &str) -> Result<()> {
        open::that(magnet)?;
        Ok(())
    }
}
