use mind_place_state::{
    ClaimEvidence, ClaimTransition, MemoryClaim, SourceDescriptor, SourceKey, StateError,
    StateStore,
};
use rusqlite::Connection;

fn fixture() -> (tempfile::TempDir, StateStore, String, String) {
    let directory = tempfile::tempdir().unwrap();
    let store = StateStore::open(directory.path().join("state.sqlite3")).unwrap();
    let source1 = store
        .register_source(&SourceDescriptor {
            provider: "chatgpt",
            source_namespace: "account-1",
            source_kind: "conversation",
            key: SourceKey::External("conversation-1"),
        })
        .unwrap();
    let source2 = store
        .register_source(&SourceDescriptor {
            provider: "chatgpt",
            source_namespace: "account-1",
            source_kind: "conversation",
            key: SourceKey::External("conversation-2"),
        })
        .unwrap();
    (directory, store, source1, source2)
}
fn claim(id: &str) -> MemoryClaim {
    MemoryClaim {
        id: id.into(),
        statement: "Lives in Bandung".into(),
        asserted_by: "user".into(),
        epistemic_origin: "explicit".into(),
        status: "active".into(),
    }
}
fn evidence(source: &str, kind: &str) -> Vec<ClaimEvidence> {
    vec![ClaimEvidence {
        source_ref: source.into(),
        evidence_kind: kind.into(),
    }]
}

#[test]
fn event_history_and_as_of_are_durable_and_truthful() {
    let (_dir, state, s1, s2) = fixture();
    state
        .create_evidenced_claim(&claim("claim:a"), &evidence(&s1, "support"))
        .unwrap();
    let created = state.claim_history("claim:a").unwrap().unwrap();
    assert_eq!(created.len(), 1);
    assert_eq!(created[0].revision, 1);
    assert_eq!(
        state
            .claim_as_of("claim:a", created[0].recorded_at_ms - 1)
            .unwrap(),
        None
    );
    assert_eq!(
        state
            .claim_as_of("claim:a", created[0].recorded_at_ms)
            .unwrap()
            .unwrap()
            .status,
        "active"
    );
    let disputed = state
        .transition_claim(
            "claim:a",
            ClaimTransition::Dispute,
            "Conflicting direct observation",
            &evidence(&s2, "contradiction"),
            None,
        )
        .unwrap();
    assert_eq!(disputed.revision, 2);
    assert!(disputed.recorded_at_ms > created[0].recorded_at_ms);
    let uncertain = state
        .transition_claim(
            "claim:a",
            ClaimTransition::MarkUncertain,
            "Cannot resolve yet",
            &evidence(&s2, "observation"),
            None,
        )
        .unwrap();
    assert_eq!(uncertain.revision, 3);
    let historical = state
        .claim_as_of("claim:a", disputed.recorded_at_ms)
        .unwrap()
        .unwrap();
    assert_eq!(historical.status, "disputed");
    assert_eq!(historical.revision, 2);
    assert_eq!(historical.evidence.len(), 2);
    assert_eq!(
        state
            .claim_as_of("claim:a", uncertain.recorded_at_ms)
            .unwrap()
            .unwrap()
            .status,
        "uncertain"
    );
    let reopened = StateStore::open(state.path()).unwrap();
    let history = reopened.claim_history("claim:a").unwrap().unwrap();
    assert_eq!(history.len(), 3);
    assert_eq!(
        history[1].reason.as_deref(),
        Some("Conflicting direct observation")
    );
    assert_eq!(
        reopened
            .claim_as_of("claim:a", i64::MAX)
            .unwrap()
            .unwrap()
            .status,
        "uncertain"
    );
    assert_eq!(
        reopened.claim_as_of("does:not:exist", i64::MAX).unwrap(),
        None
    );
}

#[test]
fn supersession_and_retraction_are_terminal_and_require_known_successor() {
    let (_dir, state, s1, _) = fixture();
    state
        .create_evidenced_claim(&claim("claim:a"), &evidence(&s1, "support"))
        .unwrap();
    assert!(matches!(
        state.transition_claim(
            "claim:a",
            ClaimTransition::Supersede,
            "New data",
            &evidence(&s1, "correction"),
            Some("claim:missing")
        ),
        Err(StateError::InvalidValue)
    ));
    state
        .create_evidenced_claim(&claim("claim:b"), &evidence(&s1, "support"))
        .unwrap();
    let changed = state
        .transition_claim(
            "claim:a",
            ClaimTransition::Supersede,
            "Old address was replaced",
            &evidence(&s1, "correction"),
            Some("claim:b"),
        )
        .unwrap();
    assert_eq!(changed.superseded_by.as_deref(), Some("claim:b"));
    assert!(matches!(
        state.transition_claim(
            "claim:a",
            ClaimTransition::Reinstate,
            "Incorrect restore",
            &evidence(&s1, "support"),
            None
        ),
        Err(StateError::InvalidValue)
    ));
    state
        .transition_claim(
            "claim:b",
            ClaimTransition::Retract,
            "Owner retracts this assertion",
            &evidence(&s1, "retraction"),
            None,
        )
        .unwrap();
    assert_eq!(
        state
            .claim_as_of("claim:b", i64::MAX)
            .unwrap()
            .unwrap()
            .status,
        "retracted"
    );
    assert!(matches!(
        state.transition_claim(
            "claim:b",
            ClaimTransition::Dispute,
            "Cannot change",
            &evidence(&s1, "contradiction"),
            None
        ),
        Err(StateError::InvalidValue)
    ));
}

#[test]
fn missing_or_forged_evidence_and_duplicate_revisions_fail_atomically() {
    let (_dir, state, s1, _) = fixture();
    assert!(matches!(
        state.create_evidenced_claim(
            &claim("claim:bad"),
            &evidence(
                "src:v1:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
                "support"
            )
        ),
        Err(StateError::InvalidValue)
    ));
    assert!(state.claim_history("claim:bad").unwrap().is_none());
    assert!(matches!(
        state.create_evidenced_claim(&claim("claim:none"), &[]),
        Err(StateError::InvalidValue)
    ));
    assert!(state.claim_history("claim:none").unwrap().is_none());
    state
        .create_evidenced_claim(&claim("claim:ok"), &evidence(&s1, "support"))
        .unwrap();
    let invalid = vec![
        evidence(&s1, "support")[0].clone(),
        evidence(&s1, "support")[0].clone(),
    ];
    assert!(matches!(
        state.transition_claim(
            "claim:ok",
            ClaimTransition::Dispute,
            "duplicate",
            &invalid,
            None
        ),
        Err(StateError::InvalidValue)
    ));
    assert_eq!(state.claim_history("claim:ok").unwrap().unwrap().len(), 1);
    let raw = Connection::open(state.path()).unwrap();
    raw.pragma_update(None, "foreign_keys", "ON").unwrap();
    assert!(raw
        .execute(
            "DELETE FROM memory_claim_events WHERE claim_id='claim:ok'",
            []
        )
        .is_err());
    assert!(raw.execute("UPDATE memory_claim_event_evidence SET evidence_kind='correction' WHERE claim_id='claim:ok'",[]).is_err());
    assert!(raw
        .execute(
            "UPDATE memory_claims SET statement='forged' WHERE id='claim:ok'",
            []
        )
        .is_err());
    assert!(raw
        .execute("DELETE FROM source_identities WHERE id=?1", [&s1])
        .is_err());
}

#[test]
fn old_v1_created_events_keep_as_of_without_invented_revision_evidence() {
    let (_dir, state, s1, _) = fixture();
    state.create_memory_claim(&claim("claim:legacy")).unwrap();
    state
        .add_claim_source_evidence("claim:legacy", &s1, "support")
        .unwrap();
    let old = state.claim_history("claim:legacy").unwrap().unwrap();
    assert_eq!(old.len(), 1);
    assert!(old[0].evidence.is_empty()); // old evidence did not record an evidence timestamp
    assert!(state
        .claim_as_of("claim:legacy", i64::MAX)
        .unwrap()
        .unwrap()
        .evidence
        .is_empty());
    let r = state
        .transition_claim(
            "claim:legacy",
            ClaimTransition::Dispute,
            "New contradictory evidence",
            &evidence(&s1, "contradiction"),
            None,
        )
        .unwrap();
    assert_eq!(r.revision, 2);
    assert_eq!(
        state
            .claim_as_of("claim:legacy", i64::MAX)
            .unwrap()
            .unwrap()
            .evidence
            .len(),
        1
    );
}
