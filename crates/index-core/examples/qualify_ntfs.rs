//! Explicit MP-4L1 local qualification; never opens the owner's archive.
//! Invoke through scripts/qualify-ntfs.ps1, not as a substitute for MP-4L2/4U.
#[cfg(windows)]
mod native {
    use mind_place_index::{documents::DocumentIndex, IndexError};
    use mind_place_state::{
        DocumentScanner, ScanPolicy, StateError, StateStore, VirtualDocumentPath, WatchEvent,
    };
    use rusqlite::Connection;
    use serde_json::{json, Value};
    use std::{
        fs,
        path::{Path, PathBuf},
        process::{Command, Stdio},
        sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            Arc, Barrier,
        },
        thread,
        time::Instant,
    };

    struct Fixture {
        home: PathBuf,
        root: PathBuf,
        state: StateStore,
        id: String,
        index: DocumentIndex,
    }
    impl Fixture {
        fn new() -> Self {
            let parent = PathBuf::from(
                std::env::var_os("MP_NTFS_FIXTURES").expect("use qualification runner"),
            );
            assert!(parent.is_absolute());
            // Keep synthetic fixtures for inspection. Never recursively delete a race fixture.
            let home = tempfile::Builder::new()
                .prefix("ntfs-")
                .tempdir_in(parent)
                .unwrap()
                .keep();
            let root = home.join("approved");
            fs::create_dir(&root).unwrap();
            let state = StateStore::open(home.join("authority/state.sqlite3")).unwrap();
            let id = state.register_document_root(&root).unwrap().id;
            state.approve_document_root(&id).unwrap();
            let index = DocumentIndex::open(home.join("document-index.sqlite3")).unwrap();
            Self {
                home,
                root,
                state,
                id,
                index,
            }
        }
        fn uri(&self, parts: &[&str]) -> String {
            VirtualDocumentPath::from_segments(
                &self.id,
                &parts.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            )
            .unwrap()
            .to_uri()
        }
        fn refresh(&self) {
            self.index
                .refresh_root(&self.state, &self.id, || Ok(()))
                .unwrap();
        }
        fn hits(&self, term: &str) -> usize {
            self.index
                .search(&self.state, term, 25, || Ok(()))
                .unwrap()
                .len()
        }
        fn generation(&self) -> i64 {
            Connection::open(self.home.join("document-index.sqlite3"))
                .unwrap()
                .query_row(
                    "SELECT coalesce(max(generation),0) FROM document_meta",
                    [],
                    |r| r.get(0),
                )
                .unwrap()
        }
        fn outside(&self) -> PathBuf {
            let outside = self.home.join("outside");
            fs::create_dir(&outside).unwrap();
            fs::write(outside.join("secret.md"), b"outsidecanary").unwrap();
            outside
        }
    }
    // Invoke only PowerShell's native cmdlet with literal argument binding; no cmd /c.
    fn junction(link: &Path, target: &Path) {
        let mut child = Command::new("powershell.exe").args(["-NoProfile", "-NonInteractive", "-Command",
            "& { param($link, $target) $ErrorActionPreference='Stop'; New-Item -ItemType Junction -Path $link -Target $target | Out-Null }"])
            .arg(link).arg(target).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
        let deadline = Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success(), "junction fixture creation failed");
                return;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("junction fixture process timed out");
            }
            thread::sleep(std::time::Duration::from_millis(2));
        }
    }
    fn junctions() -> Value {
        let f = Fixture::new();
        fs::write(f.root.join("safe.md"), "allowedcanary").unwrap();
        let outside = f.outside();
        junction(&f.root.join("escape"), &outside);
        assert!(f
            .state
            .read_document_bytes(&f.uri(&["escape", "secret.md"]), 1024)
            .is_err());
        let alias = f.home.join("root-alias");
        junction(&alias, &f.root);
        assert!(f.state.register_document_root(&alias).is_err());
        f.refresh();
        assert_eq!(f.hits("allowedcanary"), 1);
        assert_eq!(f.hits("outsidecanary"), 0);
        // Remove only the reparse entries, never their targets.
        fs::remove_dir(f.root.join("escape")).unwrap();
        fs::remove_dir(alias).unwrap();
        assert!(outside.join("secret.md").exists());
        json!({"status":"PASS","junctions_created":2,"outside_hits":0})
    }
    fn symlinks() -> Value {
        let f = Fixture::new();
        let outside = f.outside();
        if let Err(error) =
            std::os::windows::fs::symlink_file(outside.join("secret.md"), f.root.join("file.md"))
        {
            return json!({"status":"BLOCKED","reason":"file_symlink_privilege","os_error":error.raw_os_error()});
        }
        if let Err(error) = std::os::windows::fs::symlink_dir(&outside, f.root.join("directory")) {
            fs::remove_file(f.root.join("file.md")).unwrap();
            return json!({"status":"BLOCKED","reason":"directory_symlink_privilege","os_error":error.raw_os_error()});
        }
        for parts in [vec!["file.md"], vec!["directory", "secret.md"]] {
            assert!(f.state.read_document_bytes(&f.uri(&parts), 1024).is_err());
        }
        f.refresh();
        assert_eq!(f.hits("outsidecanary"), 0);
        fs::remove_file(f.root.join("file.md")).unwrap();
        fs::remove_dir(f.root.join("directory")).unwrap();
        json!({"status":"PASS","symlinks_created":2,"outside_hits":0})
    }
    fn hardlinks() -> Value {
        let f = Fixture::new();
        fs::write(f.root.join("safe.md"), "allowedcanary").unwrap();
        f.refresh();
        let generation = f.generation();
        let outside = f.outside();
        fs::hard_link(outside.join("secret.md"), f.root.join("hard.md")).unwrap();
        assert!(f
            .state
            .read_document_bytes(&f.uri(&["hard.md"]), 1024)
            .is_err());
        assert!(f.index.refresh_root(&f.state, &f.id, || Ok(())).is_err());
        assert_eq!(f.generation(), generation);
        assert_eq!(f.hits("outsidecanary"), 0);
        json!({"status":"PASS","generation_unchanged":true,"outside_hits":0})
    }
    fn replacement() -> Value {
        let f = Fixture::new();
        fs::write(f.root.join("safe.md"), "allowedcanary").unwrap();
        f.refresh();
        let generation = f.generation();
        fs::rename(&f.root, f.home.join("prior-root")).unwrap();
        fs::create_dir(&f.root).unwrap();
        fs::write(f.root.join("safe.md"), "replacementcanary").unwrap();
        assert!(f
            .state
            .read_document_bytes(&f.uri(&["safe.md"]), 1024)
            .is_err());
        let reopened = StateStore::open(f.state.path()).unwrap();
        assert!(reopened
            .read_document_bytes(&f.uri(&["safe.md"]), 1024)
            .is_err());
        assert_eq!(f.hits("allowedcanary"), 0);
        assert!(f.index.refresh_root(&f.state, &f.id, || Ok(())).is_err());
        assert_eq!(f.generation(), generation);
        json!({"status":"PASS","restart_denied":true,"stale_hits":0})
    }
    fn reconciliation() -> Value {
        let f = Fixture::new();
        fs::write(f.root.join("before.md"), "originalcanary").unwrap();
        f.refresh();
        let objects = mind_place_state::DocumentObjectStore::open(f.state.clone()).unwrap();
        let old_uri = f.uri(&["before.md"]);
        let old = objects
            .current_version(&old_uri, || Ok(()))
            .unwrap()
            .unwrap();
        // Real filesystem changes without a watcher hint: model missed events.
        fs::rename(f.root.join("before.md"), f.root.join("after.md")).unwrap();
        fs::write(f.root.join("after.md"), "changedcanary").unwrap();
        fs::write(f.root.join("added.md"), "addedcanary").unwrap();
        assert_eq!(f.hits("originalcanary"), 0);
        assert_eq!(f.hits("addedcanary"), 0);
        f.refresh();
        assert_eq!(f.hits("changedcanary"), 1);
        assert_eq!(f.hits("addedcanary"), 1);
        fs::remove_file(f.root.join("after.md")).unwrap();
        assert_eq!(f.hits("changedcanary"), 0);
        f.refresh();
        assert_eq!(f.hits("addedcanary"), 1);
        assert_eq!(
            objects.read_version(&old_uri, &old.id, || Ok(())).unwrap(),
            b"originalcanary"
        );
        let mut scanner = DocumentScanner::new(f.state.clone(), ScanPolicy::default()).unwrap();
        for event in [
            WatchEvent::Changed,
            WatchEvent::Deleted,
            WatchEvent::Renamed,
            WatchEvent::Overflow,
            WatchEvent::Disconnected,
        ] {
            scanner.rescan(&f.id, || Ok(())).unwrap();
            scanner.watch_hint(&f.id, event).unwrap();
            assert!(scanner.needs_rescan(&f.id));
            assert!(scanner.current_uris(&f.id, || Ok(())).unwrap().is_none());
            scanner.rescan(&f.id, || Ok(())).unwrap();
            assert_eq!(
                scanner
                    .current_uris(&f.id, || Ok(()))
                    .unwrap()
                    .unwrap()
                    .len(),
                1
            );
        }
        assert!(DocumentScanner::new(f.state.clone(), ScanPolicy::default())
            .unwrap()
            .needs_rescan(&f.id));
        json!({"status":"PASS","watch_hints":5,"lost_event_reconciliation":"explicit_full_rescan","raw_version_preserved":true})
    }
    fn retry_mutation(mut action: impl FnMut() -> std::io::Result<()>) {
        let deadline = Instant::now() + std::time::Duration::from_secs(3);
        loop {
            match action() {
                Ok(()) => return,
                Err(error)
                    if matches!(error.raw_os_error(), Some(5 | 32))
                        && Instant::now() < deadline =>
                {
                    // NTFS may transiently deny rename/delete while a reader owns a handle.
                    thread::sleep(std::time::Duration::from_millis(2));
                }
                Err(error) => panic!("fixture mutation failed: {error}"),
            }
        }
    }
    fn junction_race() -> Value {
        let f = Fixture::new();
        let outside = f.outside();
        let slot = f.root.join("slot");
        let parked = f.home.join("parked");
        fs::create_dir(&slot).unwrap();
        fs::write(slot.join("secret.md"), "allowedcanary").unwrap();
        assert_eq!(
            f.state
                .read_document_bytes(&f.uri(&["slot", "secret.md"]), 1024)
                .unwrap(),
            b"allowedcanary"
        );
        let gate = Arc::new(Barrier::new(2));
        let stop = Arc::new(AtomicBool::new(false));
        let swaps = Arc::new(AtomicUsize::new(0));
        let live = Arc::new(AtomicBool::new(false));
        let observed_live = Arc::new(AtomicUsize::new(0));
        let worker_live = live.clone();
        let worker_observed = observed_live.clone();
        let deadline = Instant::now() + std::time::Duration::from_secs(45);
        let (worker_gate, worker_stop, worker_swaps) = (gate.clone(), stop.clone(), swaps.clone());
        let worker = thread::spawn(move || {
            worker_gate.wait();
            // A bounded real adversarial writer: no recursive moves/deletes.
            for _ in 0..32 {
                if worker_stop.load(Ordering::SeqCst) || Instant::now() >= deadline {
                    break;
                }
                retry_mutation(|| fs::rename(&slot, &parked));
                junction(&slot, &outside);
                worker_swaps.fetch_add(1, Ordering::SeqCst);
                let previous = worker_observed.load(Ordering::SeqCst);
                worker_live.store(true, Ordering::SeqCst);
                let overlap_deadline = Instant::now() + std::time::Duration::from_millis(100);
                while worker_observed.load(Ordering::SeqCst) == previous
                    && Instant::now() < overlap_deadline
                {
                    thread::sleep(std::time::Duration::from_millis(1));
                }
                worker_live.store(false, Ordering::SeqCst);
                retry_mutation(|| fs::remove_dir(&slot));
                retry_mutation(|| fs::rename(&parked, &slot));
            }
        });
        gate.wait();
        let mut allowed = 0;
        let mut denied = 0;
        let mut leaked = 0;
        // Continue through at least one completed swap, with a bounded deadline.
        while (allowed + denied < 128 || swaps.load(Ordering::SeqCst) < 2)
            && Instant::now() < deadline
        {
            let epoch = swaps.load(Ordering::SeqCst);
            let before_live = live.load(Ordering::SeqCst);
            let read = f
                .state
                .read_document_bytes(&f.uri(&["slot", "secret.md"]), 1024);
            if before_live && live.load(Ordering::SeqCst) && swaps.load(Ordering::SeqCst) == epoch {
                assert!(read.is_err(), "read through a live junction was accepted");
                observed_live.fetch_add(1, Ordering::SeqCst);
            }
            match read {
                Ok(bytes) if bytes == b"allowedcanary" => allowed += 1,
                Ok(_) => {
                    leaked += 1;
                    break;
                }
                Err(_) => denied += 1,
            }
            thread::sleep(std::time::Duration::from_millis(1));
        }
        stop.store(true, Ordering::SeqCst);
        worker.join().unwrap();
        assert_eq!(leaked, 0, "junction race disclosed outside bytes");
        assert!(
            swaps.load(Ordering::SeqCst) >= 2,
            "insufficient native swap coverage"
        );
        assert!(denied > 0, "race did not overlap an absent/reparse entry");
        assert!(
            observed_live.load(Ordering::SeqCst) > 0,
            "no proven read overlap with a live junction"
        );
        json!({"status":"PASS","allowed_reads":allowed,"denied_reads":denied,"live_junction_denials":observed_live.load(Ordering::SeqCst),"swaps":swaps.load(Ordering::SeqCst),"outside_reads":leaked})
    }
    fn revoke_scan() -> Value {
        let f = Fixture::new();
        fs::write(f.root.join("safe.md"), "allowedcanary").unwrap();
        let mut scanner = DocumentScanner::new(f.state.clone(), ScanPolicy::default()).unwrap();
        let mut checks = 0;
        let result = scanner.rescan(&f.id, || {
            checks += 1;
            if checks == 4 {
                f.state.revoke_document_root(&f.id).unwrap();
            }
            Ok(()) // Unlock remains live; root permission is separate authority.
        });
        assert!(checks >= 4);
        assert!(result.is_err());
        assert!(scanner.needs_rescan(&f.id));
        assert!(scanner.current_uris(&f.id, || Ok(())).unwrap().is_none());
        assert!(f
            .state
            .read_document_bytes(&f.uri(&["safe.md"]), 1024)
            .is_err());
        json!({"status":"PASS","revoke_checkpoint":4,"inventory_published":false})
    }
    fn revoker(
        f: &Fixture,
    ) -> (
        std::sync::mpsc::SyncSender<()>,
        std::sync::mpsc::Receiver<()>,
        thread::JoinHandle<()>,
    ) {
        let state = f.state.clone();
        let root = f.id.clone();
        let (trigger, start) = std::sync::mpsc::sync_channel(1);
        let (finish, completed) = std::sync::mpsc::sync_channel(1);
        let worker = thread::spawn(move || {
            start
                .recv_timeout(std::time::Duration::from_secs(10))
                .unwrap();
            state.revoke_document_root(&root).unwrap();
            finish.send(()).unwrap();
        });
        (trigger, completed, worker)
    }
    fn revoke_index_commit() -> Value {
        let f = Fixture::new();
        fs::write(f.root.join("safe.md"), "allowedcanary").unwrap();
        let mut baseline = 0;
        f.index
            .refresh_root(&f.state, &f.id, || {
                baseline += 1;
                Ok(())
            })
            .unwrap();
        let previous = f.generation();
        let (revoke, completed, worker) = revoker(&f);
        let mut calls = 0;
        let mut revoked = false;
        let result = f.index.refresh_root(&f.state, &f.id, || {
            calls += 1;
            // Last pre-commit unlock checkpoint, after root identity validation.
            if calls == baseline - 1 {
                revoke.send(()).unwrap();
                completed
                    .recv_timeout(std::time::Duration::from_secs(10))
                    .unwrap();
                revoked = true;
            }
            Ok(())
        });
        worker.join().unwrap();
        assert!(revoked, "revoke checkpoint not reached");
        let generation = f.generation();
        let hits = f.hits("allowedcanary");
        json!({"status":if result.is_err() && generation == previous && hits == 0 {"PASS"} else {"FAIL"},
            "guard_calls":calls,"revoke_checkpoint":baseline-1,"operation_ok":result.is_ok(),
            "generation_before":previous,"generation_after":generation,"post_revoke_hits":hits,
            "expected":"revocation before commit aborts publication"})
    }
    fn revoke_search_return() -> Value {
        let f = Fixture::new();
        fs::write(f.root.join("safe.md"), "allowedcanary").unwrap();
        f.refresh();
        let mut baseline = 0;
        assert_eq!(
            f.index
                .search(&f.state, "allowedcanary", 25, || {
                    baseline += 1;
                    Ok(())
                })
                .unwrap()
                .len(),
            1
        );
        let mut calls = 0;
        let mut revoked = false;
        let (revoke, completed, worker) = revoker(&f);
        let result = f.index.search(&f.state, "allowedcanary", 25, || {
            calls += 1;
            if calls == baseline {
                revoke.send(()).unwrap();
                completed
                    .recv_timeout(std::time::Duration::from_secs(10))
                    .unwrap();
                revoked = true;
            }
            Ok(())
        });
        worker.join().unwrap();
        assert!(revoked, "final search checkpoint not reached");
        let disclosed = result.as_ref().map(|hits| hits.len()).unwrap_or(0);
        assert_eq!(f.hits("allowedcanary"), 0); // Subsequent requests are fenced.
        json!({"status":if disclosed == 0 {"PASS"} else {"FAIL"},"guard_calls":calls,
            "revoke_checkpoint":baseline,"revoked_before_return":true,"returned_hits":disclosed,
            "expected":"zero disclosure when root is revoked before search returns"})
    }
    fn lock_boundary() -> Value {
        let f = Fixture::new();
        fs::write(f.root.join("safe.md"), "allowedcanary").unwrap();
        f.refresh();
        let generation = f.generation();
        let mut count = 0;
        assert!(f
            .index
            .refresh_root(&f.state, &f.id, || {
                count += 1;
                if count >= 3 {
                    Err(IndexError::Revoked)
                } else {
                    Ok(())
                }
            })
            .is_err());
        assert_eq!(f.generation(), generation);
        let mut baseline = 0;
        f.index
            .search(&f.state, "allowedcanary", 25, || {
                baseline += 1;
                Ok(())
            })
            .unwrap();
        let mut count = 0;
        assert!(f
            .index
            .search(&f.state, "allowedcanary", 25, || {
                count += 1;
                if count == baseline {
                    Err(IndexError::Revoked)
                } else {
                    Ok(())
                }
            })
            .is_err());
        let mut scanner = DocumentScanner::new(f.state.clone(), ScanPolicy::default()).unwrap();
        assert!(scanner
            .rescan(&f.id, || Err(StateError::DocumentDenied))
            .is_err());
        json!({"status":"PASS","lock_guard_denied":true,"generation_unchanged":true})
    }
    pub fn run() {
        let cases: [(&str, fn() -> Value); 10] = [
            ("junction_containment", junctions),
            ("symlink_containment", symlinks),
            ("hardlink_and_atomic_generation", hardlinks),
            ("root_replacement_restart", replacement),
            ("missed_events_reconciliation", reconciliation),
            ("concurrent_junction_swap", junction_race),
            ("revoke_during_scan", revoke_scan),
            ("revoke_before_index_commit", revoke_index_commit),
            ("revoke_before_search_return", revoke_search_return),
            ("lock_guard_boundary", lock_boundary),
        ];
        let mut receipts = Vec::new();
        for (name, case) in cases {
            let start = Instant::now();
            let mut receipt = std::panic::catch_unwind(case).unwrap_or_else(|panic| {
                let message = panic
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_else(|| "native_case_panicked".into());
                json!({"status":"FAIL","reason":message})
            });
            receipt["case"] = json!(name);
            receipt["elapsed_ms"] = json!(start.elapsed().as_millis());
            println!("{}", receipt);
            receipts.push(receipt);
        }
        let failed = receipts.iter().any(|r| r["status"] == "FAIL");
        let blocked = receipts.iter().any(|r| r["status"] == "BLOCKED");
        let status = if failed {
            "FAIL"
        } else if blocked {
            "PARTIAL"
        } else {
            "PASS"
        };
        let report = json!({"schema":"mind-place/mp-4l1/v1","task_id":"MP-4L1","status":status,
            "product_sha":std::env::var("MP_NTFS_PRODUCT_SHA").unwrap(),"cases":receipts,
            "scope":"local native NTFS core APIs; synthetic fixtures; no installed UI or private corpus",
            "not_run":["MP-4L2 representative corpus","MP-4U owner UI acceptance","five-minute scheduler wall-clock timing"],
            "fixture_retention":"kept outside repository for inspection; no recursive cleanup"});
        fs::write(
            std::env::var_os("MP_NTFS_RECEIPT").unwrap(),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
        println!("MP-4L1 {status}");
        if status != "PASS" {
            std::process::exit(1);
        }
    }
}

fn main() {
    #[cfg(windows)]
    native::run();
    #[cfg(not(windows))]
    {
        eprintln!("MP-4L1 BLOCKED: requires native Windows NTFS");
        std::process::exit(2);
    }
}
