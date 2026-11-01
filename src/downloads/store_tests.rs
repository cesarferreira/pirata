use super::*;

const HASH: &str = "0123456789abcdef0123456789abcdef01234567";

fn request() -> DownloadRequest {
    DownloadRequest {
        id: "42".into(),
        name: "Example Download".into(),
        magnet: format!("magnet:?xt=urn:btih:{HASH}"),
        expected_bytes: 100,
    }
}

fn engine(root: &Path) -> LibrqbitConfig {
    LibrqbitConfig {
        download_dir: Some(root.display().to_string()),
    }
}

#[test]
fn separates_active_files_from_completed_files_and_state() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(&engine(root.path()), &DownloadsConfig::default()).unwrap();
    let record = store.begin(&request()).unwrap();
    let root = root.path().canonicalize().unwrap();
    assert_eq!(
        store.source(&record),
        root.join(format!("incomplete/{HASH}"))
    );
    assert_eq!(
        store.destination(&record),
        root.join(format!("completed/Example Download [{HASH}]"))
    );
    assert!(!store.destination(&record).exists());
}

#[test]
fn promotes_nested_files_without_copying_data_and_retargets_saved_state() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(&engine(root.path()), &DownloadsConfig::default()).unwrap();
    let mut record = store.begin(&request()).unwrap();
    let source = store.source(&record);
    fs::create_dir(source.join("subfolder")).unwrap();
    fs::write(source.join("subfolder/file.bin"), b"verified data").unwrap();
    let destination = store.promote(&mut record).unwrap();
    assert!(
        same_file::is_same_file(
            source.join("subfolder/file.bin"),
            destination.join("subfolder/file.bin")
        )
        .unwrap()
    );
    assert!(
        source.exists(),
        "source must remain available until the engine is retargeted"
    );
    store.complete(&mut record).unwrap();
    assert!(!source.exists());
    assert_eq!(
        fs::read(destination.join("subfolder/file.bin")).unwrap(),
        b"verified data"
    );
    assert_eq!(store.records().unwrap()[0].phase, Phase::Completed);
}

#[test]
fn refuses_an_untracked_incomplete_path_instead_of_overwriting_it() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(&engine(root.path()), &DownloadsConfig::default()).unwrap();
    let source = store.layout.incomplete.join(HASH);
    fs::create_dir(&source).unwrap();
    fs::write(source.join("keep.txt"), b"existing").unwrap();
    assert!(store.begin(&request()).is_err());
    assert_eq!(fs::read(source.join("keep.txt")).unwrap(), b"existing");
    assert!(store.records().unwrap().is_empty());
}

#[test]
fn refuses_an_existing_completed_directory_even_when_empty() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(&engine(root.path()), &DownloadsConfig::default()).unwrap();
    let mut record = store.begin(&request()).unwrap();
    fs::write(store.source(&record).join("file.bin"), b"new").unwrap();
    fs::create_dir(store.destination(&record)).unwrap();
    assert!(store.promote(&mut record).is_err());
    assert!(
        store
            .destination(&record)
            .read_dir()
            .unwrap()
            .next()
            .is_none()
    );
    assert!(store.source(&record).join("file.bin").exists());
}

#[test]
fn reopening_after_interrupted_promotion_preserves_files_and_resumes_links() {
    let root = tempfile::tempdir().unwrap();
    let source;
    let destination;
    {
        let store = Store::open(&engine(root.path()), &DownloadsConfig::default()).unwrap();
        let mut record = store.begin(&request()).unwrap();
        source = store.source(&record);
        destination = store.destination(&record);
        fs::write(source.join("first.bin"), b"first").unwrap();
        fs::write(source.join("second.bin"), b"second").unwrap();
        record.phase = Phase::Promoting;
        store.save(&record).unwrap();
        fs::create_dir(&destination).unwrap();
        fs::hard_link(source.join("first.bin"), destination.join("first.bin")).unwrap();
    }
    let store = Store::open(&engine(root.path()), &DownloadsConfig::default()).unwrap();
    let mut record = store.begin(&request()).unwrap();
    store.promote(&mut record).unwrap();
    store.complete(&mut record).unwrap();
    assert!(!source.exists());
    assert_eq!(fs::read(destination.join("second.bin")).unwrap(), b"second");
}

#[test]
fn changing_folder_settings_does_not_retarget_existing_download_records() {
    let root = tempfile::tempdir().unwrap();
    let (source, destination);
    {
        let store = Store::open(&engine(root.path()), &DownloadsConfig::default()).unwrap();
        let record = store.begin(&request()).unwrap();
        source = store.source(&record);
        destination = store.destination(&record);
        fs::write(source.join("file.bin"), b"saved data").unwrap();
    }
    let config = DownloadsConfig {
        incomplete_dir: Some("new-incomplete".into()),
        completed_dir: Some("new-completed".into()),
        ..DownloadsConfig::default()
    };
    let store = Store::open(&engine(root.path()), &config).unwrap();
    let mut record = store.begin(&request()).unwrap();
    assert_eq!(store.source(&record), source);
    assert_eq!(store.destination(&record), destination);
    store.promote(&mut record).unwrap();
    store.complete(&mut record).unwrap();
    assert_eq!(
        fs::read(destination.join("file.bin")).unwrap(),
        b"saved data"
    );
}

#[cfg(unix)]
#[test]
fn failing_to_save_initial_state_does_not_leave_an_untracked_download_directory() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(&engine(root.path()), &DownloadsConfig::default()).unwrap();
    fs::set_permissions(&store.layout.state, fs::Permissions::from_mode(0o500)).unwrap();
    // Privileged test runners may bypass directory write permissions.
    if tempfile::NamedTempFile::new_in(&store.layout.state).is_ok() {
        fs::set_permissions(&store.layout.state, fs::Permissions::from_mode(0o700)).unwrap();
        return;
    }
    let result = store.begin(&request());
    fs::set_permissions(&store.layout.state, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(result.is_err());
    assert!(!store.layout.incomplete.join(HASH).exists());
    assert!(store.begin(&request()).is_ok());
}

#[test]
fn failed_promotion_keeps_both_originals_when_a_file_conflicts() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(&engine(root.path()), &DownloadsConfig::default()).unwrap();
    let mut record = store.begin(&request()).unwrap();
    fs::write(store.source(&record).join("file.bin"), b"new").unwrap();
    record.phase = Phase::Promoting;
    store.save(&record).unwrap();
    fs::create_dir(store.destination(&record)).unwrap();
    fs::write(store.destination(&record).join("file.bin"), b"original").unwrap();
    assert!(store.promote(&mut record).is_err());
    assert_eq!(
        fs::read(store.source(&record).join("file.bin")).unwrap(),
        b"new"
    );
    assert_eq!(
        fs::read(store.destination(&record).join("file.bin")).unwrap(),
        b"original"
    );
}

#[test]
fn rejects_nested_library_paths() {
    let root = tempfile::tempdir().unwrap();
    let config = DownloadsConfig {
        completed_dir: Some("incomplete/completed".into()),
        ..DownloadsConfig::default()
    };
    assert!(Store::open(&engine(root.path()), &config).is_err());
}

#[test]
fn prevents_two_sessions_from_writing_the_same_download_state() {
    let root = tempfile::tempdir().unwrap();
    let first = Store::open(&engine(root.path()), &DownloadsConfig::default()).unwrap();
    assert!(Store::open(&engine(root.path()), &DownloadsConfig::default()).is_err());
    drop(first);
    assert!(Store::open(&engine(root.path()), &DownloadsConfig::default()).is_ok());
}

#[test]
fn sanitizes_torrent_names_to_a_single_bounded_path_component() {
    assert_eq!(safe_name("../../outside/escape"), "_.._outside_escape");
    assert!(safe_name(&"🎬".repeat(200)).len() <= 120);
    assert_eq!(safe_name("..."), "download");
}

#[cfg(unix)]
#[test]
fn recovery_never_follows_a_completed_directory_symlink() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let store = Store::open(&engine(root.path()), &DownloadsConfig::default()).unwrap();
    let mut record = store.begin(&request()).unwrap();
    fs::write(store.source(&record).join("file.bin"), b"source").unwrap();
    record.phase = Phase::Promoting;
    store.save(&record).unwrap();
    std::os::unix::fs::symlink(outside.path(), store.destination(&record)).unwrap();
    assert!(store.promote(&mut record).is_err());
    assert!(!outside.path().join("file.bin").exists());
    assert!(store.source(&record).join("file.bin").exists());
}
