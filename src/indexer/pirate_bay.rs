use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use scraper::{Html, Selector};
use serde::Deserialize;
use tokio::process::Command;

use crate::indexer::Indexer;
use crate::model::Torrent;
use crate::util::{
    command_exists, deserialize_optional_string, deserialize_string_from_any,
    deserialize_u32_from_any, deserialize_u64_from_any,
};

const API_BASE: &str = "https://apibay.org";
const HTML_BASE: &str = "https://thepiratebay.org";
const TRENDING_TODAY_PATH: &str = "/precompiled/data_top100_recent.json";

#[derive(Debug, Clone)]
pub struct PirateBayIndexer {
    client: Client,
}

impl PirateBayIndexer {
    pub fn new() -> Result<Self> {
        let client = Client::builder()
            .user_agent("pirata/0.1")
            .build()
            .context("failed to build HTTP client")?;
        Ok(Self { client })
    }

    async fn fetch_magnet_from_html(&self, id: &str) -> Result<Option<String>> {
        let response = self
            .client
            .get(format!("{HTML_BASE}/description.php"))
            .query(&[("id", id)])
            .send()
            .await?;
        if !response.status().is_success() {
            return Ok(None);
        }

        let body = response.text().await?;
        let document = Html::parse_document(&body);
        let selector = Selector::parse("a[href^=\"magnet:\"]").expect("valid selector");
        Ok(document
            .select(&selector)
            .find_map(|element| element.value().attr("href"))
            .map(ToOwned::to_owned))
    }

    async fn fetch_magnet_via_cli(&self, id: &str) -> Result<Option<String>> {
        if !command_exists("piratebay") {
            return Ok(None);
        }

        let output = Command::new("piratebay")
            .arg("--json")
            .arg("info")
            .arg(id)
            .output()
            .await
            .context("failed to run piratebay CLI for torrent info")?;

        if !output.status.success() {
            return Ok(None);
        }

        let value: serde_json::Value =
            serde_json::from_slice(&output.stdout).context("failed to parse piratebay CLI JSON")?;

        Ok(value
            .get("magnet")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(ToOwned::to_owned))
    }
}

#[async_trait]
impl Indexer for PirateBayIndexer {
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<Torrent>> {
        let response = self
            .client
            .get(format!("{API_BASE}/q.php"))
            .query(&[("q", query)])
            .send()
            .await?
            .error_for_status()?;
        let items: Vec<ApiTorrent> = response.json().await?;

        Ok(items
            .into_iter()
            .filter(|item| {
                item.id != "0"
                    && item
                        .info_hash
                        .as_deref()
                        .is_some_and(|hash| !hash.trim().is_empty())
            })
            .map(Torrent::from)
            .take(limit)
            .collect())
    }

    async fn trending(&self, limit: usize) -> Result<Vec<Torrent>> {
        let response = self
            .client
            .get(format!("{API_BASE}{TRENDING_TODAY_PATH}"))
            .send()
            .await?
            .error_for_status()?;
        let items: Vec<ApiTorrent> = response.json().await?;

        Ok(items
            .into_iter()
            .filter(|item| {
                item.id != "0"
                    && item
                        .info_hash
                        .as_deref()
                        .is_some_and(|hash| !hash.trim().is_empty())
            })
            .map(Torrent::from)
            .take(limit)
            .collect())
    }

    async fn info(&self, id: &str) -> Result<Torrent> {
        let response = self
            .client
            .get(format!("{API_BASE}/t.php"))
            .query(&[("id", id)])
            .send()
            .await?
            .error_for_status()?;
        let item: ApiTorrent = response.json().await?;
        let mut torrent = Torrent::from(item);
        if torrent.magnet.is_none() {
            torrent.magnet = self.fetch_magnet_via_cli(id).await?;
        }
        if torrent.magnet.is_none() {
            torrent.magnet = self.fetch_magnet_from_html(id).await?;
        }
        Ok(torrent)
    }
}

#[derive(Debug, Deserialize)]
struct ApiTorrent {
    #[serde(deserialize_with = "deserialize_string_from_any")]
    pub id: String,
    pub name: String,
    #[serde(default, deserialize_with = "deserialize_optional_string")]
    pub info_hash: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_string")]
    pub magnet: Option<String>,
    #[serde(deserialize_with = "deserialize_u32_from_any")]
    pub seeders: u32,
    #[serde(deserialize_with = "deserialize_u32_from_any")]
    pub leechers: u32,
    #[serde(rename = "size", deserialize_with = "deserialize_u64_from_any")]
    pub size_bytes: u64,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub descr: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_string")]
    pub category: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_string")]
    pub subcategory: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_u64")]
    pub added: Option<u64>,
}

impl From<ApiTorrent> for Torrent {
    fn from(value: ApiTorrent) -> Self {
        Self {
            id: value.id,
            name: value.name,
            info_hash: value.info_hash.unwrap_or_default(),
            magnet: value.magnet,
            seeders: value.seeders,
            leechers: value.leechers,
            size_bytes: value.size_bytes,
            status: value.status.filter(|item| !item.trim().is_empty()),
            uploaded_by: value.username.filter(|item| !item.trim().is_empty()),
            description: value.descr.filter(|item| !item.trim().is_empty()),
            category: value.category.filter(|item| !item.trim().is_empty()),
            subcategory: value.subcategory.filter(|item| !item.trim().is_empty()),
            added: value.added,
        }
    }
}

fn deserialize_optional_u64<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    let Some(value) = value else {
        return Ok(None);
    };

    match value {
        serde_json::Value::String(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                Ok(None)
            } else {
                trimmed.parse().map(Some).map_err(serde::de::Error::custom)
            }
        }
        serde_json::Value::Number(number) => number
            .as_u64()
            .ok_or_else(|| serde::de::Error::custom("invalid added field"))
            .map(Some),
        _ => Err(serde::de::Error::custom("invalid added field")),
    }
}

#[cfg(test)]
mod tests {
    use super::ApiTorrent;

    #[test]
    fn parses_trending_today_payload() {
        let item: ApiTorrent = serde_json::from_str(
            r#"{
                "id": 84216239,
                "info_hash": "15B75A030D1A69E906488C74C7B05D71C375B092",
                "category": 208,
                "name": "Example release",
                "status": "vip",
                "size": 14463932136,
                "seeders": 1,
                "leechers": 1,
                "username": "EXTRG",
                "added": 1787577904
            }"#,
        )
        .expect("trending payload should deserialize");

        assert_eq!(item.id, "84216239");
        assert_eq!(item.name, "Example release");
        assert_eq!(item.seeders, 1);
        assert_eq!(item.size_bytes, 14_463_932_136);
    }
}
