//! MP-10C: opt-in real approved-root -> scanner -> object store -> FTS qualification.
//! Only synthetic temporary files. Never access an owner's archive.
#![cfg(windows)]
use mind_place_index::documents::DocumentIndex;
use mind_place_state::StateStore;
use rusqlite::Connection;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    path::{Path, PathBuf},
    time::Instant,
};

const ROOT_MAX: usize = 10_000;
const SECTIONS: usize = 10;

fn fixture(root: usize, file: usize, last: bool) -> String {
    let mut result = String::new();
    for section in 0..SECTIONS {
        let marker = if root == 0 && file == 0 && section == 0 {
            " mp10cfirstneedle"
        } else if last && section == SECTIONS - 1 {
            " mp10clastneedle"
        } else {
            ""
        };
        result.push_str(&format!("## Section {section}\nSynthetic evidence root {root:02} file {file:05} segment {section} and provenance{marker}\n"));
    }
    result
}
fn filename(root: &Path, file: usize) -> PathBuf {
    root.join(format!("doc-{file:05}.md"))
}
fn db_bytes(path: &Path) -> u64 {
    ["", "-wal", "-shm"]
        .iter()
        .map(|ext| {
            let name = format!("{}{ext}", path.file_name().unwrap().to_string_lossy());
            fs::metadata(path.with_file_name(name))
                .map(|v| v.len())
                .unwrap_or(0)
        })
        .sum()
}
fn hash_files(roots: &[PathBuf], total: usize) -> String {
    let mut digest = Sha256::new();
    for (i, root) in roots.iter().enumerate() {
        for file in 0..total.saturating_sub(i * ROOT_MAX).min(ROOT_MAX) {
            digest.update(format!("{i:02}/{file:05}\0").as_bytes());
            digest.update(fs::read(filename(root, file)).unwrap());
        }
    }
    format!("{:x}", digest.finalize())
}
#[repr(C)]
#[derive(Default)]
struct Counters {
    cb: u32,
    faults: u32,
    peak_working_set: usize,
    working_set: usize,
    peak_paged_pool: usize,
    paged_pool: usize,
    peak_nonpaged_pool: usize,
    nonpaged_pool: usize,
    pagefile: usize,
    peak_pagefile: usize,
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentProcess() -> *mut std::ffi::c_void;
}
#[link(name = "psapi")]
unsafe extern "system" {
    fn GetProcessMemoryInfo(process: *mut std::ffi::c_void, out: *mut Counters, size: u32) -> i32;
}
fn peak_rss() -> Option<u64> {
    let mut c = Counters {
        cb: std::mem::size_of::<Counters>() as u32,
        ..Default::default()
    };
    let success = unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) };
    (success != 0).then_some(c.peak_working_set as u64)
}
fn median_us(mut f: impl FnMut()) -> u128 {
    f();
    let mut samples = Vec::new();
    for _ in 0..7 {
        let begin = Instant::now();
        f();
        samples.push(begin.elapsed().as_micros());
    }
    samples.sort_unstable();
    samples[3]
}

#[test]
#[ignore = "explicit MP-10C Windows 100/100000 real-document qualification"]
fn qualifies_document_scale() {
    let files: usize = env::var("MP_10C_FILES")
        .expect("set MP_10C_FILES")
        .parse()
        .unwrap();
    assert!([100, 100_000].contains(&files), "never silently downscale");
    let dir = tempfile::tempdir().unwrap();
    let state_path = dir.path().join("state.sqlite3");
    let index_path = dir.path().join("document-index.sqlite3");
    let state = StateStore::open(&state_path).unwrap();
    let index = DocumentIndex::open(&index_path).unwrap();
    let roots = files.div_ceil(ROOT_MAX);
    let mut paths = Vec::new();
    let mut ids = Vec::new();
    let started = Instant::now();
    for root in 0..roots {
        let path = dir.path().join(format!("approved-{root:02}"));
        fs::create_dir(&path).unwrap();
        let in_root = (files - root * ROOT_MAX).min(ROOT_MAX);
        for file in 0..in_root {
            fs::write(
                filename(&path, file),
                fixture(root, file, root == roots - 1 && file == in_root - 1),
            )
            .unwrap();
        }
        let grant = state.register_document_root(&path).unwrap();
        state.approve_document_root(&grant.id).unwrap();
        ids.push(grant.id);
        paths.push(path);
        println!(
            "MP_10C_PROGRESS phase=fixture roots_done={}/{} files_done={} elapsed_ms={}",
            root + 1,
            roots,
            ((root + 1) * ROOT_MAX).min(files),
            started.elapsed().as_millis()
        );
    }
    let generation_ms = started.elapsed().as_millis();
    let raw_hash = hash_files(&paths, files);
    let started = Instant::now();
    let mut actual_files = 0;
    let mut actual_chunks = 0;
    let mut per_root_index_ms = Vec::new();
    for id in &ids {
        let root_started = Instant::now();
        let report = index.refresh_root(&state, id, || Ok(())).unwrap();
        assert_eq!(report.skipped_files, 0);
        assert_eq!(report.scanned_files, report.indexed_files);
        assert_eq!(report.indexed_chunks, report.indexed_files * SECTIONS);
        actual_files += report.indexed_files;
        actual_chunks += report.indexed_chunks;
        per_root_index_ms.push(root_started.elapsed().as_millis());
        println!(
            "MP_10C_PROGRESS phase=index roots_done={}/{} indexed_files={} extracted_chunks={} elapsed_ms={}",
            per_root_index_ms.len(),
            roots,
            actual_files,
            actual_chunks,
            started.elapsed().as_millis()
        );
    }
    let initial_index_ms = started.elapsed().as_millis();
    assert_eq!(actual_files, files);
    assert_eq!(actual_chunks, files * SECTIONS);
    let first = index
        .search(&state, "mp10cfirstneedle", 5, || Ok(()))
        .unwrap();
    let last = index
        .search(&state, "mp10clastneedle", 5, || Ok(()))
        .unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(last.len(), 1);
    assert_ne!(first[0].uri, last[0].uri);
    let query_median_us = median_us(|| {
        assert_eq!(
            index
                .search(&state, "mp10cfirstneedle", 5, || Ok(()))
                .unwrap(),
            first
        );
    });
    let started = Instant::now();
    let again = index.refresh_root(&state, &ids[0], || Ok(())).unwrap();
    let reconciliation_first_root_ms = started.elapsed().as_millis();
    println!(
        "MP_10C_PROGRESS phase=reconciliation files={} elapsed_ms={}",
        files.min(ROOT_MAX),
        reconciliation_first_root_ms
    );
    assert_eq!(again.indexed_chunks, files.min(ROOT_MAX) * SECTIONS);
    let db = Connection::open(&index_path).unwrap();
    let total: i64 = db
        .query_row("SELECT COUNT(*) FROM document_fts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(total as usize, actual_chunks);
    let health: String = db
        .query_row("PRAGMA quick_check(1)", [], |r| r.get(0))
        .unwrap();
    assert_eq!(health, "ok");
    db.execute(
        "INSERT INTO document_fts(document_fts,rank) VALUES('integrity-check',1)",
        [],
    )
    .unwrap();
    drop(db);
    let started = Instant::now();
    let reopened_state = StateStore::open(&state_path).unwrap();
    let reopened_index = DocumentIndex::open(&index_path).unwrap();
    let reopen_ms = started.elapsed().as_millis();
    assert_eq!(
        reopened_index
            .search(&reopened_state, "mp10clastneedle", 5, || Ok(()))
            .unwrap(),
        last
    );
    assert_eq!(hash_files(&paths, files), raw_hash);
    let report = json!({
        "schema": "mind-place-mp10c-document-scale/v1",
        "fixture": "synthetic-only", "mode": "real-approved-root-index",
        "platform": env::consts::OS, "sqlite_version": rusqlite::version(),
        "target_files": files, "indexed_files": actual_files, "extracted_chunks": actual_chunks,
        "approved_roots": roots, "production_files_per_root_cap": ROOT_MAX,
        "generation_ms": generation_ms, "initial_index_ms": initial_index_ms,
        "per_root_index_ms": per_root_index_ms,
        "reconciliation_first_root_ms": reconciliation_first_root_ms,
        "evidence_search_median_us": query_median_us, "state_and_fts_reopen_ms": reopen_ms,
        "peak_process_working_set_bytes": peak_rss(),
        "index_disk_bytes": db_bytes(&index_path), "state_disk_bytes": db_bytes(&state_path),
        "fixture_sha256": raw_hash, "raw_bytes_unchanged": true, "sqlite_and_fts_integrity": "pass",
        "owner_device": "NOT_RUN", "webview_cold_start": "NOT_RUN"
    });
    let output = serde_json::to_string_pretty(&report).unwrap();
    println!("MP_10C_RECEIPT {output}");
    if let Ok(location) = env::var("MP_10C_RECEIPT") {
        let path = PathBuf::from(location);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, format!("{output}\n")).unwrap();
    }
}
