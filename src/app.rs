use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use dialoguer::{Input, Select, theme::ColorfulTheme};
use indicatif::{ProgressBar, ProgressStyle};
use serde::Serialize;

use crate::cache::SearchCache;
use crate::cli::{Cli, Commands, LuckyArgs, SearchArgs, TuiArgs};
use crate::config::AppConfig;
use crate::downloader::Downloader;
use crate::downloader::system::SystemDownloader;
use crate::downloader::transmission::TransmissionDownloader;
use crate::history::{
    DownloadHistory, DownloadHistoryEntry, merge_tracked_downloads, now_epoch_secs,
};
use crate::indexer::Indexer;
use crate::indexer::pirate_bay::PirateBayIndexer;
use crate::model::{DownloaderKind, IndexerKind, SearchSort, Torrent, TrackedDownload};
use crate::output::{print_json, print_search_table, print_torrent_info, print_tracked_downloads};
use crate::util::parse_size_filter;

#[derive(Debug, Clone)]
pub struct App {
    config: AppConfig,
}

impl App {
    pub fn new(config: AppConfig) -> Self {
        Self { config }
    }

    pub async fn run(&self, cli: Cli) -> Result<()> {
        let mode = resolve_run_mode(&cli)?;
        let indexer_kind = cli.global.indexer.unwrap_or(self.config.defaults.indexer);
        let downloader_kind = cli
            .global
            .downloader
            .unwrap_or(self.config.defaults.downloader);
        let limit = self.config.defaults.search_limit;
        let cache = SearchCache::new(self.config.cache_dir()?, self.config.cache_ttl());
        let history = DownloadHistory::new(self.config.history_path()?);

        match mode {
            RunMode::Tui(args) => {
                self.handle_tui(
                    indexer_kind,
                    downloader_kind,
                    cli.global.open,
                    args,
                    limit,
                    &cache,
                    &history,
                )
                .await
            }
            RunMode::Command(Commands::Search(args)) => {
                self.handle_search(
                    indexer_kind,
                    downloader_kind,
                    cli.global.open,
                    cli.global.json,
                    args,
                    limit,
                    &cache,
                    &history,
                )
                .await
            }
            RunMode::Command(Commands::Info(args)) => {
                let indexer = self.indexer(indexer_kind)?;
                let torrent = indexer.info(&args.id).await?;
                if cli.global.json {
                    print_json(&torrent)?;
                } else {
                    print_torrent_info(&torrent);
                }
                Ok(())
            }
            RunMode::Command(Commands::Magnet(args)) => {
                let indexer = self.indexer(indexer_kind)?;
                let torrent = indexer.info(&args.id).await?;
                let magnet = torrent.resolved_magnet();
                if cli.global.json {
                    print_json(&MagnetOutput {
                        id: torrent.id,
                        magnet,
                    })?;
                } else {
                    println!("{magnet}");
                }
                Ok(())
            }
            RunMode::Command(Commands::Add(args)) => {
                let indexer = self.indexer(indexer_kind)?;
                let torrent = indexer.info(&args.id).await?;
                self.start_torrent(&torrent, downloader_kind, cli.global.open, &history)
                    .await?;
                if cli.global.json {
                    print_json(&ActionOutput::added(
                        torrent,
                        downloader_kind,
                        cli.global.open,
                    ))?;
                } else {
                    println!(
                        "Added '{}' via {}",
                        torrent.name,
                        self.action_target(downloader_kind, cli.global.open)
                    );
                }
                Ok(())
            }
            RunMode::Command(Commands::Lucky(args)) => {
                self.handle_lucky(
                    indexer_kind,
                    downloader_kind,
                    cli.global.open,
                    cli.global.json,
                    args,
                    limit,
                    &cache,
                    &history,
                )
                .await
            }
            RunMode::Command(Commands::Tui(_)) => unreachable!("interactive mode is handled above"),
        }
    }

    async fn handle_tui(
        &self,
        indexer_kind: IndexerKind,
        downloader_kind: DownloaderKind,
        open: bool,
        args: TuiArgs,
        default_limit: usize,
        cache: &SearchCache,
        history: &DownloadHistory,
    ) -> Result<()> {
        let query = match args.query {
            Some(query) => query,
            None => {
                let tracked = self
                    .load_tracked_downloads(downloader_kind, history)
                    .await?;
                if !tracked.is_empty() {
                    print_tracked_downloads(&tracked);
                    println!();
                }
                prompt_for_query()?
            }
        };
        self.run_interactive_search(
            indexer_kind,
            downloader_kind,
            open,
            query,
            default_limit,
            cache,
            history,
        )
        .await
    }

    async fn handle_search(
        &self,
        indexer_kind: IndexerKind,
        downloader_kind: DownloaderKind,
        open: bool,
        json: bool,
        args: SearchArgs,
        default_limit: usize,
        cache: &SearchCache,
        history: &DownloadHistory,
    ) -> Result<()> {
        if json && args.interactive {
            bail!("--json cannot be combined with --interactive");
        }

        let limit = args.limit.unwrap_or(default_limit);
        if args.interactive {
            return self
                .run_interactive_search(
                    indexer_kind,
                    downloader_kind,
                    open,
                    args.query,
                    limit,
                    cache,
                    history,
                )
                .await;
        }

        let results = self
            .load_search_results(indexer_kind, &args.query, limit, cache)
            .await?;
        let results = sort_results(results, args.sort);

        if json {
            print_json(&results)?;
        } else {
            print_search_table(&results);
        }
        Ok(())
    }

    async fn handle_lucky(
        &self,
        indexer_kind: IndexerKind,
        downloader_kind: DownloaderKind,
        open: bool,
        json: bool,
        args: LuckyArgs,
        default_limit: usize,
        cache: &SearchCache,
        history: &DownloadHistory,
    ) -> Result<()> {
        let min_size = args
            .min_size
            .as_deref()
            .map(parse_size_filter)
            .transpose()?;
        let max_size = args
            .max_size
            .as_deref()
            .map(parse_size_filter)
            .transpose()?;
        let limit = args.limit.unwrap_or(default_limit);
        let results = self
            .load_search_results(indexer_kind, &args.query, limit, cache)
            .await?;

        let mut candidates: Vec<ScoredTorrent> = results
            .into_iter()
            .filter(|torrent| torrent.seeders >= args.min_seeders)
            .filter(|torrent| !args.trusted_only || torrent.is_trusted())
            .filter(|torrent| min_size.is_none_or(|min| torrent.size_bytes >= min))
            .filter(|torrent| max_size.is_none_or(|max| torrent.size_bytes <= max))
            .map(|torrent| {
                let score = score_torrent(&torrent);
                ScoredTorrent { torrent, score }
            })
            .collect();

        candidates.sort_by(|left, right| right.score.total_cmp(&left.score));
        let Some(chosen) = candidates.into_iter().next() else {
            bail!("no torrent matched the lucky filters");
        };

        if !args.dry_run {
            self.start_torrent(&chosen.torrent, downloader_kind, open, history)
                .await?;
        }

        if json {
            print_json(&LuckyOutput {
                action: if args.dry_run {
                    "dry-run".to_string()
                } else {
                    "added".to_string()
                },
                downloader: self.action_target(downloader_kind, open).to_string(),
                score: chosen.score,
                torrent: chosen.torrent,
            })?;
        } else if args.dry_run {
            println!(
                "Selected '{}' (score {:.2})",
                chosen.torrent.name, chosen.score
            );
            print_torrent_info(&chosen.torrent);
        } else {
            println!(
                "Added '{}' via {} (score {:.2})",
                chosen.torrent.name,
                self.action_target(downloader_kind, open),
                chosen.score
            );
        }

        Ok(())
    }

    async fn load_search_results(
        &self,
        indexer_kind: IndexerKind,
        query: &str,
        limit: usize,
        cache: &SearchCache,
    ) -> Result<Vec<Torrent>> {
        if let Some(results) = cache.get(query, limit).await? {
            return Ok(results);
        }

        let indexer = self.indexer(indexer_kind)?;
        let results = indexer.search(query, limit).await?;
        cache.put(query, limit, &results).await?;
        Ok(results)
    }

    async fn run_interactive_search(
        &self,
        indexer_kind: IndexerKind,
        downloader_kind: DownloaderKind,
        open: bool,
        query: String,
        limit: usize,
        cache: &SearchCache,
        history: &DownloadHistory,
    ) -> Result<()> {
        let mut results = self
            .load_search_results(indexer_kind, &query, limit, cache)
            .await?;
        results = sort_results(results, SearchSort::Seeders);
        if results.is_empty() {
            bail!("no results found for '{}'", query);
        }

        let items: Vec<String> = results
            .iter()
            .map(|torrent| {
                format!(
                    "{} | {} seeders | {} | {}",
                    torrent.id,
                    torrent.seeders,
                    crate::util::format_size(torrent.size_bytes),
                    torrent.name
                )
            })
            .collect();
        let selection = Select::with_theme(&ColorfulTheme::default())
            .with_prompt("Select a torrent to add")
            .items(&items)
            .default(0)
            .interact_opt()?;
        if let Some(index) = selection {
            let torrent = results.swap_remove(index);
            let target = self.action_target(downloader_kind, open);
            let progress = ProgressBar::new_spinner();
            progress.set_style(progress_style());
            progress.set_message(interactive_add_message(&torrent, target));
            progress.enable_steady_tick(Duration::from_millis(120));

            match self
                .start_torrent(&torrent, downloader_kind, open, history)
                .await
            {
                Ok(()) => progress.finish_with_message(format!(
                    "Started download for '{}' via {}",
                    torrent.name, target
                )),
                Err(error) => {
                    progress.finish_and_clear();
                    return Err(error);
                }
            }
        }
        Ok(())
    }

    async fn start_torrent(
        &self,
        torrent: &Torrent,
        downloader_kind: DownloaderKind,
        open: bool,
        history: &DownloadHistory,
    ) -> Result<()> {
        if open {
            let downloader = SystemDownloader;
            return downloader.add_torrent(torrent).await;
        }

        match downloader_kind {
            DownloaderKind::Transmission => {
                let downloader = TransmissionDownloader::new(self.config.transmission.clone())?;
                downloader.add_torrent(torrent).await?;
                if let Some(download) = self.resolve_tracked_download(&downloader, torrent).await? {
                    history
                        .upsert(DownloadHistoryEntry::from_tracked_download(
                            &download,
                            now_epoch_secs(),
                        ))
                        .await?;
                }
                Ok(())
            }
            DownloaderKind::System => {
                let downloader = SystemDownloader;
                downloader.add_torrent(torrent).await
            }
            DownloaderKind::Qbittorrent | DownloaderKind::Aria2 => Err(anyhow!(
                "{downloader_kind} downloader is not implemented yet"
            )),
        }
    }

    async fn resolve_tracked_download(
        &self,
        downloader: &TransmissionDownloader,
        torrent: &Torrent,
    ) -> Result<Option<TrackedDownload>> {
        if let Some(download) = downloader.get_download_by_hash(&torrent.info_hash).await? {
            return Ok(Some(download));
        }

        let Some(download_dir) = &self.config.transmission.download_dir else {
            return Ok(None);
        };

        Ok(Some(TrackedDownload {
            info_hash: torrent.info_hash.clone(),
            name: torrent.name.clone(),
            target_path: std::path::PathBuf::from(download_dir).join(&torrent.name),
            downloader: DownloaderKind::Transmission,
            percent_done: 0,
            completed: false,
        }))
    }

    async fn load_tracked_downloads(
        &self,
        downloader_kind: DownloaderKind,
        history: &DownloadHistory,
    ) -> Result<Vec<TrackedDownload>> {
        let history_entries = history.load_visible().await?;
        let live_downloads = if downloader_kind == DownloaderKind::Transmission {
            let downloader = TransmissionDownloader::new(self.config.transmission.clone())?;
            downloader.list_downloads().await.unwrap_or_default()
        } else {
            Vec::new()
        };
        let (updated_history, tracked) =
            merge_tracked_downloads(history_entries, live_downloads, now_epoch_secs());
        history.save(&updated_history).await?;
        Ok(tracked)
    }

    fn indexer(&self, kind: IndexerKind) -> Result<Box<dyn Indexer>> {
        match kind {
            IndexerKind::Piratebay => Ok(Box::new(PirateBayIndexer::new()?)),
        }
    }

    fn action_target(&self, downloader_kind: DownloaderKind, open: bool) -> &'static str {
        if open {
            "system handler"
        } else {
            match downloader_kind {
                DownloaderKind::Transmission => "transmission",
                DownloaderKind::Qbittorrent => "qbittorrent",
                DownloaderKind::Aria2 => "aria2",
                DownloaderKind::System => "system handler",
            }
        }
    }
}

#[derive(Debug, Serialize)]
struct MagnetOutput {
    id: String,
    magnet: String,
}

#[derive(Debug, Serialize)]
struct ActionOutput {
    action: String,
    downloader: String,
    torrent: Torrent,
}

impl ActionOutput {
    fn added(torrent: Torrent, downloader: DownloaderKind, open: bool) -> Self {
        Self {
            action: "added".to_string(),
            downloader: if open {
                "system".to_string()
            } else {
                downloader.to_string()
            },
            torrent,
        }
    }
}

#[derive(Debug, Serialize)]
struct LuckyOutput {
    action: String,
    downloader: String,
    score: f64,
    torrent: Torrent,
}

#[derive(Debug)]
struct ScoredTorrent {
    torrent: Torrent,
    score: f64,
}

#[derive(Debug)]
enum RunMode {
    Tui(TuiArgs),
    Command(Commands),
}

fn resolve_run_mode(cli: &Cli) -> Result<RunMode> {
    match cli.command.clone() {
        None => {
            if cli.global.json {
                bail!("--json cannot be combined with interactive mode");
            }
            Ok(RunMode::Tui(TuiArgs { query: None }))
        }
        Some(Commands::Tui(args)) => {
            if cli.global.json {
                bail!("--json cannot be combined with interactive mode");
            }
            Ok(RunMode::Tui(args))
        }
        Some(command) => Ok(RunMode::Command(command)),
    }
}

fn sort_results(mut results: Vec<Torrent>, sort: SearchSort) -> Vec<Torrent> {
    match sort {
        SearchSort::Seeders => results.sort_by(|left, right| right.seeders.cmp(&left.seeders)),
        SearchSort::Leechers => results.sort_by(|left, right| right.leechers.cmp(&left.leechers)),
        SearchSort::Size => results.sort_by(|left, right| right.size_bytes.cmp(&left.size_bytes)),
        SearchSort::Name => results.sort_by(|left, right| left.name.cmp(&right.name)),
    }
    results
}

fn score_torrent(torrent: &Torrent) -> f64 {
    let base = (torrent.seeders as f64).sqrt() * 10.0;
    let status_bonus = match torrent.normalized_status().as_deref() {
        Some("vip") => 30.0,
        Some("trusted") => 15.0,
        _ => 0.0,
    };

    base + status_bonus - (torrent.leechers as f64 * 0.5)
}

fn prompt_for_query() -> Result<String> {
    let query = Input::<String>::with_theme(&ColorfulTheme::default())
        .with_prompt("Search query")
        .interact_text()?;
    let query = query.trim().to_string();
    if query.is_empty() {
        bail!("search query cannot be empty");
    }
    Ok(query)
}

fn interactive_add_message(torrent: &Torrent, target: &str) -> String {
    format!("Starting download for '{}' via {}...", torrent.name, target)
}

fn progress_style() -> ProgressStyle {
    ProgressStyle::with_template("{spinner} {msg}")
        .expect("valid progress template")
        .tick_chars("|/-\\ ")
}

#[cfg(test)]
mod tests {
    use crate::{
        cli::{Cli, Commands},
        model::Torrent,
    };

    use clap::Parser;

    use super::{RunMode, interactive_add_message, resolve_run_mode, score_torrent};

    #[test]
    fn lucky_scoring_prefers_vip_seeded_results() {
        let vip = Torrent {
            id: "1".into(),
            name: "vip".into(),
            info_hash: "hash1".into(),
            magnet: None,
            seeders: 100,
            leechers: 10,
            size_bytes: 1,
            status: Some("vip".into()),
            uploaded_by: None,
            description: None,
            category: None,
            subcategory: None,
            added: None,
        };
        let plain = Torrent {
            status: Some("member".into()),
            ..vip.clone()
        };

        assert!(score_torrent(&vip) > score_torrent(&plain));
    }

    #[test]
    fn default_invocation_resolves_to_tui_mode() {
        let cli = Cli::try_parse_from(["pirata"]).expect("cli should parse");
        let mode = resolve_run_mode(&cli).expect("mode should resolve");
        assert!(matches!(mode, RunMode::Tui(_)));
    }

    #[test]
    fn explicit_tui_mode_rejects_json() {
        let cli = Cli::try_parse_from(["pirata", "--json"]).expect("cli should parse");
        let error = resolve_run_mode(&cli).expect_err("mode should reject json");
        assert!(
            error
                .to_string()
                .contains("cannot be combined with interactive mode")
        );
    }

    #[test]
    fn explicit_subcommands_stay_non_interactive() {
        let cli = Cli::try_parse_from(["pirata", "search", "ubuntu"]).expect("cli should parse");
        let mode = resolve_run_mode(&cli).expect("mode should resolve");
        assert!(matches!(mode, RunMode::Command(Commands::Search(_))));
    }

    #[test]
    fn interactive_add_message_mentions_torrent_and_target() {
        let torrent = Torrent {
            id: "1".into(),
            name: "ubuntu".into(),
            info_hash: "hash1".into(),
            magnet: None,
            seeders: 10,
            leechers: 1,
            size_bytes: 1,
            status: None,
            uploaded_by: None,
            description: None,
            category: None,
            subcategory: None,
            added: None,
        };

        let message = interactive_add_message(&torrent, "transmission");
        assert_eq!(
            message,
            "Starting download for 'ubuntu' via transmission..."
        );
    }
}
