use anyhow::Result;
use clap::Parser;

use pirate_ctl::app::App;
use pirate_ctl::cli::Cli;
use pirate_ctl::config::AppConfig;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = AppConfig::load(cli.config.clone()).await?;
    let app = App::new(config);

    app.run(cli).await
}
