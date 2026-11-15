use mind_place_archive::{ArchiveStore, Capture, Message};
use mind_place_index::IndexStore;
use mind_place_integration::{
    ClientAuthority, IntegrationCore, IntegrationError, ToolBatch, ToolCall, ToolName,
};
use mind_place_security::SecurityGate;
use mind_place_state::{ClientCapability as Cap, StateStore};
use serde_json::json;
use sha2::Digest;

const ID: &str = "11111111-2222-4333-8444-555555555555";
struct Fixture {
    _dir: tempfile::TempDir,
    archive: ArchiveStore,
    index: IndexStore,
    security: SecurityGate,
    state: StateStore,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let archive = ArchiveStore::open(dir.path().join("archive-v1")).unwrap();
        let state = StateStore::open(dir.path().join("state.sqlite3")).unwrap();
        let index = IndexStore::open(dir.path().join("index.sqlite3")).unwrap();
        archive
            .save(Capture {
                schema_version: 1,
                provider: "chatgpt".into(),
                conversation_id: ID.into(),
                source_url: format!("https://chatgpt.com/c/{ID}"),
                title: "Authorization fixture".into(),
                conversation_created_at: None,
                conversation_updated_at: None,
                captured_at: "2026-10-18T10:00:00Z".into(),
                capture_method: "manual-dom".into(),
                completeness: "partial".into(),
                warnings: vec![],
                messages: vec![Message {
                    id: "msg-1".into(),
                    role: "user".into(),
                    text: "violet spacecraft authorization".into(),
                    format: "plain".into(),
                    authored_at: None,
                    model: None,
                    attachments: vec![],
                }],
            })
            .unwrap();
        index.sync(&archive, &state).unwrap();
        let security = SecurityGate::open(dir.path().join("security.json")).unwrap();
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
    fn unlock(&self) {
        self.security.initialize(b"test-secret-123").unwrap();
    }
    fn batch() -> ToolBatch {
        ToolBatch {
            calls: vec![ToolCall {
                name: ToolName::MemorySearch,
                arguments: json!({"queries": ["violet spacecraft"]}),
            }],
        }
    }
}

#[test]
fn locked_owner_cannot_issue_or_read_and_tokens_are_never_stored_plaintext() {
    let f = Fixture::new();
    assert!(matches!(
        f.owner().issue("Codex", &[Cap::Read]),
        Err(IntegrationError::Locked)
    ));
    f.unlock();
    let client = f.owner().issue("Codex", &[Cap::Read]).unwrap();
    assert!(client.credential().starts_with("mpc_v1_"));
    let raw = std::fs::read(f.state.path()).unwrap();
    assert!(!raw
        .windows(client.credential().len())
        .any(|w| w == client.credential().as_bytes()));
    assert_eq!(
        f.owner().authorize(client.credential(), Cap::Read).unwrap(),
        client.client.id
    );
    f.security.lock();
    assert!(matches!(
        f.core()
            .execute_client_batch(&f.state, client.credential(), &Fixture::batch()),
        Err(IntegrationError::Locked)
    ));
}

#[test]
fn read_is_not_write_or_admin_and_admin_is_not_implicit_read() {
    let f = Fixture::new();
    f.unlock();
    let reader = f.owner().issue("ChatGPT", &[Cap::Read]).unwrap();
    assert!(matches!(
        f.owner().authorize(reader.credential(), Cap::Admin),
        Err(IntegrationError::AccessDenied)
    ));
    assert!(matches!(
        f.owner().authorize(reader.credential(), Cap::Write),
        Err(IntegrationError::AccessDenied)
    ));
    let admin = f
        .owner()
        .issue("Trusted maintenance", &[Cap::Admin])
        .unwrap();
    assert!(matches!(
        f.core()
            .execute_client_batch(&f.state, admin.credential(), &Fixture::batch()),
        Err(IntegrationError::AccessDenied)
    ));
    let result = f
        .core()
        .execute_client_batch(&f.state, reader.credential(), &Fixture::batch())
        .unwrap();
    assert_eq!(result.results[0].result["hits"][0]["message_id"], "msg-1");
    let audit = f.state.client_audit(&reader.client.id, 10).unwrap();
    assert_eq!(audit[0].event, "read_completed");
    assert_eq!(audit[1].event, "read_attempt");
    assert_eq!(audit[2].event, "issued");
}

#[test]
fn immediate_capability_revoke_disable_rotation_and_terminal_revoke() {
    let f = Fixture::new();
    f.unlock();
    let client = f
        .owner()
        .issue("One brain", &[Cap::Read, Cap::Write])
        .unwrap();
    let id = &client.client.id;
    assert!(f
        .core()
        .execute_client_batch(&f.state, client.credential(), &Fixture::batch())
        .is_ok());
    f.owner().set_capability(id, Cap::Read, false).unwrap();
    assert!(matches!(
        f.core()
            .execute_client_batch(&f.state, client.credential(), &Fixture::batch()),
        Err(IntegrationError::AccessDenied)
    ));
    assert!(f.owner().authorize(client.credential(), Cap::Write).is_ok());
    f.owner().set_capability(id, Cap::Read, true).unwrap();
    f.owner().set_enabled(id, false).unwrap();
    assert!(matches!(
        f.owner().authorize(client.credential(), Cap::Read),
        Err(IntegrationError::AccessDenied)
    ));
    f.owner().set_enabled(id, true).unwrap();
    let rotated = f.owner().rotate(id).unwrap();
    assert!(matches!(
        f.owner().authorize(client.credential(), Cap::Read),
        Err(IntegrationError::AccessDenied)
    ));
    assert!(f.owner().authorize(rotated.credential(), Cap::Read).is_ok());
    f.owner().revoke(id).unwrap();
    assert!(matches!(
        f.core()
            .execute_client_batch(&f.state, rotated.credential(), &Fixture::batch()),
        Err(IntegrationError::AccessDenied)
    ));
    assert!(f.owner().set_enabled(id, true).is_err());
    assert!(f.owner().rotate(id).is_err());
    let reopened = StateStore::open(f.state.path()).unwrap();
    assert_eq!(reopened.client(id).unwrap().unwrap().state, "revoked");
    assert_eq!(
        reopened
            .authorize_client_hash(
                &format!(
                    "{:x}",
                    sha2::Sha256::digest(rotated.credential().as_bytes())
                ),
                Cap::Read
            )
            .unwrap(),
        None
    );
}

#[test]
fn mid_batch_capability_revoke_discards_all_evidence() {
    let f = Fixture::new();
    f.unlock();
    let client = f.owner().issue("Test agent", &[Cap::Read]).unwrap();
    let mut reads = 0;
    let result = f.core().execute_batch(
        &ToolBatch {
            calls: vec![Fixture::batch().calls[0].clone(); 2],
        },
        || {
            reads += 1;
            if reads == 4 {
                f.owner().revoke(&client.client.id).unwrap();
            }
            f.owner()
                .authorize(client.credential(), Cap::Read)
                .map(|_| ())
        },
    );
    assert!(result.is_err());
    assert!(reads >= 4);
}

#[test]
fn malformed_and_unknown_credentials_are_not_accepted() {
    let f = Fixture::new();
    f.unlock();
    let client = f.owner().issue("Reader", &[Cap::Read]).unwrap();
    for fake in [
        "mpc_v1_short".to_string(),
        "mpc_v1_".to_string() + &"0".repeat(64),
        client.credential().replace("mpc_v1_", "MPC_v1_"),
    ] {
        assert!(f
            .core()
            .execute_client_batch(&f.state, &fake, &Fixture::batch())
            .is_err());
    }
    assert!(f.owner().issue("No grant", &[]).is_err());
    assert!(f
        .owner()
        .issue("Duplicates", &[Cap::Read, Cap::Read])
        .is_err());
}
