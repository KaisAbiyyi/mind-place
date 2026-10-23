use mind_place_archive::{extension_id, ArchiveStore, Capture};
use mind_place_capture_host::serve_one;
use serde_json::{json, Value};
use std::{collections::HashSet, fs, io::Cursor, path::Path};
use tempfile::tempdir;

fn fixture() -> Capture {
    serde_json::from_str(include_str!(
        "../../../test/fixtures/capture/chatgpt-v1.json"
    ))
    .unwrap()
}

fn submit(capture: &Capture, root: &Path) -> Value {
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
fn three_browser_sources_and_generic_import_remain_isolated_and_immutable() {
    let dir = tempdir().unwrap();
    let mut seen = HashSet::new();
    let mut snapshots = Vec::new();
    for (provider, conversation_id, source_url) in [
        (
            "chatgpt",
            "123e4567-e89b-12d3-a456-426614174000",
            "https://chatgpt.com/c/123e4567-e89b-12d3-a456-426614174000",
        ),
        (
            "claude",
            "123e4567-e89b-12d3-a456-426614174000",
            "https://claude.ai/chat/123e4567-e89b-12d3-a456-426614174000",
        ),
        (
            "gemini",
            "AbCd1234efGh5678",
            "https://gemini.google.com/app/AbCd1234efGh5678",
        ),
    ] {
        let mut original = fixture();
        original.provider = provider.into();
        original.conversation_id = conversation_id.into();
        original.source_url = source_url.into();
        let first = submit(&original, dir.path());
        assert_eq!(first["ok"], true, "{provider}: {first:?}");
        assert_eq!(first["completeness"], "partial");
        let id = first["id"].as_str().unwrap().to_owned();
        let snapshot = first["snapshot_id"].as_str().unwrap().to_owned();
        assert!(seen.insert(id.clone()), "provider identity collision");
        let raw_path = dir
            .path()
            .join("conversations")
            .join(&id)
            .join("captures")
            .join(format!("{snapshot}.jsonl"));
        let raw = fs::read(&raw_path).unwrap();
        let mut repeat = original.clone();
        repeat.captured_at = "2026-10-24T00:00:00Z".into();
        assert_eq!(submit(&repeat, dir.path())["unchanged"], true);
        assert_eq!(fs::read(&raw_path).unwrap(), raw);
        snapshots.push((id, snapshot, original, raw_path, raw));
    }

    let store = ArchiveStore::open(dir.path()).unwrap();
    let generic_source = br#"[{"role":"user","content":"synthetic file only"}]"#;
    let imported = store.import_generic("json", generic_source).unwrap();
    assert!(imported.raw_preserved);
    assert!(imported.errors.is_empty());
    let generic_id = imported.saved.unwrap().id;
    assert!(seen.insert(generic_id.clone()));
    let imported_capture = store.read(&generic_id, None).unwrap().capture;
    assert_eq!(imported_capture.provider, "generic");
    assert_eq!(imported_capture.capture_method, "generic-import");
    assert!(imported_capture.source_url.starts_with("mindplace-import://generic/"));
    assert_eq!(
        fs::read(
            store
                .root()
                .join("imports/generic")
                .join(format!("{}.raw", imported.raw_sha256))
        )
        .unwrap(),
        generic_source
    );
    assert_eq!(store.list("").unwrap().len(), 4);

    let (gemini_id, _, gemini_first, _, _) = &snapshots[2];
    let mut regenerated = gemini_first.clone();
    regenerated.messages[1].text = "Synthetic regeneration: different visible answer".into();
    let newer = submit(&regenerated, dir.path());
    assert_eq!(newer["ok"], true);
    assert_ne!(newer["snapshot_id"], snapshots[2].1);
    assert_eq!(store.snapshots(gemini_id).unwrap().len(), 2);
    for (id, snapshot, original, path, raw) in &snapshots {
        assert_eq!(store.read(id, Some(snapshot)).unwrap().capture, *original);
        assert_eq!(fs::read(path).unwrap(), *raw, "raw evidence overwritten");
    }
    assert_eq!(store.list("").unwrap().len(), 4);
}

#[test]
fn native_host_rejects_cross_provider_spoof_and_generic_browser_bypass_without_writes() {
    let dir = tempdir().unwrap();
    let base = fixture();
    for (provider, conversation_id, source_url, mode) in [
        (
            "claude",
            "123e4567-e89b-12d3-a456-426614174000",
            "https://gemini.google.com/app/AbCd1234efGh5678",
            "manual-dom",
        ),
        (
            "gemini",
            "AbCd1234efGh5678",
            "https://claude.ai/chat/123e4567-e89b-12d3-a456-426614174000",
            "manual-dom",
        ),
        (
            "chatgpt",
            "123e4567-e89b-12d3-a456-426614174000",
            "https://chatgpt.com/c/123e4567-e89b-12d3-a456-426614174000?temporary-chat=true",
            "manual-dom",
        ),
        (
            "generic",
            "123e4567-e89b-12d3-a456-426614174000",
            "mindplace-import://generic/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "manual-dom",
        ),
        (
            "claude",
            "123e4567-e89b-12d3-a456-426614174000",
            "https://claude.ai/chat/123e4567-e89b-12d3-a456-426614174000",
            "automatic-dom",
        ),
        (
            "gemini",
            "AbCd1234efGh5678",
            "https://gemini.google.com/app/AbCd1234efGh5678",
            "automatic-dom",
        ),
    ] {
        let mut forged = base.clone();
        forged.provider = provider.into();
        forged.conversation_id = conversation_id.into();
        forged.source_url = source_url.into();
        forged.capture_method = mode.into();
        assert_eq!(submit(&forged, dir.path())["ok"], false, "{provider} spoof");
    }
    assert!(ArchiveStore::open(dir.path())
        .unwrap()
        .list("")
        .unwrap()
        .is_empty());
}
