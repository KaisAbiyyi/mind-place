//! MP-4L2 owner-approved real-file sample, staged on D: outside source repositories.
//! Only a disposable staging marker is mutated; evidence never includes extracted text.
#[cfg(windows)]
fn main() {
    use mind_place_documents::{
        extract_bytes, extract_with_check, DocumentFormat, ExtractionLimits,
    };
    use mind_place_index::documents::DocumentIndex;
    use mind_place_state::{
        DocumentObjectStore, DocumentScanner, ScanPolicy, StateStore, VirtualDocumentPath,
    };
    use serde_json::{json, Value};
    use std::{collections::BTreeMap, fs, path::PathBuf, time::Instant};

    let root = PathBuf::from(std::env::var_os("MP_REAL_STAGING").expect("staging env"));
    let evidence = PathBuf::from(std::env::var_os("MP_REAL_EVIDENCE").expect("evidence env"));
    assert!(root.is_absolute() && evidence.is_absolute());
    assert!(
        root.starts_with(&evidence),
        "only disposable staging under evidence"
    );
    let state = StateStore::open(evidence.join("authority/state.sqlite3")).unwrap();
    let id = state.register_document_root(&root).unwrap().id;
    state.approve_document_root(&id).unwrap();
    let index = DocumentIndex::open(evidence.join("document-index.sqlite3")).unwrap();
    let mut scanner = DocumentScanner::new(state.clone(), ScanPolicy::default()).unwrap();

    let mut by_format: BTreeMap<String, Value> = BTreeMap::new();
    for entry in fs::read_dir(&root).unwrap() {
        let entry = entry.unwrap();
        if !entry.file_type().unwrap().is_file() {
            continue;
        }
        let path = entry.path();
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let Some(format) = DocumentFormat::from_extension(&ext) else {
            continue;
        };
        let bytes = fs::read(&path).unwrap();
        let result = extract_bytes(format, &bytes, ExtractionLimits::default());
        let item = by_format.entry(ext).or_insert_with(
            || json!({"files":0,"extract_ok":0,"nonempty":0,"warnings":0,"errors":0}),
        );
        item["files"] = json!(item["files"].as_u64().unwrap() + 1);
        match result {
            Ok(extracted) => {
                item["extract_ok"] = json!(item["extract_ok"].as_u64().unwrap() + 1);
                if !extracted.text.trim().is_empty() {
                    item["nonempty"] = json!(item["nonempty"].as_u64().unwrap() + 1);
                }
                item["warnings"] =
                    json!(item["warnings"].as_u64().unwrap() + extracted.warnings.len() as u64);
            }
            Err(_) => item["errors"] = json!(item["errors"].as_u64().unwrap() + 1),
        }
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let uri = VirtualDocumentPath::from_segments(&id, &[name])
            .unwrap()
            .to_uri();
        let started = Instant::now();
        let mut auth_calls = 0u64;
        let checked = extract_with_check(format, &bytes, ExtractionLimits::default(), || {
            auth_calls += 1;
            state.require_approved_document_uri(&uri).is_ok()
        });
        let label = match &checked {
            Ok(_) => "checked_ok",
            Err(mind_place_documents::ExtractionError::Limit("elapsed_millis")) => {
                "checked_elapsed_limit"
            }
            Err(mind_place_documents::ExtractionError::Limit(_)) => "checked_other_limit",
            Err(mind_place_documents::ExtractionError::Revoked) => "checked_revoked",
            Err(mind_place_documents::ExtractionError::Invalid) => "checked_invalid",
            Err(mind_place_documents::ExtractionError::Unsupported) => "checked_unsupported",
        };
        item[label] = json!(item[label].as_u64().unwrap_or(0) + 1);
        item["checked_auth_calls"] =
            json!(item["checked_auth_calls"].as_u64().unwrap_or(0) + auth_calls);
        item["checked_total_ms"] = json!(
            item["checked_total_ms"].as_u64().unwrap_or(0) + started.elapsed().as_millis() as u64
        );
    }

    let start = Instant::now();
    let first = scanner.rescan(&id, || Ok(())).unwrap();
    let initial_scan_ms = start.elapsed().as_millis();
    let start = Instant::now();
    let first_index = index.refresh_root(&state, &id, || Ok(())).unwrap();
    let initial_index_ms = start.elapsed().as_millis();
    let start = Instant::now();
    let noop = scanner.rescan(&id, || Ok(())).unwrap();
    let noop_scan_ms = start.elapsed().as_millis();
    let start = Instant::now();
    let noop_index = index.refresh_root(&state, &id, || Ok(())).unwrap();
    let noop_index_ms = start.elapsed().as_millis();

    let marker = root.join("local_revision_probe.md");
    assert_eq!(fs::read(&marker).unwrap(), b"mp4l2olduniquemarker2026\n");
    let old_uri = VirtualDocumentPath::from_segments(&id, &["local_revision_probe.md".to_owned()])
        .unwrap()
        .to_uri();
    let objects = DocumentObjectStore::open(state.clone()).unwrap();
    let old = objects
        .current_version(&old_uri, || Ok(()))
        .unwrap()
        .unwrap();
    let old_hits = index
        .search(&state, "mp4l2olduniquemarker2026", 25, || Ok(()))
        .unwrap()
        .len();
    fs::write(&marker, b"mp4l2newuniquemarker2026\n").unwrap();
    let start = Instant::now();
    let changed = scanner.rescan(&id, || Ok(())).unwrap();
    let changed_scan_ms = start.elapsed().as_millis();
    let start = Instant::now();
    let changed_index = index.refresh_root(&state, &id, || Ok(())).unwrap();
    let changed_index_ms = start.elapsed().as_millis();
    let new_head = objects
        .current_version(&old_uri, || Ok(()))
        .unwrap()
        .unwrap();
    let stale_hits = index
        .search(&state, "mp4l2olduniquemarker2026", 25, || Ok(()))
        .unwrap()
        .len();
    let new_hits = index
        .search(&state, "mp4l2newuniquemarker2026", 25, || Ok(()))
        .unwrap()
        .len();
    let versions = objects
        .versions(&old_uri, None, 10, || Ok(()))
        .unwrap()
        .len();
    let history_readable = objects.read_version(&old_uri, &old.id, || Ok(())).unwrap()
        == b"mp4l2olduniquemarker2026\n";
    let basic_ok = first.complete
        && noop.complete
        && changed.complete
        && first.visited_files >= 5
        && first.new_versions == first.visited_files
        && noop.new_versions == 0
        && noop.unchanged_versions == first.visited_files
        && changed.new_versions == 1
        && changed.unchanged_versions + 1 == first.visited_files
        && old_hits == 1
        && stale_hits == 0
        && new_hits == 1
        && new_head.id != old.id
        && versions == 2
        && history_readable;
    let real_office = ["pdf", "docx", "pptx"].iter().all(|s| {
        by_format
            .get(*s)
            .map(|v| v["nonempty"].as_u64().unwrap_or(0) > 0)
            .unwrap_or(false)
    });
    let overall = if basic_ok
        && real_office
        && first_index.skipped_files == 0
        && changed_index.skipped_files == 0
    {
        "PASS"
    } else {
        "PARTIAL"
    };
    let receipt = json!({
        "task":"MP-4L2", "kind":"native_real_staged_corpus", "status":overall,
        "product_main_sha":"08b310b2b14639594edcf5210d9a28b8b677846f",
        "approved_source_root":"D drive / owner-approved projects, locally staged subset",
        "source_files_unchanged":true,
        "formats":by_format,
        "initial":{"scan_ms":initial_scan_ms,"index_ms":initial_index_ms,"files":first.visited_files,"new_versions":first.new_versions,"indexed_files":first_index.indexed_files,"skipped_files":first_index.skipped_files},
        "noop":{"scan_ms":noop_scan_ms,"index_ms":noop_index_ms,"new_versions":noop.new_versions,"unchanged_versions":noop.unchanged_versions,"indexed_files":noop_index.indexed_files},
        "changed":{"scan_ms":changed_scan_ms,"index_ms":changed_index_ms,"new_versions":changed.new_versions,"unchanged_versions":changed.unchanged_versions,"indexed_files":changed_index.indexed_files,"skipped_files":changed_index.skipped_files},
        "version_history_two_versions":versions==2,"old_version_readable":history_readable,
        "stale_search_suppressed":stale_hits==0,"new_search_fresh":new_hits==1,
        "scope":"Real sampled input files staged locally; one disposable marker edited, not original source; filenames/excerpts omitted",
        "not_qualified":"Live original tree re-scan, user UI, inaccessible or unsampled files, optional XLSX if not found"
    });
    fs::write(
        evidence.join("real-receipt.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
    println!("{}", receipt);
    if !basic_ok {
        std::process::exit(1);
    }
    if overall != "PASS" {
        std::process::exit(3);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Native Windows only");
    std::process::exit(2);
}
