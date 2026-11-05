use mind_place_archive::{
    importers::{
        chatgpt_export::ChatGptExportParser, gemini_takeout::GeminiTakeoutParser, ReconciledItem,
    },
    validate_historical_capture, ArchiveStore, Capture, HistoricalImportAuthorization,
    ImportSourceInventory,
};
use mind_place_security::{ImportAuthorizationCapability, SecurityGate, UnlockLease};
use sha2::{Digest, Sha256};
use std::time::Duration;

const UNAUTHENTICATED: &[u8] =
    include_bytes!("fixtures/import-second-correction/unauthenticated-historical.json");
const NONRECIPROCAL: &[u8] =
    include_bytes!("fixtures/import-second-correction/chatgpt-nonreciprocal.json");
const DUPLICATE: &[u8] =
    include_bytes!("fixtures/import-second-correction/chatgpt-duplicate-message.json");
const QUOTED_CARDS: &[u8] =
    include_bytes!("fixtures/import-second-correction/gemini-single-quoted-cards.html");
const RESPONSE_WORD: &[u8] =
    include_bytes!("fixtures/import-second-correction/gemini-prompted-in-response.html");
const TRAVERSAL: &[u8] =
    include_bytes!("fixtures/import-second-correction/gemini-traversal-url.html");

#[test]
fn unauthenticated_historical_write_is_rejected_without_side_effects() {
    let dir = tempfile::tempdir().unwrap();
    let store = ArchiveStore::open(dir.path().join("archive-v1")).unwrap();
    let capture: Capture = serde_json::from_slice(UNAUTHENTICATED).unwrap();
    assert!(store.save_historical_import(capture, "chatgpt").is_err());
    assert_eq!(
        std::fs::read_dir(store.root().join("conversations"))
            .unwrap()
            .count(),
        0
    );
    assert!(!store.root().join(".archive.lock").exists());
}

#[test]
fn nonreciprocal_selected_leaf_is_not_complete() {
    let items = ChatGptExportParser::parse_conversations_json(NONRECIPROCAL).unwrap();
    match &items[0] {
        ReconciledItem::PreservedRawOnly {
            evidence,
            disposition_reason,
            ..
        } => {
            assert!(!disposition_reason.is_empty());
            let original: Vec<serde_json::Value> = serde_json::from_slice(NONRECIPROCAL).unwrap();
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&evidence.payload_bytes).unwrap(),
                original[0]
            );
        }
        ReconciledItem::Conversation { capture, .. } => assert_eq!(capture.completeness, "partial"),
    }
}

#[test]
fn duplicate_message_ids_never_emit_an_invalid_capture() {
    let items = ChatGptExportParser::parse_conversations_json(DUPLICATE).unwrap();
    match &items[0] {
        ReconciledItem::Conversation { capture, .. } => {
            validate_historical_capture(&mut capture.clone(), "chatgpt").unwrap();
        }
        ReconciledItem::PreservedRawOnly {
            evidence,
            disposition_reason,
            ..
        } => {
            assert!(disposition_reason.to_lowercase().contains("duplicate"));
            assert!(String::from_utf8_lossy(&evidence.payload_bytes).contains("answer"));
        }
    }
}

#[test]
fn single_quoted_html_cards_remain_separate_occurrences() {
    let items = GeminiTakeoutParser::parse_cards_html(QUOTED_CARDS).unwrap();
    assert_eq!(items.len(), 2);
    let mut ids = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let ReconciledItem::Conversation {
            capture,
            raw_evidence,
            ..
        } = item
        else {
            panic!("expected projection")
        };
        ids.push(&capture.conversation_id);
        assert_eq!(capture.messages.len(), 2);
        assert_eq!(capture.messages[0].text, ["first", "second"][index]);
        let raw = String::from_utf8_lossy(&raw_evidence.payload_bytes);
        assert!(!raw.contains(["answer two", "answer one"][index]));
        assert!(QUOTED_CARDS
            .windows(raw_evidence.payload_bytes.len())
            .any(|slice| slice == raw_evidence.payload_bytes));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&raw_evidence.metadata_json).unwrap()
                ["occurrence_index"],
            index
        );
    }
    assert_ne!(ids[0], ids[1]);
}

#[test]
fn prompted_inside_assistant_text_cannot_fabricate_a_user_turn() {
    let items = GeminiTakeoutParser::parse_cards_html(RESPONSE_WORD).unwrap();
    let ReconciledItem::Conversation {
        capture,
        raw_evidence,
        ..
    } = &items[0]
    else {
        panic!("expected projection")
    };
    assert_eq!(capture.messages.len(), 2);
    assert_eq!(capture.messages[0].role, "user");
    assert_eq!(capture.messages[0].text, "real question");
    assert_eq!(capture.messages[1].role, "assistant");
    assert_eq!(
        capture.messages[1].text,
        "The word Prompted appears in this answer."
    );
    assert!(RESPONSE_WORD.starts_with(&raw_evidence.payload_bytes));
}

#[test]
fn traversal_suffix_cannot_supply_an_official_gemini_identity() {
    let items = GeminiTakeoutParser::parse_cards_html(TRAVERSAL).unwrap();
    let ReconciledItem::Conversation {
        capture,
        raw_evidence,
        ..
    } = &items[0]
    else {
        panic!("expected projection")
    };
    assert!(capture
        .source_url
        .starts_with("mindplace-import://gemini-canvas/"));
    assert_ne!(capture.conversation_id, "c_goodvalid123");
    assert!(String::from_utf8_lossy(&raw_evidence.payload_bytes)
        .contains("c_goodvalid123/../../arbitrary"));
    validate_historical_capture(&mut capture.clone(), "gemini").unwrap();
}

struct OwnerFixture {
    home: tempfile::TempDir,
    store: ArchiveStore,
    gate: SecurityGate,
    lease: UnlockLease,
    capability: ImportAuthorizationCapability,
    inventory: ImportSourceInventory,
}

impl OwnerFixture {
    fn new() -> Self {
        let home = tempfile::tempdir().unwrap();
        let store = ArchiveStore::open(home.path().join("archive-v1")).unwrap();
        let gate = SecurityGate::open(home.path().join("security-v1.json")).unwrap();
        gate.initialize(b"synthetic-owner-credential").unwrap();
        let lease = gate.require_unlocked().unwrap();
        let source_root = home.path().join("source");
        std::fs::create_dir(&source_root).unwrap();
        std::fs::write(source_root.join("fixture.json"), UNAUTHENTICATED).unwrap();
        let inventory = ImportSourceInventory {
            provider: "chatgpt".into(),
            source_root,
            total_files: 1,
            total_bytes: UNAUTHENTICATED.len() as u64,
            manifest_sha256: Sha256::digest(UNAUTHENTICATED).into(),
        };
        let capability = gate
            .authorize_import_session(
                lease,
                b"synthetic-owner-credential",
                "chatgpt",
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

    fn capture(&self) -> Capture {
        serde_json::from_slice(UNAUTHENTICATED).unwrap()
    }
}

#[test]
fn owner_authorized_import_round_trips_and_deduplicates() {
    let fixture = OwnerFixture::new();
    let save = || {
        fixture
            .store
            .save_authorized_historical_import(fixture.capture(), &fixture.authorization())
            .unwrap()
    };
    let first = save();
    let second = save();
    assert!(!first.unchanged);
    assert!(second.unchanged);
    assert_eq!(first.snapshot_id, second.snapshot_id);
    let reopened = ArchiveStore::open(fixture.store.root()).unwrap();
    let stored = reopened.read(&first.id, Some(&first.snapshot_id)).unwrap();
    assert_eq!(stored.capture, fixture.capture());
}

#[test]
fn historical_write_rechecks_capability_scope_revocation_and_gate_instance() {
    let fixture = OwnerFixture::new();
    let attempt = |auth: &HistoricalImportAuthorization<'_>| {
        assert!(fixture
            .store
            .save_authorized_historical_import(fixture.capture(), auth)
            .is_err());
        assert_eq!(
            std::fs::read_dir(fixture.store.root().join("conversations"))
                .unwrap()
                .count(),
            0
        );
        assert!(!fixture.store.root().join(".archive.lock").exists());
    };
    let mut auth = fixture.authorization();
    auth.job_id = "wrong-job";
    attempt(&auth);
    for change in 0..3 {
        let mut inventory = fixture.inventory.clone();
        match change {
            0 => inventory.provider = "gemini".into(),
            1 => inventory.manifest_sha256 = [0; 32],
            _ => inventory.source_root = fixture.home.path().to_owned(),
        }
        let mut auth = fixture.authorization();
        auth.inventory = &inventory;
        attempt(&auth);
    }
    let reopened_gate = SecurityGate::open(fixture.gate.config_path()).unwrap();
    reopened_gate.unlock(b"synthetic-owner-credential").unwrap();
    let mut auth = fixture.authorization();
    auth.gate = &reopened_gate;
    auth.lease = reopened_gate.require_unlocked().unwrap();
    attempt(&auth);

    fixture
        .gate
        .revoke_import_capability(fixture.capability.token())
        .unwrap();
    attempt(&fixture.authorization());
    fixture.gate.lock();
    attempt(&fixture.authorization());
    fixture.gate.unlock(b"synthetic-owner-credential").unwrap();
    attempt(&fixture.authorization());
}

#[test]
fn owner_authorized_plaintext_import_still_refuses_a_vault_marker() {
    let fixture = OwnerFixture::new();
    std::fs::write(
        fixture.home.path().join("vault-v1.json"),
        b"synthetic-marker",
    )
    .unwrap();
    assert!(fixture
        .store
        .save_authorized_historical_import(fixture.capture(), &fixture.authorization())
        .is_err());
    assert!(!fixture.store.root().join(".archive.lock").exists());
}

#[test]
fn expired_capability_and_unrelated_owner_profile_cannot_write() {
    let fixture = OwnerFixture::new();
    let expired = fixture
        .gate
        .authorize_import_session(
            fixture.lease,
            b"synthetic-owner-credential",
            "chatgpt",
            &fixture.inventory.source_root,
            fixture.inventory.manifest_sha256,
            Duration::ZERO,
        )
        .unwrap();
    let mut auth = fixture.authorization();
    auth.capability = &expired;
    auth.job_id = expired.job_id();
    assert!(fixture
        .store
        .save_authorized_historical_import(fixture.capture(), &auth)
        .is_err());

    let other_store =
        ArchiveStore::open(fixture.home.path().join("other-profile/archive-v1")).unwrap();
    let other_gate =
        SecurityGate::open(fixture.home.path().join("other-profile/security-v1.json")).unwrap();
    other_gate.initialize(b"other-synthetic-owner").unwrap();
    assert!(other_store
        .save_authorized_historical_import(fixture.capture(), &fixture.authorization())
        .is_err());
    assert!(!fixture.store.root().join(".archive.lock").exists());
    assert!(!other_store.root().join(".archive.lock").exists());
}

#[test]
fn chatgpt_other_inconsistent_edges_and_node_ids_preserve_raw_only() {
    let original: serde_json::Value = serde_json::from_slice(DUPLICATE).unwrap();
    for case in 0..5 {
        let mut record = original.clone();
        record[0]["mapping"]["leaf"]["message"]["id"] = "m-leaf".into();
        match case {
            0 => record[0]["mapping"]["root"]["children"] = serde_json::json!([]),
            1 => record[0]["mapping"]["leaf"]["id"] = "wrong-key".into(),
            2 => record[0]["mapping"]["root"]["children"] = serde_json::json!(["leaf", "missing"]),
            3 => record[0]["mapping"]["root"]["children"] = serde_json::json!(["leaf", "leaf"]),
            _ => {
                record[0]["mapping"]["root"]["parent"] = "leaf".into();
                record[0]["mapping"]["leaf"]["children"] = serde_json::json!(["root"]);
            }
        }
        let bytes = serde_json::to_vec(&record).unwrap();
        let items = ChatGptExportParser::parse_conversations_json(bytes.as_slice()).unwrap();
        assert!(
            matches!(&items[0], ReconciledItem::PreservedRawOnly { .. }),
            "case {case}"
        );
    }
}

#[test]
fn valid_chatgpt_branch_ignores_duplicate_ids_on_unselected_alternatives() {
    let mut record: serde_json::Value = serde_json::from_slice(DUPLICATE).unwrap();
    record[0]["mapping"]["leaf"]["message"]["id"] = "m-leaf".into();
    record[0]["mapping"]["alternate"] = record[0]["mapping"]["leaf"].clone();
    record[0]["mapping"]["alternate"]["id"] = "alternate".into();
    record[0]["mapping"]["root"]["children"] = serde_json::json!(["leaf", "alternate"]);
    let bytes = serde_json::to_vec(&record).unwrap();
    let items = ChatGptExportParser::parse_conversations_json(bytes.as_slice()).unwrap();
    let ReconciledItem::Conversation {
        capture,
        raw_evidence,
        ..
    } = &items[0]
    else {
        panic!("valid branch refused")
    };
    assert_eq!(capture.completeness, "complete");
    assert_eq!(capture.messages.len(), 2);
    validate_historical_capture(&mut capture.clone(), "chatgpt").unwrap();
    assert!(String::from_utf8_lossy(&raw_evidence.payload_bytes).contains("alternate"));
}

#[test]
fn html_card_attributes_comments_and_raw_text_do_not_change_occurrence_boundaries() {
    let html = r#"<html><!-- <div class='outer-cell'>fake</div> --><script>"<div class='outer-cell'>fake</div>"</script><DIV data-x='>' CLASS='other outer-cell'><div>Prompted café &amp; tea<BR/><p>first &lt;reply&gt;</p></div></DIV><div class=outer-cell>Prompted second<br/><p>second reply</p></div></html>"#;
    let items = GeminiTakeoutParser::parse_cards_html(html.as_bytes()).unwrap();
    assert_eq!(items.len(), 2);
    for (index, item) in items.iter().enumerate() {
        let ReconciledItem::Conversation {
            capture,
            raw_evidence,
            ..
        } = item
        else {
            panic!()
        };
        assert_eq!(capture.messages.len(), 2, "card {index}: {capture:?}");
        assert_eq!(capture.messages[0].text, ["café & tea", "second"][index]);
        assert!(!String::from_utf8_lossy(&raw_evidence.payload_bytes).contains("fake"));
        validate_historical_capture(&mut capture.clone(), "gemini").unwrap();
    }
}

#[test]
fn assistant_paragraph_and_quoted_code_prompted_markers_are_not_users() {
    for body in [
        "<p>Prompted is an activity label.</p>",
        "<p>Answer <b>Prompted</b> remains here.</p>",
        "<p>Answer<code>Prompted fake</code> remains here.</p>",
        "<blockquote>Prompted quoted user<br></blockquote><p>answer</p>",
    ] {
        let html = format!("<div class='outer-cell'>{body}</div>");
        let items = GeminiTakeoutParser::parse_cards_html(html.as_bytes()).unwrap();
        let ReconciledItem::Conversation { capture, .. } = &items[0] else {
            panic!()
        };
        assert_eq!(capture.messages.len(), 1);
        assert_eq!(capture.messages[0].role, "assistant");
        assert_eq!(capture.completeness, "partial");
    }
}

#[test]
fn gemini_accepts_only_a_complete_canonical_conversation_route() {
    for suffix in [
        "/../../arbitrary",
        "/extra",
        "/",
        "/%2e%2e/c_othergood123",
        "?next=other",
        "#fragment",
        "%2fextra",
    ] {
        let html = format!("<div class='outer-cell'>Prompted query<br><p>reply</p><a href='https://gemini.google.com/app/c_goodvalid123{suffix}'>Link</a></div>");
        let items = GeminiTakeoutParser::parse_cards_html(html.as_bytes()).unwrap();
        let ReconciledItem::Conversation {
            capture,
            raw_evidence,
            ..
        } = &items[0]
        else {
            panic!()
        };
        assert!(
            capture
                .source_url
                .starts_with("mindplace-import://gemini-canvas/"),
            "{suffix}"
        );
        assert_eq!(raw_evidence.payload_bytes, html.as_bytes());
    }
    let html = b"<div class='outer-cell'>Prompted query<br><p>reply</p><a href='https://gemini.google.com/app/c_goodvalid123'>Link</a></div>";
    let items = GeminiTakeoutParser::parse_cards_html(html.as_slice()).unwrap();
    let ReconciledItem::Conversation { capture, .. } = &items[0] else {
        panic!()
    };
    assert_eq!(capture.conversation_id, "c_goodvalid123");
    assert_eq!(
        capture.source_url,
        "https://gemini.google.com/app/c_goodvalid123"
    );
}

#[test]
fn nested_assistant_block_is_not_a_new_prompt_container() {
    let html = b"<div class='outer-cell'>Prompted real question<br><p>Answer intro</p><div>Prompted artificial question<br><p>Answer continuation</p></div></div>";
    let items = GeminiTakeoutParser::parse_cards_html(html.as_slice()).unwrap();
    let ReconciledItem::Conversation { capture, .. } = &items[0] else {
        panic!()
    };
    assert_eq!(capture.messages.len(), 2);
    assert_eq!(capture.messages[0].text, "real question");
    assert!(capture.messages[1].text.contains("Answer intro"));
    assert!(capture.messages[1].text.contains("Answer continuation"));
}

#[test]
fn inline_markup_cannot_hide_a_url_suffix() {
    let html = b"<div class='outer-cell'>Prompted query<br><p>reply</p>https://gemini.google.com/app/c_goodvalid123<span>/../../arbitrary</span></div>";
    let items = GeminiTakeoutParser::parse_cards_html(html.as_slice()).unwrap();
    let ReconciledItem::Conversation {
        capture,
        raw_evidence,
        ..
    } = &items[0]
    else {
        panic!()
    };
    assert!(capture
        .source_url
        .starts_with("mindplace-import://gemini-canvas/"));
    assert_eq!(raw_evidence.payload_bytes, html.as_slice());
}

#[test]
fn repeated_flat_prompted_labels_remain_partial_without_inferred_users() {
    for separator in [" ", "<br>"] {
        let html = format!("<div class='outer-cell'>Prompted real question<br><p>Answer intro</p>{separator}Prompted artificial question<br><p>Answer continuation</p></div>");
        let items = GeminiTakeoutParser::parse_cards_html(html.as_bytes()).unwrap();
        let ReconciledItem::Conversation {
            capture,
            raw_evidence,
            ..
        } = &items[0]
        else {
            panic!()
        };
        assert_eq!(capture.messages.len(), 2);
        assert_eq!(capture.completeness, "partial");
        assert_eq!(capture.messages[0].text, "real question");
        assert!(capture
            .warnings
            .iter()
            .any(|warning| warning.contains("Ambiguous")));
        assert_eq!(raw_evidence.payload_bytes, html.as_bytes());
    }
}
