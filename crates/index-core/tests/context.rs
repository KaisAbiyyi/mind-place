use mind_place_archive::{ArchiveStore, Capture, Message};
use mind_place_index::{
    ContextBudget, ContextPreset, ContextRequest, IndexError, IndexStore, QueryTerm,
    RetrievalFilters,
};
use mind_place_state::StateStore;
use std::fs;

const A: &str = "11111111-2222-4333-8444-555555555555";
const B: &str = "22222222-3333-4444-8555-666666666666";
const C: &str = "33333333-4444-4555-8666-777777777777";

fn message(id: &str, text: &str) -> Message {
    Message {
        id: id.into(), role: "user".into(), text: text.into(),
        format: "plain".into(), authored_at: None, model: None, attachments: vec![],
    }
}
fn capture(id: &str, title: &str, texts: &[&str], captured_at: &str) -> Capture {
    Capture {
        schema_version: 1, provider: "chatgpt".into(),
        conversation_id: id.into(), source_url: format!("https://chatgpt.com/c/{id}"),
        title: title.into(), conversation_created_at: None,
        conversation_updated_at: None, captured_at: captured_at.into(),
        capture_method: "manual-dom".into(), completeness: "partial".into(),
        warnings: vec![],
        messages: texts.iter().enumerate().map(|(i, t)| message(&format!("msg-{i}"), t)).collect(),
    }
}
fn query(word: &str) -> ContextRequest {
    ContextRequest { queries: vec![QueryTerm::Text(word.into())], ..Default::default() }
}
struct Rig {
    _temp: tempfile::TempDir,
    archive: ArchiveStore, state: StateStore, index: IndexStore,
}
impl Rig {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let archive = ArchiveStore::open(temp.path().join("archive-v1")).unwrap();
        let state = StateStore::open(temp.path().join("state.sqlite3")).unwrap();
        let index = IndexStore::open(temp.path().join("index.sqlite3")).unwrap();
        Self { _temp: temp, archive, state, index }
    }
    fn add(&self, id: &str, title: &str, texts: &[&str], date: &str) -> mind_place_archive::SaveReceipt {
        let receipt = self.archive.save(capture(id, title, texts, date)).unwrap();
        self.index.sync(&self.archive, &self.state).unwrap();
        receipt
    }
    fn pack(&self, request: &ContextRequest) -> mind_place_index::ContextPack {
        self.index.build_context(&self.archive, request).unwrap()
    }
}

#[test]
fn balanced_windows_deduplicate_overlap_and_are_reproducible() {
    let rig = Rig::new();
    rig.add(A, "Notes", &["intro", "needle one", "needle two", "outro"], "2026-10-18T01:00:00Z");
    let pack = rig.pack(&query("needle"));
    assert_eq!(pack.groups.len(), 1);
    let group = &pack.groups[0];
    let ids: Vec<_> = group.excerpts.iter().map(|e| e.message_index.unwrap()).collect();
    assert_eq!(ids.len(), 4);
    assert_eq!(ids.iter().copied().collect::<std::collections::BTreeSet<_>>().len(), 4);
    assert!(group.excerpts.iter().any(|e| e.text == "intro"));
    assert!(group.excerpts.iter().any(|e| e.text == "outro"));
    assert!(group.excerpts.iter().all(|e| e.kind == "message"));
    assert!(group.source_id.starts_with("src:v1:"));
    assert_eq!(pack.source_count, 1);
    assert_eq!(pack.excerpt_count, 4);
    assert_eq!(rig.pack(&query("needle")), pack);
}

#[test]
fn presets_and_diversity_bound_dominant_conversations() {
    let rig = Rig::new();
    rig.add(A, "Dominant", &["shared one", "shared two", "shared three", "shared four"],
        "2026-10-18T01:00:00Z");
    rig.add(B, "Alternative", &["shared different", "background"],
        "2026-10-18T02:00:00Z");
    rig.add(C, "Third", &["shared other", "background"],
        "2026-10-18T03:00:00Z");
    let mut request = query("shared");
    request.preset = ContextPreset::Fast;
    request.max_sources = 3;
    request.max_per_source = 1;
    let pack = rig.pack(&request);
    assert_eq!(pack.groups.len(), 3);
    assert_eq!(pack.source_count, 3);
    assert!(pack.groups.iter().all(|g| g.excerpts.len() == 1));
    let mut limited = request;
    limited.max_sources = 1;
    let one = rig.pack(&limited);
    assert_eq!(one.source_count, 1);
    assert!(one.truncated);
    assert!(one.truncation_reasons.iter().any(|r| r == "source_limit"));

    let mut neighbors = query("shared");
    neighbors.preset = ContextPreset::SourceFirst;
    neighbors.max_sources = 2;
    let source_first = rig.pack(&neighbors);
    assert!(source_first.groups.iter().any(|g| g.excerpts.len() > 1));
    assert!(source_first.groups.iter().all(|g| g.excerpts.len() <= 4));
}

#[test]
fn historical_uses_capture_cutoff_and_challenge_keeps_historical_evidence() {
    let rig = Rig::new();
    let before = rig.add(A, "Project", &["needle original plan"],
        "2026-08-01T01:00:00Z");
    let after = rig.add(A, "Project", &["needle revised plan"],
        "2026-10-01T01:00:00Z");
    assert_ne!(before.snapshot_id, after.snapshot_id);
    let current = rig.pack(&query("needle"));
    assert!(current.groups.iter().all(|g| g.snapshot_id == after.snapshot_id));
    let mut historical = query("needle");
    historical.preset = ContextPreset::Historical;
    historical.as_of = Some("2026-09-01T00:00:00Z".into());
    let pack = rig.pack(&historical);
    assert_eq!(pack.groups.len(), 1);
    assert_eq!(pack.groups[0].snapshot_id, before.snapshot_id);
    assert!(pack.groups[0].excerpts.iter().any(|x| x.text.contains("original")));
    let mut challenge = query("needle");
    challenge.preset = ContextPreset::Challenge;
    let contrast = rig.pack(&challenge);
    assert!(contrast.groups.iter().any(|g| g.snapshot_id == before.snapshot_id));
    assert!(contrast.groups.iter().any(|g| g.snapshot_id == after.snapshot_id));
    assert_eq!(contrast.source_count, 1);
    assert!(contrast.groups.iter().any(|g| !g.indexed_is_current));
}

#[test]
fn tight_budget_bounds_unicode_and_emits_phase_2b_cursor() {
    let rig = Rig::new();
    let content = format!("needle {}", "🌌日本語".repeat(100));
    rig.add(A, "Evidence", &[&content], "2026-10-18T01:00:00Z");
    let mut request = query("needle");
    request.preset = ContextPreset::Fast;
    request.budget = ContextBudget {
        max_bytes: 37, max_estimated_tokens: 8, max_latency_ms: 30_000,
    };
    let pack = rig.pack(&request);
    assert_eq!(pack.groups.len(), 1);
    assert!(pack.truncated);
    assert!(pack.text_bytes <= 32);
    assert!(pack.estimated_tokens <= 8);
    assert_eq!(pack.excerpt_count, 1);
    assert!(pack.groups[0].next_cursor.is_some());
    assert_eq!(pack.groups[0].continuation_message_id.as_deref(), Some("msg-0"));
    let excerpt = &pack.groups[0].excerpts[0];
    assert!(content.starts_with(&excerpt.text));
    assert!(content.is_char_boundary(excerpt.byte_end));
    assert!(pack.truncation_reasons.iter().any(|r| r == "partial_fetch"));
}

#[test]
fn title_only_hit_is_verified_from_raw_and_not_discarded() {
    let rig = Rig::new();
    let saved = rig.add(A, "starlight-index title", &["some ordinary content"],
        "2026-10-18T01:00:00Z");
    let mut request = query("starlight");
    request.preset = ContextPreset::Fast;
    let pack = rig.pack(&request);
    assert_eq!(pack.groups.len(), 1);
    assert_eq!(pack.groups[0].snapshot_id, saved.snapshot_id);
    assert_eq!(pack.groups[0].excerpts[0].kind, "title");
    assert_eq!(pack.groups[0].excerpts[0].message_index, None);
    assert_eq!(pack.groups[0].excerpts[0].text, "starlight-index title");
}

#[test]
fn invalid_policy_and_unsupported_filters_fail_closed() {
    let rig = Rig::new();
    rig.add(A, "Notes", &["needle evidence"], "2026-10-18T01:00:00Z");
    let mut historical = query("needle");
    historical.preset = ContextPreset::Historical;
    assert!(matches!(rig.index.build_context(&rig.archive, &historical), Err(IndexError::InvalidQuery)));
    let mut normal = query("needle");
    normal.as_of = Some("2026-10-01T00:00:00Z".into());
    assert!(matches!(rig.index.build_context(&rig.archive, &normal), Err(IndexError::InvalidQuery)));
    normal.as_of = None;
    normal.filters.include_history = true;
    assert!(matches!(rig.index.build_context(&rig.archive, &normal), Err(IndexError::InvalidQuery)));
    let mut bad_budget = query("needle");
    bad_budget.budget.max_bytes = 1;
    assert!(matches!(rig.index.build_context(&rig.archive, &bad_budget), Err(IndexError::InvalidQuery)));
    let mut bad_limit = query("needle");
    bad_limit.max_sources = 0;
    assert!(matches!(rig.index.build_context(&rig.archive, &bad_limit), Err(IndexError::InvalidQuery)));
    assert!(serde_json::from_str::<ContextRequest>(
        r#"{"queries":["needle"],"filters":{"roots":["/private"]}}"#
    ).is_err());
}

#[test]
fn revocation_and_tampered_raw_fail_without_disclosing_partial_packs() {
    let rig = Rig::new();
    let saved = rig.add(A, "Notes", &["needle confidential data"], "2026-10-18T01:00:00Z");
    let mut checks = 0usize;
    let error = rig.index.build_context_with_check(&rig.archive, &query("needle"), || {
        checks += 1;
        if checks >= 3 { Err(IndexError::Revoked) } else { Ok(()) }
    });
    assert!(matches!(error, Err(IndexError::Revoked)));
    assert!(checks >= 3);
    let path = rig.archive.root().join("conversations").join(saved.id)
        .join("captures").join(format!("{}.jsonl", saved.snapshot_id));
    let old = fs::read_to_string(&path).unwrap();
    fs::write(&path, old.replace("confidential", "compromised")).unwrap();
    assert!(matches!(
        rig.index.build_context(&rig.archive, &query("needle")),
        Err(IndexError::InvalidEvidence)
    ));
}
