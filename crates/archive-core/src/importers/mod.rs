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

/// Where a raw record sits in the source member it was cut from. `sha256` is computed by the
/// extraction layer from the bytes it actually sliced, independently of the parser's own claims.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecordSpan {
    pub offset: u64,
    pub length: u64,
    pub sha256: String,
}

impl RecordSpan {
    pub fn of(offset: u64, bytes: &[u8]) -> Self {
        use sha2::{Digest, Sha256};
        Self {
            offset,
            length: bytes.len() as u64,
            sha256: format!("{:x}", Sha256::digest(bytes)),
        }
    }
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

/// Hard limits applied while a top-level JSON array is read from an untrusted stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoundedArrayLimits {
    /// Largest accepted array element, in bytes. Reading stops the moment a record
    /// would exceed this size, before the record is buffered or parsed.
    pub max_record_bytes: usize,
    /// Largest run of consecutive whitespace between structural tokens. This stops a
    /// stream that never produces a record from being read forever.
    pub max_separator_bytes: usize,
}

/// Reads one element at a time from a top-level JSON array without ever holding more than
/// `limits.max_record_bytes` of a single record. Elements are handed to `callback` as the
/// exact original bytes (surrounding whitespace excluded), so evidence stays byte-faithful.
///
/// Scope of the guarantee: this enforces structure (one array, comma separated, nothing
/// after the closing bracket) and size. It does not validate the element contents; the
/// caller parses each record with `serde_json`, which also rejects mismatched brackets.
pub fn stream_bounded_top_level_array<R, F>(
    reader: R,
    limits: BoundedArrayLimits,
    mut callback: F,
) -> Result<usize, Box<dyn std::error::Error>>
where
    R: std::io::Read,
    F: FnMut(&[u8]) -> Result<(), Box<dyn std::error::Error>>,
{
    stream_bounded_top_level_array_spanned(reader, limits, |_offset, record| callback(record))
}

/// Like [`stream_bounded_top_level_array`], and additionally reports the absolute byte offset
/// of each element's first byte in the stream. The offset and the element's length let a caller
/// bind a record to the exact source bytes it was cut from.
pub fn stream_bounded_top_level_array_spanned<R, F>(
    mut reader: R,
    limits: BoundedArrayLimits,
    mut callback: F,
) -> Result<usize, Box<dyn std::error::Error>>
where
    R: std::io::Read,
    F: FnMut(u64, &[u8]) -> Result<(), Box<dyn std::error::Error>>,
{
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Phase {
        /// Before the opening `[`.
        Start,
        /// After `[` or `,`: an element (or, only right after `[`, the closing `]`) is next.
        BeforeElement,
        /// Inside an element.
        Element,
        /// After an element: `,` or `]` is next.
        AfterElement,
        /// After the closing `]`: only whitespace is allowed.
        End,
    }
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Kind {
        Container,
        Text,
        Scalar,
    }

    let mut phase = Phase::Start;
    let mut kind = Kind::Container;
    let mut after_comma = false;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    let mut separator_run = 0usize;
    let mut record = Vec::new();
    let mut record_start = 0u64;
    let mut count = 0usize;
    let mut chunk = vec![0u8; 64 * 1024];
    // Bytes consumed before the current chunk.
    let mut base = 0u64;

    loop {
        let read = match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(Box::new(error)),
        };
        let mut index = 0;
        let chunk_base = base;
        base += read as u64;
        while index < read {
            let byte = chunk[index];
            let is_space = byte.is_ascii_whitespace();

            if phase == Phase::Element
                && kind == Kind::Scalar
                && (is_space || byte == b',' || byte == b']')
            {
                // A scalar has no closing token: a delimiter ends it and is then re-read
                // under the next phase.
                callback(record_start, &record)?;
                count += 1;
                record.clear();
                phase = Phase::AfterElement;
                separator_run = 0;
                continue;
            }
            index += 1;

            if phase != Phase::Element && is_space {
                separator_run += 1;
                if separator_run > limits.max_separator_bytes {
                    return Err("JSON array whitespace run exceeds the allowed limit".into());
                }
                continue;
            }

            match phase {
                Phase::End => return Err("Unexpected data after the top-level JSON array".into()),
                Phase::Start => {
                    if byte != b'[' {
                        return Err("Expected a top-level JSON array".into());
                    }
                    phase = Phase::BeforeElement;
                    after_comma = false;
                    separator_run = 0;
                }
                Phase::AfterElement => {
                    separator_run = 0;
                    match byte {
                        b',' => {
                            phase = Phase::BeforeElement;
                            after_comma = true;
                        }
                        b']' => phase = Phase::End,
                        _ => return Err("Malformed JSON array: expected ',' or ']'".into()),
                    }
                }
                Phase::BeforeElement | Phase::Element => {
                    if phase == Phase::BeforeElement {
                        separator_run = 0;
                        match byte {
                            b']' if after_comma => {
                                return Err("Malformed JSON array: trailing comma".into())
                            }
                            b']' => {
                                phase = Phase::End;
                                continue;
                            }
                            b',' => return Err("Malformed JSON array: empty element".into()),
                            _ => {}
                        }
                        kind = match byte {
                            b'{' | b'[' => Kind::Container,
                            b'"' => Kind::Text,
                            _ => Kind::Scalar,
                        };
                        phase = Phase::Element;
                        depth = 0;
                        in_string = false;
                        escaped = false;
                        record.clear();
                        // `index` was already advanced past this element's first byte.
                        record_start = chunk_base + index as u64 - 1;
                    }
                    if record.len() >= limits.max_record_bytes {
                        return Err(format!(
                            "Import record exceeds the {} byte limit",
                            limits.max_record_bytes
                        )
                        .into());
                    }
                    record.push(byte);

                    let mut finished = false;
                    if in_string {
                        if escaped {
                            escaped = false;
                        } else if byte == b'\\' {
                            escaped = true;
                        } else if byte == b'"' {
                            in_string = false;
                            finished = kind == Kind::Text;
                        }
                    } else if byte == b'"' {
                        in_string = true;
                    } else if kind == Kind::Container {
                        match byte {
                            b'{' | b'[' => depth += 1,
                            b'}' | b']' => {
                                depth = depth.saturating_sub(1);
                                finished = depth == 0;
                            }
                            _ => {}
                        }
                    }
                    if finished {
                        callback(record_start, &record)?;
                        count += 1;
                        record.clear();
                        phase = Phase::AfterElement;
                    }
                }
            }
        }
    }

    if phase == Phase::End {
        Ok(count)
    } else {
        Err("Unexpected end of input inside the top-level JSON array".into())
    }
}

/// Reads a top-level array through serde. This buffers each record completely before any
/// size check is possible, so it is **not** suitable for untrusted input; use
/// [`stream_bounded_top_level_array`] for provider exports.
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
    deserializer
        .end()
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
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
    fn test_stream_top_level_array_rejects_trailing_garbage() {
        let json = b"[] NOT_JSON";
        let res = stream_top_level_array(&json[..], |_: serde_json::Value| Ok(()));
        assert!(res.is_err());
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

    const LIMITS: BoundedArrayLimits = BoundedArrayLimits {
        max_record_bytes: 64,
        max_separator_bytes: 16,
    };

    fn collect(input: &[u8], limits: BoundedArrayLimits) -> Result<Vec<Vec<u8>>, String> {
        let mut out = Vec::new();
        stream_bounded_top_level_array(input, limits, |record| {
            out.push(record.to_vec());
            Ok(())
        })
        .map(|count| {
            assert_eq!(count, out.len());
            out
        })
        .map_err(|error| error.to_string())
    }

    #[test]
    fn bounded_stream_returns_exact_original_record_bytes() {
        let input = b" [ {\"a\" : 1,\"b\":[1, 2]} ,\"x,]\\\"y\" , 42 , null,[ ]]\n";
        let records = collect(input, LIMITS).unwrap();
        assert_eq!(
            records,
            vec![
                b"{\"a\" : 1,\"b\":[1, 2]}".to_vec(),
                b"\"x,]\\\"y\"".to_vec(),
                b"42".to_vec(),
                b"null".to_vec(),
                b"[ ]".to_vec(),
            ]
        );
        assert_eq!(collect(b"[]", LIMITS).unwrap().len(), 0);
        assert_eq!(collect(b"  [\n]\n", LIMITS).unwrap().len(), 0);
    }

    #[test]
    fn bounded_stream_is_independent_of_read_chunking() {
        struct OneByte<'a>(&'a [u8]);
        impl std::io::Read for OneByte<'_> {
            fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
                match self.0.split_first() {
                    Some((byte, rest)) if !out.is_empty() => {
                        out[0] = *byte;
                        self.0 = rest;
                        Ok(1)
                    }
                    _ => Ok(0),
                }
            }
        }
        let input = b"[{\"k\":\"v\"},7,\"s\"]";
        let mut out = Vec::new();
        stream_bounded_top_level_array(OneByte(input), LIMITS, |record| {
            out.push(record.to_vec());
            Ok(())
        })
        .unwrap();
        assert_eq!(
            out,
            vec![b"{\"k\":\"v\"}".to_vec(), b"7".to_vec(), b"\"s\"".to_vec()]
        );
    }

    #[test]
    fn bounded_stream_rejects_an_oversized_record_before_buffering_it() {
        // Exactly at the limit passes; one byte more fails.
        let at_limit = format!("[\"{}\"]", "a".repeat(LIMITS.max_record_bytes - 2));
        assert!(collect(at_limit.as_bytes(), LIMITS).is_ok());
        let over = format!("[\"{}\"]", "a".repeat(LIMITS.max_record_bytes - 1));
        let error = collect(over.as_bytes(), LIMITS).unwrap_err();
        assert!(error.contains("byte limit"), "{error}");
    }

    #[test]
    fn bounded_stream_stops_reading_an_endless_record() {
        // An unterminated record must fail after the limit, not drain the reader.
        struct Endless {
            served: usize,
        }
        impl std::io::Read for Endless {
            fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
                let prefix: &[u8] = if self.served == 0 { b"[\"" } else { b"" };
                let mut written = 0;
                for byte in prefix.iter().copied().chain(std::iter::repeat(b'a')) {
                    if written == out.len() {
                        break;
                    }
                    out[written] = byte;
                    written += 1;
                }
                self.served += written;
                Ok(written)
            }
        }
        let mut reader = Endless { served: 0 };
        let result = stream_bounded_top_level_array(&mut reader, LIMITS, |_| Ok(()));
        assert!(result.is_err());
        // One read chunk beyond the limit at most: bounded, not unbounded.
        assert!(reader.served <= 64 * 1024 + 2, "served {}", reader.served);
    }

    #[test]
    fn bounded_stream_rejects_whitespace_floods_and_never_recurses_on_nesting() {
        let flood = format!("[{}1]", " ".repeat(LIMITS.max_separator_bytes + 1));
        assert!(collect(flood.as_bytes(), LIMITS).is_err());
        let trailing = format!("[1]{}", " ".repeat(LIMITS.max_separator_bytes + 1));
        assert!(collect(trailing.as_bytes(), LIMITS).is_err());
        // Nesting is bounded by the record limit, and serde_json's own recursion limit
        // rejects it when parsed; the stream itself never recurses.
        let deep = format!("[{}{}]", "[".repeat(30), "]".repeat(30));
        assert_eq!(collect(deep.as_bytes(), LIMITS).unwrap().len(), 1);
    }

    #[test]
    fn bounded_stream_rejects_malformed_structure() {
        for bad in [
            &b""[..],
            b"   ",
            b"{}",
            b"[",
            b"[{\"a\":1}",
            b"[1,]",
            b"[,1]",
            b"[1 2]",
            b"[1,,2]",
            b"[1] x",
            b"[1]]",
            b"\xef\xbb\xbf[1]",
            b"[\"unterminated]",
        ] {
            assert!(
                collect(bad, LIMITS).is_err(),
                "accepted {:?}",
                String::from_utf8_lossy(bad)
            );
        }
    }

    #[test]
    fn bounded_stream_propagates_callback_errors() {
        let result = stream_bounded_top_level_array(&b"[1,2]"[..], LIMITS, |_| Err("stop".into()));
        assert_eq!(result.unwrap_err().to_string(), "stop");
    }

    #[test]
    fn test_stream_top_level_array_rejects_object() {
        let json = br#"{"id": 1, "title": "Not an array"}"#;
        let res = stream_top_level_array(&json[..], |_item: DummyRecord| Ok(()));
        assert!(res.is_err());
    }
}
