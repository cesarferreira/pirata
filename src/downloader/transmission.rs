use std::io::{BufReader, Read};
use std::process::Stdio;
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use reqwest::{Client, StatusCode, Url};
use serde::Serialize;
use tokio::process::Command;

use crate::config::{TransmissionClient, TransmissionConfig};
use crate::downloader::Downloader;
use crate::model::Torrent;
use crate::util::ensure_transmission_cli_available;

#[derive(Debug, Clone)]
pub struct TransmissionDownloader {
    client: Client,
    config: TransmissionConfig,
}

impl TransmissionDownloader {
    pub fn new(config: TransmissionConfig) -> Result<Self> {
        let client = Client::builder()
            .user_agent("pirate-ctl/0.1")
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

    fn spawn_via_standalone_cli(&self, magnet: &str) -> Result<u32> {
        ensure_transmission_cli_available()?;

        let mut command = std::process::Command::new("transmission-cli");
        command.stdin(Stdio::null());
        command.stdout(Stdio::null());
        command.stderr(Stdio::null());

        if let Some(download_dir) = &self.config.download_dir {
            command.arg("-w").arg(download_dir);
        }
        command.arg(magnet);

        let child = command
            .spawn()
            .context("failed to start standalone transmission-cli fallback")?;
        Ok(child.id())
    }

    fn run_via_standalone_cli_foreground(&self, magnet: &str) -> Result<()> {
        ensure_transmission_cli_available()?;

        let mut command = self.foreground_transmission_command(magnet);
        println!("Starting transmission-cli...");

        let mut child = command
            .spawn()
            .context("failed to run transmission-cli")?;
        let stdout = child
            .stdout
            .take()
            .context("failed to capture transmission-cli stdout")?;
        let stderr = child
            .stderr
            .take()
            .context("failed to capture transmission-cli stderr")?;

        let (sender, receiver) = mpsc::channel();
        spawn_cli_reader(stdout, sender.clone());
        spawn_cli_reader(stderr, sender);

        let mut stopped_after_completion = false;
        let mut last_line = String::new();
        let mut last_visible_update = Instant::now();
        let started_at = Instant::now();
        let mut saw_progress_output = false;

        loop {
            while let Ok(line) = receiver.try_recv() {
                match classify_cli_line(&line) {
                    CliLine::Ignore => {}
                    CliLine::Display(rendered) => {
                        saw_progress_output = true;
                        if rendered != last_line {
                            println!("{rendered}");
                            last_line = rendered;
                            last_visible_update = Instant::now();
                        }
                    }
                    CliLine::Complete(rendered) => {
                        println!("{rendered}");
                        stopped_after_completion = true;
                    }
                }
            }

            if stopped_after_completion {
                let _ = child.kill();
                let _ = child.wait();
                return Ok(());
            }

            if let Some(status) = child
                .try_wait()
                .context("failed to read transmission-cli status")?
            {
                return if status.success() {
                    Ok(())
                } else {
                    bail!("transmission-cli exited with status {status}")
                };
            }

            match receiver.recv_timeout(Duration::from_millis(120)) {
                Ok(line) => match classify_cli_line(&line) {
                    CliLine::Ignore => {}
                    CliLine::Display(rendered) => {
                        saw_progress_output = true;
                        if rendered != last_line {
                            println!("{rendered}");
                            last_line = rendered;
                            last_visible_update = Instant::now();
                        }
                    }
                    CliLine::Complete(rendered) => {
                        println!("{rendered}");
                        stopped_after_completion = true;
                    }
                },
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {}
            }

            if last_visible_update.elapsed() >= Duration::from_secs(2) && !stopped_after_completion {
                let message = if saw_progress_output {
                    format!("Still downloading... {}s elapsed", started_at.elapsed().as_secs())
                } else {
                    format!("Connecting to peers... {}s elapsed", started_at.elapsed().as_secs())
                };
                if message != last_line {
                    println!("{message}");
                    last_line = message;
                }
                last_visible_update = Instant::now();
            }
        }
    }

    fn foreground_transmission_command(&self, magnet: &str) -> std::process::Command {
        if cfg!(target_os = "macos") {
            let mut command = std::process::Command::new("script");
            command.stdin(Stdio::null());
            command.stdout(Stdio::piped());
            command.stderr(Stdio::piped());
            command.arg("-q").arg("/dev/null").arg("transmission-cli");
            if let Some(download_dir) = &self.config.download_dir {
                command.arg("-w").arg(download_dir);
            }
            command.arg(magnet);
            command
        } else {
            let mut command = std::process::Command::new("script");
            command.stdin(Stdio::null());
            command.stdout(Stdio::piped());
            command.stderr(Stdio::piped());
            command.arg("-q").arg("-e").arg("-f").arg("-c");
            command.arg(self.linux_transmission_cli_shell_command(magnet));
            command.arg("/dev/null");
            command
        }
    }

    fn linux_transmission_cli_shell_command(&self, magnet: &str) -> String {
        let mut parts = vec!["transmission-cli".to_string()];
        if let Some(download_dir) = &self.config.download_dir {
            parts.push("-w".to_string());
            parts.push(shell_quote(download_dir));
        }
        parts.push(shell_quote(magnet));
        parts.join(" ")
    }
}

#[async_trait]
impl Downloader for TransmissionDownloader {
    async fn add_torrent(&self, torrent: &Torrent) -> Result<()> {
        let magnet = torrent.resolved_magnet();

        match self.config.client {
            TransmissionClient::Cli => self.run_via_standalone_cli_foreground(&magnet),
            TransmissionClient::Rpc => match self.add_via_rpc(&magnet).await {
                Ok(()) => Ok(()),
                Err(rpc_error) => self.add_via_cli(&magnet).await.map_err(|remote_error| {
                    anyhow!(
                        "Transmission RPC failed: {rpc_error}; transmission-remote fallback also failed: {remote_error}"
                    )
                }),
            },
            TransmissionClient::Auto => match self.run_via_standalone_cli_foreground(&magnet) {
                Ok(()) => Ok(()),
                Err(cli_error) => match self.add_via_rpc(&magnet).await {
                    Ok(()) => Ok(()),
                    Err(rpc_error) => self.add_via_cli(&magnet).await.map_err(|remote_error| {
                        anyhow!(
                            "standalone transmission-cli failed: {cli_error}; Transmission RPC also failed: {rpc_error}; transmission-remote fallback also failed: {remote_error}"
                        )
                    }),
                },
            },
        }
    }

    async fn add_magnet(&self, magnet: &str) -> Result<()> {
        match self.config.client {
            TransmissionClient::Cli => self.spawn_via_standalone_cli(magnet).map(|_| ()),
            TransmissionClient::Rpc => match self.add_via_rpc(magnet).await {
                Ok(()) => Ok(()),
                Err(rpc_error) => self.add_via_cli(magnet).await.map_err(|remote_error| {
                    anyhow!(
                        "Transmission RPC failed: {rpc_error}; transmission-remote fallback also failed: {remote_error}"
                    )
                }),
            },
            TransmissionClient::Auto => match self.spawn_via_standalone_cli(magnet) {
                Ok(_) => Ok(()),
                Err(cli_error) => match self.add_via_rpc(magnet).await {
                    Ok(()) => Ok(()),
                    Err(rpc_error) => self.add_via_cli(magnet).await.map_err(|remote_error| {
                        anyhow!(
                            "standalone transmission-cli failed: {cli_error}; Transmission RPC also failed: {rpc_error}; transmission-remote fallback also failed: {remote_error}"
                        )
                    }),
                },
            },
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

enum CliLine {
    Ignore,
    Display(String),
    Complete(String),
}

fn spawn_cli_reader<R>(stream: R, sender: Sender<String>)
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        let mut buffer = Vec::new();
        let mut byte = [0_u8; 1];

        loop {
            match reader.read(&mut byte) {
                Ok(0) => {
                    emit_cli_buffer(&buffer, &sender);
                    break;
                }
                Ok(_) => match byte[0] {
                    b'\n' | b'\r' => {
                        emit_cli_buffer(&buffer, &sender);
                        buffer.clear();
                    }
                    value => buffer.push(value),
                },
                Err(_) => break,
            }
        }
    });
}

fn emit_cli_buffer(buffer: &[u8], sender: &Sender<String>) {
    if buffer.is_empty() {
        return;
    }

    let text = String::from_utf8_lossy(buffer);
    let sanitized: String = text
        .chars()
        .filter(|character| !character.is_control() || matches!(character, '\n' | '\r' | '\t'))
        .collect();
    let trimmed = sanitized.trim();
    if trimmed.is_empty() {
        return;
    }

    let _ = sender.send(trimmed.to_string());
}

fn classify_cli_line(line: &str) -> CliLine {
    let trimmed = line.trim();
    if trimmed.is_empty() || is_transmission_noise(trimmed) {
        return CliLine::Ignore;
    }

    if is_completion_marker(trimmed) {
        return CliLine::Complete("Download complete. Stopping before seeding.".to_string());
    }

    if let Some(cleaned) = cleaned_progress_line(trimmed) {
        return CliLine::Display(cleaned);
    }

    CliLine::Display(trimmed.to_string())
}

fn is_transmission_noise(line: &str) -> bool {
    line.starts_with('[')
        && [
            "web.cc:",
            "rpc-server.cc:",
            "session.cc:",
            "net.cc:",
            "port-forwarding.cc:",
            "tr-udp.cc:",
        ]
        .iter()
        .any(|needle| line.contains(needle))
}

fn is_completion_marker(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains("pausing torrent") || lower.starts_with("seeding,")
}

fn cleaned_progress_line(line: &str) -> Option<String> {
    if let Some(index) = line.find("Progress:") {
        return Some(line[index..].to_string());
    }

    if line.starts_with("Downloading")
        || line.starts_with("Verifying")
        || line.starts_with("Magnetized")
        || line.starts_with("Got")
        || line.starts_with("Seeding,")
    {
        return Some(line.to_string());
    }

    None
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

#[cfg(test)]
mod tests {
    use super::{
        CliLine, classify_cli_line, is_completion_marker, is_transmission_noise, shell_quote,
    };

    #[test]
    fn filters_internal_transmission_logs() {
        assert!(is_transmission_noise(
            "[2026-03-27 18:04:12.262] rpc-server.cc:923: Serving RPC and Web requests"
        ));
        assert!(!is_transmission_noise("Progress: 12.5%"));
    }

    #[test]
    fn detects_completion_markers() {
        assert!(is_completion_marker(
            "[2026-03-27 18:04:37.267] Clinton D.: Pausing torrent"
        ));
        assert!(is_completion_marker(
            "Seeding, uploading to 0 of 1 peer(s), 0 kB/s [0.00]"
        ));
    }

    #[test]
    fn classifies_progress_lines() {
        match classify_cli_line("Progress: 12.5%") {
            CliLine::Display(line) => assert_eq!(line, "Progress: 12.5%"),
            _ => panic!("expected display line"),
        }
    }

    #[test]
    fn quotes_shell_arguments() {
        assert_eq!(shell_quote("abc"), "'abc'");
        assert_eq!(shell_quote("a'b"), "'a'\"'\"'b'");
    }
}
