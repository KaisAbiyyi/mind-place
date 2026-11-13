//! MP-IMPORT-FIX-01 (P1-1): historical-write authority ledger for the plaintext archive.
//!
//! Every public `ArchiveStore` entry point that can persist a capture is classified here as
//! either **requires `HistoricalImportAuthorization`** or **documented exception**. The
//! exceptions and the reasons are written down in
//! `docs/implementation-summary/2026-11-11-mp-import-fix-01.md`; a new public entry point that
//! is not classified here fails `every_public_persisting_entry_point_is_classified`.
//!
//! Synthetic fixtures only.

use mind_place_archive::{
    importers::chatgpt_export::ChatGptExportParser, ArchiveStore, Capture,
    HistoricalImportAuthorization, ImportSourceInventory, Message,
};
use mind_place_security::{ImportAuthorizationCapability, SecurityGate, UnlockLease};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Entry points that persist a capture, and how each one is governed.
///
/// `RequiresAuthorization`: cannot persist without a live, bound `HistoricalImportAuthorization`.
/// `RejectsAll`: a retained API shim that always refuses.
/// `DocumentedException`: owner-initiated pre-existing migration path that does NOT take the
/// authorization; its limits are asserted by the tests below.
#[allow(dead_code)]
#[derive(Debug, PartialEq, Eq)]
enum Governance {
    RequiresAuthorization,
    RejectsAll,
    DocumentedException,
}

const LEDGER: &[(&str, Governance)] = &[
    (
        "save_authorized_historical_import",
        Governance::RequiresAuthorization,
    ),
    // MP-IMPORT-RAW-01: raw-evidence-preserving source import. Takes no inventory or hash from
    // the caller; measures the source itself and verifies the gate per item.
    (
        "import_authorized_source",
        Governance::RequiresAuthorization,
    ),
    ("save_historical_import", Governance::RejectsAll),
    // Ordinary save: refuses takeout-import / export-import; accepts live DOM captures and,
    // as the sink of the two importers below, legacy-import / generic-import captures.
    ("save", Governance::DocumentedException),
    ("import_generic", Governance::DocumentedException),
    ("import_legacy", Governance::DocumentedException),
];

fn capture(provider: &str, method: &str, id: &str, url: &str) -> Capture {
    Capture {
        schema_version: 1,
        provider: provider.into(),
        conversation_id: id.into(),
        source_url: url.into(),
        title: "Synthetic authority fixture".into(),
        conversation_created_at: None,
        conversation_updated_at: None,
        captured_at: "2026-11-11T00:00:00Z".into(),
        capture_method: method.into(),
        completeness: "partial".into(),
        warnings: vec!["synthetic".into()],
        messages: vec![Message {
            id: "m1".into(),
            role: "user".into(),
            text: "synthetic text".into(),
            format: "plain".into(),
            authored_at: None,
            model: None,
            attachments: vec![],
        }],
    }
}

const CHATGPT_ID: &str = "12345678-1234-4123-8123-123456789abc";
const GEMINI_ID: &str = "c_synthetic123456";

fn chatgpt_export() -> Capture {
    capture(
        "chatgpt",
        "export-import",
        CHATGPT_ID,
        &format!("https://chatgpt.com/c/{CHATGPT_ID}"),
    )
}

fn gemini_takeout() -> Capture {
    capture(
        "gemini",
        "takeout-import",
        GEMINI_ID,
        &format!("https://gemini.google.com/app/{GEMINI_ID}"),
    )
}

fn live_chatgpt() -> Capture {
    let mut c = capture(
        "chatgpt",
        "manual-dom",
        CHATGPT_ID,
        &format!("https://chatgpt.com/c/{CHATGPT_ID}"),
    );
    c.warnings.clear();
    c
}

fn archive() -> (tempfile::TempDir, ArchiveStore) {
    let home = tempfile::tempdir().unwrap();
    let store = ArchiveStore::open(home.path().join("archive-v1")).unwrap();
    (home, store)
}

fn stored_conversations(store: &ArchiveStore) -> usize {
    std::fs::read_dir(store.root().join("conversations"))
        .unwrap()
        .count()
}

struct Owner {
    home: tempfile::TempDir,
    store: ArchiveStore,
    gate: SecurityGate,
    lease: UnlockLease,
    capability: ImportAuthorizationCapability,
    inventory: ImportSourceInventory,
}

impl Owner {
    fn new(provider: &str) -> Self {
        let home = tempfile::tempdir().unwrap();
        let store = ArchiveStore::open(home.path().join("archive-v1")).unwrap();
        let gate = SecurityGate::open(home.path().join("security-v1.json")).unwrap();
        gate.initialize(b"synthetic-owner-credential").unwrap();
        let lease = gate.require_unlocked().unwrap();
        let source_root = home.path().join("source");
        std::fs::create_dir(&source_root).unwrap();
        std::fs::write(source_root.join("fixture.json"), b"synthetic").unwrap();
        let inventory = ImportSourceInventory {
            provider: provider.into(),
            source_root,
            total_files: 1,
            total_bytes: 9,
            manifest_sha256: Sha256::digest(b"synthetic").into(),
        };
        let capability = gate
            .authorize_import_session(
                lease,
                b"synthetic-owner-credential",
                provider,
                &inventory.source_root,
                inventory.manifest_sha256,
                Duration::from_secs(60),
            )
            .unwrap();
        Self {
            home,
            store,
            gate,
            lease,
            capability,
            inventory,
        }
    }

    fn authorization(&self) -> HistoricalImportAuthorization<'_> {
        HistoricalImportAuthorization {
            gate: &self.gate,
            lease: self.lease,
            capability: &self.capability,
            job_id: self.capability.job_id(),
            inventory: &self.inventory,
        }
    }
}

// ---- save_authorized_historical_import: REQUIRES AUTHORIZATION ----

#[test]
fn authorized_entry_persists_only_historical_methods_for_the_authorized_provider() {
    let owner = Owner::new("chatgpt");
    // A live DOM capture is not a historical import, even with a valid authorization.
    assert!(owner
        .store
        .save_authorized_historical_import(live_chatgpt(), &owner.authorization())
        .is_err());
    // A different provider than the one authorized is refused.
    assert!(owner
        .store
        .save_authorized_historical_import(gemini_takeout(), &owner.authorization())
        .is_err());
    assert_eq!(stored_conversations(&owner.store), 0);
    // The authorized provider and method persists.
    let receipt = owner
        .store
        .save_authorized_historical_import(chatgpt_export(), &owner.authorization())
        .unwrap();
    assert!(!receipt.unchanged);
    assert_eq!(stored_conversations(&owner.store), 1);
    let _ = &owner.home;
}

// ---- save_historical_import: REJECTS ALL ----

#[test]
fn legacy_unauthenticated_historical_entry_rejects_every_provider() {
    let (_home, store) = archive();
    for (provider, capture) in [("chatgpt", chatgpt_export()), ("gemini", gemini_takeout())] {
        assert!(store.save_historical_import(capture, provider).is_err());
    }
    assert_eq!(stored_conversations(&store), 0);
    assert!(!store.root().join(".archive.lock").exists());
}

// ---- save: DOCUMENTED EXCEPTION (bounded) ----

#[test]
fn ordinary_save_refuses_export_and_takeout_historical_methods() {
    let (_home, store) = archive();
    assert!(store.save(chatgpt_export()).is_err());
    assert!(store.save(gemini_takeout()).is_err());
    assert_eq!(stored_conversations(&store), 0);
    assert!(!store.root().join(".archive.lock").exists());
}

#[test]
fn ordinary_save_exception_is_limited_to_live_capture_and_legacy_or_generic_methods() {
    let (_home, store) = archive();
    // Live capture: the ordinary purpose of `save`.
    assert!(store.save(live_chatgpt()).is_ok());
    // legacy-import is accepted by `save` (documented exception: it is the sink of
    // `import_legacy`, and a stored legacy capture is re-validated on every read).
    let legacy = capture(
        "chatgpt",
        "legacy-import",
        CHATGPT_ID,
        &format!("https://chatgpt.com/c/{CHATGPT_ID}"),
    );
    assert!(store.save(legacy).is_ok());
    // generic-import is accepted only when its immutable raw source exists; a fabricated
    // generic capture without preserved evidence is refused.
    let generic = capture(
        "generic",
        "generic-import",
        &format!("{:x}", Sha256::digest(b"never preserved")),
        "mindplace-import://generic/never-preserved",
    );
    assert!(store.save(generic).is_err());
    // No other historical method exists on this path: every method outside the set below
    // is refused for every provider.
    for provider in ["chatgpt", "gemini", "claude", "generic"] {
        for method in ["export-import", "takeout-import", "unknown-import", ""] {
            let mut c = live_chatgpt();
            c.provider = provider.into();
            c.capture_method = method.into();
            assert!(
                store.save(c).is_err(),
                "provider {provider} method {method:?} must be refused"
            );
        }
    }
}

// ---- import_generic: DOCUMENTED EXCEPTION (bounded) ----

#[test]
fn generic_import_can_only_mint_generic_provider_generic_method_captures() {
    let (_home, store) = archive();
    // Input shaped like a ChatGPT export, or like an owner-forged native capture, still
    // produces a generic capture at most; it can never become an export/takeout capture.
    let chatgpt_shaped = serde_json::to_vec(&serde_json::json!({
        "id": CHATGPT_ID,
        "title": "export-shaped",
        "provider": "chatgpt",
        "capture_method": "export-import",
        "messages": [{"role": "user", "content": "text"}]
    }))
    .unwrap();
    let receipt = store.import_generic("json", &chatgpt_shaped).unwrap();
    assert!(receipt.raw_preserved);
    if let Some(saved) = receipt.saved {
        let stored = store.read(&saved.id, None).unwrap();
        assert_eq!(stored.capture.provider, "generic");
        assert_eq!(stored.capture.capture_method, "generic-import");
    }
    // Unsupported formats and empty input are refused before anything is written.
    assert!(store.import_generic("zip", b"x").is_err());
    assert!(store.import_generic("json", b"").is_err());
}

// ---- import_legacy: DOCUMENTED EXCEPTION (bounded) ----

#[test]
fn legacy_import_requires_an_existing_trusted_directory_and_never_writes_historical_methods() {
    let (home, store) = archive();
    // A missing source directory writes nothing.
    assert!(store.import_legacy(&home.path().join("missing")).is_err());
    // An empty legacy profile imports nothing and leaves no conversation behind.
    let empty = home.path().join("sessions");
    std::fs::create_dir(&empty).unwrap();
    let receipt = store.import_legacy(&empty).unwrap();
    assert_eq!(receipt.imported, 0);
    assert_eq!(stored_conversations(&store), 0);
    // The captures it does write are always `legacy-import`; that invariant is asserted
    // end-to-end by `tests/legacy.rs` (`capture_method == "legacy-import"`), and the
    // `save` sink refuses takeout-import / export-import (see above).
}

// ---- ledger guard ----

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn every_public_persisting_entry_point_is_classified() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    let mut found = Vec::new();
    let mut save_under_lock_calls = 0usize;
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        // Skip in-file unit test modules: they may call private helpers freely.
        let production = text.split("#[cfg(test)]").next().unwrap();
        for line in production.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("pub fn ") {
                let name = rest.split(['(', '<']).next().unwrap();
                if name.contains("save") || name.contains("import") {
                    found.push(name.to_owned());
                }
            }
            save_under_lock_calls += line.matches("self.save_under_lock(").count();
        }
    }
    found.sort();
    found.dedup();
    let mut expected: Vec<String> = LEDGER.iter().map(|(name, _)| (*name).to_owned()).collect();
    expected.sort();
    assert_eq!(
        found, expected,
        "A public archive-core entry point that saves/imports is not in the historical-write \
         ledger. Classify it in tests/historical_write_authority.rs and \
         docs/implementation-summary/2026-11-11-mp-import-fix-01.md."
    );
    // The shared sink is reachable from exactly three places: ordinary save, the
    // authorization-gated capture entry point, and the authorization-gated source import
    // (`historical/job.rs`, which publishes raw evidence and provenance first).
    assert_eq!(
        save_under_lock_calls, 3,
        "save_under_lock call sites changed; re-classify every historical write path"
    );
    // Only the authorization-gated entry accepts the authorization type.
    let store_src = std::fs::read_to_string(src.join("store.rs")).unwrap();
    assert_eq!(
        store_src
            .matches("authorization: &HistoricalImportAuthorization")
            .count(),
        1
    );
}

// ---- parsers never persist ----

#[test]
fn parsers_produce_values_and_never_touch_the_archive() {
    let (_home, store) = archive();
    let export = serde_json::to_vec(&serde_json::json!([{
        "id": CHATGPT_ID, "title": "t", "create_time": 1700000000.0, "current_node": "n1",
        "mapping": {"n1": {"id":"n1","parent":null,"children":[],"message":{
            "id":"m1","author":{"role":"user"},"content":{"content_type":"text","parts":["x"]}}}}
    }]))
    .unwrap();
    let items = ChatGptExportParser::parse_conversations_json(&export[..]).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(stored_conversations(&store), 0);
}
