//! MP-R1-WP01: the three official-export importers (ChatGPT export, Gemini Takeout, Claude.ai
//! export) end to end through the raw-first job (`import_authorized_source`), on the committed
//! **sanitized** fixtures (structure of a synthetic export, no private text). No real export is
//! read here; real exports are the owner's local runbook (`docs/qualification/MP-R1-WP01-import-trio-local-runbook.md`).

use mind_place_archive::historical::{
    ImportJobLimits, ImportJobReceipt, ImportMember, ManifestLimits, MemberKind, NoProgress,
    SourceImportRequest, SourceManifest,
};
use mind_place_archive::importers::{
    chatgpt_export::ChatGptExportParser, claude_export::ClaudeExportParser,
    gemini_takeout::GeminiTakeoutParser, ReconciledItem,
};
use mind_place_archive::ArchiveStore;
use mind_place_security::SecurityGate;
use std::path::{Path, PathBuf};
use std::time::Duration;

const CRED: &[u8] = b"synthetic-owner-credential";
const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../test/fixtures/import-sanitizer/parser-compat"
);
const GEMINI_PATH: &str = "Takeout/My Activity/Gemini Apps/MyActivity.html";

struct Fx {
    home: tempfile::TempDir,
    store: ArchiveStore,
    gate: SecurityGate,
}

impl Fx {
    fn new() -> Self {
        let home = tempfile::tempdir().unwrap();
        let store = ArchiveStore::open(home.path().join("archive-v1")).unwrap();
        let gate = SecurityGate::open(home.path().join("security-v1.json")).unwrap();
        gate.initialize(CRED).unwrap();
        Self { home, store, gate }
    }

    /// Lays out an export folder: `(relative path, fixture file name)`.
    fn export(&self, name: &str, files: &[(&str, &str)]) -> PathBuf {
        let root = self.home.path().join(name);
        for (relative, fixture) in files {
            let target = root.join(relative);
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            std::fs::copy(Path::new(FIXTURES).join(fixture), target).unwrap();
        }
        root
    }

    fn import(&self, provider: &str, source: &Path) -> ImportJobReceipt {
        let manifest = SourceManifest::measure(source, ManifestLimits::default()).unwrap();
        let members: Vec<ImportMember> = manifest
            .candidate_members(provider)
            .into_iter()
            .map(|entry| ImportMember {
                path: entry.path.clone(),
                kind: MemberKind::for_member(provider, &entry.path).unwrap(),
            })
            .collect();
        assert!(!members.is_empty(), "{provider}: no candidate member");
        let lease = self.gate.require_unlocked().unwrap();
        let cap = self
            .gate
            .authorize_import_session(
                lease,
                CRED,
                provider,
                source,
                *manifest.sha256(),
                Duration::from_secs(300),
            )
            .unwrap();
        let mut request = SourceImportRequest::new(&self.gate, lease, &cap, &members);
        request.progress = &NoProgress;
        request.limits = ImportJobLimits::default();
        self.store.import_authorized_source(&request).unwrap()
    }

    fn files_under(&self, sub: &str) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let mut pending = vec![self.store.root().join(sub)];
        while let Some(dir) = pending.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    pending.push(path);
                } else {
                    out.push(path);
                }
            }
        }
        out
    }
}

fn counts(items: &[ReconciledItem]) -> (u64, u64) {
    let projected = items
        .iter()
        .filter(|item| matches!(item, ReconciledItem::Conversation { .. }))
        .count() as u64;
    (projected, items.len() as u64 - projected)
}

/// Every evidence record of `provider` has a raw blob equal to the exact source slice it names.
fn assert_raw_equals_source(fx: &Fx, provider: &str, source: &Path) {
    let mut checked = 0;
    for path in fx.files_under("imports/evidence") {
        let record: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        if record["provider"] != provider {
            continue;
        }
        checked += 1;
        let member = std::fs::read(source.join(record["member_path"].as_str().unwrap())).unwrap();
        let offset = record["offset"].as_u64().unwrap() as usize;
        let length = record["length"].as_u64().unwrap() as usize;
        let sha = record["payload_sha256"].as_str().unwrap();
        let blob = std::fs::read(
            fx.store
                .root()
                .join("imports/raw")
                .join(&sha[..2])
                .join(format!("{sha}.raw")),
        )
        .unwrap();
        assert_eq!(blob, member[offset..offset + length]);
    }
    assert!(checked > 0, "{provider}: no evidence record");
}

#[test]
fn the_three_official_exports_import_raw_first_from_sanitized_fixtures() {
    let fx = Fx::new();

    let chatgpt = fx.export(
        "chatgpt",
        &[("conversations.json", "chatgpt-sanitized.json")],
    );
    let gemini = fx.export("gemini", &[(GEMINI_PATH, "gemini-sanitized.html")]);
    let claude = fx.export(
        "claude",
        &[
            ("conversations.json", "claude-sanitized.json"),
            ("projects.json", "claude-projects-sanitized.json"),
        ],
    );
    // The account file of a Claude export is never a member (it is only measured).
    std::fs::write(
        claude.join("users.json"),
        br#"[{"uuid":"x","full_name":"lorem","email_address":"lorem"}]"#,
    )
    .unwrap();

    let expected_chatgpt = counts(
        &ChatGptExportParser::parse_conversations_json(
            std::fs::read(chatgpt.join("conversations.json"))
                .unwrap()
                .as_slice(),
        )
        .unwrap(),
    );
    let expected_gemini = counts(
        &GeminiTakeoutParser::parse_cards_html(
            std::fs::read(gemini.join(GEMINI_PATH)).unwrap().as_slice(),
        )
        .unwrap(),
    );
    let claude_conversations = ClaudeExportParser::parse_conversations_json(
        std::fs::read(claude.join("conversations.json"))
            .unwrap()
            .as_slice(),
    )
    .unwrap();
    let claude_projects = ClaudeExportParser::parse_projects_json(
        std::fs::read(claude.join("projects.json"))
            .unwrap()
            .as_slice(),
    )
    .unwrap();
    let (claude_projected, claude_raw) = counts(&claude_conversations);

    let mut published = 0;
    for (provider, source, (projected, raw_only)) in [
        ("chatgpt", &chatgpt, expected_chatgpt),
        ("gemini", &gemini, expected_gemini),
        (
            "claude",
            &claude,
            (claude_projected, claude_raw + claude_projects.len() as u64),
        ),
    ] {
        let receipt = fx.import(provider, source);
        assert_eq!(receipt.provider, provider);
        assert_eq!(
            receipt.imported + receipt.deduplicated,
            projected,
            "{provider}"
        );
        assert_eq!(receipt.raw_only + receipt.rejected, raw_only, "{provider}");
        assert_eq!(receipt.records_scanned, projected + raw_only, "{provider}");
        assert!(receipt.imported > 0, "{provider}: nothing projected");
        assert!(receipt
            .members
            .iter()
            .all(|member| !member.path.ends_with("users.json")));
        published += receipt.imported;
        assert_raw_equals_source(&fx, provider, source);

        // Re-running the same import is idempotent and reports the stored receipt.
        let again = fx.import(provider, source);
        assert!(again.already_complete, "{provider}");
        assert_eq!(again.job_key, receipt.job_key);
    }
    let conversations = std::fs::read_dir(fx.store.root().join("conversations"))
        .unwrap()
        .count() as u64;
    assert_eq!(conversations, published);
    // Claude projections are stored as historical `export-import` captures of provider claude.
    let stored: Vec<serde_json::Value> = fx
        .files_under("conversations")
        .into_iter()
        .filter(|path| path.extension().is_some_and(|e| e == "jsonl"))
        .map(|path| {
            let text = std::fs::read_to_string(path).unwrap();
            serde_json::from_str(text.lines().next().unwrap()).unwrap()
        })
        .collect();
    let claude_stored: Vec<_> = stored
        .iter()
        .filter(|capture| capture["provider"] == "claude")
        .collect();
    assert_eq!(claude_stored.len() as u64, claude_projected);
    for capture in claude_stored {
        assert_eq!(capture["capture_method"], "export-import");
        assert!(capture["source_url"]
            .as_str()
            .unwrap()
            .starts_with("https://claude.ai/chat/"));
    }
}

/// Runs one import and returns the error text (the import must fail).
fn import_error(fx: &Fx, provider: &str, source: &Path, members: &[ImportMember]) -> String {
    let manifest = SourceManifest::measure(source, ManifestLimits::default()).unwrap();
    let lease = fx.gate.require_unlocked().unwrap();
    let cap = fx
        .gate
        .authorize_import_session(
            lease,
            CRED,
            provider,
            source,
            *manifest.sha256(),
            Duration::from_secs(300),
        )
        .unwrap();
    let request = SourceImportRequest::new(&fx.gate, lease, &cap, members);
    fx.store
        .import_authorized_source(&request)
        .unwrap_err()
        .to_string()
}

#[test]
fn an_export_imported_under_the_other_provider_is_refused_before_anything_is_published() {
    let fx = Fx::new();
    let chatgpt = fx.export(
        "chatgpt",
        &[("conversations.json", "chatgpt-sanitized.json")],
    );
    let claude = fx.export("claude", &[("conversations.json", "claude-sanitized.json")]);
    for (provider, source, kind) in [
        ("claude", &chatgpt, MemberKind::ClaudeConversations),
        ("chatgpt", &claude, MemberKind::ChatgptConversations),
    ] {
        let members = [ImportMember {
            path: "conversations.json".into(),
            kind,
        }];
        let error = import_error(&fx, provider, source, &members);
        assert!(
            error.starts_with("E_PROVIDER_MISMATCH"),
            "{provider}: {error}"
        );
    }
    assert!(fx.files_under("imports/raw").is_empty());
    assert!(fx.files_under("imports/evidence").is_empty());
    assert!(fx.files_under("conversations").is_empty());

    // Records of no known shape stay raw-only evidence, as before (not a provider mismatch).
    let odd = fx.home.path().join("odd");
    std::fs::create_dir_all(&odd).unwrap();
    std::fs::write(odd.join("conversations.json"), br#"[1, {"a": 2}]"#).unwrap();
    let receipt = fx.import("claude", &odd);
    assert_eq!((receipt.imported, receipt.raw_only), (0, 2));
}

#[test]
fn duplicate_claude_conversations_across_shards_get_distinct_identities() {
    let fx = Fx::new();
    let claude = fx.export(
        "claude",
        &[
            ("conversations-000.json", "claude-sanitized.json"),
            ("conversations-001.json", "claude-sanitized.json"),
        ],
    );
    let single = ClaudeExportParser::parse_conversations_json(
        std::fs::read(claude.join("conversations-000.json"))
            .unwrap()
            .as_slice(),
    )
    .unwrap();
    let (projected, _) = counts(&single);
    let receipt = fx.import("claude", &claude);
    // The tracker spans both shards: the second copy of each conversation is a new identity,
    // not a duplicate write of the first.
    assert_eq!(receipt.imported, 2 * projected);
    assert_eq!(receipt.deduplicated, 0);
    let conversations = std::fs::read_dir(fx.store.root().join("conversations"))
        .unwrap()
        .count() as u64;
    assert_eq!(conversations, 2 * projected);
}

#[test]
fn a_member_kind_that_does_not_match_its_file_is_refused() {
    let fx = Fx::new();
    let claude = fx.export(
        "claude",
        &[
            ("conversations.json", "claude-sanitized.json"),
            ("projects.json", "claude-projects-sanitized.json"),
        ],
    );
    for (path, kind) in [
        ("conversations.json", MemberKind::ClaudeProjects),
        ("projects.json", MemberKind::ClaudeConversations),
    ] {
        let members = [ImportMember {
            path: path.into(),
            kind,
        }];
        let error = import_error(&fx, "claude", &claude, &members);
        assert!(error.starts_with("E_MEMBERS"), "{path}: {error}");
    }
}

#[test]
fn an_account_file_named_explicitly_is_refused_before_anything_is_staged() {
    let fx = Fx::new();
    let claude = fx.export("claude", &[("conversations.json", "claude-sanitized.json")]);
    std::fs::write(
        claude.join("users.json"),
        br#"[{"uuid":"1b2c3d4e-0000-4000-8000-0000000000ff","full_name":"x","email_address":"x"}]"#,
    )
    .unwrap();
    let manifest = SourceManifest::measure(&claude, ManifestLimits::default()).unwrap();
    let lease = fx.gate.require_unlocked().unwrap();
    let cap = fx
        .gate
        .authorize_import_session(
            lease,
            CRED,
            "claude",
            &claude,
            *manifest.sha256(),
            Duration::from_secs(300),
        )
        .unwrap();
    // A caller that bypasses `for_member` and claims a conversations kind is still refused.
    let members = [
        ImportMember {
            path: "conversations.json".into(),
            kind: MemberKind::ClaudeConversations,
        },
        ImportMember {
            path: "users.json".into(),
            kind: MemberKind::ClaudeConversations,
        },
    ];
    let request = SourceImportRequest::new(&fx.gate, lease, &cap, &members);
    let error = fx.store.import_authorized_source(&request).unwrap_err();
    assert!(error.to_string().starts_with("E_MEMBERS"), "{error}");
    assert!(fx.files_under("imports/raw").is_empty());
    assert!(fx.files_under("imports/evidence").is_empty());
}

#[test]
fn an_explicit_claude_member_of_the_wrong_kind_is_refused_by_the_job() {
    let fx = Fx::new();
    let claude = fx.export("claude", &[("conversations.json", "claude-sanitized.json")]);
    let manifest = SourceManifest::measure(&claude, ManifestLimits::default()).unwrap();
    let lease = fx.gate.require_unlocked().unwrap();
    let cap = fx
        .gate
        .authorize_import_session(
            lease,
            CRED,
            "claude",
            &claude,
            *manifest.sha256(),
            Duration::from_secs(300),
        )
        .unwrap();
    // A ChatGPT kind under a Claude authorization is refused before anything is read.
    let members = [ImportMember {
        path: "conversations.json".into(),
        kind: MemberKind::ChatgptConversations,
    }];
    let request = SourceImportRequest::new(&fx.gate, lease, &cap, &members);
    let error = fx.store.import_authorized_source(&request).unwrap_err();
    assert!(error.to_string().starts_with("E_MEMBERS"), "{error}");
    assert_eq!(
        MemberKind::for_member("claude", "a/projects.json"),
        Some(MemberKind::ClaudeProjects)
    );
    assert_eq!(
        MemberKind::for_member("claude", "conversations-001.json"),
        Some(MemberKind::ClaudeConversations)
    );
    assert_eq!(MemberKind::for_member("claude", "chat.html"), None);
    // Account files are never members, under any provider or kind, at any depth or case.
    for (provider, path) in [
        ("claude", "users.json"),
        ("claude", "nested/USERS.json"),
        ("chatgpt", "user.json"),
        ("chatgpt", "a/User.JSON"),
    ] {
        assert_eq!(MemberKind::for_member(provider, path), None, "{path}");
    }
    assert_eq!(MemberKind::for_member("other", "conversations.json"), None);
}
