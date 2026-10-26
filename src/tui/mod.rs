// The TUI layout, palette, and keybindings are adapted from piratebay.
// Copyright (c) 2025 Tsiry Sandratraina
// https://github.com/tsirysndr/piratebay
// Licensed under the MIT License.

mod app;
mod clipboard;
mod download;
mod ui;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Result, anyhow};
use librqbit::{Session, SessionOptions};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use tokio::sync::mpsc::{self, UnboundedSender};

use crate::cache::SearchCache;
use crate::indexer::Indexer;
use crate::model::SearchSort;

use app::{App, InputMode, Msg, View};

/// Launch the interactive TUI. Downloads are written to `output_dir`.
pub async fn run(
    output_dir: PathBuf,
    indexer: Arc<dyn Indexer>,
    cache: SearchCache,
    initial_query: Option<String>,
    limit: usize,
    sort: SearchSort,
) -> Result<()> {
    tokio::fs::create_dir_all(&output_dir).await.ok();

    // A shared torrent session powers every download started from the UI.
    // Disable DHT/session persistence: a TUI is ephemeral and this avoids
    // touching on-disk state (and the failures that come with it).
    let opts = SessionOptions {
        disable_dht_persistence: true,
        fastresume: false,
        persistence: None,
        ..Default::default()
    };
    let session: Option<Arc<Session>> = Session::new_with_opts(output_dir.clone(), opts).await.ok();

    let (tx, mut rx) = mpsc::unbounded_channel::<Msg>();
    spawn_input_thread(tx.clone());

    let mut app = App::new(
        indexer,
        cache,
        limit,
        sort,
        session,
        output_dir,
        tx,
        initial_query,
    );
    if !app.query.trim().is_empty() {
        app.fetch();
    }

    let mut terminal = ratatui::init();
    let result = async {
        terminal.draw(|frame| ui::render(frame, &mut app))?;
        while let Some(msg) = rx.recv().await {
            match msg {
                Msg::Term(event) => handle_term(&mut app, event),
                Msg::Results(generation, view, category, res) => {
                    app.apply_results(generation, view, category, res)
                }
                Msg::DlProgress {
                    id,
                    name,
                    downloaded,
                    total,
                    speed,
                    done,
                } => app.on_progress(id, name, downloaded, total, speed, done),
                Msg::DlError { id, err } => app.on_download_error(id, err),
            }
            if app.should_quit {
                break;
            }
            terminal.draw(|frame| ui::render(frame, &mut app))?;
        }
        Ok::<(), anyhow::Error>(())
    }
    .await;

    ratatui::restore();
    result.map_err(|error| anyhow!(error))
}

/// Blocking-read terminal events on a dedicated thread and forward them into
/// the async event loop's channel.
fn spawn_input_thread(tx: UnboundedSender<Msg>) {
    std::thread::spawn(move || {
        while let Ok(event) = event::read() {
            if tx.send(Msg::Term(event)).is_err() {
                break;
            }
        }
    });
}

fn handle_term(app: &mut App, event: Event) {
    let Event::Key(key) = event else {
        return;
    };
    if key.kind != KeyEventKind::Press {
        return;
    }

    if app.show_help {
        match key.code {
            KeyCode::Char('?') | KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter => {
                app.show_help = false
            }
            _ => {}
        }
        return;
    }
    if app.show_detail {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter => app.show_detail = false,
            KeyCode::Char('d') => app.download_selected(),
            KeyCode::Char('c') | KeyCode::Char('y') => app.copy_magnet(),
            _ => {}
        }
        return;
    }

    match app.input_mode {
        InputMode::Editing => match key.code {
            KeyCode::Esc => app.input_mode = InputMode::Normal,
            KeyCode::Enter => {
                app.input_mode = InputMode::Normal;
                app.view = View::Search;
                app.category = app::Category::All;
                app.fetch();
            }
            KeyCode::Backspace => app.input_backspace(),
            KeyCode::Delete => app.input_delete(),
            KeyCode::Left => app.input_left(),
            KeyCode::Right => app.input_right(),
            KeyCode::Home => app.input_home(),
            KeyCode::End => app.input_end(),
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.should_quit = true;
            }
            KeyCode::Char(c) => app.input_insert(c),
            _ => {}
        },
        InputMode::Normal => match key.code {
            KeyCode::Char('q') => app.should_quit = true,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.should_quit = true;
            }
            KeyCode::Char('/') | KeyCode::Char('i') => {
                app.category = app::Category::All;
                app.input_mode = InputMode::Editing;
                app.input_end();
            }
            KeyCode::Down | KeyCode::Char('j') => app.select_next(),
            KeyCode::Up | KeyCode::Char('k') => app.select_prev(),
            KeyCode::PageDown => app.select_page(10),
            KeyCode::PageUp => app.select_page(-10),
            KeyCode::Char('g') | KeyCode::Home => app.select_page(-(i32::MAX as isize)),
            KeyCode::Char('G') | KeyCode::End => app.select_page(i32::MAX as isize),
            KeyCode::Right | KeyCode::Tab | KeyCode::Char('l') => app.next_category(1),
            KeyCode::Left | KeyCode::BackTab | KeyCode::Char('h') => app.next_category(-1),
            KeyCode::Char('d') => app.download_selected(),
            KeyCode::Char('c') | KeyCode::Char('y') => app.copy_magnet(),
            KeyCode::Char('t') => app.toggle_view(),
            KeyCode::Char('r') => app.fetch(),
            KeyCode::Enter => {
                if app.selected().is_some() {
                    app.show_detail = true;
                }
            }
            KeyCode::Char('?') => app.show_help = true,
            KeyCode::Esc => {}
            _ => {}
        },
    }
}
