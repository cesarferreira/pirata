use std::process::Stdio;

use anyhow::{Context, Result, bail};
use async_trait::async_trait;

use crate::config::Aria2Config;
use crate::downloader::Downloader;
use crate::model::Torrent;
use crate::util::ensure_aria2_available;

#[derive(Debug, Clone)]
pub struct Aria2Downloader {
    config: Aria2Config,
}

impl Aria2Downloader {
    pub fn new(config: Aria2Config) -> Self {
        Self { config }
    }

    fn build_command(&self, magnet: &str) -> std::process::Command {
        let mut command = std::process::Command::new("aria2c");
        command.arg("--seed-time=0");
        command.arg("--summary-interval=1");
        command.arg("--show-console-readout=true");
        command.arg("--console-log-level=warn");
        if let Some(download_dir) = &self.config.download_dir {
            command.arg("--dir").arg(download_dir);
        }
        command.arg(magnet);
        command
    }

    fn spawn_background(&self, magnet: &str) -> Result<()> {
        ensure_aria2_available()?;

        let mut command = self.build_command(magnet);
        command.stdin(Stdio::null());
        command.stdout(Stdio::null());
        command.stderr(Stdio::null());
        command
            .spawn()
            .context("failed to start aria2c in background")?;
        Ok(())
    }

    fn run_foreground(&self, torrent: &Torrent, magnet: &str) -> Result<()> {
        ensure_aria2_available()?;

        println!(
            "Downloading '{}' to {} | listed seeders {} | listed leechers {}",
            torrent.name,
            self.config.download_target_display(),
            torrent.seeders,
            torrent.leechers
        );

        let status = self
            .build_command(magnet)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .context("failed to run aria2c")?;

        if status.success() {
            Ok(())
        } else {
            bail!("aria2c exited with status {status}")
        }
    }
}

#[async_trait]
impl Downloader for Aria2Downloader {
    async fn add_torrent(&self, torrent: &Torrent) -> Result<()> {
        self.run_foreground(torrent, &torrent.resolved_magnet())
    }

    async fn add_magnet(&self, magnet: &str) -> Result<()> {
        self.spawn_background(magnet)
    }
}
