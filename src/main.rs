use anyhow::Result;
use clap::Parser;

use pirata::app::App;
use pirata::cli::Cli;
use pirata::config::AppConfig;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = AppConfig::load(cli.config.clone()).await?;
    let app = App::new(config);

    app.run(cli).await
}
