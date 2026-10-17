use mind_place_archive::{extension_id, ArchiveStore};
use mind_place_capture_host::serve_one;
use serde_json::{json, Value};
use std::io::Cursor;
use tempfile::tempdir;

fn origin() -> String {
    format!("chrome-extension://{}/", extension_id())
}
fn frame(value: Value) -> Vec<u8> {
    let body = serde_json::to_vec(&value).unwrap();
    let mut out = (body.len() as u32).to_le_bytes().to_vec();
    out.extend(body);
    out
}
fn response(out: &[u8]) -> Value {
    assert!(out.len() >= 4, "missing native response frame");
    let len = u32::from_le_bytes(out[..4].try_into().unwrap()) as usize;
    assert_eq!(out.len(), len + 4);
    serde_json::from_slice(&out[4..]).unwrap()
}
fn request() -> Value {
    json!({"protocol":1,"operation":"save","capture":{"schema_version":1,"provider":"chatgpt","conversation_id":"12345678-1234-4123-8123-123456789abc","source_url":"https://chatgpt.com/c/12345678-1234-4123-8123-123456789abc","title":"Native saved chat","captured_at":"2026-10-15T04:00:00Z","capture_method":"manual-dom","completeness":"partial","warnings":["Loaded messages only"],"messages":[{"id":"user-1","role":"user","text":"Keep the exact source","format":"plain","attachments":[]}]}})
}

#[test]
fn automatic_dom_is_durable_but_cannot_claim_complete_capture() {
    let dir = tempdir().unwrap();
    let mut automatic = request();
    automatic["capture"]["capture_method"] = json!("automatic-dom");
    let mut out = vec![];
    serve_one(
        Cursor::new(frame(automatic.clone())),
        &mut out,
        dir.path(),
        &origin(),
    )
    .unwrap();
    assert_eq!(response(&out)["ok"], true);
    automatic["capture"]["completeness"] = json!("complete");
    let mut rejected = vec![];
    serve_one(
        Cursor::new(frame(automatic)),
        &mut rejected,
        dir.path(),
        &origin(),
    )
    .unwrap();
    assert_eq!(response(&rejected)["ok"], false);
}

#[test]
fn save_ack_is_framed_after_durable_storage_and_is_deduplicated() {
    let dir = tempdir().unwrap();
    let mut out = vec![];
    serve_one(
        Cursor::new(frame(request())),
        &mut out,
        dir.path(),
        &origin(),
    )
    .unwrap();
    let ack = response(&out);
    assert_eq!(ack["ok"], true);
    assert_eq!(ack["message_count"], 1);
    assert_eq!(ack["unchanged"], false);
    let id = ack["id"].as_str().unwrap();
    let store = ArchiveStore::open(dir.path()).unwrap();
    assert_eq!(
        store.read(id, None).unwrap().capture.messages[0].text,
        "Keep the exact source"
    );
    let mut second = vec![];
    serve_one(
        Cursor::new(frame(request())),
        &mut second,
        dir.path(),
        &origin(),
    )
    .unwrap();
    assert_eq!(response(&second)["unchanged"], true);
}

#[test]
fn untrusted_extension_origin_does_not_read_or_write_archive() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("never-created");
    let mut out = vec![];
    serve_one(
        Cursor::new(frame(request())),
        &mut out,
        &root,
        "chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/",
    )
    .unwrap();
    assert_eq!(response(&out)["ok"], false);
    assert!(!root.exists());
}

#[test]
fn oversized_or_torn_frames_get_failure_receipts_without_panics() {
    for input in [
        ((32 * 1024 * 1024 + 1) as u32).to_le_bytes().to_vec(),
        vec![5, 0, 0, 0, b'{'],
        vec![0, 0, 0, 0],
    ] {
        let dir = tempdir().unwrap();
        let mut out = vec![];
        serve_one(Cursor::new(input), &mut out, dir.path(), &origin()).unwrap();
        assert_eq!(response(&out)["ok"], false);
    }
}

#[test]
fn invalid_json_unsupported_operations_and_provider_mismatch_are_rejected() {
    let mut wrong = request();
    wrong["capture"]["source_url"] =
        json!("https://example.com/c/12345678-1234-4123-8123-123456789abc");
    for input in [
        frame(json!({"protocol":1,"operation":"exec_command","cmd":"anything"})),
        frame(json!({"protocol":2,"operation":"ping"})),
        frame(wrong),
        vec![1, 0, 0, 0, b'{'],
    ] {
        let dir = tempdir().unwrap();
        let mut out = vec![];
        serve_one(Cursor::new(input), &mut out, dir.path(), &origin()).unwrap();
        assert_eq!(response(&out)["ok"], false);
        assert!(ArchiveStore::open(dir.path())
            .unwrap()
            .list("")
            .unwrap()
            .is_empty());
    }
}

#[test]
fn ping_has_no_storage_side_effects() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("absent");
    let mut out = vec![];
    serve_one(
        Cursor::new(frame(json!({"protocol":1,"operation":"ping"}))),
        &mut out,
        &root,
        &origin(),
    )
    .unwrap();
    assert_eq!(response(&out)["protocol"], 1);
    assert!(!root.exists());
}
