use anyhow::Result;
use clap::Parser;

use pirate_ctl::app::App;
use pirate_ctl::cli::{Cli, Commands, SetupArgs};
use pirate_ctl::config::{AppConfig, default_config_path};
use pirate_ctl::setup::{can_prompt_for_setup, run_setup_wizard};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let config_path = cli.config.clone().unwrap_or_else(default_config_path);
    let config_exists = tokio::fs::try_exists(&config_path).await.unwrap_or(false);

    if !config_exists
        && !matches!(&cli.command, Commands::Doctor | Commands::Setup(_))
    {
        if cli.global.json || !can_prompt_for_setup() {
            anyhow::bail!(
                "config missing at {}. Run `pirate-ctl setup` in a terminal to create it.",
                config_path.display()
            );
        }

        run_setup_wizard(
            cli.config.clone(),
            AppConfig::default(),
            &SetupArgs { download_dir: None },
            true,
        )
        .await?;
    }

    let config = AppConfig::load(cli.config.clone()).await?;
    let app = App::new(config);

    app.run(cli).await
}
