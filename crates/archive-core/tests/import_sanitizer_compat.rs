//! MP-IMPORT-SANITIZE-01 cross-check: a sanitized export must behave exactly like its original in
//! the real import parsers. Both files of each pair are synthetic (invented) fixtures; the
//! sanitized one is produced by `scripts/import-sanitizer` (see test/import-sanitizer.test.mjs,
//! which also fails if the committed sanitized files drift from the tool's output).

use mind_place_archive::importers::{
    chatgpt_export::ChatGptExportParser, gemini_takeout::GeminiTakeoutParser, ReconciledItem,
};

const CHATGPT_ORIGINAL: &[u8] =
    include_bytes!("../../../test/fixtures/import-sanitizer/parser-compat/chatgpt-synthetic.json");
const CHATGPT_SANITIZED: &[u8] =
    include_bytes!("../../../test/fixtures/import-sanitizer/parser-compat/chatgpt-sanitized.json");
const GEMINI_ORIGINAL: &[u8] =
    include_bytes!("../../../test/fixtures/import-sanitizer/parser-compat/gemini-synthetic.html");
const GEMINI_SANITIZED: &[u8] =
    include_bytes!("../../../test/fixtures/import-sanitizer/parser-compat/gemini-sanitized.html");

/// Everything about a parsed item that depends on structure rather than on text content.
#[derive(Debug, PartialEq, Eq)]
enum Shape {
    Conversation {
        roles: Vec<String>,
        formats: Vec<String>,
        message_times: Vec<bool>,
        attachments: Vec<usize>,
        completeness: String,
        warnings: usize,
        official_url: bool,
        date: String,
    },
    RawOnly {
        reason: String,
        assets: usize,
    },
}

fn shape(item: &ReconciledItem) -> Shape {
    match item {
        ReconciledItem::Conversation {
            capture, assets, ..
        } => {
            assert!(assets.is_empty());
            Shape::Conversation {
                roles: capture.messages.iter().map(|m| m.role.clone()).collect(),
                formats: capture.messages.iter().map(|m| m.format.clone()).collect(),
                message_times: capture
                    .messages
                    .iter()
                    .map(|m| m.authored_at.is_some())
                    .collect(),
                attachments: capture
                    .messages
                    .iter()
                    .map(|m| m.attachments.len())
                    .collect(),
                completeness: capture.completeness.clone(),
                warnings: capture.warnings.len(),
                official_url: capture.source_url.starts_with("https://"),
                date: capture.captured_at.chars().take(10).collect(),
            }
        }
        ReconciledItem::PreservedRawOnly {
            disposition_reason,
            assets,
            ..
        } => Shape::RawOnly {
            reason: disposition_reason.clone(),
            assets: assets.len(),
        },
    }
}

#[test]
fn sanitized_chatgpt_export_parses_with_the_same_shape_as_the_original() {
    let original = ChatGptExportParser::parse_conversations_json(CHATGPT_ORIGINAL).unwrap();
    let sanitized = ChatGptExportParser::parse_conversations_json(CHATGPT_SANITIZED).unwrap();
    assert_eq!(original.len(), 6);
    let (a, b): (Vec<_>, Vec<_>) = (
        original.iter().map(shape).collect(),
        sanitized.iter().map(shape).collect(),
    );
    assert_eq!(a, b);
    // The fixture really exercises the interesting cases, so equality is not vacuous.
    assert!(
        a.iter().any(|s| matches!(s, Shape::RawOnly { .. })),
        "raw-only conversation expected"
    );
    assert!(a.iter().any(
        |s| matches!(s, Shape::Conversation { completeness, .. } if completeness == "complete")
    ));
    assert!(a.iter().any(
        |s| matches!(s, Shape::Conversation { completeness, .. } if completeness == "partial")
    ));
    assert!(a.iter().any(|s| matches!(s, Shape::Conversation { attachments, .. } if attachments.iter().any(|n| *n > 0))));
    // Raw evidence identity: every record maps to a distinct sanitized source identifier.
    let ids = |items: &[ReconciledItem]| -> Vec<String> {
        items
            .iter()
            .map(|item| match item {
                ReconciledItem::Conversation { raw_evidence, .. } => {
                    raw_evidence.source_identifier.clone()
                }
                ReconciledItem::PreservedRawOnly { evidence, .. } => {
                    evidence.source_identifier.clone()
                }
            })
            .collect()
    };
    let (ids_a, ids_b) = (ids(&original), ids(&sanitized));
    assert_eq!(
        ids_a.iter().collect::<std::collections::HashSet<_>>().len(),
        ids_b.iter().collect::<std::collections::HashSet<_>>().len()
    );
    assert!(
        ids_a.iter().zip(&ids_b).all(|(x, y)| x != y),
        "identifiers are pseudonymized"
    );
}

#[test]
fn sanitized_gemini_takeout_parses_with_the_same_shape_as_the_original() {
    let original = GeminiTakeoutParser::parse_cards_html(GEMINI_ORIGINAL).unwrap();
    let sanitized = GeminiTakeoutParser::parse_cards_html(GEMINI_SANITIZED).unwrap();
    assert_eq!(original.len(), 7, "six cards plus one duplicate");
    let (a, b): (Vec<_>, Vec<_>) = (
        original.iter().map(shape).collect(),
        sanitized.iter().map(shape).collect(),
    );
    assert_eq!(a, b);
    assert!(
        a.iter()
            .any(|s| matches!(s, Shape::Conversation { roles, .. } if roles.len() >= 4)),
        "multi-turn card expected"
    );
    assert!(
        a.iter()
            .any(|s| matches!(s, Shape::Conversation { roles, .. } if roles == &["assistant"])),
        "response-only card expected"
    );
    assert!(
        a.iter().any(|s| matches!(
            s,
            Shape::Conversation {
                official_url: false,
                ..
            }
        )),
        "non-official identity expected"
    );
}

#[test]
fn sanitized_exports_do_not_contain_the_synthetic_private_strings() {
    for (name, original, sanitized) in [
        ("chatgpt", CHATGPT_ORIGINAL, CHATGPT_SANITIZED),
        ("gemini", GEMINI_ORIGINAL, GEMINI_SANITIZED),
    ] {
        let original = String::from_utf8_lossy(original);
        let sanitized = String::from_utf8_lossy(sanitized);
        assert!(
            original.contains("privatemail.test"),
            "{name}: fixture carries private-looking strings"
        );
        for needle in [
            "privatemail",
            "example.net",
            "quartzfinch",
            "marmalade",
            "Notes about",
        ] {
            assert!(!sanitized.contains(needle), "{name}: {needle} survived");
        }
    }
}
