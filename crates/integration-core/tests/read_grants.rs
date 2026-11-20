//! MP-7MCP-02 / MP-ADR-006: per-client, per-source read grants (default deny)
//! and the negative / revocation matrix for the credentialed read dispatcher.
//! Synthetic data only; this is not evidence about a real Claude client.
use mind_place_archive::{ArchiveStore, Capture, Message};
use mind_place_index::{IndexError, IndexStore};
use mind_place_integration::{
    ClientAuthority, IntegrationCore, IntegrationError, ToolBatch, ToolCall, ToolName,
};
use mind_place_security::SecurityGate;
use mind_place_state::{ClientCapability as Cap, StateError, StateStore};
use serde_json::{json, Value};

const CONV_A: &str = "11111111-2222-4333-8444-555555555555";
const CONV_B: &str = "22222222-3333-4444-8555-666666666666";
const TEXT_A: &str = "violet spacecraft alpha";
const TEXT_B: &str = "amber lighthouse bravo";

struct Evidence {
    source: String,
    archive: String,
    snapshot: String,
}

struct Fixture {
    _dir: tempfile::TempDir,
    archive: ArchiveStore,
    index: IndexStore,
    security: SecurityGate,
    state: StateStore,
}

fn capture(conversation: &str, message: &str, text: &str) -> Capture {
    Capture {
        schema_version: 1,
        provider: "chatgpt".into(),
        conversation_id: conversation.into(),
        source_url: format!("https://chatgpt.com/c/{conversation}"),
        title: format!("Grant fixture {message}"),
        conversation_created_at: None,
        conversation_updated_at: None,
        captured_at: "2026-10-18T10:00:00Z".into(),
        capture_method: "manual-dom".into(),
        completeness: "partial".into(),
        warnings: vec![],
        messages: vec![Message {
            id: message.into(),
            role: "user".into(),
            text: text.into(),
            format: "plain".into(),
            authored_at: None,
            model: None,
            attachments: vec![],
        }],
    }
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let archive = ArchiveStore::open(dir.path().join("archive-v1")).unwrap();
        let state = StateStore::open(dir.path().join("state.sqlite3")).unwrap();
        let index = IndexStore::open(dir.path().join("index.sqlite3")).unwrap();
        archive.save(capture(CONV_A, "mA", TEXT_A)).unwrap();
        archive.save(capture(CONV_B, "mB", TEXT_B)).unwrap();
        index.sync(&archive, &state).unwrap();
        let security = SecurityGate::open(dir.path().join("security.json")).unwrap();
        security.initialize(b"test-secret-123").unwrap();
        Self {
            _dir: dir,
            archive,
            index,
            security,
            state,
        }
    }
    fn owner(&self) -> ClientAuthority<'_> {
        ClientAuthority::new(&self.state, &self.security)
    }
    fn core(&self) -> IntegrationCore<'_> {
        IntegrationCore::new(&self.archive, &self.index, &self.security)
    }
    fn call(name: ToolName, arguments: Value) -> ToolBatch {
        ToolBatch {
            calls: vec![ToolCall { name, arguments }],
        }
    }
    /// Evidence ids for one conversation, read through the trusted unscoped
    /// internal path (never through a client credential).
    fn evidence(&self, text: &str) -> Evidence {
        let reply = self
            .core()
            .execute_batch(
                &Self::call(ToolName::MemorySearch, json!({"queries": [text]})),
                || Ok(()),
            )
            .unwrap();
        let hit = &reply.results[0].result["hits"][0];
        Evidence {
            source: hit["source_id"].as_str().unwrap().to_owned(),
            archive: hit["archive_id"].as_str().unwrap().to_owned(),
            snapshot: hit["snapshot_id"].as_str().unwrap().to_owned(),
        }
    }
    fn search(&self, credential: &str, text: &str) -> Result<Value, IntegrationError> {
        self.core()
            .execute_client_batch(
                &self.state,
                credential,
                &Self::call(ToolName::MemorySearch, json!({"queries": [text]})),
            )
            .map(|reply| reply.results[0].result.clone())
    }
    fn fetch(&self, credential: &str, e: &Evidence) -> Result<Value, IntegrationError> {
        self.core()
            .execute_client_batch(
                &self.state,
                credential,
                &Self::call(
                    ToolName::MemoryFetch,
                    json!({
                        "source_id": e.source, "archive_id": e.archive,
                        "snapshot_id": e.snapshot, "granularity": "revision"
                    }),
                ),
            )
            .map(|reply| reply.results[0].result.clone())
    }
    fn context(&self, credential: &str, text: &str) -> Result<Value, IntegrationError> {
        self.core()
            .execute_client_batch(
                &self.state,
                credential,
                &Self::call(ToolName::ContextBuild, json!({"queries": [text]})),
            )
            .map(|reply| reply.results[0].result.clone())
    }
}

fn hit_sources(result: &Value) -> Vec<String> {
    result["hits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["source_id"].as_str().unwrap().to_owned())
        .collect()
}

fn is_unknown_source_error(error: &IntegrationError) -> bool {
    matches!(error, IntegrationError::Index(IndexError::InvalidQuery))
}

#[test]
fn default_deny_reads_nothing_across_search_fetch_and_context() {
    let f = Fixture::new();
    let (a, b) = (f.evidence(TEXT_A), f.evidence(TEXT_B));
    let client = f.owner().issue("No grants", &[Cap::Read]).unwrap();
    let cred = client.credential();
    assert!(hit_sources(&f.search(cred, "violet").unwrap()).is_empty());
    assert!(hit_sources(&f.search(cred, "amber").unwrap()).is_empty());
    assert!(is_unknown_source_error(&f.fetch(cred, &a).unwrap_err()));
    assert!(is_unknown_source_error(&f.fetch(cred, &b).unwrap_err()));
    let context = f.context(cred, "violet spacecraft").unwrap().to_string();
    assert!(!context.contains(&a.source) && !context.contains("violet spacecraft alpha"));
}

#[test]
fn only_granted_sources_are_visible_and_denial_is_indistinguishable_from_unknown() {
    let f = Fixture::new();
    let (a, b) = (f.evidence(TEXT_A), f.evidence(TEXT_B));
    let client = f.owner().issue("Reader A", &[Cap::Read]).unwrap();
    let cred = client.credential();
    assert_eq!(
        f.owner()
            .grant_read_sources(&client.client.id, std::slice::from_ref(&a.source))
            .unwrap(),
        1
    );
    // Search and context see A only.
    assert_eq!(
        hit_sources(&f.search(cred, "violet").unwrap()),
        [a.source.clone()]
    );
    assert!(hit_sources(&f.search(cred, "amber").unwrap()).is_empty());
    let own_context = f.context(cred, "violet spacecraft").unwrap().to_string();
    assert!(own_context.contains(&a.source) && !own_context.contains(&b.source));
    let other_context = f.context(cred, "amber lighthouse").unwrap().to_string();
    assert!(!other_context.contains(&b.source) && !other_context.contains(TEXT_B));
    // Exact fetch: A works; B fails exactly like a source that does not exist.
    assert!(f.fetch(cred, &a).is_ok());
    let denied = f.fetch(cred, &b).unwrap_err();
    let unknown = f
        .fetch(
            cred,
            &Evidence {
                source: format!("src:v1:{}", "0".repeat(64)),
                archive: b.archive.clone(),
                snapshot: b.snapshot.clone(),
            },
        )
        .unwrap_err();
    assert!(is_unknown_source_error(&denied) && is_unknown_source_error(&unknown));
    assert_eq!(denied.code(), unknown.code());
    assert_eq!(denied.to_string(), unknown.to_string());
}

#[test]
fn request_filters_cannot_widen_the_grant() {
    let f = Fixture::new();
    let (a, b) = (f.evidence(TEXT_A), f.evidence(TEXT_B));
    let client = f.owner().issue("Reader A", &[Cap::Read]).unwrap();
    f.owner()
        .grant_read_sources(&client.client.id, std::slice::from_ref(&a.source))
        .unwrap();
    let both = f
        .core()
        .execute_client_batch(
            &f.state,
            client.credential(),
            &Fixture::call(
                ToolName::MemorySearch,
                json!({"queries": ["lighthouse", "spacecraft"],
                       "filters": {"source_ids": [a.source, b.source]}}),
            ),
        )
        .unwrap();
    assert_eq!(hit_sources(&both.results[0].result), [a.source.clone()]);
    let only_b = f
        .core()
        .execute_client_batch(
            &f.state,
            client.credential(),
            &Fixture::call(
                ToolName::MemorySearch,
                json!({"queries": ["lighthouse"], "filters": {"source_ids": [b.source]}}),
            ),
        )
        .unwrap();
    assert!(hit_sources(&only_b.results[0].result).is_empty());
}

#[test]
fn clients_are_isolated_from_each_other_and_other_credentials_do_not_inherit() {
    let f = Fixture::new();
    let (a, b) = (f.evidence(TEXT_A), f.evidence(TEXT_B));
    let one = f.owner().issue("One", &[Cap::Read]).unwrap();
    let two = f.owner().issue("Two", &[Cap::Read]).unwrap();
    f.owner()
        .grant_read_sources(&one.client.id, std::slice::from_ref(&a.source))
        .unwrap();
    f.owner()
        .grant_read_sources(&two.client.id, std::slice::from_ref(&b.source))
        .unwrap();
    assert_eq!(
        hit_sources(&f.search(one.credential(), "violet").unwrap()),
        [a.source.clone()]
    );
    assert!(hit_sources(&f.search(one.credential(), "amber").unwrap()).is_empty());
    assert_eq!(
        hit_sources(&f.search(two.credential(), "amber").unwrap()),
        [b.source.clone()]
    );
    assert!(hit_sources(&f.search(two.credential(), "violet").unwrap()).is_empty());
    // Client two cannot fetch client one's source by guessing its ids.
    assert!(is_unknown_source_error(
        &f.fetch(two.credential(), &a).unwrap_err()
    ));
    assert!(is_unknown_source_error(
        &f.fetch(one.credential(), &b).unwrap_err()
    ));
    // A freshly issued client never inherits anyone's grants.
    let three = f.owner().issue("Three", &[Cap::Read]).unwrap();
    assert!(hit_sources(&f.search(three.credential(), "violet").unwrap()).is_empty());
}

#[test]
fn grants_are_not_a_capability_and_legacy_bits_confer_no_scope() {
    let f = Fixture::new();
    let a = f.evidence(TEXT_A);
    // Granting sources to a client without the read capability reads nothing.
    let admin = f.owner().issue("Admin only", &[Cap::Admin]).unwrap();
    f.owner()
        .grant_read_sources(&admin.client.id, std::slice::from_ref(&a.source))
        .unwrap();
    assert!(matches!(
        f.search(admin.credential(), "violet"),
        Err(IntegrationError::AccessDenied)
    ));
    // Write/admin bits do not gain read through grants either.
    let writer = f.owner().issue("Writer", &[Cap::Write]).unwrap();
    assert!(matches!(
        f.search(writer.credential(), "violet"),
        Err(IntegrationError::AccessDenied)
    ));
    // Revoking the global read capability stops access even with grants kept.
    let reader = f.owner().issue("Reader", &[Cap::Read]).unwrap();
    f.owner()
        .grant_read_sources(&reader.client.id, std::slice::from_ref(&a.source))
        .unwrap();
    assert!(f.search(reader.credential(), "violet").is_ok());
    f.owner()
        .set_capability(&reader.client.id, Cap::Read, false)
        .unwrap();
    assert!(matches!(
        f.search(reader.credential(), "violet"),
        Err(IntegrationError::AccessDenied)
    ));
}

#[test]
fn revoke_applies_to_the_next_request_and_regrant_restores_it() {
    let f = Fixture::new();
    let a = f.evidence(TEXT_A);
    let client = f.owner().issue("Reader", &[Cap::Read]).unwrap();
    let cred = client.credential();
    f.owner()
        .grant_read_sources(&client.client.id, std::slice::from_ref(&a.source))
        .unwrap();
    assert!(!hit_sources(&f.search(cred, "violet").unwrap()).is_empty());
    assert_eq!(
        f.owner()
            .revoke_read_sources(&client.client.id, std::slice::from_ref(&a.source))
            .unwrap(),
        1
    );
    assert!(hit_sources(&f.search(cred, "violet").unwrap()).is_empty());
    assert!(is_unknown_source_error(&f.fetch(cred, &a).unwrap_err()));
    assert_eq!(
        f.owner()
            .grant_read_sources(&client.client.id, std::slice::from_ref(&a.source))
            .unwrap(),
        1
    );
    assert!(!hit_sources(&f.search(cred, "violet").unwrap()).is_empty());
    assert_eq!(
        f.owner()
            .revoke_all_read_sources(&client.client.id)
            .unwrap(),
        1
    );
    assert!(f
        .owner()
        .read_sources(&client.client.id)
        .unwrap()
        .is_empty());
    assert!(hit_sources(&f.search(cred, "violet").unwrap()).is_empty());
}

#[test]
fn grant_change_during_an_in_flight_request_fails_closed_without_evidence() {
    let f = Fixture::new();
    let a = f.evidence(TEXT_A);
    let batch = ToolBatch {
        calls: vec![
            ToolCall {
                name: ToolName::MemorySearch,
                arguments: json!({"queries": ["violet"]}),
            };
            2
        ],
    };
    for trigger in [2usize, 4, 6] {
        let client = f.owner().issue("In flight", &[Cap::Read]).unwrap();
        f.owner()
            .grant_read_sources(&client.client.id, std::slice::from_ref(&a.source))
            .unwrap();
        let mut seen = 0usize;
        let outcome = f.core().execute_client_batch_observed(
            &f.state,
            client.credential(),
            &batch,
            &mut || {
                seen += 1;
                if seen == trigger {
                    f.owner()
                        .revoke_read_sources(&client.client.id, std::slice::from_ref(&a.source))
                        .unwrap();
                }
            },
        );
        assert!(
            matches!(outcome, Err(IntegrationError::AccessDenied)),
            "revocation at recheck {trigger} must discard the whole batch"
        );
        assert!(seen >= trigger);
    }
    // Granting (a policy change too) during a request also fails closed.
    let other = f.evidence(TEXT_B);
    let client = f.owner().issue("In flight grant", &[Cap::Read]).unwrap();
    f.owner()
        .grant_read_sources(&client.client.id, std::slice::from_ref(&a.source))
        .unwrap();
    let mut seen = 0usize;
    let outcome =
        f.core()
            .execute_client_batch_observed(&f.state, client.credential(), &batch, &mut || {
                seen += 1;
                if seen == 3 {
                    f.owner()
                        .grant_read_sources(&client.client.id, std::slice::from_ref(&other.source))
                        .unwrap();
                }
            });
    assert!(matches!(outcome, Err(IntegrationError::AccessDenied)));
}

#[test]
fn rotation_keeps_grants_for_the_new_credential_and_kills_the_old_one() {
    let f = Fixture::new();
    let a = f.evidence(TEXT_A);
    let client = f.owner().issue("Rotating", &[Cap::Read]).unwrap();
    f.owner()
        .grant_read_sources(&client.client.id, std::slice::from_ref(&a.source))
        .unwrap();
    let old = client.credential().to_owned();
    let rotated = f.owner().rotate(&client.client.id).unwrap();
    assert!(matches!(
        f.search(&old, "violet"),
        Err(IntegrationError::AccessDenied)
    ));
    assert_eq!(
        hit_sources(&f.search(rotated.credential(), "violet").unwrap()),
        [a.source.clone()]
    );
    // Disable / terminal revoke stop reads regardless of grants.
    f.owner().set_enabled(&client.client.id, false).unwrap();
    assert!(matches!(
        f.search(rotated.credential(), "violet"),
        Err(IntegrationError::AccessDenied)
    ));
}

#[test]
fn locked_vault_discloses_nothing_even_with_grants() {
    let f = Fixture::new();
    let a = f.evidence(TEXT_A);
    let client = f.owner().issue("Reader", &[Cap::Read]).unwrap();
    f.owner()
        .grant_read_sources(&client.client.id, std::slice::from_ref(&a.source))
        .unwrap();
    f.security.lock();
    assert!(matches!(
        f.search(client.credential(), "violet"),
        Err(IntegrationError::Locked)
    ));
    assert!(matches!(
        f.owner()
            .grant_read_sources(&client.client.id, std::slice::from_ref(&a.source)),
        Err(IntegrationError::Locked)
    ));
}

#[test]
fn grant_validation_rejects_wildcards_malformed_and_unknown_targets() {
    let f = Fixture::new();
    let a = f.evidence(TEXT_A);
    let client = f.owner().issue("Reader", &[Cap::Read]).unwrap();
    let id = client.client.id.clone();
    for bad in [
        "*".to_owned(),
        "src:v1:*".to_owned(),
        "src:v1:".to_owned(),
        String::new(),
        format!("src:v1:{}", "G".repeat(64)),
        format!("src:v1:{}", "0".repeat(63)),
        format!("src:v2:{}", "0".repeat(64)),
        a.source.to_uppercase(),
    ] {
        assert!(
            matches!(
                f.owner().grant_read_sources(&id, &[bad]),
                Err(IntegrationError::Authority(StateError::InvalidValue))
            ),
            "malformed grant must be rejected"
        );
    }
    assert!(f.owner().grant_read_sources(&id, &[]).is_err());
    // Unknown client, and a terminally revoked client, cannot be granted.
    assert!(f
        .owner()
        .grant_read_sources("client:v1:unknown", std::slice::from_ref(&a.source))
        .is_err());
    f.owner().revoke(&id).unwrap();
    assert!(f
        .owner()
        .grant_read_sources(&id, std::slice::from_ref(&a.source))
        .is_err());
}

#[test]
fn grant_bookkeeping_is_idempotent_sorted_bounded_and_audited() {
    let f = Fixture::new();
    let (a, b) = (f.evidence(TEXT_A), f.evidence(TEXT_B));
    let client = f.owner().issue("Reader", &[Cap::Read]).unwrap();
    let id = client.client.id.clone();
    let before = f.state.client_policy_revision(&id).unwrap().unwrap();
    assert_eq!(
        f.owner()
            .grant_read_sources(&id, &[b.source.clone(), a.source.clone(), a.source.clone()])
            .unwrap(),
        2
    );
    let after = f.state.client_policy_revision(&id).unwrap().unwrap();
    assert!(after > before);
    // Re-granting changes nothing and does not move the policy revision.
    assert_eq!(
        f.owner()
            .grant_read_sources(&id, std::slice::from_ref(&a.source))
            .unwrap(),
        0
    );
    assert_eq!(f.state.client_policy_revision(&id).unwrap().unwrap(), after);
    let mut expected = vec![a.source.clone(), b.source.clone()];
    expected.sort();
    assert_eq!(f.owner().read_sources(&id).unwrap(), expected);
    // Grants never carry source ids into the audit trail.
    let audit = f.state.client_audit(&id, 10).unwrap();
    assert_eq!(audit[0].event, "capability_granted");
    assert_eq!(audit[0].capability.as_deref(), Some("read"));
    // Hard per-client bound.
    let limit = mind_place_state::MAX_READ_GRANTS_PER_CLIENT;
    let bulk: Vec<String> = (0..limit as u64 + 1)
        .map(|n| format!("src:v1:{n:064x}"))
        .collect();
    let bounded = f.owner().issue("Bulk", &[Cap::Read]).unwrap();
    for chunk in bulk[..limit].chunks(1000) {
        f.owner()
            .grant_read_sources(&bounded.client.id, chunk)
            .unwrap();
    }
    assert!(f
        .owner()
        .grant_read_sources(&bounded.client.id, &bulk[limit..])
        .is_err());
    assert_eq!(
        f.owner().read_sources(&bounded.client.id).unwrap().len(),
        limit
    );
}
