use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Result, anyhow, bail};
use async_trait::async_trait;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use tokio::task::JoinSet;

use crate::config::{DownloadsConfig, LibrqbitConfig};
use crate::downloader::Downloader;
use crate::downloads::{DownloadManager, DownloadProgress, DownloadRequest};
use crate::model::Torrent;

#[derive(Debug, Clone)]
pub struct LibrqbitDownloader {
    config: LibrqbitConfig,
    downloads: DownloadsConfig,
}

impl LibrqbitDownloader {
    pub fn new(config: LibrqbitConfig) -> Self {
        Self::with_downloads(config, DownloadsConfig::default())
    }

    pub fn with_downloads(config: LibrqbitConfig, downloads: DownloadsConfig) -> Self {
        Self { config, downloads }
    }
}

#[async_trait]
impl Downloader for LibrqbitDownloader {
    async fn add_torrent(&self, torrent: &Torrent) -> Result<()> {
        run_download(
            &self.config,
            self.downloads.clone(),
            DownloadRequest::from_torrent(torrent),
        )
        .await
    }

    async fn add_magnet(&self, magnet: &str) -> Result<()> {
        run_download(
            &self.config,
            self.downloads.clone(),
            DownloadRequest::from_magnet(magnet)?,
        )
        .await
    }
}

pub async fn download_magnet(magnet: &str, name: &str, output: PathBuf) -> Result<()> {
    let mut request = DownloadRequest::from_magnet(magnet)?;
    request.name = name.to_owned();
    run_download(
        &LibrqbitConfig {
            download_dir: Some(output.display().to_string()),
        },
        DownloadsConfig::default(),
        request,
    )
    .await
}

async fn run_download(
    config: &LibrqbitConfig,
    downloads: DownloadsConfig,
    request: DownloadRequest,
) -> Result<()> {
    let manager = DownloadManager::new(config, downloads).await?;
    let progress = progress_bar(&request);
    let result = tokio::select! {
        result = download_with_progress(manager.clone(), request, progress) => result.map(|_| ()),
        _ = tokio::signal::ctrl_c() => Err(anyhow!("download paused; run `pirata resume` to continue")),
    };
    manager.shutdown().await;
    result
}

pub async fn resume(config: &LibrqbitConfig, downloads: DownloadsConfig) -> Result<()> {
    let manager = DownloadManager::new(config, downloads).await?;
    let requests = manager.resumable()?;
    if requests.is_empty() {
        println!("No saved downloads to resume.");
        manager.shutdown().await;
        return Ok(());
    }
    let progress = MultiProgress::new();
    let mut tasks = JoinSet::new();
    for request in requests {
        let bar = progress.add(progress_bar(&request));
        tasks.spawn(download_with_progress(manager.clone(), request, bar));
    }
    let result = tokio::select! {
        result = async {
            let mut errors = Vec::new();
            while let Some(result) = tasks.join_next().await {
                match result {
                    Ok(Ok(_)) => {}
                    Ok(Err(error)) => errors.push(format!("{error:#}")),
                    Err(error) => errors.push(error.to_string()),
                }
            }
            if !errors.is_empty() { bail!("Some downloads could not resume:\n{}", errors.join("\n")); }
            Ok(())
        } => result,
        _ = tokio::signal::ctrl_c() => Err(anyhow!("downloads paused; run `pirata resume` to continue")),
    };
    tasks.abort_all();
    manager.shutdown().await;
    result
}

fn progress_bar(request: &DownloadRequest) -> ProgressBar {
    let bar = ProgressBar::new(request.expected_bytes);
    bar.set_prefix(request.name.clone());
    bar.set_style(
        ProgressStyle::with_template(
            "{prefix:.cyan} [{wide_bar:.cyan/black}] {bytes}/{total_bytes} {bytes_per_sec} {msg}",
        )
        .expect("progress template"),
    );
    bar
}

async fn download_with_progress(
    manager: Arc<DownloadManager>,
    request: DownloadRequest,
    bar: ProgressBar,
) -> Result<PathBuf> {
    let result = manager
        .download(request, |update: DownloadProgress| {
            bar.set_length(update.total);
            bar.set_position(update.downloaded);
            bar.set_message(update.status);
        })
        .await;
    match &result {
        Ok(path) => bar.finish_with_message(format!("Completed → {}", path.display())),
        Err(error) => bar.abandon_with_message(format!("{error:#}")),
    }
    result
}
