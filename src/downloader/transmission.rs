use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use reqwest::{Client, StatusCode, Url};
use serde::Serialize;
use tokio::process::Command;

use crate::config::TransmissionConfig;
use crate::downloader::Downloader;

#[derive(Debug, Clone)]
pub struct TransmissionDownloader {
    client: Client,
    config: TransmissionConfig,
}

impl TransmissionDownloader {
    pub fn new(config: TransmissionConfig) -> Result<Self> {
        let client = Client::builder()
            .user_agent("pirata/0.1")
            .timeout(rpc_timeout())
            .build()
            .context("failed to build HTTP client")?;
        Ok(Self { client, config })
    }

    async fn add_via_rpc(&self, magnet: &str) -> Result<()> {
        let payload = TorrentAddRequest::new(magnet, self.config.download_dir.clone());
        let response = self.send_rpc(None, &payload).await?;
        if response.status() == StatusCode::CONFLICT {
            let session_id = response
                .headers()
                .get("x-transmission-session-id")
                .context("Transmission RPC requires a session id but did not provide one")?
                .to_str()
                .context("invalid x-transmission-session-id header")?
                .to_string();
            let retry = self.send_rpc(Some(session_id), &payload).await?;
            Self::assert_success(retry).await
        } else {
            Self::assert_success(response).await
        }
    }

    async fn send_rpc(
        &self,
        session_id: Option<String>,
        payload: &TorrentAddRequest<'_>,
    ) -> Result<reqwest::Response> {
        let mut request = self.client.post(&self.config.rpc_url).json(payload);
        if let Some(session_id) = session_id {
            request = request.header("x-transmission-session-id", session_id);
        }
        if let Some(username) = &self.config.username {
            request = request.basic_auth(username, self.config.password.as_ref());
        }
        Ok(request.send().await?)
    }

    async fn assert_success(response: reqwest::Response) -> Result<()> {
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("Transmission RPC returned {status}: {body}");
        }

        let body: RpcResponse = response.json().await?;
        if body.result.eq_ignore_ascii_case("success") {
            Ok(())
        } else {
            Err(anyhow!("Transmission RPC error: {}", body.result))
        }
    }

    async fn add_via_cli(&self, magnet: &str) -> Result<()> {
        let url = Url::parse(&self.config.rpc_url)
            .with_context(|| format!("invalid Transmission rpc_url: {}", self.config.rpc_url))?;
        let host = url
            .host_str()
            .context("Transmission rpc_url is missing a host")?;
        let port = url
            .port_or_known_default()
            .context("Transmission rpc_url is missing a port")?;

        let mut command = Command::new("transmission-remote");
        command.arg(format!("{host}:{port}"));
        if let Some(username) = &self.config.username {
            let password = self.config.password.clone().unwrap_or_default();
            command.arg("-n").arg(format!("{username}:{password}"));
        }
        if let Some(download_dir) = &self.config.download_dir {
            command.arg("-w").arg(download_dir);
        }
        command.arg("-a").arg(magnet);

        let output = command.output().await.context(
            "failed to start transmission-remote fallback; install Transmission CLI or configure RPC",
        )?;
        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            bail!(
                "transmission-remote failed with status {}: {}{}",
                output.status,
                stdout,
                stderr
            );
        }
    }
}

fn rpc_timeout() -> Duration {
    Duration::from_secs(2)
}

#[async_trait]
impl Downloader for TransmissionDownloader {
    async fn add_magnet(&self, magnet: &str) -> Result<()> {
        match self.add_via_rpc(magnet).await {
            Ok(()) => Ok(()),
            Err(rpc_error) => self
                .add_via_cli(magnet)
                .await
                .map_err(|cli_error| anyhow!("{rpc_error}; fallback also failed: {cli_error}")),
        }
    }
}

#[derive(Debug, Serialize)]
struct TorrentAddRequest<'a> {
    method: &'static str,
    arguments: TorrentAddArguments<'a>,
}

impl<'a> TorrentAddRequest<'a> {
    fn new(filename: &'a str, download_dir: Option<String>) -> Self {
        Self {
            method: "torrent-add",
            arguments: TorrentAddArguments {
                filename,
                download_dir,
            },
        }
    }
}

#[derive(Debug, Serialize)]
struct TorrentAddArguments<'a> {
    filename: &'a str,
    #[serde(rename = "download-dir", skip_serializing_if = "Option::is_none")]
    download_dir: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct RpcResponse {
    result: String,
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::{net::TcpListener, time::timeout};

    use super::TransmissionDownloader;
    use crate::config::TransmissionConfig;

    #[tokio::test]
    async fn rpc_attempt_times_out_quickly_when_server_never_responds() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let address = listener.local_addr().expect("listener address");
        tokio::spawn(async move {
            let _ = listener.accept().await;
            tokio::time::sleep(Duration::from_secs(10)).await;
        });

        let downloader = TransmissionDownloader::new(TransmissionConfig {
            rpc_url: format!("http://{address}/transmission/rpc"),
            username: None,
            password: None,
            download_dir: None,
        })
        .expect("downloader");

        let result = timeout(
            Duration::from_secs(3),
            downloader.add_via_rpc("magnet:?xt=urn:btih:0123456789ABCDEF"),
        )
        .await;

        assert!(result.is_ok(), "rpc add attempt exceeded outer timeout");
    }
}
