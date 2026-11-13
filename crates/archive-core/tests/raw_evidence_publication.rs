//! MP-IMPORT-RAW-01: atomic raw-evidence and projection publication with an independently
//! measured source manifest. Synthetic fixtures only; no real export is read.
//!
//! Every test asserts on disk state (raw blobs, evidence, projections, job directory), not only
//! on return values.

use mind_place_archive::historical::{
    ImportJobLimits, ImportJobReceipt, ImportMember, ImportPoint, ImportProgress, ItemOutcome,
    ManifestLimits, MemberKind, SourceImportRequest, SourceManifest,
};
use mind_place_archive::importers::chatgpt_export::ChatGptExportParser;
use mind_place_archive::importers::BoundedArrayLimits;
use mind_place_archive::{ArchiveError, ArchiveStore, Result};
use mind_place_security::{ImportAuthorizationCapability, SecurityGate};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;

const CRED: &[u8] = b"synthetic-owner-credential";
const A: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const B: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const C: &str = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";
const EMPTY: &str = "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee";

// ---------- fixtures ----------

fn conv(id: &str, title: &str, text: &str) -> String {
    serde_json::to_string(&serde_json::json!({
        "id": id, "title": title, "create_time": 1700000000.5, "update_time": 1700000100.5,
        "current_node": "n2",
        "mapping": {
            "n1": {"id":"n1","parent":null,"children":["n2","n3"],"message":{
                "id":"m1","author":{"role":"user"},"create_time":1700000001.0,
                "content":{"content_type":"text","parts":[text]}}},
            "n2": {"id":"n2","parent":"n1","children":[],"message":{
                "id":"m2","author":{"role":"assistant"},"create_time":1700000002.0,
                "content":{"content_type":"text","parts":[format!("reply to {text}")]}}},
            "n3": {"id":"n3","parent":"n1","children":[],"message":{
                "id":"m3","author":{"role":"assistant"},"create_time":1700000003.0,
                "content":{"content_type":"text","parts":[format!("ALT-BRANCH {text}")]}}}
        }
    }))
    .unwrap()
}

fn empty_mapping(id: &str) -> String {
    serde_json::to_string(&serde_json::json!({"id": id, "title": "empty", "mapping": {}})).unwrap()
}

fn export(records: &[String]) -> Vec<u8> {
    format!("[\n  {}\n]\n", records.join(",\n  ")).into_bytes()
}

fn standard_export() -> Vec<u8> {
    export(&[
        conv(A, "First", "alpha question"),
        conv(B, "Second", "beta question"),
        empty_mapping(EMPTY),
        r#"{"x":1}"#.to_owned(),
    ])
}

fn three_conversations() -> Vec<u8> {
    export(&[
        conv(A, "First", "alpha question"),
        conv(B, "Second", "beta question"),
        conv(C, "Third", "gamma question"),
    ])
}

struct Hook<F: Fn(ImportPoint) -> Result<()>>(F);

impl<F: Fn(ImportPoint) -> Result<()>> ImportProgress for Hook<F> {
    fn checkpoint(&self, point: ImportPoint) -> Result<()> {
        (self.0)(point)
    }
}

fn crash_at(target: ImportPoint) -> Hook<impl Fn(ImportPoint) -> Result<()>> {
    Hook(move |point| {
        if point == target {
            Err(ArchiveError::Invalid("SIMULATED_CRASH".into()))
        } else {
            Ok(())
        }
    })
}

struct Fx {
    home: tempfile::TempDir,
    store: ArchiveStore,
    gate: SecurityGate,
    provider: &'static str,
}

impl Fx {
    fn new(provider: &'static str) -> Self {
        let home = tempfile::tempdir().unwrap();
        let store = ArchiveStore::open(home.path().join("archive-v1")).unwrap();
        let gate = SecurityGate::open(home.path().join("security-v1.json")).unwrap();
        gate.initialize(CRED).unwrap();
        Self {
            home,
            store,
            gate,
            provider,
        }
    }

    fn source(&self, name: &str, files: &[(&str, &[u8])]) -> PathBuf {
        let dir = self.home.path().join(name);
        for (path, bytes) in files {
            let file = dir.join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, bytes).unwrap();
        }
        dir
    }

    fn authorize(&self, dir: &Path) -> ImportAuthorizationCapability {
        let manifest = SourceManifest::measure(dir, ManifestLimits::default()).unwrap();
        self.gate
            .authorize_import_session(
                self.gate.require_unlocked().unwrap(),
                CRED,
                self.provider,
                dir,
                *manifest.sha256(),
                Duration::from_secs(300),
            )
            .unwrap()
    }

    fn run(
        &self,
        cap: &ImportAuthorizationCapability,
        members: &[ImportMember],
        progress: &dyn ImportProgress,
        limits: ImportJobLimits,
    ) -> Result<ImportJobReceipt> {
        let mut request = SourceImportRequest::new(
            &self.gate,
            self.gate.require_unlocked().unwrap(),
            cap,
            members,
        );
        request.progress = progress;
        request.limits = limits;
        self.store.import_authorized_source(&request)
    }

    fn run_default(&self, dir: &Path, members: &[ImportMember]) -> Result<ImportJobReceipt> {
        let cap = self.authorize(dir);
        self.run(
            &cap,
            members,
            &mind_place_archive::historical::NoProgress,
            ImportJobLimits::default(),
        )
    }

    fn archive(&self) -> &Path {
        self.store.root()
    }
}

fn chatgpt(path: &str) -> ImportMember {
    ImportMember {
        path: path.into(),
        kind: MemberKind::ChatgptConversations,
    }
}

fn gemini(path: &str) -> ImportMember {
    ImportMember {
        path: path.into(),
        kind: MemberKind::GeminiTakeoutHtml,
    }
}

fn error_text(result: Result<ImportJobReceipt>) -> String {
    result.expect_err("expected the import to fail").to_string()
}

fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if !dir.exists() {
        return out;
    }
    let mut pending = vec![dir.to_path_buf()];
    while let Some(current) = pending.pop() {
        for entry in std::fs::read_dir(current).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// Digest of every file under `dir` whose path (relative, `/` separated) passes `keep`.
fn tree_digest(dir: &Path, keep: impl Fn(&str) -> bool) -> String {
    let mut hasher = Sha256::new();
    for file in files_under(dir) {
        let relative = file
            .strip_prefix(dir)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if keep(&relative) {
            hasher.update(relative.as_bytes());
            hasher.update(Sha256::digest(std::fs::read(&file).unwrap()));
        }
    }
    format!("{:x}", hasher.finalize())
}

/// Per-file listing used to explain a digest mismatch.
fn published_listing(fx: &Fx) -> Vec<String> {
    files_under(fx.archive())
        .iter()
        .map(|file| {
            let relative = file
                .strip_prefix(fx.archive())
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            format!(
                "{relative} {:x}",
                Sha256::digest(std::fs::read(file).unwrap())
            )
        })
        .filter(|line| {
            line.starts_with("conversations/")
                || line.starts_with("imports/raw/")
                || line.starts_with("imports/evidence/")
                || line.starts_with("imports/dispositions/")
        })
        .collect()
}

fn published_digest(fx: &Fx) -> String {
    tree_digest(fx.archive(), |p| {
        p.starts_with("conversations/")
            || p.starts_with("imports/raw/")
            || p.starts_with("imports/evidence/")
            || p.starts_with("imports/dispositions/")
    })
}

fn count_under(fx: &Fx, sub: &str) -> usize {
    files_under(&fx.archive().join(sub)).len()
}

fn conversations(fx: &Fx) -> usize {
    std::fs::read_dir(fx.archive().join("conversations"))
        .unwrap()
        .count()
}

fn evidence_records(fx: &Fx) -> Vec<serde_json::Value> {
    files_under(&fx.archive().join("imports").join("evidence"))
        .iter()
        .map(|path| serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap())
        .collect()
}

fn raw_blob(fx: &Fx, sha: &str) -> PathBuf {
    fx.archive()
        .join("imports")
        .join("raw")
        .join(&sha[..2])
        .join(format!("{sha}.raw"))
}

fn job_dirs(fx: &Fx) -> Vec<PathBuf> {
    let jobs = fx.archive().join("imports").join("jobs");
    if !jobs.exists() {
        return Vec::new();
    }
    std::fs::read_dir(jobs)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_dir())
        .collect()
}

/// Invariant that must hold after any crash: every evidence record has its raw blob and the raw
/// blob hashes to its name, and no conversation is visible that has no evidence at all.
fn assert_evidence_invariants(fx: &Fx) {
    let evidence = evidence_records(fx);
    for record in &evidence {
        let sha = record["payload_sha256"].as_str().unwrap();
        let bytes = std::fs::read(raw_blob(fx, sha)).expect("evidence without a raw blob");
        assert_eq!(format!("{:x}", Sha256::digest(&bytes)), sha);
        assert_eq!(bytes.len() as u64, record["length"].as_u64().unwrap());
    }
    assert!(
        conversations(fx) <= evidence.len(),
        "a conversation is visible without evidence"
    );
}

// ---------- publication, receipt, byte identity ----------

#[test]
fn publishes_raw_evidence_and_projections_and_the_receipt_equals_the_disk() {
    let fx = Fx::new("chatgpt");
    let bytes = standard_export();
    let dir = fx.source("export", &[("conversations.json", &bytes)]);
    let receipt = fx
        .run_default(&dir, &[chatgpt("conversations.json")])
        .unwrap();

    assert_eq!(receipt.records_scanned, 4);
    assert_eq!(receipt.imported, 2);
    assert_eq!(receipt.raw_only, 2);
    assert_eq!(receipt.rejected, 0);
    assert_eq!(receipt.deduplicated, 0);
    assert!(!receipt.already_complete && !receipt.resumed);
    assert_eq!(receipt.raw_blobs_new, 4);
    assert_eq!(receipt.evidence_new, 4);
    assert_eq!(receipt.exceptions.len(), 2);
    assert!(receipt
        .exceptions
        .iter()
        .all(|item| item.outcome == ItemOutcome::RawOnly && item.reason.is_some()));
    // The receipt says what is on disk.
    assert_eq!(
        count_under(&fx, "imports/raw") as u64,
        receipt.raw_blobs_new
    );
    assert_eq!(
        count_under(&fx, "imports/evidence") as u64,
        receipt.evidence_new
    );
    assert_eq!(count_under(&fx, "imports/dispositions"), 4);
    assert_eq!(conversations(&fx) as u64, receipt.imported);
    // Member accounting: record bytes plus unpreserved framing equal the member size.
    let member = &receipt.members[0];
    assert_eq!(member.bytes, bytes.len() as u64);
    assert_eq!(member.records, 4);
    assert_eq!(member.record_bytes + member.unpreserved_bytes, member.bytes);
    assert!(member.unpreserved_bytes > 0);
    // The stored receipt equals the returned one; the spool is gone.
    let job = &job_dirs(&fx)[0];
    let stored: ImportJobReceipt =
        serde_json::from_slice(&std::fs::read(job.join("receipt.json")).unwrap()).unwrap();
    assert_eq!(stored, receipt);
    assert!(!job.join("items").exists());
    assert!(fx.store.incomplete_jobs().unwrap().is_empty());
}

#[test]
fn raw_blobs_are_byte_identical_to_the_independently_read_source_slices() {
    let fx = Fx::new("chatgpt");
    let bytes = standard_export();
    let dir = fx.source("export", &[("conversations.json", &bytes)]);
    fx.run_default(&dir, &[chatgpt("conversations.json")])
        .unwrap();

    let source = std::fs::read(dir.join("conversations.json")).unwrap();
    let evidence = evidence_records(&fx);
    assert_eq!(evidence.len(), 4);
    for record in &evidence {
        let offset = record["offset"].as_u64().unwrap() as usize;
        let length = record["length"].as_u64().unwrap() as usize;
        let slice = &source[offset..offset + length];
        let blob =
            std::fs::read(raw_blob(&fx, record["payload_sha256"].as_str().unwrap())).unwrap();
        assert_eq!(blob, slice, "raw blob differs from the source slice");
        assert_eq!(record["member_path"], "conversations.json");
        assert_eq!(
            record["member_sha256"].as_str().unwrap(),
            format!("{:x}", Sha256::digest(&source))
        );
        assert_eq!(
            record["member_bytes"].as_u64().unwrap(),
            source.len() as u64
        );
    }
    // Alternate branches and the unparseable record are in the raw evidence.
    let all_raw: Vec<u8> = evidence
        .iter()
        .flat_map(|r| std::fs::read(raw_blob(&fx, r["payload_sha256"].as_str().unwrap())).unwrap())
        .collect();
    let text = String::from_utf8(all_raw).unwrap();
    assert!(text.contains("ALT-BRANCH alpha question"));
    assert!(text.contains(r#"{"x":1}"#));
}

#[test]
fn gemini_cards_are_preserved_with_their_exact_bytes_and_non_card_bytes_are_counted() {
    let fx = Fx::new("gemini");
    let html = concat!(
        "<html><body>PREFIX-NOT-A-CARD\n",
        r#"<div class="outer-cell"><div class="content-cell">Prompted one<br>4 Nov 2026, 21:15:40 WIB<br><p>reply one</p></div><div class="content-cell">https://gemini.google.com/app/c_cardone0001</div></div>"#,
        "\n",
        r#"<div class="outer-cell"><div class="content-cell">Prompted two<br><p>reply two</p></div><div class="content-cell">https://gemini.google.com/app/c_cardtwo0002</div></div>"#,
        "\n",
        r#"<div class="outer-cell"><div class="content-cell"></div></div>"#,
        "\n</body></html>"
    );
    let dir = fx.source(
        "takeout",
        &[("My Activity/MyActivity.html", html.as_bytes())],
    );
    let receipt = fx
        .run_default(&dir, &[gemini("My Activity/MyActivity.html")])
        .unwrap();
    assert_eq!(receipt.records_scanned, 3);
    assert_eq!(receipt.imported, 2);
    assert_eq!(receipt.raw_only, 1);
    assert!(receipt.members[0].unpreserved_bytes > 0);
    for record in evidence_records(&fx) {
        let offset = record["offset"].as_u64().unwrap() as usize;
        let length = record["length"].as_u64().unwrap() as usize;
        let blob =
            std::fs::read(raw_blob(&fx, record["payload_sha256"].as_str().unwrap())).unwrap();
        assert_eq!(blob, html.as_bytes()[offset..offset + length]);
        assert_eq!(record["envelope_domain"], "gemini-takeout-html");
    }
}

#[test]
fn a_live_capture_is_never_displaced_by_an_imported_snapshot() {
    use mind_place_archive::{Capture, Message};
    let fx = Fx::new("chatgpt");
    let live = Capture {
        schema_version: 1,
        provider: "chatgpt".into(),
        conversation_id: A.into(),
        source_url: format!("https://chatgpt.com/c/{A}"),
        title: "Live".into(),
        conversation_created_at: None,
        conversation_updated_at: None,
        captured_at: "2026-11-01T00:00:00Z".into(),
        capture_method: "manual-dom".into(),
        completeness: "partial".into(),
        warnings: vec!["only loaded messages".into()],
        messages: vec![Message {
            id: "live-1".into(),
            role: "user".into(),
            text: "live text".into(),
            format: "plain".into(),
            authored_at: None,
            model: None,
            attachments: vec![],
        }],
    };
    let saved = fx.store.save(live).unwrap();
    let bytes = three_conversations();
    let dir = fx.source("export", &[("conversations.json", &bytes)]);
    let receipt = fx
        .run_default(&dir, &[chatgpt("conversations.json")])
        .unwrap();
    assert_eq!(receipt.imported, 3);
    let read = fx.store.read(&saved.id, None).unwrap();
    assert_eq!(read.capture.capture_method, "manual-dom");
    assert_eq!(fx.store.snapshots(&saved.id).unwrap().len(), 2);
}

// ---------- idempotence and deduplication ----------

#[test]
fn reimport_of_the_same_source_is_idempotent_and_writes_nothing() {
    let fx = Fx::new("chatgpt");
    let bytes = standard_export();
    let dir = fx.source("export", &[("conversations.json", &bytes)]);
    let first = fx
        .run_default(&dir, &[chatgpt("conversations.json")])
        .unwrap();
    let before = tree_digest(fx.archive(), |p| !p.ends_with(".lock"));
    let second = fx
        .run_default(&dir, &[chatgpt("conversations.json")])
        .unwrap();
    assert!(second.already_complete);
    assert_eq!(second.imported, first.imported);
    assert_eq!(second.job_key, first.job_key);
    assert_eq!(before, tree_digest(fx.archive(), |p| !p.ends_with(".lock")));
}

#[test]
fn a_new_export_with_the_same_content_reuses_raw_blobs_and_adds_provenance() {
    let fx = Fx::new("chatgpt");
    let first_bytes = export(&[
        conv(A, "First", "alpha question"),
        conv(B, "Second", "beta question"),
    ]);
    let dir1 = fx.source("export1", &[("conversations.json", &first_bytes)]);
    let first = fx
        .run_default(&dir1, &[chatgpt("conversations.json")])
        .unwrap();
    assert_eq!(first.imported, 2);

    // Same two records at different offsets, plus one new conversation.
    let second_bytes = export(&[
        conv(C, "Third", "gamma question"),
        conv(A, "First", "alpha question"),
        conv(B, "Second", "beta question"),
    ]);
    let dir2 = fx.source("export2", &[("conversations.json", &second_bytes)]);
    let second = fx
        .run_default(&dir2, &[chatgpt("conversations.json")])
        .unwrap();
    assert_ne!(second.job_key, first.job_key);
    assert_eq!(second.imported, 1);
    assert_eq!(second.deduplicated, 2);
    assert_eq!(second.raw_blobs_new, 1);
    assert_eq!(second.raw_blobs_reused, 2);
    assert_eq!(second.evidence_new, 3);
    // Three distinct raw blobs, five provenance records, three conversations.
    assert_eq!(count_under(&fx, "imports/raw"), 3);
    assert_eq!(count_under(&fx, "imports/evidence"), 5);
    assert_eq!(conversations(&fx), 3);
}

#[test]
fn identical_records_in_one_job_store_the_bytes_once_with_two_provenance_records() {
    let fx = Fx::new("chatgpt");
    let record = conv(A, "First", "alpha question");
    let bytes = export(&[record.clone(), record]);
    let dir = fx.source("export", &[("conversations.json", &bytes)]);
    let receipt = fx
        .run_default(&dir, &[chatgpt("conversations.json")])
        .unwrap();
    assert_eq!(receipt.records_scanned, 2);
    assert_eq!(receipt.raw_blobs_new, 1);
    assert_eq!(receipt.raw_blobs_reused, 1);
    assert_eq!(count_under(&fx, "imports/raw"), 1);
    assert_eq!(count_under(&fx, "imports/evidence"), 2);
    // The duplicate identity is disambiguated, not silently merged.
    assert_eq!(conversations(&fx), 2);
}

#[test]
fn ids_that_differ_only_by_case_are_disambiguated_across_members() {
    let fx = Fx::new("chatgpt");
    let one = export(&[conv(A, "First", "alpha question")]);
    let two = export(&[conv(&A.to_uppercase(), "Again", "alpha question again")]);
    let dir = fx.source(
        "export",
        &[
            ("conversations-000.json", &one),
            ("conversations-001.json", &two),
        ],
    );
    let receipt = fx
        .run_default(
            &dir,
            &[
                chatgpt("conversations-000.json"),
                chatgpt("conversations-001.json"),
            ],
        )
        .unwrap();
    assert_eq!(receipt.imported, 2);
    assert_eq!(conversations(&fx), 2);
}

// ---------- source substitution, truncation, TOCTOU ----------

fn assert_nothing_written(fx: &Fx) {
    assert_eq!(conversations(fx), 0);
    assert_eq!(count_under(fx, "imports/raw"), 0);
    assert_eq!(count_under(fx, "imports/evidence"), 0);
    assert_eq!(count_under(fx, "imports/dispositions"), 0);
    assert!(job_dirs(fx).is_empty(), "a job directory was left behind");
}

#[test]
fn a_source_changed_after_authorization_is_refused_before_anything_is_written() {
    type Mutation = fn(&Path);
    let cases: [(&str, Mutation); 5] = [
        ("same-size substitution", |dir| {
            let path = dir.join("conversations.json");
            let mut bytes = std::fs::read(&path).unwrap();
            let at = bytes.iter().position(|b| *b == b'a').unwrap();
            bytes[at] = b'b';
            std::fs::write(path, bytes).unwrap();
        }),
        ("truncated", |dir| {
            let path = dir.join("conversations.json");
            let bytes = std::fs::read(&path).unwrap();
            std::fs::write(path, &bytes[..bytes.len() / 2]).unwrap();
        }),
        ("extended", |dir| {
            let path = dir.join("conversations.json");
            let mut bytes = std::fs::read(&path).unwrap();
            bytes.extend_from_slice(b"\n");
            std::fs::write(path, bytes).unwrap();
        }),
        ("file added", |dir| {
            std::fs::write(dir.join("extra.json"), b"[]").unwrap();
        }),
        ("file removed", |dir| {
            std::fs::remove_file(dir.join("other.json")).unwrap();
        }),
    ];
    for (name, mutate) in cases {
        let fx = Fx::new("chatgpt");
        let bytes = standard_export();
        let dir = fx.source(
            "export",
            &[("conversations.json", &bytes), ("other.json", b"[]")],
        );
        let cap = fx.authorize(&dir);
        mutate(&dir);
        let message = error_text(fx.run(
            &cap,
            &[chatgpt("conversations.json")],
            &mind_place_archive::historical::NoProgress,
            ImportJobLimits::default(),
        ));
        assert!(message.contains("E_SOURCE_CHANGED"), "{name}: {message}");
        assert_nothing_written(&fx);
    }
}

fn big_export() -> Vec<u8> {
    // Several 64 KiB read chunks, with the first record fully inside the first chunk.
    let records: Vec<String> = (0..40)
        .map(|i| {
            let id = format!("{:08x}-0000-4000-8000-{:012x}", i + 1, i + 1);
            conv(
                &id,
                &format!("Conversation {i}"),
                &format!("question {i} {}", "x".repeat(8000)),
            )
        })
        .collect();
    export(&records)
}

#[test]
fn a_member_changed_while_it_is_parsed_is_detected_by_the_hash_and_nothing_is_visible() {
    let fx = Fx::new("chatgpt");
    let bytes = big_export();
    assert!(bytes.len() > 4 * 64 * 1024);
    let dir = fx.source("export", &[("conversations.json", &bytes)]);
    let cap = fx.authorize(&dir);
    let path = dir.join("conversations.json");
    let hook = Hook(|point| {
        if point == (ImportPoint::Staged { seq: 0 }) {
            // Same length, different bytes near the end, after the first chunk was read.
            let mut changed = std::fs::read(&path).unwrap();
            let at = changed.len() - 200;
            changed[at] = if changed[at] == b'y' { b'z' } else { b'y' };
            std::fs::write(&path, changed).unwrap();
        }
        Ok(())
    });
    let message = error_text(fx.run(
        &cap,
        &[chatgpt("conversations.json")],
        &hook,
        ImportJobLimits::default(),
    ));
    assert!(
        message.contains("E_SOURCE_MISMATCH") || message.contains("E_SOURCE_PARSE"),
        "{message}"
    );
    assert_nothing_written(&fx);
}

#[test]
fn a_file_added_while_staging_is_detected_before_publication() {
    let fx = Fx::new("chatgpt");
    let bytes = three_conversations();
    let dir = fx.source("export", &[("conversations.json", &bytes)]);
    let cap = fx.authorize(&dir);
    let extra = dir.join("late.json");
    let hook = Hook(|point| {
        if point == (ImportPoint::Staged { seq: 0 }) {
            std::fs::write(&extra, b"[]").unwrap();
        }
        Ok(())
    });
    let message = error_text(fx.run(
        &cap,
        &[chatgpt("conversations.json")],
        &hook,
        ImportJobLimits::default(),
    ));
    assert!(message.contains("E_SOURCE_CHANGED"), "{message}");
    assert_nothing_written(&fx);
}

#[test]
fn changing_the_source_after_verification_cannot_alter_what_is_stored() {
    let fx = Fx::new("chatgpt");
    let bytes = three_conversations();
    let dir = fx.source("export", &[("conversations.json", &bytes)]);
    let cap = fx.authorize(&dir);
    let path = dir.join("conversations.json");
    let hook = Hook(|point| {
        if point == ImportPoint::Verified {
            std::fs::write(&path, b"[]").unwrap();
        }
        Ok(())
    });
    let receipt = fx
        .run(
            &cap,
            &[chatgpt("conversations.json")],
            &hook,
            ImportJobLimits::default(),
        )
        .unwrap();
    assert_eq!(receipt.imported, 3);
    // Evidence still carries the verified original bytes, not the substituted file.
    let first = &evidence_records(&fx)[0];
    let blob = std::fs::read(raw_blob(&fx, first["payload_sha256"].as_str().unwrap())).unwrap();
    assert!(std::str::from_utf8(&blob).unwrap().contains("question"));
    assert_eq!(
        first["member_sha256"].as_str().unwrap(),
        format!("{:x}", Sha256::digest(&bytes))
    );
}

#[test]
fn a_source_that_is_itself_truncated_fails_to_parse_and_leaves_nothing() {
    let fx = Fx::new("chatgpt");
    let bytes = three_conversations();
    let dir = fx.source(
        "export",
        &[("conversations.json", &bytes[..bytes.len() - 40])],
    );
    let message = error_text(fx.run_default(&dir, &[chatgpt("conversations.json")]));
    assert!(message.contains("E_SOURCE_PARSE"), "{message}");
    assert!(!message.contains("alpha"), "error must not echo content");
    assert_nothing_written(&fx);
}

#[test]
fn a_symlink_in_the_source_is_refused() {
    let fx = Fx::new("chatgpt");
    let bytes = three_conversations();
    let dir = fx.source("export", &[("conversations.json", &bytes)]);
    let target = fx.source("elsewhere", &[("secret.json", b"[]")]);
    #[cfg(unix)]
    let linked = std::os::unix::fs::symlink(target.join("secret.json"), dir.join("link.json"));
    #[cfg(windows)]
    let linked =
        std::os::windows::fs::symlink_file(target.join("secret.json"), dir.join("link.json"));
    if linked.is_err() {
        // BLOCKED, not PASS: this runner cannot create symbolic links (Windows without
        // Developer Mode). The Linux hosted job runs this test for real.
        eprintln!("BLOCKED: symbolic links cannot be created on this runner");
        return;
    }
    let message = SourceManifest::measure(&dir, ManifestLimits::default())
        .unwrap_err()
        .to_string();
    assert!(message.contains("E_SOURCE_LINK"), "{message}");
}

// ---------- crash, recovery and authorization ----------

#[test]
fn a_crash_at_any_publication_step_is_repaired_by_running_the_import_again() {
    // Reference run on identical bytes: what a clean import produces.
    let reference = Fx::new("chatgpt");
    let bytes = three_conversations();
    let ref_dir = reference.source("export", &[("conversations.json", &bytes)]);
    reference
        .run_default(&ref_dir, &[chatgpt("conversations.json")])
        .unwrap();
    let expected = published_digest(&reference);

    let points = [
        ImportPoint::Verified,
        ImportPoint::BeforeItem { seq: 1 },
        ImportPoint::AfterRaw { seq: 1 },
        ImportPoint::AfterEvidence { seq: 1 },
        ImportPoint::AfterProjection { seq: 1 },
        ImportPoint::AfterItem { seq: 1 },
        ImportPoint::BeforeComplete,
    ];
    for point in points {
        let fx = Fx::new("chatgpt");
        let dir = fx.source("export", &[("conversations.json", &bytes)]);
        let members = [chatgpt("conversations.json")];
        let cap = fx.authorize(&dir);
        let crashed = fx.run(&cap, &members, &crash_at(point), ImportJobLimits::default());
        assert!(
            error_text(crashed).contains("SIMULATED_CRASH"),
            "{point:?} did not crash"
        );
        assert_evidence_invariants(&fx);
        let incomplete = fx.store.incomplete_jobs().unwrap();
        assert_eq!(
            incomplete.len(),
            1,
            "{point:?}: job not listed as incomplete"
        );
        assert!(incomplete[0].staged);

        // A new authorization over the unchanged source resumes the same job.
        let cap = fx.authorize(&dir);
        let receipt = fx
            .run(
                &cap,
                &members,
                &mind_place_archive::historical::NoProgress,
                ImportJobLimits::default(),
            )
            .unwrap();
        assert!(receipt.resumed, "{point:?}");
        assert_eq!(receipt.records_scanned, 3);
        assert_eq!(
            receipt.imported + receipt.deduplicated,
            3,
            "{point:?}: every conversation must be accounted for"
        );
        if published_digest(&fx) != expected {
            let clean = published_listing(&reference);
            let now = published_listing(&fx);
            let only_clean: Vec<_> = clean.iter().filter(|l| !now.contains(l)).collect();
            let only_now: Vec<_> = now.iter().filter(|l| !clean.contains(l)).collect();
            panic!("{point:?}: disk differs from a clean run: clean-only {only_clean:?} resumed-only {only_now:?}");
        }
        assert!(fx.store.incomplete_jobs().unwrap().is_empty());
        assert_evidence_invariants(&fx);
    }
}

#[test]
fn a_crash_while_staging_leaves_nothing_visible_and_a_rerun_succeeds() {
    let fx = Fx::new("chatgpt");
    let bytes = three_conversations();
    let dir = fx.source("export", &[("conversations.json", &bytes)]);
    let cap = fx.authorize(&dir);
    let members = [chatgpt("conversations.json")];
    let error = error_text(fx.run(
        &cap,
        &members,
        &crash_at(ImportPoint::Staged { seq: 1 }),
        ImportJobLimits::default(),
    ));
    assert!(error.contains("SIMULATED_CRASH"));
    assert_nothing_written(&fx);
    let receipt = fx.run_default(&dir, &members).unwrap();
    assert_eq!(receipt.imported, 3);
}

#[test]
fn locking_the_gate_mid_job_stops_publication_and_the_job_resumes_after_unlock() {
    let fx = Fx::new("chatgpt");
    let bytes = three_conversations();
    let dir = fx.source("export", &[("conversations.json", &bytes)]);
    let cap = fx.authorize(&dir);
    let members = [chatgpt("conversations.json")];
    let hook = Hook(|point| {
        if point == (ImportPoint::BeforeItem { seq: 1 }) {
            fx.gate.lock();
        }
        Ok(())
    });
    // `run` needs a lease from before the lock; take it explicitly.
    let lease = fx.gate.require_unlocked().unwrap();
    let mut request = SourceImportRequest::new(&fx.gate, lease, &cap, &members);
    request.progress = &hook;
    let message = fx
        .store
        .import_authorized_source(&request)
        .unwrap_err()
        .to_string();
    assert!(message.contains("E_AUTHORIZATION"), "{message}");
    assert_eq!(conversations(&fx), 1, "only the first item may be visible");
    assert_evidence_invariants(&fx);
    let incomplete = fx.store.incomplete_jobs().unwrap();
    assert_eq!((incomplete[0].items, incomplete[0].items_published), (3, 1));

    fx.gate.unlock(CRED).unwrap();
    let receipt = fx.run_default(&dir, &members).unwrap();
    assert!(receipt.resumed);
    assert_eq!(receipt.imported + receipt.deduplicated, 3);
    assert_eq!(conversations(&fx), 3);
}

#[test]
fn revoking_the_authorization_mid_job_stops_publication() {
    let fx = Fx::new("chatgpt");
    let bytes = three_conversations();
    let dir = fx.source("export", &[("conversations.json", &bytes)]);
    let cap = fx.authorize(&dir);
    let hook = Hook(|point| {
        if point == (ImportPoint::BeforeItem { seq: 2 }) {
            fx.gate.revoke_import_job(cap.job_id()).unwrap();
        }
        Ok(())
    });
    let message = error_text(fx.run(
        &cap,
        &[chatgpt("conversations.json")],
        &hook,
        ImportJobLimits::default(),
    ));
    assert!(message.contains("E_AUTHORIZATION"), "{message}");
    assert_eq!(conversations(&fx), 2);
    // The revoked capability stays unusable; a fresh authorization can finish the job.
    let again = fx.authorize(&dir);
    let receipt = fx
        .run(
            &again,
            &[chatgpt("conversations.json")],
            &mind_place_archive::historical::NoProgress,
            ImportJobLimits::default(),
        )
        .unwrap();
    assert_eq!(receipt.imported + receipt.deduplicated, 3);
}

#[test]
fn a_foreign_gate_a_wrong_provider_and_a_vault_marker_write_nothing() {
    let fx = Fx::new("chatgpt");
    let bytes = three_conversations();
    let dir = fx.source("export", &[("conversations.json", &bytes)]);
    let cap = fx.authorize(&dir);

    // Members of another provider than the authorization.
    let message = error_text(fx.run(
        &cap,
        &[gemini("conversations.json")],
        &mind_place_archive::historical::NoProgress,
        ImportJobLimits::default(),
    ));
    assert!(message.contains("E_MEMBERS"), "{message}");

    // A gate of another profile.
    let other = tempfile::tempdir().unwrap();
    let foreign = SecurityGate::open(other.path().join("security-v1.json")).unwrap();
    foreign.initialize(CRED).unwrap();
    let foreign_cap = foreign
        .authorize_import_session(
            foreign.require_unlocked().unwrap(),
            CRED,
            "chatgpt",
            &dir,
            *SourceManifest::measure(&dir, ManifestLimits::default())
                .unwrap()
                .sha256(),
            Duration::from_secs(60),
        )
        .unwrap();
    let members = [chatgpt("conversations.json")];
    let mut request = SourceImportRequest::new(
        &foreign,
        foreign.require_unlocked().unwrap(),
        &foreign_cap,
        &members,
    );
    request.limits = ImportJobLimits::default();
    assert!(fx.store.import_authorized_source(&request).is_err());
    // A capability from the right gate used with the wrong gate object.
    let mut request = SourceImportRequest::new(
        &fx.gate,
        fx.gate.require_unlocked().unwrap(),
        &foreign_cap,
        &members,
    );
    request.limits = ImportJobLimits::default();
    assert!(fx.store.import_authorized_source(&request).is_err());
    assert!(!fx.archive().join("imports").exists());

    // Encrypted vault marker fence.
    std::fs::write(fx.home.path().join("vault-v1.json"), b"synthetic-marker").unwrap();
    let message = error_text(fx.run(
        &cap,
        &members,
        &mind_place_archive::historical::NoProgress,
        ImportJobLimits::default(),
    ));
    assert!(message.contains("Encrypted vault"), "{message}");
    assert!(!fx.archive().join("imports").exists());
    assert_eq!(conversations(&fx), 0);
}

#[test]
fn the_gate_manifest_hash_must_be_the_measured_one() {
    let fx = Fx::new("chatgpt");
    let bytes = three_conversations();
    let dir = fx.source("export", &[("conversations.json", &bytes)]);
    // An owner flow that pins a hash it did not measure (here a made-up value) cannot import.
    let forged = fx
        .gate
        .authorize_import_session(
            fx.gate.require_unlocked().unwrap(),
            CRED,
            "chatgpt",
            &dir,
            [7u8; 32],
            Duration::from_secs(60),
        )
        .unwrap();
    let message = error_text(fx.run(
        &forged,
        &[chatgpt("conversations.json")],
        &mind_place_archive::historical::NoProgress,
        ImportJobLimits::default(),
    ));
    assert!(message.contains("E_SOURCE_CHANGED"), "{message}");
    assert_nothing_written(&fx);
}

// ---------- integrity of what is stored ----------

#[test]
fn existing_raw_evidence_is_never_replaced() {
    let fx = Fx::new("chatgpt");
    let bytes = three_conversations();
    let dir = fx.source("export", &[("conversations.json", &bytes)]);
    // Find the first record's hash the same way the importer does.
    let mut first_sha = String::new();
    ChatGptExportParser::stream_conversations_json_spanned(
        &bytes[..],
        BoundedArrayLimits {
            max_record_bytes: 1 << 20,
            max_separator_bytes: 1024,
        },
        |span, _| {
            if first_sha.is_empty() {
                first_sha = span.sha256.clone();
            }
            Ok(())
        },
    )
    .unwrap();
    let planted = raw_blob(&fx, &first_sha);
    std::fs::create_dir_all(planted.parent().unwrap()).unwrap();
    std::fs::write(&planted, b"tampered raw evidence").unwrap();

    let message = error_text(fx.run_default(&dir, &[chatgpt("conversations.json")]));
    assert!(message.contains("E_RAW_CORRUPT"), "{message}");
    assert_eq!(std::fs::read(&planted).unwrap(), b"tampered raw evidence");
    assert_eq!(conversations(&fx), 0, "no projection without verified raw");
}

#[test]
fn a_tampered_spool_is_detected_before_anything_is_published() {
    let fx = Fx::new("chatgpt");
    let bytes = three_conversations();
    let dir = fx.source("export", &[("conversations.json", &bytes)]);
    let members = [chatgpt("conversations.json")];
    let cap = fx.authorize(&dir);
    fx.run(
        &cap,
        &members,
        &crash_at(ImportPoint::Verified),
        ImportJobLimits::default(),
    )
    .unwrap_err();
    let item = job_dirs(&fx)[0].join("items").join("00000001.json");
    let mut text = std::fs::read(&item).unwrap();
    let at = text.iter().position(|b| *b == b'q').unwrap();
    text[at] = b'Q';
    std::fs::write(&item, text).unwrap();

    let message = error_text(fx.run_default(&dir, &members));
    assert!(message.contains("E_STAGING_CORRUPT"), "{message}");
    assert_eq!(conversations(&fx), 0);
    assert_eq!(count_under(&fx, "imports/raw"), 0);
    // Discarding the spool lets the owner run the import again from the source.
    let key = fx.store.incomplete_jobs().unwrap()[0].job_key.clone();
    fx.store
        .discard_staged_job(&fx.gate, fx.gate.require_unlocked().unwrap(), &key)
        .unwrap();
    assert_eq!(fx.run_default(&dir, &members).unwrap().imported, 3);
}

// ---------- bounds ----------

#[test]
fn oversized_and_abusive_sources_are_bounded_and_leave_nothing() {
    let fx = Fx::new("chatgpt");
    let bytes = three_conversations();
    let dir = fx.source("export", &[("conversations.json", &bytes)]);
    let members = [chatgpt("conversations.json")];
    let run = |limits: ImportJobLimits| {
        let cap = fx.authorize(&dir);
        error_text(fx.run(
            &cap,
            &members,
            &mind_place_archive::historical::NoProgress,
            limits,
        ))
    };

    let mut limits = ImportJobLimits::default();
    limits.max_items = 2;
    assert!(run(limits).contains("E_SOURCE_LIMIT"));
    assert_nothing_written(&fx);

    let mut limits = ImportJobLimits::default();
    limits.chatgpt_stream = BoundedArrayLimits {
        max_record_bytes: 200,
        max_separator_bytes: 1024,
    };
    assert!(run(limits).contains("E_SOURCE_LIMIT"));
    assert_nothing_written(&fx);

    let mut limits = ImportJobLimits::default();
    limits.max_staged_raw_bytes = 100;
    assert!(run(limits).contains("E_SOURCE_LIMIT"));
    assert_nothing_written(&fx);

    let mut limits = ImportJobLimits::default();
    limits.max_members = 0;
    assert!(run(limits).contains("E_MEMBERS"));
    assert_nothing_written(&fx);
}

#[test]
fn the_manifest_measurement_is_bounded() {
    let fx = Fx::new("chatgpt");
    let dir = fx.source(
        "many",
        &[
            ("a/b/c/d.json", b"[]"),
            ("e.json", b"[]"),
            ("f.json", b"[]"),
        ],
    );
    let tight = |edit: fn(&mut ManifestLimits)| {
        let mut limits = ManifestLimits::default();
        edit(&mut limits);
        SourceManifest::measure(&dir, limits)
            .unwrap_err()
            .to_string()
    };
    assert!(tight(|l| l.max_entries = 3).contains("E_SOURCE_LIMIT"));
    assert!(tight(|l| l.max_depth = 2).contains("E_SOURCE_LIMIT"));
    assert!(tight(|l| l.max_path_bytes = 4).contains("E_SOURCE_LIMIT"));
    assert!(tight(|l| l.max_total_bytes = 3).contains("E_SOURCE_LIMIT"));
}

#[test]
fn the_manifest_hash_is_deterministic_and_covers_names_sizes_and_content() {
    let fx = Fx::new("chatgpt");
    let one = fx.source("one", &[("a.json", b"[1]"), ("sub/b.json", b"[2]")]);
    let same = fx.source("same", &[("sub/b.json", b"[2]"), ("a.json", b"[1]")]);
    let content = fx.source("content", &[("a.json", b"[3]"), ("sub/b.json", b"[2]")]);
    let renamed = fx.source("renamed", &[("c.json", b"[1]"), ("sub/b.json", b"[2]")]);
    let measure = |dir: &Path| {
        *SourceManifest::measure(dir, ManifestLimits::default())
            .unwrap()
            .sha256()
    };
    assert_eq!(measure(&one), measure(&same));
    assert_ne!(measure(&one), measure(&content));
    assert_ne!(measure(&one), measure(&renamed));
    let manifest = SourceManifest::measure(&one, ManifestLimits::default()).unwrap();
    assert_eq!(manifest.total_files(), 2);
    assert_eq!(manifest.total_bytes(), 6);
    assert!(manifest.entry("sub/b.json").is_some());
    manifest
        .verify_unchanged(ManifestLimits::default())
        .unwrap();
    std::fs::write(one.join("a.json"), b"[9]").unwrap();
    assert!(manifest
        .verify_unchanged(ManifestLimits::default())
        .unwrap_err()
        .to_string()
        .contains("E_SOURCE_CHANGED"));
}

// ---------- reviewer findings (round 1 on PR #168) ----------

fn finished_job(fx: &Fx) -> (PathBuf, ImportJobReceipt) {
    let bytes = three_conversations();
    let dir = fx.source("export", &[("conversations.json", &bytes)]);
    let receipt = fx
        .run_default(&dir, &[chatgpt("conversations.json")])
        .unwrap();
    (dir, receipt)
}

fn discard(fx: &Fx, key: &str) -> Result<()> {
    fx.store
        .discard_staged_job(&fx.gate, fx.gate.require_unlocked().unwrap(), key)
}

#[test]
fn a_finished_job_is_an_audit_record_and_cannot_be_discarded() {
    let fx = Fx::new("chatgpt");
    let (_, receipt) = finished_job(&fx);
    let before = tree_digest(fx.archive(), |p| !p.ends_with(".lock"));
    let message = discard(&fx, &receipt.job_key).unwrap_err().to_string();
    assert!(message.contains("E_JOB_COMPLETE"), "{message}");
    assert_eq!(before, tree_digest(fx.archive(), |p| !p.ends_with(".lock")));
    let job = &job_dirs(&fx)[0];
    assert!(job.join("receipt.json").exists() && job.join("state.json").exists());

    // A receipt on disk protects the job even when its state file is unreadable.
    std::fs::write(job.join("state.json"), b"not json").unwrap();
    assert!(discard(&fx, &receipt.job_key)
        .unwrap_err()
        .to_string()
        .contains("E_JOB_COMPLETE"));
    assert!(job.join("receipt.json").exists());
}

#[test]
fn discarding_needs_the_owner_gate_and_only_removes_unfinished_spools() {
    let fx = Fx::new("chatgpt");
    let bytes = three_conversations();
    let dir = fx.source("export", &[("conversations.json", &bytes)]);
    let members = [chatgpt("conversations.json")];
    let cap = fx.authorize(&dir);
    fx.run(
        &cap,
        &members,
        &crash_at(ImportPoint::BeforeItem { seq: 1 }),
        ImportJobLimits::default(),
    )
    .unwrap_err();
    let key = fx.store.incomplete_jobs().unwrap()[0].job_key.clone();
    let published = published_digest(&fx);

    // A foreign gate and a locked gate cannot discard.
    let other = tempfile::tempdir().unwrap();
    let foreign = SecurityGate::open(other.path().join("security-v1.json")).unwrap();
    foreign.initialize(CRED).unwrap();
    assert!(fx
        .store
        .discard_staged_job(&foreign, foreign.require_unlocked().unwrap(), &key)
        .is_err());
    let lease = fx.gate.require_unlocked().unwrap();
    fx.gate.lock();
    let message = fx
        .store
        .discard_staged_job(&fx.gate, lease, &key)
        .unwrap_err()
        .to_string();
    assert!(message.contains("E_AUTHORIZATION"), "{message}");
    assert_eq!(fx.store.incomplete_jobs().unwrap().len(), 1);
    fx.gate.unlock(CRED).unwrap();

    // Unknown and invalid keys are harmless or refused.
    assert!(discard(&fx, &"0".repeat(64)).is_ok());
    assert!(discard(&fx, "../escape").is_err());

    // Unfinished: removed, and what was published stays.
    discard(&fx, &key).unwrap();
    assert!(fx.store.incomplete_jobs().unwrap().is_empty());
    assert_eq!(published_digest(&fx), published);
    assert!(conversations(&fx) >= 1);

    // An unreadable state with no receipt is not provably finished: it may be discarded.
    let cap = fx.authorize(&dir);
    fx.run(
        &cap,
        &members,
        &crash_at(ImportPoint::Verified),
        ImportJobLimits::default(),
    )
    .unwrap_err();
    let job = job_dirs(&fx)[0].clone();
    std::fs::write(job.join("state.json"), b"garbage").unwrap();
    let key = job.file_name().unwrap().to_string_lossy().into_owned();
    discard(&fx, &key).unwrap();
    assert!(job_dirs(&fx).is_empty());
}

type Damage = fn(&Fx, &Path);

#[test]
fn a_finished_job_is_checked_against_the_disk_before_success_is_reported() {
    let damages: [(&str, Damage); 5] = [
        ("raw blob", |fx, _| {
            let raw = files_under(&fx.archive().join("imports/raw"));
            std::fs::remove_file(&raw[0]).unwrap();
        }),
        ("evidence", |fx, _| {
            let evidence = files_under(&fx.archive().join("imports/evidence"));
            std::fs::remove_file(&evidence[0]).unwrap();
        }),
        ("disposition link", |fx, _| {
            let links = files_under(&fx.archive().join("imports/dispositions"));
            std::fs::remove_file(&links[0]).unwrap();
        }),
        ("projection", |fx, _| {
            let meta = files_under(&fx.archive().join("conversations"))
                .into_iter()
                .find(|p| p.ends_with("meta.json"))
                .unwrap();
            std::fs::remove_file(meta).unwrap();
        }),
        ("ledger", |_, job| {
            std::fs::remove_file(job.join("ledger.jsonl")).unwrap();
        }),
    ];
    for (name, damage) in damages {
        let fx = Fx::new("chatgpt");
        let (dir, receipt) = finished_job(&fx);
        let members = [chatgpt("conversations.json")];
        let expected = published_digest(&fx);

        // A healthy replay says what it checked.
        let again = fx.run_default(&dir, &members).unwrap();
        assert!(again.already_complete, "{name}");
        assert_eq!(again.published_check, "presence-verified");
        assert_eq!(receipt.published_check, "published-now");
        fx.store
            .verify_published_job(&receipt.job_key, true)
            .unwrap();

        let job = job_dirs(&fx)[0].clone();
        damage(&fx, &job);
        // The stored receipt is a claim: without repair the replay must fail, not succeed.
        let message = error_text(fx.run_default(&dir, &members));
        assert!(
            message.contains("E_PUBLISHED_MISMATCH"),
            "{name}: {message}"
        );
        assert!(fx
            .store
            .verify_published_job(&receipt.job_key, false)
            .unwrap_err()
            .to_string()
            .contains("E_PUBLISHED_MISMATCH"));
        assert!(
            job.join("receipt.json").exists(),
            "{name}: receipt must survive a failed check"
        );

        // Explicit repair re-publishes what is missing and keeps the old audit files.
        let cap = fx.authorize(&dir);
        let mut request = SourceImportRequest::new(
            &fx.gate,
            fx.gate.require_unlocked().unwrap(),
            &cap,
            &members,
        );
        request.repair = true;
        let repaired = fx.store.import_authorized_source(&request).unwrap();
        assert!(!repaired.already_complete, "{name}");
        assert_eq!(repaired.published_check, "published-now");
        assert_eq!(
            published_digest(&fx),
            expected,
            "{name}: repair differs from the original"
        );
        fx.store
            .verify_published_job(&receipt.job_key, true)
            .unwrap();
        let superseded = std::fs::read_dir(&job)
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with("receipt.superseded-")
            })
            .count();
        assert!(superseded >= 1, "{name}: the old receipt must be kept");
    }
}

#[test]
fn a_same_size_corruption_of_a_raw_blob_needs_the_deep_check() {
    let fx = Fx::new("chatgpt");
    let (_, receipt) = finished_job(&fx);
    let raw = files_under(&fx.archive().join("imports/raw"))[0].clone();
    let mut bytes = std::fs::read(&raw).unwrap();
    bytes[3] ^= 0x01;
    std::fs::write(&raw, bytes).unwrap();
    // The cheap check is documented as presence and size only.
    fx.store
        .verify_published_job(&receipt.job_key, false)
        .unwrap();
    assert!(fx
        .store
        .verify_published_job(&receipt.job_key, true)
        .unwrap_err()
        .to_string()
        .contains("E_PUBLISHED_MISMATCH"));
}

#[test]
fn resuming_with_a_different_raw_byte_cap_never_reuses_the_old_spool() {
    let fx = Fx::new("chatgpt");
    let bytes = three_conversations();
    let dir = fx.source("export", &[("conversations.json", &bytes)]);
    let members = [chatgpt("conversations.json")];
    let cap = fx.authorize(&dir);
    fx.run(
        &cap,
        &members,
        &crash_at(ImportPoint::Verified),
        ImportJobLimits::default(),
    )
    .unwrap_err();
    let original = fx.store.incomplete_jobs().unwrap()[0].job_key.clone();

    let tighter = ImportJobLimits {
        max_staged_raw_bytes: 100,
        ..ImportJobLimits::default()
    };
    let cap = fx.authorize(&dir);
    let message = error_text(fx.run(
        &cap,
        &members,
        &mind_place_archive::historical::NoProgress,
        tighter,
    ));
    assert!(message.contains("E_SOURCE_LIMIT"), "{message}");
    // The generous-cap spool is a different job: still there, untouched, nothing published.
    let incomplete = fx.store.incomplete_jobs().unwrap();
    assert_eq!(incomplete.len(), 1);
    assert_eq!(incomplete[0].job_key, original);
    assert_eq!(conversations(&fx), 0);
    // And the original job still resumes under its own limits.
    let receipt = fx.run_default(&dir, &members).unwrap();
    assert_eq!(receipt.job_key, original);
    assert!(receipt.resumed);
}

// ---------- reviewer findings (round 2 on PR #168) ----------

fn job_dir_of(fx: &Fx) -> PathBuf {
    job_dirs(fx)[0].clone()
}

fn repair(fx: &Fx, dir: &Path) -> Result<ImportJobReceipt> {
    let members = [chatgpt("conversations.json")];
    let cap = fx.authorize(dir);
    let mut request = SourceImportRequest::new(
        &fx.gate,
        fx.gate.require_unlocked().unwrap(),
        &cap,
        &members,
    );
    request.repair = true;
    fx.store.import_authorized_source(&request)
}

#[test]
fn a_missing_malformed_or_foreign_receipt_is_a_typed_mismatch_and_can_be_repaired() {
    let foreign_receipt = {
        let other = Fx::new("chatgpt");
        let bytes = export(&[conv(A, "Other", "other question")]);
        let dir = other.source("export", &[("conversations.json", &bytes)]);
        other
            .run_default(&dir, &[chatgpt("conversations.json")])
            .unwrap();
        std::fs::read_to_string(job_dir_of(&other).join("receipt.json")).unwrap()
    };
    let foreign: &'static str = Box::leak(foreign_receipt.into_boxed_str());
    let cases: Vec<(&str, Box<dyn Fn(&Path)>)> = vec![
        (
            "missing",
            Box::new(|job| std::fs::remove_file(job.join("receipt.json")).unwrap()),
        ),
        (
            "truncated",
            Box::new(|job| {
                let text = std::fs::read(job.join("receipt.json")).unwrap();
                std::fs::write(job.join("receipt.json"), &text[..text.len() / 2]).unwrap();
            }),
        ),
        (
            "malformed",
            Box::new(|job| std::fs::write(job.join("receipt.json"), b"not json").unwrap()),
        ),
        (
            "valid receipt of another job",
            Box::new(move |job| std::fs::write(job.join("receipt.json"), foreign).unwrap()),
        ),
        (
            "valid but with altered counts",
            Box::new(|job| {
                let mut value: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(job.join("receipt.json")).unwrap())
                        .unwrap();
                value["imported"] = serde_json::json!(99);
                std::fs::write(
                    job.join("receipt.json"),
                    serde_json::to_vec(&value).unwrap(),
                )
                .unwrap();
            }),
        ),
        (
            "valid but claiming another manifest",
            Box::new(|job| {
                let mut value: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(job.join("receipt.json")).unwrap())
                        .unwrap();
                value["manifest_sha256"] = serde_json::json!("0".repeat(64));
                std::fs::write(
                    job.join("receipt.json"),
                    serde_json::to_vec(&value).unwrap(),
                )
                .unwrap();
            }),
        ),
    ];
    for (name, tamper) in cases {
        let fx = Fx::new("chatgpt");
        let (dir, receipt) = finished_job(&fx);
        let expected = published_digest(&fx);
        let job = job_dir_of(&fx);
        let had_receipt = name != "missing";
        tamper(&job);

        // Never reported as a verified finished job; typed, and the same with or without a check.
        let message = error_text(fx.run_default(&dir, &[chatgpt("conversations.json")]));
        assert!(
            message.contains("E_PUBLISHED_MISMATCH"),
            "{name}: {message}"
        );
        assert!(fx
            .store
            .verify_published_job(&receipt.job_key, false)
            .unwrap_err()
            .to_string()
            .contains("E_PUBLISHED_MISMATCH"));

        // Explicit repair: a correct receipt again, the old audit files kept, nothing lost.
        let repaired = repair(&fx, &dir).unwrap();
        assert!(!repaired.already_complete, "{name}");
        assert_eq!(repaired.job_key, receipt.job_key);
        assert_eq!(published_digest(&fx), expected, "{name}");
        fx.store
            .verify_published_job(&receipt.job_key, true)
            .unwrap();
        let again = fx
            .run_default(&dir, &[chatgpt("conversations.json")])
            .unwrap();
        assert!(again.already_complete && again.published_check == "presence-verified");
        let superseded = std::fs::read_dir(&job)
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with("receipt.superseded-")
            })
            .count();
        assert_eq!(superseded, had_receipt as usize, "{name}");
    }
}

#[test]
fn deep_verification_binds_evidence_and_links_to_the_ledger() {
    let fx = Fx::new("chatgpt");
    let (_, receipt) = finished_job(&fx);
    let key = receipt.job_key.clone();
    let link_path = files_under(&fx.archive().join("imports/dispositions"))[0].clone();
    let evidence_path = files_under(&fx.archive().join("imports/evidence"))[0].clone();
    let link_original = std::fs::read(&link_path).unwrap();
    let evidence_original = std::fs::read(&evidence_path).unwrap();
    fx.store.verify_published_job(&key, true).unwrap();

    let edit = |path: &Path, original: &[u8], change: &dyn Fn(&mut serde_json::Value)| {
        let mut value: serde_json::Value = serde_json::from_slice(original).unwrap();
        change(&mut value);
        std::fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
    };
    type Change = Box<dyn Fn(&mut serde_json::Value)>;
    let link_changes: Vec<(&str, Change)> = vec![
        (
            "link points at another snapshot",
            Box::new(|v| v["snapshot_id"] = serde_json::json!("f".repeat(64))),
        ),
        (
            "link says raw-only",
            Box::new(|v| v["disposition"] = serde_json::json!("raw-only")),
        ),
        (
            "link carries a reason",
            Box::new(|v| v["reason"] = serde_json::json!("invented")),
        ),
        (
            "link names another evidence record",
            Box::new(|v| v["evidence_id"] = serde_json::json!("a".repeat(64))),
        ),
        (
            "link names another importer",
            Box::new(|v| v["importer_version"] = serde_json::json!("other/9")),
        ),
    ];
    for (name, change) in link_changes {
        edit(&link_path, &link_original, change.as_ref());
        // Valid JSON, same file name: only the deep check sees it (documented).
        fx.store.verify_published_job(&key, false).unwrap();
        let message = fx
            .store
            .verify_published_job(&key, true)
            .unwrap_err()
            .to_string();
        assert!(
            message.contains("E_PUBLISHED_MISMATCH"),
            "{name}: {message}"
        );
        std::fs::write(&link_path, &link_original).unwrap();
        fx.store.verify_published_job(&key, true).unwrap();
    }

    let evidence_changes: Vec<(&str, Change)> = vec![
        (
            "metadata changed, payload hash kept",
            Box::new(|v| v["metadata_json"] = serde_json::json!("{\"edited\":true}")),
        ),
        (
            "source identifier changed",
            Box::new(|v| v["source_identifier"] = serde_json::json!("another-conversation")),
        ),
        (
            "member path changed",
            Box::new(|v| v["member_path"] = serde_json::json!("other.json")),
        ),
        (
            "span moved",
            Box::new(|v| v["offset"] = serde_json::json!(1)),
        ),
        (
            "manifest changed",
            Box::new(|v| v["manifest_sha256"] = serde_json::json!("0".repeat(64))),
        ),
        (
            "provider changed",
            Box::new(|v| v["provider"] = serde_json::json!("gemini")),
        ),
    ];
    for (name, change) in evidence_changes {
        edit(&evidence_path, &evidence_original, change.as_ref());
        fx.store.verify_published_job(&key, false).unwrap();
        let message = fx
            .store
            .verify_published_job(&key, true)
            .unwrap_err()
            .to_string();
        assert!(
            message.contains("E_PUBLISHED_MISMATCH"),
            "{name}: {message}"
        );
        std::fs::write(&evidence_path, &evidence_original).unwrap();
        fx.store.verify_published_job(&key, true).unwrap();
    }
}
