use mind_place_archive::{ArchiveStore, Capture, Message};
use mind_place_index::{
    FetchBudget, FetchCursor, FetchGranularity, FetchRequest, IndexError, IndexStore,
};
use mind_place_state::StateStore;
use std::fs;

const SOURCE: &str = "11111111-2222-4333-8444-555555555555";

fn capture(first: &str, second: &str, third: &str) -> Capture {
    Capture {
        schema_version: 1, provider: "chatgpt".into(),
        conversation_id: SOURCE.into(),
        source_url: format!("https://chatgpt.com/c/{SOURCE}"),
        title: "Project evidence".into(),
        conversation_created_at: None, conversation_updated_at: None,
        captured_at: "2026-10-18T09:00:00Z".into(),
        capture_method: "manual-dom".into(), completeness: "partial".into(), warnings: vec![],
        messages: vec![
            message("one", "user", first),
            message("two", "assistant", second),
            message("three", "user", third),
        ],
    }
}
fn message(id: &str, role: &str, text: &str) -> Message {
    Message {
        id: id.into(), role: role.into(), text: text.into(), format: "plain".into(),
        authored_at: None, model: None, attachments: vec![],
    }
}
struct Rig {
    _dir: tempfile::TempDir,
    archive: ArchiveStore,
    state: StateStore,
    index: IndexStore,
}
impl Rig {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let archive = ArchiveStore::open(dir.path().join("archive-v1")).unwrap();
        let state = StateStore::open(dir.path().join("state.sqlite3")).unwrap();
        let index = IndexStore::open(dir.path().join("index.sqlite3")).unwrap();
        Self { _dir: dir, archive, state, index }
    }
    fn save(&self, capture: Capture) -> FetchRequest {
        let saved = self.archive.save(capture).unwrap();
        self.index.sync(&self.archive, &self.state).unwrap();
        let hit = self.index.search("needle", true, 10).unwrap()
            .into_iter().find(|hit| hit.snapshot_id == saved.snapshot_id).unwrap();
        FetchRequest {
            source_id: hit.source_id,
            archive_id: saved.id,
            snapshot_id: saved.snapshot_id,
            message_id: Some("two".into()),
            ..Default::default()
        }
    }
    fn fetch(&self, request: &FetchRequest) -> mind_place_index::FetchResponse {
        self.index.fetch(&self.archive, request).unwrap()
    }
}

#[test]
fn message_window_and_revision_use_exact_immutable_provenance() {
    let rig = Rig::new();
    let base = rig.save(capture("Before", "needle precise answer", "After"));
    let one = rig.fetch(&base);
    assert_eq!(one.segments.len(), 1);
    assert_eq!(one.segments[0].text, "needle precise answer");
    assert_eq!(one.segments[0].message_index, 1);
    assert_eq!(one.segments[0].message_id, "two");
    assert_eq!(one.snapshot_id, base.snapshot_id);
    assert_eq!(one.selected_start, 1);
    assert_eq!(one.selected_end_exclusive, 2);
    assert!(one.indexed_is_current);
    assert!(!one.truncated);

    let window = rig.fetch(&FetchRequest {
        granularity: FetchGranularity::Window, before: 1, after: 1,
        ..base.clone()
    });
    assert_eq!(window.segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(),
        vec!["Before", "needle precise answer", "After"]);
    assert_eq!(window.selected_start, 0);
    assert_eq!(window.selected_end_exclusive, 3);

    let revision = rig.fetch(&FetchRequest {
        granularity: FetchGranularity::Revision, message_id: None, ..base
    });
    assert_eq!(revision.segments.len(), 3);
    assert_eq!(revision.text_bytes, "Before".len() + "needle precise answer".len() + "After".len());
    assert_eq!(revision.estimated_tokens, revision.text_bytes.div_ceil(4));
    assert_eq!(revision.completeness, "partial");
}

#[test]
fn pagination_never_drops_or_duplicates_unicode_on_byte_and_token_limits() {
    let rig = Rig::new();
    let content = format!("{}{}", "cat".repeat(33), "🌌日本語".repeat(37));
    let mut request = rig.save(capture("before", &format!("needle {content}"), "after"));
    request.budget = FetchBudget { max_bytes: 37, max_estimated_tokens: 7, max_latency_ms: 30_000 };
    let mut out = String::new();
    let mut attempts = 0;
    loop {
        let page = rig.fetch(&request);
        assert!(page.text_bytes <= 28);
        assert!(page.estimated_tokens <= 7);
        for s in &page.segments {
            assert_eq!(s.message_index, 1);
            assert_eq!(s.byte_start, out.len());
            out.push_str(&s.text);
            assert_eq!(s.byte_end, out.len());
        }
        attempts += 1;
        if !page.truncated {
            assert!(page.next_cursor.is_none());
            break;
        }
        assert!(page.truncation_reason.is_some());
        request.cursor = page.next_cursor;
        assert!(attempts < 200);
    }
    assert_eq!(out, format!("needle {content}"));
    assert!(attempts > 5);
}

#[test]
fn zero_length_messages_and_segment_limit_advance_without_loops() {
    let rig = Rig::new();
    let mut c = capture("needle", "", "followup");
    c.messages = (0..120).map(|i| message(&format!("m{i}"), "user", "")).collect();
    c.messages[0].text = "needle".into();
    let mut request = rig.save(c);
    request.granularity = FetchGranularity::Revision;
    request.message_id = None;
    let first = rig.fetch(&request);
    assert_eq!(first.segments.len(), 100);
    assert_eq!(first.next_cursor, Some(FetchCursor { message_index: 100, byte_offset: 0 }));
    assert!(first.truncated);
    assert_eq!(first.truncation_reason.as_deref(), Some("segment_limit"));
    request.cursor = first.next_cursor;
    let final_page = rig.fetch(&request);
    assert_eq!(final_page.segments.len(), 20);
    assert!(!final_page.truncated);
}

#[test]
fn pinned_history_survives_new_current_revision_without_content_substitution() {
    let rig = Rig::new();
    let old = rig.save(capture("previous", "needle old decision", "context"));
    let new = rig.save(capture("current", "needle new decision", "context"));
    let old_result = rig.fetch(&old);
    let new_result = rig.fetch(&new);
    assert_eq!(old_result.segments[0].text, "needle old decision");
    assert_eq!(new_result.segments[0].text, "needle new decision");
    assert_eq!(old_result.source_id, new_result.source_id);
    assert_ne!(old_result.snapshot_id, new_result.snapshot_id);
    assert!(!old_result.indexed_is_current);
    assert!(new_result.indexed_is_current);
}

#[test]
fn invalid_handles_budgets_cursors_and_revocation_fail_closed() {
    let rig = Rig::new();
    let base = rig.save(capture("prior", "needle secure source", "following"));
    let cases = [
        FetchRequest { source_id: "src:v1:".into(), ..base.clone() },
        FetchRequest { archive_id: "a".repeat(64), ..base.clone() },
        FetchRequest { snapshot_id: "f".repeat(64), ..base.clone() },
        FetchRequest { message_id: Some("missing".into()), ..base.clone() },
        FetchRequest { message_id: None, ..base.clone() },
        FetchRequest { before: 1, ..base.clone() },
        FetchRequest { granularity: FetchGranularity::Revision, message_id: Some("two".into()), ..base.clone() },
        FetchRequest { granularity: FetchGranularity::Window, before: 21, ..base.clone() },
        FetchRequest { budget: FetchBudget { max_bytes: 3, ..Default::default() }, ..base.clone() },
        FetchRequest { budget: FetchBudget { max_estimated_tokens: 0, ..Default::default() }, ..base.clone() },
        FetchRequest { budget: FetchBudget { max_latency_ms: 0, ..Default::default() }, ..base.clone() },
        FetchRequest { cursor: Some(FetchCursor { message_index: 0, byte_offset: 0 }), ..base.clone() },
        FetchRequest { cursor: Some(FetchCursor { message_index: 1, byte_offset: 9999 }), ..base.clone() },
        FetchRequest { cursor: Some(FetchCursor { message_index: 2, byte_offset: 1 }), ..base.clone() },
    ];
    for request in cases {
        assert!(matches!(rig.index.fetch(&rig.archive, &request), Err(IndexError::InvalidQuery)));
    }
    let mut checked = false;
    let revoked = rig.index.fetch_with_check(&rig.archive, &base, || {
        checked = true;
        Err(IndexError::Revoked)
    });
    assert!(checked);
    assert!(matches!(revoked, Err(IndexError::Revoked)));
    assert!(serde_json::from_str::<FetchRequest>(
        r#"{"source_id":"x","archive_id":"y","snapshot_id":"z","folder_root":"private"}"#,
    ).is_err());
}

#[test]
fn tampered_raw_revision_is_rejected_even_with_valid_index_provenance() {
    let rig = Rig::new();
    let request = rig.save(capture("prior", "needle immutable evidence", "context"));
    let path = rig.archive.root().join("conversations").join(&request.archive_id)
        .join("captures").join(format!("{}.jsonl", request.snapshot_id));
    let old = fs::read_to_string(&path).unwrap();
    fs::write(&path, old.replace("immutable evidence", "modified  evidence")).unwrap();
    assert!(matches!(
        rig.index.fetch(&rig.archive, &request),
        Err(IndexError::InvalidEvidence)
    ));
}
