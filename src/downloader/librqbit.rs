use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use crossterm::style::Stylize;
use indicatif::{ProgressBar, ProgressStyle};
use librqbit::{AddTorrent, AddTorrentOptions, ManagedTorrent, Session};

use crate::config::LibrqbitConfig;
use crate::downloader::Downloader;
use crate::model::Torrent;

#[derive(Debug, Clone)]
pub struct LibrqbitDownloader {
    config: LibrqbitConfig,
}

impl LibrqbitDownloader {
    pub fn new(config: LibrqbitConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Downloader for LibrqbitDownloader {
    async fn add_torrent(&self, torrent: &Torrent) -> Result<()> {
        let output = self
            .config
            .download_dir_path()
            .context("librqbit download directory is not configured")?;
        download_magnet(&torrent.resolved_magnet(), &torrent.name, output).await
    }

    async fn add_magnet(&self, magnet: &str) -> Result<()> {
        let output = self
            .config
            .download_dir_path()
            .context("librqbit download directory is not configured")?;
        download_magnet(magnet, magnet, output).await
    }
}

pub async fn download_magnet(magnet: &str, name: &str, output: PathBuf) -> Result<()> {
    tokio::fs::create_dir_all(&output)
        .await
        .with_context(|| format!("failed to create {}", output.display()))?;

    println!("{} {}", "Download:".bold().cyan(), name.magenta());
    println!(
        "{} {}",
        "Target:".bold().blue(),
        output.display().to_string().white()
    );

    let session = Session::new(output)
        .await
        .map_err(|error| anyhow!("librqbit session init failed: {error}"))?;

    let handle = add_magnet(&session, magnet).await?;
    handle
        .wait_until_initialized()
        .await
        .map_err(|error| anyhow!("metadata fetch failed: {error}"))?;

    let progress = make_progress_bar(handle.stats().total_bytes);
    let progress_handle = Arc::clone(&handle);
    let progress_bar = progress.clone();
    let progress_task =
        tokio::spawn(async move { drive_progress(progress_handle, progress_bar).await });

    let result = handle
        .wait_until_completed()
        .await
        .map_err(|error| anyhow!("download failed: {error}"));

    progress_task.abort();
    let _ = progress_task.await;
    if result.is_ok() {
        progress.finish_with_message("done");
    }
    result
}

pub async fn add_magnet(session: &Arc<Session>, magnet: &str) -> Result<Arc<ManagedTorrent>> {
    let options = AddTorrentOptions {
        overwrite: true,
        ..Default::default()
    };
    session
        .add_torrent(AddTorrent::from_url(magnet.to_string()), Some(options))
        .await
        .map_err(|error| anyhow!("add torrent failed: {error}"))?
        .into_handle()
        .ok_or_else(|| anyhow!("librqbit session returned no handle"))
}

fn make_progress_bar(total: u64) -> ProgressBar {
    let progress = ProgressBar::new(total.max(1));
    let template = "  {spinner:.43} [{elapsed_precise:.250}] \
                    [{bar:32.43/238}] \
                    {bytes:>10.45}/{total_bytes:.250} \
                    {bytes_per_sec:>11.215}  ETA {eta:.250}";
    progress.set_style(
        ProgressStyle::with_template(template)
            .unwrap()
            .progress_chars("━╸ ")
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"),
    );
    progress.enable_steady_tick(Duration::from_millis(100));
    progress
}

async fn drive_progress(handle: Arc<ManagedTorrent>, progress: ProgressBar) {
    let mut interval = tokio::time::interval(Duration::from_millis(500));
    loop {
        interval.tick().await;
        let stats = handle.stats();
        if stats.total_bytes > 0 && progress.length() != Some(stats.total_bytes) {
            progress.set_length(stats.total_bytes);
        }
        progress.set_position(stats.progress_bytes);
        if stats.finished {
            break;
        }
    }
}
