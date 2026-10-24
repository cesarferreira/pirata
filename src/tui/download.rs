use std::sync::Arc;
use std::time::Duration;

use librqbit::Session;
use tokio::sync::mpsc::UnboundedSender;
use tokio::time::Instant;

use super::app::Msg;
use crate::downloader::librqbit::add_magnet;

/// Add `magnet` to the shared session and stream progress back to the UI.
/// Speed is derived from byte deltas so the TUI stays independent of librqbit's
/// internal estimator types.
pub fn spawn_download(
    session: Arc<Session>,
    magnet: String,
    id: String,
    name: String,
    tx: UnboundedSender<Msg>,
) {
    tokio::spawn(async move {
        let handle = match add_magnet(&session, &magnet).await {
            Ok(handle) => handle,
            Err(error) => {
                let _ = tx.send(Msg::DlError {
                    id,
                    err: error.to_string(),
                });
                return;
            }
        };

        if let Err(error) = handle.wait_until_initialized().await {
            let _ = tx.send(Msg::DlError {
                id,
                err: format!("metadata fetch failed: {error}"),
            });
            return;
        }

        let mut interval = tokio::time::interval(Duration::from_millis(500));
        let mut last_bytes = 0u64;
        let mut last_tick = Instant::now();

        loop {
            interval.tick().await;
            let stats = handle.stats();

            let now = Instant::now();
            let dt = now.duration_since(last_tick).as_secs_f64();
            let speed = if dt > 0.0 {
                (stats.progress_bytes.saturating_sub(last_bytes)) as f64 / dt
            } else {
                0.0
            };
            last_bytes = stats.progress_bytes;
            last_tick = now;

            let _ = tx.send(Msg::DlProgress {
                id: id.clone(),
                name: name.clone(),
                downloaded: stats.progress_bytes,
                total: stats.total_bytes,
                speed,
                done: stats.finished,
            });

            if stats.finished {
                break;
            }
        }
    });
}
