use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use fs2::FileExt;
use librqbit::Magnet;
use serde::{Deserialize, Serialize};

use crate::config::{DownloadsConfig, LibrqbitConfig};
use crate::model::Torrent;

#[derive(Clone, Debug)]
pub struct DownloadRequest {
    pub id: String,
    pub name: String,
    pub magnet: String,
    pub expected_bytes: u64,
}

impl DownloadRequest {
    pub fn from_torrent(torrent: &Torrent) -> Self {
        Self {
            id: torrent.id.clone(),
            name: torrent.name.clone(),
            magnet: torrent.resolved_magnet(),
            expected_bytes: torrent.size_bytes,
        }
    }

    pub fn from_magnet(magnet: &str) -> Result<Self> {
        let hash = magnet_hash(magnet)?;
        Ok(Self {
            id: hash.clone(),
            name: hash,
            magnet: magnet.to_owned(),
            expected_bytes: 0,
        })
    }
}

#[derive(Clone, Debug)]
pub struct DownloadLayout {
    pub root: PathBuf,
    pub incomplete: PathBuf,
    pub completed: PathBuf,
    pub state: PathBuf,
}

impl DownloadLayout {
    pub fn resolve(engine: &LibrqbitConfig, config: &DownloadsConfig) -> Result<Self> {
        let root = engine
            .download_dir_path()
            .context("download directory is not configured")?;
        let root = expand_home(&root)?;
        let resolve = |custom: &Option<PathBuf>, default: &str| -> Result<PathBuf> {
            let path = expand_home(custom.as_deref().unwrap_or(Path::new(default)))?;
            Ok(if path.is_absolute() {
                path
            } else {
                root.join(path)
            })
        };
        Ok(Self {
            incomplete: resolve(&config.incomplete_dir, "incomplete")?,
            completed: resolve(&config.completed_dir, "completed")?,
            state: root.join(".pirata"),
            root,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum Phase {
    Downloading,
    Promoting,
    Completed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Record {
    pub info_hash: String,
    pub id: String,
    pub name: String,
    pub magnet: String,
    pub phase: Phase,
    pub hook_done: bool,
    pub source: PathBuf,
    pub destination: PathBuf,
    #[serde(default)]
    pub files: Vec<PayloadFile>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct PayloadFile {
    pub path: PathBuf,
    pub len: u64,
}

impl Record {
    pub fn request(&self) -> DownloadRequest {
        DownloadRequest {
            id: self.id.clone(),
            name: self.name.clone(),
            magnet: self.magnet.clone(),
            expected_bytes: 0,
        }
    }
}

pub(super) struct Store {
    pub layout: DownloadLayout,
    _lock: File,
}

impl Drop for Store {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self._lock);
    }
}

impl Store {
    pub fn open(engine: &LibrqbitConfig, config: &DownloadsConfig) -> Result<Self> {
        if config.min_free_space_percent >= 100 {
            bail!("downloads.min_free_space_percent must be less than 100");
        }
        if config.hook_timeout_seconds == 0 {
            bail!("downloads.hook_timeout_seconds must be greater than zero");
        }
        if config
            .on_complete
            .first()
            .is_some_and(|program| program.trim().is_empty())
        {
            bail!("downloads.on_complete must start with an executable path");
        }
        let mut layout = DownloadLayout::resolve(engine, config)?;
        for path in [
            &layout.root,
            &layout.incomplete,
            &layout.completed,
            &layout.state,
        ] {
            fs::create_dir_all(path)
                .with_context(|| format!("cannot create {}", path.display()))?;
        }
        layout.root = layout.root.canonicalize()?;
        layout.incomplete = layout.incomplete.canonicalize()?;
        layout.completed = layout.completed.canonicalize()?;
        layout.state = layout.state.canonicalize()?;
        for (a, b) in [
            (&layout.incomplete, &layout.completed),
            (&layout.incomplete, &layout.state),
            (&layout.completed, &layout.state),
        ] {
            if a.starts_with(b) || b.starts_with(a) {
                bail!(
                    "incomplete, completed, and state directories must be separate, non-nested folders"
                );
            }
        }
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(layout.state.join("session.lock"))?;
        lock.try_lock_exclusive()
            .context("another Pirata instance is managing this download directory")?;
        let probe = tempfile::NamedTempFile::new_in(&layout.incomplete)?;
        let destination = tempfile::tempdir_in(&layout.completed)?;
        fs::hard_link(probe.path(), destination.path().join("probe")).context(
            "incomplete and completed directories must support hardlinks on the same filesystem",
        )?;
        Ok(Self {
            layout,
            _lock: lock,
        })
    }

    pub fn source(&self, record: &Record) -> PathBuf {
        record.source.clone()
    }

    pub fn destination(&self, record: &Record) -> PathBuf {
        record.destination.clone()
    }

    pub fn begin(&self, request: &DownloadRequest) -> Result<Record> {
        let info_hash = magnet_hash(&request.magnet)?;
        let path = self.record_path(&info_hash);
        if path.try_exists()? {
            return self.read(&path);
        }
        let source = self.layout.incomplete.join(&info_hash);
        let destination =
            self.layout
                .completed
                .join(format!("{} [{}]", safe_name(&request.name), info_hash));
        let record = Record {
            info_hash,
            id: request.id.clone(),
            name: request.name.clone(),
            magnet: request.magnet.clone(),
            phase: Phase::Downloading,
            hook_done: false,
            files: Vec::new(),
            source,
            destination,
        };
        if self.source(&record).try_exists()? || self.destination(&record).try_exists()? {
            bail!(
                "download path already exists without a matching Pirata record; existing files were left untouched"
            );
        }
        fs::create_dir(self.source(&record))?;
        if let Err(error) = self.save(&record) {
            // This call exclusively created an empty directory; never remove payload recursively.
            let _ = fs::remove_dir(self.source(&record));
            return Err(error)
                .context("could not save initial download state; no transfer was started");
        }
        Ok(record)
    }

    pub fn records(&self) -> Result<Vec<Record>> {
        let mut records = Vec::new();
        for entry in fs::read_dir(&self.layout.state)? {
            let path = entry?.path();
            if path.extension().is_some_and(|ext| ext == "json") {
                records.push(self.read(&path)?);
            }
        }
        records.sort_by(|a, b| a.info_hash.cmp(&b.info_hash));
        Ok(records)
    }

    fn read(&self, path: &Path) -> Result<Record> {
        let record: Record = serde_json::from_reader(File::open(path)?)
            .with_context(|| format!("cannot read download record {}", path.display()))?;
        if magnet_hash(&record.magnet)? != record.info_hash
            || path != self.record_path(&record.info_hash)
        {
            bail!(
                "download record has an invalid torrent identity: {}",
                path.display()
            );
        }
        if !record.source.is_absolute()
            || !record.destination.is_absolute()
            || record
                .source
                .file_name()
                .is_none_or(|name| name != record.info_hash.as_str())
            || record.source.starts_with(&record.destination)
            || record.destination.starts_with(&record.source)
        {
            bail!(
                "download record has invalid or overlapping saved paths: {}",
                path.display()
            );
        }
        Ok(record)
    }

    fn record_path(&self, hash: &str) -> PathBuf {
        self.layout.state.join(format!("{hash}.json"))
    }

    pub fn save(&self, record: &Record) -> Result<()> {
        let mut file = tempfile::NamedTempFile::new_in(&self.layout.state)?;
        serde_json::to_writer_pretty(&mut file, record)?;
        file.as_file().sync_all()?;
        file.persist(self.record_path(&record.info_hash))?;
        sync_directory(&self.layout.state)?;
        Ok(())
    }

    pub fn torrent_path(&self, record: &Record) -> PathBuf {
        self.layout
            .state
            .join(format!("{}.torrent", record.info_hash))
    }

    pub fn save_torrent(&self, record: &Record, bytes: &[u8]) -> Result<()> {
        use std::io::Write;
        let mut file = tempfile::NamedTempFile::new_in(&self.layout.state)?;
        file.write_all(bytes)?;
        file.as_file().sync_all()?;
        file.persist(self.torrent_path(record))?;
        sync_directory(&self.layout.state)?;
        Ok(())
    }

    pub fn promote(&self, record: &mut Record) -> Result<PathBuf> {
        // The manager must verify completion and detach the torrent before promotion.
        if record.phase == Phase::Downloading && self.destination(record).try_exists()? {
            bail!("completed download path already exists; existing files were left untouched");
        }
        record.phase = Phase::Promoting;
        self.save(record)?;
        let destination = self.destination(record);
        match fs::create_dir(&destination) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
        ensure_directory(&destination)?;
        link_tree(&self.source(record), &destination)?;
        sync_directory(
            destination
                .parent()
                .context("completed path has no parent")?,
        )?;
        Ok(destination)
    }

    pub fn complete(&self, record: &mut Record) -> Result<()> {
        record.phase = Phase::Completed;
        self.save(record)?;
        self.cleanup_source(record)
    }

    pub fn cleanup_source(&self, record: &Record) -> Result<()> {
        let source = self.source(record);
        if source.try_exists()? {
            verify_links(&source, &self.destination(record))?;
            fs::remove_dir_all(&source)?;
            sync_directory(source.parent().context("incomplete path has no parent")?)?;
        }
        Ok(())
    }

    pub fn validate_payload(&self, record: &Record, folder: &Path, completed: bool) -> Result<()> {
        use std::path::Component;
        ensure_directory(folder)?;
        if completed && record.files.is_empty() {
            bail!("completed download has no saved file manifest");
        }
        for file in &record.files {
            let components = file.path.components().collect::<Vec<_>>();
            if components.is_empty()
                || components
                    .iter()
                    .any(|part| !matches!(part, Component::Normal(_)))
            {
                bail!("invalid file path in saved torrent metadata");
            }
            let mut path = folder.to_owned();
            for (index, component) in components.iter().enumerate() {
                path.push(component);
                match fs::symlink_metadata(&path) {
                    Ok(metadata) => {
                        let last = index + 1 == components.len();
                        if metadata.file_type().is_symlink()
                            || (!last && !metadata.is_dir())
                            || (last && !metadata.is_file())
                        {
                            bail!("unsafe file path in download: {}", path.display());
                        }
                        if last && completed && metadata.len() != file.len {
                            bail!("completed file was changed: {}", path.display());
                        }
                    }
                    Err(error) if !completed && error.kind() == std::io::ErrorKind::NotFound => {
                        break;
                    }
                    Err(error) => {
                        return Err(error).with_context(|| {
                            format!(
                                "completed file is missing or unreadable: {}",
                                path.display()
                            )
                        });
                    }
                }
            }
        }
        Ok(())
    }
}

pub(super) fn magnet_hash(magnet: &str) -> Result<String> {
    Magnet::parse(magnet)?
        .as_id20()
        .map(|hash| hash.as_string())
        .context("magnet must contain a BitTorrent v1 info hash")
}

pub(super) fn expand_home(path: &Path) -> Result<PathBuf> {
    if let Ok(remainder) = path.strip_prefix("~") {
        let home = directories::BaseDirs::new().context("cannot determine home directory")?;
        return Ok(home.home_dir().join(remainder));
    }
    Ok(path.to_path_buf())
}

pub(super) fn safe_name(name: &str) -> String {
    let mut result = String::new();
    for character in name.chars() {
        if result.len() + character.len_utf8() > 120 {
            break;
        }
        result.push(
            if character.is_control() || "<>:\"/\\|?*".contains(character) {
                '_'
            } else {
                character
            },
        );
    }
    let trimmed = result.trim_matches([' ', '.']);
    if trimmed.is_empty() {
        "download".to_string()
    } else {
        trimmed.to_string()
    }
}

fn link_tree(source: &Path, destination: &Path) -> Result<()> {
    ensure_directory(source)?;
    ensure_directory(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_dir() {
            match fs::create_dir(&target) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.into()),
            }
            link_tree(&entry.path(), &target)?;
        } else if kind.is_file() {
            File::open(entry.path())?.sync_all()?;
            match fs::hard_link(entry.path(), &target) {
                Ok(()) => {}
                Err(error)
                    if error.kind() == std::io::ErrorKind::AlreadyExists
                        && same_file::is_same_file(entry.path(), &target)? => {}
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!(
                            "cannot promote {} without replacing existing files",
                            target.display()
                        )
                    });
                }
            }
        } else {
            bail!(
                "cannot promote non-regular download file {}",
                entry.path().display()
            );
        }
    }
    sync_directory(destination)
}

fn verify_links(source: &Path, destination: &Path) -> Result<()> {
    ensure_directory(source)?;
    ensure_directory(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            verify_links(&entry.path(), &target)?;
        } else if !entry.file_type()?.is_file() || !same_file::is_same_file(entry.path(), &target)?
        {
            bail!(
                "incomplete files were retained because the completed copy differs: {}",
                entry.path().display()
            );
        }
    }
    Ok(())
}

pub(super) fn ensure_directory(path: &Path) -> Result<()> {
    if !fs::symlink_metadata(path)?.file_type().is_dir() {
        bail!(
            "download path is not a regular directory: {}",
            path.display()
        );
    }
    Ok(())
}

fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
