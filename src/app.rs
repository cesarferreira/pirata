use anyhow::{Result, anyhow, bail};
use dialoguer::{FuzzySelect, theme::ColorfulTheme};
use serde::Serialize;

use crate::cache::SearchCache;
use crate::cli::{Cli, Commands, LuckyArgs, SearchArgs};
use crate::config::AppConfig;
use crate::downloader::Downloader;
use crate::downloader::system::SystemDownloader;
use crate::downloader::transmission::TransmissionDownloader;
use crate::indexer::Indexer;
use crate::indexer::pirate_bay::PirateBayIndexer;
use crate::model::{DownloaderKind, IndexerKind, SearchSort, Torrent};
use crate::output::{print_json, print_search_table, print_torrent_info};
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
        let indexer_kind = cli.global.indexer.unwrap_or(self.config.defaults.indexer);
        let downloader_kind = cli
            .global
            .downloader
            .unwrap_or(self.config.defaults.downloader);
        let limit = self.config.defaults.search_limit;
        let cache = SearchCache::new(self.config.cache_dir()?, self.config.cache_ttl());

        match cli.command {
            Commands::Search(args) => {
                self.handle_search(
                    indexer_kind,
                    downloader_kind,
                    cli.global.open,
                    cli.global.json,
                    args,
                    limit,
                    &cache,
                )
                .await
            }
            Commands::Info(args) => {
                let indexer = self.indexer(indexer_kind)?;
                let torrent = indexer.info(&args.id).await?;
                if cli.global.json {
                    print_json(&torrent)?;
                } else {
                    print_torrent_info(&torrent);
                }
                Ok(())
            }
            Commands::Magnet(args) => {
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
            Commands::Add(args) => {
                let indexer = self.indexer(indexer_kind)?;
                let torrent = indexer.info(&args.id).await?;
                self.dispatch_torrent(&torrent, downloader_kind, cli.global.open)
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
            Commands::Lucky(args) => {
                self.handle_lucky(
                    indexer_kind,
                    downloader_kind,
                    cli.global.open,
                    cli.global.json,
                    args,
                    limit,
                    &cache,
                )
                .await
            }
        }
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
    ) -> Result<()> {
        if json && args.interactive {
            bail!("--json cannot be combined with --interactive");
        }

        let limit = args.limit.unwrap_or(default_limit);
        let results = self
            .load_search_results(indexer_kind, &args.query, limit, cache)
            .await?;
        let mut results = sort_results(results, args.sort);

        if args.interactive {
            if results.is_empty() {
                bail!("no results found for '{}'", args.query);
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
            let selection = FuzzySelect::with_theme(&ColorfulTheme::default())
                .with_prompt("Select a torrent to add")
                .items(&items)
                .interact_opt()?;
            if let Some(index) = selection {
                let torrent = results.swap_remove(index);
                self.dispatch_torrent(&torrent, downloader_kind, open)
                    .await?;
                println!(
                    "Added '{}' via {}",
                    torrent.name,
                    self.action_target(downloader_kind, open)
                );
            }
            return Ok(());
        }

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
            self.dispatch_torrent(&chosen.torrent, downloader_kind, open)
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

    async fn dispatch_torrent(
        &self,
        torrent: &Torrent,
        downloader_kind: DownloaderKind,
        open: bool,
    ) -> Result<()> {
        if open {
            let downloader = SystemDownloader;
            return downloader.add_torrent(torrent).await;
        }

        let downloader = self.downloader(downloader_kind)?;
        downloader.add_torrent(torrent).await
    }

    fn indexer(&self, kind: IndexerKind) -> Result<Box<dyn Indexer>> {
        match kind {
            IndexerKind::Piratebay => Ok(Box::new(PirateBayIndexer::new()?)),
        }
    }

    fn downloader(&self, kind: DownloaderKind) -> Result<Box<dyn Downloader>> {
        match kind {
            DownloaderKind::Transmission => Ok(Box::new(TransmissionDownloader::new(
                self.config.transmission.clone(),
            )?)),
            DownloaderKind::System => Ok(Box::new(SystemDownloader)),
            DownloaderKind::Qbittorrent | DownloaderKind::Aria2 => {
                Err(anyhow!("{kind} downloader is not implemented yet"))
            }
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

#[cfg(test)]
mod tests {
    use crate::model::Torrent;

    use super::score_torrent;

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
}
