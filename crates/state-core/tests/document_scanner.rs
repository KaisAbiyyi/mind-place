use mind_place_state::{DocumentScanner, ScanPolicy, StateError, StateStore, WatchEvent};
use std::{collections::BTreeSet, fs};

fn fixture() -> (tempfile::TempDir, StateStore, String) {
    let dir = tempfile::tempdir().unwrap();
    let state = StateStore::open(dir.path().join("authority/state.sqlite3")).unwrap();
    let root_path = dir.path().join("approved-documents");
    fs::create_dir_all(&root_path).unwrap();
    let root = state.register_document_root(&root_path).unwrap();
    state.approve_document_root(&root.id).unwrap();
    (dir, state, root.id)
}

#[test]
fn policy_is_bounded_and_watcher_reconciliation_is_not_trusted_without_rescan() {
    let (_dir, state, root) = fixture();
    let mut invalid = ScanPolicy::default();
    invalid.max_files = 0;
    assert!(matches!(
        DocumentScanner::new(state.clone(), invalid),
        Err(StateError::InvalidValue)
    ));
    let mut invalid = ScanPolicy::default();
    invalid.max_bytes_per_file = 64 * 1024 * 1024;
    assert!(matches!(
        DocumentScanner::new(state.clone(), invalid),
        Err(StateError::InvalidValue)
    ));
    let mut invalid = ScanPolicy::default();
    invalid.extensions = BTreeSet::from([".rs".to_owned()]);
    assert!(matches!(
        DocumentScanner::new(state.clone(), invalid),
        Err(StateError::InvalidValue)
    ));

    let mut scanner = DocumentScanner::new(state, ScanPolicy::default()).unwrap();
    assert!(scanner.needs_rescan(&root)); // restart never trusts old state
    assert_eq!(scanner.current_uris(&root, || Ok(())).unwrap(), None);
    for event in [
        WatchEvent::Changed,
        WatchEvent::Deleted,
        WatchEvent::Renamed,
        WatchEvent::Overflow,
        WatchEvent::Disconnected,
    ] {
        scanner.watch_hint(&root, event).unwrap();
        assert!(scanner.needs_rescan(&root));
    }
    assert!(scanner
        .watch_hint(
            "root:v1:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
            WatchEvent::Changed
        )
        .is_err());
}

#[cfg(windows)]
#[test]
fn windows_scan_deduplicates_rescans_reconciles_rename_and_revocation() {
    let (dir, state, root) = fixture();
    let folder = dir.path().join("approved-documents");
    fs::write(folder.join("alpha.md"), b"first").unwrap();
    fs::write(folder.join("note.exe"), b"excluded").unwrap();
    fs::create_dir(folder.join("node_modules")).unwrap();
    fs::write(folder.join("node_modules").join("secret.md"), b"excluded").unwrap();

    let mut scanner = DocumentScanner::new(state.clone(), ScanPolicy::default()).unwrap();
    let first = scanner.rescan(&root, || Ok(())).unwrap();
    assert!(first.complete);
    assert_eq!(
        (
            first.visited_files,
            first.new_versions,
            first.unchanged_versions,
            first.removed_paths
        ),
        (1, 1, 0, 0)
    );
    assert_eq!(
        scanner
            .current_uris(&root, || Ok(()))
            .unwrap()
            .unwrap()
            .len(),
        1
    );

    let second = scanner.rescan(&root, || Ok(())).unwrap();
    assert_eq!((second.new_versions, second.unchanged_versions), (0, 1));
    fs::write(folder.join("alpha.md"), b"changed").unwrap();
    scanner.watch_hint(&root, WatchEvent::Changed).unwrap();
    assert!(scanner.current_uris(&root, || Ok(())).unwrap().is_none()); // no stale view on event
    let third = scanner.rescan(&root, || Ok(())).unwrap();
    assert_eq!(third.new_versions, 1);

    fs::rename(folder.join("alpha.md"), folder.join("bravo.md")).unwrap();
    scanner.watch_hint(&root, WatchEvent::Renamed).unwrap();
    let fourth = scanner.rescan(&root, || Ok(())).unwrap();
    assert_eq!(fourth.new_versions, 1);
    assert_eq!(fourth.removed_paths, 1);

    // Revocation filters inventory immediately even without receiving a watch event.
    state.revoke_document_root(&root).unwrap();
    assert!(scanner.current_uris(&root, || Ok(())).is_err());
    assert!(scanner.rescan(&root, || Ok(())).is_err());
    scanner.invalidate_root(&root);
    assert!(scanner.needs_rescan(&root));
}

#[cfg(windows)]
#[test]
fn windows_guard_abort_never_publishes_inventory() {
    let (dir, state, root) = fixture();
    fs::write(dir.path().join("approved-documents").join("a.md"), b"test").unwrap();
    let mut scanner = DocumentScanner::new(state, ScanPolicy::default()).unwrap();
    let mut checks = 0;
    let failed = scanner.rescan(&root, || {
        checks += 1;
        if checks > 2 {
            Err(StateError::DocumentDenied)
        } else {
            Ok(())
        }
    });
    assert!(failed.is_err());
    assert!(scanner.needs_rescan(&root));
    assert_eq!(scanner.current_uris(&root, || Ok(())).unwrap(), None);
}

#[cfg(windows)]
#[test]
fn windows_handle_enumerator_skips_reparse_and_rejects_root_swap() {
    let (dir, state, root) = fixture();
    let path = dir.path().join("approved-documents");
    fs::write(path.join("allowed.md"), b"authorized").unwrap();
    let outside = dir.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("secret.md"), b"not authorized").unwrap();
    // GitHub Windows runners may restrict symlinks; exercise whenever allowed.
    if std::os::windows::fs::symlink_dir(&outside, path.join("link")).is_ok() {
        let mut scanner = DocumentScanner::new(state.clone(), ScanPolicy::default()).unwrap();
        let result = scanner.rescan(&root, || Ok(())).unwrap();
        assert_eq!(result.visited_files, 1);
        assert_eq!(
            scanner
                .current_uris(&root, || Ok(()))
                .unwrap()
                .unwrap()
                .len(),
            1
        );
    }
    let mut scanner = DocumentScanner::new(state.clone(), ScanPolicy::default()).unwrap();
    scanner.rescan(&root, || Ok(())).unwrap();
    fs::rename(&path, dir.path().join("prior-root")).unwrap();
    fs::create_dir(&path).unwrap();
    fs::write(path.join("spoof.md"), b"untrusted").unwrap();
    assert!(scanner.current_uris(&root, || Ok(())).is_err());
    assert!(scanner.rescan(&root, || Ok(())).is_err());
    assert!(scanner.needs_rescan(&root));
}


#[cfg(windows)]
#[test]
fn windows_global_traversal_budget_and_total_bytes_are_bounded() {
    let (dir, state, root) = fixture();
    let path = dir.path().join("approved-documents");
    fs::create_dir(path.join("first")).unwrap();
    fs::create_dir(path.join("second")).unwrap();
    fs::write(path.join("first").join("a.md"), b"hello").unwrap();
    fs::write(path.join("second").join("b.md"), b"world").unwrap();

    let mut small = ScanPolicy::default();
    small.max_files = 3; // root 2 dirs + first child, second child exceeds cap
    let mut scanner = DocumentScanner::new(state.clone(), small).unwrap();
    assert!(matches!(scanner.rescan(&root, || Ok(())), Err(StateError::DocumentLimit)));
    assert!(scanner.needs_rescan(&root));
    assert_eq!(scanner.current_uris(&root, || Ok(())).unwrap(), None);

    let mut small = ScanPolicy::default();
    small.max_total_bytes = 6; // 5 + 5 > 6 across folders
    let mut scanner = DocumentScanner::new(state, small).unwrap();
    assert!(matches!(scanner.rescan(&root, || Ok(())), Err(StateError::DocumentLimit)));
    assert!(scanner.needs_rescan(&root));
}
