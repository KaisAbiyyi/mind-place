use mind_place_archive::{ArchiveStore, Capture};
use mind_place_capture_host::serve_one;
use serde_json::{json, Value};
use std::io::Cursor;
use tempfile::tempdir;

fn capture() -> Value {
    json!({
        "schema_version": 1, "provider": "claude",
        "conversation_id": "123e4567-e89b-12d3-a456-426614174000",
        "source_url": "https://claude.ai/chat/123e4567-e89b-12d3-a456-426614174000",
        "title": "Synthetic Claude conversation", "captured_at": "2026-10-23T13:00:00Z",
        "capture_method": "manual-dom", "completeness": "partial",
        "warnings": ["Only loaded messages were captured."],
        "messages": [
            {"id":"claude:user:u1", "role":"user", "text":"Question", "format":"plain", "attachments":[]},
            {"id":"claude:assistant:a1:branch-b2:regen-2", "role":"assistant",
             "text":"Reply", "format":"plain", "model":"claude-test", "attachments":[]}
        ]
    })
}

fn submit(capture: Value, root: &std::path::Path) -> Value {
    let bytes = serde_json::to_vec(&json!({"protocol":1,"operation":"save","capture":capture})).unwrap();
    let mut frame = (bytes.len() as u32).to_le_bytes().to_vec();
    frame.extend(bytes);
    let mut out = Vec::new();
    serve_one(Cursor::new(frame), &mut out, root,
        &format!("chrome-extension://{}/", mind_place_archive::extension_id())).unwrap();
    serde_json::from_slice(&out[4..]).unwrap()
}

#[test]
fn claude_native_roundtrip_keeps_branch_observation_and_prior_revision() {
    let dir = tempdir().unwrap();
    let first = capture();
    let receipt = submit(first.clone(), dir.path());
    assert_eq!(receipt["ok"], true);
    let id = receipt["id"].as_str().unwrap();
    let snapshot = receipt["snapshot_id"].as_str().unwrap();
    let store = ArchiveStore::open(dir.path()).unwrap();
    let recorded: Capture = store.read(id, None).unwrap().capture;
    assert_eq!(serde_json::to_value(recorded).unwrap(), first);
    assert_eq!(submit(first.clone(), dir.path())["unchanged"], true);
    let mut second = first;
    second["messages"][1]["id"] = json!("claude:assistant:a1:branch-b2:regen-3");
    second["messages"][1]["text"] = json!("New revision");
    let newer = submit(second, dir.path());
    assert_eq!(newer["ok"], true);
    assert_ne!(newer["snapshot_id"], snapshot);
    assert_eq!(store.read(id, Some(snapshot)).unwrap().capture.messages[1].text, "Reply");
    assert_eq!(store.snapshots(id).unwrap().len(), 2);
}

#[test]
fn native_claude_rejects_origin_identity_privacy_and_mode_spoofing() {
    for (field, value) in [
        ("provider",json!("chatgpt")),
        ("source_url",json!("https://claude.ai.evil.test/chat/123e4567-e89b-12d3-a456-426614174000")),
        ("source_url",json!("https://claude.ai/chat/123e4567-e89b-12d3-a456-426614174000?temporary=true")),
        ("source_url",json!("https://claude.ai/chat/223e4567-e89b-12d3-a456-426614174000")),
        ("conversation_id",json!("223e4567-e89b-12d3-a456-426614174000")),
        ("capture_method",json!("automatic-dom")), ("capture_method",json!("legacy-import")),
        ("completeness",json!("complete"))
    ] {
        let dir = tempdir().unwrap();
        let mut bad = capture();
        bad[field] = value;
        assert_eq!(submit(bad, dir.path())["ok"], false, "{field}");
        assert!(ArchiveStore::open(dir.path()).unwrap().list("").unwrap().is_empty());
    }
}
