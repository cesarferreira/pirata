use std::sync::Arc;

use tokio::sync::mpsc::UnboundedSender;

use super::app::Msg;
use crate::downloads::{DownloadManager, DownloadRequest};

pub fn spawn_download(
    manager: Arc<DownloadManager>,
    request: DownloadRequest,
    tx: UnboundedSender<Msg>,
) {
    tokio::spawn(async move {
        let id = request.id.clone();
        let name = request.name.clone();
        let mut last_status = "";
        let result = manager
            .download(request, |update| {
                let _ = tx.send(Msg::DlProgress {
                    id: id.clone(),
                    name: name.clone(),
                    downloaded: update.downloaded,
                    total: update.total,
                    speed: update.speed,
                    done: update.done,
                });
                if update.status != last_status {
                    let _ = tx.send(Msg::DlStatus {
                        id: id.clone(),
                        status: update.status,
                    });
                    last_status = update.status;
                }
            })
            .await;
        match result {
            Ok(path) => {
                let _ = tx.send(Msg::DlComplete { path });
            }
            Err(error) => {
                let _ = tx.send(Msg::DlError {
                    id,
                    err: format!("{error:#}"),
                });
            }
        }
    });
}
