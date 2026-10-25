use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use librqbit::Session;
use ratatui::crossterm::event::Event;
use ratatui::widgets::TableState;
use tokio::sync::mpsc::UnboundedSender;

use crate::cache::SearchCache;
use crate::indexer::Indexer;
use crate::model::{SearchSort, Torrent, sort_results};

use super::download::spawn_download;

/// Categories mirror The Pirate Bay's top navigation. `All` is the free-text
/// search scope; the rest map onto apibay's numeric category listings.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Category {
    All,
    Audio,
    Video,
    Applications,
    Games,
    Porn,
    Other,
}

impl Category {
    pub const ORDER: [Category; 7] = [
        Category::All,
        Category::Audio,
        Category::Video,
        Category::Applications,
        Category::Games,
        Category::Porn,
        Category::Other,
    ];

    pub fn title(&self) -> &'static str {
        match self {
            Category::All => "All",
            Category::Audio => "Audio",
            Category::Video => "Video",
            Category::Applications => "Apps",
            Category::Games => "Games",
            Category::Porn => "XXX",
            Category::Other => "Other",
        }
    }

    pub fn index(&self) -> usize {
        Category::ORDER.iter().position(|c| c == self).unwrap_or(0)
    }

    /// Category `delta` steps away, wrapping around the tab bar.
    pub fn cycle(self, delta: isize) -> Category {
        let len = Self::ORDER.len() as isize;
        let idx = (self.index() as isize + delta).rem_euclid(len) as usize;
        Self::ORDER[idx]
    }

    fn search_query(self, typed: &str) -> Option<String> {
        match self {
            Category::All => {
                let typed = typed.trim();
                (!typed.is_empty()).then(|| typed.to_string())
            }
            Category::Audio => Some("category:100".to_string()),
            Category::Video => Some("category:200".to_string()),
            Category::Applications => Some("category:300".to_string()),
            Category::Games => Some("category:400".to_string()),
            Category::Porn => Some("category:500".to_string()),
            Category::Other => Some("category:600".to_string()),
        }
    }
}

/// Map a torrent's numeric category code to a short icon + label for display.
pub fn category_label(code: &str) -> (&'static str, &'static str) {
    match code.chars().next() {
        Some('1') => ("♪", "Audio"),
        Some('2') => ("▶", "Video"),
        Some('3') => ("⚙", "Apps"),
        Some('4') => ("🎮", "Games"),
        Some('5') => ("♥", "XXX"),
        _ => ("◈", "Other"),
    }
}

#[derive(PartialEq, Eq)]
pub enum InputMode {
    /// Navigating the results list.
    Normal,
    /// Typing in the search box.
    Editing,
}

/// A download in flight, keyed by torrent id in [`App::downloads`].
pub struct Download {
    pub name: String,
    pub downloaded: u64,
    pub total: u64,
    pub speed: f64,
    pub done: bool,
    pub error: Option<String>,
}

/// Messages funneled into the single event loop from the input thread and the
/// async fetch / download tasks.
pub enum Msg {
    Term(Event),
    /// Results for request `generation` in `category`. Stale generations are dropped.
    Results(u64, Category, Result<Vec<Torrent>, String>),
    DlProgress {
        id: String,
        name: String,
        downloaded: u64,
        total: u64,
        speed: f64,
        done: bool,
    },
    DlError {
        id: String,
        err: String,
    },
}

pub struct App {
    pub indexer: Arc<dyn Indexer>,
    pub cache: SearchCache,
    pub limit: usize,
    pub sort: SearchSort,
    pub session: Option<Arc<Session>>,
    pub output_dir: PathBuf,
    pub tx: UnboundedSender<Msg>,

    pub query: String,
    /// Cursor position within `query`, measured in characters (0..=char_count).
    pub cursor: usize,
    pub input_mode: InputMode,
    pub category: Category,

    pub results: Vec<Torrent>,
    pub table_state: TableState,
    pub loading: bool,
    pub status: String,
    /// Monotonic request id; only the newest fetch is allowed to land.
    pub generation: u64,

    pub downloads: std::collections::HashMap<String, Download>,
    /// Whether the torrent detail popup is open.
    pub show_detail: bool,
    pub show_help: bool,
    pub should_quit: bool,
}

impl App {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        indexer: Arc<dyn Indexer>,
        cache: SearchCache,
        limit: usize,
        sort: SearchSort,
        session: Option<Arc<Session>>,
        output_dir: PathBuf,
        tx: UnboundedSender<Msg>,
        initial_query: Option<String>,
    ) -> Self {
        let query = initial_query.unwrap_or_default();
        let cursor = query.chars().count();
        let input_mode = if query.trim().is_empty() {
            InputMode::Editing
        } else {
            InputMode::Normal
        };
        Self {
            indexer,
            cache,
            limit,
            sort,
            session,
            output_dir,
            tx,
            query,
            cursor,
            input_mode,
            category: Category::All,
            results: Vec::new(),
            table_state: TableState::default(),
            loading: false,
            status: "Type a query and press Enter — or pick a category with ← / →".into(),
            generation: 0,
            downloads: std::collections::HashMap::new(),
            show_detail: false,
            show_help: false,
            should_quit: false,
        }
    }

    pub fn has_initial_query(&self) -> bool {
        !self.query.trim().is_empty()
    }

    fn cursor_byte(&self) -> usize {
        self.query
            .char_indices()
            .nth(self.cursor)
            .map(|(i, _)| i)
            .unwrap_or(self.query.len())
    }

    fn char_count(&self) -> usize {
        self.query.chars().count()
    }

    pub fn input_insert(&mut self, c: char) {
        let byte = self.cursor_byte();
        self.query.insert(byte, c);
        self.cursor += 1;
    }

    /// Delete the character before the cursor (Backspace).
    pub fn input_backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        self.cursor -= 1;
        let byte = self.cursor_byte();
        self.query.remove(byte);
    }

    /// Delete the character under the cursor (Delete).
    pub fn input_delete(&mut self) {
        if self.cursor >= self.char_count() {
            return;
        }
        let byte = self.cursor_byte();
        self.query.remove(byte);
    }

    pub fn input_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn input_right(&mut self) {
        if self.cursor < self.char_count() {
            self.cursor += 1;
        }
    }

    pub fn input_home(&mut self) {
        self.cursor = 0;
    }

    pub fn input_end(&mut self) {
        self.cursor = self.char_count();
    }

    pub fn selected(&self) -> Option<&Torrent> {
        self.table_state
            .selected()
            .and_then(|i| self.results.get(i))
    }

    pub fn select_next(&mut self) {
        if self.results.is_empty() {
            return;
        }
        let i = match self.table_state.selected() {
            Some(i) => (i + 1).min(self.results.len() - 1),
            None => 0,
        };
        self.table_state.select(Some(i));
    }

    pub fn select_prev(&mut self) {
        if self.results.is_empty() {
            return;
        }
        let i = self
            .table_state
            .selected()
            .map(|i| i.saturating_sub(1))
            .unwrap_or(0);
        self.table_state.select(Some(i));
    }

    pub fn select_page(&mut self, delta: isize) {
        if self.results.is_empty() {
            return;
        }
        let cur = self.table_state.selected().unwrap_or(0) as isize;
        let next = cur
            .saturating_add(delta)
            .clamp(0, self.results.len() as isize - 1);
        self.table_state.select(Some(next as usize));
    }

    /// Kick off a fetch for the current category / query and bump the generation so
    /// in-flight older requests are ignored when they return.
    pub fn fetch(&mut self) {
        self.generation += 1;
        let generation = self.generation;
        let category = self.category;
        let Some(query) = category.search_query(&self.query) else {
            self.results.clear();
            self.table_state.select(None);
            self.loading = false;
            self.status = "Type a query and press Enter — or pick a category with ← / →".into();
            return;
        };

        self.loading = true;
        self.status = match category {
            Category::All => format!("Searching for \"{query}\"…"),
            other => format!("Browsing {}…", other.title()),
        };

        let indexer = Arc::clone(&self.indexer);
        let cache = self.cache.clone();
        let tx = self.tx.clone();
        let limit = self.limit;
        let sort = self.sort;

        tokio::spawn(async move {
            let res = load_results(indexer, cache, &query, limit, sort)
                .await
                .map_err(|error| error.to_string());
            let _ = tx.send(Msg::Results(generation, category, res));
        });
    }

    /// Apply a settled fetch result, dropping empty/sentinel rows apibay
    /// returns when there are no matches.
    pub fn apply_results(
        &mut self,
        generation: u64,
        category: Category,
        res: Result<Vec<Torrent>, String>,
    ) {
        if generation != self.generation {
            return;
        }
        self.loading = false;
        match res {
            Ok(mut torrents) => {
                if torrents.len() == 1 && torrents[0].id == "0" {
                    torrents.clear();
                }
                self.results = torrents;
                if self.results.is_empty() {
                    self.table_state.select(None);
                    self.status = "No results found".into();
                } else {
                    self.table_state.select(Some(0));
                    let scope = match category {
                        Category::All => format!("\"{}\"", self.query.trim()),
                        other => other.title().to_string(),
                    };
                    self.status = format!("{} results for {}", self.results.len(), scope);
                }
            }
            Err(error) => {
                self.results.clear();
                self.table_state.select(None);
                self.status = format!("Error: {error}");
            }
        }
    }

    pub fn set_category(&mut self, category: Category) {
        if self.category != category {
            self.category = category;
            self.input_mode = InputMode::Normal;
            self.fetch();
        }
    }

    pub fn next_category(&mut self, delta: isize) {
        self.set_category(self.category.cycle(delta));
    }

    /// Start downloading the highlighted torrent, or update the status if it is
    /// already downloading / the session failed to start.
    pub fn download_selected(&mut self) {
        let Some(torrent) = self.selected() else {
            self.status = "Nothing selected to download".into();
            return;
        };
        let id = torrent.id.clone();
        let name = torrent.name.clone();
        let magnet = torrent.resolved_magnet();

        if self.downloads.contains_key(&id) {
            self.status = format!("Already downloading: {name}");
            return;
        }
        let Some(session) = self.session.clone() else {
            self.status = "Torrent session unavailable — cannot download".into();
            return;
        };

        self.downloads.insert(
            id.clone(),
            Download {
                name: name.clone(),
                downloaded: 0,
                total: 0,
                speed: 0.0,
                done: false,
                error: None,
            },
        );
        self.status = format!("Downloading → {}", self.output_dir.display());
        spawn_download(session, magnet, id, name, self.tx.clone());
    }

    /// Copy the highlighted torrent's magnet link to the system clipboard.
    pub fn copy_magnet(&mut self) {
        let Some(torrent) = self.selected() else {
            self.status = "Nothing selected to copy".into();
            return;
        };
        let magnet = torrent.resolved_magnet();
        self.status = match super::clipboard::copy(&magnet) {
            Ok(()) => "Magnet link copied to clipboard".into(),
            Err(error) => format!("Copy failed: {error}"),
        };
    }

    pub fn on_progress(
        &mut self,
        id: String,
        name: String,
        downloaded: u64,
        total: u64,
        speed: f64,
        done: bool,
    ) {
        let entry = self.downloads.entry(id).or_insert_with(|| Download {
            name,
            downloaded: 0,
            total: 0,
            speed: 0.0,
            done: false,
            error: None,
        });
        entry.downloaded = downloaded;
        entry.total = total;
        entry.speed = speed;
        entry.done = done;
    }

    pub fn on_download_error(&mut self, id: String, err: String) {
        if let Some(download) = self.downloads.get_mut(&id) {
            download.error = Some(err);
        }
    }

    /// Number of downloads still running (not finished, no error).
    pub fn active_downloads(&self) -> usize {
        self.downloads
            .values()
            .filter(|download| !download.done && download.error.is_none())
            .count()
    }
}

async fn load_results(
    indexer: Arc<dyn Indexer>,
    cache: SearchCache,
    query: &str,
    limit: usize,
    sort: SearchSort,
) -> Result<Vec<Torrent>> {
    if let Ok(Some(results)) = cache.get(query, limit).await {
        return Ok(sort_results(results, sort));
    }

    let results = indexer.search(query, limit).await?;
    let _ = cache.put(query, limit, &results).await;
    Ok(sort_results(results, sort))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Duration;

    use async_trait::async_trait;
    use tokio::sync::mpsc;

    use super::*;
    use crate::indexer::Indexer;

    struct StubIndexer;

    #[async_trait]
    impl Indexer for StubIndexer {
        async fn search(&self, _query: &str, _limit: usize) -> Result<Vec<Torrent>> {
            Ok(Vec::new())
        }

        async fn trending(&self, _limit: usize) -> Result<Vec<Torrent>> {
            Ok(Vec::new())
        }

        async fn info(&self, _id: &str) -> Result<Torrent> {
            anyhow::bail!("unused")
        }
    }

    pub(crate) fn test_app() -> App {
        let (tx, _rx) = mpsc::unbounded_channel();
        App::new(
            Arc::new(StubIndexer),
            SearchCache::new(
                PathBuf::from("/tmp/pirata-tui-test"),
                Duration::from_secs(60),
            ),
            20,
            SearchSort::Seeders,
            None,
            PathBuf::from("/tmp/pirata-downloads"),
            tx,
            None,
        )
    }

    fn torrent(id: &str, name: &str, category: &str) -> Torrent {
        Torrent {
            id: id.into(),
            name: name.into(),
            info_hash: "0123456789ABCDEF0123456789ABCDEF01234567".into(),
            magnet: None,
            seeders: 10,
            leechers: 1,
            size_bytes: 1024,
            status: Some("vip".into()),
            uploaded_by: Some("tester".into()),
            description: None,
            category: Some(category.into()),
            subcategory: None,
            added: Some(1_613_347_200),
        }
    }

    #[test]
    fn category_labels_map_by_leading_digit() {
        assert_eq!(category_label("101"), ("♪", "Audio"));
        assert_eq!(category_label("207"), ("▶", "Video"));
        assert_eq!(category_label("399"), ("⚙", "Apps"));
        assert_eq!(category_label("400"), ("🎮", "Games"));
        assert_eq!(category_label("505"), ("♥", "XXX"));
        assert_eq!(category_label("605"), ("◈", "Other"));
        assert_eq!(category_label(""), ("◈", "Other"));
    }

    #[test]
    fn category_cycle_wraps_both_directions() {
        assert_eq!(Category::All.cycle(1), Category::Audio);
        assert_eq!(Category::All.cycle(-1), Category::Other);
        assert_eq!(Category::Other.cycle(1), Category::All);
        assert_eq!(Category::All.cycle(7), Category::All);
    }

    #[test]
    fn apply_results_populates_and_selects_first() {
        let mut app = test_app();
        let rows = vec![torrent("1", "First", "200"), torrent("2", "Second", "200")];
        app.apply_results(app.generation, Category::Video, Ok(rows));
        assert_eq!(app.results.len(), 2);
        assert_eq!(app.table_state.selected(), Some(0));
        assert!(app.status.contains("2 results for Video"));
        assert!(!app.loading);
    }

    #[test]
    fn apply_results_drops_no_results_sentinel() {
        let mut app = test_app();
        app.apply_results(
            app.generation,
            Category::All,
            Ok(vec![torrent("0", "No results", "0")]),
        );
        assert!(app.results.is_empty());
        assert_eq!(app.table_state.selected(), None);
        assert_eq!(app.status, "No results found");
    }

    #[test]
    fn apply_results_ignores_stale_generation() {
        let mut app = test_app();
        app.generation = 5;
        app.apply_results(4, Category::All, Ok(vec![torrent("1", "Late", "200")]));
        assert!(app.results.is_empty());
    }

    #[test]
    fn apply_results_surfaces_errors() {
        let mut app = test_app();
        app.apply_results(app.generation, Category::All, Err("boom".into()));
        assert!(app.results.is_empty());
        assert!(app.status.starts_with("Error:"));
    }

    #[test]
    fn selection_stays_within_bounds() {
        let mut app = test_app();
        let rows = (0..3)
            .map(|i| torrent(&i.to_string(), "T", "200"))
            .collect();
        app.apply_results(app.generation, Category::Video, Ok(rows));

        app.select_next();
        app.select_next();
        app.select_next();
        assert_eq!(app.table_state.selected(), Some(2));

        app.select_prev();
        assert_eq!(app.table_state.selected(), Some(1));

        app.select_page(isize::MAX);
        assert_eq!(app.table_state.selected(), Some(2));

        app.select_page(-(isize::MAX));
        assert_eq!(app.table_state.selected(), Some(0));
    }

    #[test]
    fn download_without_session_reports_status() {
        let mut app = test_app();
        app.apply_results(
            app.generation,
            Category::Video,
            Ok(vec![torrent("1", "Movie", "200")]),
        );
        app.download_selected();
        assert!(app.downloads.is_empty());
        assert!(app.status.contains("session unavailable"));
    }

    #[test]
    fn input_edits_at_cursor_position() {
        let mut app = test_app();
        for c in "ubuntu".chars() {
            app.input_insert(c);
        }
        assert_eq!(app.query, "ubuntu");
        assert_eq!(app.cursor, 6);

        app.input_left();
        app.input_left();
        assert_eq!(app.cursor, 4);
        app.input_insert('X');
        assert_eq!(app.query, "ubunXtu");
        assert_eq!(app.cursor, 5);

        app.input_backspace();
        assert_eq!(app.query, "ubuntu");
        assert_eq!(app.cursor, 4);

        app.input_delete();
        assert_eq!(app.query, "ubunu");
        assert_eq!(app.cursor, 4);
    }

    #[test]
    fn input_cursor_movement_clamps_at_bounds() {
        let mut app = test_app();
        for c in "ab".chars() {
            app.input_insert(c);
        }
        app.input_home();
        assert_eq!(app.cursor, 0);
        app.input_left();
        assert_eq!(app.cursor, 0);
        app.input_backspace();
        assert_eq!(app.query, "ab");

        app.input_end();
        assert_eq!(app.cursor, 2);
        app.input_right();
        assert_eq!(app.cursor, 2);
        app.input_delete();
        assert_eq!(app.query, "ab");
    }

    #[test]
    fn input_handles_multibyte_chars() {
        let mut app = test_app();
        for c in "café".chars() {
            app.input_insert(c);
        }
        assert_eq!(app.cursor, 4);
        app.input_backspace();
        assert_eq!(app.query, "caf");
    }

    #[test]
    fn download_progress_tracks_active_count() {
        let mut app = test_app();
        app.on_progress("a".into(), "A".into(), 50, 100, 1.0, false);
        app.on_progress("b".into(), "B".into(), 100, 100, 0.0, true);
        assert_eq!(app.downloads.len(), 2);
        assert_eq!(app.active_downloads(), 1);
    }

    #[test]
    fn renders_search_shell() {
        let mut app = test_app();
        let backend = ratatui::backend::TestBackend::new(120, 32);
        let mut terminal = ratatui::Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| super::super::ui::render(frame, &mut app))
            .expect("draw");
        let view: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(!view.contains("PIRATA"), "{view}");
        assert!(view.contains("Search"), "{view}");
        assert!(view.contains("Categories"), "{view}");
        assert!(view.contains("Results"), "{view}");
        assert!(view.contains("No torrents"), "{view}");
    }
}
