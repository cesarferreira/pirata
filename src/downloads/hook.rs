use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use tokio::process::Command;

use super::store::Record;
use crate::config::DownloadsConfig;

pub(super) async fn run(
    config: &DownloadsConfig,
    record: &Record,
    destination: &Path,
) -> Result<()> {
    let Some((program, arguments)) = config.on_complete.split_first() else {
        return Ok(());
    };
    let mut command = Command::new(super::store::expand_home(Path::new(program))?);
    command
        .args(arguments)
        .env("PIRATA_DOWNLOAD_DIR", destination)
        .env("PIRATA_DOWNLOAD_NAME", &record.name)
        .env("PIRATA_INFO_HASH", &record.info_hash)
        .env("PIRATA_TORRENT_ID", &record.id)
        .env(
            "TR_TORRENT_DIR",
            destination
                .parent()
                .context("completed path has no parent")?,
        )
        .env(
            "TR_TORRENT_NAME",
            destination
                .file_name()
                .context("completed path has no name")?,
        )
        .env("TR_TORRENT_HASH", &record.info_hash)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let status = tokio::time::timeout(
        Duration::from_secs(config.hook_timeout_seconds),
        command.status(),
    )
    .await
    .context("completion hook timed out")?
    .with_context(|| format!("could not start completion hook {program}"))?;
    if !status.success() {
        bail!("completion hook {program} failed with {status}");
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::super::store::Phase;
    use super::*;

    fn record() -> Record {
        Record {
            info_hash: "0123456789abcdef0123456789abcdef01234567".into(),
            id: "42".into(),
            name: "A movie; not a command".into(),
            magnet: "unused".into(),
            phase: Phase::Completed,
            hook_done: false,
            files: Vec::new(),
            source: "/incomplete/hash".into(),
            destination: "/completed/movie".into(),
        }
    }

    #[tokio::test]
    async fn passes_completed_paths_as_environment_without_shell_interpolation() {
        let root = tempfile::tempdir().unwrap();
        let output = root.path().join("hook-result");
        let destination = root.path().join("completed movie");
        let config = DownloadsConfig { on_complete: vec!["sh".into(), "-c".into(), "printf '%s\\n' \"$PIRATA_DOWNLOAD_DIR\" \"$PIRATA_DOWNLOAD_NAME\" \"$PIRATA_INFO_HASH\" \"$TR_TORRENT_DIR/$TR_TORRENT_NAME\" > \"$1\"".into(), "hook".into(), output.display().to_string()], ..DownloadsConfig::default() };
        run(&config, &record(), &destination).await.unwrap();
        let actual = std::fs::read_to_string(output).unwrap_or_default();
        assert_eq!(
            actual,
            format!(
                "{}\nA movie; not a command\n0123456789abcdef0123456789abcdef01234567\n{}\n",
                destination.display(),
                destination.display()
            )
        );
    }

    #[tokio::test]
    async fn reports_nonzero_hook_exit_as_a_retryable_completion_error() {
        let config = DownloadsConfig {
            on_complete: vec!["sh".into(), "-c".into(), "exit 7".into()],
            ..DownloadsConfig::default()
        };
        assert!(
            run(&config, &record(), Path::new("/completed"))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn terminates_a_hook_that_exceeds_its_timeout() {
        let config = DownloadsConfig {
            on_complete: vec!["sleep".into(), "30".into()],
            hook_timeout_seconds: 1,
            ..DownloadsConfig::default()
        };
        assert!(
            run(&config, &record(), Path::new("/completed"))
                .await
                .is_err()
        );
    }
}
