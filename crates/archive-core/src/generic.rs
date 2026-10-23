//! Explicit, bounded, raw-preserving imports for plain transcript interchange.
//! Deliberately not a provider-specific export parser. No filesystem path is accepted.
use crate::{ArchiveError, ArchiveStore, Capture, Message, Result, SaveReceipt};
use serde::Serialize;
use serde_json::Value;

pub const MAX_GENERIC_IMPORT_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, Serialize)]
pub struct GenericImportReceipt {
    pub format: String,
    pub raw_sha256: String,
    pub raw_preserved: bool,
    pub saved: Option<SaveReceipt>,
    pub errors: Vec<String>,
}

fn invalid(message: impl Into<String>) -> ArchiveError {
    ArchiveError::Invalid(message.into())
}

fn role(value: &str) -> Option<&'static str> {
    match value.trim().to_ascii_lowercase().as_str() {
        "user" => Some("user"),
        "assistant" => Some("assistant"),
        "system" => Some("system"),
        "tool" => Some("tool"),
        _ => None,
    }
}

fn message(index: usize, kind: &str, text: String, format: &str) -> Result<Message> {
    let role = role(kind).ok_or_else(|| invalid("Unrecognized explicit transcript role"))?;
    if text.len() > 4 * 1024 * 1024 {
        return Err(invalid("Transcript message exceeds 4 MiB"));
    }
    Ok(Message {
        // This ordinal is an importer record locator, NOT a claimed source message ID.
        id: format!("record-{index:05}"),
        role: role.into(),
        text,
        format: format.into(),
        authored_at: None,
        model: None,
        attachments: Vec::new(),
    })
}

fn json_messages(source: &str) -> Result<(String, Vec<Message>)> {
    let data: Value = serde_json::from_str(source)?;
    let (title, records) = match &data {
        Value::Array(values) => ("Imported JSON transcript".to_owned(), values),
        Value::Object(map) => {
            if map.keys().any(|key| !matches!(key.as_str(), "title" | "messages")) {
                return Err(invalid("JSON transcript has unsupported top-level fields"));
            }
            let values = map.get("messages").and_then(Value::as_array)
                .ok_or_else(|| invalid("JSON transcript requires a messages array"))?;
            let title = map.get("title").and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("Imported JSON transcript");
            (title.to_owned(), values)
        }
        _ => return Err(invalid("JSON transcript must be an array or an object with messages")),
    };
    if records.is_empty() || records.len() > 10_000 {
        return Err(invalid("JSON transcript requires 1 to 10000 explicit messages"));
    }
    let mut messages = Vec::with_capacity(records.len());
    for (index, item) in records.iter().enumerate() {
        let record = item.as_object()
            .ok_or_else(|| invalid(format!("JSON record {index} is not an object")))?;
        if record.keys().any(|key| !matches!(key.as_str(), "role" | "content" | "text" | "id" | "authored_at" | "model" | "attachments")) {
            return Err(invalid(format!("JSON record {index} contains unsupported fields")));
        }
        let kind = record.get("role").and_then(Value::as_str)
            .ok_or_else(|| invalid(format!("JSON record {index} has no explicit role")))?;
        // Refuse ambiguous representations rather than picking one and dropping evidence.
        if record.contains_key("content") && record.contains_key("text") {
            return Err(invalid(format!("JSON record {index} has ambiguous content and text")));
        }
        let body = record.get("content").or_else(|| record.get("text"))
            .and_then(Value::as_str)
            .ok_or_else(|| invalid(format!("JSON record {index} requires string content/text")))?;
        if record.get("attachments").is_some_and(|value| !value.is_null() && value != &Value::Array(vec![])) {
            return Err(invalid(format!("JSON record {index} contains unsupported attachments")));
        }
        let mut item = message(index, kind, body.to_owned(), "plain")?;
        if let Some(id) = record.get("id") {
            let id = id.as_str().filter(|id| !id.is_empty() && id.len() <= 256)
                .ok_or_else(|| invalid(format!("JSON record {index} has invalid id")))?;
            item.id = id.into();
        }
        if let Some(value) = record.get("authored_at") {
            item.authored_at = Some(value.as_str()
                .ok_or_else(|| invalid(format!("JSON record {index} has invalid authored_at")))?.into());
        }
        if let Some(value) = record.get("model") {
            item.model = Some(value.as_str()
                .ok_or_else(|| invalid(format!("JSON record {index} has invalid model")))?.into());
        }
        messages.push(item);
    }
    Ok((title, messages))
}

fn markdown_messages(source: &str) -> Result<(String, Vec<Message>)> {
    let mut title = "Imported Markdown transcript".to_owned();
    let mut records: Vec<Message> = Vec::new();
    let mut current: Option<&'static str> = None;
    let mut body = String::new();
    let mut preamble = String::new();
    for line in source.split_inclusive('\n') {
        let header = line.trim_end_matches(['\r', '\n']).trim();
        let next = header.strip_prefix("## ").and_then(role)
            .or_else(|| header.strip_prefix("### ").and_then(role));
        if let Some(kind) = next {
            if let Some(previous) = current {
                records.push(message(records.len(), previous, body.trim_matches('\n').to_owned(), "markdown")?);
                body.clear();
            } else if !preamble.trim().is_empty() {
                let pre = preamble.trim();
                if let Some(name) = pre.strip_prefix("# ") {
                    if name.trim().is_empty() || name.lines().count() != 1 {
                        return Err(invalid("Markdown transcript has unsupported preamble"));
                    }
                    title = name.trim().to_owned();
                } else {
                    return Err(invalid("Markdown transcript requires explicit role headers"));
                }
            }
            current = Some(kind);
        } else if current.is_some() {
            body.push_str(line);
        } else {
            preamble.push_str(line);
        }
    }
    if let Some(kind) = current {
        records.push(message(records.len(), kind, body.trim_matches('\n').to_owned(), "markdown")?);
    }
    if records.is_empty() || records.len() > 10_000 {
        return Err(invalid("Markdown transcript requires 1 to 10000 ## Role headers"));
    }
    Ok((title, records))
}

fn unescape_entities(source: &str) -> String {
    source.replace("&nbsp;", " ").replace("&quot;", "\"")
        .replace("&#39;", "'").replace("&apos;", "'")
        .replace("&lt;", "<").replace("&gt;", ">")
        .replace("&amp;", "&")
}

// Only semantic, explicitly role-labelled article/section elements are admitted.
// A generic web page is not a transcript. There is no script execution or URL fetch.
fn html_text(fragment: &str) -> Result<String> {
    let mut out = String::new();
    let mut rest = fragment;
    while !rest.is_empty() {
        if let Some(start) = rest.find('<') {
            out.push_str(&unescape_entities(&rest[..start]));
            let end = rest[start..].find('>')
                .ok_or_else(|| invalid("Unterminated HTML tag"))? + start;
            let tag = rest[start + 1..end].trim().to_ascii_lowercase();
            if tag.starts_with("script") || tag.starts_with("style") || tag.starts_with("iframe")
                || tag.starts_with("object") || tag.starts_with("svg") {
                return Err(invalid("Active HTML content cannot be imported as transcript text"));
            }
            if matches!(tag.as_str(), "br" | "br/" | "/p" | "/div" | "/li" | "/pre") {
                out.push('\n');
            }
            rest = &rest[end + 1..];
        } else {
            out.push_str(&unescape_entities(rest));
            break;
        }
    }
    if out.len() > 4 * 1024 * 1024 {
        return Err(invalid("HTML transcript text exceeds 4 MiB"));
    }
    Ok(out.trim_matches('\n').to_owned())
}

fn html_messages(source: &str) -> Result<(String, Vec<Message>)> {
    let lower = source.to_ascii_lowercase();
    if ["<script", "<style", "<iframe", "<object", "<svg", "<!--", "<template"]
        .iter().any(|marker| lower.contains(marker)) {
        return Err(invalid("HTML transcript contains unsupported active/hidden markup"));
    }
    let prefixes = ["<article data-role=\"", "<section data-role=\""];
    let mut offset = 0;
    let mut records = Vec::new();
    while offset < lower.len() {
        let found = prefixes.iter().filter_map(|prefix| {
            lower[offset..].find(prefix).map(|index| (offset + index, *prefix))
        }).min_by_key(|(index, _)| *index);
        let Some((start, prefix)) = found else { break };
        let role_start = start + prefix.len();
        let role_end = lower[role_start..].find('"')
            .ok_or_else(|| invalid("HTML role attribute is unterminated"))? + role_start;
        let kind = role(&lower[role_start..role_end])
            .ok_or_else(|| invalid("HTML element has unsupported role"))?;
        let tag_end = lower[role_end..].find('>')
            .ok_or_else(|| invalid("HTML element opening tag is unterminated"))? + role_end;
        let close = if prefix.starts_with("<article") { "</article>" } else { "</section>" };
        let close_start = lower[tag_end + 1..].find(close)
            .ok_or_else(|| invalid("HTML transcript role element is not closed"))? + tag_end + 1;
        let inside = &source[tag_end + 1..close_start];
        if prefixes.iter().any(|prefix| inside.to_ascii_lowercase().contains(prefix)) {
            return Err(invalid("Nested transcript records are unsupported"));
        }
        records.push(message(records.len(), kind, html_text(inside)?, "plain")?);
        if records.len() > 10_000 {
            return Err(invalid("HTML transcript exceeds 10000 role elements"));
        }
        offset = close_start + close.len();
    }
    if records.is_empty() {
        return Err(invalid("HTML transcript needs <article data-role=\"user\"> or <section data-role=\"assistant\">"));
    }
    Ok(("Imported HTML transcript".into(), records))
}

fn parse(format: &str, raw: &[u8], sha: &str) -> Result<Capture> {
    let source = std::str::from_utf8(raw)
        .map_err(|_| invalid("Generic transcript is not valid UTF-8"))?;
    let (title, messages) = match format {
        "json" => json_messages(source)?,
        "markdown" => markdown_messages(source)?,
        "html" => html_messages(source)?,
        _ => return Err(invalid("Unsupported generic transcript format")),
    };
    if title.len() > 1024 {
        return Err(invalid("Transcript title exceeds 1024 bytes"));
    }
    Ok(Capture {
        schema_version: 1,
        provider: "generic".into(),
        conversation_id: sha.into(),
        // Internal source locator, not a claim that the imported content came from a website.
        source_url: format!("mindplace-import://generic/{sha}"),
        title,
        conversation_created_at: None,
        conversation_updated_at: None,
        captured_at: chrono::Utc::now().to_rfc3339(),
        capture_method: "generic-import".into(),
        completeness: "partial".into(),
        warnings: vec![
            "Generic transcript import: source bytes retained; no complete history claim.".into(),
            format!("Raw record mapping: ordinal record index, importer generic-{format}/1."),
        ],
        messages,
    })
}

impl ArchiveStore {
    /// Owner-mediated explicit import; accepts bytes rather than paths, never fetches remote URLs.
    /// Retains the original artifact even if parsing/normalization fails.
    pub fn import_generic(&self, format: &str, source: &[u8]) -> Result<GenericImportReceipt> {
        if !matches!(format, "json" | "markdown" | "html") {
            return Err(invalid("Generic import format must be json, markdown, or html"));
        }
        if source.is_empty() || source.len() > MAX_GENERIC_IMPORT_BYTES {
            return Err(invalid("Generic import source must contain 1 byte to 8 MiB"));
        }
        let sha = crate::store::digest(source);
        let imports_root = self.root().join("imports");
        crate::store::checked_dir(&imports_root)?;
        let root = imports_root.join("generic");
        crate::store::checked_dir(&root)?;
        let raw_path = root.join(format!("{sha}.raw"));
        crate::store::no_link(&raw_path)?;
        if raw_path.exists() {
            if crate::store::bounded_read(&raw_path, MAX_GENERIC_IMPORT_BYTES)? != source {
                return Err(invalid("Existing raw source hash content mismatch"));
            }
        } else if let Err(error) = crate::store::atomic_write(&raw_path, source, true) {
            // A concurrent same-hash import may win the immutable creation race.
            if !raw_path.exists()
                || crate::store::bounded_read(&raw_path, MAX_GENERIC_IMPORT_BYTES)? != source {
                return Err(error);
            }
        }
        let mut receipt = GenericImportReceipt {
            format: format.into(),
            raw_sha256: sha.clone(),
            raw_preserved: true,
            saved: None,
            errors: Vec::new(),
        };
        match parse(format, source, &sha).and_then(|capture| self.save(capture)) {
            Ok(saved) => receipt.saved = Some(saved),
            Err(error) => receipt.errors.push(error.to_string().chars().take(400).collect()),
        }
        Ok(receipt)
    }
}
