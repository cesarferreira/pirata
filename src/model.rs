use std::fmt;
use std::path::PathBuf;

use clap::ValueEnum;
use serde::{Deserialize, Serialize};

use crate::util::encode_component;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum, Default)]
#[serde(rename_all = "lowercase")]
pub enum IndexerKind {
    #[default]
    Piratebay,
}

impl fmt::Display for IndexerKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Piratebay => write!(f, "piratebay"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum, Default)]
#[serde(rename_all = "lowercase")]
pub enum DownloaderKind {
    Transmission,
    Qbittorrent,
    #[default]
    Aria2,
    System,
}

impl fmt::Display for DownloaderKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transmission => write!(f, "transmission"),
            Self::Qbittorrent => write!(f, "qbittorrent"),
            Self::Aria2 => write!(f, "aria2"),
            Self::System => write!(f, "system"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum, Default)]
#[serde(rename_all = "lowercase")]
pub enum SearchSort {
    #[default]
    Seeders,
    Leechers,
    Size,
    Name,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Torrent {
    pub id: String,
    pub name: String,
    pub info_hash: String,
    pub magnet: Option<String>,
    pub seeders: u32,
    pub leechers: u32,
    pub size_bytes: u64,
    pub status: Option<String>,
    pub uploaded_by: Option<String>,
    pub description: Option<String>,
    pub category: Option<String>,
    pub subcategory: Option<String>,
    pub added: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrackedDownload {
    pub info_hash: String,
    pub name: String,
    pub target_path: PathBuf,
    pub downloader: DownloaderKind,
    pub percent_done: u8,
    pub completed: bool,
}

impl Torrent {
    pub fn resolved_magnet(&self) -> String {
        self.magnet
            .clone()
            .unwrap_or_else(|| build_magnet_link(&self.info_hash, &self.name))
    }

    pub fn normalized_status(&self) -> Option<String> {
        self.status
            .as_ref()
            .map(|value| value.trim().to_lowercase())
    }

    pub fn is_trusted(&self) -> bool {
        matches!(
            self.normalized_status().as_deref(),
            Some("vip" | "trusted" | "helper" | "moderator" | "supermod")
        )
    }
}

pub fn build_magnet_link(info_hash: &str, name: &str) -> String {
    let mut magnet = format!(
        "magnet:?xt=urn:btih:{}&dn={}",
        info_hash,
        encode_component(name)
    );

    for tracker in DEFAULT_PUBLIC_TRACKERS {
        magnet.push_str("&tr=");
        magnet.push_str(&encode_component(tracker));
    }

    magnet
}

const DEFAULT_PUBLIC_TRACKERS: &[&str] = &[
    "udp://tracker.opentrackr.org:1337/announce",
    "udp://open.stealth.si:80/announce",
    "udp://tracker.torrent.eu.org:451/announce",
    "udp://explodie.org:6969/announce",
    "https://tracker.opentrackr.org:443/announce",
];

#[cfg(test)]
mod tests {
    use super::{Torrent, build_magnet_link};

    #[test]
    fn builds_fallback_magnet() {
        let torrent = Torrent {
            id: "1".into(),
            name: "ubuntu iso".into(),
            info_hash: "ABCDEF".into(),
            magnet: None,
            seeders: 1,
            leechers: 2,
            size_bytes: 100,
            status: None,
            uploaded_by: None,
            description: None,
            category: None,
            subcategory: None,
            added: None,
        };

        assert_eq!(
            torrent.resolved_magnet(),
            "magnet:?xt=urn:btih:ABCDEF&dn=ubuntu%20iso&tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337%2Fannounce&tr=udp%3A%2F%2Fopen.stealth.si%3A80%2Fannounce&tr=udp%3A%2F%2Ftracker.torrent.eu.org%3A451%2Fannounce&tr=udp%3A%2F%2Fexplodie.org%3A6969%2Fannounce&tr=https%3A%2F%2Ftracker.opentrackr.org%3A443%2Fannounce"
        );
        assert_eq!(
            build_magnet_link("ABCDEF", "ubuntu iso"),
            torrent.resolved_magnet()
        );
    }
}
