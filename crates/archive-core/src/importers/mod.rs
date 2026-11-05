use serde::de::{self, Deserializer, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::marker::PhantomData;
use std::path::PathBuf;

pub mod chatgpt_export;
pub mod gemini_takeout;

use crate::Capture;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImportSourceInventory {
    pub provider: String,
    pub source_root: PathBuf,
    pub total_files: usize,
    pub total_bytes: u64,
    pub manifest_sha256: [u8; 32],
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RawEvidenceEnvelope {
    pub provider: String,
    pub source_identifier: String,
    pub envelope_domain: String,
    pub payload_bytes: Vec<u8>,
    pub payload_sha256: String,
    pub metadata_json: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveredAsset {
    pub provider_file_id: String,
    pub relative_path: PathBuf,
    pub declared_name: String,
    pub verified_mime: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum ReconciledItem {
    Conversation {
        capture: Capture,
        raw_evidence: RawEvidenceEnvelope,
        assets: Vec<DiscoveredAsset>,
    },
    PreservedRawOnly {
        evidence: RawEvidenceEnvelope,
        disposition_reason: String,
        assets: Vec<DiscoveredAsset>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct BatchImportReceipt {
    pub total_records_scanned: usize,
    pub conversations_imported: usize,
    pub raw_only_preserved: usize,
    pub skipped_unchanged: usize,
    pub failed: usize,
    pub assets_ingested: usize,
    pub assets_reused: usize,
    pub warnings: Vec<String>,
}

pub fn stream_top_level_array<R, T, F>(
    reader: R,
    mut callback: F,
) -> Result<usize, Box<dyn std::error::Error>>
where
    R: std::io::Read,
    T: for<'de> Deserialize<'de>,
    F: FnMut(T) -> Result<(), Box<dyn std::error::Error>>,
{
    let mut deserializer = serde_json::Deserializer::from_reader(reader);

    struct ArrayVisitor<'a, Item, Handler> {
        callback: &'a mut Handler,
        count: usize,
        _marker: PhantomData<Item>,
    }

    impl<'de, 'a, Item, Handler> Visitor<'de> for ArrayVisitor<'a, Item, Handler>
    where
        Item: Deserialize<'de>,
        Handler: FnMut(Item) -> Result<(), Box<dyn std::error::Error>>,
    {
        type Value = usize;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a top-level JSON array")
        }

        fn visit_seq<A>(mut self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            while let Some(element) = seq.next_element::<Item>()? {
                (self.callback)(element).map_err(de::Error::custom)?;
                self.count += 1;
            }
            Ok(self.count)
        }
    }

    let visitor = ArrayVisitor {
        callback: &mut callback,
        count: 0,
        _marker: PhantomData,
    };

    let total = deserializer.deserialize_seq(visitor)?;
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Deserialize, PartialEq, Eq)]
    struct DummyRecord {
        id: u64,
        title: String,
    }

    #[test]
    fn test_stream_top_level_array_empty() {
        let json = b"[]";
        let mut count = 0;
        let total = stream_top_level_array(&json[..], |_item: DummyRecord| {
            count += 1;
            Ok(())
        })
        .unwrap();
        assert_eq!(total, 0);
        assert_eq!(count, 0);
    }

    #[test]
    fn test_stream_top_level_array_multiple_items() {
        let json = br#"[
            {"id": 1, "title": "First"},
            {"id": 2, "title": "Second"},
            {"id": 3, "title": "Third"}
        ]"#;

        let mut collected = Vec::new();
        let total = stream_top_level_array(&json[..], |item: DummyRecord| {
            collected.push(item);
            Ok(())
        })
        .unwrap();

        assert_eq!(total, 3);
        assert_eq!(collected.len(), 3);
        assert_eq!(collected[0].id, 1);
        assert_eq!(collected[1].title, "Second");
        assert_eq!(collected[2].id, 3);
    }

    #[test]
    fn test_stream_top_level_array_rejects_object() {
        let json = br#"{"id": 1, "title": "Not an array"}"#;
        let res = stream_top_level_array(&json[..], |_item: DummyRecord| Ok(()));
        assert!(res.is_err());
    }
}
