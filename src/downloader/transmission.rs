use std::io::{self, BufReader, Read, Write};
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

    fn run_via_standalone_cli_foreground(&self, torrent: &Torrent, magnet: &str) -> Result<()> {
        ensure_transmission_cli_available()?;

        let mut command = self.foreground_transmission_command(magnet);
        let mut active_status_line = false;
        println!(
            "Downloading '{}' to {} | listed seeders {} | listed leechers {}",
            torrent.name,
            self.config.download_target_display(),
            torrent.seeders,
            torrent.leechers
        );
        render_status_line("Starting transmission-cli...", &mut active_status_line)?;

        let mut child = command.spawn().context("failed to run transmission-cli")?;
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
        let mut last_progress_line: Option<String> = None;

        loop {
            while let Ok(line) = receiver.try_recv() {
                match classify_cli_line(&line) {
                    CliLine::Ignore => {}
                    CliLine::Display(rendered) => {
                        last_progress_line = Some(rendered.clone());
                        if rendered != last_line {
                            render_status_line(&rendered, &mut active_status_line)?;
                            last_line = rendered;
                            last_visible_update = Instant::now();
                        }
                    }
                    CliLine::Complete(rendered) => {
                        finish_status_line(&mut active_status_line)?;
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
                finish_status_line(&mut active_status_line)?;
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
                        last_progress_line = Some(rendered.clone());
                        if rendered != last_line {
                            render_status_line(&rendered, &mut active_status_line)?;
                            last_line = rendered;
                            last_visible_update = Instant::now();
                        }
                    }
                    CliLine::Complete(rendered) => {
                        finish_status_line(&mut active_status_line)?;
                        println!("{rendered}");
                        stopped_after_completion = true;
                    }
                },
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {}
            }

            if last_visible_update.elapsed() >= Duration::from_secs(2) && !stopped_after_completion
            {
                let message = idle_status_line(
                    last_progress_line.as_deref(),
                    started_at.elapsed().as_secs(),
                );
                if message != last_line {
                    render_status_line(&message, &mut active_status_line)?;
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

fn rpc_timeout() -> Duration {
    Duration::from_secs(2)
}

#[async_trait]
impl Downloader for TransmissionDownloader {
    async fn add_torrent(&self, torrent: &Torrent) -> Result<()> {
        let magnet = torrent.resolved_magnet();

        match self.config.client {
            TransmissionClient::Cli => self.run_via_standalone_cli_foreground(torrent, &magnet),
            TransmissionClient::Rpc => match self.add_via_rpc(&magnet).await {
                Ok(()) => Ok(()),
                Err(rpc_error) => self.add_via_cli(&magnet).await.map_err(|remote_error| {
                    anyhow!(
                        "Transmission RPC failed: {rpc_error}; transmission-remote fallback also failed: {remote_error}"
                    )
                }),
            },
            TransmissionClient::Auto => match self.run_via_standalone_cli_foreground(torrent, &magnet) {
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
    line.starts_with("transmission-cli ")
        || line == "^D"
        || line.starts_with("^D")
        || line.starts_with('[')
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
        return Some(format_progress_line(&line[index..]));
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

fn matches_zero_peers(line: Option<&str>) -> bool {
    let Some(line) = line else {
        return false;
    };
    line.contains("dl from 0 of 0 peer")
        || line.contains("dl from 0 of 0 peers")
        || line.contains("peers 0 of 0 peer")
        || line.contains("peers 0 of 0 peers")
}

fn format_progress_line(line: &str) -> String {
    let body = line.strip_prefix("Progress:").unwrap_or(line).trim();
    let parts: Vec<&str> = body.split(", ").collect();
    if parts.len() < 3 {
        return line.to_string();
    }

    let percent = parts[0].trim();
    let peer_info = strip_suffix_from(
        parts[1]
            .trim()
            .strip_prefix("dl from ")
            .unwrap_or(parts[1].trim()),
        " (",
    );
    let down_speed = extract_parenthesized(parts[1]).unwrap_or("?");
    let up_peers = strip_suffix_from(
        parts[2]
            .trim()
            .strip_prefix("ul to ")
            .unwrap_or(parts[2].trim()),
        " (",
    );
    let up_speed = extract_parenthesized(parts[2]).unwrap_or("?");
    let eta = extract_eta(body).unwrap_or("unknown");

    format!(
        "Progress {percent} | peers {peer_info} | down {down_speed} | up {up_speed} to {up_peers} | eta {eta}"
    )
}

fn extract_parenthesized(part: &str) -> Option<&str> {
    let start = part.find('(')?;
    let end = part[start + 1..].find(')')?;
    Some(&part[start + 1..start + 1 + end])
}

fn extract_eta(line: &str) -> Option<&str> {
    let start = line.rfind('[')?;
    let end = line[start + 1..].find(']')?;
    Some(&line[start + 1..start + 1 + end])
}

fn strip_suffix_from<'a>(value: &'a str, needle: &str) -> &'a str {
    value.split_once(needle).map_or(value, |(prefix, _)| prefix)
}

fn idle_status_line(last_progress_line: Option<&str>, elapsed_secs: u64) -> String {
    let elapsed = format_elapsed(elapsed_secs);
    match last_progress_line {
        Some(line) if matches_zero_peers(Some(line)) => {
            format!("{line} | waiting for peers | {elapsed} elapsed")
        }
        Some(line) => format!("{line} | no new update for {elapsed}"),
        None => format!("Connecting to peers... {elapsed} elapsed"),
    }
}

fn format_elapsed(elapsed_secs: u64) -> String {
    let hours = elapsed_secs / 3600;
    let minutes = (elapsed_secs % 3600) / 60;
    let seconds = elapsed_secs % 60;

    if hours > 0 {
        format!("{hours}h {minutes:02}m")
    } else if minutes > 0 {
        format!("{minutes}m {seconds:02}s")
    } else {
        format!("{seconds}s")
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn render_status_line(message: &str, active_status_line: &mut bool) -> Result<()> {
    let mut stdout = io::stdout().lock();
    write!(stdout, "\r\x1b[2K{message}")?;
    stdout.flush()?;
    *active_status_line = true;
    Ok(())
}

fn finish_status_line(active_status_line: &mut bool) -> Result<()> {
    if *active_status_line {
        let mut stdout = io::stdout().lock();
        writeln!(stdout)?;
        stdout.flush()?;
        *active_status_line = false;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        CliLine, classify_cli_line, extract_eta, format_elapsed, format_progress_line,
        idle_status_line, is_completion_marker, is_transmission_noise, matches_zero_peers,
        shell_quote,
    };

    #[test]
    fn filters_internal_transmission_logs() {
        assert!(is_transmission_noise(
            "[2026-03-27 18:04:12.262] rpc-server.cc:923: Serving RPC and Web requests"
        ));
        assert!(is_transmission_noise("transmission-cli 4.0.6 (38c164933e)"));
        assert!(is_transmission_noise(
            "^Dtransmission-cli 4.0.6 (38c164933e)"
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

    #[test]
    fn detects_zero_peer_progress() {
        assert!(matches_zero_peers(Some(
            "Progress: 0.0%, dl from 0 of 0 peers (0 kB/s), ul to 0 (0 kB/s) [None]"
        )));
        assert!(!matches_zero_peers(Some(
            "Progress: 12.0%, dl from 2 of 8 peers (120 kB/s), ul to 0 (0 kB/s) [None]"
        )));
    }

    #[test]
    fn formats_idle_status_from_last_progress() {
        assert_eq!(
            idle_status_line(
                Some(
                    "Progress 0.0% | peers 0 of 0 peers | down 0 kB/s | up 0 kB/s to 0 | eta None"
                ),
                90
            ),
            "Progress 0.0% | peers 0 of 0 peers | down 0 kB/s | up 0 kB/s to 0 | eta None | waiting for peers | 1m 30s elapsed"
        );
        assert_eq!(
            idle_status_line(
                Some(
                    "Progress 12.0% | peers 2 of 8 peers | down 120 kB/s | up 0 kB/s to 0 | eta 4m"
                ),
                12
            ),
            "Progress 12.0% | peers 2 of 8 peers | down 120 kB/s | up 0 kB/s to 0 | eta 4m | no new update for 12s"
        );
    }

    #[test]
    fn formats_elapsed_durations() {
        assert_eq!(format_elapsed(8), "8s");
        assert_eq!(format_elapsed(100), "1m 40s");
        assert_eq!(format_elapsed(3723), "1h 02m");
    }

    #[test]
    fn extracts_eta_from_progress_line() {
        assert_eq!(
            extract_eta(
                "Progress: 15.0%, dl from 2 of 8 peers (120 kB/s), ul to 0 (0 kB/s) [4m 12s]"
            ),
            Some("4m 12s")
        );
        assert_eq!(
            extract_eta("Progress: 0.0%, dl from 0 of 0 peers (0 kB/s), ul to 0 (0 kB/s) [None]"),
            Some("None")
        );
    }

    #[test]
    fn formats_progress_line_with_eta_and_speeds() {
        assert_eq!(
            format_progress_line(
                "Progress: 15.0%, dl from 2 of 8 peers (120 kB/s), ul to 1 (4 kB/s) [4m 12s]"
            ),
            "Progress 15.0% | peers 2 of 8 peers | down 120 kB/s | up 4 kB/s to 1 | eta 4m 12s"
        );
    }
}
