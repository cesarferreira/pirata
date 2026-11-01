use super::*;
use std::io::Write;

async fn manager(root: &Path, config: DownloadsConfig) -> Arc<DownloadManager> {
    let engine = LibrqbitConfig {
        download_dir: Some(root.display().to_string()),
    };
    let store = Store::open(&engine, &config).unwrap();
    let session = Session::new_with_opts(
        store.layout.incomplete.clone(),
        SessionOptions {
            disable_dht: true,
            disable_dht_persistence: true,
            ..SessionOptions::default()
        },
    )
    .await
    .unwrap();
    Arc::new(DownloadManager {
        store,
        config,
        session,
        active: Mutex::new(HashMap::new()),
    })
}

fn no_reserve() -> DownloadsConfig {
    DownloadsConfig {
        min_free_space_bytes: 0,
        min_free_space_percent: 0,
        ..DownloadsConfig::default()
    }
}

async fn fixture(
    manager: &DownloadManager,
    root: &Path,
    partial: bool,
) -> (DownloadRequest, Vec<u8>) {
    let data = (0..65536)
        .map(|index| (index % 251) as u8)
        .collect::<Vec<_>>();
    let payload = root.join("payload.bin");
    fs::write(&payload, &data).unwrap();
    let torrent = librqbit::create_torrent(
        &payload,
        librqbit::CreateTorrentOptions {
            piece_length: Some(16384),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let hash = torrent.info_hash().as_string();
    let request = DownloadRequest {
        id: "42".into(),
        name: "payload.bin".into(),
        magnet: format!("magnet:?xt=urn:btih:{hash}"),
        expected_bytes: data.len() as u64,
    };
    let record = manager.store.begin(&request).unwrap();
    manager
        .store
        .save_torrent(&record, &torrent.as_bytes().unwrap())
        .unwrap();
    let mut file = fs::File::create(manager.store.source(&record).join("payload.bin")).unwrap();
    file.write_all(if partial { &data[..16384] } else { &data })
        .unwrap();
    file.set_len(data.len() as u64).unwrap();
    (request, data)
}

#[test]
fn refuses_downloads_that_would_consume_reserved_headroom() {
    let config = DownloadsConfig {
        min_free_space_bytes: 100,
        min_free_space_percent: 10,
        ..DownloadsConfig::default()
    };
    assert!(check_space(500, 1000, 401, &config).is_err());
    assert!(check_space(500, 1000, 400, &config).is_ok());
    assert!(check_space(99, 1000, 0, &config).is_err());
    assert!(check_space(500, 10000, 0, &config).is_err());
    assert!(check_space(1000, 10000, 0, &config).is_ok());
}

#[tokio::test(flavor = "multi_thread")]
async fn verifies_real_torrent_data_before_promotion_and_reseeds_from_completed_path() {
    let root = tempfile::tempdir().unwrap();
    let download_root = root.path().join("downloads");
    let first = manager(&download_root, no_reserve()).await;
    let (request, data) = fixture(&first, root.path(), false).await;
    let mut finished = false;
    let destination = first
        .download(request.clone(), |update| {
            finished |= update.done;
        })
        .await
        .unwrap();
    assert!(finished);
    assert_eq!(fs::read(destination.join("payload.bin")).unwrap(), data);
    let record = first.store.records().unwrap().remove(0);
    assert!(!first.store.source(&record).exists());
    assert_eq!(
        first.session.with_torrents(|torrents| torrents
            .filter(|(_, handle)| handle.stats().finished)
            .count()),
        1
    );
    first.shutdown().await;
    drop(first);
    let second = manager(&download_root, no_reserve()).await;
    assert_eq!(second.resumable().unwrap().len(), 1);
    let restored = second.download(request, |_| {}).await.unwrap();
    assert_eq!(restored, destination);
    assert!(!second.store.source(&record).exists());
    second.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn low_space_leaves_verified_partial_data_paused_and_resumable() {
    let root = tempfile::tempdir().unwrap();
    let config = DownloadsConfig {
        min_free_space_bytes: u64::MAX,
        min_free_space_percent: 0,
        ..DownloadsConfig::default()
    };
    let manager = manager(&root.path().join("downloads"), config).await;
    let (request, data) = fixture(&manager, root.path(), true).await;
    let result =
        tokio::time::timeout(Duration::from_secs(5), manager.download(request, |_| {})).await;
    assert!(
        result.is_ok(),
        "low-space check must stop the download before waiting for peers"
    );
    assert!(result.unwrap().is_err());
    let record = manager.store.records().unwrap().remove(0);
    assert_eq!(record.phase, Phase::Downloading);
    assert!(!manager.store.destination(&record).exists());
    let stats = manager.session.with_torrents(|torrents| {
        torrents
            .map(|(_, handle)| (handle.is_paused(), handle.stats().progress_bytes))
            .collect::<Vec<_>>()
    });
    assert_eq!(stats, vec![(true, 16384)]);
    assert_eq!(
        &fs::read(manager.store.source(&record).join("payload.bin")).unwrap()[..16384],
        &data[..16384]
    );
    manager.shutdown().await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn retries_a_failed_hook_without_redownloading_or_replaying_successful_hooks() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let script = root.path().join("hook");
    let counter = root.path().join("hook-count");
    fs::write(&script, "#!/bin/sh\nexit 7\n").unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    let config = DownloadsConfig {
        on_complete: vec![script.display().to_string(), counter.display().to_string()],
        ..no_reserve()
    };
    let manager = manager(&root.path().join("downloads"), config).await;
    let (request, _) = fixture(&manager, root.path(), false).await;
    assert!(manager.download(request.clone(), |_| {}).await.is_err());
    let record = manager.store.records().unwrap().remove(0);
    assert_eq!(record.phase, Phase::Completed);
    assert!(!record.hook_done);
    assert!(!manager.store.source(&record).exists());
    fs::write(&script, "#!/bin/sh\nprintf 'called\\n' >> \"$1\"\n").unwrap();
    manager.download(request.clone(), |_| {}).await.unwrap();
    manager.download(request, |_| {}).await.unwrap();
    assert_eq!(fs::read_to_string(counter).unwrap(), "called\n");
    manager.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn manually_deleted_completed_files_are_not_requeued() {
    let root = tempfile::tempdir().unwrap();
    let manager = manager(&root.path().join("downloads"), no_reserve()).await;
    let (request, _) = fixture(&manager, root.path(), false).await;
    let destination = manager.download(request, |_| {}).await.unwrap();
    fs::remove_dir_all(destination).unwrap();
    assert!(manager.resumable().unwrap().is_empty());
    manager.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn deleting_a_file_inside_a_completed_folder_does_not_recreate_it_on_resume() {
    let root = tempfile::tempdir().unwrap();
    let manager = manager(&root.path().join("downloads"), no_reserve()).await;
    let (request, _) = fixture(&manager, root.path(), false).await;
    let destination = manager.download(request.clone(), |_| {}).await.unwrap();
    fs::remove_file(destination.join("payload.bin")).unwrap();
    assert!(manager.resumable().unwrap().is_empty());
    assert!(manager.download(request, |_| {}).await.is_err());
    assert!(!destination.join("payload.bin").exists());
    manager.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_changed_completed_file_is_not_truncated_during_resume_verification() {
    let root = tempfile::tempdir().unwrap();
    let manager = manager(&root.path().join("downloads"), no_reserve()).await;
    let (request, mut expected) = fixture(&manager, root.path(), false).await;
    let destination = manager.download(request.clone(), |_| {}).await.unwrap();
    let path = destination.join("payload.bin");
    fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"extra user data")
        .unwrap();
    expected.extend_from_slice(b"extra user data");
    assert!(manager.download(request, |_| {}).await.is_err());
    assert_eq!(fs::read(path).unwrap(), expected);
    manager.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn padding_torrents_complete_and_resume_without_requiring_virtual_padding_files() {
    use sha1::{Digest, Sha1};
    let root = tempfile::tempdir().unwrap();
    let manager = manager(&root.path().join("downloads"), no_reserve()).await;
    let mut payload = b"hello world".to_vec();
    payload.extend_from_slice(&[0; 5]);
    let mut info = b"d5:filesld6:lengthi11e4:pathl11:payload.bineed4:attr1:p6:lengthi5e4:pathl4:.pad1:5eee4:name4:pack12:piece lengthi16384e6:pieces20:".to_vec();
    info.extend_from_slice(&Sha1::digest(&payload));
    info.push(b'e');
    let hash = format!("{:x}", Sha1::digest(&info));
    let mut torrent = b"d4:info".to_vec();
    torrent.extend_from_slice(&info);
    torrent.push(b'e');
    let request = DownloadRequest {
        id: "padding".into(),
        name: "pack".into(),
        magnet: format!("magnet:?xt=urn:btih:{hash}"),
        expected_bytes: 16,
    };
    let record = manager.store.begin(&request).unwrap();
    fs::write(
        manager.store.source(&record).join("payload.bin"),
        b"hello world",
    )
    .unwrap();
    manager.store.save_torrent(&record, &torrent).unwrap();
    let destination = manager.download(request.clone(), |_| {}).await.unwrap();
    assert_eq!(
        fs::read(destination.join("payload.bin")).unwrap(),
        b"hello world"
    );
    assert!(!destination.join(".pad/5").exists());
    assert_eq!(manager.store.records().unwrap()[0].files.len(), 1);
    assert_eq!(manager.resumable().unwrap().len(), 1);
    manager.download(request, |_| {}).await.unwrap();
    manager.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn downloads_from_a_local_peer_then_promotes_and_keeps_a_seeding_handle() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let root = tempfile::tempdir().unwrap();
    let seed_root = root.path().join("seed");
    fs::create_dir(&seed_root).unwrap();
    let data = (0..65536)
        .map(|index| (index % 251) as u8)
        .collect::<Vec<_>>();
    let payload = seed_root.join("payload.bin");
    fs::write(&payload, &data).unwrap();
    let torrent = librqbit::create_torrent(
        &payload,
        librqbit::CreateTorrentOptions {
            piece_length: Some(16384),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let bytes = torrent.as_bytes().unwrap();
    let seeder = Session::new_with_opts(
        seed_root.clone(),
        SessionOptions {
            disable_dht: true,
            disable_dht_persistence: true,
            listen_port_range: Some(24000..26000),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let seed = seeder
        .add_torrent(
            AddTorrent::from_bytes(bytes.clone()),
            Some(AddTorrentOptions {
                output_folder: Some(seed_root.display().to_string()),
                overwrite: true,
                paused: true,
                disable_trackers: true,
                ..Default::default()
            }),
        )
        .await
        .unwrap()
        .into_handle()
        .unwrap();
    seed.wait_until_initialized().await.unwrap();
    assert!(seed.stats().finished);
    seeder.unpause(&seed).await.unwrap();
    let seed_port = seeder.tcp_listen_port().unwrap();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let tracker_url = format!("http://{}/announce", listener.local_addr().unwrap());
    let tracker_requests = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let request_count = tracker_requests.clone();
    let tracker = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            request_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let mut request = [0; 8192];
            let _ = socket.read(&mut request).await;
            let mut body = b"d8:completei1e10:incompletei1e8:intervali1e5:peers6:".to_vec();
            body.extend_from_slice(&[127, 0, 0, 1]);
            body.extend_from_slice(&seed_port.to_be_bytes());
            body.push(b'e');
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = socket.write_all(header.as_bytes()).await;
            let _ = socket.write_all(&body).await;
        }
    });

    let engine = LibrqbitConfig {
        download_dir: Some(root.path().join("downloads").display().to_string()),
    };
    let config = no_reserve();
    let store = Store::open(&engine, &config).unwrap();
    let session = Session::new_with_opts(
        store.layout.incomplete.clone(),
        SessionOptions {
            disable_dht: true,
            disable_dht_persistence: true,
            trackers: [reqwest::Url::parse(&tracker_url).unwrap()]
                .into_iter()
                .collect(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let manager = DownloadManager {
        store,
        config,
        session,
        active: Mutex::new(HashMap::new()),
    };
    let request = DownloadRequest::from_magnet(&format!(
        "magnet:?xt=urn:btih:{}",
        torrent.info_hash().as_string()
    ))
    .unwrap();
    let record = manager.store.begin(&request).unwrap();
    manager.store.save_torrent(&record, &bytes).unwrap();
    assert!(
        fs::read_dir(manager.store.source(&record))
            .unwrap()
            .next()
            .is_none()
    );
    let mut completed = false;
    let result = tokio::time::timeout(
        Duration::from_secs(15),
        manager.download(request, |progress| completed |= progress.done),
    )
    .await;
    if result.is_err() {
        eprintln!(
            "tracker requests: {}, seed port: {}, downloader stats: {:?}, seeder stats: {:?}",
            tracker_requests.load(std::sync::atomic::Ordering::SeqCst),
            seed_port,
            manager.session.with_torrents(|torrents| torrents
                .map(|(_, handle)| handle.stats())
                .collect::<Vec<_>>()),
            seed.stats()
        );
    }
    manager.shutdown().await;
    seeder.stop().await;
    tracker.abort();
    let destination = result.expect("local peer download timed out").unwrap();
    assert!(completed);
    assert_eq!(fs::read(destination.join("payload.bin")).unwrap(), data);
    assert!(!manager.store.source(&record).exists());
}
