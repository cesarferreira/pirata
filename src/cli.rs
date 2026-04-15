use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::model::{DownloaderKind, IndexerKind, SearchSort};

#[derive(Debug, Clone, Parser)]
#[command(
    name = "pirata",
    version,
    about = "Search torrents and send magnets to downloaders"
)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalArgs,
    #[command(subcommand)]
    pub command: Option<Commands>,
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

#[derive(Debug, Clone, Subcommand)]
pub enum Commands {
    #[command(visible_alias = "t")]
    Tui(TuiArgs),
    #[command(visible_alias = "s")]
    Search(SearchArgs),
    #[command(visible_alias = "i")]
    Info(IdArgs),
    #[command(visible_alias = "m")]
    Magnet(IdArgs),
    #[command(visible_alias = "a")]
    Add(IdArgs),
    #[command(visible_alias = "l")]
    Lucky(LuckyArgs),
}

#[derive(Debug, Clone, Args)]
pub struct TuiArgs {
    pub query: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct IdArgs {
    pub id: String,
}

#[derive(Debug, Clone, Args)]
pub struct SearchArgs {
    pub query: String,
    #[arg(long)]
    pub limit: Option<usize>,
    #[arg(long, value_enum, default_value_t = SearchSort::Seeders)]
    pub sort: SearchSort,
    #[arg(long)]
    pub interactive: bool,
}

#[derive(Debug, Clone, Args)]
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

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{Cli, Commands};

    #[test]
    fn parses_no_subcommand_for_default_tui_mode() {
        let cli = Cli::try_parse_from(["pirata"]).expect("cli should parse");
        assert!(cli.command.is_none());
    }

    #[test]
    fn parses_explicit_tui_command_with_optional_query() {
        let cli = Cli::try_parse_from(["pirata", "tui", "ubuntu"]).expect("cli should parse");
        match cli.command {
            Some(Commands::Tui(args)) => assert_eq!(args.query.as_deref(), Some("ubuntu")),
            other => panic!("expected tui command, got {other:?}"),
        }
    }

    #[test]
    fn parses_short_aliases() {
        let search = Cli::try_parse_from(["pirata", "s", "ubuntu"]).expect("search alias");
        assert!(matches!(
            search.command,
            Some(Commands::Search(args)) if args.query == "ubuntu"
        ));

        let info = Cli::try_parse_from(["pirata", "i", "123"]).expect("info alias");
        assert!(matches!(
            info.command,
            Some(Commands::Info(args)) if args.id == "123"
        ));

        let magnet = Cli::try_parse_from(["pirata", "m", "123"]).expect("magnet alias");
        assert!(matches!(
            magnet.command,
            Some(Commands::Magnet(args)) if args.id == "123"
        ));

        let add = Cli::try_parse_from(["pirata", "a", "123"]).expect("add alias");
        assert!(matches!(
            add.command,
            Some(Commands::Add(args)) if args.id == "123"
        ));

        let lucky = Cli::try_parse_from(["pirata", "l", "ubuntu"]).expect("lucky alias");
        assert!(matches!(
            lucky.command,
            Some(Commands::Lucky(args)) if args.query == "ubuntu"
        ));
    }
}
