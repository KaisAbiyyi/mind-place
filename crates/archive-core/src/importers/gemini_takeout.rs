use sha2::{Digest, Sha256};
use std::io::Read;

use crate::importers::{RawEvidenceEnvelope, ReconciledItem};
use crate::{Capture, Message};

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

    let time_parts: Vec<&str> = parts[3].trim_matches(',').split(':').collect();
    if time_parts.len() != 3 {
        return None;
    }
    let hour: u32 = time_parts[0].parse().ok()?;
    let minute: u32 = time_parts[1].parse().ok()?;
    let second: u32 = time_parts[2].parse().ok()?;

    let tz_str = parts[4].to_ascii_uppercase();
    let tz_offset = match tz_str.as_str() {
        "WIB" => "+07:00",
        "WITA" => "+08:00",
        "WIT" => "+09:00",
        "UTC" | "GMT" | "Z" => "+00:00",
        "PST" => "-08:00",
        "PDT" => "-07:00",
        "EST" => "-05:00",
        "EDT" => "-04:00",
        _ => "+00:00",
    };

    let formatted =
        format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}{tz_offset}");
    if chrono::DateTime::parse_from_rfc3339(&formatted).is_ok() {
        Some(formatted)
    } else {
        None
    }
}

fn strip_html_tags(s: &str) -> String {
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

pub struct GeminiTakeoutParser;

impl GeminiTakeoutParser {
    pub fn parse_cards_html<R: Read>(
        mut reader: R,
    ) -> Result<Vec<ReconciledItem>, Box<dyn std::error::Error>> {
        let mut html = String::new();
        reader.read_to_string(&mut html)?;

        let mut items = Vec::new();
        let card_markers: Vec<_> = html
            .match_indices("<div class=\"outer-cell")
            .map(|(i, _)| i)
            .collect();

        if card_markers.is_empty() {
            // Check if entire HTML is a single snippet or canvas without outer-cell wrapper
            if !html.trim().is_empty() {
                let item = Self::parse_single_card(&html);
                items.push(item);
            }
            return Ok(items);
        }

        for (idx, &start) in card_markers.iter().enumerate() {
            let end = if idx + 1 < card_markers.len() {
                card_markers[idx + 1]
            } else {
                html.len()
            };
            let card_slice = &html[start..end];
            let item = Self::parse_single_card(card_slice);
            items.push(item);
        }

        Ok(items)
    }

    fn parse_single_card(card_html: &str) -> ReconciledItem {
        let raw_bytes = card_html.as_bytes().to_vec();
        let raw_sha256 = format!("{:x}", Sha256::digest(&raw_bytes));

        // 1. Extract session URL if present
        let session_url = if let Some(pos) = card_html.find("https://gemini.google.com/app/") {
            let rest = &card_html[pos..];
            let end = rest
                .find(|c: char| c.is_whitespace() || c == '<' || c == '"' || c == '\'')
                .unwrap_or(rest.len());
            let url = rest[..end].trim_end_matches(|c: char| c == '/' || c == ':');
            Some(url.to_string())
        } else {
            None
        };

        // 2. Extract conversation ID
        let (conversation_id, source_url) = if let Some(ref url) = session_url {
            let id = url
                .strip_prefix("https://gemini.google.com/app/")
                .unwrap_or(&raw_sha256);
            (id.to_string(), url.clone())
        } else {
            // Gemini Canvas or untitled snippet: deterministic exact content-hash
            (
                raw_sha256.clone(),
                format!("mindplace-import://gemini-canvas/{raw_sha256}"),
            )
        };

        // 3. Extract timestamp
        let mut timestamp = None;
        for line in card_html.lines() {
            let trimmed = line.trim();
            // Look for date tokens
            if (trimmed.contains("WIB") || trimmed.contains("UTC") || trimmed.contains("GMT"))
                && (trimmed.contains("2024")
                    || trimmed.contains("2025")
                    || trimmed.contains("2026")
                    || trimmed.contains("2027"))
            {
                let clean = strip_html_tags(trimmed);
                if let Some(ts) = parse_takeout_timestamp(&clean) {
                    timestamp = Some(ts);
                    break;
                }
            }
        }
        let captured_at = timestamp.unwrap_or_else(|| "2026-10-15T03:00:00.000Z".to_string());

        // 4. Extract Prompt text
        let mut prompt_text = String::new();
        if let Some(pos) = card_html.find("Prompted") {
            let rest = &card_html[pos + "Prompted".len()..];
            let end = rest
                .find("<br>")
                .or_else(|| rest.find("</div>"))
                .unwrap_or(rest.len());
            prompt_text = strip_html_tags(&rest[..end]);
        }

        // 5. Extract Response text
        let mut response_text = String::new();
        if let Some(p_start) = card_html.find("<p>") {
            let rest = &card_html[p_start + 3..];
            if let Some(p_end) = rest.find("</p>") {
                response_text = strip_html_tags(&rest[..p_end]);
            }
        }

        // If both prompt and response are completely empty, preserve as raw only
        if prompt_text.is_empty() && response_text.is_empty() {
            let raw_evidence = RawEvidenceEnvelope {
                provider: "gemini".into(),
                source_identifier: conversation_id,
                envelope_domain: "gemini-takeout-html".into(),
                payload_bytes: raw_bytes,
                payload_sha256: raw_sha256,
                metadata_json: "{}".into(),
            };
            return ReconciledItem::PreservedRawOnly {
                evidence: raw_evidence,
                disposition_reason:
                    "Card contained no extractable user prompt or assistant response text".into(),
                assets: vec![],
            };
        }

        // Default prompt if only response was present (or vice-versa)
        if prompt_text.is_empty() {
            prompt_text = "Prompt details omitted in Takeout activity log".to_string();
        }

        let mut messages = Vec::new();
        messages.push(Message {
            id: format!("{conversation_id}-user-1"),
            role: "user".into(),
            text: prompt_text.clone(),
            format: "plain".into(),
            authored_at: Some(captured_at.clone()),
            model: None,
            attachments: vec![],
        });

        let completeness = if !response_text.is_empty() {
            messages.push(Message {
                id: format!("{conversation_id}-assistant-1"),
                role: "assistant".into(),
                text: response_text,
                format: "markdown".into(),
                authored_at: Some(captured_at.clone()),
                model: Some("gemini-takeout".into()),
                attachments: vec![],
            });
            "complete".to_string()
        } else {
            "partial".to_string()
        };

        let title: String = prompt_text.chars().take(80).collect();
        let title = if title.trim().is_empty() {
            "Gemini Takeout Import".to_string()
        } else {
            title
        };

        let capture = Capture {
            schema_version: 1,
            provider: "gemini".into(),
            conversation_id: conversation_id.clone(),
            source_url,
            title,
            conversation_created_at: Some(captured_at.clone()),
            conversation_updated_at: None,
            captured_at,
            capture_method: "takeout-import".into(),
            completeness,
            warnings: vec![],
            messages,
        };

        let raw_evidence = RawEvidenceEnvelope {
            provider: "gemini".into(),
            source_identifier: conversation_id,
            envelope_domain: "gemini-takeout-html".into(),
            payload_bytes: raw_bytes,
            payload_sha256: raw_sha256,
            metadata_json: "{}".into(),
        };

        ReconciledItem::Conversation {
            capture,
            raw_evidence,
            assets: vec![],
        }
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

                // Verify with historical capture validator
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
