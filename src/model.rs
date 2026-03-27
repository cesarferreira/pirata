use std::fmt;

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
    #[default]
    Transmission,
    Qbittorrent,
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
    format!(
        "magnet:?xt=urn:btih:{}&dn={}",
        info_hash,
        encode_component(name)
    )
}

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
            "magnet:?xt=urn:btih:ABCDEF&dn=ubuntu%20iso"
        );
        assert_eq!(
            build_magnet_link("ABCDEF", "ubuntu iso"),
            torrent.resolved_magnet()
        );
    }
}
