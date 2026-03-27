use std::collections::VecDeque;
use std::io::{self, BufReader, Read};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Gauge, List, ListItem, ListState, Paragraph, Wrap};

use crate::config::TransmissionConfig;
use crate::model::Torrent;
use crate::util::{ensure_transmission_cli_available, format_size};

const MAX_LOG_LINES: usize = 14;
const TICK_RATE: Duration = Duration::from_millis(100);

pub fn run_search_tui(
    query: String,
    results: Vec<Torrent>,
    transmission: TransmissionConfig,
) -> Result<()> {
    if results.is_empty() {
        bail!("no results found for '{query}'");
    }
    ensure_transmission_cli_available()?;

    let mut terminal = setup_terminal()?;
    let mut app = SearchTui::new(query, results, transmission);
    let run_result = app.run(&mut terminal);
    let restore_result = restore_terminal(&mut terminal);

    match (run_result, restore_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) => Err(error),
        (Ok(()), Err(error)) => Err(error),
        (Err(run_error), Err(_restore_error)) => Err(run_error),
    }
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode().context("failed to enable raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).context("failed to enter alternate screen")?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("failed to initialize terminal")?;
    terminal.hide_cursor().context("failed to hide cursor")?;
    terminal.clear().context("failed to clear terminal")?;
    Ok(terminal)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    disable_raw_mode().context("failed to disable raw mode")?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)
        .context("failed to leave alternate screen")?;
    terminal.show_cursor().context("failed to show cursor")?;
    Ok(())
}

struct SearchTui {
    query: String,
    results: Vec<Torrent>,
    selected: usize,
    transmission: TransmissionConfig,
    download: Option<DownloadSession>,
    should_quit: bool,
    tick: usize,
}

impl SearchTui {
    fn new(query: String, results: Vec<Torrent>, transmission: TransmissionConfig) -> Self {
        Self {
            query,
            results,
            selected: 0,
            transmission,
            download: None,
            should_quit: false,
            tick: 0,
        }
    }

    fn run(&mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
        while !self.should_quit {
            self.tick = self.tick.wrapping_add(1);
            if let Some(download) = self.download.as_mut() {
                download.drain_events();
                download.poll_child()?;
            }

            terminal.draw(|frame| self.draw(frame))?;

            if event::poll(TICK_RATE).context("failed to poll terminal events")? {
                let Event::Key(key) = event::read().context("failed to read terminal event")?
                else {
                    continue;
                };
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                self.handle_key(key.code)?;
            }
        }

        Ok(())
    }

    fn draw(&mut self, frame: &mut ratatui::Frame<'_>) {
        if let Some(download) = self.download.as_ref() {
            self.draw_download(frame, download);
        } else {
            self.draw_selection(frame);
        }
    }

    fn draw_selection(&self, frame: &mut ratatui::Frame<'_>) {
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(10),
                Constraint::Length(3),
            ])
            .split(frame.area());

        let header = Paragraph::new(format!(
            "pirate-ctl tui | query: {} | {} result(s)",
            self.query,
            self.results.len()
        ))
        .block(Block::default().borders(Borders::ALL).title("Search"))
        .style(Style::default().fg(Color::Cyan));
        frame.render_widget(header, layout[0]);

        let body = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(62), Constraint::Percentage(38)])
            .split(layout[1]);

        let items: Vec<ListItem<'_>> = self
            .results
            .iter()
            .map(|torrent| {
                ListItem::new(format!(
                    "{:>4} seeders  {:>8}  {}",
                    torrent.seeders,
                    format_size(torrent.size_bytes),
                    torrent.name
                ))
            })
            .collect();
        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title("Results"))
            .highlight_style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol(">> ");
        let mut state = ListState::default();
        state.select(Some(self.selected));
        frame.render_stateful_widget(list, body[0], &mut state);

        let selected = &self.results[self.selected];
        let detail_lines = vec![
            Line::from(format!("Name: {}", selected.name)),
            Line::from(format!("ID: {}", selected.id)),
            Line::from(format!("Seeders: {}", selected.seeders)),
            Line::from(format!("Leechers: {}", selected.leechers)),
            Line::from(format!("Size: {}", format_size(selected.size_bytes))),
            Line::from(format!(
                "Status: {}",
                selected.status.as_deref().unwrap_or("-")
            )),
            Line::from(format!(
                "Uploader: {}",
                selected.uploaded_by.as_deref().unwrap_or("-")
            )),
        ];
        let details = Paragraph::new(detail_lines)
            .block(Block::default().borders(Borders::ALL).title("Details"))
            .wrap(Wrap { trim: true });
        frame.render_widget(details, body[1]);

        let footer =
            Paragraph::new("Up/Down or j/k to move. Enter downloads with transmission-cli. q quits.")
                .block(Block::default().borders(Borders::ALL).title("Keys"));
        frame.render_widget(footer, layout[2]);
    }

    fn draw_download(&self, frame: &mut ratatui::Frame<'_>, download: &DownloadSession) {
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(5),
                Constraint::Length(3),
                Constraint::Min(8),
                Constraint::Length(3),
            ])
            .split(frame.area());

        let summary = Paragraph::new(vec![
            Line::from(format!("Torrent: {}", download.torrent.name)),
            Line::from(format!(
                "Size: {} | Seeders: {} | Elapsed: {}s",
                format_size(download.torrent.size_bytes),
                download.torrent.seeders,
                download.started_at.elapsed().as_secs()
            )),
            Line::from(format!("Info hash: {}", download.torrent.info_hash)),
        ])
        .block(Block::default().borders(Borders::ALL).title("Downloading"))
        .wrap(Wrap { trim: false });
        frame.render_widget(summary, layout[0]);

        let gauge = Gauge::default()
            .block(Block::default().borders(Borders::ALL).title("Progress"))
            .gauge_style(
                Style::default()
                    .fg(if download.is_finished() {
                        Color::Green
                    } else {
                        Color::LightBlue
                    })
                    .bg(Color::Black)
                    .add_modifier(Modifier::BOLD),
            )
            .ratio(download.progress_ratio(self.tick))
            .label(download.progress_label(self.tick));
        frame.render_widget(gauge, layout[1]);

        let logs = download.logs_for_render();
        let log_widget = Paragraph::new(logs)
            .block(Block::default().borders(Borders::ALL).title("Downloader Output"))
            .wrap(Wrap { trim: false });
        frame.render_widget(log_widget, layout[2]);

        let footer = Paragraph::new(download.footer_text())
            .block(Block::default().borders(Borders::ALL).title("Keys"));
        frame.render_widget(footer, layout[3]);
    }

    fn handle_key(&mut self, key: KeyCode) -> Result<()> {
        if self.download.is_some() {
            return self.handle_download_key(key);
        }

        match key {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected > 0 {
                    self.selected -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.selected + 1 < self.results.len() {
                    self.selected += 1;
                }
            }
            KeyCode::Enter => {
                let torrent = self.results[self.selected].clone();
                self.download = Some(DownloadSession::start(torrent, &self.transmission)?);
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                self.should_quit = true;
            }
            _ => {}
        }

        Ok(())
    }

    fn handle_download_key(&mut self, key: KeyCode) -> Result<()> {
        let Some(download) = self.download.as_mut() else {
            return Ok(());
        };

        match key {
            KeyCode::Esc | KeyCode::Char('q') => {
                if download.is_finished() {
                    self.should_quit = true;
                } else {
                    download.abort()?;
                    self.should_quit = true;
                }
            }
            KeyCode::Enter => {
                if download.is_finished() {
                    self.should_quit = true;
                }
            }
            _ => {}
        }

        Ok(())
    }
}

struct DownloadSession {
    torrent: Torrent,
    child: Option<Child>,
    receiver: Receiver<DownloadEvent>,
    logs: VecDeque<String>,
    progress: Option<f64>,
    status_text: String,
    started_at: Instant,
    outcome: Option<DownloadOutcome>,
}

impl DownloadSession {
    fn start(torrent: Torrent, config: &TransmissionConfig) -> Result<Self> {
        let (child, receiver) = spawn_transmission_cli(&torrent, config)?;
        let mut logs = VecDeque::new();
        logs.push_back("Started transmission-cli".to_string());

        Ok(Self {
            torrent,
            child: Some(child),
            receiver,
            logs,
            progress: None,
            status_text: "Connecting to peers...".to_string(),
            started_at: Instant::now(),
            outcome: None,
        })
    }

    fn drain_events(&mut self) {
        while let Ok(event) = self.receiver.try_recv() {
            match event {
                DownloadEvent::Output(line) => {
                    if let Some(progress) = parse_progress(&line) {
                        self.progress = Some(progress);
                    }
                    self.status_text = line.clone();
                    self.push_log(line);
                }
                DownloadEvent::ReadError(error) => {
                    self.push_log(error.clone());
                    self.status_text = error;
                }
            }
        }
    }

    fn poll_child(&mut self) -> Result<()> {
        let Some(child) = self.child.as_mut() else {
            return Ok(());
        };

        if let Some(status) = child
            .try_wait()
            .context("failed to read transmission-cli status")?
        {
            if status.success() {
                self.progress = Some(1.0);
                self.status_text = "Download finished".to_string();
                self.push_log("transmission-cli exited successfully".to_string());
                self.outcome = Some(DownloadOutcome::Success);
            } else {
                let code = status
                    .code()
                    .map_or_else(|| "signal".to_string(), |code| code.to_string());
                let message = format!("transmission-cli exited with status {code}");
                self.status_text = message.clone();
                self.push_log(message.clone());
                self.outcome = Some(DownloadOutcome::Failed);
            }
            self.child = None;
        }

        Ok(())
    }

    fn abort(&mut self) -> Result<()> {
        if let Some(mut child) = self.child.take() {
            child
                .kill()
                .context("failed to stop transmission-cli process")?;
            let _ = child.wait();
        }
        self.status_text = "Download aborted".to_string();
        self.push_log("transmission-cli aborted by user".to_string());
        self.outcome = Some(DownloadOutcome::Aborted);
        Ok(())
    }

    fn is_finished(&self) -> bool {
        self.outcome.is_some()
    }

    fn progress_ratio(&self, tick: usize) -> f64 {
        if let Some(progress) = self.progress {
            progress
        } else if self.is_finished() {
            1.0
        } else {
            let phase = (tick % 30) as f64 / 30.0;
            if phase <= 0.5 {
                phase * 2.0
            } else {
                (1.0 - phase) * 2.0
            }
        }
    }

    fn progress_label(&self, tick: usize) -> String {
        if let Some(progress) = self.progress {
            format!("{:.1}% | {}", progress * 100.0, self.status_text)
        } else if self.is_finished() {
            self.status_text.clone()
        } else {
            let spinner = ["|", "/", "-", "\\"];
            format!(
                "{} {}",
                spinner[tick % spinner.len()],
                self.status_text
            )
        }
    }

    fn logs_for_render(&self) -> Vec<Line<'_>> {
        self.logs
            .iter()
            .map(|line| Line::from(line.as_str()))
            .collect()
    }

    fn footer_text(&self) -> &'static str {
        if self.is_finished() {
            "Enter or q to exit."
        } else {
            "q aborts the foreground download."
        }
    }

    fn push_log(&mut self, line: String) {
        if self.logs.len() == MAX_LOG_LINES {
            self.logs.pop_front();
        }
        self.logs.push_back(line);
    }
}

enum DownloadOutcome {
    Success,
    Failed,
    Aborted,
}

enum DownloadEvent {
    Output(String),
    ReadError(String),
}

fn spawn_transmission_cli(
    torrent: &Torrent,
    config: &TransmissionConfig,
) -> Result<(Child, Receiver<DownloadEvent>)> {
    ensure_transmission_cli_available()?;

    let mut command = Command::new("transmission-cli");
    command.stdin(Stdio::null());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    if let Some(download_dir) = &config.download_dir {
        command.arg("-w").arg(download_dir);
    }
    command.arg(torrent.resolved_magnet());

    let mut child = command
        .spawn()
        .context("failed to start transmission-cli")?;
    let stdout = child
        .stdout
        .take()
        .context("failed to capture transmission-cli stdout")?;
    let stderr = child
        .stderr
        .take()
        .context("failed to capture transmission-cli stderr")?;

    let (sender, receiver) = mpsc::channel();
    spawn_reader(stdout, sender.clone(), "");
    spawn_reader(stderr, sender, "stderr | ");

    Ok((child, receiver))
}

fn spawn_reader<R>(stream: R, sender: Sender<DownloadEvent>, prefix: &'static str)
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
                    emit_buffer(&buffer, &sender, prefix);
                    break;
                }
                Ok(_) => match byte[0] {
                    b'\n' | b'\r' => {
                        emit_buffer(&buffer, &sender, prefix);
                        buffer.clear();
                    }
                    value => buffer.push(value),
                },
                Err(error) => {
                    let _ = sender.send(DownloadEvent::ReadError(format!(
                        "{prefix}failed to read downloader output: {error}"
                    )));
                    break;
                }
            }
        }
    });
}

fn emit_buffer(buffer: &[u8], sender: &Sender<DownloadEvent>, prefix: &str) {
    if buffer.is_empty() {
        return;
    }

    let text = String::from_utf8_lossy(buffer);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }

    let _ = sender.send(DownloadEvent::Output(format!("{prefix}{trimmed}")));
}

fn parse_progress(line: &str) -> Option<f64> {
    let bytes = line.as_bytes();
    let mut index = 0;

    while index < bytes.len() {
        if !bytes[index].is_ascii_digit() {
            index += 1;
            continue;
        }

        let start = index;
        let mut end = index;
        let mut seen_dot = false;

        while end < bytes.len() {
            match bytes[end] {
                b'0'..=b'9' => end += 1,
                b'.' if !seen_dot => {
                    seen_dot = true;
                    end += 1;
                }
                _ => break,
            }
        }

        if end < bytes.len() && bytes[end] == b'%' {
            let value: f64 = line[start..end].parse().ok()?;
            return Some((value / 100.0).clamp(0.0, 1.0));
        }

        index = end + 1;
    }

    None
}

#[cfg(test)]
mod tests {
    use super::parse_progress;

    #[test]
    fn parses_progress_percentages() {
        assert_eq!(parse_progress("Progress: 12.5%"), Some(0.125));
        assert_eq!(parse_progress("99% complete"), Some(0.99));
        assert_eq!(parse_progress("no percentage here"), None);
    }
}
