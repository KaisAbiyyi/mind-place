use mind_place_archive::{ArchiveStore, Capture, Message, MAX_GENERIC_IMPORT_BYTES};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::fs;
use tempfile::tempdir;

fn archive() -> (tempfile::TempDir, ArchiveStore) {
    let root = tempdir().unwrap();
    let store = ArchiveStore::open(root.path().join("archive")).unwrap();
    (root, store)
}

#[test]
fn json_raw_roundtrip_identity_and_idempotent_snapshot() {
    let (_tmp, store) = archive();
    let raw = serde_json::to_vec(&json!({
        "title": "Synthetic imported session",
        "messages": [
            {"role": "user", "content": "hello", "authored_at": "2026-10-23T00:00:00Z"},
            {"role": "assistant", "text": "world", "model": "synthetic"}
        ]
    }))
    .unwrap();
    let sha = format!("{:x}", Sha256::digest(&raw));
    let first = store.import_generic("json", &raw).unwrap();
    assert!(first.raw_preserved);
    assert_eq!(first.raw_sha256, sha);
    assert!(first.errors.is_empty());
    assert!(!first.saved.as_ref().unwrap().unchanged);
    let id = first.saved.as_ref().unwrap().id.clone();
    let conversation = store.read(&id, None).unwrap();
    assert_eq!(conversation.capture.provider, "generic");
    assert_eq!(conversation.capture.capture_method, "generic-import");
    assert_eq!(
        conversation.capture.source_url,
        format!("mindplace-import://generic/{sha}")
    );
    assert_eq!(conversation.capture.messages.len(), 2);
    assert_eq!(
        conversation.capture.messages[0].authored_at.as_deref(),
        Some("2026-10-23T00:00:00Z")
    );
    assert_eq!(
        conversation.capture.messages[1].model.as_deref(),
        Some("synthetic")
    );
    assert!(
        conversation.capture.messages[1].authored_at.is_none(),
        "missing source time is not invented"
    );
    assert_eq!(conversation.capture.completeness, "partial");
    assert_eq!(
        fs::read(
            store
                .root()
                .join("imports/generic")
                .join(format!("{sha}.raw"))
        )
        .unwrap(),
        raw
    );
    let next = store.import_generic("json", &raw).unwrap();
    assert!(next.saved.unwrap().unchanged);
    assert_eq!(store.snapshots(&id).unwrap().len(), 1);
}

#[test]
fn markdown_role_boundaries_and_html_entities_are_explicit() {
    let (_tmp, store) = archive();
    let markdown = b"# Synthetic notes\n\n## User\nQuestion\n## Assistant\n**Answer**\n";
    let md = store
        .import_generic("markdown", markdown)
        .unwrap()
        .saved
        .unwrap();
    let capture = store.read(&md.id, None).unwrap().capture;
    assert_eq!(capture.title, "Synthetic notes");
    assert_eq!(capture.messages[0].text, "Question");
    assert_eq!(capture.messages[1].text, "**Answer**");
    assert_eq!(capture.messages[1].format, "markdown");
    let html = br#"<html><body><article data-role="user"><p>A &amp; B</p></article><section data-role="assistant"><p>&lt;safe&gt;</p></section></body></html>"#;
    let receipt = store.import_generic("html", html).unwrap();
    assert!(receipt.errors.is_empty(), "{:?}", receipt.errors);
    let chat = store
        .read(&receipt.saved.unwrap().id, None)
        .unwrap()
        .capture;
    assert_eq!(chat.messages.len(), 2);
    assert_eq!(chat.messages[0].text, "A & B");
    assert_eq!(chat.messages[1].text, "<safe>");
}

#[test]
fn unsupported_and_ambiguous_content_keeps_original_evidence_without_fake_chat() {
    let (_tmp, store) = archive();
    for (format, raw) in [
        ("json", br#"{"messages":[{"role":"user","content":"a","text":"b"}]}"#.as_slice()),
        ("json", br#"{"messages":[{"role":"unknown","content":"private"}]}"#.as_slice()),
        ("json", br#"{"messages":[{"role":"user","content":{"parts":["unknown"]}}]}"#.as_slice()),
        ("json", br#"{"messages":[{"role":"user","content":"a","attachments":[{"url":"https://example.org"}]}]}"#.as_slice()),
        ("markdown", b"An ordinary markdown README, not a transcript".as_slice()),
        ("html", b"<script>alert(1)</script><article data-role=\"user\">a</article>".as_slice()),
        ("html", b"<p>Generic web page without labels</p>".as_slice()),
    ] {
        let receipt = store.import_generic(format, raw).unwrap();
        assert!(receipt.raw_preserved);
        assert!(receipt.saved.is_none(), "unsupported source was published as a chat");
        assert!(!receipt.errors.is_empty());
        assert_eq!(fs::read(store.root().join("imports/generic").join(format!("{}.raw", receipt.raw_sha256))).unwrap(), raw);
    }
    assert!(store.list("").unwrap().is_empty());
}

#[test]
fn size_bounds_and_unsupported_formats_fail_before_copying() {
    let (_tmp, store) = archive();
    assert!(store.import_generic("text", b"hello").is_err());
    assert!(store.import_generic("json", b"").is_err());
    assert!(store
        .import_generic("json", &vec![0; MAX_GENERIC_IMPORT_BYTES + 1])
        .is_err());
    assert!(!store.root().join("imports").exists());
}

#[test]
fn archive_rejects_generic_identity_spoofing_and_dom_bypass() {
    let (_tmp, store) = archive();
    let raw = br#"[{"role":"user","text":"truth"}]"#;
    let receipt = store.import_generic("json", raw).unwrap();
    let id = receipt.saved.unwrap().id;
    let mut capture = store.read(&id, None).unwrap().capture;
    capture.capture_method = "automatic-dom".into();
    assert!(store.save(capture.clone()).is_err());
    capture.capture_method = "generic-import".into();
    capture.source_url = "https://chatgpt.com/c/12345678-1234-4123-8123-123456789abc".into();
    assert!(store.save(capture.clone()).is_err());
    capture.source_url = "mindplace-import://generic/".to_string() + &"0".repeat(64);
    assert!(store.save(capture.clone()).is_err());
    // Even a valid-shaped ChatGPT payload cannot claim to be a generic import.
    let fake = Capture {
        provider: "chatgpt".into(),
        capture_method: "generic-import".into(),
        messages: vec![Message {
            id: "a".into(),
            role: "user".into(),
            text: "a".into(),
            format: "plain".into(),
            authored_at: None,
            model: None,
            attachments: vec![],
        }],
        ..capture
    };
    assert!(store.save(fake).is_err());
}
