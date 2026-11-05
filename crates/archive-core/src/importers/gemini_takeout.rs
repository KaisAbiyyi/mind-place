use html5gum::{DefaultEmitter, Token, Tokenizer};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Read;

use crate::importers::{RawEvidenceEnvelope, ReconciledItem};
use crate::{Capture, Message};

const MAX_TAKEOUT_HTML_BYTES: u64 = 64 * 1024 * 1024; // 64 MiB

pub fn parse_takeout_timestamp(s: &str) -> Option<String> {
    let s = s.trim();
    if chrono::DateTime::parse_from_rfc3339(s).is_ok() {
        return Some(s.to_owned());
    }

    // Pattern: "4 Nov 2026, 21:15:40 WIB" or "04 Nov 2026, 21:15:40 GMT"
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.len() < 5 {
        return None;
    }

    let day: u32 = parts[0].trim_matches(',').parse().ok()?;
    let month_str = parts[1].to_ascii_lowercase();
    let month: u32 = match month_str.as_str() {
        "jan" | "januari" | "january" => 1,
        "feb" | "februari" | "february" => 2,
        "mar" | "maret" | "march" => 3,
        "apr" | "april" => 4,
        "may" | "mei" => 5,
        "jun" | "juni" | "june" => 6,
        "jul" | "juli" | "july" => 7,
        "aug" | "agu" | "agustus" | "august" => 8,
        "sep" | "september" => 9,
        "oct" | "okt" | "oktober" | "october" => 10,
        "nov" | "november" => 11,
        "dec" | "des" | "desember" | "december" => 12,
        _ => return None,
    };
    let year: i32 = parts[2].trim_matches(',').parse().ok()?;
    if year < 1970 || year > 2100 {
        return None;
    }

    let time_parts: Vec<&str> = parts[3].trim_matches(',').split(':').collect();
    if time_parts.len() != 3 {
        return None;
    }
    let hour: u32 = time_parts[0].parse().ok()?;
    let minute: u32 = time_parts[1].parse().ok()?;
    let second: u32 = time_parts[2].parse().ok()?;

    let tz_str = parts[4].trim_matches(',').to_ascii_uppercase();
    let tz_offset = match tz_str.as_str() {
        "WIB" => "+07:00",
        "WITA" => "+08:00",
        "WIT" => "+09:00",
        "UTC" | "GMT" | "Z" => "+00:00",
        "PST" => "-08:00",
        "PDT" => "-07:00",
        "EST" => "-05:00",
        "EDT" => "-04:00",
        "CST" => "-06:00",
        "CDT" => "-05:00",
        "MST" => "-07:00",
        "MDT" => "-06:00",
        "BST" => "+01:00",
        "CET" => "+01:00",
        "CEST" => "+02:00",
        "JST" | "KST" => "+09:00",
        "AEST" => "+10:00",
        "AEDT" => "+11:00",
        _ => return None, // Reject unknown timezones rather than assuming UTC
    };

    let formatted =
        format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}{tz_offset}");
    if chrono::DateTime::parse_from_rfc3339(&formatted).is_ok() {
        Some(formatted)
    } else {
        None
    }
}

pub fn strip_html_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        if c == '<' {
            in_tag = true;
        } else if c == '>' {
            in_tag = false;
        } else if !in_tag {
            out.push(c);
        }
    }
    // Decode common HTML entities
    out.replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&#39;", "'")
        .trim()
        .to_string()
}

// HTML5 tokenization supplies decoded attributes and byte spans; evidence is
// always sliced from the original input, never serialized from parsed markup.
fn html_tokens(html: &str) -> impl Iterator<Item = Token<usize>> + '_ {
    let mut emitter = DefaultEmitter::<usize>::new_with_span();
    emitter.naively_switch_states(true);
    Tokenizer::new_with_emitter(html, emitter).map(|token| token.expect("infallible string reader"))
}

fn extract_p_tags(html: &str) -> Vec<String> {
    let mut paragraphs = Vec::new();
    let mut paragraph = None::<String>;
    let mut ignored = 0usize;
    for token in html_tokens(html) {
        match token {
            Token::StartTag(tag) if matches!(tag.name.as_ref(), b"script" | b"style") => {
                ignored += 1
            }
            Token::EndTag(tag) if matches!(tag.name.as_ref(), b"script" | b"style") => {
                ignored = ignored.saturating_sub(1)
            }
            Token::StartTag(tag) if tag.name == b"p" && ignored == 0 => {
                if let Some(text) = paragraph.take().filter(|text| !text.trim().is_empty()) {
                    paragraphs.push(text.trim().to_owned());
                }
                paragraph = Some(String::new());
            }
            Token::EndTag(tag) if tag.name == b"p" && ignored == 0 => {
                if let Some(text) = paragraph.take().filter(|text| !text.trim().is_empty()) {
                    paragraphs.push(text.trim().to_owned());
                }
            }
            Token::StartTag(tag) if tag.name == b"br" && ignored == 0 => {
                if let Some(text) = &mut paragraph {
                    text.push('\n');
                }
            }
            Token::String(text) if ignored == 0 => {
                if let Some(paragraph) = &mut paragraph {
                    paragraph.push_str(&String::from_utf8_lossy(&text));
                }
            }
            _ => {}
        }
    }
    if let Some(text) = paragraph.filter(|text| !text.trim().is_empty()) {
        paragraphs.push(text.trim().to_owned());
    }
    paragraphs
}

fn prompt_markers(html: &str) -> (Vec<usize>, bool) {
    let mut markers = Vec::new();
    // name, opening-tag offset, inherited exclusion, explicit prompt-cell class
    let mut stack = Vec::<(Vec<u8>, usize, bool, bool)>::new();
    let mut prompt_container = None::<(usize, usize, bool)>;
    let mut ambiguous = false;
    let mut at_block_start = true;
    let mut raw_text_start = 0;
    for token in html_tokens(html) {
        match token {
            Token::StartTag(tag) => {
                raw_text_start = tag.span.end;
                let excluded = stack.last().is_some_and(|frame| frame.2)
                    || matches!(
                        tag.name.as_ref(),
                        b"p" | b"pre"
                            | b"code"
                            | b"script"
                            | b"style"
                            | b"textarea"
                            | b"blockquote"
                    )
                    || tag
                        .attributes
                        .get(b"class".as_slice())
                        .is_some_and(|value| {
                            value.split(|byte| byte.is_ascii_whitespace()).any(|class| {
                                class
                                    .windows(8)
                                    .any(|part| part.eq_ignore_ascii_case(b"response"))
                            })
                        });
                if matches!(tag.name.as_ref(), b"div" | b"br" | b"p") {
                    at_block_start = true;
                }
                if !matches!(
                    tag.name.as_ref(),
                    b"area"
                        | b"base"
                        | b"br"
                        | b"col"
                        | b"embed"
                        | b"hr"
                        | b"img"
                        | b"input"
                        | b"link"
                        | b"meta"
                        | b"param"
                        | b"source"
                        | b"track"
                        | b"wbr"
                ) {
                    let prompt_cell =
                        tag.attributes
                            .get(b"class".as_slice())
                            .is_some_and(|value| {
                                value
                                    .split(|byte| byte.is_ascii_whitespace())
                                    .any(|class| class == b"content-cell")
                            });
                    stack.push((tag.name.to_vec(), tag.span.start, excluded, prompt_cell));
                }
            }
            Token::EndTag(tag) => {
                raw_text_start = tag.span.end;
                if let Some(index) = stack
                    .iter()
                    .rposition(|frame| frame.0.as_slice() == tag.name.as_ref())
                {
                    stack.truncate(index);
                }
                if matches!(tag.name.as_ref(), b"div" | b"p") {
                    at_block_start = true;
                }
            }
            Token::Comment(value) => raw_text_start = value.span.end,
            Token::Doctype(value) => raw_text_start = value.span.end,
            Token::String(text) => {
                let value = String::from_utf8_lossy(&text);
                let is_label = at_block_start
                    && value
                        .trim_start()
                        .strip_prefix("Prompted")
                        .is_some_and(|rest| rest.starts_with(char::is_whitespace));
                let direct_prompt_text = stack
                    .last()
                    .is_none_or(|frame| frame.0 == b"div" && !frame.2);
                if is_label && direct_prompt_text {
                    let container = stack.last().map_or(0, |frame| frame.1);
                    let parent = stack
                        .iter()
                        .rev()
                        .skip(1)
                        .find(|frame| frame.0 == b"div")
                        .map_or(0, |frame| frame.1);
                    let prompt_cell = stack.last().is_some_and(|frame| frame.3);
                    // Repeated flat labels are indistinguishable from response prose.
                    // Only distinct peer provider content cells establish further turns.
                    if prompt_container.is_none_or(|prior| {
                        prior.0 != container && prior.1 == parent && prior.2 && prompt_cell
                    }) {
                        // String token spans may cover only the final entity expansion.
                        // Tag spans reliably delimit the original text's starting offset.
                        let raw = &html[raw_text_start..];
                        let trimmed = raw.trim_start();
                        if trimmed.starts_with("Prompted") {
                            markers.push(raw_text_start + raw.len() - trimmed.len());
                            prompt_container = Some((container, parent, prompt_cell));
                        }
                    } else {
                        ambiguous = true;
                    }
                }
                if !value.trim().is_empty() {
                    at_block_start = false;
                }
            }
            _ => {}
        }
    }
    (markers, ambiguous)
}

fn prompt_text(html: &str) -> String {
    let mut text = String::new();
    for token in html_tokens(html) {
        match token {
            Token::StartTag(tag)
                if matches!(tag.name.as_ref(), b"br" | b"p" | b"script" | b"style") =>
            {
                break
            }
            Token::EndTag(tag) if tag.name == b"div" => break,
            Token::String(value) => text.push_str(&String::from_utf8_lossy(&value)),
            _ => {}
        }
    }
    text.strip_prefix("Prompted")
        .unwrap_or(&text)
        .trim()
        .to_owned()
}

pub struct GeminiTakeoutParser;

impl GeminiTakeoutParser {
    pub fn parse_cards_html<R: Read>(
        mut reader: R,
    ) -> Result<Vec<ReconciledItem>, Box<dyn std::error::Error>> {
        let mut html = String::new();
        reader
            .by_ref()
            .take(MAX_TAKEOUT_HTML_BYTES + 1)
            .read_to_string(&mut html)?;
        if html.len() > MAX_TAKEOUT_HTML_BYTES as usize {
            return Err("Gemini Takeout HTML exceeds maximum allowed 64 MiB limit".into());
        }

        let mut items = Vec::new();
        let mut seen_identities: HashMap<String, usize> = HashMap::new();
        let mut div_depth = 0usize;
        let mut active_card = None;
        let mut ranges = Vec::new();
        for token in html_tokens(&html) {
            match token {
                Token::StartTag(tag) if tag.name == b"div" => {
                    let is_card = tag
                        .attributes
                        .get(b"class".as_slice())
                        .is_some_and(|value| {
                            value
                                .split(|byte| byte.is_ascii_whitespace())
                                .any(|class| class == b"outer-cell")
                        });
                    if is_card && active_card.is_none() {
                        active_card = Some((tag.span.start, div_depth));
                    }
                    div_depth += 1;
                }
                Token::EndTag(tag) if tag.name == b"div" => {
                    div_depth = div_depth.saturating_sub(1);
                    if let Some((start, depth)) = active_card {
                        if depth == div_depth {
                            ranges.push(start..tag.span.end);
                            active_card = None;
                        }
                    }
                }
                _ => {}
            }
        }
        if let Some((start, _)) = active_card {
            ranges.push(start..html.len());
        }
        if ranges.is_empty() && !html.trim().is_empty() {
            ranges.push(0..html.len());
        }
        for (idx, range) in ranges.into_iter().enumerate() {
            items.push(Self::parse_single_card(
                &html[range],
                idx,
                &mut seen_identities,
            ));
        }

        Ok(items)
    }

    fn parse_single_card(
        card_html: &str,
        occurrence_index: usize,
        seen_identities: &mut HashMap<String, usize>,
    ) -> ReconciledItem {
        let raw_bytes = card_html.as_bytes().to_vec();
        let raw_sha256 = format!("{:x}", Sha256::digest(&raw_bytes));

        // 1. Validate official Gemini conversation URL if present
        let official_id = Self::extract_valid_gemini_id(card_html);

        // 2. Derive stable, collision-resistant occurrence identity
        let (conversation_id, source_url) = if let Some(ref id) = official_id {
            let count = seen_identities.entry(id.clone()).or_insert(0);
            *count += 1;
            if *count == 1 {
                (id.clone(), format!("https://gemini.google.com/app/{id}"))
            } else {
                let disambiguated = format!("{id}-occ{occurrence_index}");
                (
                    disambiguated.clone(),
                    format!("mindplace-import://gemini-takeout/{disambiguated}"),
                )
            }
        } else {
            let canvas_base = raw_sha256.clone();
            let count = seen_identities.entry(canvas_base.clone()).or_insert(0);
            *count += 1;
            if *count == 1 {
                (
                    canvas_base.clone(),
                    format!("mindplace-import://gemini-canvas/{canvas_base}"),
                )
            } else {
                let disambiguated = format!(
                    "{:x}",
                    Sha256::digest(format!("{canvas_base}:occ:{occurrence_index}").as_bytes())
                );
                (
                    disambiguated.clone(),
                    format!("mindplace-import://gemini-canvas/{disambiguated}"),
                )
            }
        };

        // 3. Structure-aware timestamp extraction
        let mut timestamp = None;
        for line in card_html.lines() {
            let clean = strip_html_tags(line);
            if let Some(ts) = parse_takeout_timestamp(&clean) {
                timestamp = Some(ts);
                break;
            }
        }

        let mut warnings = Vec::new();
        let captured_at = if let Some(ref ts) = timestamp {
            ts.clone()
        } else {
            warnings.push(
                "Provider event timestamp unavailable in Takeout card; using import processing time"
                    .into(),
            );
            chrono::Utc::now().to_rfc3339()
        };

        // 4. Structure-aware turn extraction
        let (messages, completeness, title) = Self::extract_turns(
            card_html,
            &conversation_id,
            timestamp.as_deref(),
            &mut warnings,
        );

        if messages.is_empty() {
            let raw_evidence = RawEvidenceEnvelope {
                provider: "gemini".into(),
                source_identifier: conversation_id,
                envelope_domain: "gemini-takeout-html".into(),
                payload_bytes: raw_bytes,
                payload_sha256: raw_sha256,
                metadata_json: serde_json::json!({
                    "occurrence_index": occurrence_index,
                })
                .to_string(),
            };
            return ReconciledItem::PreservedRawOnly {
                evidence: raw_evidence,
                disposition_reason:
                    "Card contained no extractable user prompt or assistant response text".into(),
                assets: vec![],
            };
        }

        let capture = Capture {
            schema_version: 1,
            provider: "gemini".into(),
            conversation_id: conversation_id.clone(),
            source_url,
            title,
            conversation_created_at: timestamp,
            conversation_updated_at: None,
            captured_at,
            capture_method: "takeout-import".into(),
            completeness,
            warnings,
            messages,
        };

        let raw_evidence = RawEvidenceEnvelope {
            provider: "gemini".into(),
            source_identifier: conversation_id,
            envelope_domain: "gemini-takeout-html".into(),
            payload_bytes: raw_bytes,
            payload_sha256: raw_sha256,
            metadata_json: serde_json::json!({
                "occurrence_index": occurrence_index,
            })
            .to_string(),
        };

        ReconciledItem::Conversation {
            capture,
            raw_evidence,
            assets: vec![],
        }
    }

    fn extract_valid_gemini_id(card_html: &str) -> Option<String> {
        // Only a complete canonical official route can supply an identity.
        // Compare the original spelling as well: URL parsers normalize dot segments.
        fn canonical_id(candidate: &str) -> Option<String> {
            let url = url::Url::parse(candidate).ok()?;
            let id = url.path().strip_prefix("/app/")?;
            if url.scheme() != "https"
                || url.host_str() != Some("gemini.google.com")
                || url.port().is_some()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
                || !(8..=128).contains(&id.len())
                || !id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
                || candidate != format!("https://gemini.google.com/app/{id}")
            {
                return None;
            }
            Some(id.to_owned())
        }
        let mut visible_text = String::new();
        let mut ignored = 0usize;
        for token in html_tokens(card_html) {
            match token {
                Token::StartTag(tag) if matches!(tag.name.as_ref(), b"script" | b"style") => {
                    ignored += 1
                }
                Token::EndTag(tag) if matches!(tag.name.as_ref(), b"script" | b"style") => {
                    ignored = ignored.saturating_sub(1)
                }
                Token::StartTag(tag) if tag.name == b"a" && ignored == 0 => {
                    if let Some(href) = tag.attributes.get(b"href".as_slice()) {
                        if let Some(id) = canonical_id(&String::from_utf8_lossy(href)) {
                            return Some(id);
                        }
                    }
                }
                Token::StartTag(tag) if matches!(tag.name.as_ref(), b"div" | b"p" | b"br") => {
                    visible_text.push(' ')
                }
                Token::EndTag(tag) if matches!(tag.name.as_ref(), b"div" | b"p") => {
                    visible_text.push(' ')
                }
                Token::String(text) if ignored == 0 => {
                    visible_text.push_str(&String::from_utf8_lossy(&text))
                }
                _ => {}
            }
        }
        // Inline markup cannot hide a suffix from complete candidate validation.
        for word in visible_text.split_whitespace() {
            if let Some(id) = canonical_id(word.strip_suffix(':').unwrap_or(word)) {
                return Some(id);
            }
        }
        None
    }

    fn extract_turns(
        card_html: &str,
        conversation_id: &str,
        timestamp: Option<&str>,
        warnings: &mut Vec<String>,
    ) -> (Vec<Message>, String, String) {
        let mut messages = Vec::new();
        let (prompt_markers, ambiguous_prompt_container) = prompt_markers(card_html);
        if ambiguous_prompt_container {
            warnings.push("Ambiguous repeated or unscoped Prompted label retained in raw evidence; no user turn inferred".into());
        }

        if !prompt_markers.is_empty() {
            for (turn_idx, &start) in prompt_markers.iter().enumerate() {
                let end = if turn_idx + 1 < prompt_markers.len() {
                    prompt_markers[turn_idx + 1]
                } else {
                    card_html.len()
                };
                let turn_slice = &card_html[start..end];

                // Extract Prompt
                let prompt_text = prompt_text(turn_slice);

                if !prompt_text.is_empty() {
                    messages.push(Message {
                        id: format!("{conversation_id}-user-{}", turn_idx + 1),
                        role: "user".into(),
                        text: prompt_text,
                        format: "plain".into(),
                        authored_at: timestamp.map(ToOwned::to_owned),
                        model: None,
                        attachments: vec![],
                    });
                }

                // Extract all <p> paragraphs in this turn's slice (supporting attributes like class/style)
                let paragraphs = extract_p_tags(turn_slice);

                if !paragraphs.is_empty() {
                    messages.push(Message {
                        id: format!("{conversation_id}-assistant-{}", turn_idx + 1),
                        role: "assistant".into(),
                        text: paragraphs.join("\n\n"),
                        format: "markdown".into(),
                        authored_at: timestamp.map(ToOwned::to_owned),
                        model: Some("gemini-takeout".into()),
                        attachments: vec![],
                    });
                }
            }
        } else {
            // Response-only card: no "Prompted" marker, but contains assistant paragraphs
            let paragraphs = extract_p_tags(card_html);

            if !paragraphs.is_empty() {
                messages.push(Message {
                    id: format!("{conversation_id}-assistant-1"),
                    role: "assistant".into(),
                    text: paragraphs.join("\n\n"),
                    format: "markdown".into(),
                    authored_at: timestamp.map(ToOwned::to_owned),
                    model: Some("gemini-takeout".into()),
                    attachments: vec![],
                });
                warnings.push("Prompt details omitted in Takeout activity log".into());
            }
        }

        // Determine completeness
        let user_count = messages.iter().filter(|m| m.role == "user").count();
        let assistant_count = messages.iter().filter(|m| m.role == "assistant").count();

        let completeness = if !ambiguous_prompt_container
            && user_count > 0
            && assistant_count > 0
            && user_count == assistant_count
        {
            "complete".to_string()
        } else {
            if user_count > 0 && assistant_count == 0 {
                warnings.push("Assistant response omitted in Takeout activity log".into());
            }
            "partial".to_string()
        };

        // Derive title
        let first_text = messages
            .first()
            .map(|m| m.text.as_str())
            .unwrap_or("Gemini Takeout Import");
        let title: String = first_text.chars().take(80).collect();
        let title = if title.trim().is_empty() {
            "Gemini Takeout Import".to_string()
        } else {
            title
        };

        (messages, completeness, title)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validate_historical_capture;

    #[test]
    fn test_parse_takeout_timestamp_indonesian_wib() {
        let ts = parse_takeout_timestamp("4 Nov 2026, 21:15:40 WIB").unwrap();
        assert_eq!(ts, "2026-11-04T21:15:40+07:00");
    }

    #[test]
    fn test_parse_takeout_timestamp_gmt() {
        let ts = parse_takeout_timestamp("15 Oct 2026, 03:00:00 GMT").unwrap();
        assert_eq!(ts, "2026-10-15T03:00:00+00:00");
    }

    #[test]
    fn test_parse_takeout_timestamp_rejects_unknown_timezone() {
        assert!(parse_takeout_timestamp("4 Nov 2026, 21:15:40 UNKNOWN").is_none());
    }

    #[test]
    fn test_parse_takeout_timestamp_preserves_old_years() {
        let ts = parse_takeout_timestamp("4 Nov 2023, 21:15:40 WIB").unwrap();
        assert_eq!(ts, "2023-11-04T21:15:40+07:00");
    }

    #[test]
    fn test_gemini_takeout_single_card_prompt_and_response() {
        let html = r#"
        <div class="outer-cell mdl-cell mdl-cell--12-col mdl-shadow--2dp">
          <div class="content-cell">
            Prompted Berikan rekomendasi film sci-fi terbaik<br>
            4 Nov 2026, 21:15:40 WIB<br>
            <p>Tentu! Berikut beberapa rekomendasi: Interstellar, Blade Runner 2049, Arrival.</p>
          </div>
          <div class="content-cell mdl-cell--12-col mdl-typography--caption">
            <b>Details:</b> https://gemini.google.com/app/c_takeout123456: <a href="https://gemini.google.com/app/c_takeout123456">Link</a>
          </div>
        </div>
        "#;

        let items = GeminiTakeoutParser::parse_cards_html(html.as_bytes()).unwrap();
        assert_eq!(items.len(), 1);

        match &items[0] {
            ReconciledItem::Conversation {
                capture,
                raw_evidence,
                assets,
            } => {
                assert_eq!(capture.provider, "gemini");
                assert_eq!(capture.conversation_id, "c_takeout123456");
                assert_eq!(
                    capture.source_url,
                    "https://gemini.google.com/app/c_takeout123456"
                );
                assert_eq!(capture.capture_method, "takeout-import");
                assert_eq!(capture.completeness, "complete");
                assert_eq!(capture.messages.len(), 2);
                assert_eq!(capture.messages[0].role, "user");
                assert!(capture.messages[0].text.contains("rekomendasi film sci-fi"));
                assert_eq!(capture.messages[1].role, "assistant");
                assert!(capture.messages[1].text.contains("Interstellar"));
                assert_eq!(capture.captured_at, "2026-11-04T21:15:40+07:00");

                let mut cap = capture.clone();
                assert!(validate_historical_capture(&mut cap, "gemini").is_ok());

                assert_eq!(raw_evidence.provider, "gemini");
                assert_eq!(raw_evidence.envelope_domain, "gemini-takeout-html");
                assert_eq!(assets.len(), 0);
            }
            _ => panic!("Expected Conversation variant"),
        }
    }

    #[test]
    fn test_gemini_takeout_multi_turn_extraction() {
        let html = r#"<div class="outer-cell"><div class="content-cell">Prompted one<br><p>reply one</p></div><div class="content-cell">Prompted two<br><p>reply two</p></div></div>"#;
        let items = GeminiTakeoutParser::parse_cards_html(html.as_bytes()).unwrap();
        assert_eq!(items.len(), 1);

        match &items[0] {
            ReconciledItem::Conversation { capture, .. } => {
                assert_eq!(capture.messages.len(), 4);
                assert_eq!(capture.messages[0].role, "user");
                assert_eq!(capture.messages[0].text, "one");
                assert_eq!(capture.messages[1].role, "assistant");
                assert_eq!(capture.messages[1].text, "reply one");
                assert_eq!(capture.messages[2].role, "user");
                assert_eq!(capture.messages[2].text, "two");
                assert_eq!(capture.messages[3].role, "assistant");
                assert_eq!(capture.messages[3].text, "reply two");
                assert_eq!(capture.completeness, "complete");
            }
            _ => panic!("Expected Conversation variant"),
        }
    }

    #[test]
    fn test_gemini_takeout_response_only_honest_projection() {
        let html = r#"<div class="outer-cell"><p>Only assistant text exists</p></div>"#;
        let items = GeminiTakeoutParser::parse_cards_html(html.as_bytes()).unwrap();
        assert_eq!(items.len(), 1);

        match &items[0] {
            ReconciledItem::Conversation { capture, .. } => {
                assert_eq!(capture.messages.len(), 1);
                assert_eq!(capture.messages[0].role, "assistant");
                assert_eq!(capture.messages[0].text, "Only assistant text exists");
                assert_eq!(capture.completeness, "partial");
                assert!(capture
                    .warnings
                    .iter()
                    .any(|w| w.contains("Prompt details omitted")));
            }
            _ => panic!("Expected Conversation variant"),
        }
    }

    #[test]
    fn test_gemini_canvas_untitled_content_hash_identity() {
        let html = r#"
        <div class="outer-cell mdl-cell mdl-cell--12-col mdl-shadow--2dp">
          <div class="content-cell">
            Prompted Canvas code snippet draft<br>
            4 Nov 2026, 22:00:00 WIB<br>
            <p>```python\ndef solve(): return 42\n```</p>
          </div>
        </div>
        "#;

        let items = GeminiTakeoutParser::parse_cards_html(html.as_bytes()).unwrap();
        assert_eq!(items.len(), 1);

        match &items[0] {
            ReconciledItem::Conversation {
                capture,
                raw_evidence,
                ..
            } => {
                assert_eq!(capture.provider, "gemini");
                assert_eq!(capture.conversation_id, raw_evidence.payload_sha256);
                assert!(capture
                    .source_url
                    .starts_with("mindplace-import://gemini-canvas/"));
                assert_eq!(capture.capture_method, "takeout-import");

                let mut cap = capture.clone();
                assert!(validate_historical_capture(&mut cap, "gemini").is_ok());
            }
            _ => panic!("Expected Conversation variant"),
        }
    }

    #[test]
    fn test_gemini_duplicate_card_occurrence_identity_non_collision() {
        let card = r#"<div class="outer-cell">Prompted same<br><p>reply same</p></div>"#;
        let items =
            GeminiTakeoutParser::parse_cards_html(format!("{card}{card}").as_bytes()).unwrap();
        assert_eq!(items.len(), 2);
        let ids: Vec<_> = items
            .iter()
            .map(|x| match x {
                ReconciledItem::Conversation { capture, .. } => capture.conversation_id.clone(),
                _ => panic!(),
            })
            .collect();
        assert_ne!(ids[0], ids[1]);
    }

    #[test]
    fn test_gemini_takeout_prompt_only_honest_projection() {
        let html = r#"<div class="outer-cell">Prompted Only user question exists<br></div>"#;
        let items = GeminiTakeoutParser::parse_cards_html(html.as_bytes()).unwrap();
        assert_eq!(items.len(), 1);

        match &items[0] {
            ReconciledItem::Conversation { capture, .. } => {
                assert_eq!(capture.messages.len(), 1);
                assert_eq!(capture.messages[0].role, "user");
                assert_eq!(capture.messages[0].text, "Only user question exists");
                assert_eq!(capture.completeness, "partial");
                assert!(capture
                    .warnings
                    .iter()
                    .any(|w| w.contains("Assistant response omitted")));
            }
            _ => panic!("Expected Conversation variant"),
        }
    }

    #[test]
    fn test_gemini_takeout_nested_material_design_markup() {
        let html = r#"
        <div class="outer-cell mdl-cell mdl-cell--12-col mdl-shadow--2dp">
          <div class="header-cell mdl-cell mdl-cell--12-col">
            <div class="content-cell mdl-cell--6-col">
              Prompted <span class="query-text">Nested query text</span><br>
              4 Nov 2026, 21:15:40 WIB<br>
            </div>
            <div class="response-wrapper">
              <p class="first-para">First response paragraph with <b>bold</b> text.</p>
              <p class="second-para">Second response paragraph with <i>italics</i>.</p>
            </div>
          </div>
        </div>
        "#;
        let items = GeminiTakeoutParser::parse_cards_html(html.as_bytes()).unwrap();
        assert_eq!(items.len(), 1);

        match &items[0] {
            ReconciledItem::Conversation { capture, .. } => {
                assert_eq!(capture.messages.len(), 2);
                assert_eq!(capture.messages[0].text, "Nested query text");
                assert!(capture.messages[1]
                    .text
                    .contains("First response paragraph"));
                assert!(capture.messages[1]
                    .text
                    .contains("Second response paragraph"));
                assert_eq!(capture.completeness, "complete");
            }
            _ => panic!("Expected Conversation variant"),
        }
    }

    #[test]
    fn test_gemini_takeout_malicious_url_fallback_to_canvas() {
        let html = r#"
        <div class="outer-cell">
          Prompted Test<br>
          <a href="https://gemini.google.com/app/../../etc/passwd">Link</a>
          <p>Response</p>
        </div>
        "#;
        let items = GeminiTakeoutParser::parse_cards_html(html.as_bytes()).unwrap();
        assert_eq!(items.len(), 1);

        match &items[0] {
            ReconciledItem::Conversation { capture, .. } => {
                // Must NOT use path traversal as conversation ID! Falls back to canvas content-hash
                assert!(!capture.conversation_id.contains(".."));
                assert!(capture
                    .source_url
                    .starts_with("mindplace-import://gemini-canvas/"));
            }
            _ => panic!("Expected Conversation variant"),
        }
    }

    #[test]
    fn test_empty_malformed_card_preserves_raw_only() {
        let html = r#"
        <div class="outer-cell mdl-cell mdl-cell--12-col mdl-shadow--2dp">
          <div class="content-cell">
            <!-- Completely empty card without text -->
          </div>
        </div>
        "#;

        let items = GeminiTakeoutParser::parse_cards_html(html.as_bytes()).unwrap();
        assert_eq!(items.len(), 1);

        match &items[0] {
            ReconciledItem::PreservedRawOnly {
                evidence,
                disposition_reason,
                ..
            } => {
                assert_eq!(evidence.provider, "gemini");
                assert!(disposition_reason.contains("no extractable user prompt"));
            }
            _ => panic!("Expected PreservedRawOnly variant"),
        }
    }
}
