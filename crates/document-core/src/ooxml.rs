//! Strict single-level ZIP/OOXML reader. Never extracts files to disk.
//! Only allowlisted document XML parts are parsed; external relationships are ignored.

use crate::{Context, DocumentFormat, ExtractionError, Result};
use crc32fast::Hasher;
use flate2::read::DeflateDecoder;
use quick_xml::events::Event;
use quick_xml::Reader;
use std::io::Read;

#[derive(Debug)]
struct Entry<'a> {
    name: &'a str,
    method: u16,
    compressed: usize,
    expanded: usize,
    crc32: u32,
    local_offset: usize,
}

fn u16at(bytes: &[u8], at: usize) -> Result<u16> {
    let part = bytes.get(at..at + 2).ok_or(ExtractionError::Invalid)?;
    Ok(u16::from_le_bytes([part[0], part[1]]))
}

fn u32at(bytes: &[u8], at: usize) -> Result<u32> {
    let part = bytes.get(at..at + 4).ok_or(ExtractionError::Invalid)?;
    Ok(u32::from_le_bytes([part[0], part[1], part[2], part[3]]))
}

fn inside(bytes: &[u8], start: usize, count: usize) -> Result<&[u8]> {
    let end = start.checked_add(count).ok_or(ExtractionError::Invalid)?;
    bytes.get(start..end).ok_or(ExtractionError::Invalid)
}

fn central_directory<'a>(ctx: &mut Context<'_>, bytes: &'a [u8]) -> Result<Vec<Entry<'a>>> {
    if bytes.len() < 22 || &bytes[..2] != b"PK" {
        return Err(ExtractionError::Invalid);
    }
    let min_at = bytes.len().saturating_sub(65_557);
    let eocd = (min_at..=bytes.len() - 22)
        .rev()
        .find(|&p| {
            &bytes[p..p + 4] == b"PK\x05\x06"
                && u16at(bytes, p + 20).is_ok_and(|n| p + 22 + usize::from(n) == bytes.len())
        })
        .ok_or(ExtractionError::Invalid)?;
    if u16at(bytes, eocd + 4)? != 0 || u16at(bytes, eocd + 6)? != 0 {
        return Err(ExtractionError::Unsupported);
    }
    let count = usize::from(u16at(bytes, eocd + 10)?);
    if count != usize::from(u16at(bytes, eocd + 8)?) {
        return Err(ExtractionError::Unsupported);
    }
    if count > ctx.limits.max_entries {
        return Err(ExtractionError::Limit("zip_entries"));
    }
    let dir_size = u32at(bytes, eocd + 12)? as usize;
    let dir_start = u32at(bytes, eocd + 16)? as usize;
    if dir_size == u32::MAX as usize || dir_start == u32::MAX as usize || count == u16::MAX as usize
    {
        return Err(ExtractionError::Unsupported); // ZIP64 is intentionally excluded.
    }
    let dir_end = dir_start
        .checked_add(dir_size)
        .ok_or(ExtractionError::Invalid)?;
    if dir_end > eocd {
        return Err(ExtractionError::Invalid);
    }
    let mut position = dir_start;
    let mut entries = Vec::with_capacity(count);
    let mut expanded_total = 0usize;
    for _ in 0..count {
        ctx.poll()?;
        if u32at(bytes, position)? != 0x0201_4b50 {
            return Err(ExtractionError::Invalid);
        }
        let flags = u16at(bytes, position + 8)?;
        if flags & 1 != 0 {
            return Err(ExtractionError::Unsupported); // No encrypted archives.
        }
        let method = u16at(bytes, position + 10)?;
        let crc32 = u32at(bytes, position + 16)?;
        let compressed = u32at(bytes, position + 20)? as usize;
        let expanded = u32at(bytes, position + 24)? as usize;
        let name_size = usize::from(u16at(bytes, position + 28)?);
        let extra_size = usize::from(u16at(bytes, position + 30)?);
        let comment_size = usize::from(u16at(bytes, position + 32)?);
        let local_offset = u32at(bytes, position + 42)? as usize;
        if compressed == u32::MAX as usize
            || expanded == u32::MAX as usize
            || local_offset == u32::MAX as usize
        {
            return Err(ExtractionError::Unsupported);
        }
        let name = std::str::from_utf8(inside(bytes, position + 46, name_size)?)
            .map_err(|_| ExtractionError::Unsupported)?;
        if name.is_empty()
            || name.starts_with('/')
            || name.contains('\\')
            || name.contains(':')
            || name.contains('\0')
            || name
                .split('/')
                .any(|p| p == "." || p == ".." || p.is_empty())
        {
            return Err(ExtractionError::Invalid);
        }
        expanded_total = expanded_total
            .checked_add(expanded)
            .ok_or(ExtractionError::Limit("expanded_bytes"))?;
        if expanded_total > ctx.limits.max_expanded_bytes {
            return Err(ExtractionError::Limit("expanded_bytes"));
        }
        entries.push(Entry {
            name,
            method,
            compressed,
            expanded,
            crc32,
            local_offset,
        });
        position = position
            .checked_add(46)
            .and_then(|n| n.checked_add(name_size))
            .and_then(|n| n.checked_add(extra_size))
            .and_then(|n| n.checked_add(comment_size))
            .ok_or(ExtractionError::Invalid)?;
        if position > dir_end {
            return Err(ExtractionError::Invalid);
        }
    }
    if position != dir_end {
        return Err(ExtractionError::Invalid);
    }
    entries.sort_by(|a, b| a.name.cmp(b.name));
    if entries.windows(2).any(|pair| pair[0].name == pair[1].name) {
        return Err(ExtractionError::Invalid); // No ambiguous duplicate entries.
    }
    Ok(entries)
}

fn is_target(format: DocumentFormat, name: &str) -> bool {
    match format {
        DocumentFormat::Docx => {
            name == "word/document.xml"
                || name.starts_with("word/header") && name.ends_with(".xml")
                || name.starts_with("word/footer") && name.ends_with(".xml")
                || name == "word/footnotes.xml"
        }
        DocumentFormat::Pptx => {
            name.starts_with("ppt/slides/slide")
                && name.ends_with(".xml")
                && name["ppt/slides/slide".len()..name.len() - 4]
                    .chars()
                    .all(|c| c.is_ascii_digit())
        }
        DocumentFormat::Xlsx => {
            name == "xl/sharedStrings.xml"
                || name.starts_with("xl/worksheets/sheet")
                    && name.ends_with(".xml")
                    && name["xl/worksheets/sheet".len()..name.len() - 4]
                        .chars()
                        .all(|c| c.is_ascii_digit())
        }
        _ => false,
    }
}

fn uncompress(ctx: &mut Context<'_>, source: &[u8], entry: &Entry<'_>) -> Result<Vec<u8>> {
    let offset = entry.local_offset;
    if u32at(source, offset)? != 0x0403_4b50 {
        return Err(ExtractionError::Invalid);
    }
    if u16at(source, offset + 8)? != entry.method {
        return Err(ExtractionError::Invalid);
    }
    let local_name_size = usize::from(u16at(source, offset + 26)?);
    let local_extra_size = usize::from(u16at(source, offset + 28)?);
    let local_name = inside(source, offset + 30, local_name_size)?;
    if local_name != entry.name.as_bytes() {
        return Err(ExtractionError::Invalid);
    }
    let data_start = offset
        .checked_add(30)
        .and_then(|n| n.checked_add(local_name_size))
        .and_then(|n| n.checked_add(local_extra_size))
        .ok_or(ExtractionError::Invalid)?;
    let compressed = inside(source, data_start, entry.compressed)?;
    let mut output = Vec::with_capacity(entry.expanded.min(1024 * 1024));
    match entry.method {
        0 => {
            if compressed.len() != entry.expanded {
                return Err(ExtractionError::Invalid);
            }
            ctx.expanded(compressed.len())?;
            output.extend_from_slice(compressed);
        }
        8 => {
            let mut reader = DeflateDecoder::new(compressed);
            let mut buffer = [0u8; 8192];
            loop {
                ctx.poll()?;
                let n = reader
                    .read(&mut buffer)
                    .map_err(|_| ExtractionError::Invalid)?;
                if n == 0 {
                    break;
                }
                let next = output
                    .len()
                    .checked_add(n)
                    .ok_or(ExtractionError::Limit("expanded_bytes"))?;
                if next > entry.expanded {
                    return Err(ExtractionError::Invalid);
                }
                ctx.expanded(n)?;
                output.extend_from_slice(&buffer[..n]);
            }
        }
        _ => return Err(ExtractionError::Unsupported),
    }
    if output.len() != entry.expanded {
        return Err(ExtractionError::Invalid);
    }
    let mut crc = Hasher::new();
    crc.update(&output);
    if crc.finalize() != entry.crc32 {
        return Err(ExtractionError::Invalid);
    }
    Ok(output)
}

fn parse_xml(ctx: &mut Context<'_>, bytes: &[u8]) -> Result<String> {
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(false);
    let mut scratch = Vec::new();
    let mut depth = 0usize;
    let mut capturing: Option<usize> = None;
    let mut output = String::new();
    loop {
        ctx.poll()?;
        let event = reader
            .read_event_into(&mut scratch)
            .map_err(|_| ExtractionError::Invalid)?;
        match event {
            Event::Start(e) => {
                depth += 1;
                if depth > ctx.limits.max_xml_depth {
                    return Err(ExtractionError::Limit("xml_depth"));
                }
                let name = e.name();
                let local = name
                    .as_ref()
                    .rsplit(|c| *c == b':')
                    .next()
                    .unwrap_or_default();
                if local == b"t" || local == b"v" {
                    capturing = Some(depth);
                }
            }
            Event::End(e) => {
                if depth == 0 {
                    return Err(ExtractionError::Invalid);
                }
                let name = e.name();
                let local = name
                    .as_ref()
                    .rsplit(|c| *c == b':')
                    .next()
                    .unwrap_or_default();
                if matches!(local, b"p" | b"row" | b"si" | b"tr") {
                    output.push('\n');
                }
                if capturing == Some(depth) {
                    capturing = None;
                    output.push(' ');
                }
                depth -= 1;
            }
            Event::Text(e) if capturing.is_some() => {
                let raw = std::str::from_utf8(e.as_ref()).map_err(|_| ExtractionError::Invalid)?;
                let decoded =
                    quick_xml::escape::unescape(raw).map_err(|_| ExtractionError::Invalid)?;
                output.push_str(&decoded);
            }
            Event::CData(e) if capturing.is_some() => {
                output.push_str(
                    std::str::from_utf8(e.as_ref()).map_err(|_| ExtractionError::Invalid)?,
                );
            }
            Event::DocType(_) | Event::GeneralRef(_) => {
                return Err(ExtractionError::Unsupported);
            }
            Event::Eof => break,
            _ => {}
        }
        if output.len() > ctx.limits.max_output_bytes {
            return Err(ExtractionError::Limit("output_bytes"));
        }
        scratch.clear();
    }
    if depth != 0 {
        return Err(ExtractionError::Invalid);
    }
    Ok(output)
}

pub(super) fn extract_ooxml(
    ctx: &mut Context<'_>,
    bytes: &[u8],
    format: DocumentFormat,
) -> Result<()> {
    let entries = central_directory(ctx, bytes)?;
    let mut matched = 0usize;
    for entry in entries {
        if !is_target(format, entry.name) {
            continue;
        }
        ctx.poll()?;
        matched += 1;
        let xml = uncompress(ctx, bytes, &entry)?;
        let text = parse_xml(ctx, &xml)?;
        ctx.emit(entry.name, &text)?;
    }
    if matched == 0 {
        return Err(ExtractionError::Unsupported);
    }
    if format == DocumentFormat::Xlsx {
        ctx.warn("Spreadsheet shared strings and sheet cells are extracted as separate sections; formula results are not recalculated.");
    }
    if format == DocumentFormat::Pptx {
        ctx.warn("Slides are ordered by part name; media, notes and speaker order are not reconstructed.");
    }
    Ok(())
}
