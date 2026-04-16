use anyhow::Result;
use clap::Parser;

use pirata::app::App;
use pirata::cli::{Cli, Commands, SetupArgs};
use pirata::config::{AppConfig, default_config_path};
use pirata::setup::{can_prompt_for_setup, run_setup_wizard};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let config_path = cli.config.clone().unwrap_or_else(default_config_path);
    let config_exists = tokio::fs::try_exists(&config_path).await.unwrap_or(false);

    let skips_setup_gate = matches!(
        cli.command.as_ref(),
        Some(Commands::Doctor) | Some(Commands::Setup(_))
    );

    if !config_exists && !skips_setup_gate {
        if cli.global.json || !can_prompt_for_setup() {
            anyhow::bail!(
                "config missing at {}. Run `pirata setup` in a terminal to create it.",
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
