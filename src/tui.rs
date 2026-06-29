use std::collections::VecDeque;
use std::io::{self, BufReader, Read};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

use crate::config::{Aria2Config, TransmissionConfig};
use crate::history::{DownloadHistory, DownloadHistoryEntry};
use crate::model::{Torrent, TrackedDownload};
use crate::state::{DetachedDownloadRecord, load_recent_detached_downloads, record_detached_download};
use crate::util::{ensure_aria2_available, ensure_transmission_cli_available, format_size};

const MAX_LOG_LINES: usize = 14;
const TICK_RATE: Duration = Duration::from_millis(250);
const DETACHED_REFRESH_TICKS: usize = 4;

pub fn run_search_tui<F>(
    initial_query: Option<String>,
    backend: TuiDownloader,
    history_entries: Vec<DownloadHistoryEntry>,
    history_path: PathBuf,
    search: F,
    hydrate: impl FnMut(Torrent) -> Result<Torrent>,
) -> Result<()>
where
    F: FnMut(&str) -> Result<Vec<Torrent>>,
{
    backend.ensure_available()?;

    let mut terminal = setup_terminal()?;
    let mut app = SearchTui::new(
        initial_query,
        backend,
        history_entries,
        history_path,
        search,
        hydrate,
    )?;
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

#[derive(Clone)]
pub enum TuiDownloader {
    Transmission(TransmissionConfig),
    Aria2(Aria2Config),
}

impl TuiDownloader {
    fn ensure_available(&self) -> Result<()> {
        match self {
            Self::Transmission(_) => ensure_transmission_cli_available(),
            Self::Aria2(_) => ensure_aria2_available(),
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Self::Transmission(_) => "transmission-cli",
            Self::Aria2(_) => "aria2c",
        }
    }

    fn download_target_display(&self) -> String {
        match self {
            Self::Transmission(config) => config.download_target_display(),
            Self::Aria2(config) => config.download_target_display(),
        }
    }

    fn target_path_for(&self, torrent: &Torrent) -> PathBuf {
        let maybe_dir = match self {
            Self::Transmission(config) => config.download_dir_path(),
            Self::Aria2(config) => config.download_dir_path(),
        };

        maybe_dir
            .map(|dir| dir.join(&torrent.name))
            .unwrap_or_default()
    }
}

struct SearchTui<F, H>
where
    F: FnMut(&str) -> Result<Vec<Torrent>>,
    H: FnMut(Torrent) -> Result<Torrent>,
{
    query_input: String,
    query: Option<String>,
    results: Vec<Torrent>,
    selected_result: usize,
    selected_download: usize,
    downloads: Vec<DownloadSession>,
    backend: TuiDownloader,
    history: DownloadHistory,
    should_quit: bool,
    tick: usize,
    focus: FocusPane,
    status_message: String,
    search: F,
    hydrate: H,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FocusPane {
    Query,
    Results,
    Downloads,
}

impl<F, H> SearchTui<F, H>
where
    F: FnMut(&str) -> Result<Vec<Torrent>>,
    H: FnMut(Torrent) -> Result<Torrent>,
{
    fn new(
        initial_query: Option<String>,
        backend: TuiDownloader,
        history_entries: Vec<DownloadHistoryEntry>,
        history_path: PathBuf,
        search: F,
        hydrate: H,
    ) -> Result<Self> {
        let mut downloads: Vec<DownloadSession> = load_recent_detached_downloads(24)?
            .into_iter()
            .map(DownloadSession::from_detached_record)
            .collect();
        for entry in history_entries {
            if downloads
                .iter()
                .any(|download| download.torrent.info_hash == entry.info_hash)
            {
                continue;
            }
            downloads.push(DownloadSession::from_history_entry(entry));
        }
        let mut app = Self {
            query_input: initial_query.unwrap_or_default(),
            query: None,
            results: Vec::new(),
            selected_result: 0,
            selected_download: 0,
            downloads,
            backend,
            history: DownloadHistory::new(history_path),
            should_quit: false,
            tick: 0,
            focus: FocusPane::Query,
            status_message:
                "Type a query and press Enter to search. Recent completed and detached downloads appear below."
                    .to_string(),
            search,
            hydrate,
        };
        if !app.query_input.trim().is_empty() {
            app.submit_query()?;
        }
        Ok(app)
    }

    fn run(&mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
        while !self.should_quit {
            self.tick = self.tick.wrapping_add(1);
            for download in &mut self.downloads {
                download.drain_events();
                download.poll_child()?;
            }
            self.persist_completed_downloads()?;
            if self.tick % DETACHED_REFRESH_TICKS == 0 {
                self.refresh_detached_downloads()?;
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

    fn persist_completed_downloads(&mut self) -> Result<()> {
        for download in &mut self.downloads {
            if !download.should_sync_history() {
                continue;
            }

            download.mark_history_synced();
            if download.target_path.as_os_str().is_empty() {
                download.push_log(
                    "Completed, but pirata could not determine the final target path to persist."
                        .to_string(),
                );
                continue;
            }

            let tracked = download.as_tracked_download();
            let entry = DownloadHistoryEntry::from_tracked_download(
                &tracked,
                crate::history::now_epoch_secs(),
            );
            self.history.upsert_blocking(entry)?;
        }

        Ok(())
    }

    fn refresh_detached_downloads(&mut self) -> Result<()> {
        let detached_downloads = load_recent_detached_downloads(24)?;
        let mut appended = 0;

        for record in detached_downloads.into_iter().rev() {
            let key = record.key();
            let already_present = self
                .downloads
                .iter()
                .any(|download| download.detached_key() == Some(key));
            if already_present {
                continue;
            }
            self.downloads
                .push(DownloadSession::from_detached_record(record));
            appended += 1;
        }

        if appended > 0 {
            self.status_message = format!(
                "Detected {appended} new external download{}.",
                if appended == 1 { "" } else { "s" }
            );
            if self.selected_download >= self.downloads.len() {
                self.selected_download = self.downloads.len().saturating_sub(1);
            }
        }

        Ok(())
    }

    fn draw(&mut self, frame: &mut ratatui::Frame<'_>) {
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(14),
                Constraint::Length(11),
                Constraint::Length(3),
            ])
            .split(frame.area());

        let header = Paragraph::new(Line::from(vec![
            Span::styled(
                " pirata ",
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            focus_badge("query", matches!(self.focus, FocusPane::Query)),
            Span::raw(" "),
            focus_badge("results", matches!(self.focus, FocusPane::Results)),
            Span::raw(" "),
            focus_badge("downloads", matches!(self.focus, FocusPane::Downloads)),
            Span::raw(format!(
                "   backend {}   results {}   active {}",
                self.backend.name(),
                self.results.len(),
                self.active_downloads()
            )),
        ]))
        .block(Block::default().borders(Borders::ALL).title("Dashboard"))
        .style(Style::default().fg(Color::White));
        frame.render_widget(header, layout[0]);

        let left = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(8)])
            .split(layout[1]);

        let query_block = Block::default()
            .borders(Borders::ALL)
            .title("Search Query")
            .border_style(self.focus_style(FocusPane::Query));
        let query = Paragraph::new(self.query_input.as_str())
            .block(query_block)
            .style(Style::default().fg(Color::White));
        frame.render_widget(query, left[0]);
        if matches!(self.focus, FocusPane::Query) {
            let cursor_x = left[0]
                .x
                .saturating_add(1 + self.query_input.chars().count() as u16);
            let cursor_y = left[0].y.saturating_add(1);
            frame.set_cursor_position((cursor_x, cursor_y));
        }

        let result_items: Vec<ListItem<'_>> = self
            .results
            .iter()
            .map(|torrent| {
                let line = Line::from(vec![
                    Span::styled(
                        format!("{:>4} ", torrent.seeders),
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled("se ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        format!("{:>4} ", torrent.leechers),
                        Style::default().fg(Color::Red),
                    ),
                    Span::styled("le ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        format!("{:>8} ", format_size(torrent.size_bytes)),
                        Style::default().fg(Color::Cyan),
                    ),
                    status_span(torrent.status.as_deref()),
                    Span::raw(" "),
                    Span::raw(torrent.name.clone()),
                ]);
                ListItem::new(line)
            })
            .collect();
        let results = List::new(result_items)
            .block(Block::default().borders(Borders::ALL).title("Results"))
            .highlight_style(
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▌ ");
        let mut results_state = ListState::default();
        results_state.select((!self.results.is_empty()).then_some(self.selected_result));
        frame.render_stateful_widget(results, left[1], &mut results_state);

        let download_items: Vec<ListItem<'_>> = if self.downloads.is_empty() {
            vec![ListItem::new(Line::from(vec![Span::styled(
                "No downloads yet. Press Enter on a result to start one.",
                Style::default().fg(Color::DarkGray),
            )]))]
        } else {
            self.downloads
                .iter()
                .map(|download| {
                    let mut spans: Vec<Span<'_>> = vec![
                        download.status_badge(),
                        Span::raw(" "),
                        Span::styled(
                            progress_bar(download.progress_ratio(), 10),
                            Style::default().fg(
                                if matches!(download.outcome, Some(DownloadOutcome::Success)) {
                                    Color::Green
                                } else {
                                    Color::Cyan
                                },
                            ),
                        ),
                        Span::raw("  "),
                        Span::styled(
                            format_elapsed_duration(download.started_at.elapsed()),
                            Style::default().fg(Color::DarkGray),
                        ),
                        Span::raw("  "),
                    ];
                    if download.is_managed_active() {
                        spans.push(Span::styled(
                            truncate_end(&download.status_text, 42),
                            Style::default().fg(Color::White),
                        ));
                    } else {
                        spans.push(Span::styled(
                            format_size(download.torrent.size_bytes),
                            Style::default().fg(Color::Cyan),
                        ));
                        spans.push(Span::raw("  "));
                        spans.push(Span::styled(
                            format!("{}se", download.torrent.seeders),
                            Style::default().fg(Color::Yellow),
                        ));
                    }
                    spans.push(Span::raw("  "));
                    spans.push(Span::raw(download.torrent.name.clone()));
                    ListItem::new(Line::from(spans))
                })
                .collect()
        };
        let downloads = List::new(download_items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Downloads")
                    .border_style(self.focus_style(FocusPane::Downloads)),
            )
            .highlight_style(
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▌ ");
        let mut downloads_state = ListState::default();
        downloads_state.select((!self.downloads.is_empty()).then_some(self.selected_download));
        frame.render_stateful_widget(downloads, layout[2], &mut downloads_state);

        let footer = Paragraph::new(vec![
            Line::from(vec![
                Span::styled(
                    "Tab",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" focus  "),
                Span::styled(
                    "Enter",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" search/start  "),
                Span::styled(
                    "/",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" edit query  "),
                Span::styled(
                    "d",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" abort download  "),
                Span::styled(
                    "q",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" detach + quit  "),
                Span::styled(
                    "Q",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" stop active + quit"),
            ]),
            Line::from(Span::styled(
                self.status_message.clone(),
                Style::default().fg(Color::White),
            )),
        ])
        .block(Block::default().borders(Borders::ALL).title("Keys"));
        frame.render_widget(footer, layout[3]);
    }

    fn handle_key(&mut self, key: KeyCode) -> Result<()> {
        match key {
            KeyCode::Tab => {
                self.cycle_focus();
                return Ok(());
            }
            KeyCode::BackTab => {
                self.cycle_focus_reverse();
                return Ok(());
            }
            KeyCode::Char('/') => {
                if matches!(self.focus, FocusPane::Query) {
                    return self.handle_query_key(key);
                }
                self.focus = FocusPane::Query;
                self.query_input.clear();
                self.status_message =
                    "Type a new query and press Enter to search again.".to_string();
                return Ok(());
            }
            KeyCode::Char('Q') => {
                self.abort_all_downloads()?;
                self.should_quit = true;
                return Ok(());
            }
            KeyCode::Char('q') | KeyCode::Esc => {
                self.detach_all_downloads()?;
                self.should_quit = true;
                return Ok(());
            }
            _ => {}
        }

        match self.focus {
            FocusPane::Query => self.handle_query_key(key),
            FocusPane::Results => self.handle_results_key(key),
            FocusPane::Downloads => self.handle_downloads_key(key),
        }
    }

    fn handle_query_key(&mut self, key: KeyCode) -> Result<()> {
        match key {
            KeyCode::Enter => self.submit_query()?,
            KeyCode::Backspace => {
                self.query_input.pop();
            }
            KeyCode::Char(character) => {
                self.query_input.push(character);
            }
            _ => {}
        }

        Ok(())
    }

    fn handle_results_key(&mut self, key: KeyCode) -> Result<()> {
        match key {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected_result > 0 {
                    self.selected_result -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.selected_result + 1 < self.results.len() {
                    self.selected_result += 1;
                }
            }
            KeyCode::Enter => {
                if let Some(torrent) = self.results.get(self.selected_result).cloned() {
                    let torrent = (self.hydrate)(torrent)?;
                    self.downloads
                        .push(DownloadSession::start(torrent.clone(), &self.backend)?);
                    self.selected_download = self.downloads.len().saturating_sub(1);
                    self.focus = FocusPane::Downloads;
                    self.status_message = format!(
                        "Started '{}' with {}. Search again while it runs.",
                        torrent.name,
                        self.backend.name()
                    );
                }
            }
            _ => {}
        }

        Ok(())
    }

    fn handle_downloads_key(&mut self, key: KeyCode) -> Result<()> {
        match key {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected_download > 0 {
                    self.selected_download -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.selected_download + 1 < self.downloads.len() {
                    self.selected_download += 1;
                }
            }
            KeyCode::Char('d') => {
                if let Some(download) = self.downloads.get_mut(self.selected_download) {
                    if download.is_managed_active() {
                        let name = download.torrent.name.clone();
                        download.abort()?;
                        self.status_message = format!("Aborted '{name}'.");
                    } else if matches!(
                        download.tracking,
                        DownloadTracking::Detached { .. } | DownloadTracking::History
                    ) {
                        self.status_message =
                            "That row was restored from saved state and cannot be stopped here."
                                .to_string();
                    }
                }
            }
            _ => {}
        }

        Ok(())
    }

    fn submit_query(&mut self) -> Result<()> {
        let query = self.query_input.trim();
        if query.is_empty() {
            self.status_message = "Enter a query before searching.".to_string();
            return Ok(());
        }

        self.status_message = format!("Searching for '{query}'...");
        let results = (self.search)(query)?;
        self.query = Some(query.to_string());
        self.results = results;
        self.selected_result = 0;
        self.focus = FocusPane::Results;
        self.status_message = if self.results.is_empty() {
            format!("No results found for '{query}'.")
        } else {
            format!(
                "Loaded {} result(s) for '{query}'. Press Enter to start a download.",
                self.results.len()
            )
        };
        Ok(())
    }

    fn cycle_focus(&mut self) {
        self.focus = match self.focus {
            FocusPane::Query => FocusPane::Results,
            FocusPane::Results => FocusPane::Downloads,
            FocusPane::Downloads => FocusPane::Query,
        };
    }

    fn cycle_focus_reverse(&mut self) {
        self.focus = match self.focus {
            FocusPane::Query => FocusPane::Downloads,
            FocusPane::Results => FocusPane::Query,
            FocusPane::Downloads => FocusPane::Results,
        };
    }

    fn active_downloads(&self) -> usize {
        self.downloads
            .iter()
            .filter(|download| download.is_managed_active())
            .count()
    }

    fn abort_all_downloads(&mut self) -> Result<()> {
        for download in &mut self.downloads {
            if download.is_managed_active() {
                download.abort()?;
            }
        }
        Ok(())
    }

    fn detach_all_downloads(&mut self) -> Result<()> {
        for download in &mut self.downloads {
            if download.is_managed_active() {
                download.detach()?;
            }
        }
        Ok(())
    }

    fn focus_style(&self, pane: FocusPane) -> Style {
        if self.focus == pane {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        }
    }

}

struct DownloadSession {
    torrent: Torrent,
    target_path: PathBuf,
    backend: SessionBackend,
    tracking: DownloadTracking,
    child: Option<Child>,
    receiver: Option<Receiver<DownloadEvent>>,
    logs: VecDeque<String>,
    progress: Option<f64>,
    status_text: String,
    context_text: Option<String>,
    started_at: Instant,
    outcome: Option<DownloadOutcome>,
    history_synced: bool,
}

impl DownloadSession {
    fn start(torrent: Torrent, backend: &TuiDownloader) -> Result<Self> {
        let ((child, receiver), session_backend) = match backend {
            TuiDownloader::Transmission(config) => (
                spawn_transmission_cli(&torrent, config)?,
                SessionBackend::Transmission,
            ),
            TuiDownloader::Aria2(config) => {
                (spawn_aria2_cli(&torrent, config)?, SessionBackend::Aria2)
            }
        };
        let mut logs = VecDeque::new();
        logs.push_back(format!("Started {}", backend.name()));
        logs.push_back(format!(
            "Downloading to {}",
            backend.download_target_display()
        ));

        Ok(Self {
            target_path: backend.target_path_for(&torrent),
            torrent,
            backend: session_backend,
            tracking: DownloadTracking::Managed,
            child: Some(child),
            receiver: Some(receiver),
            logs,
            progress: None,
            status_text: match backend {
                TuiDownloader::Transmission(_) => "Connecting to peers...".to_string(),
                TuiDownloader::Aria2(_) => "Fetching metadata...".to_string(),
            },
            context_text: None,
            started_at: Instant::now(),
            outcome: None,
            history_synced: false,
        })
    }

    fn from_detached_record(record: DetachedDownloadRecord) -> Self {
        let target_path = record
            .download_dir
            .as_ref()
            .map(PathBuf::from)
            .map(|dir| dir.join(&record.torrent.name))
            .unwrap_or_default();
        let key = record.key();
        let elapsed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .saturating_sub(record.started_unix_secs);
        let mut logs = VecDeque::new();
        logs.push_back(format!(
            "Detached transmission-cli launch (pid {}) started outside the TUI.",
            record.pid
        ));
        logs.push_back(format!(
            "Downloading to {}",
            record
                .download_dir
                .unwrap_or_else(|| "Transmission default download directory".to_string())
        ));
        logs.push_back(
            "Live progress is only available for downloads started from inside this TUI session."
                .to_string(),
        );

        Self {
            target_path,
            torrent: record.torrent,
            backend: SessionBackend::External,
            tracking: DownloadTracking::Detached { key },
            child: None,
            receiver: None,
            logs,
            progress: None,
            status_text: "Detached CLI download. Progress cannot be attached after launch."
                .to_string(),
            context_text: None,
            started_at: Instant::now() - Duration::from_secs(elapsed),
            outcome: None,
            history_synced: true,
        }
    }

    fn from_history_entry(entry: DownloadHistoryEntry) -> Self {
        let mut logs = VecDeque::new();
        logs.push_back("Recovered completed download from pirata history.".to_string());
        logs.push_back(format!("Target {}", entry.target_path.display()));

        Self {
            torrent: Torrent {
                id: entry.info_hash.clone(),
                name: entry.name,
                info_hash: entry.info_hash,
                magnet: None,
                seeders: 0,
                leechers: 0,
                size_bytes: 0,
                status: Some("completed".to_string()),
                uploaded_by: None,
                description: None,
                category: None,
                subcategory: None,
                added: None,
            },
            target_path: entry.target_path,
            backend: SessionBackend::External,
            tracking: DownloadTracking::History,
            child: None,
            receiver: None,
            logs,
            progress: Some(1.0),
            status_text: "Completed in a previous pirata session.".to_string(),
            context_text: None,
            started_at: Instant::now(),
            outcome: Some(DownloadOutcome::Success),
            history_synced: true,
        }
    }

    fn drain_events(&mut self) {
        while let Some(event) = self
            .receiver
            .as_ref()
            .and_then(|receiver| receiver.try_recv().ok())
        {
            match event {
                DownloadEvent::Output(line) => {
                    match self.backend {
                        SessionBackend::Transmission => {
                            if let Some(progress) = parse_transmission_progress(&line) {
                                self.progress = Some(progress);
                            }
                            self.status_text = line.clone();
                        }
                        SessionBackend::Aria2 => {
                            if let Some(update) =
                                parse_aria2_update(&line, self.context_text.as_deref())
                            {
                                if let Some(progress) = update.progress {
                                    self.progress = Some(progress);
                                }
                                if let Some(context) = update.context {
                                    self.context_text = Some(context);
                                }
                                self.status_text = update.status;
                            } else {
                                continue;
                            }
                        }
                        SessionBackend::External => {
                            self.status_text = line.clone();
                        }
                    }
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
                self.push_log(format!(
                    "{} exited successfully",
                    self.backend.display_name()
                ));
                self.outcome = Some(DownloadOutcome::Success);
            } else {
                let code = status
                    .code()
                    .map_or_else(|| "signal".to_string(), |code| code.to_string());
                let message = format!("{} exited with status {code}", self.backend.display_name());
                self.status_text = message.clone();
                self.push_log(message.clone());
                self.outcome = Some(DownloadOutcome::Failed);
            }
            self.child = None;
        }

        Ok(())
    }

    fn abort(&mut self) -> Result<()> {
        if matches!(
            self.tracking,
            DownloadTracking::Detached { .. } | DownloadTracking::History
        ) {
            self.status_text = "Detached download cannot be controlled from this TUI.".to_string();
            self.push_log(
                "This row was loaded from saved state and cannot be aborted here.".to_string(),
            );
            return Ok(());
        }
        if let Some(mut child) = self.child.take() {
            child
                .kill()
                .with_context(|| format!("failed to stop {}", self.backend.display_name()))?;
            let _ = child.wait();
        }
        self.status_text = "Download aborted".to_string();
        self.push_log(format!("{} aborted by user", self.backend.display_name()));
        self.outcome = Some(DownloadOutcome::Aborted);
        Ok(())
    }

    fn detach(&mut self) -> Result<()> {
        let Some(child) = self.child.take() else {
            return Ok(());
        };
        let pid = child.id();
        // Drop without killing — the downloader keeps running in the background.
        drop(child);
        let download_dir = self
            .target_path
            .parent()
            .and_then(|p| p.to_str())
            .map(String::from);
        record_detached_download(&self.torrent, pid, download_dir)?;
        Ok(())
    }

    fn is_finished(&self) -> bool {
        self.outcome.is_some()
    }

    fn is_managed_active(&self) -> bool {
        matches!(self.tracking, DownloadTracking::Managed) && !self.is_finished()
    }

    fn detached_key(&self) -> Option<(u32, u64)> {
        match self.tracking {
            DownloadTracking::Managed => None,
            DownloadTracking::Detached { key } => Some(key),
            DownloadTracking::History => None,
        }
    }

    fn should_sync_history(&self) -> bool {
        matches!(self.tracking, DownloadTracking::Managed)
            && matches!(self.outcome, Some(DownloadOutcome::Success))
            && !self.history_synced
    }

    fn mark_history_synced(&mut self) {
        self.history_synced = true;
    }

    fn as_tracked_download(&self) -> TrackedDownload {
        TrackedDownload {
            info_hash: self.torrent.info_hash.clone(),
            name: self.torrent.name.clone(),
            target_path: self.target_path.clone(),
            downloader: match self.backend {
                SessionBackend::Transmission => crate::model::DownloaderKind::Transmission,
                SessionBackend::Aria2 => crate::model::DownloaderKind::Aria2,
                SessionBackend::External => crate::model::DownloaderKind::System,
            },
            percent_done: self
                .progress
                .map(|progress| (progress * 100.0).round() as u8)
                .unwrap_or(100),
            completed: matches!(self.outcome, Some(DownloadOutcome::Success)),
        }
    }

    fn progress_ratio(&self) -> f64 {
        if let Some(progress) = self.progress {
            progress
        } else if matches!(self.outcome, Some(DownloadOutcome::Success)) {
            1.0
        } else {
            0.0
        }
    }

    fn progress_summary(&self) -> String {
        if let Some(progress) = self.progress {
            format!("{:>5.1}%", progress * 100.0)
        } else if matches!(self.tracking, DownloadTracking::Detached { .. }) {
            " ext ".to_string()
        } else if matches!(self.tracking, DownloadTracking::History) {
            "hist ".to_string()
        } else if matches!(self.outcome, Some(DownloadOutcome::Success)) {
            "100.0%".to_string()
        } else {
            " meta ".to_string()
        }
    }

    fn status_badge(&self) -> Span<'static> {
        if matches!(self.tracking, DownloadTracking::Detached { .. }) {
            return Span::styled(" ext ", Style::default().fg(Color::Black).bg(Color::Blue));
        }
        if matches!(self.tracking, DownloadTracking::History) {
            return Span::styled(" hist ", Style::default().fg(Color::Black).bg(Color::Green));
        }

        match self.outcome {
            Some(DownloadOutcome::Success) => {
                Span::styled(" done ", Style::default().fg(Color::Black).bg(Color::Green))
            }
            Some(DownloadOutcome::Failed) => {
                Span::styled(" fail ", Style::default().fg(Color::White).bg(Color::Red))
            }
            Some(DownloadOutcome::Aborted) => Span::styled(
                " stop ",
                Style::default().fg(Color::Black).bg(Color::Yellow),
            ),
            None => Span::styled(
                self.progress_summary(),
                Style::default().fg(Color::Black).bg(Color::Cyan),
            ),
        }
    }

    fn push_log(&mut self, line: String) {
        if self.logs.len() == MAX_LOG_LINES {
            self.logs.pop_front();
        }
        self.logs.push_back(line);
    }
}

#[derive(Clone, Copy)]
enum SessionBackend {
    Transmission,
    Aria2,
    External,
}

impl SessionBackend {
    fn display_name(&self) -> &'static str {
        match self {
            Self::Transmission => "transmission-cli",
            Self::Aria2 => "aria2c",
            Self::External => "external downloader",
        }
    }
}

enum DownloadOutcome {
    Success,
    Failed,
    Aborted,
}

enum DownloadTracking {
    Managed,
    Detached { key: (u32, u64) },
    History,
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

fn spawn_aria2_cli(
    torrent: &Torrent,
    config: &Aria2Config,
) -> Result<(Child, Receiver<DownloadEvent>)> {
    ensure_aria2_available()?;

    let mut command = build_aria2_tui_command(torrent, config);

    let mut child = command.spawn().context("failed to start aria2c")?;
    let stdout = child
        .stdout
        .take()
        .context("failed to capture aria2c stdout")?;
    let stderr = child
        .stderr
        .take()
        .context("failed to capture aria2c stderr")?;

    let (sender, receiver) = mpsc::channel();
    spawn_reader(stdout, sender.clone(), "");
    spawn_reader(stderr, sender, "");

    Ok((child, receiver))
}

fn build_aria2_tui_command(torrent: &Torrent, config: &Aria2Config) -> Command {
    let mut command = new_aria2_tui_command();
    command.stdin(Stdio::null());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    command.arg("--seed-time=0");
    command.arg("--summary-interval=5");
    command.arg("--show-console-readout=true");
    command.arg("--truncate-console-readout=false");
    command.arg("--download-result=hide");
    command.arg("--enable-color=false");
    command.arg("--console-log-level=error");
    command.arg("--bt-max-peers=30");
    command.arg("--file-allocation=none");
    if let Some(download_dir) = &config.download_dir {
        command.arg("--dir").arg(download_dir);
    }
    command.arg(torrent.resolved_magnet());
    command
}

#[cfg(unix)]
fn new_aria2_tui_command() -> Command {
    let mut command = Command::new("nice");
    command.arg("-n").arg("10").arg("aria2c");
    command
}

#[cfg(not(unix))]
fn new_aria2_tui_command() -> Command {
    Command::new("aria2c")
}

fn spawn_reader<R>(stream: R, sender: Sender<DownloadEvent>, prefix: &'static str)
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        let mut buffer = Vec::new();
        let mut chunk = [0_u8; 8192];

        loop {
            match reader.read(&mut chunk) {
                Ok(0) => {
                    emit_buffer(&buffer, &sender, prefix);
                    break;
                }
                Ok(bytes_read) => {
                    for line in drain_output_chunk(&mut buffer, &chunk[..bytes_read], prefix) {
                        let _ = sender.send(DownloadEvent::Output(line));
                    }
                }
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

fn drain_output_chunk(buffer: &mut Vec<u8>, chunk: &[u8], prefix: &str) -> Vec<String> {
    let mut lines = Vec::new();
    for &byte in chunk {
        match byte {
            b'\n' | b'\r' => {
                if let Some(line) = format_output_buffer(buffer, prefix) {
                    lines.push(line);
                }
                buffer.clear();
            }
            value => buffer.push(value),
        }
    }
    lines
}

fn emit_buffer(buffer: &[u8], sender: &Sender<DownloadEvent>, prefix: &str) {
    if let Some(line) = format_output_buffer(buffer, prefix) {
        let _ = sender.send(DownloadEvent::Output(line));
    }
}

fn format_output_buffer(buffer: &[u8], prefix: &str) -> Option<String> {
    if buffer.is_empty() {
        return None;
    }
    let text = String::from_utf8_lossy(buffer);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    Some(format!("{prefix}{trimmed}"))
}

fn parse_transmission_progress(line: &str) -> Option<f64> {
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

struct Aria2Update {
    status: String,
    progress: Option<f64>,
    context: Option<String>,
}

fn parse_aria2_update(line: &str, current_context: Option<&str>) -> Option<Aria2Update> {
    let trimmed = line.trim();
    if trimmed.is_empty() || is_aria2_noise(trimmed) {
        return None;
    }

    if let Some(context) = parse_aria2_context(trimmed) {
        return Some(Aria2Update {
            status: format!("metadata | {}", truncate_middle(&context, 26)),
            progress: None,
            context: Some(context),
        });
    }

    if let Some(progress) = parse_aria2_progress_line(trimmed) {
        return Some(Aria2Update {
            status: render_aria2_status(&progress, current_context),
            progress: progress.ratio,
            context: None,
        });
    }

    None
}

fn is_aria2_noise(line: &str) -> bool {
    line.starts_with("*** Download Progress Summary")
        || line.chars().all(|character| matches!(character, '=' | '-'))
        || line.contains("Failed to load DHT routing table")
        || line.contains("Exception caught while loading DHT routing table")
}

fn parse_aria2_context(line: &str) -> Option<String> {
    let value = line.strip_prefix("FILE: ")?.trim();
    Some(
        value
            .strip_prefix("[MEMORY][METADATA]")
            .unwrap_or(value)
            .trim()
            .to_string(),
    )
}

struct ParsedAria2Progress {
    ratio: Option<f64>,
    peers: Option<String>,
    seeds: Option<String>,
    download_speed: Option<String>,
    upload_speed: Option<String>,
    eta: Option<String>,
}

fn parse_aria2_progress_line(line: &str) -> Option<ParsedAria2Progress> {
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

    let mut progress = ParsedAria2Progress {
        ratio: calculate_size_ratio(complete, total),
        peers: None,
        seeds: None,
        download_speed: None,
        upload_speed: None,
        eta: None,
    };

    for field in fields {
        if let Some(value) = field.strip_prefix("CN:") {
            progress.peers = Some(value.to_string());
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

fn render_aria2_status(progress: &ParsedAria2Progress, current_context: Option<&str>) -> String {
    let mut parts = Vec::new();
    if let Some(ratio) = progress.ratio {
        parts.push(format!("{:>5.1}%", ratio * 100.0));
    } else if let Some(context) = current_context {
        parts.push(format!("meta {}", truncate_middle(context, 18)));
    } else {
        parts.push("metadata".to_string());
    }
    if let Some(peers) = &progress.peers {
        parts.push(format!("peers {peers}"));
    }
    if let Some(seeds) = &progress.seeds {
        parts.push(format!("seeds {seeds}"));
    }
    if let Some(download_speed) = &progress.download_speed {
        parts.push(format!("down {download_speed}/s"));
    }
    if let Some(upload_speed) = &progress.upload_speed {
        parts.push(format!("up {upload_speed}/s"));
    }
    if let Some(eta) = &progress.eta {
        parts.push(format!("eta {eta}"));
    }
    parts.join(" | ")
}

fn calculate_size_ratio(complete: &str, total: &str) -> Option<f64> {
    let complete = parse_aria2_size_to_bytes(complete)?;
    let total = parse_aria2_size_to_bytes(total)?;
    if total == 0 {
        return None;
    }
    Some((complete as f64 / total as f64).clamp(0.0, 1.0))
}

fn parse_aria2_size_to_bytes(value: &str) -> Option<u64> {
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

fn truncate_end(value: &str, max_chars: usize) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= max_chars {
        return value.to_string();
    }
    let truncated: String = chars.iter().take(max_chars.saturating_sub(1)).collect();
    format!("{truncated}…")
}

fn focus_badge(label: &'static str, active: bool) -> Span<'static> {
    if active {
        Span::styled(
            format!(" {label} "),
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(
            format!(" {label} "),
            Style::default().fg(Color::Gray).bg(Color::DarkGray),
        )
    }
}

fn status_span(status: Option<&str>) -> Span<'static> {
    let value = status.unwrap_or("-").trim().to_ascii_lowercase();
    match value.as_str() {
        "vip" => Span::styled(" vip ", Style::default().fg(Color::Black).bg(Color::Green)),
        "trusted" => Span::styled(
            " trusted ",
            Style::default().fg(Color::Black).bg(Color::Yellow),
        ),
        "-" | "" => Span::styled(" - ", Style::default().fg(Color::DarkGray)),
        other => Span::styled(
            format!(" {other} "),
            Style::default().fg(Color::White).bg(Color::Blue),
        ),
    }
}

fn progress_bar(ratio: f64, width: usize) -> String {
    let ratio = ratio.clamp(0.0, 1.0);
    let filled = (ratio * width as f64).round() as usize;
    let empty = width.saturating_sub(filled);
    format!("{}{}", "█".repeat(filled), "░".repeat(empty))
}

fn format_elapsed_duration(duration: Duration) -> String {
    let total_secs = duration.as_secs();
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;

    if hours > 0 {
        format!("{hours}h {minutes}m")
    } else if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{seconds}s")
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use crate::config::Aria2Config;
    use crate::model::Torrent;

    use super::{
        DownloadSession, DownloadTracking, SessionBackend, TICK_RATE, build_aria2_tui_command,
        calculate_size_ratio, drain_output_chunk, format_elapsed_duration, parse_aria2_context,
        parse_aria2_progress_line, parse_transmission_progress,
    };

    #[test]
    fn parses_progress_percentages() {
        assert_eq!(parse_transmission_progress("Progress: 12.5%"), Some(0.125));
        assert_eq!(parse_transmission_progress("99% complete"), Some(0.99));
        assert_eq!(parse_transmission_progress("no percentage here"), None);
    }

    #[test]
    fn parses_aria2_progress() {
        let progress =
            parse_aria2_progress_line("[#167abb 512KiB/10MiB CN:4 SD:8 DL:1.2MiB ETA:8s]")
                .expect("aria2 progress");
        assert_eq!(progress.peers.as_deref(), Some("4"));
        assert_eq!(progress.seeds.as_deref(), Some("8"));
        assert_eq!(progress.download_speed.as_deref(), Some("1.2MiB"));
    }

    #[test]
    fn parses_aria2_context_metadata() {
        assert_eq!(
            parse_aria2_context("FILE: [MEMORY][METADATA]Dune Part Two"),
            Some("Dune Part Two".to_string())
        );
    }

    #[test]
    fn calculates_aria2_size_ratio() {
        assert_eq!(calculate_size_ratio("512KiB", "1MiB"), Some(0.5));
    }

    #[test]
    fn active_download_badge_uses_stable_percentage() {
        let mut download = test_download_session(SessionBackend::Aria2);

        assert_eq!(download.status_badge().content.as_ref(), " meta ");
        assert_eq!(download.status_badge().content.as_ref(), " meta ");


        download.progress = Some(0.425);

        assert_eq!(download.status_badge().content.as_ref(), " 42.5%");
        assert_eq!(download.status_badge().content.as_ref(), " 42.5%");
    }

    #[test]
    fn unknown_progress_is_stable_and_empty() {
        let download = test_download_session(SessionBackend::Aria2);

        assert_eq!(download.progress_ratio(), 0.0);
        assert_eq!(download.progress_ratio(), 0.0);
    }

    #[test]
    fn formats_elapsed_time_for_activity_pane() {
        assert_eq!(format_elapsed_duration(Duration::from_secs(45)), "45s");
        assert_eq!(format_elapsed_duration(Duration::from_secs(125)), "2m 5s");
        assert_eq!(format_elapsed_duration(Duration::from_secs(3_900)), "1h 5m");
    }

    #[test]
    fn tui_tick_rate_is_throttled_to_avoid_busy_redraws() {
        assert!(TICK_RATE >= Duration::from_millis(200));
    }

    #[test]
    fn tui_aria2_command_uses_resource_friendly_progress_output() {
        let torrent = test_torrent("Example");
        let command = build_aria2_tui_command(&torrent, &Aria2Config { download_dir: None });
        let args: Vec<String> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();

        assert!(args.iter().any(|arg| arg == "--summary-interval=5"));
        assert!(args.iter().any(|arg| arg == "--bt-max-peers=30"));
        assert!(args.iter().any(|arg| arg == "--file-allocation=none"));
    }

    #[test]
    fn output_chunks_are_split_on_carriage_returns_and_newlines() {
        let mut buffer = Vec::new();

        assert_eq!(
            drain_output_chunk(&mut buffer, b"first\rsecond\npart", ""),
            vec!["first".to_string(), "second".to_string()]
        );
        assert_eq!(buffer, b"part");
        assert_eq!(
            drain_output_chunk(&mut buffer, b"ial\r", "stderr | "),
            vec!["stderr | partial".to_string()]
        );
        assert!(buffer.is_empty());
    }

    fn test_download_session(backend: SessionBackend) -> DownloadSession {
        DownloadSession {
            torrent: test_torrent("Example"),
            target_path: PathBuf::from("/tmp/Example"),
            backend,
            tracking: DownloadTracking::Managed,
            child: None,
            receiver: None,
            logs: VecDeque::new(),
            progress: None,
            status_text: "Fetching metadata...".to_string(),
            context_text: None,
            started_at: Instant::now(),
            outcome: None,
            history_synced: false,
        }
    }

    fn test_torrent(name: &str) -> Torrent {
        Torrent {
            id: "1".to_string(),
            name: name.to_string(),
            info_hash: format!("abc123{name}"),
            magnet: None,
            seeders: 1,
            leechers: 0,
            size_bytes: 1024,
            status: None,
            uploaded_by: None,
            description: None,
            category: None,
            subcategory: None,
            added: None,
        }
    }
}
