use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::io::Read;

use crate::importers::{stream_top_level_array, RawEvidenceEnvelope, ReconciledItem};
use crate::{Capture, Message};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ChatGptExportConversation {
    pub id: String,
    pub title: Option<String>,
    pub create_time: Option<f64>,
    pub update_time: Option<f64>,
    pub current_node: Option<String>,
    pub mapping: HashMap<String, ChatGptNode>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ChatGptNode {
    pub id: String,
    pub message: Option<ChatGptMessage>,
    pub parent: Option<String>,
    #[serde(default)]
    pub children: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ChatGptMessage {
    pub id: String,
    pub author: ChatGptAuthor,
    pub create_time: Option<f64>,
    pub update_time: Option<f64>,
    pub content: Option<ChatGptContent>,
    pub status: Option<String>,
    pub end_turn: Option<bool>,
    pub weight: Option<f64>,
    pub metadata: Option<serde_json::Value>,
    pub recipient: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ChatGptAuthor {
    pub role: String,
    pub name: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ChatGptContent {
    pub content_type: String,
    pub parts: Option<Vec<serde_json::Value>>,
    pub text: Option<String>,
}

pub struct ChatGptExportParser;

impl ChatGptExportParser {
    pub fn parse_conversations_json<R: Read>(
        reader: R,
    ) -> Result<Vec<ReconciledItem>, Box<dyn std::error::Error>> {
        let mut items = Vec::new();
        stream_top_level_array(reader, |conv: ChatGptExportConversation| {
            let item = Self::reconcile_conversation(conv)?;
            items.push(item);
            Ok(())
        })?;
        Ok(items)
    }

    pub fn reconcile_conversation(
        conv: ChatGptExportConversation,
    ) -> Result<ReconciledItem, Box<dyn std::error::Error>> {
        let raw_bytes = serde_json::to_vec(&conv)?;
        let raw_sha256 = format!("{:x}", Sha256::digest(&raw_bytes));

        let raw_evidence = RawEvidenceEnvelope {
            provider: "chatgpt".into(),
            source_identifier: conv.id.clone(),
            envelope_domain: "chatgpt-export-dag".into(),
            payload_bytes: raw_bytes,
            payload_sha256: raw_sha256,
            metadata_json: serde_json::json!({
                "original_node_count": conv.mapping.len(),
                "has_current_node": conv.current_node.is_some(),
            })
            .to_string(),
        };

        let messages = Self::project_active_branch(&conv);
        if messages.is_empty() {
            return Ok(ReconciledItem::PreservedRawOnly {
                evidence: raw_evidence,
                disposition_reason:
                    "ChatGPT export conversation contained zero valid message nodes".into(),
                assets: vec![],
            });
        }

        let title = conv
            .title
            .as_deref()
            .map(|t| t.trim())
            .filter(|t| !t.is_empty())
            .unwrap_or("ChatGPT Export Conversation")
            .chars()
            .take(120)
            .collect();

        let captured_at = conv
            .create_time
            .or(conv.update_time)
            .and_then(timestamp_to_rfc3339)
            .unwrap_or_else(|| "2026-10-15T03:00:00.000Z".to_string());

        let source_url = format!("https://chatgpt.com/c/{}", conv.id);

        let capture = Capture {
            schema_version: 1,
            provider: "chatgpt".into(),
            conversation_id: conv.id.to_ascii_lowercase(),
            source_url,
            title,
            conversation_created_at: conv.create_time.and_then(timestamp_to_rfc3339),
            conversation_updated_at: conv.update_time.and_then(timestamp_to_rfc3339),
            captured_at,
            capture_method: "export-import".into(),
            completeness: "complete".into(),
            warnings: vec![],
            messages,
        };

        Ok(ReconciledItem::Conversation {
            capture,
            raw_evidence,
            assets: vec![],
        })
    }

    fn project_active_branch(conv: &ChatGptExportConversation) -> Vec<Message> {
        let mut ordered_messages = Vec::new();
        let mut visited = HashSet::new();

        if let Some(mut curr_id) = conv.current_node.clone() {
            let mut path_nodes = Vec::new();
            while let Some(node) = conv.mapping.get(&curr_id) {
                if !visited.insert(curr_id.clone()) {
                    // Prevent infinite loops on cyclic mapping
                    break;
                }
                if let Some(msg) = &node.message {
                    path_nodes.push(msg);
                }
                if let Some(parent_id) = &node.parent {
                    curr_id = parent_id.clone();
                } else {
                    break;
                }
            }
            path_nodes.reverse(); // Chronological order: root -> leaf
            for msg in path_nodes {
                if let Some(converted) = Self::convert_message(msg) {
                    ordered_messages.push(converted);
                }
            }
        } else {
            // Fallback: traverse from roots
            let roots: Vec<_> = conv
                .mapping
                .values()
                .filter(|n| n.parent.is_none())
                .collect();
            for root in roots {
                let mut curr = Some(root);
                while let Some(node) = curr {
                    if !visited.insert(node.id.clone()) {
                        break;
                    }
                    if let Some(msg) = &node.message {
                        if let Some(converted) = Self::convert_message(msg) {
                            ordered_messages.push(converted);
                        }
                    }
                    curr = node
                        .children
                        .first()
                        .and_then(|c_id| conv.mapping.get(c_id));
                }
            }
        }

        // Deduplicate message IDs if identical IDs appear
        let mut seen_ids = HashSet::new();
        for (idx, msg) in ordered_messages.iter_mut().enumerate() {
            if !seen_ids.insert(msg.id.clone()) {
                msg.id = format!("{}-{idx}", msg.id);
            }
        }

        ordered_messages
    }

    fn convert_message(raw: &ChatGptMessage) -> Option<Message> {
        let role = match raw.author.role.as_str() {
            "user" => "user",
            "assistant" => "assistant",
            "system" => "system",
            "tool" => "tool",
            _ => "tool",
        };

        let mut text = String::new();
        if let Some(content) = &raw.content {
            if let Some(parts) = &content.parts {
                for part in parts {
                    match part {
                        serde_json::Value::String(s) => {
                            text.push_str(s);
                        }
                        serde_json::Value::Object(map) => {
                            if let Some(serde_json::Value::String(s)) = map.get("text") {
                                text.push_str(s);
                            } else {
                                text.push_str(&part.to_string());
                            }
                        }
                        _ => {
                            text.push_str(&part.to_string());
                        }
                    }
                }
            } else if let Some(t) = &content.text {
                text.push_str(t);
            }
        }

        // Skip completely empty system prompts or placeholder root nodes
        if text.trim().is_empty() && role == "system" {
            return None;
        }

        let format = if text.contains("```") || text.contains("**") || text.contains('#') {
            "markdown".to_string()
        } else {
            "plain".to_string()
        };

        Some(Message {
            id: raw.id.clone(),
            role: role.to_string(),
            text,
            format,
            authored_at: raw.create_time.and_then(timestamp_to_rfc3339),
            model: raw
                .metadata
                .as_ref()
                .and_then(|m| m.get("model_slug"))
                .and_then(|s| s.as_str())
                .map(|s| s.to_string()),
            attachments: vec![],
        })
    }
}

fn timestamp_to_rfc3339(ts: f64) -> Option<String> {
    let secs = ts as i64;
    let nsecs = ((ts - secs as f64) * 1_000_000_000.0) as u32;
    chrono::DateTime::from_timestamp(secs, nsecs).map(|dt| dt.to_rfc3339())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validate_historical_capture;

    #[test]
    fn test_chatgpt_linear_conversation() {
        let json = br#"[
            {
                "id": "12345678-1234-4123-8123-123456789abc",
                "title": "Quantum Computing",
                "create_time": 1700000000.0,
                "update_time": 1700000010.0,
                "current_node": "node-2",
                "mapping": {
                    "node-1": {
                        "id": "node-1",
                        "parent": null,
                        "children": ["node-2"],
                        "message": {
                            "id": "msg-1",
                            "author": {"role": "user"},
                            "create_time": 1700000000.0,
                            "content": {"content_type": "text", "parts": ["What is superposition?"]}
                        }
                    },
                    "node-2": {
                        "id": "node-2",
                        "parent": "node-1",
                        "children": [],
                        "message": {
                            "id": "msg-2",
                            "author": {"role": "assistant"},
                            "create_time": 1700000005.0,
                            "content": {"content_type": "text", "parts": ["Superposition is a principle of quantum mechanics..."]},
                            "metadata": {"model_slug": "gpt-4"}
                        }
                    }
                }
            }
        ]"#;

        let items = ChatGptExportParser::parse_conversations_json(&json[..]).unwrap();
        assert_eq!(items.len(), 1);

        match &items[0] {
            ReconciledItem::Conversation {
                capture,
                raw_evidence,
                ..
            } => {
                assert_eq!(capture.provider, "chatgpt");
                assert_eq!(
                    capture.conversation_id,
                    "12345678-1234-4123-8123-123456789abc"
                );
                assert_eq!(
                    capture.source_url,
                    "https://chatgpt.com/c/12345678-1234-4123-8123-123456789abc"
                );
                assert_eq!(capture.title, "Quantum Computing");
                assert_eq!(capture.capture_method, "export-import");
                assert_eq!(capture.completeness, "complete");
                assert_eq!(capture.messages.len(), 2);
                assert_eq!(capture.messages[0].role, "user");
                assert_eq!(capture.messages[0].text, "What is superposition?");
                assert_eq!(capture.messages[1].role, "assistant");
                assert_eq!(capture.messages[1].model.as_deref(), Some("gpt-4"));

                let mut cap = capture.clone();
                assert!(validate_historical_capture(&mut cap, "chatgpt").is_ok());

                assert_eq!(raw_evidence.provider, "chatgpt");
                assert_eq!(raw_evidence.envelope_domain, "chatgpt-export-dag");
            }
            _ => panic!("Expected Conversation variant"),
        }
    }

    #[test]
    fn test_chatgpt_branching_dag_projects_active_branch_only() {
        let json = br#"[
            {
                "id": "abcdef12-3456-7890-abcd-ef1234567890",
                "title": "Branching Story",
                "create_time": 1700000000.0,
                "current_node": "node-branch-b",
                "mapping": {
                    "node-root": {
                        "id": "node-root",
                        "parent": null,
                        "children": ["node-branch-a", "node-branch-b"],
                        "message": {
                            "id": "msg-root",
                            "author": {"role": "user"},
                            "content": {"content_type": "text", "parts": ["Write a story"]}
                        }
                    },
                    "node-branch-a": {
                        "id": "node-branch-a",
                        "parent": "node-root",
                        "children": [],
                        "message": {
                            "id": "msg-a",
                            "author": {"role": "assistant"},
                            "content": {"content_type": "text", "parts": ["Alternative A: Once upon a time in space..."]}
                        }
                    },
                    "node-branch-b": {
                        "id": "node-branch-b",
                        "parent": "node-root",
                        "children": [],
                        "message": {
                            "id": "msg-b",
                            "author": {"role": "assistant"},
                            "content": {"content_type": "text", "parts": ["Alternative B: Once upon a time in the ocean..."]}
                        }
                    }
                }
            }
        ]"#;

        let items = ChatGptExportParser::parse_conversations_json(&json[..]).unwrap();
        assert_eq!(items.len(), 1);

        match &items[0] {
            ReconciledItem::Conversation {
                capture,
                raw_evidence,
                ..
            } => {
                // Projection must contain root -> branch-b (2 messages), NOT branch-a!
                assert_eq!(capture.messages.len(), 2);
                assert_eq!(capture.messages[0].id, "msg-root");
                assert_eq!(capture.messages[1].id, "msg-b");
                assert!(capture.messages[1].text.contains("Alternative B"));

                // But the raw evidence envelope preserves ALL nodes (including branch-a)!
                let raw_str = String::from_utf8(raw_evidence.payload_bytes.clone()).unwrap();
                assert!(raw_str.contains("Alternative A"));
                assert!(raw_str.contains("Alternative B"));
            }
            _ => panic!("Expected Conversation variant"),
        }
    }

    #[test]
    fn test_chatgpt_cyclic_mapping_does_not_infinite_loop() {
        let json = br#"[
            {
                "id": "11112222-3333-4444-5555-666677778888",
                "title": "Cyclic Graph",
                "create_time": 1700000000.0,
                "current_node": "cycle-1",
                "mapping": {
                    "cycle-1": {
                        "id": "cycle-1",
                        "parent": "cycle-2",
                        "children": ["cycle-2"],
                        "message": {
                            "id": "msg-c1",
                            "author": {"role": "user"},
                            "content": {"content_type": "text", "parts": ["Looping"]}
                        }
                    },
                    "cycle-2": {
                        "id": "cycle-2",
                        "parent": "cycle-1",
                        "children": ["cycle-1"],
                        "message": {
                            "id": "msg-c2",
                            "author": {"role": "assistant"},
                            "content": {"content_type": "text", "parts": ["Infinite"]}
                        }
                    }
                }
            }
        ]"#;

        // Must complete safely without hanging or stack overflowing!
        let items = ChatGptExportParser::parse_conversations_json(&json[..]).unwrap();
        assert_eq!(items.len(), 1);
    }
}
