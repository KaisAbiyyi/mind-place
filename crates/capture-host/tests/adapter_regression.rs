use mind_place_archive::{extension_id, ArchiveStore, Capture};
use mind_place_capture_host::serve_one;
use serde_json::{json, Value};
use std::{fs, io::Cursor};
use tempfile::tempdir;

// Shared with the serialized browser adapter test; synthetic authored text only.
fn fixture() -> Capture {
    serde_json::from_str(include_str!(
        "../../../test/fixtures/capture/chatgpt-v1.json"
    ))
    .unwrap()
}

fn submit(capture: &Capture, root: &std::path::Path) -> Value {
    let bytes =
        serde_json::to_vec(&json!({"protocol":1,"operation":"save","capture":capture})).unwrap();
    let mut frame = (bytes.len() as u32).to_le_bytes().to_vec();
    frame.extend(bytes);
    let mut out = Vec::new();
    serve_one(
        Cursor::new(frame),
        &mut out,
        root,
        &format!("chrome-extension://{}/", extension_id()),
    )
    .unwrap();
    let size = u32::from_le_bytes(out[..4].try_into().unwrap()) as usize;
    assert_eq!(size, out.len() - 4);
    serde_json::from_slice(&out[4..]).unwrap()
}

#[test]
fn browser_adapter_fixture_preserves_v1_identity_dedup_and_immutable_revisions() {
    let dir = tempdir().unwrap();
    let original = fixture();
    let first = submit(&original, dir.path());
    assert_eq!(first["ok"], true);
    assert_eq!(first["unchanged"], false);
    assert_eq!(first["message_count"], 2);
    assert_eq!(first["completeness"], "partial");
    // Historical V1 digest framing and normalized field order must not drift.
    let id = "ef9e5c9d3760f7fb4396e7fca0d86df9bdbd057e7dbb91d6e55d592b512d6aae";
    let snapshot = "7a8983e284ddb1baf9763a9db50a8d192fc757461026642c16dd5856b5c0439a";
    assert_eq!(first["id"], id);
    assert_eq!(first["snapshot_id"], snapshot);
    let raw_path = dir
        .path()
        .join("conversations")
        .join(id)
        .join("captures")
        .join(format!("{snapshot}.jsonl"));
    let raw = fs::read(&raw_path).unwrap();
    let store = ArchiveStore::open(dir.path()).unwrap();
    assert_eq!(store.read(id, None).unwrap().capture, original);
    assert_eq!(serde_json::from_slice::<Capture>(&raw).unwrap(), original);

    let mut repeated = original.clone();
    repeated.captured_at = "2026-10-23T13:00:00.000Z".into();
    assert_eq!(submit(&repeated, dir.path())["unchanged"], true);
    assert_eq!(fs::read(&raw_path).unwrap(), raw);

    let mut automatic = repeated;
    automatic.capture_method = "automatic-dom".into();
    automatic.messages.remove(0);
    automatic.warnings = vec!["Only loaded messages; no scrolling.".into()];
    let second = submit(&automatic, dir.path());
    assert_eq!(second["ok"], true);
    assert_ne!(second["snapshot_id"], snapshot);
    let reopened = ArchiveStore::open(dir.path()).unwrap();
    assert_eq!(reopened.read(id, None).unwrap().capture, automatic);
    assert_eq!(reopened.read(id, Some(snapshot)).unwrap().capture, original);
    assert_eq!(fs::read(&raw_path).unwrap(), raw);
    assert_eq!(reopened.snapshots(id).unwrap().len(), 2);
}

#[test]
fn native_host_independently_rejects_adapter_provider_identity_and_mode_spoofing() {
    for (key, value) in [
        ("provider", json!("claude")),
        ("schema_version", json!(2)),
        (
            "source_url",
            json!("https://chatgpt.com.evil.test/c/123e4567-e89b-12d3-a456-426614174000"),
        ),
        (
            "source_url",
            json!("https://chatgpt.com/c/123e4567-e89b-12d3-a456-426614174000?temporary-chat=true"),
        ),
        (
            "conversation_id",
            json!("223e4567-e89b-12d3-a456-426614174000"),
        ),
        ("capture_method", json!("legacy-import")),
        ("completeness", json!("complete")),
    ] {
        let dir = tempdir().unwrap();
        let mut value_capture = serde_json::to_value(fixture()).unwrap();
        value_capture[key] = value;
        let capture = serde_json::from_value(value_capture).unwrap();
        assert_eq!(submit(&capture, dir.path())["ok"], false, "{key}");
        assert!(ArchiveStore::open(dir.path())
            .unwrap()
            .list("")
            .unwrap()
            .is_empty());
    }
}

#[test]
fn gemini_native_host_roundtrip_revisions_and_fail_closed_identity() {
    let dir = tempdir().unwrap();
    let mut first_capture = fixture();
    first_capture.provider = "gemini".into();
    first_capture.conversation_id = "AbCd1234efGh5678".into();
    first_capture.source_url = "https://gemini.google.com/app/AbCd1234efGh5678".into();
    first_capture.messages[1].model = Some("gemini-synthetic".into());
    let first = submit(&first_capture, dir.path());
    assert_eq!(first["ok"], true);
    let id = first["id"].as_str().unwrap();
    let snapshot = first["snapshot_id"].as_str().unwrap();
    let path = dir
        .path()
        .join("conversations")
        .join(id)
        .join("captures")
        .join(format!("{snapshot}.jsonl"));
    let original = fs::read(&path).unwrap();

    let mut repeat = first_capture.clone();
    repeat.captured_at = "2026-10-23T14:00:00.000Z".into();
    assert_eq!(submit(&repeat, dir.path())["unchanged"], true);

    repeat.messages[1].text = "Regenerated visible Gemini answer".into();
    let second = submit(&repeat, dir.path());
    assert_eq!(second["ok"], true);
    assert_ne!(second["snapshot_id"], snapshot);
    let archive = ArchiveStore::open(dir.path()).unwrap();
    assert_eq!(archive.snapshots(id).unwrap().len(), 2);
    assert_eq!(
        archive.read(id, Some(snapshot)).unwrap().capture,
        first_capture
    );
    assert_eq!(fs::read(&path).unwrap(), original);
    assert_eq!(
        archive.read(id, None).unwrap().capture.messages[1].text,
        "Regenerated visible Gemini answer"
    );

    for (field, value) in [
        (
            "source_url",
            json!("https://gemini.google.com.evil.test/app/AbCd1234efGh5678"),
        ),
        (
            "source_url",
            json!("https://gemini.google.com/app/AbCd1234efGh5678?temporary=true"),
        ),
        ("conversation_id", json!("abcd1234efgh5678")),
        ("capture_method", json!("automatic-dom")),
        ("capture_method", json!("legacy-import")),
        ("completeness", json!("complete")),
    ] {
        let isolated = tempdir().unwrap();
        let mut spoofed = serde_json::to_value(&first_capture).unwrap();
        spoofed[field] = value;
        let spoofed: Capture = serde_json::from_value(spoofed).unwrap();
        assert_eq!(submit(&spoofed, isolated.path())["ok"], false, "{field}");
        assert!(ArchiveStore::open(isolated.path())
            .unwrap()
            .list("")
            .unwrap()
            .is_empty());
    }
}
