use std::io::{self, BufReader, Read, Write};
use std::process::Stdio;
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use crossterm::style::Stylize;

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
        command.arg("--truncate-console-readout=false");
        command.arg("--download-result=hide");
        command.arg("--enable-color=false");
        command.arg("--console-log-level=error");
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
            "{} {}",
            "Download:".bold().cyan(),
            torrent.name.as_str().magenta()
        );
        println!(
            "{} {}",
            "Target:".bold().blue(),
            self.config.download_target_display().as_str().white()
        );
        println!(
            "{} {}  {} {}",
            "Listed swarm:".bold().blue(),
            format!("{} seeders", torrent.seeders).green(),
            format!("{} leechers", torrent.leechers).yellow(),
            "from indexer".dark_grey()
        );

        let mut command = self.build_command(magnet);
        command.stdin(Stdio::null());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());

        let mut child = command.spawn().context("failed to run aria2c")?;
        let stdout = child.stdout.take().context("failed to capture aria2c stdout")?;
        let stderr = child.stderr.take().context("failed to capture aria2c stderr")?;

        let (sender, receiver) = mpsc::channel();
        spawn_cli_reader(stdout, sender.clone());
        spawn_cli_reader(stderr, sender);

        let started_at = Instant::now();
        let mut active_status_line = false;
        let mut last_rendered_line = String::new();
        let mut last_visible_update = Instant::now();
        let mut latest_progress: Option<Aria2Progress> = None;
        let mut latest_context: Option<Aria2Context> = None;

        render_status_line("Starting aria2c...", &mut active_status_line)?;

        loop {
            while let Ok(line) = receiver.try_recv() {
                if let Some(rendered) =
                    process_aria2_line(&line, &mut latest_progress, &mut latest_context)
                {
                    if rendered != last_rendered_line {
                        render_status_line(&rendered, &mut active_status_line)?;
                        last_rendered_line = rendered;
                        last_visible_update = Instant::now();
                    }
                }
            }

            if let Some(status) = child.try_wait().context("failed to read aria2c status")? {
                finish_status_line(&mut active_status_line)?;
                return if status.success() {
                    Ok(())
                } else {
                    bail!("aria2c exited with status {status}")
                };
            }

            match receiver.recv_timeout(Duration::from_millis(120)) {
                Ok(line) => {
                    if let Some(rendered) =
                        process_aria2_line(&line, &mut latest_progress, &mut latest_context)
                    {
                        if rendered != last_rendered_line {
                            render_status_line(&rendered, &mut active_status_line)?;
                            last_rendered_line = rendered;
                            last_visible_update = Instant::now();
                        }
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {}
            }

            if last_visible_update.elapsed() >= Duration::from_secs(2) {
                let message = idle_status_line(
                    latest_progress.as_ref(),
                    latest_context.as_ref(),
                    started_at.elapsed().as_secs(),
                );
                if message != last_rendered_line {
                    render_status_line(&message, &mut active_status_line)?;
                    last_rendered_line = message;
                }
                last_visible_update = Instant::now();
            }
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct Aria2Progress {
    complete: String,
    total: String,
    connections: Option<String>,
    seeds: Option<String>,
    download_speed: Option<String>,
    upload_speed: Option<String>,
    eta: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Aria2Context {
    Metadata(String),
    File(String),
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

fn process_aria2_line(
    line: &str,
    latest_progress: &mut Option<Aria2Progress>,
    latest_context: &mut Option<Aria2Context>,
) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() || is_aria2_noise(trimmed) {
        return None;
    }

    if let Some(progress) = parse_progress_line(trimmed) {
        *latest_progress = Some(progress);
        return Some(render_progress(
            latest_progress.as_ref()?,
            latest_context.as_ref(),
        ));
    }

    if let Some(context) = parse_context_line(trimmed) {
        *latest_context = Some(context);
        return latest_progress
            .as_ref()
            .map(|progress| render_progress(progress, latest_context.as_ref()))
            .or_else(|| Some(render_context_only(latest_context.as_ref()?)));
    }

    None
}

fn is_aria2_noise(line: &str) -> bool {
    line.starts_with("*** Download Progress Summary")
        || line.chars().all(|character| matches!(character, '=' | '-'))
        || line.contains("Failed to load DHT routing table")
        || line.contains("Exception caught while loading DHT routing table")
}

fn parse_progress_line(line: &str) -> Option<Aria2Progress> {
    if !line.starts_with("[#") {
        return None;
    }

    let body = line
        .trim_start_matches("[#")
        .strip_suffix(']')
        .unwrap_or(line)
        .trim();
    let mut fields = body.split_whitespace();
    let _gid = fields.next()?;
    let transfer = fields.next()?;
    let (complete, total) = transfer.split_once('/')?;

    let mut progress = Aria2Progress {
        complete: complete.to_string(),
        total: total.to_string(),
        connections: None,
        seeds: None,
        download_speed: None,
        upload_speed: None,
        eta: None,
    };

    for field in fields {
        if let Some(value) = field.strip_prefix("CN:") {
            progress.connections = Some(value.to_string());
        } else if let Some(value) = field.strip_prefix("SD:") {
            progress.seeds = Some(value.to_string());
        } else if let Some(value) = field.strip_prefix("DL:") {
            progress.download_speed = Some(value.to_string());
        } else if let Some(value) = field.strip_prefix("UL:") {
            progress.upload_speed = Some(value.to_string());
        } else if let Some(value) = field.strip_prefix("ETA:") {
            progress.eta = Some(value.to_string());
        }
    }

    Some(progress)
}

fn parse_context_line(line: &str) -> Option<Aria2Context> {
    let value = line.strip_prefix("FILE: ")?.trim();
    if let Some(name) = value.strip_prefix("[MEMORY][METADATA]") {
        return Some(Aria2Context::Metadata(name.trim().to_string()));
    }

    Some(Aria2Context::File(value.to_string()))
}

fn render_progress(progress: &Aria2Progress, context: Option<&Aria2Context>) -> String {
    let mut parts = Vec::new();

    match context {
        Some(Aria2Context::Metadata(name)) => {
            parts.push("metadata".bold().cyan().to_string());
            parts.push(truncate_middle(name, 28).magenta().to_string());
        }
        Some(Aria2Context::File(path)) => {
            if let Some(percent) = calculate_percent(&progress.complete, &progress.total) {
                parts.push(format!("{percent:>5.1}%").bold().green().to_string());
            } else {
                parts.push(format!(
                    "{} {}",
                    "done".bold().green(),
                    format!("{}/{}", progress.complete, progress.total).white()
                ));
            }
            parts.push(
                truncate_middle(path.rsplit('/').next().unwrap_or(path), 24)
                    .magenta()
                    .to_string(),
            );
        }
        None => {
            if let Some(percent) = calculate_percent(&progress.complete, &progress.total) {
                parts.push(format!("{percent:>5.1}%").bold().green().to_string());
            } else {
                parts.push(format!(
                    "{} {}",
                    "done".bold().green(),
                    format!("{}/{}", progress.complete, progress.total).white()
                ));
            }
        }
    }

    if let Some(connections) = &progress.connections {
        parts.push(format!(
            "{} {}",
            "peers".bold().blue(),
            connections.as_str().white()
        ));
    }
    if let Some(seeds) = &progress.seeds {
        parts.push(format!(
            "{} {}",
            "seeds".bold().blue(),
            seeds.as_str().white()
        ));
    }
    if let Some(download_speed) = &progress.download_speed {
        parts.push(format!(
            "{} {}",
            "down".bold().green(),
            format!("{download_speed}/s").white()
        ));
    }
    if let Some(upload_speed) = &progress.upload_speed {
        parts.push(format!(
            "{} {}",
            "up".bold().yellow(),
            format!("{upload_speed}/s").white()
        ));
    }
    if let Some(eta) = &progress.eta {
        parts.push(format!(
            "{} {}",
            "eta".bold().cyan(),
            eta.as_str().white()
        ));
    }

    parts.join(" | ")
}

fn render_context_only(context: &Aria2Context) -> String {
    match context {
        Aria2Context::Metadata(name) => format!(
            "{} {}",
            "metadata".bold().cyan(),
            truncate_middle(name, 28).magenta()
        ),
        Aria2Context::File(path) => format!(
            "{} {}",
            "target".bold().blue(),
            truncate_middle(path.rsplit('/').next().unwrap_or(path), 24).magenta()
        ),
    }
}

fn idle_status_line(
    latest_progress: Option<&Aria2Progress>,
    latest_context: Option<&Aria2Context>,
    elapsed_secs: u64,
) -> String {
    let elapsed = format_elapsed(elapsed_secs);
    match latest_progress {
        Some(progress) => {
            let mut line = render_progress(progress, latest_context);
            if progress.connections.as_deref() == Some("0") {
                line.push_str(&format!(" | {}", "waiting for peers".bold().yellow()));
            } else {
                line.push_str(&format!(" | {}", "holding last update".dark_grey()));
            }
            line.push_str(&format!(" | {}", elapsed.dark_grey()));
            line
        }
        None => match latest_context {
            Some(context) => format!("{} | {}", render_context_only(context), elapsed.dark_grey()),
            None => format!(
                "{} | {}",
                "connecting to peers".bold().cyan(),
                elapsed.dark_grey()
            ),
        },
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

fn truncate_middle(value: &str, max_chars: usize) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= max_chars {
        return value.to_string();
    }

    if max_chars <= 3 {
        return "...".to_string();
    }

    let head_len = (max_chars - 3) / 2;
    let tail_len = max_chars - 3 - head_len;
    let head: String = chars.iter().take(head_len).collect();
    let tail: String = chars
        .iter()
        .skip(chars.len().saturating_sub(tail_len))
        .collect();
    format!("{head}...{tail}")
}

fn calculate_percent(complete: &str, total: &str) -> Option<f64> {
    let complete = parse_size_to_bytes(complete)?;
    let total = parse_size_to_bytes(total)?;
    if total == 0 {
        return None;
    }
    Some((complete as f64 / total as f64) * 100.0)
}

fn parse_size_to_bytes(value: &str) -> Option<u64> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    let split_at = trimmed
        .find(|character: char| !(character.is_ascii_digit() || character == '.'))
        .unwrap_or(trimmed.len());
    let (number, unit) = trimmed.split_at(split_at);
    let amount: f64 = number.parse().ok()?;
    let multiplier = match unit.trim() {
        "" | "B" => 1.0,
        "KiB" => 1024.0,
        "MiB" => 1024.0 * 1024.0,
        "GiB" => 1024.0 * 1024.0 * 1024.0,
        "TiB" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => return None,
    };

    Some((amount * multiplier).round() as u64)
}

#[cfg(test)]
mod tests {
    use super::{
        Aria2Context, Aria2Progress, format_elapsed, idle_status_line, is_aria2_noise,
        parse_context_line, parse_progress_line, parse_size_to_bytes, render_progress,
        truncate_middle,
    };

    fn strip_ansi(value: &str) -> String {
        let mut result = String::new();
        let mut chars = value.chars().peekable();

        while let Some(character) = chars.next() {
            if character == '\u{1b}' {
                if matches!(chars.peek(), Some('[')) {
                    let _ = chars.next();
                    for next in chars.by_ref() {
                        if ('@'..='~').contains(&next) {
                            break;
                        }
                    }
                }
                continue;
            }
            result.push(character);
        }

        result
    }

    #[test]
    fn filters_aria2_noise() {
        assert!(is_aria2_noise(
            "*** Download Progress Summary as of Fri Mar 27 22:44:23 2026 ***"
        ));
        assert!(is_aria2_noise(
            "Exception: [DHTRoutingTableDeserializer.cc:83] errorCode=1 Failed to load DHT routing table"
        ));
        assert!(is_aria2_noise("===================================================================================="));
        assert!(!is_aria2_noise("[#167abb 0B/0B CN:0 SD:0 DL:0B]"));
    }

    #[test]
    fn parses_aria2_progress_lines() {
        let progress = parse_progress_line("[#167abb 512KiB/10MiB CN:4 SD:8 DL:1.2MiB ETA:8s]")
            .expect("progress line");
        assert_eq!(progress.complete, "512KiB");
        assert_eq!(progress.total, "10MiB");
        assert_eq!(progress.connections.as_deref(), Some("4"));
        assert_eq!(progress.seeds.as_deref(), Some("8"));
        assert_eq!(progress.download_speed.as_deref(), Some("1.2MiB"));
        assert_eq!(progress.eta.as_deref(), Some("8s"));
    }

    #[test]
    fn parses_metadata_context_lines() {
        assert_eq!(
            parse_context_line("FILE: [MEMORY][METADATA]Dune Part Two")
                .expect("context line"),
            Aria2Context::Metadata("Dune Part Two".to_string())
        );
    }

    #[test]
    fn renders_metadata_progress() {
        let progress = Aria2Progress {
            complete: "0B".to_string(),
            total: "0B".to_string(),
            connections: Some("0".to_string()),
            seeds: Some("0".to_string()),
            download_speed: Some("0B".to_string()),
            upload_speed: None,
            eta: None,
        };

        assert_eq!(
            strip_ansi(&render_progress(
                &progress,
                Some(&Aria2Context::Metadata("Dune Part Two".to_string()))
            )),
            "metadata | Dune Part Two | peers 0 | seeds 0 | down 0B/s"
        );
    }

    #[test]
    fn formats_idle_status() {
        let progress = Aria2Progress {
            complete: "0B".to_string(),
            total: "0B".to_string(),
            connections: Some("0".to_string()),
            seeds: Some("0".to_string()),
            download_speed: Some("0B".to_string()),
            upload_speed: None,
            eta: None,
        };

        assert_eq!(
            strip_ansi(&idle_status_line(
                Some(&progress),
                Some(&Aria2Context::Metadata("Dune Part Two".to_string())),
                90
            )),
            "metadata | Dune Part Two | peers 0 | seeds 0 | down 0B/s | waiting for peers | 1m 30s"
        );
    }

    #[test]
    fn formats_elapsed_durations() {
        assert_eq!(format_elapsed(8), "8s");
        assert_eq!(format_elapsed(100), "1m 40s");
        assert_eq!(format_elapsed(3723), "1h 02m");
    }

    #[test]
    fn truncates_middle() {
        assert_eq!(truncate_middle("abcdefghijk", 8), "ab...ijk");
        assert_eq!(truncate_middle("short", 8), "short");
    }

    #[test]
    fn parses_sizes_to_bytes() {
        assert_eq!(parse_size_to_bytes("0B"), Some(0));
        assert_eq!(parse_size_to_bytes("1KiB"), Some(1024));
        assert_eq!(parse_size_to_bytes("1.5MiB"), Some(1_572_864));
    }
}
