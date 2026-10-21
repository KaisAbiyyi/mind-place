//! Resource-bounded extraction of untrusted document bytes.
//! No path, permission, archive, UI or indexing authority is granted by this crate.
//! An authorized caller must revalidate its own read lease before and after extraction.

mod ooxml;
mod pdf;

use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

const CHUNK_BYTES: usize = 4096;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentFormat {
    PlainText,
    Markdown,
    Source,
    Pdf,
    Docx,
    Pptx,
    Xlsx,
}

impl DocumentFormat {
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.trim_start_matches('.').to_ascii_lowercase().as_str() {
            "txt" | "log" | "csv" | "tsv" => Some(Self::PlainText),
            "md" | "mdx" | "markdown" => Some(Self::Markdown),
            "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "json" | "toml" | "yaml"
            | "yml" | "html" | "css" | "sql" | "go" | "java" | "c" | "cpp"
            | "h" | "hpp" | "sh" | "ps1" | "rb" | "php" => Some(Self::Source),
            "pdf" => Some(Self::Pdf),
            "docx" => Some(Self::Docx),
            "pptx" => Some(Self::Pptx),
            "xlsx" => Some(Self::Xlsx),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct ExtractionLimits {
    pub max_input_bytes: usize,
    pub max_expanded_bytes: usize,
    pub max_output_bytes: usize,
    pub max_chunks: usize,
    pub max_entries: usize,
    pub max_pdf_objects: usize,
    pub max_pages: usize,
    pub max_xml_depth: usize,
    pub max_millis: u64,
}

impl Default for ExtractionLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: 16 * 1024 * 1024,
            max_expanded_bytes: 32 * 1024 * 1024,
            max_output_bytes: 2 * 1024 * 1024,
            max_chunks: 1024,
            max_entries: 512,
            max_pdf_objects: 10_000,
            max_pages: 250,
            max_xml_depth: 96,
            max_millis: 2_000,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExtractedChunk {
    pub ordinal: u32,
    /// A source-relative section hint, not an authoritative page or file identity.
    pub section: String,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExtractedDocument {
    pub format: DocumentFormat,
    pub text: String,
    pub chunks: Vec<ExtractedChunk>,
    /// Non-authoritative, public parser capability caveats; never filesystem paths.
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ExtractionError {
    #[error("unsupported document encoding, container or feature")]
    Unsupported,
    #[error("malformed or damaged document")]
    Invalid,
    #[error("document resource limit exceeded: {0}")]
    Limit(&'static str),
    #[error("document extraction cancelled or authorization revoked")]
    Revoked,
}

pub type Result<T> = std::result::Result<T, ExtractionError>;

/// Standalone, pure byte extraction; does not open paths or grant read authority.
pub fn extract_bytes(
    format: DocumentFormat,
    input: &[u8],
    limits: ExtractionLimits,
) -> Result<ExtractedDocument> {
    extract_with_check(format, input, limits, || true)
}

/// The caller must supply a synchronous live-authorization/cancellation check.
/// On any error the entire extraction is discarded; partial text is never returned.
pub fn extract_with_check(
    format: DocumentFormat,
    input: &[u8],
    limits: ExtractionLimits,
    mut authorized: impl FnMut() -> bool,
) -> Result<ExtractedDocument> {
    if limits.max_input_bytes == 0
        || limits.max_expanded_bytes == 0
        || limits.max_output_bytes == 0
        || limits.max_chunks == 0
        || limits.max_entries == 0
        || limits.max_pdf_objects == 0
        || limits.max_pages == 0
        || limits.max_xml_depth == 0
        || limits.max_millis == 0
    {
        return Err(ExtractionError::Limit("configuration"));
    }
    if input.len() > limits.max_input_bytes {
        return Err(ExtractionError::Limit("input_bytes"));
    }
    let mut ctx = Context {
        limits,
        started: Instant::now(),
        authorized: &mut authorized,
        expanded: 0,
        output_bytes: 0,
        chunks: Vec::new(),
        warnings: Vec::new(),
    };
    ctx.poll()?;
    match format {
        DocumentFormat::PlainText | DocumentFormat::Markdown | DocumentFormat::Source => {
            parse_utf8(&mut ctx, input, format)?;
        }
        DocumentFormat::Pdf => pdf::extract_pdf(&mut ctx, input)?,
        DocumentFormat::Docx | DocumentFormat::Pptx | DocumentFormat::Xlsx => {
            ooxml::extract_ooxml(&mut ctx, input, format)?;
        }
    }
    ctx.poll()?;
    let text = ctx
        .chunks
        .iter()
        .map(|chunk| chunk.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    Ok(ExtractedDocument {
        format,
        text,
        chunks: ctx.chunks,
        warnings: ctx.warnings,
    })
}

pub(crate) struct Context<'a> {
    pub limits: ExtractionLimits,
    started: Instant,
    authorized: &'a mut dyn FnMut() -> bool,
    expanded: usize,
    output_bytes: usize,
    chunks: Vec<ExtractedChunk>,
    warnings: Vec<String>,
}

impl Context<'_> {
    pub fn poll(&mut self) -> Result<()> {
        if !(self.authorized)() {
            return Err(ExtractionError::Revoked);
        }
        if self.started.elapsed() > Duration::from_millis(self.limits.max_millis) {
            return Err(ExtractionError::Limit("elapsed_millis"));
        }
        Ok(())
    }

    pub fn expanded(&mut self, bytes: usize) -> Result<()> {
        self.expanded = self
            .expanded
            .checked_add(bytes)
            .ok_or(ExtractionError::Limit("expanded_bytes"))?;
        if self.expanded > self.limits.max_expanded_bytes {
            return Err(ExtractionError::Limit("expanded_bytes"));
        }
        self.poll()
    }

    pub fn warn(&mut self, warning: &str) {
        if !self.warnings.iter().any(|w| w == warning) {
            self.warnings.push(warning.to_owned());
        }
    }

    pub fn emit(&mut self, section: &str, value: &str) -> Result<()> {
        let value = value.trim();
        if value.is_empty() {
            return Ok(());
        }
        if value.contains('\0') {
            return Err(ExtractionError::Invalid);
        }
        let next = self
            .output_bytes
            .checked_add(value.len())
            .and_then(|n| n.checked_add(1))
            .ok_or(ExtractionError::Limit("output_bytes"))?;
        if next > self.limits.max_output_bytes {
            return Err(ExtractionError::Limit("output_bytes"));
        }
        let mut start = 0;
        while start < value.len() {
            self.poll()?;
            let mut end = (start + CHUNK_BYTES).min(value.len());
            while !value.is_char_boundary(end) {
                end -= 1;
            }
            if end == start {
                return Err(ExtractionError::Invalid);
            }
            if self.chunks.len() >= self.limits.max_chunks {
                return Err(ExtractionError::Limit("chunks"));
            }
            self.chunks.push(ExtractedChunk {
                ordinal: self.chunks.len() as u32,
                section: section.to_owned(),
                text: value[start..end].to_owned(),
            });
            start = end;
        }
        self.output_bytes = next;
        Ok(())
    }
}

fn parse_utf8(ctx: &mut Context<'_>, input: &[u8], format: DocumentFormat) -> Result<()> {
    ctx.expanded(input.len())?;
    let raw = std::str::from_utf8(input).map_err(|_| ExtractionError::Unsupported)?;
    let raw = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    if raw.contains('\0') {
        return Err(ExtractionError::Invalid);
    }
    let mut section = String::from("Document");
    let mut buffer = String::new();
    for (line_number, line) in raw.lines().enumerate() {
        if line_number % 64 == 0 {
            ctx.poll()?;
        }
        let heading = format == DocumentFormat::Markdown
            && line.starts_with('#')
            && line.trim_start_matches('#').starts_with(' ');
        if heading && !buffer.trim().is_empty() {
            ctx.emit(&section, &buffer)?;
            buffer.clear();
        }
        if heading {
            section = line.trim().chars().take(160).collect();
        }
        if buffer.len() + line.len() + 1 > CHUNK_BYTES && !buffer.is_empty() {
            ctx.emit(&section, &buffer)?;
            buffer.clear();
        }
        buffer.push_str(line);
        buffer.push('\n');
        if buffer.len() > CHUNK_BYTES * 8 {
            ctx.emit(&section, &buffer)?;
            buffer.clear();
        }
    }
    ctx.emit(&section, &buffer)?;
    Ok(())
}
