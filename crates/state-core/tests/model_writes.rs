//! MP-SEC-11: model-client writes are always `asserted_by=model`, never accepted,
//! never promoted, and cloud-tier writes are confined to a per-client quarantine.
use mind_place_state::{
    quarantine_scope_id, serialize_ranked_context, ClaimEvidence, ClientCapability as Cap,
    CredentialTier, MemoryClaim, MemoryWrite, ModelClaimProposal, RankingMode, RankingPolicy,
    SourceDescriptor, SourceKey, StateError, StateStore,
};
use serde_json::Value;

const VECTORS: &str =
    include_str!("../../../test/vectors/mp-sec-11-output-boundary-vectors.v1.json");
const FAR_FUTURE_MS: i64 = 4_102_444_800_000;
const MODEL: &str = "client:v1:model-a";
const OTHER: &str = "client:v1:model-b";

fn h(c: char) -> String {
    c.to_string().repeat(64)
}

struct Fixture {
    _dir: tempfile::TempDir,
    db: StateStore,
    source: String,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let db = StateStore::open(dir.path().join("state.sqlite3")).unwrap();
    let source = db
        .register_source(&SourceDescriptor {
            provider: "chatgpt",
            source_namespace: "private",
            source_kind: "conversation",
            key: SourceKey::External("source-1"),
        })
        .unwrap();
    // Model clients cite only sources they can read (MP-SEC-11 follow-up).
    db.issue_client(MODEL, "Model A", &h('a'), &[Cap::Write, Cap::Read])
        .unwrap();
    db.issue_client(OTHER, "Model B", &h('d'), &[Cap::Write, Cap::Read])
        .unwrap();
    db.grant_client_read_sources(MODEL, std::slice::from_ref(&source))
        .unwrap();
    db.grant_client_read_sources(OTHER, std::slice::from_ref(&source))
        .unwrap();
    db.issue_client("client:v1:reader", "Reader", &h('c'), &[Cap::Read])
        .unwrap();
    db.issue_client(
        "client:v1:owner",
        "Owner",
        &h('b'),
        &[Cap::Admin, Cap::Write],
    )
    .unwrap();
    db.create_private_scope(&h('b'), "scope:ordinary", "Ordinary", None)
        .unwrap();
    db.provision_quarantine_scope(&h('b'), MODEL).unwrap();
    db.provision_quarantine_scope(&h('b'), OTHER).unwrap();
    Fixture {
        _dir: dir,
        db,
        source,
    }
}

fn proposal(f: &Fixture, id: &str, scope: Option<&str>, origin: &str) -> ModelClaimProposal {
    ModelClaimProposal {
        id: id.into(),
        statement: format!("Model proposal {id}"),
        epistemic_origin: origin.into(),
        scope_id: scope.map(str::to_owned),
        evidence: vec![ClaimEvidence {
            source_ref: f.source.clone(),
            evidence_kind: "support".into(),
        }],
    }
}

fn vectors() -> Value {
    serde_json::from_str(VECTORS).unwrap()
}

fn scope_for(label: &str) -> Option<String> {
    match label {
        "none" => None,
        "own" => Some(quarantine_scope_id(MODEL).unwrap()),
        "other_client" => Some(quarantine_scope_id(OTHER).unwrap()),
        "ordinary" => Some("scope:ordinary".into()),
        other => panic!("unknown scope label {other}"),
    }
}

#[test]
fn shared_vectors_pin_tier_scope_and_origin_rules() {
    let v = vectors();
    for case in v["modelWrite"]["cases"].as_array().unwrap() {
        let f = fixture();
        let id = case["id"].as_str().unwrap();
        let tier = match case["tier"].as_str().unwrap() {
            "cloud" => CredentialTier::Cloud,
            "local" => CredentialTier::Local,
            other => panic!("tier {other}"),
        };
        let scope = scope_for(case["scope"].as_str().unwrap());
        let p = proposal(
            &f,
            "claim:vec",
            scope.as_deref(),
            case["origin"].as_str().unwrap(),
        );
        let result =
            f.db.apply_model_memory_write(&h('a'), "mpsec11-vector-key-0001", tier, &p);
        match case["expect"].as_str().unwrap() {
            "ok" => {
                result.unwrap_or_else(|e| panic!("{id}: expected ok, got {e:?}"));
                let state =
                    f.db.claim_as_of("claim:vec", FAR_FUTURE_MS)
                        .unwrap()
                        .unwrap();
                assert_eq!(state.asserted_by, "model", "{id}");
                assert_eq!(state.status, "uncertain", "{id}");
                assert_eq!(state.scope_id, scope, "{id}");
            }
            "denied" => {
                assert!(
                    matches!(result, Err(StateError::MemoryDenied)),
                    "{id}: {result:?}"
                );
                assert!(f.db.claim_history("claim:vec").unwrap().is_none(), "{id}");
            }
            "invalid" => {
                assert!(
                    matches!(result, Err(StateError::InvalidValue)),
                    "{id}: {result:?}"
                );
                assert!(f.db.claim_history("claim:vec").unwrap().is_none(), "{id}");
            }
            other => panic!("expect {other}"),
        }
    }
}

#[test]
fn forged_asserted_by_status_or_promotion_fields_are_rejected_at_parse() {
    let v = vectors();
    let ok: ModelClaimProposal = serde_json::from_value(serde_json::json!({
        "id": "claim:ok", "statement": "x", "evidence": []
    }))
    .unwrap();
    assert_eq!(ok.epistemic_origin, "inferred");
    for forged in v["modelWrite"]["forgedProposals"].as_array().unwrap() {
        let parsed = serde_json::from_value::<ModelClaimProposal>(forged["json"].clone());
        assert!(parsed.is_err(), "{} must not parse", forged["id"]);
    }
}

#[test]
fn model_claims_never_become_accepted_or_user_authored() {
    let f = fixture();
    let own = quarantine_scope_id(MODEL).unwrap();
    f.db.apply_model_memory_write(
        &h('a'),
        "mpsec11-never-accepted-1",
        CredentialTier::Local,
        &proposal(&f, "claim:local", None, "inferred"),
    )
    .unwrap();
    f.db.apply_model_memory_write(
        &h('a'),
        "mpsec11-never-accepted-2",
        CredentialTier::Cloud,
        &proposal(&f, "claim:cloud", Some(&own), "summarized"),
    )
    .unwrap();
    for id in ["claim:local", "claim:cloud"] {
        let state = f.db.claim_as_of(id, FAR_FUTURE_MS).unwrap().unwrap();
        assert_eq!(state.asserted_by, "model");
        assert_eq!(state.status, "uncertain");
        assert_ne!(state.status, "active");
        // Exactly one revision: no automatic transition ever follows a model write.
        assert_eq!(f.db.claim_history(id).unwrap().unwrap().len(), 1);
    }
}

#[test]
fn model_proposals_cannot_collide_with_or_alter_owner_claims() {
    let f = fixture();
    let owner_claim = MemoryClaim {
        id: "claim:owner".into(),
        statement: "Owner stated preference".into(),
        asserted_by: "user".into(),
        epistemic_origin: "explicit".into(),
        status: "active".into(),
    };
    let proof = vec![ClaimEvidence {
        source_ref: f.source.clone(),
        evidence_kind: "support".into(),
    }];
    f.db.apply_memory_write(
        &h('b'),
        "mpsec11-owner-create-01",
        &MemoryWrite::Create {
            claim: &owner_claim,
            scope_id: None,
            evidence: &proof,
        },
    )
    .unwrap();
    // The model-tier entry point has no supersede/forget operation at all. The
    // internal guard also denies those operations if a future caller tried.
    let before = f.db.claim_history("claim:owner").unwrap().unwrap().len();
    // A model proposal repeating the owner's statement is a duplicate: rejected.
    let mut dup = proposal(&f, "claim:dup", None, "inferred");
    dup.statement = "owner stated preference".into();
    assert!(matches!(
        f.db.apply_model_memory_write(
            &h('a'),
            "mpsec11-duplicate-0001",
            CredentialTier::Local,
            &dup
        ),
        Err(StateError::InvalidValue)
    ));
    assert_eq!(
        f.db.claim_history("claim:owner").unwrap().unwrap().len(),
        before
    );
    let state =
        f.db.claim_as_of("claim:owner", FAR_FUTURE_MS)
            .unwrap()
            .unwrap();
    assert_eq!(state.asserted_by, "user");
    assert_eq!(state.status, "active");
}

#[test]
fn capability_is_rechecked_and_reader_or_revoked_credentials_cannot_write() {
    let f = fixture();
    let p = proposal(&f, "claim:cap", None, "inferred");
    assert!(matches!(
        f.db.apply_model_memory_write(
            &h('c'),
            "mpsec11-reader-denied-1",
            CredentialTier::Local,
            &p
        ),
        Err(StateError::MemoryDenied)
    ));
    assert!(matches!(
        f.db.apply_model_memory_write(
            &h('e'),
            "mpsec11-unknown-token-1",
            CredentialTier::Local,
            &p
        ),
        Err(StateError::MemoryDenied)
    ));
    f.db.revoke_client(MODEL).unwrap();
    assert!(matches!(
        f.db.apply_model_memory_write(
            &h('a'),
            "mpsec11-revoked-denied1",
            CredentialTier::Local,
            &p
        ),
        Err(StateError::MemoryDenied)
    ));
    assert!(f.db.claim_history("claim:cap").unwrap().is_none());
}

#[test]
fn quarantine_is_owner_provisioned_and_per_client() {
    let f = fixture();
    // A client whose quarantine scope the owner has not provisioned cannot write.
    f.db.issue_client(
        "client:v1:fresh",
        "Fresh",
        &h('f'),
        &[Cap::Write, Cap::Read],
    )
    .unwrap();
    f.db.grant_client_read_sources("client:v1:fresh", std::slice::from_ref(&f.source))
        .unwrap();
    let scope = quarantine_scope_id("client:v1:fresh").unwrap();
    let p = proposal(&f, "claim:fresh", Some(&scope), "inferred");
    assert!(matches!(
        f.db.apply_model_memory_write(
            &h('f'),
            "mpsec11-unprovisioned-1",
            CredentialTier::Cloud,
            &p
        ),
        Err(StateError::MemoryDenied)
    ));
    // Only an admin credential can provision, and only for a known client.
    assert!(matches!(
        f.db.provision_quarantine_scope(&h('a'), "client:v1:fresh"),
        Err(StateError::MemoryDenied)
    ));
    assert!(matches!(
        f.db.provision_quarantine_scope(&h('b'), "client:v1:ghost"),
        Err(StateError::InvalidValue)
    ));
    assert!(matches!(
        f.db.provision_quarantine_scope(&h('b'), "not-a-client-id"),
        Err(StateError::InvalidValue)
    ));
    let created =
        f.db.provision_quarantine_scope(&h('b'), "client:v1:fresh")
            .unwrap();
    assert_eq!(created.id, scope);
    // Idempotent.
    assert_eq!(
        f.db.provision_quarantine_scope(&h('b'), "client:v1:fresh")
            .unwrap(),
        created
    );
    f.db.apply_model_memory_write(
        &h('f'),
        "mpsec11-unprovisioned-1",
        CredentialTier::Cloud,
        &p,
    )
    .unwrap();
    // Quarantine ids are distinct per client.
    assert_ne!(
        quarantine_scope_id(MODEL).unwrap(),
        quarantine_scope_id(OTHER).unwrap()
    );
}

#[test]
fn quarantined_claims_stay_out_of_ordinary_ranking_and_are_labelled_model_authored() {
    let f = fixture();
    let own = quarantine_scope_id(MODEL).unwrap();
    f.db.apply_model_memory_write(
        &h('a'),
        "mpsec11-ranking-hidden1",
        CredentialTier::Cloud,
        &proposal(&f, "claim:q", Some(&own), "inferred"),
    )
    .unwrap();
    f.db.apply_model_memory_write(
        &h('a'),
        "mpsec11-ranking-global1",
        CredentialTier::Local,
        &proposal(&f, "claim:g", None, "inferred"),
    )
    .unwrap();
    let policy = |scope: Option<&str>| RankingPolicy {
        mode: RankingMode::Challenge,
        as_of_ms: None,
        scope_id: scope.map(str::to_owned),
        limit: 20,
    };
    // Ordinary (non-challenge) ranking never surfaces an uncertain model claim
    // as a current belief, in any scope.
    let balanced = RankingPolicy {
        mode: RankingMode::Balanced,
        as_of_ms: None,
        scope_id: Some(own.clone()),
        limit: 20,
    };
    assert!(f
        .db
        .rank_claims(&["claim:q", "claim:g"], &balanced)
        .unwrap()
        .is_empty());
    // The default (unscoped) challenge view never includes quarantined candidates.
    let default_view =
        f.db.rank_claims(&["claim:q", "claim:g"], &policy(None))
            .unwrap();
    assert_eq!(default_view.len(), 1);
    assert_eq!(default_view[0].claim.id, "claim:g");
    // Only an explicit, owner-chosen scope reveals it, and it stays labelled.
    let owner_view = f.db.rank_claims(&["claim:q"], &policy(Some(&own))).unwrap();
    assert_eq!(owner_view.len(), 1);
    let text = serialize_ranked_context(&owner_view).unwrap();
    assert!(text.contains("MEMORY CANDIDATES (untrusted"));
    assert!(text.contains("asserted_by=model"));
    assert!(!text.contains("asserted_by=user"));
}

#[test]
fn injected_statement_cannot_forge_another_ranked_line() {
    let f = fixture();
    let v = vectors();
    let payload = v["injectionPayloads"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == "forged-claim-line")
        .unwrap()["text"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut p = proposal(&f, "claim:inject", None, "inferred");
    p.statement = payload;
    f.db.apply_model_memory_write(
        &h('a'),
        "mpsec11-inject-line-001",
        CredentialTier::Local,
        &p,
    )
    .unwrap();
    let ranked =
        f.db.rank_claims(
            &["claim:inject"],
            &RankingPolicy {
                mode: RankingMode::Challenge,
                as_of_ms: None,
                scope_id: None,
                limit: 5,
            },
        )
        .unwrap();
    let text = serialize_ranked_context(&ranked).unwrap();
    // One header plus exactly one candidate line: the statement is a JSON string,
    // so its newline is escaped and cannot start a forged owner line.
    assert_eq!(text.lines().count(), 2, "{text}");
    assert!(text.lines().nth(1).unwrap().contains("asserted_by=model"));
    assert!(!text.contains("\n- [EVIDENCE_LINKED_HYPOTHESIS; current_candidate; asserted_by=user"));
}

#[test]
fn replay_with_same_key_is_idempotent_and_different_body_conflicts() {
    let f = fixture();
    let p = proposal(&f, "claim:replay", None, "inferred");
    let first =
        f.db.apply_model_memory_write(&h('a'), "mpsec11-replay-key-001", CredentialTier::Local, &p)
            .unwrap();
    let second =
        f.db.apply_model_memory_write(&h('a'), "mpsec11-replay-key-001", CredentialTier::Local, &p)
            .unwrap();
    assert_eq!(first, second);
    let mut changed = p.clone();
    changed.statement = "A different body".into();
    assert!(matches!(
        f.db.apply_model_memory_write(
            &h('a'),
            "mpsec11-replay-key-001",
            CredentialTier::Local,
            &changed
        ),
        Err(StateError::InvalidValue)
    ));
}

// ---- MP-SEC-11 follow-up: evidence must be readable by the writing client ----

/// A second registered source that no model client has been granted.
fn register_ungranted(f: &Fixture) -> String {
    f.db.register_source(&SourceDescriptor {
        provider: "chatgpt",
        source_namespace: "private",
        source_kind: "conversation",
        key: SourceKey::External("source-ungranted"),
    })
    .unwrap()
}

/// Well-formed `src:v1:` id that is not in the source registry.
const UNREGISTERED: &str =
    "src:v1:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn citing(id: &str, sources: &[&str]) -> ModelClaimProposal {
    ModelClaimProposal {
        id: id.into(),
        statement: format!("Model proposal {id}"),
        epistemic_origin: "inferred".into(),
        scope_id: None,
        evidence: sources
            .iter()
            .map(|s| ClaimEvidence {
                source_ref: (*s).into(),
                evidence_kind: "support".into(),
            })
            .collect(),
    }
}

fn write(f: &Fixture, key: &str, p: &ModelClaimProposal) -> Result<(), StateError> {
    f.db.apply_model_memory_write(&h('a'), key, CredentialTier::Local, p)
        .map(|_| ())
}

fn assert_denied_and_absent(f: &Fixture, result: Result<(), StateError>, claim: &str) {
    assert!(
        matches!(result, Err(StateError::MemoryDenied)),
        "{claim}: {result:?}"
    );
    assert!(f.db.claim_history(claim).unwrap().is_none(), "{claim}");
}

#[test]
fn evidence_source_with_read_grant_is_accepted() {
    let f = fixture();
    write(
        &f,
        "mpsec11-ev-granted-0001",
        &citing("claim:ev-ok", &[&f.source]),
    )
    .unwrap();
    let state =
        f.db.claim_as_of("claim:ev-ok", FAR_FUTURE_MS)
            .unwrap()
            .unwrap();
    assert_eq!(state.asserted_by, "model");
    assert_eq!(state.status, "uncertain");
    // Cloud tier into its own quarantine follows the same rule.
    let own = quarantine_scope_id(MODEL).unwrap();
    f.db.apply_model_memory_write(
        &h('a'),
        "mpsec11-ev-granted-0002",
        CredentialTier::Cloud,
        &proposal(&f, "claim:ev-cloud", Some(&own), "inferred"),
    )
    .unwrap();
}

#[test]
fn evidence_source_without_read_grant_is_denied_opaquely() {
    let f = fixture();
    let ungranted = register_ungranted(&f);
    assert_denied_and_absent(
        &f,
        write(
            &f,
            "mpsec11-ev-nogrant-0001",
            &citing("claim:ev-no", &[&ungranted]),
        ),
        "claim:ev-no",
    );
    // One readable source does not launder an unreadable one, in either order.
    assert_denied_and_absent(
        &f,
        write(
            &f,
            "mpsec11-ev-nogrant-0002",
            &citing("claim:ev-mix1", &[&f.source, &ungranted]),
        ),
        "claim:ev-mix1",
    );
    assert_denied_and_absent(
        &f,
        write(
            &f,
            "mpsec11-ev-nogrant-0003",
            &citing("claim:ev-mix2", &[&ungranted, &f.source]),
        ),
        "claim:ev-mix2",
    );
    // A grant held by ANOTHER client confers nothing on this one.
    f.db.grant_client_read_sources(OTHER, std::slice::from_ref(&ungranted))
        .unwrap();
    assert_denied_and_absent(
        &f,
        write(
            &f,
            "mpsec11-ev-nogrant-0004",
            &citing("claim:ev-other", &[&ungranted]),
        ),
        "claim:ev-other",
    );
    // A per-source grant without the global read capability is not read authority.
    f.db.issue_client("client:v1:writeonly", "Write only", &h('9'), &[Cap::Write])
        .unwrap();
    f.db.grant_client_read_sources("client:v1:writeonly", std::slice::from_ref(&f.source))
        .unwrap();
    let r =
        f.db.apply_model_memory_write(
            &h('9'),
            "mpsec11-ev-nogrant-0005",
            CredentialTier::Local,
            &citing("claim:ev-wo", &[&f.source]),
        )
        .map(|_| ());
    assert_denied_and_absent(&f, r, "claim:ev-wo");
}

#[test]
fn unregistered_source_is_indistinguishable_from_ungranted_source() {
    let f = fixture();
    let ungranted = register_ungranted(&f);
    let a = write(
        &f,
        "mpsec11-ev-probe-00001",
        &citing("claim:probe-a", &[&ungranted]),
    );
    let b = write(
        &f,
        "mpsec11-ev-probe-00002",
        &citing("claim:probe-b", &[UNREGISTERED]),
    );
    // Same variant, same rendering: nothing reveals which id exists.
    assert_eq!(format!("{a:?}"), format!("{b:?}"));
    assert_eq!(
        a.as_ref().unwrap_err().to_string(),
        b.as_ref().unwrap_err().to_string()
    );
    assert_denied_and_absent(&f, a, "claim:probe-a");
    assert_denied_and_absent(&f, b, "claim:probe-b");
    // Even a grant the owner issued for an unregistered id does not turn the
    // denial into a registry answer.
    f.db.grant_client_read_sources(MODEL, &[UNREGISTERED.to_owned()])
        .unwrap();
    let c = write(
        &f,
        "mpsec11-ev-probe-00003",
        &citing("claim:probe-c", &[UNREGISTERED]),
    );
    assert_eq!(
        format!("{c:?}"),
        format!("{:?}", Err::<(), _>(StateError::MemoryDenied))
    );
    assert_denied_and_absent(&f, c, "claim:probe-c");
}

#[test]
fn revoked_read_grant_denies_new_writes_and_replays() {
    let f = fixture();
    let p = citing("claim:ev-rev", &[&f.source]);
    write(&f, "mpsec11-ev-revoke-00001", &p).unwrap();
    f.db.revoke_client_read_sources(MODEL, std::slice::from_ref(&f.source))
        .unwrap();
    assert_denied_and_absent(
        &f,
        write(
            &f,
            "mpsec11-ev-revoke-00002",
            &citing("claim:ev-rev2", &[&f.source]),
        ),
        "claim:ev-rev2",
    );
    // Replaying the earlier request is rechecked, not served from the receipt.
    assert!(matches!(
        write(&f, "mpsec11-ev-revoke-00001", &p),
        Err(StateError::MemoryDenied)
    ));
    // Revoking the global read capability has the same effect as the grant.
    f.db.grant_client_read_sources(MODEL, std::slice::from_ref(&f.source))
        .unwrap();
    write(
        &f,
        "mpsec11-ev-revoke-00003",
        &citing("claim:ev-rev3", &[&f.source]),
    )
    .unwrap();
    f.db.set_client_capability(MODEL, Cap::Read, false).unwrap();
    assert_denied_and_absent(
        &f,
        write(
            &f,
            "mpsec11-ev-revoke-00004",
            &citing("claim:ev-rev4", &[&f.source]),
        ),
        "claim:ev-rev4",
    );
}
