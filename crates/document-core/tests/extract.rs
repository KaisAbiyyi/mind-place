use crc32fast::hash;
use flate2::{write::DeflateEncoder, Compression};
use mind_place_documents::{
    extract_bytes, extract_with_check, DocumentFormat, ExtractionError, ExtractionLimits,
};
use std::io::Write;

#[test]
fn large_ooxml_revocation_discards_all_text() {
    let xml = format!(
        "<document>{}</document>",
        "<p><t>evidence</t></p>".repeat(12000)
    );
    let bytes = make_zip(&[("word/document.xml", xml.as_bytes(), false)]);
    let mut checks = 0;
    let result = extract_with_check(
        DocumentFormat::Docx,
        &bytes,
        ExtractionLimits::default(),
        || {
            checks += 1;
            checks < 6
        },
    );
    assert_eq!(result, Err(ExtractionError::Revoked));
    assert_eq!(checks, 6);
}

#[test]
fn pdf_flate_checksum_linefeed_is_not_a_stream_delimiter() {
    use flate2::write::ZlibEncoder;
    let (compressed, expected) = (0..10000)
        .find_map(|n| {
            let expected = format!("checksum-{n}");
            let content = format!("BT ({expected}) Tj ET{}", " ".repeat(n));
            let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(content.as_bytes()).unwrap();
            let compressed = encoder.finish().unwrap();
            matches!(compressed.last(), Some(10 | 13)).then_some((compressed, expected))
        })
        .expect("fixture checksum ends in CR or LF");
    let mut pdf = format!(
        "%PDF-1.7\n1 0 obj\n<< /Length {} /Filter /FlateDecode >>\nstream\n",
        compressed.len()
    )
    .into_bytes();
    pdf.extend_from_slice(&compressed);
    pdf.extend_from_slice(b"\r\nendstream\nendobj\n%%EOF");
    let extracted = extract_bytes(DocumentFormat::Pdf, &pdf, ExtractionLimits::default()).unwrap();
    assert!(extracted.text.contains(&expected));
}

fn make_zip(parts: &[(&str, &[u8], bool)]) -> Vec<u8> {
    let mut data = Vec::new();
    let mut central = Vec::new();
    for (name, content, compress) in parts {
        let body = if *compress {
            let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(content).unwrap();
            encoder.finish().unwrap()
        } else {
            content.to_vec()
        };
        let offset = data.len() as u32;
        let method: u16 = if *compress { 8 } else { 0 };
        let crc = hash(content);
        data.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        data.extend_from_slice(&20u16.to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes()); // flags
        data.extend_from_slice(&method.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes()); // timestamp
        data.extend_from_slice(&crc.to_le_bytes());
        data.extend_from_slice(&(body.len() as u32).to_le_bytes());
        data.extend_from_slice(&(content.len() as u32).to_le_bytes());
        data.extend_from_slice(&(name.len() as u16).to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(name.as_bytes());
        data.extend_from_slice(&body);

        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes()); // creator
        central.extend_from_slice(&20u16.to_le_bytes()); // needed
        central.extend_from_slice(&0u16.to_le_bytes()); // flags
        central.extend_from_slice(&method.to_le_bytes());
        central.extend_from_slice(&0u32.to_le_bytes()); // timestamp
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&(body.len() as u32).to_le_bytes());
        central.extend_from_slice(&(content.len() as u32).to_le_bytes());
        central.extend_from_slice(&(name.len() as u16).to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes()); // extra
        central.extend_from_slice(&0u16.to_le_bytes()); // comment
        central.extend_from_slice(&0u16.to_le_bytes()); // start disk
        central.extend_from_slice(&0u16.to_le_bytes()); // internal attr
        central.extend_from_slice(&0u32.to_le_bytes()); // external attr
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
    }
    let start = data.len() as u32;
    data.extend_from_slice(&central);
    data.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    data.extend_from_slice(&0u16.to_le_bytes());
    data.extend_from_slice(&0u16.to_le_bytes());
    data.extend_from_slice(&(parts.len() as u16).to_le_bytes());
    data.extend_from_slice(&(parts.len() as u16).to_le_bytes());
    data.extend_from_slice(&(central.len() as u32).to_le_bytes());
    data.extend_from_slice(&start.to_le_bytes());
    data.extend_from_slice(&0u16.to_le_bytes());
    data
}

#[test]
fn markdown_sections_and_source_are_bounded() {
    let input = b"# First\nHello\n## Next\nWorld";
    let doc = extract_bytes(DocumentFormat::Markdown, input, ExtractionLimits::default()).unwrap();
    assert!(doc.chunks.iter().any(|c| c.section == "# First"));
    assert!(doc.chunks.iter().any(|c| c.section == "## Next"));
    assert!(doc.text.contains("Hello"));
    assert!(doc.text.contains("World"));
    let source = extract_bytes(
        DocumentFormat::Source,
        b"fn main() {}",
        ExtractionLimits::default(),
    )
    .unwrap();
    assert!(source.text.contains("fn main"));
    let invalid = extract_bytes(
        DocumentFormat::Source,
        b"\0secret",
        ExtractionLimits::default(),
    );
    assert_eq!(invalid, Err(ExtractionError::Invalid));
    let invalid_utf8 = extract_bytes(
        DocumentFormat::PlainText,
        &[0xff, 0xfe],
        ExtractionLimits::default(),
    );
    assert_eq!(invalid_utf8, Err(ExtractionError::Unsupported));
}

#[test]
fn small_limits_and_revocation_are_fail_closed() {
    let tiny = ExtractionLimits {
        max_input_bytes: 3,
        ..ExtractionLimits::default()
    };
    assert_eq!(
        extract_bytes(DocumentFormat::PlainText, b"1234", tiny),
        Err(ExtractionError::Limit("input_bytes"))
    );
    let limited = ExtractionLimits {
        max_output_bytes: 5,
        ..ExtractionLimits::default()
    };
    assert_eq!(
        extract_bytes(DocumentFormat::PlainText, b"some lengthy data", limited),
        Err(ExtractionError::Limit("output_bytes"))
    );
    let mut calls = 0;
    let cancelled = extract_with_check(
        DocumentFormat::Markdown,
        b"hello",
        ExtractionLimits::default(),
        || {
            calls += 1;
            calls < 2
        },
    );
    assert_eq!(cancelled, Err(ExtractionError::Revoked));
}

#[test]
fn docx_and_pptx_and_xlsx_extract_allowlisted_xml() {
    let word = b"<w:document xmlns:w=\"w\"><w:body><w:p><w:r><w:t>Hello &amp; world &#x21;</w:t></w:r></w:p></w:body></w:document>";
    let docx = make_zip(&[
        ("word/document.xml", word, true),
        (
            "docProps/app.xml",
            b"<Properties><secret>ignored</secret></Properties>",
            false,
        ),
    ]);
    let extracted =
        extract_bytes(DocumentFormat::Docx, &docx, ExtractionLimits::default()).unwrap();
    assert!(extracted.text.contains("Hello & world !"));
    assert!(!extracted.text.contains("ignored"));
    assert!(extracted
        .chunks
        .iter()
        .all(|c| c.section == "word/document.xml"));

    // Reject arbitrary named references instead of attempting DTD expansion.
    let hostile_reference = make_zip(&[(
        "word/document.xml",
        b"<w:p><w:t>&unknown;</w:t></w:p>",
        false,
    )]);
    assert_eq!(
        extract_bytes(
            DocumentFormat::Docx,
            &hostile_reference,
            ExtractionLimits::default()
        ),
        Err(ExtractionError::Unsupported)
    );

    let slide = make_zip(&[(
        "ppt/slides/slide1.xml",
        b"<p:sp xmlns:p=\"p\"><a:t xmlns:a=\"a\">Presentation</a:t></p:sp>",
        true,
    )]);
    let pptx = extract_bytes(DocumentFormat::Pptx, &slide, ExtractionLimits::default()).unwrap();
    assert!(pptx.text.contains("Presentation"));

    let sheet = make_zip(&[(
        "xl/worksheets/sheet1.xml",
        b"<worksheet><row><c><v>42</v></c></row></worksheet>",
        false,
    )]);
    let xlsx = extract_bytes(DocumentFormat::Xlsx, &sheet, ExtractionLimits::default()).unwrap();
    assert!(xlsx.text.contains("42"));
}

#[test]
fn zip_bomb_declared_size_is_rejected_before_inflate() {
    let word = vec![b'A'; 64 * 1024];
    let zip = make_zip(&[("word/document.xml", &word, true)]);
    let limits = ExtractionLimits {
        max_expanded_bytes: 100,
        ..ExtractionLimits::default()
    };
    assert_eq!(
        extract_bytes(DocumentFormat::Docx, &zip, limits),
        Err(ExtractionError::Limit("expanded_bytes"))
    );
}

#[test]
fn malformed_crc_duplicate_names_and_traversal_are_rejected() {
    let xml = b"<w:p><w:t>hello</w:t></w:p>";
    let good = make_zip(&[("word/document.xml", xml, true)]);
    let mut corrupted = good.clone();
    // Central-directory CRC starts at central signature + 16.
    let central = corrupted
        .windows(4)
        .position(|w| w == b"PK\x01\x02")
        .unwrap();
    corrupted[central + 16] ^= 0xA0;
    assert_eq!(
        extract_bytes(
            DocumentFormat::Docx,
            &corrupted,
            ExtractionLimits::default()
        ),
        Err(ExtractionError::Invalid)
    );
    let duplicates = make_zip(&[
        ("word/document.xml", xml, false),
        ("word/document.xml", xml, false),
    ]);
    assert_eq!(
        extract_bytes(
            DocumentFormat::Docx,
            &duplicates,
            ExtractionLimits::default()
        ),
        Err(ExtractionError::Invalid)
    );
    let traversal = make_zip(&[("../word/document.xml", xml, false)]);
    assert_eq!(
        extract_bytes(
            DocumentFormat::Docx,
            &traversal,
            ExtractionLimits::default()
        ),
        Err(ExtractionError::Invalid)
    );
}

#[test]
fn xml_depth_dtd_and_text_explosion_are_bounded() {
    let nested = b"<a><a><a><w:t>Hello</w:t></a></a></a>";
    let zip = make_zip(&[("word/document.xml", nested, false)]);
    let limits = ExtractionLimits {
        max_xml_depth: 2,
        ..ExtractionLimits::default()
    };
    assert_eq!(
        extract_bytes(DocumentFormat::Docx, &zip, limits),
        Err(ExtractionError::Limit("xml_depth"))
    );
    let hostile = make_zip(&[(
        "word/document.xml",
        b"<!DOCTYPE a [<!ENTITY x \"exfil\">]><a><w:t>&x;</w:t></a>",
        false,
    )]);
    assert_eq!(
        extract_bytes(DocumentFormat::Docx, &hostile, ExtractionLimits::default()),
        Err(ExtractionError::Unsupported)
    );
}

#[test]
fn pdf_plaintext_and_flate_streams_and_page_limits() {
    let pdf = b"%PDF-1.4\n1 0 obj\n<</Type /Page>>\nendobj\n2 0 obj\n<</Length 28>>\nstream\nBT (Extracted page content) Tj ET\nendstream\nendobj\n%%EOF\n";
    let doc = extract_bytes(DocumentFormat::Pdf, pdf, ExtractionLimits::default()).unwrap();
    assert!(doc.text.contains("Extracted page content"));
    assert!(doc.warnings.iter().any(|w| w.contains("best-effort")));
    let limits = ExtractionLimits {
        max_pages: 1,
        ..ExtractionLimits::default()
    };
    let two_pages = b"%PDF-1.4\n/Type /Page\n/Type /Page\nBT (X) Tj ET";
    assert_eq!(
        extract_bytes(DocumentFormat::Pdf, two_pages, limits),
        Err(ExtractionError::Limit("pdf_pages"))
    );
}

#[test]
fn extension_mapping_does_not_accept_macro_enabled_office_archives() {
    assert_eq!(
        DocumentFormat::from_extension(".DOCX"),
        Some(DocumentFormat::Docx)
    );
    assert_eq!(
        DocumentFormat::from_extension("mdx"),
        Some(DocumentFormat::Markdown)
    );
    assert_eq!(DocumentFormat::from_extension("xlsm"), None);
    assert_eq!(DocumentFormat::from_extension("docm"), None);
}

#[test]
fn pdf_operator_letters_inside_literal_are_preserved() {
    let bytes = b"%PDF-1.7\n1 0 obj\n<< /Length 50 >>\nstream\nBT (METADATA and ET inside text) Tj ET\nendstream\nendobj";
    let doc = extract_bytes(DocumentFormat::Pdf, bytes, ExtractionLimits::default()).unwrap();
    assert!(doc.text.contains("METADATA and ET inside text"));
}

#[test]
fn pdf_previous_stream_filter_does_not_bleed_into_plain_stream() {
    let bytes = b"%PDF-1.7\n1 0 obj\n<< /Filter /DCTDecode >>\nstream\nunsupported\nendstream\nendobj\n2 0 obj\n<< /Length 30 >>\nstream\nBT (visible second stream) Tj ET\nendstream\nendobj";
    let doc = extract_bytes(DocumentFormat::Pdf, bytes, ExtractionLimits::default()).unwrap();
    assert!(doc.text.contains("visible second stream"));
}
