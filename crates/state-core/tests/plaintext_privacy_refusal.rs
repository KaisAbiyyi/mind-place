//! MP-9C review P1/P2: plaintext legacy state has no privacy disclosure fence,
//! so a tombstone committed there could race an in-flight legacy read. Source
//! redaction and every forget step therefore refuse plaintext state outright;
//! no tombstone can appear under a running legacy read.
use mind_place_state::{
    ClientCapability as Cap, SourceDescriptor, SourceKey, StateError, StateStore,
};

#[test]
fn plaintext_state_refuses_redaction_and_every_forget_step() {
    let home = tempfile::tempdir().unwrap();
    let db = StateStore::open(home.path().join("state.sqlite3")).unwrap();
    let admin = "a".repeat(64);
    db.issue_client("client:v1:owner", "Owner", &admin, &[Cap::Admin])
        .unwrap();
    let source = db
        .register_source(&SourceDescriptor {
            provider: "chatgpt",
            source_namespace: "synthetic",
            source_kind: "conversation",
            key: SourceKey::External("plaintext-source"),
        })
        .unwrap();
    assert!(matches!(
        db.redact_source(&admin, &source, "Owner request"),
        Err(StateError::VaultRequired)
    ));
    assert!(matches!(
        db.preview_source_forget(&admin, &source, "Owner request"),
        Err(StateError::VaultRequired)
    ));
    assert!(matches!(
        db.confirm_source_forget(&admin, &source, &"f".repeat(64), &"0".repeat(64)),
        Err(StateError::VaultRequired)
    ));
    assert!(matches!(
        db.source_forget_progress_as_owner(&source),
        Err(StateError::VaultRequired)
    ));
    assert_eq!(
        StateError::VaultRequired.code(),
        "MP_PRIVACY_REQUIRES_VAULT"
    );
    assert!(!db.source_is_redacted(&source).unwrap());
    assert!(!db.has_source_redactions().unwrap());
    // Legacy reads/registration of the same source keep working.
    assert!(db.source(&source).unwrap().is_some());
}
