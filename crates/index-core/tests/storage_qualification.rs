//! Reproducible archive-to-FTS scale qualification (synthetic, no user data).
//! Run explicitly with MP_QUAL_CHUNKS=10000|100000|1000000.
//! This is ignored in normal unit tests; Windows qualification CI runs all tiers.
use mind_place_archive::{ArchiveStore, Capture, Message};
use mind_place_index::{ContextPreset, ContextRequest, IndexStore, QueryTerm, RetrievalRequest};
use mind_place_state::{canonical_source_id, MemoryClaim, SourceDescriptor, SourceKey, StateStore};
use rusqlite::Connection;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    time::Instant,
};

const MESSAGES_PER_CONVERSATION: usize = 10_000;
const NAMESPACE: &str = "mind-place-chatgpt-archive-v1";

fn conversation_id(number: usize) -> String {
    format!("00000000-0000-4000-8000-{number:012x}")
}

fn capture(number: usize, messages: usize, last: bool) -> Capture {
    let conversation_id = conversation_id(number);
    let messages = (0..messages)
        .map(|part| {
            let marker = if number == 0 && part == 0 {
                " firstrareneedle"
            } else if last && part == messages - 1 {
                " lastrareneedle"
            } else {
                ""
            };
            Message {
                id: format!("message-{part:05}"),
                role: if part % 2 == 0 { "user" } else { "assistant" }.into(),
                text: format!(
                    "archival qualification passage conversation {number} message {part} with durable provenance{marker}"
                ),
                format: "plain".into(),
                authored_at: None,
                model: None,
                attachments: vec![],
            }
        })
        .collect();

    Capture {
        schema_version: 1,
        provider: "chatgpt".into(),
        conversation_id: conversation_id.clone(),
        source_url: format!("https://chatgpt.com/c/{conversation_id}"),
        title: format!("Qualification conversation {number}"),
        conversation_created_at: None,
        conversation_updated_at: None,
        captured_at: "2026-10-18T08:00:00Z".into(),
        capture_method: "manual-dom".into(),
        completeness: "partial".into(),
        warnings: vec![],
        messages,
    }
}

fn raw_hashes(archive: &ArchiveStore, snapshots: &[(String, String)]) -> BTreeMap<String, String> {
    snapshots
        .iter()
        .map(|(archive_id, snapshot_id)| {
            let name = format!("{archive_id}/{snapshot_id}");
            let path = archive
                .root()
                .join("conversations")
                .join(archive_id)
                .join("captures")
                .join(format!("{snapshot_id}.jsonl"));
            let digest = format!("{:x}", Sha256::digest(fs::read(path).unwrap()));
            (name, digest)
        })
        .collect()
}

fn canonical(number: usize) -> String {
    let key = conversation_id(number);
    canonical_source_id(&SourceDescriptor {
        provider: "chatgpt",
        source_namespace: NAMESPACE,
        source_kind: "conversation",
        key: SourceKey::External(&key),
    })
    .unwrap()
}

fn db_bytes(path: &Path) -> u64 {
    ["", "-wal", "-shm"]
        .iter()
        .map(|suffix| {
            let filename = format!("{}{suffix}", path.file_name().unwrap().to_string_lossy());
            fs::metadata(path.with_file_name(filename))
                .map(|meta| meta.len())
                .unwrap_or(0)
        })
        .sum()
}

fn delete_disposable_index(path: &Path) {
    // All IndexStore operations open and close their own SQLite connections.
    // Delete only the disposable index and its sidecars, never state or raw files.
    fs::remove_file(path).unwrap();
    for suffix in ["-wal", "-shm"] {
        let filename = format!("{}{suffix}", path.file_name().unwrap().to_string_lossy());
        let sidecar = path.with_file_name(filename);
        if sidecar.exists() {
            fs::remove_file(sidecar).unwrap();
        }
    }
}

fn fts_integrity(path: &Path) {
    let db = Connection::open(path).unwrap();
    let foreign_keys: i64 = db
        .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(foreign_keys, 0, "derived FK violation");
    let health: String = db
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .unwrap();
    assert_eq!(health, "ok", "derived index integrity");
    // rank=1 also compares external FTS rows against the content table.
    db.execute(
        "INSERT INTO chunks_fts(chunks_fts, rank) VALUES('integrity-check', 1)",
        [],
    )
    .unwrap();
}

fn median_us(mut action: impl FnMut()) -> u128 {
    action();
    let mut samples = Vec::with_capacity(9);
    for _ in 0..9 {
        let start = Instant::now();
        action();
        samples.push(start.elapsed().as_micros());
    }
    samples.sort_unstable();
    samples[4]
}

#[test]
#[ignore = "explicit Windows CI/storage qualification only"]
fn qualifies_archive_scale() {
    let count: usize = env::var("MP_QUAL_CHUNKS")
        .expect("MP_QUAL_CHUNKS must be set explicitly")
        .parse()
        .expect("valid integer chunk tier");
    assert!([10_000, 100_000, 1_000_000].contains(&count));

    let dir = tempfile::tempdir().unwrap();
    let archive = ArchiveStore::open(dir.path().join("archive-v1")).unwrap();
    let state_path = dir.path().join("state.sqlite3");
    let index_path = dir.path().join("index.sqlite3");
    let state = StateStore::open(&state_path).unwrap();
    let index = IndexStore::open(&index_path).unwrap();
    let total_conversations = count / MESSAGES_PER_CONVERSATION;
    let mut receipts = Vec::with_capacity(total_conversations);

    let generate_started = Instant::now();
    for number in 0..total_conversations {
        let receipt = archive
            .save(capture(
                number,
                MESSAGES_PER_CONVERSATION,
                number == total_conversations - 1,
            ))
            .unwrap();
        receipts.push((receipt.id, receipt.snapshot_id));
    }
    let generated_ms = generate_started.elapsed().as_millis();
    let evidence_before = raw_hashes(&archive, &receipts);

    let sync_started = Instant::now();
    let initial = index.sync(&archive, &state).unwrap();
    let sync_ms = sync_started.elapsed().as_millis();
    assert_eq!(initial.sources as usize, total_conversations);
    assert_eq!(initial.revisions as usize, total_conversations);
    assert_eq!(initial.chunks as usize, count + total_conversations);
    assert_eq!(initial.inserted_revisions as usize, total_conversations);

    let first_id = canonical(0);
    let last_id = canonical(total_conversations - 1);
    assert!(state.source(&first_id).unwrap().is_some());
    assert!(state.source(&last_id).unwrap().is_some());

    // This is user-authored state, not reconstructible from the disposable FTS.
    state
        .create_project("qualification-project", "Durable project")
        .unwrap();
    state
        .create_memory_claim(&MemoryClaim {
            id: "qualification-claim".into(),
            statement: "Canonical evidence survives index deletion".into(),
            asserted_by: "user".into(),
            epistemic_origin: "explicit".into(),
            status: "active".into(),
        })
        .unwrap();
    state
        .add_claim_source_evidence("qualification-claim", &last_id, "source")
        .unwrap();

    let lookup_started = Instant::now();
    let first = index.search("firstrareneedle", false, 10).unwrap();
    let last = index.search("lastrareneedle", false, 10).unwrap();
    let lookup_ms = lookup_started.elapsed().as_millis();
    assert_eq!(first.len(), 1);
    assert_eq!(last.len(), 1);
    assert_eq!(first[0].source_id, first_id);
    assert_eq!(last[0].source_id, last_id);
    assert_eq!(first[0].message_id, "message-00000");
    assert_eq!(last[0].message_id, "message-09999");
    fts_integrity(&index_path);

    // Same runner and 1E corpus: relative guardrails, not user-device SLAs.
    let ranked_query = RetrievalRequest {
        queries: vec![QueryTerm::Text("firstrareneedle".into())],
        ..Default::default()
    };
    let context_query = ContextRequest {
        queries: vec![QueryTerm::Text("firstrareneedle".into())],
        preset: ContextPreset::Fast,
        ..Default::default()
    };
    let legacy_median_us = median_us(|| {
        assert_eq!(index.search("firstrareneedle", false, 10).unwrap().len(), 1);
    });
    let ranked_median_us = median_us(|| {
        let result = index.inspect_retrieval(&ranked_query).unwrap();
        assert_eq!(result.result.hits.len(), 1);
        assert_eq!(result.result.hits[0].source_id, first_id);
        assert_eq!(result.result.hits[0].score_contributions.len(), 1);
    });
    let context_median_us = median_us(|| {
        let result = index.inspect_context(&archive, &context_query).unwrap();
        assert_eq!(result.result.source_count, 1);
        assert_eq!(result.result.groups[0].source_id, first_id);
    });
    let ranked_gate_us = (legacy_median_us * 40).max(100_000);
    let context_gate_us = (legacy_median_us * 200).max(2_000_000);
    assert!(ranked_median_us <= ranked_gate_us, "ranked inspection latency regression");
    assert!(context_median_us <= context_gate_us, "context inspection latency regression");
    let ranked_before_rebuild = index.retrieve(&ranked_query).unwrap();

    let noop_started = Instant::now();
    let noop = index.sync(&archive, &state).unwrap();
    let noop_ms = noop_started.elapsed().as_millis();
    assert_eq!(noop.inserted_revisions, 0);
    assert_eq!(noop.chunks, initial.chunks);
    let first_index_bytes = db_bytes(&index_path);

    // Physical deletion is intentional; state/source IDs and JSONL are sacred.
    delete_disposable_index(&index_path);
    let rebuilt = IndexStore::open(&index_path).unwrap();
    assert_eq!(rebuilt.stats().unwrap().chunks, 0);
    assert!(rebuilt
        .search("firstrareneedle", false, 10)
        .unwrap()
        .is_empty());

    let rebuild_started = Instant::now();
    let restored = rebuilt.rebuild(&archive, &state).unwrap();
    let rebuild_ms = rebuild_started.elapsed().as_millis();
    assert_eq!(restored.sources, initial.sources);
    assert_eq!(restored.revisions, initial.revisions);
    assert_eq!(restored.chunks, initial.chunks);
    assert_eq!(restored.inserted_revisions, initial.inserted_revisions);
    assert_eq!(raw_hashes(&archive, &receipts), evidence_before);
    // Reopening validates the authoritative state schema and persistent identities.
    let reopened_state = StateStore::open(&state_path).unwrap();
    assert!(reopened_state.source(&first_id).unwrap().is_some());
    assert!(reopened_state.source(&last_id).unwrap().is_some());
    assert_eq!(
        reopened_state
            .get_project("qualification-project")
            .unwrap()
            .unwrap()
            .name,
        "Durable project"
    );
    let canonical_db = Connection::open(&state_path).unwrap();
    let evidence_count: i64 = canonical_db
        .query_row(
            "SELECT COUNT(*) FROM claim_evidence
         WHERE claim_id='qualification-claim' AND source_ref=?1",
            [&last_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(evidence_count, 1, "curated evidence reference survived");
    let claim_count: i64 = canonical_db
        .query_row(
            "SELECT COUNT(*) FROM memory_claims WHERE id='qualification-claim'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(claim_count, 1);
    drop(canonical_db);

    let post_lookup_started = Instant::now();
    assert_eq!(rebuilt.search("firstrareneedle", false, 10).unwrap(), first);
    assert_eq!(rebuilt.search("lastrareneedle", false, 10).unwrap(), last);
    let restored_lookup_ms = post_lookup_started.elapsed().as_millis();
    fts_integrity(&index_path);

    let receipt = json!({
        "schema": "mind-place-phase-1e-storage-qualification-v1",
        "kind": "synthetic-archive-end-to-end",
        "platform": env::consts::OS,
        "sqlite_version": rusqlite::version(),
        "target_message_chunks": count,
        "actual_fts_chunks": restored.chunks,
        "conversations": total_conversations,
        "revisions": restored.revisions,
        "generated_ms": generated_ms,
        "initial_sync_ms": sync_ms,
        "rare_lookup_pair_ms": lookup_ms,
        "noop_sync_ms": noop_ms,
        "delete_and_rebuild_ms": rebuild_ms,
        "restored_lookup_pair_ms": restored_lookup_ms,
        "index_bytes_before_delete": first_index_bytes,
        "index_bytes_after_rebuild": db_bytes(&index_path),
        "raw_snapshot_hashes_preserved": true,
        "authority_source_identities_and_claim_evidence_preserved": true,
        "sqlite_fts5_and_fk_integrity": "pass"
    });
    let rendered = serde_json::to_string_pretty(&receipt).unwrap();
    println!("MP_PHASE_1E_RECEIPT {rendered}");
    if let Ok(output) = env::var("MP_QUAL_RECEIPT") {
        let output = PathBuf::from(output);
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(output, format!("{rendered}\n")).unwrap();
    }
}
