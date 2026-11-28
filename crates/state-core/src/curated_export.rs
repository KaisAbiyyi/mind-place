//! MP-R1-WP03: Read-only deterministic Markdown export of accepted curated nodes.
//!
//! Exports only nodes and active edges materialized from `published`
//! generations (`staged` and `rejected` proposals are never disclosed here).
use crate::{
    curated::{
        curated_node_has_redacted_source, evidence_spans_have_redacted_source, load_edge_evidence,
        load_node_aliases, load_node_all_evidence, AttrValue, CuratedEdgeRel, CuratedNodeKind,
        CuratedNodeStatus, EvidenceSpan,
    },
    topics::{fetch_topic, resolve_topic_by_id_or_slug, AssertionSource, TopicWorkStatus},
    valid_text, Result, StateError, StateStore,
};
use rusqlite::Connection;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt::Write as _;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct TopicMarkdownExport {
    pub topic_id: String,
    pub topic_slug: String,
    pub topic_name: String,
    pub is_project: bool,
    pub work_status: Option<TopicWorkStatus>,
    pub accepted_node_count: usize,
    pub accepted_edge_count: usize,
    pub digest_sha256: String,
    pub markdown: String,
}

struct ExportNode {
    id: String,
    canonical_key: String,
    kind: CuratedNodeKind,
    current_rev: i64,
    title: String,
    summary: String,
    body: Option<String>,
    attrs: BTreeMap<String, AttrValue>,
    aliases: Vec<String>,
    status: CuratedNodeStatus,
    supersedes_key: Option<String>,
    superseded_by_key: Option<String>,
    asserted_by: AssertionSource,
    model_id: Option<String>,
    confidence: Option<f64>,
    evidence: Vec<(i64, EvidenceSpan)>,
}

struct ExportEdge {
    from_id: String,
    from_key: String,
    rel: CuratedEdgeRel,
    to_id: String,
    to_key: String,
    evidence: Vec<EvidenceSpan>,
}

fn format_attr(value: &AttrValue) -> String {
    match value {
        AttrValue::Bool(b) => b.to_string(),
        AttrValue::Integer(i) => i.to_string(),
        AttrValue::Float(f) => format!("{f}"),
        AttrValue::String(s) => s.clone(),
    }
}

fn escape_inline(text: &str) -> String {
    text.replace('\r', " ")
        .replace('\n', " ")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn render_blockquote(text: &str) -> String {
    let mut out = String::new();
    for line in text.replace('\r', "").split('\n') {
        let safe = line.replace('<', "&lt;").replace('>', "&gt;");
        let _ = writeln!(&mut out, "> {safe}");
    }
    out
}

fn load_export_nodes(db: &Connection, topic_id: &str) -> Result<Vec<ExportNode>> {
    let mut key_stmt = db.prepare("SELECT id,canonical_key FROM curated_nodes")?;
    let key_map: BTreeMap<String, String> = key_stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<std::result::Result<BTreeMap<_, _>, _>>()?;
    drop(key_stmt);

    let mut stmt = db.prepare(
        "SELECT id,canonical_key,kind,current_rev,title,summary,body,attrs_json,
                status,supersedes_node_id,superseded_by_node_id,asserted_by,model_id,confidence
         FROM curated_nodes
         WHERE topic_id=?1
         ORDER BY kind, slug, canonical_key",
    )?;
    let rows = stmt
        .query_map([topic_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, Option<String>>(6)?,
                r.get::<_, String>(7)?,
                r.get::<_, String>(8)?,
                r.get::<_, Option<String>>(9)?,
                r.get::<_, Option<String>>(10)?,
                r.get::<_, String>(11)?,
                r.get::<_, Option<String>>(12)?,
                r.get::<_, Option<f64>>(13)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    drop(stmt);

    let mut nodes = Vec::with_capacity(rows.len());
    for row in rows {
        let attrs: BTreeMap<String, AttrValue> =
            serde_json::from_str(&row.7).map_err(|_| StateError::Corrupt)?;
        let aliases = load_node_aliases(db, &row.0)?;
        let evidence = load_node_all_evidence(db, &row.0)?;
        if !evidence.iter().any(|(rev, _)| *rev == row.3) {
            return Err(StateError::Corrupt);
        }
        if curated_node_has_redacted_source(db, &row.0)? {
            continue;
        }
        let supersedes_key = match row.9.as_deref() {
            Some(id) => {
                let key = key_map.get(id).cloned().ok_or(StateError::Corrupt)?;
                if curated_node_has_redacted_source(db, id)? {
                    None
                } else {
                    Some(key)
                }
            }
            None => None,
        };
        let superseded_by_key = match row.10.as_deref() {
            Some(id) => {
                let key = key_map.get(id).cloned().ok_or(StateError::Corrupt)?;
                if curated_node_has_redacted_source(db, id)? {
                    None
                } else {
                    Some(key)
                }
            }
            None => None,
        };
        nodes.push(ExportNode {
            id: row.0,
            canonical_key: row.1,
            kind: CuratedNodeKind::parse(&row.2).map_err(|_| StateError::Corrupt)?,
            current_rev: row.3,
            title: row.4,
            summary: row.5,
            body: row.6,
            attrs,
            aliases,
            status: CuratedNodeStatus::parse(&row.8)?,
            supersedes_key,
            superseded_by_key,
            asserted_by: AssertionSource::parse(&row.11)?,
            model_id: row.12,
            confidence: row.13,
            evidence,
        });
    }
    Ok(nodes)
}

fn load_export_edges(db: &Connection, topic_id: &str) -> Result<Vec<ExportEdge>> {
    let mut stmt = db.prepare(
        "SELECT e.id,e.from_node_id,f.canonical_key,e.rel,e.to_node_id,t.canonical_key
         FROM curated_edges e
         JOIN curated_nodes f ON f.id=e.from_node_id
         JOIN curated_nodes t ON t.id=e.to_node_id
         WHERE (e.topic_id=?1 OR f.topic_id=?1 OR t.topic_id=?1)
           AND e.status='active'
         ORDER BY f.canonical_key, e.rel, t.canonical_key",
    )?;
    let rows = stmt
        .query_map([topic_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    drop(stmt);

    let mut edges = Vec::with_capacity(rows.len());
    for (edge_id, from_id, from_key, rel_str, to_id, to_key) in rows {
        let evidence = load_edge_evidence(db, &edge_id)?;
        if evidence.is_empty() {
            return Err(StateError::Corrupt);
        }
        if evidence_spans_have_redacted_source(db, &evidence)?
            || curated_node_has_redacted_source(db, &from_id)?
            || curated_node_has_redacted_source(db, &to_id)?
        {
            continue;
        }
        edges.push(ExportEdge {
            from_id,
            from_key,
            rel: CuratedEdgeRel::parse(&rel_str).map_err(|_| StateError::Corrupt)?,
            to_id,
            to_key,
            evidence,
        });
    }
    Ok(edges)
}

impl StateStore {
    /// Generates a deterministic, read-only Markdown view of all accepted
    /// curated nodes and active edges for a topic (resolved by id or slug).
    pub fn export_topic_markdown(&self, topic_id_or_slug: &str) -> Result<TopicMarkdownExport> {
        valid_text(topic_id_or_slug)?;
        let db = self.read_connection()?;
        let topic = resolve_topic_by_id_or_slug(&db, topic_id_or_slug.trim())?
            .ok_or(StateError::InvalidValue)?;
        let parent_slug = match topic.parent_topic_id.as_deref() {
            Some(pid) => Some(fetch_topic(&db, pid)?.ok_or(StateError::Corrupt)?.slug),
            None => None,
        };
        let nodes = load_export_nodes(&db, &topic.id)?;
        let edges = load_export_edges(&db, &topic.id)?;

        let mut md = String::new();
        writeln!(
            &mut md,
            "<!-- Read-only Mind Place curated export (accepted nodes only). Authoritative state lives in state.sqlite3. -->"
        )
        .map_err(|_| StateError::Corrupt)?;
        writeln!(&mut md, "# {}", escape_inline(&topic.name)).map_err(|_| StateError::Corrupt)?;
        writeln!(&mut md).map_err(|_| StateError::Corrupt)?;
        writeln!(&mut md, "- **Topic slug:** `{}`", topic.slug).map_err(|_| StateError::Corrupt)?;
        if let Some(parent) = parent_slug {
            writeln!(&mut md, "- **Parent topic:** `{parent}`").map_err(|_| StateError::Corrupt)?;
        }
        if topic.is_project {
            let status = topic.work_status.map_or("active", TopicWorkStatus::as_str);
            writeln!(&mut md, "- **Project work status:** `{status}`")
                .map_err(|_| StateError::Corrupt)?;
        }
        if let Some(desc) = topic.description.as_deref() {
            writeln!(&mut md, "- **Description:** {}", escape_inline(desc))
                .map_err(|_| StateError::Corrupt)?;
        }
        writeln!(&mut md, "- **Accepted nodes:** {}", nodes.len())
            .map_err(|_| StateError::Corrupt)?;
        writeln!(&mut md, "- **Accepted active edges:** {}", edges.len())
            .map_err(|_| StateError::Corrupt)?;

        if nodes.is_empty() {
            writeln!(&mut md, "\n_No accepted curated nodes._").map_err(|_| StateError::Corrupt)?;
        } else {
            for node in &nodes {
                writeln!(&mut md).map_err(|_| StateError::Corrupt)?;
                writeln!(
                    &mut md,
                    "## {} (`{}`)",
                    escape_inline(&node.title),
                    node.canonical_key
                )
                .map_err(|_| StateError::Corrupt)?;
                writeln!(&mut md).map_err(|_| StateError::Corrupt)?;
                write!(
                    &mut md,
                    "- **Kind:** `{}` · **Status:** `{}` · **Rev:** `{}` · **Asserted by:** `{}`",
                    node.kind.as_str(),
                    node.status.as_str(),
                    node.current_rev,
                    node.asserted_by.as_str()
                )
                .map_err(|_| StateError::Corrupt)?;
                if let Some(model) = node.model_id.as_deref() {
                    write!(&mut md, " (`{}`)", escape_inline(model))
                        .map_err(|_| StateError::Corrupt)?;
                }
                if let Some(conf) = node.confidence {
                    write!(&mut md, " · **Confidence:** `{conf:.2}`")
                        .map_err(|_| StateError::Corrupt)?;
                }
                writeln!(&mut md).map_err(|_| StateError::Corrupt)?;
                if let Some(prev) = node.supersedes_key.as_deref() {
                    writeln!(&mut md, "- **Supersedes:** `{prev}`")
                        .map_err(|_| StateError::Corrupt)?;
                }
                if let Some(next) = node.superseded_by_key.as_deref() {
                    writeln!(&mut md, "- **Superseded by:** `{next}`")
                        .map_err(|_| StateError::Corrupt)?;
                }
                if !node.aliases.is_empty() {
                    let joined: Vec<String> = node
                        .aliases
                        .iter()
                        .map(|a| format!("`{}`", escape_inline(a)))
                        .collect();
                    writeln!(&mut md, "- **Aliases:** {}", joined.join(", "))
                        .map_err(|_| StateError::Corrupt)?;
                }
                if !node.attrs.is_empty() {
                    let pairs: Vec<String> = node
                        .attrs
                        .iter()
                        .map(|(k, v)| format!("`{k}`={}", escape_inline(&format_attr(v))))
                        .collect();
                    writeln!(&mut md, "- **Attributes:** {}", pairs.join(", "))
                        .map_err(|_| StateError::Corrupt)?;
                }
                writeln!(&mut md).map_err(|_| StateError::Corrupt)?;
                md.push_str(&render_blockquote(&node.summary));
                if let Some(body) = node.body.as_deref() {
                    writeln!(&mut md).map_err(|_| StateError::Corrupt)?;
                    md.push_str(&render_blockquote(body));
                }

                let related: Vec<&ExportEdge> = edges
                    .iter()
                    .filter(|e| e.from_id == node.id || e.to_id == node.id)
                    .collect();
                if !related.is_empty() {
                    writeln!(&mut md, "\n**Relations:**").map_err(|_| StateError::Corrupt)?;
                    for edge in related {
                        if edge.from_id == node.id {
                            writeln!(
                                &mut md,
                                "- `{} -> {}` (`{}`) [{} spans]",
                                edge.rel.as_str(),
                                edge.to_key,
                                edge.from_key,
                                edge.evidence.len()
                            )
                            .map_err(|_| StateError::Corrupt)?;
                        } else {
                            writeln!(
                                &mut md,
                                "- `{} <- {}` (`{}`) [{} spans]",
                                edge.rel.as_str(),
                                edge.from_key,
                                edge.to_key,
                                edge.evidence.len()
                            )
                            .map_err(|_| StateError::Corrupt)?;
                        }
                    }
                }

                writeln!(&mut md, "\n**Evidence spans:**").map_err(|_| StateError::Corrupt)?;
                for (rev, span) in &node.evidence {
                    writeln!(
                        &mut md,
                        "- [rev `{rev}`] `{}` msg `{}` span `[{}..{}]` sha `{}`",
                        span.src, span.msg, span.span[0], span.span[1], span.sha
                    )
                    .map_err(|_| StateError::Corrupt)?;
                }
            }
        }

        let digest_sha256 = format!("{:x}", Sha256::digest(md.as_bytes()));
        Ok(TopicMarkdownExport {
            topic_id: topic.id,
            topic_slug: topic.slug,
            topic_name: topic.name,
            is_project: topic.is_project,
            work_status: topic.work_status,
            accepted_node_count: nodes.len(),
            accepted_edge_count: edges.len(),
            digest_sha256,
            markdown: md,
        })
    }
}
