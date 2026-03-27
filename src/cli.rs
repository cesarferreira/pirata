use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::model::{DownloaderKind, IndexerKind, SearchSort};

#[derive(Debug, Parser)]
#[command(
    name = "pirate-ctl",
    version,
    about = "Search torrents and send magnets to downloaders"
)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalArgs,
    #[command(subcommand)]
    pub command: Commands,
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,
}

#[derive(Debug, Args, Clone)]
pub struct GlobalArgs {
    #[arg(long, global = true)]
    pub json: bool,
    #[arg(long, global = true, value_enum)]
    pub indexer: Option<IndexerKind>,
    #[arg(long, global = true, value_enum)]
    pub downloader: Option<DownloaderKind>,
    #[arg(long, global = true)]
    pub open: bool,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    Search(SearchArgs),
    Info(IdArgs),
    Magnet(IdArgs),
    Add(IdArgs),
    Lucky(LuckyArgs),
    Tui(TuiArgs),
    Doctor,
    Setup(SetupArgs),
}

#[derive(Debug, Args)]
pub struct IdArgs {
    pub id: String,
}

#[derive(Debug, Args)]
pub struct SearchArgs {
    pub query: String,
    #[arg(long)]
    pub limit: Option<usize>,
    #[arg(long, value_enum, default_value_t = SearchSort::Seeders)]
    pub sort: SearchSort,
    #[arg(long)]
    pub interactive: bool,
}

#[derive(Debug, Args)]
pub struct LuckyArgs {
    pub query: String,
    #[arg(long)]
    pub limit: Option<usize>,
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long, default_value_t = 0)]
    pub min_seeders: u32,
    #[arg(long)]
    pub trusted_only: bool,
    #[arg(long)]
    pub min_size: Option<String>,
    #[arg(long)]
    pub max_size: Option<String>,
}

#[derive(Debug, Args)]
pub struct TuiArgs {
    pub query: Option<String>,
    #[arg(long)]
    pub limit: Option<usize>,
    #[arg(long, value_enum, default_value_t = SearchSort::Seeders)]
    pub sort: SearchSort,
}

#[derive(Debug, Args)]
pub struct SetupArgs {
    #[arg(long)]
    pub download_dir: Option<PathBuf>,
}
