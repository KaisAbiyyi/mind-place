use mind_place_archive::{ArchiveStore, ImportReceipt};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf};
use tempfile::tempdir;

const CONVERSATION_ID: &str = "12345678-1234-4123-8123-123456789abc";

struct LegacyFixture {
    _temp: tempfile::TempDir,
    sessions: PathBuf,
    session: PathBuf,
    archive: PathBuf,
}

fn fixture(conversation_id: Value) -> LegacyFixture {
    let temp = tempdir().unwrap();
    let sessions = temp.path().join("old-profile").join("sessions");
    let session = sessions.join("session-one");
    let archive = temp.path().join("archive-v1");
    fs::create_dir_all(session.join("assets")).unwrap();
    fs::create_dir_all(session.join("messages")).unwrap();
    fs::create_dir_all(session.join("handoffs")).unwrap();

    fs::write(
        session.join("meta.json"),
        serde_json::to_vec(&json!({
            "id": "session-one",
            "title": "Legacy notes",
            "conversationId": conversation_id,
            "chatIds": [CONVERSATION_ID],
            "startedAt": 1781488200000_i64,
            "updatedAt": 1781488800000_i64
        }))
        .unwrap(),
    )
    .unwrap();

    let png = b"\x89PNG\r\n\x1a\nlegacy-png";
    let png_name = format!("{}.png", "a".repeat(32));
    fs::write(session.join("assets").join(&png_name), png).unwrap();
    let overflow_name = format!("{}.txt", "b".repeat(32));
    fs::write(
        session.join("assets").join(&overflow_name),
        "full assistant answer from overflow asset",
    )
    .unwrap();

    let user_event = json!({
        "seq": 1, "time": 1781488300000_i64, "authoredAt": 1781488300000_i64,
        "source": "extension", "kind": "user_message", "messageId": "user-1",
        "message": { "text": "journal user body", "truncated": false, "chars": 17 },
        "assets": [{ "id": png_name, "mimeType": "image/png", "bytes": png.len() }]
    });
    let assistant_event = json!({
        "seq": 2, "time": 1781488400000_i64, "authoredAt": 1781488400000_i64,
        "source": "extension", "kind": "assistant_message", "messageId": "assistant-1",
        "model": "gpt-test", "resolvedModel": "gpt-test-resolved", "final": true,
        "message": { "text": "short assistant prefix", "truncated": true, "chars": 40, "assetId": overflow_name }
    });
    let image_event = json!({
        "seq": 3, "time": 1781488500000_i64, "source": "extension", "kind": "native_image",
        "messageId": "assistant-1", "providerAssetId": "native-one", "previewStatus": "available",
        "asset": { "id": png_name, "mimeType": "image/png", "bytes": png.len() }
    });
    let tool_event = json!({
        "seq": 4, "time": 1781488600000_i64, "source": "mcp", "kind": "tool_call",
        "call": { "callId": "raw-evidence", "tool": "exec_command" }
    });
    let mut journal = String::new();
    for event in [&user_event, &assistant_event, &image_event, &tool_event] {
        journal.push_str(&serde_json::to_string(event).unwrap());
        journal.push('\n');
    }
    fs::write(session.join("events.jsonl"), journal).unwrap();

    let user_key = "user_message\0user-1";
    let legacy_user = json!({
        "seq": 1, "time": 1781488300000_i64, "source": "extension", "kind": "user_message",
        "messageId": "user-1", "message": { "text": "legacy-map user body", "truncated": false, "chars": 20 }
    });
    fs::write(
        session.join("messages.json"),
        serde_json::to_vec(&json!({ user_key: legacy_user })).unwrap(),
    )
    .unwrap();
    let shard_user = json!({
        "seq": 1, "time": 1781488300000_i64, "source": "extension", "kind": "user_message",
        "messageId": "user-1", "message": { "text": "canonical shard user body", "truncated": false, "chars": 26 },
        "assets": [{ "id": png_name, "mimeType": "image/png", "bytes": png.len() }]
    });
    let shard_key = format!(
        "{}.json",
        format!("{:x}", Sha256::digest(user_key.as_bytes()))
    );
    fs::write(
        session.join("messages").join(shard_key),
        serde_json::to_vec(&shard_user).unwrap(),
    )
    .unwrap();

    fs::write(
        session.join("handoffs").join("handoff-1.json"),
        b"{\"callId\":\"handoff-preserved\"}\n",
    )
    .unwrap();
    fs::create_dir_all(session.join("unknown/nested")).unwrap();
    fs::write(
        session.join("unknown/nested/opaque.bin"),
        [0, 1, 2, 255, 10],
    )
    .unwrap();

    LegacyFixture {
        _temp: temp,
        sessions,
        session,
        archive,
    }
}

fn raw_revision(store: &ArchiveStore, session_name: &str) -> PathBuf {
    let parent = store.root().join("legacy").join(session_name);
    let mut revisions: Vec<_> = fs::read_dir(parent)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    revisions.sort();
    assert_eq!(revisions.len(), 1, "expected one raw source revision");
    revisions.remove(0)
}

#[test]
fn import_converts_overlay_messages_overflow_and_images_and_preserves_full_raw_session() {
    let f = fixture(json!(CONVERSATION_ID));
    let store = ArchiveStore::open(&f.archive).unwrap();
    let receipt = store.import_legacy(&f.sessions).unwrap();

    assert_eq!(receipt.imported, 1);
    assert_eq!(receipt.skipped, 0);
    assert_eq!(receipt.failed, 0);
    assert!(receipt
        .errors
        .iter()
        .any(|e| e.contains("observed local content")));
    let conversations = store.list("").unwrap();
    assert_eq!(conversations.len(), 1);
    let conversation = store.read(&conversations[0].id, None).unwrap();
    assert_eq!(conversation.capture.capture_method, "legacy-import");
    assert_eq!(conversation.capture.conversation_created_at, None);
    assert_eq!(conversation.capture.conversation_updated_at, None);
    assert_eq!(
        conversation.summary.date_source.as_deref(),
        Some("message-authored")
    );
    assert_eq!(
        conversation.summary.conversation_date.as_deref(),
        Some("2026-06-15T01:53:20.000Z")
    );
    assert_eq!(conversation.capture.completeness, "partial");
    assert!(conversation
        .capture
        .warnings
        .iter()
        .any(|w| w.to_lowercase().contains("observed")));
    assert_eq!(conversation.capture.messages.len(), 2);
    assert_eq!(
        conversation.capture.messages[0].text,
        "canonical shard user body"
    );
    assert_eq!(
        conversation.capture.messages[1].text,
        "full assistant answer from overflow asset"
    );
    assert_eq!(
        conversation.capture.messages[1].model.as_deref(),
        Some("gpt-test-resolved")
    );
    assert_eq!(conversation.capture.messages[0].attachments.len(), 1);
    assert_eq!(conversation.capture.messages[1].attachments.len(), 1);
    for message in &conversation.capture.messages {
        let image = &message.attachments[0];
        assert_eq!(image.mime_type, "image/png");
        assert!(
            image.asset_path.is_some(),
            "local images should be externalized by archive storage"
        );
        let resolved = store
            .asset(
                &conversation.summary.id,
                image.asset_path.as_deref().unwrap(),
            )
            .unwrap();
        assert!(resolved.data_url.starts_with("data:image/png;base64,"));
    }

    let raw = raw_revision(&store, "session-one");
    assert_eq!(
        fs::read(raw.join("events.jsonl")).unwrap(),
        fs::read(f.session.join("events.jsonl")).unwrap()
    );
    assert_eq!(
        fs::read(raw.join("messages.json")).unwrap(),
        fs::read(f.session.join("messages.json")).unwrap()
    );
    assert_eq!(
        fs::read(raw.join("handoffs/handoff-1.json")).unwrap(),
        b"{\"callId\":\"handoff-preserved\"}\n"
    );
    assert_eq!(
        fs::read(raw.join("unknown/nested/opaque.bin")).unwrap(),
        [0, 1, 2, 255, 10]
    );
    assert!(raw.join("messages").read_dir().unwrap().next().is_some());
    assert_eq!(
        fs::read(f.session.join("events.jsonl")).unwrap(),
        fs::read(raw.join("events.jsonl")).unwrap(),
        "import must leave its source untouched"
    );
    let durable_receipt: ImportReceipt =
        serde_json::from_slice(&fs::read(store.root().join("legacy/import-receipt.json")).unwrap())
            .unwrap();
    assert_eq!(durable_receipt.imported, 1);
    assert_eq!(durable_receipt.failed, 0);
}

#[test]
fn repeated_import_is_idempotent_and_changed_source_keeps_a_separate_raw_revision() {
    let f = fixture(json!(CONVERSATION_ID));
    let store = ArchiveStore::open(&f.archive).unwrap();
    let first = store.import_legacy(&f.sessions).unwrap();
    assert_eq!(first.imported, 1);
    let repeated = store.import_legacy(&f.sessions).unwrap();
    assert_eq!(repeated.imported, 0);
    assert_eq!(repeated.skipped, 1);
    assert_eq!(repeated.failed, 0);
    assert_eq!(
        fs::read_dir(store.root().join("legacy/session-one"))
            .unwrap()
            .count(),
        1
    );

    let extra_path = f.session.join("unknown/nested/new-evidence.bin");
    fs::write(&extra_path, b"later raw evidence").unwrap();
    let changed = store.import_legacy(&f.sessions).unwrap();
    assert_eq!(changed.imported, 1);
    assert_eq!(
        fs::read_dir(store.root().join("legacy/session-one"))
            .unwrap()
            .count(),
        2
    );
    assert!(fs::read_dir(store.root().join("legacy/session-one"))
        .unwrap()
        .all(|entry| entry.unwrap().path().join("unknown/nested").exists()));
}

#[test]
fn missing_provider_identity_preserves_raw_data_without_inventing_a_conversation() {
    let f = fixture(Value::Null);
    let store = ArchiveStore::open(&f.archive).unwrap();
    let receipt = store.import_legacy(&f.sessions).unwrap();
    assert_eq!(receipt.imported, 0);
    assert_eq!(receipt.skipped, 1);
    assert_eq!(receipt.failed, 0);
    assert!(receipt
        .errors
        .iter()
        .any(|e| e.contains("session-one") && e.to_lowercase().contains("conversation")));
    assert!(store.list("").unwrap().is_empty());
    let raw = raw_revision(&store, "session-one");
    assert_eq!(
        fs::read(raw.join("events.jsonl")).unwrap(),
        fs::read(f.session.join("events.jsonl")).unwrap()
    );
}

#[test]
fn unsafe_or_oversized_session_fails_as_a_unit_and_is_never_partially_published() {
    let f = fixture(json!(CONVERSATION_ID));
    fs::write(
        f.session.join("too-large.bin"),
        vec![0u8; 32 * 1024 * 1024 + 1],
    )
    .unwrap();
    let store = ArchiveStore::open(&f.archive).unwrap();
    let receipt = store.import_legacy(&f.sessions).unwrap();
    assert_eq!(receipt.imported, 0);
    assert_eq!(receipt.skipped, 0);
    assert_eq!(receipt.failed, 1);
    assert!(receipt.errors.iter().any(|e| e.contains("session-one")));
    assert!(!store.root().join("legacy/session-one").exists());
}

#[test]
fn missing_overflow_and_media_are_reported_without_silent_transcript_loss() {
    let f = fixture(json!(CONVERSATION_ID));
    fs::remove_file(
        f.session
            .join("assets")
            .join(format!("{}.txt", "b".repeat(32))),
    )
    .unwrap();
    fs::remove_file(
        f.session
            .join("assets")
            .join(format!("{}.png", "a".repeat(32))),
    )
    .unwrap();
    let store = ArchiveStore::open(&f.archive).unwrap();
    let receipt = store.import_legacy(&f.sessions).unwrap();
    assert_eq!(receipt.imported, 1);
    let conversation = store.read(&store.list("").unwrap()[0].id, None).unwrap();
    assert_eq!(
        conversation.capture.messages[1].text,
        "short assistant prefix"
    );
    assert!(conversation
        .capture
        .warnings
        .iter()
        .any(|w| w.contains("Overflow text for message assistant-1")));
    assert!(conversation
        .capture
        .messages
        .iter()
        .flat_map(|m| &m.attachments)
        .all(|a| a.unavailable_reason.is_some()));
    assert!(receipt
        .errors
        .iter()
        .any(|e| e.contains("warning") && e.contains("Overflow text for message assistant-1")));
}

#[test]
fn malformed_media_references_are_reported_while_the_message_and_raw_events_are_preserved() {
    let f = fixture(json!(CONVERSATION_ID));
    let mut journal = fs::read_to_string(f.session.join("events.jsonl")).unwrap();
    journal.push_str(
        &serde_json::to_string(&json!({
            "seq": 5, "source": "extension", "kind": "assistant_message",
            "messageId": "assistant-with-bad-media",
            "message": { "text": "Visible text survives malformed media", "truncated": false },
            "assets": [{ "id": 7 }, null],
            "attachments": [{ "id": 9 }, { "name": "missing upload id" }]
        }))
        .unwrap(),
    );
    journal.push('\n');
    journal.push_str(
        &serde_json::to_string(&json!({
            "seq": 6, "source": "extension", "kind": "native_image",
            "messageId": "assistant-with-bad-media"
        }))
        .unwrap(),
    );
    journal.push('\n');
    fs::write(f.session.join("events.jsonl"), &journal).unwrap();

    let store = ArchiveStore::open(&f.archive).unwrap();
    let receipt = store.import_legacy(&f.sessions).unwrap();
    assert_eq!(receipt.imported, 1);
    let conversation = store.read(&store.list("").unwrap()[0].id, None).unwrap();
    let message = conversation
        .capture
        .messages
        .iter()
        .find(|message| message.id == "legacy:assistant:assistant-with-bad-media")
        .unwrap();
    assert_eq!(message.text, "Visible text survives malformed media");
    assert!(message.attachments.is_empty());
    assert!(conversation.capture.warnings.iter().any(|warning| {
        warning.contains("assistant-with-bad-media") && warning.contains("asset")
    }));
    assert!(conversation.capture.warnings.iter().any(|warning| {
        warning.contains("assistant-with-bad-media") && warning.contains("upload")
    }));
    assert!(conversation.capture.warnings.iter().any(|warning| {
        warning.contains("native image") && warning.contains("assistant-with-bad-media")
    }));
    assert_eq!(
        fs::read(f.session.join("events.jsonl")).unwrap(),
        journal.as_bytes()
    );
}

#[cfg(unix)]
#[test]
fn symbolic_link_inside_session_rejects_that_session_before_copying() {
    use std::os::unix::fs::symlink;
    let f = fixture(json!(CONVERSATION_ID));
    let outside = f._temp.path().join("outside.bin");
    fs::write(&outside, b"outside").unwrap();
    symlink(&outside, f.session.join("linked.bin")).unwrap();
    let store = ArchiveStore::open(&f.archive).unwrap();
    let receipt = store.import_legacy(&f.sessions).unwrap();
    assert_eq!(receipt.failed, 1);
    assert!(!store.root().join("legacy/session-one").exists());
}
