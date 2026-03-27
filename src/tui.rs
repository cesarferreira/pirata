use std::collections::VecDeque;
use std::io::{self, BufReader, Read};
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
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};

use crate::config::TransmissionConfig;
use crate::model::Torrent;
use crate::state::{DetachedDownloadRecord, load_recent_detached_downloads};
use crate::util::{ensure_transmission_cli_available, format_size};

const MAX_LOG_LINES: usize = 14;
const TICK_RATE: Duration = Duration::from_millis(100);

pub fn run_search_tui<F>(
    initial_query: Option<String>,
    transmission: TransmissionConfig,
    search: F,
) -> Result<()>
where
    F: FnMut(&str) -> Result<Vec<Torrent>>,
{
    ensure_transmission_cli_available()?;

    let mut terminal = setup_terminal()?;
    let mut app = SearchTui::new(initial_query, transmission, search)?;
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

struct SearchTui<F>
where
    F: FnMut(&str) -> Result<Vec<Torrent>>,
{
    query_input: String,
    query: Option<String>,
    results: Vec<Torrent>,
    selected_result: usize,
    selected_download: usize,
    downloads: Vec<DownloadSession>,
    transmission: TransmissionConfig,
    should_quit: bool,
    tick: usize,
    focus: FocusPane,
    status_message: String,
    search: F,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FocusPane {
    Query,
    Results,
    Downloads,
}

impl<F> SearchTui<F>
where
    F: FnMut(&str) -> Result<Vec<Torrent>>,
{
    fn new(initial_query: Option<String>, transmission: TransmissionConfig, search: F) -> Result<Self> {
        let detached_downloads = load_recent_detached_downloads(24)?
            .into_iter()
            .map(DownloadSession::from_detached_record)
            .collect();
        let mut app = Self {
            query_input: initial_query.unwrap_or_default(),
            query: None,
            results: Vec::new(),
            selected_result: 0,
            selected_download: 0,
            downloads: detached_downloads,
            transmission,
            should_quit: false,
            tick: 0,
            focus: FocusPane::Query,
            status_message: "Type a query and press Enter to search. Detached CLI launches appear below.".to_string(),
            search,
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
            if self.tick % 10 == 0 {
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
            self.downloads.push(DownloadSession::from_detached_record(record));
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
            Span::styled(" pirate-ctl ", Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw("  "),
            focus_badge("query", matches!(self.focus, FocusPane::Query)),
            Span::raw(" "),
            focus_badge("results", matches!(self.focus, FocusPane::Results)),
            Span::raw(" "),
            focus_badge("downloads", matches!(self.focus, FocusPane::Downloads)),
            Span::raw(format!(
                "   results {}   active {}",
                self.results.len(),
                self.active_downloads()
            )),
        ]))
        .block(Block::default().borders(Borders::ALL).title("Dashboard"))
        .style(Style::default().fg(Color::White));
        frame.render_widget(header, layout[0]);

        let top = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(56),
                Constraint::Percentage(44),
            ])
            .split(layout[1]);
        let left = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(8)])
            .split(top[0]);

        let query_block = Block::default()
            .borders(Borders::ALL)
            .title("Search Query")
            .border_style(self.focus_style(FocusPane::Query));
        let query = Paragraph::new(self.query_input.as_str())
            .block(query_block)
            .style(Style::default().fg(Color::White));
        frame.render_widget(query, left[0]);
        if matches!(self.focus, FocusPane::Query) {
            let cursor_x = left[0].x.saturating_add(1 + self.query_input.chars().count() as u16);
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
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled("seed ", Style::default().fg(Color::DarkGray)),
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

        let details = self.result_details_lines();
        let detail_widget = Paragraph::new(details)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Selection")
                    .border_style(self.focus_style(FocusPane::Results)),
            )
            .wrap(Wrap { trim: true });
        frame.render_widget(detail_widget, top[1]);

        let bottom = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
            .split(layout[2]);

        let download_items: Vec<ListItem<'_>> = if self.downloads.is_empty() {
            vec![ListItem::new(Line::from(vec![Span::styled(
                "No downloads yet. Press Enter on a result to start one.",
                Style::default().fg(Color::DarkGray),
            )]))]
        } else {
            self.downloads
                .iter()
                .map(|download| {
                    let line = Line::from(vec![
                        download.status_badge(self.tick),
                        Span::raw(" "),
                        Span::styled(download.progress_summary(self.tick), Style::default().fg(Color::LightBlue)),
                        Span::raw(" "),
                        Span::raw(download.torrent.name.clone()),
                    ]);
                    ListItem::new(line)
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
        frame.render_stateful_widget(downloads, bottom[0], &mut downloads_state);

        let download_detail_widget = Paragraph::new(self.download_details_lines(self.tick))
            .block(Block::default().borders(Borders::ALL).title("Activity"))
            .wrap(Wrap { trim: true });
        frame.render_widget(download_detail_widget, bottom[1]);

        let footer = Paragraph::new(vec![
            Line::from(vec![
                Span::styled("Tab", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::raw(" focus  "),
                Span::styled("Enter", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::raw(" search/start  "),
                Span::styled("/", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::raw(" edit query  "),
                Span::styled("d", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::raw(" abort download  "),
                Span::styled("q", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::raw(" stop active + quit  "),
                Span::styled("Q", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
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
                self.status_message = "Type a new query and press Enter to search again.".to_string();
                return Ok(());
            }
            KeyCode::Char('Q') => {
                self.abort_all_downloads()?;
                self.should_quit = true;
                return Ok(());
            }
            KeyCode::Char('q') | KeyCode::Esc => {
                if self.active_downloads() > 0 {
                    self.abort_all_downloads()?;
                }
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
                    self.downloads
                        .push(DownloadSession::start(torrent.clone(), &self.transmission)?);
                    self.selected_download = self.downloads.len().saturating_sub(1);
                    self.focus = FocusPane::Downloads;
                    self.status_message = format!(
                        "Started '{}' with transmission-cli. Search again while it runs.",
                        torrent.name
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
                    } else if matches!(download.tracking, DownloadTracking::Detached { .. }) {
                        self.status_message =
                            "That row was started outside the TUI and cannot be stopped here."
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

    fn focus_style(&self, pane: FocusPane) -> Style {
        if self.focus == pane {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        }
    }

    fn result_details_lines(&self) -> Vec<Line<'static>> {
        if let Some(selected) = self.results.get(self.selected_result) {
            vec![
                Line::from(vec![
                    Span::styled("Name ", Style::default().fg(Color::DarkGray)),
                    Span::raw(selected.name.clone()),
                ]),
                Line::from(vec![
                    Span::styled("ID ", Style::default().fg(Color::DarkGray)),
                    Span::styled(selected.id.clone(), Style::default().fg(Color::Yellow)),
                ]),
                Line::from(vec![
                    Span::styled("Seeders ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        selected.seeders.to_string(),
                        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                    ),
                    Span::raw("   "),
                    Span::styled("Leechers ", Style::default().fg(Color::DarkGray)),
                    Span::styled(selected.leechers.to_string(), Style::default().fg(Color::Red)),
                ]),
                Line::from(vec![
                    Span::styled("Size ", Style::default().fg(Color::DarkGray)),
                    Span::styled(format_size(selected.size_bytes), Style::default().fg(Color::Cyan)),
                ]),
                Line::from(vec![
                    Span::styled("Status ", Style::default().fg(Color::DarkGray)),
                    status_span(selected.status.as_deref()),
                ]),
                Line::from(vec![
                    Span::styled("Uploader ", Style::default().fg(Color::DarkGray)),
                    Span::raw(selected.uploaded_by.clone().unwrap_or_else(|| "-".to_string())),
                ]),
                Line::from(""),
                Line::from("Enter starts a tracked download in this TUI session."),
                Line::from("Detached downloads started elsewhere also appear below, without live progress."),
            ]
        } else if let Some(query) = &self.query {
            vec![
                Line::from(format!("No results loaded for '{query}'.")),
                Line::from("Edit the query and press Enter to try again."),
            ]
        } else {
            vec![
                Line::from("Start by typing a search query."),
                Line::from("The results list and downloads panel stay visible together."),
            ]
        }
    }

    fn download_details_lines(&self, tick: usize) -> Vec<Line<'static>> {
        let Some(download) = self.downloads.get(self.selected_download) else {
            return vec![
                Line::from("No downloads yet."),
                Line::from("Select a result and press Enter to start one."),
            ];
        };

        let mut lines = vec![
            Line::from(vec![
                Span::styled("Torrent ", Style::default().fg(Color::DarkGray)),
                Span::raw(download.torrent.name.clone()),
            ]),
            Line::from(vec![
                Span::styled("Progress ", Style::default().fg(Color::DarkGray)),
                Span::styled(download.progress_summary(tick), Style::default().fg(Color::LightBlue).add_modifier(Modifier::BOLD)),
                Span::raw("  "),
                Span::raw(download.progress_label(tick)),
            ]),
            Line::from(vec![
                Span::styled("Size ", Style::default().fg(Color::DarkGray)),
                Span::raw(format_size(download.torrent.size_bytes)),
                Span::raw("  "),
                Span::styled("Seeders ", Style::default().fg(Color::DarkGray)),
                Span::raw(download.torrent.seeders.to_string()),
                Span::raw("  "),
                Span::styled("Elapsed ", Style::default().fg(Color::DarkGray)),
                Span::raw(format!("{}s", download.started_at.elapsed().as_secs())),
            ]),
            Line::from(vec![
                Span::styled("Gauge ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    progress_bar(download.progress_ratio(tick), 24),
                    Style::default().fg(if download.is_finished() {
                        Color::Green
                    } else {
                        Color::Cyan
                    }),
                ),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "Latest output",
                Style::default().add_modifier(Modifier::BOLD).fg(Color::Yellow),
            )),
        ];

        let logs = download.logs_for_render();
        if logs.is_empty() {
            lines.push(Line::from("No output yet."));
        } else {
            lines.extend(logs.into_iter().rev().take(5).rev());
        }

        lines
    }
}

struct DownloadSession {
    torrent: Torrent,
    tracking: DownloadTracking,
    child: Option<Child>,
    receiver: Option<Receiver<DownloadEvent>>,
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
        logs.push_back(format!(
            "Downloading to {}",
            config.download_target_display()
        ));

        Ok(Self {
            torrent,
            tracking: DownloadTracking::Managed,
            child: Some(child),
            receiver: Some(receiver),
            logs,
            progress: None,
            status_text: "Connecting to peers...".to_string(),
            started_at: Instant::now(),
            outcome: None,
        })
    }

    fn from_detached_record(record: DetachedDownloadRecord) -> Self {
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
            torrent: record.torrent,
            tracking: DownloadTracking::Detached { key },
            child: None,
            receiver: None,
            logs,
            progress: None,
            status_text: "Detached CLI download. Progress cannot be attached after launch.".to_string(),
            started_at: Instant::now() - Duration::from_secs(elapsed),
            outcome: None,
        }
    }

    fn drain_events(&mut self) {
        while let Some(event) = self.receiver.as_ref().and_then(|receiver| receiver.try_recv().ok()) {
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
        if matches!(self.tracking, DownloadTracking::Detached { .. }) {
            self.status_text = "Detached download cannot be controlled from this TUI.".to_string();
            self.push_log("This row was loaded from saved state and cannot be aborted here.".to_string());
            return Ok(());
        }
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

    fn is_managed_active(&self) -> bool {
        matches!(self.tracking, DownloadTracking::Managed) && !self.is_finished()
    }

    fn detached_key(&self) -> Option<(u32, u64)> {
        match self.tracking {
            DownloadTracking::Managed => None,
            DownloadTracking::Detached { key } => Some(key),
        }
    }

    fn progress_ratio(&self, tick: usize) -> f64 {
        if let Some(progress) = self.progress {
            progress
        } else if matches!(self.tracking, DownloadTracking::Detached { .. }) {
            0.18
        } else if self.is_finished() {
            1.0
        } else {
            let phase = (tick % 20) as f64 / 20.0;
            0.12 + if phase <= 0.5 {
                phase * 0.36
            } else {
                (1.0 - phase) * 0.36
            }
        }
    }

    fn progress_label(&self, tick: usize) -> String {
        if let Some(progress) = self.progress {
            format!("{:.1}% | {}", progress * 100.0, self.status_text)
        } else if matches!(self.tracking, DownloadTracking::Detached { .. }) {
            self.status_text.clone()
        } else if self.is_finished() {
            self.status_text.clone()
        } else {
            let spinner = ["|", "/", "-", "\\"];
            format!(
                "{} estimating | {}",
                spinner[tick % spinner.len()],
                self.status_text
            )
        }
    }

    fn progress_summary(&self, tick: usize) -> String {
        if let Some(progress) = self.progress {
            format!("{:>5.1}%", progress * 100.0)
        } else if matches!(self.tracking, DownloadTracking::Detached { .. }) {
            " ext ".to_string()
        } else if self.is_finished() {
            "100.0%".to_string()
        } else {
            let spinner = ["···", "•··", "••·", "•••"];
            format!(" {} ", spinner[tick % spinner.len()])
        }
    }

    fn status_badge(&self, tick: usize) -> Span<'static> {
        if matches!(self.tracking, DownloadTracking::Detached { .. }) {
            return Span::styled(" ext ", Style::default().fg(Color::Black).bg(Color::Blue));
        }

        match self.outcome {
            Some(DownloadOutcome::Success) => {
                Span::styled(" done ", Style::default().fg(Color::Black).bg(Color::Green))
            }
            Some(DownloadOutcome::Failed) => {
                Span::styled(" fail ", Style::default().fg(Color::White).bg(Color::Red))
            }
            Some(DownloadOutcome::Aborted) => {
                Span::styled(" stop ", Style::default().fg(Color::Black).bg(Color::Yellow))
            }
            None => {
                let spinner = ["cli", "run", "net", "io "];
                Span::styled(
                    format!(" {} ", spinner[tick % spinner.len()]),
                    Style::default().fg(Color::Black).bg(Color::Cyan),
                )
            }
        }
    }

    fn logs_for_render(&self) -> Vec<Line<'static>> {
        self.logs
            .iter()
            .map(|line| Line::from(line.clone()))
            .collect()
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

enum DownloadTracking {
    Managed,
    Detached { key: (u32, u64) },
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
        "trusted" => {
            Span::styled(" trusted ", Style::default().fg(Color::Black).bg(Color::Yellow))
        }
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
