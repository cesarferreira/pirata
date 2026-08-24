use std::collections::VecDeque;
use std::io::{self, BufReader, Read};
use std::path::{Path, PathBuf};
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
use crate::state::{
    DetachedDownloadRecord, load_recent_detached_downloads, record_detached_download,
};
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
    trending: impl FnMut() -> Result<Vec<Torrent>>,
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
        trending,
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

struct SearchTui<F, T, H>
where
    F: FnMut(&str) -> Result<Vec<Torrent>>,
    T: FnMut() -> Result<Vec<Torrent>>,
    H: FnMut(Torrent) -> Result<Torrent>,
{
    query_input: String,
    query: Option<String>,
    results: Vec<Torrent>,
    trending: Vec<Torrent>,
    selected_result: usize,
    selected_trending: usize,
    selected_download: usize,
    downloads: Vec<DownloadSession>,
    backend: TuiDownloader,
    history: DownloadHistory,
    should_quit: bool,
    tick: usize,
    focus: FocusPane,
    status_message: String,
    search: F,
    load_trending: T,
    hydrate: H,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FocusPane {
    Query,
    Results,
    Trending,
    Downloads,
}

impl<F, T, H> SearchTui<F, T, H>
where
    F: FnMut(&str) -> Result<Vec<Torrent>>,
    T: FnMut() -> Result<Vec<Torrent>>,
    H: FnMut(Torrent) -> Result<Torrent>,
{
    fn new(
        initial_query: Option<String>,
        backend: TuiDownloader,
        history_entries: Vec<DownloadHistoryEntry>,
        history_path: PathBuf,
        search: F,
        trending: T,
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
            trending: Vec::new(),
            selected_result: 0,
            selected_trending: 0,
            selected_download: 0,
            downloads,
            backend,
            history: DownloadHistory::new(history_path),
            should_quit: false,
            tick: 0,
            focus: FocusPane::Query,
            status_message:
                "Loading popular releases from the last 48 hours. Type a query and press Enter to search."
                    .to_string(),
            search,
            load_trending: trending,
            hydrate,
        };
        app.refresh_trending()?;
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
                download.refresh_disk_progress();
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
            focus_badge("trending", matches!(self.focus, FocusPane::Trending)),
            Span::raw(" "),
            focus_badge("downloads", matches!(self.focus, FocusPane::Downloads)),
            Span::raw(format!(
                "   backend {}   results {}   trending {}   active {}",
                self.backend.name(),
                self.results.len(),
                self.trending.len(),
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
            .visible_results()
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
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(self.results_title())
                    .border_style(self.results_focus_style()),
            )
            .highlight_style(
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▌ ");
        let mut results_state = ListState::default();
        results_state
            .select((!self.visible_results().is_empty()).then_some(self.selected_visible_result()));
        frame.render_stateful_widget(results, left[1], &mut results_state);

        let download_width = layout[2].width.saturating_sub(2) as usize;
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
                        spans.push(download.eta_span());
                        spans.push(Span::raw("  "));
                        spans.push(Span::styled(
                            download.active_row_primary_text(download_width),
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
                    if !download.is_managed_active() {
                        spans.push(Span::raw("  "));
                        spans.push(Span::raw(download.torrent.name.clone()));
                    }
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
                    "r",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" refresh trending  "),
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
            KeyCode::Char('r') if matches!(self.focus, FocusPane::Trending) => {
                self.refresh_trending()?;
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
            FocusPane::Trending => self.handle_trending_key(key),
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
                    self.start_download(torrent)?;
                }
            }
            _ => {}
        }

        Ok(())
    }

    fn handle_trending_key(&mut self, key: KeyCode) -> Result<()> {
        match key {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected_trending > 0 {
                    self.selected_trending -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.selected_trending + 1 < self.trending.len() {
                    self.selected_trending += 1;
                }
            }
            KeyCode::Enter => {
                if let Some(torrent) = self.trending.get(self.selected_trending).cloned() {
                    self.start_download(torrent)?;
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

    fn refresh_trending(&mut self) -> Result<()> {
        self.status_message = "Refreshing popular releases from the last 48 hours...".to_string();
        match (self.load_trending)() {
            Ok(trending) => {
                self.trending = trending;
                self.selected_trending = 0;
                self.status_message = if self.trending.is_empty() {
                    "No popular releases from the last 48 hours are available right now."
                        .to_string()
                } else {
                    format!(
                        "Loaded {} popular release(s) from the last 48 hours. Press Tab to browse them; r refreshes.",
                        self.trending.len()
                    )
                };
            }
            Err(error) => {
                self.status_message =
                    format!("Could not refresh Popular Now: {error}. Press r to retry.");
            }
        }
        Ok(())
    }

    fn start_download(&mut self, torrent: Torrent) -> Result<()> {
        let torrent = (self.hydrate)(torrent)?;
        self.downloads
            .push(DownloadSession::start(torrent.clone(), &self.backend)?);
        self.selected_download = self.downloads.len().saturating_sub(1);
        self.focus = FocusPane::Downloads;
        self.status_message = format!(
            "Started '{}' with {}. Search or browse popular releases while it runs.",
            torrent.name,
            self.backend.name()
        );
        Ok(())
    }

    fn cycle_focus(&mut self) {
        self.focus = match self.focus {
            FocusPane::Query => FocusPane::Results,
            FocusPane::Results => FocusPane::Trending,
            FocusPane::Trending => FocusPane::Downloads,
            FocusPane::Downloads => FocusPane::Query,
        };
    }

    fn cycle_focus_reverse(&mut self) {
        self.focus = match self.focus {
            FocusPane::Query => FocusPane::Downloads,
            FocusPane::Results => FocusPane::Query,
            FocusPane::Trending => FocusPane::Results,
            FocusPane::Downloads => FocusPane::Trending,
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

    fn visible_results(&self) -> &[Torrent] {
        if matches!(self.focus, FocusPane::Trending) {
            &self.trending
        } else {
            &self.results
        }
    }

    fn selected_visible_result(&self) -> usize {
        if matches!(self.focus, FocusPane::Trending) {
            self.selected_trending
        } else {
            self.selected_result
        }
    }

    fn results_title(&self) -> &'static str {
        if matches!(self.focus, FocusPane::Trending) {
            "Popular Now · Last 48h"
        } else {
            "Results"
        }
    }

    fn results_focus_style(&self) -> Style {
        if matches!(self.focus, FocusPane::Results | FocusPane::Trending) {
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
    last_byte_sample: Option<(Instant, u64)>,
    eta_display: Option<String>,
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
            last_byte_sample: None,
            eta_display: None,
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
            last_byte_sample: None,
            eta_display: None,
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
            last_byte_sample: None,
            eta_display: None,
        }
    }

    fn refresh_disk_progress(&mut self) {
        if !self.is_managed_active() || !matches!(self.backend, SessionBackend::Aria2) {
            return;
        }
        if self.torrent.size_bytes == 0 {
            return;
        }
        let Some(download_dir) = self.target_path.parent() else {
            return;
        };
        if download_dir.as_os_str().is_empty() {
            return;
        }

        let bytes = measure_download_bytes(download_dir, &self.torrent.name);
        if bytes == 0 {
            return;
        }

        let ratio = (bytes as f64 / self.torrent.size_bytes as f64).clamp(0.0, 1.0);
        if self
            .progress
            .is_some_and(|current| current >= ratio - 0.001)
        {
            return;
        }

        self.progress = Some(ratio);

        let mut parts = vec![format!("{:>5.1}%", ratio * 100.0)];
        if let Some((previous_at, previous_bytes)) = self.last_byte_sample {
            let elapsed = previous_at.elapsed().as_secs_f64();
            if elapsed >= 0.5 && bytes > previous_bytes {
                let speed = (bytes - previous_bytes) as f64 / elapsed;
                if speed > 0.0 {
                    parts.push(format!("down {}/s", format_size(speed as u64)));
                    let remaining = self.torrent.size_bytes.saturating_sub(bytes);
                    let eta_secs = (remaining as f64 / speed).round() as u64;
                    parts.push(format!(
                        "eta {}",
                        format_elapsed_duration(Duration::from_secs(eta_secs))
                    ));
                    self.eta_display = Some(format_elapsed_duration(Duration::from_secs(eta_secs)));
                }
            }
        }
        self.last_byte_sample = Some((Instant::now(), bytes));
        self.status_text = parts.join(" | ");
    }

    fn apply_aria2_progress_line(&mut self, line: &str) {
        let Some(progress) = parse_aria2_progress_line(line.trim()) else {
            return;
        };
        let ratio = progress
            .ratio
            .or(self.progress)
            .or_else(|| estimate_progress_from_line(line, self.torrent.size_bytes));
        if let Some(ratio) = progress.ratio {
            self.progress = Some(ratio);
        }
        self.eta_display = resolve_aria2_eta(&progress, ratio, self.torrent.size_bytes);
    }

    fn eta_display_label(&self) -> String {
        if let Some(eta) = &self.eta_display {
            return eta.clone();
        }
        if self.is_metadata_phase() {
            "meta".to_string()
        } else {
            "—".to_string()
        }
    }

    fn is_metadata_phase(&self) -> bool {
        if self.eta_display.is_some() {
            return false;
        }
        if self.progress.unwrap_or(0.0) >= 0.01 {
            return false;
        }
        if self.context_text.is_some() {
            return true;
        }
        let status = self.status_text.as_str();
        status.starts_with("metadata |") || status.starts_with("Fetching metadata")
    }

    fn active_row_prefix_chars(&self) -> usize {
        char_len(&self.progress_summary())
            + 1
            + 10
            + 2
            + char_len(&format_elapsed_duration(self.started_at.elapsed()))
            + 2
            + char_len(&format!("ETA {}", self.eta_display_label()))
            + 2
    }

    fn active_row_primary_text(&self, row_width: usize) -> String {
        let budget = row_width.saturating_sub(self.active_row_prefix_chars());
        if budget <= 4 {
            return truncate_end(&self.torrent.name, budget);
        }

        if self.is_metadata_phase() {
            return truncate_end(&self.torrent.name, budget);
        }

        if let Some(snippet) = self.transfer_snippet() {
            let suffix = format!("  | {snippet}");
            let suffix_len = char_len(&suffix);
            if budget > suffix_len + 8 {
                let title_len = budget - suffix_len;
                return format!("{}{}", truncate_end(&self.torrent.name, title_len), suffix);
            }
        }

        truncate_end(&self.torrent.name, budget)
    }

    fn transfer_snippet(&self) -> Option<String> {
        if self.is_metadata_phase() {
            return None;
        }

        let compact = compact_transfer_status(&self.status_text);
        if compact.is_empty() {
            None
        } else {
            Some(compact)
        }
    }

    fn eta_span(&self) -> Span<'static> {
        let eta = self.eta_display_label();
        Span::styled(
            format!("ETA {eta}"),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
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
                                } else if let Some(progress) =
                                    estimate_progress_from_line(&line, self.torrent.size_bytes)
                                {
                                    self.progress = Some(progress);
                                }
                                if let Some(context) = update.context {
                                    self.context_text = Some(context);
                                }
                                if !update.status.is_empty() {
                                    self.status_text = update.status;
                                }
                            } else if let Some(progress) =
                                estimate_progress_from_line(&line, self.torrent.size_bytes)
                            {
                                self.progress = Some(progress);
                            } else {
                                continue;
                            }
                            self.apply_aria2_progress_line(&line);
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

    #[cfg(unix)]
    {
        return spawn_aria2_cli_with_pty(torrent, config);
    }

    #[cfg(not(unix))]
    {
        spawn_aria2_cli_piped(torrent, config)
    }
}

#[cfg(unix)]
fn spawn_aria2_cli_with_pty(
    torrent: &Torrent,
    config: &Aria2Config,
) -> Result<(Child, Receiver<DownloadEvent>)> {
    use std::fs::File;

    use nix::pty::{Winsize, openpty};

    let winsize = Winsize {
        ws_row: 40,
        ws_col: 120,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let openpty_result =
        openpty(Some(&winsize), None).context("failed to create pseudo-terminal for aria2c")?;
    let master = openpty_result.master;
    let slave = openpty_result.slave;
    let slave_stdout = slave
        .try_clone()
        .context("failed to duplicate aria2c pty stdout")?;
    let slave_stderr = slave
        .try_clone()
        .context("failed to duplicate aria2c pty stderr")?;

    let mut command = build_aria2_tui_command(torrent, config);
    command.stdin(Stdio::from(slave));
    command.stdout(Stdio::from(slave_stdout));
    command.stderr(Stdio::from(slave_stderr));

    let child = command.spawn().context("failed to start aria2c")?;

    let master_file = File::from(master);
    let (sender, receiver) = mpsc::channel();
    spawn_reader(master_file, sender, "");

    Ok((child, receiver))
}

#[cfg(not(unix))]
fn spawn_aria2_cli_piped(
    torrent: &Torrent,
    config: &Aria2Config,
) -> Result<(Child, Receiver<DownloadEvent>)> {
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
    command.arg("--summary-interval=1");
    command.arg("--show-console-readout=true");
    command.arg("--truncate-console-readout=false");
    command.arg("--download-result=hide");
    command.arg("--enable-color=false");
    command.arg("--console-log-level=error");
    command.arg("--human-readable=false");
    command.arg("--bt-max-peers=30");
    command.arg("--file-allocation=none");
    if let Some(download_dir) = config.download_dir_path() {
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

enum Aria2ContextLine {
    Metadata(String),
    File(String),
}

fn parse_aria2_context_line(line: &str) -> Option<Aria2ContextLine> {
    let value = line.strip_prefix("FILE: ")?.trim();
    if let Some(name) = value.strip_prefix("[MEMORY][METADATA]") {
        return Some(Aria2ContextLine::Metadata(name.trim().to_string()));
    }
    Some(Aria2ContextLine::File(value.to_string()))
}

fn parse_aria2_update(line: &str, current_context: Option<&str>) -> Option<Aria2Update> {
    let trimmed = line.trim();
    if trimmed.is_empty() || is_aria2_noise(trimmed) {
        return None;
    }

    if let Some(context) = parse_aria2_context_line(trimmed) {
        return match context {
            Aria2ContextLine::Metadata(name) => Some(Aria2Update {
                status: truncate_middle(&name, 36),
                progress: None,
                context: Some(name),
            }),
            Aria2ContextLine::File(path) => {
                let name = path
                    .rsplit(['/', '\\'])
                    .next()
                    .unwrap_or(path.as_str())
                    .to_string();
                Some(Aria2Update {
                    status: String::new(),
                    progress: None,
                    context: Some(name),
                })
            }
        };
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

fn compact_transfer_status(status: &str) -> String {
    status
        .split(" | ")
        .map(str::trim)
        .filter(|part| {
            !part.is_empty()
                && !part.starts_with("metadata")
                && !part.starts_with("meta ")
                && !part.starts_with("eta ")
                && !part.ends_with('%')
                && *part != "metadata"
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

fn char_len(value: &str) -> usize {
    value.chars().count()
}

fn estimate_progress_from_line(line: &str, total_bytes: u64) -> Option<f64> {
    if total_bytes == 0 {
        return None;
    }
    let trimmed = line.trim();
    if !trimmed.starts_with("[#") {
        return None;
    }

    let body = trimmed
        .trim_start_matches("[#")
        .strip_suffix(']')
        .unwrap_or(trimmed)
        .trim();
    let mut fields = body.split_whitespace();
    fields.next()?;
    let transfer = fields.next()?;
    let (complete, total) = transfer.split_once('/')?;
    if let Some(ratio) = calculate_size_ratio(complete, total) {
        return Some(ratio);
    }
    let complete_bytes = parse_aria2_size_to_bytes(complete)?;
    Some((complete_bytes as f64 / total_bytes as f64).clamp(0.0, 1.0))
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

fn resolve_aria2_eta(
    progress: &ParsedAria2Progress,
    ratio: Option<f64>,
    total_bytes: u64,
) -> Option<String> {
    if let Some(eta) = progress.eta.as_deref() {
        let trimmed = eta.trim();
        if !trimmed.is_empty() && !matches!(trimmed, "-" | "--" | "∞" | "INF" | "inf") {
            return Some(trimmed.to_string());
        }
    }

    let speed_str = progress.download_speed.as_deref()?;
    let speed = parse_aria2_size_to_bytes(speed_str.trim_end_matches("/s"))?;
    if speed == 0 || total_bytes == 0 {
        return None;
    }

    let ratio = ratio.unwrap_or(0.0);
    let remaining = (total_bytes as f64 * (1.0 - ratio).clamp(0.0, 1.0)).round() as u64;
    if remaining == 0 {
        return Some("0s".to_string());
    }

    Some(format_elapsed_duration(Duration::from_secs(
        ((remaining as f64 / speed as f64).round() as u64).max(1),
    )))
}

fn render_aria2_status(progress: &ParsedAria2Progress, current_context: Option<&str>) -> String {
    let mut parts = Vec::new();
    if let Some(ratio) = progress.ratio.filter(|ratio| *ratio >= 0.001) {
        parts.push(format!("{:>5.1}%", ratio * 100.0));
    } else if let Some(context) = current_context.filter(|_| progress.ratio.is_none()) {
        parts.push(truncate_middle(context, 22));
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
        "K" | "k" => 1024.0,
        "Ki" | "KiB" => 1024.0,
        "M" | "Mi" | "MiB" => 1024.0 * 1024.0,
        "G" | "Gi" | "GiB" => 1024.0 * 1024.0 * 1024.0,
        "T" | "Ti" | "TiB" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        "KB" => 1000.0,
        "MB" => 1_000_000.0,
        "GB" => 1_000_000_000.0,
        "TB" => 1_000_000_000_000.0,
        _ => return None,
    };

    Some((amount * multiplier).round() as u64)
}

fn measure_download_bytes(download_dir: &Path, torrent_name: &str) -> u64 {
    let direct = download_dir.join(torrent_name);
    if direct.is_file() {
        return direct.metadata().map(|meta| meta.len()).unwrap_or(0);
    }

    let nested = download_dir.join(torrent_name);
    if nested.is_dir() {
        return directory_file_bytes(&nested);
    }

    let Ok(entries) = std::fs::read_dir(download_dir) else {
        return 0;
    };

    let mut total = 0_u64;
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        if name.ends_with(".aria2") {
            continue;
        }
        if name == torrent_name || name.starts_with(&format!("{torrent_name}.")) {
            if let Ok(meta) = entry.metadata() {
                if meta.is_file() {
                    total += meta.len();
                } else if meta.is_dir() {
                    total += directory_file_bytes(&entry.path());
                }
            }
        }
    }

    if total > 0 {
        return total;
    }

    for entry in std::fs::read_dir(download_dir)
        .into_iter()
        .flatten()
        .flatten()
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.with_extension("aria2").exists() {
            total += entry.metadata().map(|meta| meta.len()).unwrap_or(0);
        }
    }

    total
}

fn directory_file_bytes(path: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };

    entries
        .flatten()
        .filter_map(|entry| entry.metadata().ok())
        .filter(|meta| meta.is_file())
        .map(|meta| meta.len())
        .sum()
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
    use std::cell::Cell;
    use std::collections::VecDeque;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use crate::config::Aria2Config;
    use crate::model::Torrent;
    use crossterm::event::KeyCode;

    use super::{
        Aria2ContextLine, DownloadSession, DownloadTracking, FocusPane, SearchTui, SessionBackend,
        TICK_RATE, build_aria2_tui_command, calculate_size_ratio, compact_transfer_status,
        drain_output_chunk, format_elapsed_duration, parse_aria2_context_line,
        parse_aria2_progress_line, parse_transmission_progress, resolve_aria2_eta,
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
        match parse_aria2_context_line("FILE: [MEMORY][METADATA]Dune Part Two").unwrap() {
            Aria2ContextLine::Metadata(name) => assert_eq!(name, "Dune Part Two"),
            Aria2ContextLine::File(_) => panic!("expected metadata context"),
        }
    }

    #[test]
    fn compact_transfer_status_drops_metadata_noise() {
        assert_eq!(
            compact_transfer_status("peers 4 | down 1MiB/s | eta 8s"),
            "peers 4 | down 1MiB/s"
        );
    }

    #[test]
    fn resolves_aria2_eta_from_speed_and_size() {
        let progress =
            parse_aria2_progress_line("[#abcd 2MiB/8MiB CN:4 DL:1MiB]").expect("progress line");
        assert_eq!(
            resolve_aria2_eta(&progress, progress.ratio, 8 * 1024 * 1024).as_deref(),
            Some("6s")
        );
    }

    #[test]
    fn calculates_aria2_size_ratio() {
        assert_eq!(calculate_size_ratio("512KiB", "1MiB"), Some(0.5));
        assert_eq!(calculate_size_ratio("512Ki", "10Mi"), Some(0.05));
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
    fn loads_and_focuses_live_trending_view() {
        let trending_torrent = test_torrent("Trending example");
        let mut tui = SearchTui::new(
            None,
            super::TuiDownloader::Aria2(Aria2Config { download_dir: None }),
            Vec::new(),
            PathBuf::from("/tmp/pirata-tui-test-history.json"),
            |_| Ok(Vec::new()),
            || Ok(vec![trending_torrent.clone()]),
            Ok,
        )
        .expect("TUI should initialize");

        assert_eq!(tui.trending.len(), 1);
        assert_eq!(tui.results_title(), "Results");
        tui.cycle_focus();
        tui.cycle_focus();
        assert_eq!(tui.focus, FocusPane::Trending);
        assert_eq!(tui.results_title(), "Popular Now · Last 48h");
    }

    #[test]
    fn enter_on_an_empty_query_does_not_run_a_search() {
        let search_calls = Cell::new(0);
        let mut tui = SearchTui::new(
            None,
            super::TuiDownloader::Aria2(Aria2Config { download_dir: None }),
            Vec::new(),
            PathBuf::from("/tmp/pirata-tui-test-history.json"),
            |_| {
                search_calls.set(search_calls.get() + 1);
                Ok(Vec::new())
            },
            || Ok(Vec::new()),
            Ok,
        )
        .expect("TUI should initialize");

        tui.handle_key(KeyCode::Enter)
            .expect("empty query should be handled");

        assert_eq!(search_calls.get(), 0);
        assert_eq!(tui.focus, FocusPane::Query);
        assert_eq!(tui.status_message, "Enter a query before searching.");
    }

    #[test]
    fn tui_aria2_command_uses_resource_friendly_progress_output() {
        let torrent = test_torrent("Example");
        let command = build_aria2_tui_command(&torrent, &Aria2Config { download_dir: None });
        let args: Vec<String> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();

        assert!(args.iter().any(|arg| arg == "--summary-interval=1"));
        assert!(args.iter().any(|arg| arg == "--human-readable=false"));
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
            last_byte_sample: None,
            eta_display: None,
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
