use mind_place_archive::{ArchiveStore, Capture, Message};
use mind_place_index::{IndexError, IndexStore};
use mind_place_state::{canonical_source_id, SourceDescriptor, SourceKey, StateStore};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};
use tempfile::TempDir;

const CONVERSATION: &str = "11111111-2222-4333-8444-555555555555";

fn capture(text: &str) -> Capture {
    Capture {
        schema_version: 1,
        provider: "chatgpt".into(),
        conversation_id: CONVERSATION.into(),
        source_url: format!("https://chatgpt.com/c/{CONVERSATION}"),
        title: "Project constellation notes".into(),
        conversation_created_at: None,
        conversation_updated_at: None,
        captured_at: "2026-10-18T06:00:00Z".into(),
        capture_method: "manual-dom".into(),
        completeness: "partial".into(),
        warnings: vec![],
        messages: vec![
            Message {
                id: "message-one".into(),
                role: "user".into(),
                text: text.into(),
                format: "plain".into(),
                authored_at: None,
                model: None,
                attachments: vec![],
            },
            Message {
                id: "message-two".into(),
                role: "assistant".into(),
                text: "Index the approved raw record without changing it.".into(),
                format: "plain".into(),
                authored_at: None,
                model: Some("test-model".into()),
                attachments: vec![],
            },
        ],
    }
}

struct Rig {
    dir: TempDir,
    archive: ArchiveStore,
    state: StateStore,
    index: IndexStore,
}

impl Rig {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let archive = ArchiveStore::open(dir.path().join("archive-v1")).unwrap();
        let state = StateStore::open(dir.path().join("state.sqlite3")).unwrap();
        let index = IndexStore::open(dir.path().join("index.sqlite3")).unwrap();
        Self {
            dir,
            archive,
            state,
            index,
        }
    }

    fn raw(&self, archive_id: &str, snapshot: &str) -> Vec<u8> {
        fs::read(
            self.archive
                .root()
                .join("conversations")
                .join(archive_id)
                .join("captures")
                .join(format!("{snapshot}.jsonl")),
        )
        .unwrap()
    }
}

#[test]
fn indexes_current_and_history_incrementally_without_mutating_raw() {
    let rig = Rig::new();
    let first = rig
        .archive
        .save(capture("violetcomet first observation"))
        .unwrap();
    let before = rig.raw(&first.id, &first.snapshot_id);
    let initial = rig.index.sync(&rig.archive, &rig.state).unwrap();
    assert_eq!(initial.sources, 1);
    assert_eq!(initial.revisions, 1);
    assert_eq!(initial.inserted_revisions, 1);
    assert!(initial.chunks >= 3);
    let found = rig.index.search("violetcomet", false, 10).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].snapshot_id, first.snapshot_id);
    assert_eq!(found[0].message_id, "message-one");
    assert_eq!(found[0].kind, "message");

    let expected = canonical_source_id(&SourceDescriptor {
        provider: "chatgpt",
        source_namespace: "mind-place-chatgpt-archive-v1",
        source_kind: "conversation",
        key: SourceKey::External(CONVERSATION),
    })
    .unwrap();
    assert_eq!(found[0].source_id, expected);
    assert!(rig.state.source(&expected).unwrap().is_some());

    let repeat = rig.index.sync(&rig.archive, &rig.state).unwrap();
    assert_eq!(repeat.inserted_revisions, 0);
    assert_eq!(repeat.chunks, initial.chunks);
    let second = rig
        .archive
        .save(capture("copperplanet second observation"))
        .unwrap();
    assert_ne!(first.snapshot_id, second.snapshot_id);
    let update = rig.index.sync(&rig.archive, &rig.state).unwrap();
    assert_eq!(update.inserted_revisions, 1);
    assert_eq!(update.revisions, 2);
    assert!(rig
        .index
        .search("violetcomet", false, 10)
        .unwrap()
        .is_empty());
    assert_eq!(rig.index.search("violetcomet", true, 10).unwrap().len(), 1);
    assert_eq!(
        rig.index.search("copperplanet", false, 10).unwrap().len(),
        1
    );
    assert_eq!(rig.raw(&first.id, &first.snapshot_id), before);
}

#[test]
fn deleting_disposable_index_then_rebuilding_preserves_authority_and_raw() {
    let rig = Rig::new();
    let saved = rig
        .archive
        .save(capture("silvertrail searchable truth"))
        .unwrap();
    rig.index.sync(&rig.archive, &rig.state).unwrap();
    let evidence_hash = Sha256::digest(rig.raw(&saved.id, &saved.snapshot_id));
    let identity = rig.index.search("silvertrail", false, 10).unwrap()[0]
        .source_id
        .clone();
    let path = rig.index.path().to_owned();
    remove_sqlite(&path);
    let recovered = IndexStore::open(&path).unwrap();
    assert!(recovered
        .search("silvertrail", false, 10)
        .unwrap()
        .is_empty());
    let stats = recovered.rebuild(&rig.archive, &rig.state).unwrap();
    assert_eq!(stats.revisions, 1);
    assert_eq!(stats.inserted_revisions, 1);
    assert_eq!(
        recovered.search("silvertrail", false, 10).unwrap()[0].source_id,
        identity
    );
    assert_eq!(
        Sha256::digest(rig.raw(&saved.id, &saved.snapshot_id)),
        evidence_hash
    );
    assert!(rig.state.source(&identity).unwrap().is_some());
}

fn remove_sqlite(path: &Path) {
    fs::remove_file(path).unwrap();
    for extension in ["-wal", "-shm"] {
        let sidecar = path.with_file_name(format!(
            "{}{extension}",
            path.file_name().unwrap().to_string_lossy()
        ));
        if sidecar.exists() {
            fs::remove_file(sidecar).unwrap();
        }
    }
}

#[test]
fn cancelled_incremental_write_rolls_back_old_index_generation() {
    let rig = Rig::new();
    rig.archive.save(capture("originapple durable")).unwrap();
    rig.index.sync(&rig.archive, &rig.state).unwrap();
    let previous = rig.index.stats().unwrap();
    rig.archive
        .save(capture("newbanana should not commit"))
        .unwrap();
    let mut attempts = 0;
    let error = rig.index.sync_with_check(&rig.archive, &rig.state, || {
        attempts += 1;
        if attempts >= 3 {
            Err(IndexError::Revoked)
        } else {
            Ok(())
        }
    });
    assert!(matches!(error, Err(IndexError::Revoked)));
    assert_eq!(rig.index.stats().unwrap(), previous);
    assert!(rig.index.search("originapple", false, 10).unwrap().len() == 1);
    assert!(rig.index.search("newbanana", true, 10).unwrap().is_empty());
}

#[test]
fn rejects_tampered_revision_without_rewriting_raw_or_index() {
    let rig = Rig::new();
    let saved = rig.archive.save(capture("authentic content")).unwrap();
    let path = rig
        .archive
        .root()
        .join("conversations")
        .join(&saved.id)
        .join("captures")
        .join(format!("{}.jsonl", saved.snapshot_id));
    let original = fs::read(&path).unwrap();
    let tampered = String::from_utf8(original.clone())
        .unwrap()
        .replace("authentic content", "replaced content");
    fs::write(&path, tampered.as_bytes()).unwrap();
    assert!(matches!(
        rig.index.sync(&rig.archive, &rig.state),
        Err(IndexError::InvalidEvidence)
    ));
    assert_eq!(rig.index.stats().unwrap().revisions, 0);
    assert_eq!(fs::read(&path).unwrap(), tampered.as_bytes());
}

#[test]
fn unicode_chunking_and_fts_queries_preserve_provenance() {
    let rig = Rig::new();
    let long = format!("{} ultravioletcluster", "日本語🌌".repeat(1200));
    let receipt = rig.archive.save(capture(&long)).unwrap();
    let stats = rig.index.sync(&rig.archive, &rig.state).unwrap();
    assert!(stats.chunks > 3);
    let hits = rig.index.search("ultravioletcluster", false, 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].snapshot_id, receipt.snapshot_id);
    assert!(rig.index.search(" ", false, 10).is_err());
    assert!(rig.index.search("cluster", false, 101).is_err());
}

#[test]
fn foreign_database_is_never_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let index_path = dir.path().join("index.sqlite3");
    let db = rusqlite::Connection::open(&index_path).unwrap();
    db.execute_batch(
        "CREATE TABLE foreign_data(secret TEXT); INSERT INTO foreign_data VALUES('keep');",
    )
    .unwrap();
    drop(db);
    assert!(matches!(
        IndexStore::open(&index_path),
        Err(IndexError::ForeignDatabase)
    ));
    let db = rusqlite::Connection::open(&index_path).unwrap();
    let value: String = db
        .query_row("SELECT secret FROM foreign_data", [], |row| row.get(0))
        .unwrap();
    assert_eq!(value, "keep");
}

#[test]
fn gemini_and_chatgpt_have_distinct_indexed_source_identities_and_revisions() {
    let rig = Rig::new();
    let original_chat = rig.archive.save(capture("chatgptonlyindexword")).unwrap();
    let mut gemini = capture("geminifirstindexword");
    gemini.provider = "gemini".into();
    gemini.conversation_id = "AbCd1234efGh5678".into();
    gemini.source_url = "https://gemini.google.com/app/AbCd1234efGh5678".into();
    gemini.messages[1].model = Some("gemini-synthetic".into());
    let first = rig.archive.save(gemini.clone()).unwrap();
    let old_raw = rig.raw(&first.id, &first.snapshot_id);
    rig.index.sync(&rig.archive, &rig.state).unwrap();
    let hit = rig.index.search("geminifirstindexword", false, 10).unwrap();
    assert_eq!(hit.len(), 1);
    let expected = canonical_source_id(&SourceDescriptor {
        provider: "gemini",
        source_namespace: "mind-place-gemini-archive-v1",
        source_kind: "conversation",
        key: SourceKey::External("AbCd1234efGh5678"),
    }).unwrap();
    assert_eq!(hit[0].source_id, expected);
    assert_ne!(first.id, original_chat.id);
    assert_eq!(rig.index.search("chatgptonlyindexword", false, 10).unwrap().len(), 1);

    gemini.messages[0].text = "geminiregeneratedindexword".into();
    let second = rig.archive.save(gemini).unwrap();
    assert_ne!(first.snapshot_id, second.snapshot_id);
    rig.index.sync(&rig.archive, &rig.state).unwrap();
    assert!(rig.index.search("geminifirstindexword", false, 10).unwrap().is_empty());
    assert_eq!(rig.index.search("geminifirstindexword", true, 10).unwrap().len(), 1);
    assert_eq!(rig.index.search("geminiregeneratedindexword", false, 10).unwrap().len(), 1);
    assert_eq!(rig.raw(&first.id, &first.snapshot_id), old_raw);
}
