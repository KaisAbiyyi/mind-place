#![cfg(feature = "sqlcipher-preview")]
//! MP-R1-WP03: Encrypted fixture-vault v9 -> v10 forward migration with
//! verified encrypted backup outside the vault root.
use mind_place_security::SecurityGate;
use mind_place_state::{
    AssertionSource, ClientCapability as Cap, CnfRecord, CuratedNodeKind, CurationProfile,
    EvidenceSpan, NewCurationGeneration, NewTopic, NewTopicMembership, SourceDescriptor, SourceKey,
    StateError, StateStore, TopicMemberKind, TopicMemberRole, APPLICATION_ID, SCHEMA_VERSION,
};
use rusqlite::{Connection, OpenFlags};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;

#[test]
fn encrypted_fixture_vault_migrates_v9_to_v10_with_encrypted_backup_only() {
    let dir = tempfile::tempdir().unwrap();
    let vault_root = dir.path().join("vault");
    fs::create_dir_all(&vault_root).unwrap();
    fs::write(vault_root.join("vault-v1.json"), b"{}").unwrap();
    let db_path = vault_root.join("state.sqlite3");
    let key = [0x42u8; 32];

    let gate = SecurityGate::open(dir.path().join("security.json")).unwrap();
    gate.initialize(b"synthetic encrypted migration secret")
        .unwrap();
    let lease = gate.require_unlocked().unwrap();

    // Seed an encrypted v9 database directly.
    {
        let conn = Connection::open_with_flags(
            &db_path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )
        .unwrap();
        let hex: String = key.iter().map(|b| format!("{b:02x}")).collect();
        conn.execute_batch(&format!("PRAGMA key = \"x'{hex}'\";"))
            .unwrap();
        conn.pragma_update(None, "application_id", APPLICATION_ID)
            .unwrap();
        for (version, sql) in [
            (1, include_str!("../src/schema_v1.sql")),
            (2, include_str!("../src/schema_v2.sql")),
            (3, include_str!("../src/schema_v3.sql")),
            (4, include_str!("../src/schema_v4.sql")),
            (5, include_str!("../src/schema_v5.sql")),
            (6, include_str!("../src/schema_v6.sql")),
            (7, include_str!("../src/schema_v7.sql")),
            (8, include_str!("../src/schema_v8.sql")),
            (9, include_str!("../src/schema_v9.sql")),
        ] {
            conn.execute_batch(sql).unwrap();
            let checksum = format!("{:x}", Sha256::digest(sql.as_bytes()));
            conn.execute(
                "INSERT INTO schema_migrations(version,checksum) VALUES(?1,?2)",
                rusqlite::params![version, checksum],
            )
            .unwrap();
        }
        conn.execute(
            "INSERT INTO projects(id,name) VALUES('proj:vault','Vault Project')",
            [],
        )
        .unwrap();
        conn.pragma_update(None, "user_version", 9).unwrap();
    }

    // Ordinary `open_encrypted` refuses silent upgrade and surfaces explicit migration-required code.
    let err = StateStore::open_encrypted(&db_path, key, &gate, lease).unwrap_err();
    assert!(matches!(
        err,
        StateError::MigrationRequired {
            current: 9,
            target: 10
        }
    ));
    assert_eq!(err.code(), "MP_STATE_MIGRATION_REQUIRED");
    assert!(!vault_root.join("state-backups").exists());

    // Backup inside vault root is refused to preserve vault backup inventory rules.
    assert!(matches!(
        StateStore::migrate_encrypted_with_backup(
            &db_path,
            key,
            &gate,
            lease,
            vault_root.join("backup-inside.sqlite3"),
        ),
        Err(StateError::UnsafePath)
    ));

    let backup_path = dir.path().join("state-v9-encrypted-backup.sqlite3");
    let upgraded =
        StateStore::migrate_encrypted_with_backup(&db_path, key, &gate, lease, &backup_path)
            .unwrap();
    assert_eq!(SCHEMA_VERSION, 10);
    assert!(!vault_root.join("state-backups").exists());

    let backup_bytes = fs::read(&backup_path).unwrap();
    assert!(!backup_bytes.starts_with(b"SQLite format 3\0"));
    assert!(!backup_bytes
        .windows(b"Vault Project".len())
        .any(|w| w == b"Vault Project"));

    assert_eq!(
        upgraded.get_project("proj:vault").unwrap().unwrap().name,
        "Vault Project"
    );
    assert!(
        upgraded
            .get_topic("proj:vault")
            .unwrap()
            .unwrap()
            .is_project
    );

    let reopened = StateStore::open_encrypted(&db_path, key, &gate, lease).unwrap();
    let src = reopened
        .register_source(&SourceDescriptor {
            provider: "claude",
            source_namespace: "vault",
            source_kind: "conversation",
            key: SourceKey::External("vault-conv"),
        })
        .unwrap();
    let topic = reopened
        .create_topic(&NewTopic {
            id: "topic:vault-notes",
            slug: "vault-notes",
            name: "Vault Notes",
            parent_topic_id: None,
            is_project: false,
            work_status: None,
            description: None,
        })
        .unwrap();
    let gen = reopened
        .create_staged_generation(&NewCurationGeneration {
            id: "gen:vault-1",
            topic_id: &topic.id,
            run_id: "run:vault-1",
            profile: CurationProfile::Brief,
            asserted_by: AssertionSource::Model,
            model_id: Some("claude-haiku-5.5"),
            base_generation_id: None,
        })
        .unwrap();
    reopened
        .append_staged_ops(
            &gen.id,
            "shard-0",
            &[CnfRecord::Upsert {
                key: "vault-notes/fact/encrypted-ok".into(),
                kind: CuratedNodeKind::Fact,
                title: "Encrypted OK".into(),
                summary: "Curated state works inside encrypted vault.".into(),
                body: None,
                attrs: BTreeMap::new(),
                aliases: vec![],
                evidence: vec![EvidenceSpan {
                    src: src.clone(),
                    msg: 0,
                    span: [0, 8],
                    sha: format!("b3:{}", "c".repeat(64)),
                    quote: None,
                }],
                confidence: Some(0.9),
                base_rev: 0,
            }],
        )
        .unwrap();
    reopened.publish_generation(&gen.id).unwrap();
    assert!(reopened
        .export_topic_markdown("vault-notes")
        .unwrap()
        .markdown
        .contains("Encrypted OK"));

    // Pending staged generation referencing a source before redaction.
    let pending = reopened
        .create_staged_generation(&NewCurationGeneration {
            id: "gen:vault-pending",
            topic_id: &topic.id,
            run_id: "run:vault-pending",
            profile: CurationProfile::Brief,
            asserted_by: AssertionSource::Model,
            model_id: Some("claude-haiku-5.5"),
            base_generation_id: Some(&gen.id),
        })
        .unwrap();
    reopened
        .append_staged_ops(
            &pending.id,
            "shard-0",
            &[CnfRecord::Upsert {
                key: "vault-notes/fact/pending-before-redaction".into(),
                kind: CuratedNodeKind::Fact,
                title: "Pending secret".into(),
                summary: "Should fail publish after source redaction.".into(),
                body: None,
                attrs: BTreeMap::new(),
                aliases: vec![],
                evidence: vec![EvidenceSpan {
                    src: src.clone(),
                    msg: 0,
                    span: [0, 8],
                    sha: format!("b3:{}", "c".repeat(64)),
                    quote: None,
                }],
                confidence: Some(0.9),
                base_rev: 0,
            }],
        )
        .unwrap();

    // Also attach topic membership before redaction so concealment can be verified.
    reopened
        .add_topic_membership(&NewTopicMembership {
            id: "tmem:vault-src",
            topic_id: &topic.id,
            member_kind: TopicMemberKind::Source,
            member_id: &src,
            role: TopicMemberRole::Primary,
            asserted_by: AssertionSource::User,
            epistemic_origin: "explicit",
            evidence_source_ref: Some(&src),
            evidence_locator: Some("msg 0"),
            confidence: None,
        })
        .unwrap();

    let owner_token = "a".repeat(64);
    reopened
        .issue_client("client:v1:owner", "Owner", &owner_token, &[Cap::Admin])
        .unwrap();
    reopened
        .redact_source(&owner_token, &src, "Privacy request")
        .unwrap();

    // Redacted source cannot be cited in new staged ops, published, or linked in memberships.
    let after = reopened
        .create_staged_generation(&NewCurationGeneration {
            id: "gen:vault-after-redact",
            topic_id: &topic.id,
            run_id: "run:vault-after-redact",
            profile: CurationProfile::Brief,
            asserted_by: AssertionSource::Model,
            model_id: Some("claude-haiku-5.5"),
            base_generation_id: Some(&gen.id),
        })
        .unwrap();
    assert!(matches!(
        reopened.append_staged_ops(
            &after.id,
            "shard-0",
            &[CnfRecord::Upsert {
                key: "vault-notes/fact/post-redaction".into(),
                kind: CuratedNodeKind::Fact,
                title: "Blocked".into(),
                summary: "Should be denied.".into(),
                body: None,
                attrs: BTreeMap::new(),
                aliases: vec![],
                evidence: vec![EvidenceSpan {
                    src: src.clone(),
                    msg: 0,
                    span: [0, 8],
                    sha: format!("b3:{}", "c".repeat(64)),
                    quote: None,
                }],
                confidence: Some(0.8),
                base_rev: 0,
            }],
        ),
        Err(StateError::MemoryDenied)
    ));
    assert!(matches!(
        reopened.publish_generation(&pending.id),
        Err(StateError::MemoryDenied)
    ));
    assert!(matches!(
        reopened.add_topic_membership(&NewTopicMembership {
            id: "tmem:vault-src-blocked",
            topic_id: &topic.id,
            member_kind: TopicMemberKind::Source,
            member_id: &src,
            role: TopicMemberRole::Member,
            asserted_by: AssertionSource::User,
            epistemic_origin: "explicit",
            evidence_source_ref: Some(&src),
            evidence_locator: None,
            confidence: None,
        }),
        Err(StateError::MemoryDenied)
    ));

    // Read & Markdown export surfaces conceal already-accepted records tied to the redacted source.
    assert!(reopened
        .get_curated_node("vault-notes/fact/encrypted-ok")
        .unwrap()
        .is_none());
    assert!(reopened
        .list_topic_curated_nodes(&topic.id, true, 50)
        .unwrap()
        .is_empty());
    assert!(reopened
        .curated_node_revisions("vault-notes/fact/encrypted-ok")
        .unwrap()
        .is_empty());
    assert!(reopened
        .list_topic_memberships(&topic.id, true, 50)
        .unwrap()
        .is_empty());
    let exported = reopened.export_topic_markdown("vault-notes").unwrap();
    assert_eq!(exported.accepted_node_count, 0);
    assert!(!exported.markdown.contains("Encrypted OK"));
}
