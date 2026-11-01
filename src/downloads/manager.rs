use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, ManagedTorrent, Session, SessionOptions,
};

use super::hook;
use super::store::{
    DownloadLayout, DownloadRequest, PayloadFile, Phase, Record, Store, ensure_directory, safe_name,
};
use crate::config::{DownloadsConfig, LibrqbitConfig};

#[derive(Clone, Debug)]
pub struct DownloadProgress {
    pub downloaded: u64,
    pub total: u64,
    pub speed: f64,
    pub done: bool,
    pub status: &'static str,
}

pub struct DownloadManager {
    store: Store,
    config: DownloadsConfig,
    session: Arc<Session>,
    active: Mutex<HashMap<String, u64>>,
}

struct Claim<'a> {
    active: &'a Mutex<HashMap<String, u64>>,
    hash: String,
}

impl Drop for Claim<'_> {
    fn drop(&mut self) {
        self.active
            .lock()
            .expect("download reservation lock")
            .remove(&self.hash);
    }
}

impl DownloadManager {
    pub async fn new(engine: &LibrqbitConfig, config: DownloadsConfig) -> Result<Arc<Self>> {
        let store = Store::open(engine, &config)?;
        // Pirata journals each torrent itself so promotion and resume share one source of truth.
        let session = Session::new_with_opts(
            store.layout.incomplete.clone(),
            SessionOptions {
                disable_dht_persistence: true,
                ..SessionOptions::default()
            },
        )
        .await
        .context("could not initialize the torrent engine")?;
        Ok(Arc::new(Self {
            store,
            config,
            session,
            active: Mutex::new(HashMap::new()),
        }))
    }

    pub fn layout(&self) -> &DownloadLayout {
        &self.store.layout
    }

    pub fn resumable(&self) -> Result<Vec<DownloadRequest>> {
        Ok(self
            .store
            .records()?
            .into_iter()
            .filter(|record| {
                record.phase != Phase::Completed
                    || self
                        .store
                        .validate_payload(record, &self.store.destination(record), true)
                        .is_ok()
            })
            .map(|record| record.request())
            .collect())
    }

    pub async fn shutdown(&self) {
        self.session.stop().await;
    }

    pub async fn download(
        &self,
        request: DownloadRequest,
        mut progress: impl FnMut(DownloadProgress) + Send,
    ) -> Result<PathBuf> {
        let hash = super::store::magnet_hash(&request.magnet)?;
        {
            let mut active = self.active.lock().expect("download reservation lock");
            if active.contains_key(&hash) {
                bail!("this torrent is already active in Pirata");
            }
            active.insert(hash.clone(), 0);
        }
        let _claim = Claim {
            active: &self.active,
            hash,
        };
        let mut record = self.store.begin(&request)?;
        self.detach(&record).await?;
        progress(DownloadProgress {
            downloaded: 0,
            total: request.expected_bytes,
            speed: 0.0,
            done: false,
            status: "Resolving metadata",
        });

        let cancelled = self.session.cancellation_token();
        let result = tokio::select! {
            result = self.run(&mut record, &mut progress) => result,
            _ = cancelled.cancelled() => bail!("download paused; run `pirata resume` or reopen the TUI to continue"),
        };
        result
    }

    async fn run(
        &self,
        record: &mut Record,
        progress: &mut (impl FnMut(DownloadProgress) + Send),
    ) -> Result<PathBuf> {
        if record.phase == Phase::Completed {
            let destination = self.store.destination(record);
            self.store.validate_payload(record, &destination, true)
                .context("completed files are missing or changed; they will not be downloaded again automatically")?;
            let handle = self.attach(record, &destination, None).await?;
            if !handle.stats().finished {
                bail!(
                    "completed files failed verification at {}; existing data was retained",
                    destination.display()
                );
            }
            self.store.cleanup_source(record)?;
            self.session.unpause(&handle).await?;
            return self
                .finish(record, handle.stats().total_bytes, progress)
                .await;
        }

        let source = self.store.source(record);
        fs::create_dir_all(&source)?;
        ensure_directory(&source)?;
        let metadata = if self.store.torrent_path(record).try_exists()? {
            AddTorrent::from_bytes(fs::read(self.store.torrent_path(record))?)
        } else {
            AddTorrent::from_url(record.magnet.clone())
        };
        let metadata = self
            .session
            .add_torrent(
                metadata,
                Some(AddTorrentOptions {
                    list_only: true,
                    ..AddTorrentOptions::default()
                }),
            )
            .await?;
        let AddTorrentResponse::ListOnly(metadata) = metadata else {
            bail!("torrent metadata lookup returned an active torrent");
        };
        if metadata.info_hash.as_string() != record.info_hash {
            bail!("saved torrent metadata does not match its download record");
        }
        let total = metadata
            .info
            .iter_file_lengths()?
            .try_fold(0u64, |sum, length| {
                sum.checked_add(length).context("torrent size overflow")
            })?;
        if record.phase == Phase::Downloading
            && let Some(name) = metadata
                .info
                .name
                .as_ref()
                .and_then(|name| std::str::from_utf8(name.as_ref()).ok())
        {
            record.name = name.to_owned();
            record.destination = record
                .destination
                .parent()
                .context("completed path has no parent")?
                .join(format!("{} [{}]", safe_name(name), record.info_hash));
        }
        self.store.save_torrent(record, &metadata.torrent_bytes)?;
        record.files = metadata
            .info
            .iter_file_details()?
            .filter(|file| !file.attrs().padding)
            .map(|file| {
                Ok(PayloadFile {
                    path: file.filename.to_pathbuf()?,
                    len: file.len,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        self.store.save(record)?;
        if fs::read_dir(&source)?.next().is_none() {
            self.reserve(record, total)?;
        }
        progress(DownloadProgress {
            downloaded: 0,
            total,
            speed: 0.0,
            done: false,
            status: "Verifying existing data",
        });
        let handle = self
            .attach(record, &source, Some(metadata.seen_peers))
            .await?;
        if !handle.stats().finished {
            self.reserve(
                record,
                handle
                    .stats()
                    .total_bytes
                    .saturating_sub(handle.stats().progress_bytes),
            )?;
            self.session.unpause(&handle).await?;
        }

        let mut interval = tokio::time::interval(Duration::from_millis(500));
        let mut last_tick = tokio::time::Instant::now();
        let mut last_bytes = handle.stats().progress_bytes;
        loop {
            interval.tick().await;
            let stats = handle.stats();
            if let Some(error) = stats.error {
                bail!("torrent engine: {error}");
            }
            let now = tokio::time::Instant::now();
            let speed = stats.progress_bytes.saturating_sub(last_bytes) as f64
                / now.duration_since(last_tick).as_secs_f64().max(0.001);
            last_bytes = stats.progress_bytes;
            last_tick = now;
            self.active
                .lock()
                .expect("download reservation lock")
                .insert(
                    record.info_hash.clone(),
                    stats.total_bytes.saturating_sub(stats.progress_bytes),
                );
            progress(DownloadProgress {
                downloaded: stats.progress_bytes,
                total: stats.total_bytes,
                speed,
                done: false,
                status: if stats.finished {
                    "Finalizing"
                } else {
                    "Downloading"
                },
            });
            if stats.finished {
                break;
            }
            if let Err(error) = self.check_free_space(&source, 0) {
                self.session.pause(&handle).await?;
                bail!(
                    "download paused: {error}. Free space and run `pirata resume` or retry in the TUI"
                );
            }
        }

        if !handle.is_paused() {
            self.session.pause(&handle).await?;
        }
        self.session.delete(handle.id().into(), false).await?;
        drop(handle);
        let destination = self.store.promote(record)?;
        let completed = self.attach(record, &destination, None).await?;
        if !completed.stats().finished {
            bail!(
                "promoted files failed verification; incomplete data was retained at {}",
                source.display()
            );
        }
        self.store.complete(record)?;
        self.session.unpause(&completed).await?;
        self.finish(record, total, progress).await
    }

    async fn attach(
        &self,
        record: &Record,
        folder: &Path,
        peers: Option<Vec<std::net::SocketAddr>>,
    ) -> Result<Arc<ManagedTorrent>> {
        ensure_directory(folder)?;
        self.store
            .validate_payload(record, folder, record.phase == Phase::Completed)?;
        let bytes = fs::read(self.store.torrent_path(record))?;
        let handle = self
            .session
            .add_torrent(
                AddTorrent::from_bytes(bytes),
                Some(AddTorrentOptions {
                    paused: true,
                    // Only directories owned by this hash's durable record are ever resumed.
                    overwrite: true,
                    output_folder: Some(
                        folder
                            .to_str()
                            .context("download path is not valid UTF-8")?
                            .to_owned(),
                    ),
                    initial_peers: peers,
                    ..AddTorrentOptions::default()
                }),
            )
            .await?
            .into_handle()
            .context("torrent engine returned no download handle")?;
        handle.wait_until_initialized().await?;
        Ok(handle)
    }

    async fn detach(&self, record: &Record) -> Result<()> {
        let id = librqbit::api::TorrentIdOrHash::parse(&record.info_hash)?;
        if let Some(handle) = self.session.get(id) {
            self.session.delete(handle.id().into(), false).await?;
        }
        Ok(())
    }

    async fn finish(
        &self,
        record: &mut Record,
        total: u64,
        progress: &mut (impl FnMut(DownloadProgress) + Send),
    ) -> Result<PathBuf> {
        let destination = self.store.destination(record);
        if !record.hook_done {
            progress(DownloadProgress {
                downloaded: total,
                total,
                speed: 0.0,
                done: false,
                status: "Running completion hook",
            });
            hook::run(&self.config, record, &destination).await
                .with_context(|| format!("download is complete at {}, but its completion hook failed; retry with `pirata resume`", destination.display()))?;
            record.hook_done = true;
            self.store.save(record)?;
        }
        progress(DownloadProgress {
            downloaded: total,
            total,
            speed: 0.0,
            done: true,
            status: "Completed",
        });
        Ok(destination)
    }

    fn reserve(&self, record: &Record, required: u64) -> Result<()> {
        let mut active = self.active.lock().expect("download reservation lock");
        let reserved = active
            .iter()
            .filter(|(hash, _)| *hash != &record.info_hash)
            .fold(required, |total, (_, bytes)| total.saturating_add(*bytes));
        self.check_free_space(&self.store.source(record), reserved)?;
        active.insert(record.info_hash.clone(), required);
        Ok(())
    }

    fn check_free_space(&self, path: &Path, required: u64) -> Result<()> {
        check_space(
            fs2::available_space(path)?,
            fs2::total_space(path)?,
            required,
            &self.config,
        )
    }
}

fn check_space(available: u64, total: u64, required: u64, config: &DownloadsConfig) -> Result<()> {
    let percent = (u128::from(total) * u128::from(config.min_free_space_percent) / 100) as u64;
    let reserve = config.min_free_space_bytes.max(percent);
    if u128::from(available) < u128::from(required) + u128::from(reserve) {
        bail!(
            "not enough free space: {} available, {} needed for active downloads plus {} reserved; free space and retry with `pirata resume`",
            crate::util::format_size(available),
            crate::util::format_size(required),
            crate::util::format_size(reserve)
        );
    }
    Ok(())
}

#[cfg(test)]
#[path = "manager_tests.rs"]
mod tests;
