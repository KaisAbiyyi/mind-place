//! Conservative, bounded PDF text-operator reader.
//! Supports visible literal/hex text in plain or Flate-compressed content streams.
//! Font CMaps, images, encryption, object streams and OCR are not interpreted.
//! Never claims complete semantic/page extraction for arbitrary PDFs.

use crate::{Context, ExtractionError, Result};
use flate2::read::ZlibDecoder;
use std::io::Read;

fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    haystack
        .get(from..)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|p| p + from)
}

fn string_literal(bytes: &[u8], start: usize) -> Result<(Vec<u8>, usize)> {
    let mut out = Vec::new();
    let mut depth = 1usize;
    let mut at = start + 1;
    while at < bytes.len() {
        let c = bytes[at];
        at += 1;
        match c {
            b'(' => {
                depth += 1;
                out.push(c);
            }
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Ok((out, at));
                }
                out.push(c);
            }
            b'\\' => {
                let escape = *bytes.get(at).ok_or(ExtractionError::Invalid)?;
                at += 1;
                match escape {
                    b'n' => out.push(b'\n'),
                    b'r' => out.push(b'\r'),
                    b't' => out.push(b'\t'),
                    b'b' => out.push(8),
                    b'f' => out.push(12),
                    b'\r' => {
                        if bytes.get(at) == Some(&b'\n') {
                            at += 1;
                        }
                    }
                    b'\n' => {}
                    b'0'..=b'7' => {
                        let mut value = u32::from(escape - b'0');
                        for _ in 0..2 {
                            match bytes.get(at) {
                                Some(b'0'..=b'7') => {
                                    value = value * 8 + u32::from(bytes[at] - b'0');
                                    at += 1;
                                }
                                _ => break,
                            }
                        }
                        out.push(value as u8);
                    }
                    _ => out.push(escape),
                }
            }
            _ => out.push(c),
        }
    }
    Err(ExtractionError::Invalid)
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn hex_literal(bytes: &[u8], start: usize) -> Result<(Vec<u8>, usize)> {
    let mut digits = Vec::new();
    let mut at = start + 1;
    while let Some(&b) = bytes.get(at) {
        at += 1;
        if b == b'>' {
            if digits.len() % 2 != 0 {
                digits.push(0);
            }
            let mut out = Vec::with_capacity(digits.len() / 2);
            for pair in digits.chunks_exact(2) {
                out.push((pair[0] << 4) | pair[1]);
            }
            return Ok((out, at));
        }
        if b.is_ascii_whitespace() {
            continue;
        }
        digits.push(hex_digit(b).ok_or(ExtractionError::Invalid)?);
    }
    Err(ExtractionError::Invalid)
}

fn decode_text(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return String::from_utf16_lossy(
            &bytes[2..]
                .chunks_exact(2)
                .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
                .collect::<Vec<_>>(),
        );
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return String::from_utf16_lossy(
            &bytes[2..]
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect::<Vec<_>>(),
        );
    }
    // ASCII and Latin-1 text survive, while nontrivial glyph/CMap remapping
    // is deliberately not guessed. The warning on the document communicates it.
    bytes
        .iter()
        .filter_map(|&b| match b {
            0 => None,
            9 | 10 | 13 => Some(' '),
            32..=126 => Some(char::from(b)),
            160..=255 => Some(char::from(b)),
            _ => None,
        })
        .collect()
}

fn delimiter(b: u8) -> bool {
    b.is_ascii_whitespace() || b"()<>[]{}/%".contains(&b)
}

fn content_text(ctx: &mut Context<'_>, data: &[u8]) -> Result<String> {
    let mut output = String::new();
    let mut cursor = 0usize;
    let mut last_auth = 0usize;
    let mut events = 0usize;
    ctx.poll()?;
    while let Some(start) = find(data, b"BT", cursor) {
        if (start > 0 && !delimiter(data[start - 1]))
            || data.get(start + 2).is_some_and(|b| !delimiter(*b))
        {
            cursor = start + 2;
            continue;
        }
        ctx.check_time()?;
        let block = &data[start + 2..];
        let mut pos = 0;
        while pos < block.len() {
            let absolute = start + 2 + pos;
            if absolute.saturating_sub(last_auth) >= 8192 || events >= 256 {
                ctx.poll()?;
                last_auth = absolute;
                events = 0;
            } else {
                ctx.check_time()?;
            }
            events += 1;
            // ET inside a literal/hex string is evidence, not an operator.
            if block[pos..].starts_with(b"ET")
                && (pos == 0 || delimiter(block[pos - 1]))
                && block.get(pos + 2).is_none_or(|b| delimiter(*b))
            {
                pos += 2;
                break;
            }
            match block[pos] {
                b'%' => {
                    while pos < block.len() && !matches!(block[pos], b'\r' | b'\n') {
                        pos += 1;
                    }
                }
                b'(' => {
                    let (literal, end) = string_literal(block, pos)?;
                    let value = decode_text(&literal);
                    if !value.is_empty() {
                        output.push_str(&value);
                        output.push(' ');
                    }
                    pos = end;
                }
                b'<' if block.get(pos + 1) == Some(&b'<') => {
                    pos += 2;
                }
                b'<' => {
                    let (literal, end) = hex_literal(block, pos)?;
                    let value = decode_text(&literal);
                    if !value.is_empty() {
                        output.push_str(&value);
                        output.push(' ');
                    }
                    pos = end;
                }
                _ => pos += 1,
            }
            if output.len() > ctx.limits.max_output_bytes {
                return Err(ExtractionError::Limit("output_bytes"));
            }
        }
        cursor = start + 2 + pos;
    }
    ctx.poll()?;
    Ok(output)
}

fn inflate_stream(ctx: &mut Context<'_>, compressed: &[u8]) -> Result<Vec<u8>> {
    let mut reader = ZlibDecoder::new(compressed);
    let mut output = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        ctx.poll()?;
        let count = reader
            .read(&mut buffer)
            .map_err(|_| ExtractionError::Invalid)?;
        if count == 0 {
            break;
        }
        ctx.expanded(count)?;
        output.extend_from_slice(&buffer[..count]);
    }
    Ok(output)
}

pub(super) fn extract_pdf(ctx: &mut Context<'_>, bytes: &[u8]) -> Result<()> {
    if !bytes.starts_with(b"%PDF-") {
        return Err(ExtractionError::Invalid);
    }
    let mut object_count = 0usize;
    let mut page_count = 0usize;
    for (n, window) in bytes.windows(11).enumerate() {
        if n % 8192 == 0 {
            ctx.poll()?;
        }
        if window.starts_with(b" obj") {
            object_count += 1;
            if object_count > ctx.limits.max_pdf_objects {
                return Err(ExtractionError::Limit("pdf_objects"));
            }
        }
        if window.starts_with(b"/Type /Page") && bytes.get(n + 11) != Some(&b's') {
            page_count += 1;
            if page_count > ctx.limits.max_pages {
                return Err(ExtractionError::Limit("pdf_pages"));
            }
        }
    }
    let mut scan_at = 0;
    let mut streams = 0usize;
    let mut supported_streams = 0usize;
    while let Some(marker) = find(bytes, b"stream", scan_at) {
        ctx.poll()?;
        let dict_start = scan_at;
        scan_at = marker + 6;
        let content_at = match bytes.get(scan_at..scan_at + 2) {
            Some(value) if value == b"\r\n" => scan_at + 2,
            _ if bytes.get(scan_at) == Some(&b'\n') => scan_at + 1,
            _ => continue,
        };
        let Some(end) = find(bytes, b"endstream", content_at) else {
            return Err(ExtractionError::Invalid);
        };
        streams += 1;
        if streams > ctx.limits.max_pdf_objects {
            return Err(ExtractionError::Limit("pdf_objects"));
        }
        let mut content_end = end;
        while content_end > content_at && matches!(bytes[content_end - 1], b'\r' | b'\n') {
            content_end -= 1;
        }
        // Never inherit a filter from a preceding stream. Limit dictionary
        // inspection to the bytes since the previous endstream boundary.
        let preceding = &bytes[dict_start..marker];
        let flate = preceding
            .windows(b"/FlateDecode".len())
            .any(|v| v == b"/FlateDecode");
        let unsupported_filter =
            preceding.windows(b"/Filter".len()).any(|v| v == b"/Filter") && !flate;
        if !unsupported_filter {
            let stream = &bytes[content_at..content_end];
            let decoded = if flate {
                // A compressed checksum can itself end in CR/LF. The zlib
                // decoder stops at its checksum and ignores the PDF delimiter.
                inflate_stream(ctx, &bytes[content_at..end])?
            } else {
                ctx.expanded(stream.len())?;
                stream.to_vec()
            };
            supported_streams += 1;
            let text = content_text(ctx, &decoded)?;
            ctx.emit(&format!("PDF stream {}", streams), &text)?;
        }
        scan_at = end + 9;
    }
    if streams == 0 {
        ctx.expanded(bytes.len())?;
        let text = content_text(ctx, bytes)?;
        ctx.emit("PDF content", &text)?;
    }
    ctx.warn("PDF text is best-effort: no OCR, font CMap mapping, encrypted content or embedded image text.");
    if streams > supported_streams {
        ctx.warn("Some PDF streams use unsupported compression filters and were skipped.");
    }
    Ok(())
}
