use mind_place_archive::{ArchiveStore, Attachment, Capture, Message};
use std::{fs, sync::Arc, thread};
use tempfile::tempdir;

fn capture() -> Capture {
    Capture {
        schema_version: 1,
        provider: "chatgpt".into(),
        conversation_id: "12345678-1234-4123-8123-123456789abc".into(),
        source_url: "https://chatgpt.com/c/12345678-1234-4123-8123-123456789abc".into(),
        title: "Catatan café 🪴".into(),
        conversation_created_at: None,
        conversation_updated_at: None,
        captured_at: "2026-10-15T03:00:00.000Z".into(),
        capture_method: "manual-dom".into(),
        completeness: "partial".into(),
        warnings: vec!["Only loaded messages were read".into()],
        messages: vec![
            Message {
                id: "u-1".into(),
                role: "user".into(),
                text: "Simpan keputusan ini".into(),
                format: "plain".into(),
                authored_at: None,
                model: None,
                attachments: vec![],
            },
            Message {
                id: "a-1".into(),
                role: "assistant".into(),
                text: "**Diputuskan**: Rust, dengan `JSONL`.".into(),
                format: "markdown".into(),
                authored_at: None,
                model: Some("gpt-test".into()),
                attachments: vec![],
            },
        ],
    }
}

fn named_capture(conversation_id: &str, title: &str, captured_at: &str) -> Capture {
    let mut result = capture();
    result.conversation_id = conversation_id.into();
    result.source_url = format!("https://chatgpt.com/c/{conversation_id}");
    result.title = title.into();
    result.captured_at = captured_at.into();
    result
}

#[test]
fn automatic_windows_preserve_previous_raw_and_known_original_date() {
    let dir = tempdir().unwrap();
    let store = ArchiveStore::open(dir.path()).unwrap();
    let mut manual = capture();
    manual.messages[0].authored_at = Some("2025-10-15T12:00:00Z".into());
    let first = store.save(manual.clone()).unwrap();
    let prior_raw = fs::read(
        dir.path()
            .join("conversations")
            .join(&first.id)
            .join("captures")
            .join(format!("{}.jsonl", first.snapshot_id)),
    )
    .unwrap();
    let mut automatic = capture();
    automatic.capture_method = "automatic-dom".into();
    automatic.captured_at = "2026-10-15T04:00:00Z".into();
    automatic.messages.remove(0);
    let saved = store.save(automatic.clone()).unwrap();
    assert_eq!(
        store.read(&saved.id, None).unwrap().capture.messages.len(),
        1
    );
    assert_eq!(
        store
            .read(&first.id, Some(&first.snapshot_id))
            .unwrap()
            .capture
            .messages
            .len(),
        2
    );
    assert_eq!(
        fs::read(
            dir.path()
                .join("conversations")
                .join(&first.id)
                .join("captures")
                .join(format!("{}.jsonl", first.snapshot_id))
        )
        .unwrap(),
        prior_raw
    );
    assert_eq!(
        store.list("").unwrap()[0].conversation_date.as_deref(),
        Some("2025-10-15T12:00:00Z")
    );
    automatic.captured_at = "2026-10-15T05:00:00Z".into();
    assert!(store.save(automatic.clone()).unwrap().unchanged);
    automatic.completeness = "complete".into();
    assert!(store.save(automatic).is_err());
    let mut legacy = manual;
    legacy.capture_method = "legacy-import".into();
    legacy.captured_at = "2026-10-15T06:00:00Z".into();
    store.save(legacy).unwrap();
    assert_eq!(
        store.read(&saved.id, None).unwrap().capture.capture_method,
        "automatic-dom"
    );
}

#[test]
fn durable_roundtrip_keeps_utf8_raw_jsonl_and_readable_markdown() {
    let dir = tempdir().unwrap();
    let store = ArchiveStore::open(dir.path()).unwrap();
    let mut original = capture();
    original.conversation_created_at = Some("2020-01-02T03:04:05Z".into());
    original.conversation_updated_at = Some("2026-10-14T12:30:00Z".into());
    let receipt = store.save(original).unwrap();
    assert!(receipt.ok);
    assert_eq!(receipt.message_count, 2);
    assert_eq!(receipt.completeness, "partial");
    let reopened = ArchiveStore::open(dir.path()).unwrap();
    let read = reopened.read(&receipt.id, None).unwrap();
    assert_eq!(
        read.capture.messages[1].text,
        "**Diputuskan**: Rust, dengan `JSONL`."
    );
    assert_eq!(read.summary.title, "Catatan café 🪴");
    let raw = fs::read_to_string(
        dir.path()
            .join("conversations")
            .join(&receipt.id)
            .join("captures")
            .join(format!("{}.jsonl", receipt.snapshot_id)),
    )
    .unwrap();
    assert_eq!(raw.lines().count(), 1);
    assert!(raw.contains("café 🪴"));
    let exported = reopened.export(&receipt.id).unwrap();
    assert!(exported.markdown.contains("## User"));
    assert!(exported
        .markdown
        .contains("**Diputuskan**: Rust, dengan `JSONL`."));
    assert!(exported
        .markdown
        .contains("Original conversation date (Updated by provider): 2026-10-14T12:30:00Z"));
    assert!(exported
        .markdown
        .contains("Conversation created by provider: 2020-01-02T03:04:05Z"));
    assert!(exported
        .markdown
        .contains("Captured: 2026-10-15T03:00:00.000Z"));
    assert!(exported.markdown.contains("partial"));
    assert_eq!(
        fs::read_to_string(exported.path).unwrap(),
        exported.markdown
    );
}

#[test]
fn absent_optional_provider_dates_remain_backward_compatible() {
    let json = serde_json::to_value(capture()).unwrap();
    assert!(json.get("conversation_created_at").is_none());
    assert!(json.get("conversation_updated_at").is_none());
    let reopened: Capture = serde_json::from_value(json).unwrap();
    assert_eq!(reopened.conversation_created_at, None);
    assert_eq!(reopened.conversation_updated_at, None);
}

#[test]
fn invalid_provider_dates_are_rejected_before_any_archive_write() {
    let dir = tempdir().unwrap();
    let store = ArchiveStore::open(dir.path()).unwrap();
    for invalid_date in ["not-a-date", "2026-10-15T25:00:00Z"] {
        let mut created = capture();
        created.conversation_created_at = Some(invalid_date.into());
        assert!(store.save(created).is_err());

        let mut updated = capture();
        updated.conversation_updated_at = Some(invalid_date.into());
        assert!(store.save(updated).is_err());
    }
    assert_eq!(
        fs::read_dir(dir.path().join("conversations"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn original_dates_keep_recaptured_old_chats_below_newer_conversations() {
    let dir = tempdir().unwrap();
    let store = ArchiveStore::open(dir.path()).unwrap();
    let mut old = named_capture(
        "12345678-1234-4123-8123-123456789abc",
        "Old conversation",
        "2026-10-15T08:00:00Z",
    );
    old.conversation_updated_at = Some("2023-05-01T12:00:00Z".into());
    let old_first = store.save(old.clone()).unwrap();

    let mut recent = named_capture(
        "12345678-1234-4123-8123-123456789abd",
        "Recent conversation",
        "2026-10-15T07:00:00Z",
    );
    recent.conversation_updated_at = Some("2025-04-01T12:00:00Z".into());
    store.save(recent).unwrap();

    old.captured_at = "2026-10-16T08:00:00Z".into();
    old.messages[1].text = "A later manual observation of old content".into();
    let old_revised = store.save(old).unwrap();
    assert_ne!(old_first.snapshot_id, old_revised.snapshot_id);

    let listed = store.list("").unwrap();
    assert_eq!(listed[0].title, "Recent conversation");
    assert_eq!(
        listed[0].conversation_date.as_deref(),
        Some("2025-04-01T12:00:00Z")
    );
    assert_eq!(listed[0].date_source.as_deref(), Some("provider-updated"));
    assert_eq!(listed[1].title, "Old conversation");
    assert_eq!(
        listed[1].conversation_date.as_deref(),
        Some("2023-05-01T12:00:00Z")
    );
    assert_eq!(listed[1].captured_at, "2026-10-16T08:00:00Z");
    assert_eq!(
        store.read(&old_revised.id, None).unwrap().capture.messages[1].text,
        "A later manual observation of old content"
    );
}

#[test]
fn gemini_missing_original_date_uses_capture_time_for_list_order_only() {
    let dir = tempdir().unwrap();
    let store = ArchiveStore::open(dir.path()).unwrap();

    let mut chatgpt = named_capture(
        "12345678-1234-4123-8123-123456789abc",
        "ChatGPT known date",
        "2026-10-15T08:00:00Z",
    );
    chatgpt.conversation_updated_at = Some("2026-10-15T07:00:00Z".into());
    store.save(chatgpt).unwrap();

    let mut unknown = named_capture(
        "12345678-1234-4123-8123-123456789abd",
        "ChatGPT undated",
        "2026-10-25T09:00:00Z",
    );
    unknown.conversation_updated_at = None;
    store.save(unknown).unwrap();

    let mut gemini = named_capture(
        "AbCd1234efGh5678",
        "Gemini fresh capture",
        "2026-10-24T07:53:49Z",
    );
    gemini.provider = "gemini".into();
    gemini.source_url = "https://gemini.google.com/app/AbCd1234efGh5678".into();
    let saved = store.save(gemini).unwrap();

    let mut dated_gemini = named_capture(
        "AbCd1234efGh5679",
        "Gemini has original date",
        "2026-10-26T07:53:49Z",
    );
    dated_gemini.provider = "gemini".into();
    dated_gemini.source_url = "https://gemini.google.com/app/AbCd1234efGh5679".into();
    dated_gemini.messages[0].authored_at = Some("2025-01-01T02:00:00Z".into());
    store.save(dated_gemini).unwrap();

    let summaries = store.list("").unwrap();
    assert_eq!(
        summaries
            .iter()
            .map(|s| s.title.as_str())
            .collect::<Vec<_>>(),
        vec![
            "Gemini fresh capture",
            "ChatGPT known date",
            "Gemini has original date",
            "ChatGPT undated"
        ]
    );
    let gemini_summary = &summaries[0];
    assert_eq!(gemini_summary.conversation_date, None);
    assert_eq!(gemini_summary.date_source, None);
    assert_eq!(gemini_summary.captured_at, "2026-10-24T07:53:49Z");
    let reloaded = store.read(&saved.id, None).unwrap();
    assert_eq!(reloaded.summary.conversation_date, None);
    assert_eq!(reloaded.summary.date_source, None);
    assert_eq!(
        summaries[2].date_source.as_deref(),
        Some("message-authored")
    );
    assert_eq!(
        summaries[2].conversation_date.as_deref(),
        Some("2025-01-01T02:00:00Z")
    );
}

#[test]
fn list_orders_parsed_instants_then_unknown_dates_by_title_and_id() {
    let dir = tempdir().unwrap();
    let store = ArchiveStore::open(dir.path()).unwrap();
    let mut offset_later = named_capture(
        "12345678-1234-4123-8123-123456789abc",
        "Offset later instant",
        "2026-10-15T03:00:00Z",
    );
    offset_later.conversation_updated_at = Some("2026-10-15T08:30:00+02:00".into());
    offset_later.messages[1].authored_at = Some("2026-10-15T10:00:00Z".into());
    store.save(offset_later).unwrap();

    let mut utc_earlier_text_but_newer = named_capture(
        "12345678-1234-4123-8123-123456789abd",
        "UTC newer instant",
        "2026-10-15T02:00:00Z",
    );
    utc_earlier_text_but_newer.conversation_updated_at = Some("2026-10-15T07:00:00Z".into());
    store.save(utc_earlier_text_but_newer).unwrap();

    let mut created_only = named_capture(
        "12345678-1234-4123-8123-123456789ab0",
        "Created only",
        "2026-10-15T22:00:00Z",
    );
    created_only.conversation_created_at = Some("2026-10-15T05:00:00Z".into());
    let created_receipt = store.save(created_only).unwrap();
    assert_eq!(
        store
            .read(&created_receipt.id, None)
            .unwrap()
            .summary
            .date_source
            .as_deref(),
        Some("provider-created")
    );

    store
        .save(named_capture(
            "12345678-1234-4123-8123-123456789abe",
            "Beta unknown",
            "2026-10-15T23:00:00Z",
        ))
        .unwrap();
    store
        .save(named_capture(
            "12345678-1234-4123-8123-123456789abf",
            "Alpha unknown",
            "2026-10-15T01:00:00Z",
        ))
        .unwrap();

    let titles: Vec<_> = store
        .list("")
        .unwrap()
        .into_iter()
        .map(|item| item.title)
        .collect();
    assert_eq!(
        titles,
        vec![
            "UTC newer instant".to_owned(),
            "Offset later instant".to_owned(),
            "Created only".to_owned(),
            "Alpha unknown".to_owned(),
            "Beta unknown".to_owned(),
        ]
    );
}

#[test]
fn old_meta_dates_are_derived_from_bounded_raw_without_rewriting_it_and_revisions_keep_their_date()
{
    let dir = tempdir().unwrap();
    let store = ArchiveStore::open(dir.path()).unwrap();
    let mut earlier = capture();
    earlier.messages[0].authored_at = Some("2024-02-03T10:00:00+02:00".into());
    earlier.messages[1].authored_at = Some("2024-02-03T07:30:00Z".into());
    let first = store.save(earlier).unwrap();
    let mut later = capture();
    later.messages[1].authored_at = Some("2025-04-05T11:00:00Z".into());
    later.messages[1].text = "Later authored answer".into();
    later.captured_at = "2026-10-15T06:00:00Z".into();
    let second = store.save(later).unwrap();

    let conversation_dir = dir.path().join("conversations").join(&second.id);
    let meta_path = conversation_dir.join("meta.json");
    let mut meta: serde_json::Value =
        serde_json::from_slice(&fs::read(&meta_path).unwrap()).unwrap();
    let summary = meta["summary"].as_object_mut().unwrap();
    summary.remove("conversation_date");
    summary.remove("date_source");
    let old_meta = serde_json::to_vec(&meta).unwrap();
    fs::write(&meta_path, &old_meta).unwrap();
    let latest_raw_path = conversation_dir
        .join("captures")
        .join(format!("{}.jsonl", second.snapshot_id));
    let latest_raw = fs::read(&latest_raw_path).unwrap();

    let listed = store.list("").unwrap();
    assert_eq!(
        listed[0].conversation_date.as_deref(),
        Some("2025-04-05T11:00:00Z")
    );
    assert_eq!(listed[0].date_source.as_deref(), Some("message-authored"));
    let reopened_earlier = store.read(&first.id, Some(&first.snapshot_id)).unwrap();
    assert_eq!(
        reopened_earlier.summary.conversation_date.as_deref(),
        Some("2024-02-03T10:00:00+02:00")
    );
    assert_eq!(
        reopened_earlier.summary.date_source.as_deref(),
        Some("message-authored")
    );
    assert_eq!(fs::read(&meta_path).unwrap(), old_meta);
    assert_eq!(fs::read(&latest_raw_path).unwrap(), latest_raw);
}

#[test]
fn same_content_at_a_later_time_deduplicates_while_revisions_remain_readable() {
    let dir = tempdir().unwrap();
    let store = ArchiveStore::open(dir.path()).unwrap();
    let first = store.save(capture()).unwrap();
    let mut same = capture();
    same.captured_at = "2026-10-15T04:00:00Z".into();
    let repeated = store.save(same).unwrap();
    assert!(repeated.unchanged);
    assert_eq!(first.snapshot_id, repeated.snapshot_id);
    assert_eq!(
        store.read(&first.id, None).unwrap().summary.snapshot_count,
        1
    );
    let mut revised = capture();
    revised.captured_at = "2026-10-15T05:00:00Z".into();
    revised.messages[1].text = "Second answer".into();
    let second = store.save(revised).unwrap();
    assert_ne!(first.snapshot_id, second.snapshot_id);
    assert_eq!(
        store
            .read(&first.id, Some(&first.snapshot_id))
            .unwrap()
            .capture
            .messages[1]
            .text,
        "**Diputuskan**: Rust, dengan `JSONL`."
    );
    assert_eq!(
        store.read(&first.id, None).unwrap().capture.messages[1].text,
        "Second answer"
    );
    assert_eq!(store.snapshots(&first.id).unwrap().len(), 2);
}

#[test]
fn a_later_partial_capture_never_overwrites_previous_raw_evidence() {
    let dir = tempdir().unwrap();
    let store = ArchiveStore::open(dir.path()).unwrap();
    let full = store.save(capture()).unwrap();
    let mut tail = capture();
    tail.messages.remove(0);
    tail.captured_at = "2026-10-15T06:00:00Z".into();
    let latest = store.save(tail).unwrap();
    assert_eq!(
        store.read(&latest.id, None).unwrap().capture.messages.len(),
        1
    );
    assert_eq!(
        store
            .read(&full.id, Some(&full.snapshot_id))
            .unwrap()
            .capture
            .messages
            .len(),
        2
    );
}

#[test]
fn search_matches_titles_and_transcript_with_unicode_without_a_model() {
    let dir = tempdir().unwrap();
    let store = ArchiveStore::open(dir.path()).unwrap();
    store.save(capture()).unwrap();
    assert_eq!(store.list("CAFÉ").unwrap().len(), 1);
    assert_eq!(store.list("keputusan").unwrap().len(), 1);
    assert!(store.list("nonexistent phrase").unwrap().is_empty());
}

#[test]
fn inline_images_are_externalized_and_references_do_not_fetch_remote_media() {
    let dir = tempdir().unwrap();
    let store = ArchiveStore::open(dir.path()).unwrap();
    let data="data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aN3sAAAAASUVORK5CYII=";
    let mut c = capture();
    c.messages[0].attachments.push(Attachment {
        id: "img-1".into(),
        name: "diagram.png".into(),
        mime_type: "image/png".into(),
        source_url: Some("https://example.invalid/private.png".into()),
        data_url: Some(data.into()),
        asset_path: None,
        unavailable_reason: None,
    });
    let receipt = store.save(c).unwrap();
    let read = store.read(&receipt.id, None).unwrap();
    let a = &read.capture.messages[0].attachments[0];
    assert!(a.data_url.is_none());
    assert_eq!(
        a.source_url.as_deref(),
        Some("https://example.invalid/private.png")
    );
    let bytes = store
        .asset(&receipt.id, a.asset_path.as_deref().unwrap())
        .unwrap();
    assert_eq!(bytes.data_url, data);
    assert!(store.asset(&receipt.id, "../../config.json").is_err());
}

#[test]
fn invalid_owner_private_chat_duplicate_message_ids_and_oversize_are_rejected_without_writes() {
    let variants: Vec<(&str, Box<dyn Fn(&mut Capture)>)> = vec![
        (
            "different origin",
            Box::new(|c| {
                c.source_url = "https://evil.example/c/12345678-1234-4123-8123-123456789abc".into()
            }),
        ),
        (
            "different conversation",
            Box::new(|c| c.conversation_id = "different".into()),
        ),
        (
            "temporary",
            Box::new(|c| c.source_url.push_str("?temporary-chat=true")),
        ),
        (
            "duplicate id",
            Box::new(|c| c.messages[1].id = "u-1".into()),
        ),
        (
            "oversize text",
            Box::new(|c| c.messages[0].text = "x".repeat(4 * 1024 * 1024 + 1)),
        ),
        (
            "fake completeness",
            Box::new(|c| c.completeness = "complete".into()),
        ),
    ];
    for (name, change) in variants {
        let dir = tempdir().unwrap();
        let store = ArchiveStore::open(dir.path()).unwrap();
        let mut c = capture();
        change(&mut c);
        assert!(store.save(c).is_err(), "{name}");
        assert!(store.list("").unwrap().is_empty(), "{name} wrote archive");
    }
}

#[test]
fn concurrent_manual_saves_keep_a_single_durable_snapshot() {
    let dir = tempdir().unwrap();
    let store = Arc::new(ArchiveStore::open(dir.path()).unwrap());
    let workers: Vec<_> = (0..6)
        .map(|_| {
            let s = store.clone();
            thread::spawn(move || s.save(capture()).unwrap())
        })
        .collect();
    let receipts: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
    assert_eq!(receipts.iter().filter(|r| !r.unchanged).count(), 1);
    assert_eq!(
        store
            .read(&receipts[0].id, None)
            .unwrap()
            .summary
            .snapshot_count,
        1
    );
}

#[test]
fn archive_and_snapshot_path_traversal_is_rejected() {
    let dir = tempdir().unwrap();
    let store = ArchiveStore::open(dir.path()).unwrap();
    assert!(store.read("../escape", None).is_err());
    assert!(store.read(&"a".repeat(64), Some("../escape")).is_err());
}

#[test]
fn exporting_a_historical_capture_keeps_its_text_and_offline_image_files() {
    let dir = tempdir().unwrap();
    let store = ArchiveStore::open(dir.path()).unwrap();
    let mut first = capture();
    first.messages[0].attachments.push(Attachment{id:"img-export".into(),name:"offline.png".into(),mime_type:"image/png".into(),source_url:None,data_url:Some("data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aN3sAAAAASUVORK5CYII=".into()),asset_path:None,unavailable_reason:None});
    let receipt = store.save(first).unwrap();
    let mut second = capture();
    second.messages[1].text = "Newer answer".into();
    store.save(second).unwrap();
    let export = store
        .export_snapshot(&receipt.id, Some(&receipt.snapshot_id))
        .unwrap();
    assert!(export.markdown.contains("**Diputuskan**"));
    assert!(!export.markdown.contains("Newer answer"));
    let read = store.read(&receipt.id, Some(&receipt.snapshot_id)).unwrap();
    let asset = read.capture.messages[0].attachments[0]
        .asset_path
        .as_deref()
        .unwrap();
    let image = std::path::Path::new(&export.path)
        .parent()
        .unwrap()
        .join("assets")
        .join(asset);
    assert!(image.is_file());
    assert!(std::fs::read(image)
        .unwrap()
        .starts_with(b"\x89PNG\r\n\x1a\n"));
}

#[test]
fn opened_snapshot_identity_is_explicit_after_revisiting_identical_older_content() {
    let dir = tempdir().unwrap();
    let store = ArchiveStore::open(dir.path()).unwrap();
    let first = store.save(capture()).unwrap();
    let mut changed = capture();
    changed.captured_at = "2026-10-15T06:00:00Z".into();
    changed.messages[1].text = "Newer content".into();
    let newer = store.save(changed).unwrap();
    let mut original = capture();
    original.captured_at = "2026-10-15T07:00:00Z".into();
    store.save(original).unwrap();
    let opened = serde_json::to_value(store.read(&first.id, None).unwrap()).unwrap();
    assert_eq!(opened["snapshot_id"], first.snapshot_id);
    assert_eq!(
        store.snapshots(&first.id).unwrap()[0].snapshot_id,
        newer.snapshot_id
    );
}

#[test]
fn importing_older_history_retains_current_manual_capture_as_the_reader_default() {
    let dir = tempdir().unwrap();
    let store = ArchiveStore::open(dir.path()).unwrap();
    let current = store.save(capture()).unwrap();
    let mut old = capture();
    old.capture_method = "legacy-import".into();
    old.captured_at = "2026-09-01T01:00:00Z".into();
    old.messages[1].text = "Old legacy content".into();
    let imported = store.save(old).unwrap();
    assert_ne!(current.snapshot_id, imported.snapshot_id);
    let read = store.read(&current.id, None).unwrap();
    assert_eq!(
        read.capture.messages[1].text,
        "**Diputuskan**: Rust, dengan `JSONL`."
    );
    assert_eq!(read.summary.snapshot_count, 2);
    assert_eq!(
        store
            .read(&current.id, Some(&imported.snapshot_id))
            .unwrap()
            .capture
            .messages[1]
            .text,
        "Old legacy content"
    );
}
