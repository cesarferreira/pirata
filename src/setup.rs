use std::io::{self, IsTerminal};
use std::path::PathBuf;

use anyhow::{Result, bail};
use dialoguer::{Input, Password, Select, theme::ColorfulTheme};

use crate::cli::SetupArgs;
use crate::config::{AppConfig, TransmissionClient, default_config_path};
use crate::model::DownloaderKind;

pub struct SetupResult {
    pub path: PathBuf,
    pub config: AppConfig,
}

pub fn can_prompt_for_setup() -> bool {
    io::stdin().is_terminal() && io::stdout().is_terminal()
}

pub async fn run_setup_wizard(
    path_override: Option<PathBuf>,
    existing: AppConfig,
    args: &SetupArgs,
    first_run: bool,
) -> Result<SetupResult> {
    if !can_prompt_for_setup() {
        bail!("interactive setup requires a terminal");
    }

    let path = path_override.unwrap_or_else(default_config_path);
    let theme = ColorfulTheme::default();
    let mut config = existing;

    if first_run {
        println!("No config found at {}.", path.display());
        println!("Starting first-run setup.");
    } else {
        println!("Configuring pirate-ctl at {}.", path.display());
    }

    let mode_items = [
        "Transmission CLI (Recommended)",
        "Transmission RPC / daemon",
        "Transmission auto fallback",
        "System magnet handler",
    ];
    let selected_mode = Select::with_theme(&theme)
        .with_prompt("Choose the default downloader")
        .items(mode_items)
        .default(match (config.defaults.downloader, config.transmission.client) {
            (DownloaderKind::Transmission, TransmissionClient::Cli) => 0,
            (DownloaderKind::Transmission, TransmissionClient::Rpc) => 1,
            (DownloaderKind::Transmission, TransmissionClient::Auto) => 2,
            (DownloaderKind::System, _) => 3,
            _ => 0,
        })
        .interact()?;

    match selected_mode {
        0 => {
            config.defaults.downloader = DownloaderKind::Transmission;
            config.transmission.client = TransmissionClient::Cli;
        }
        1 => {
            config.defaults.downloader = DownloaderKind::Transmission;
            config.transmission.client = TransmissionClient::Rpc;
        }
        2 => {
            config.defaults.downloader = DownloaderKind::Transmission;
            config.transmission.client = TransmissionClient::Auto;
        }
        3 => {
            config.defaults.downloader = DownloaderKind::System;
        }
        _ => unreachable!(),
    }

    if !matches!(config.defaults.downloader, DownloaderKind::System) {
        let current_download_dir = args
            .download_dir
            .as_ref()
            .map(|value| value.display().to_string())
            .or_else(|| config.transmission.download_dir.clone())
            .unwrap_or_default();
        let download_dir: String = Input::with_theme(&theme)
            .with_prompt("Download directory (leave blank to keep client default)")
            .default(current_download_dir)
            .allow_empty(true)
            .interact_text()?;
        config.transmission.download_dir = (!download_dir.trim().is_empty()).then_some(download_dir);

        if matches!(config.transmission.client, TransmissionClient::Rpc) {
            let rpc_url: String = Input::with_theme(&theme)
                .with_prompt("Transmission RPC URL")
                .default(config.transmission.rpc_url.clone())
                .interact_text()?;
            config.transmission.rpc_url = rpc_url;

            let username: String = Input::with_theme(&theme)
                .with_prompt("RPC username (leave blank for none)")
                .default(config.transmission.username.clone().unwrap_or_default())
                .allow_empty(true)
                .interact_text()?;
            config.transmission.username = (!username.trim().is_empty()).then_some(username);

            if config.transmission.username.is_some() {
                let password = Password::with_theme(&theme)
                    .with_prompt("RPC password (leave blank for none)")
                    .allow_empty_password(true)
                    .interact()?;
                config.transmission.password = (!password.is_empty()).then_some(password);
            } else {
                config.transmission.password = None;
            }
        }
    }

    let saved_path = config.save(Some(path.clone())).await?;
    if first_run {
        println!("Saved config to {}.", saved_path.display());
    }
    Ok(SetupResult {
        path: saved_path,
        config,
    })
}
