//! MP-4L2: native Windows document corpus qualification with disposable fixtures.
//! Invoke via scripts/qualify-corpus.ps1. Never indexes the owner's personal archive.
#[cfg(windows)]
mod native {
    use mind_place_documents::{extract_bytes, DocumentFormat, ExtractionError, ExtractionLimits};
    use mind_place_index::documents::DocumentIndex;
    use mind_place_state::{
        DocumentObjectStore, DocumentScanner, ScanPolicy, StateStore, VirtualDocumentPath,
    };
    use rusqlite::Connection;
    use serde_json::{json, Value};
    use std::{
        fs,
        path::{Path, PathBuf},
        time::Instant,
    };

    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = !0u32;
        for b in bytes {
            crc ^= u32::from(*b);
            for _ in 0..8 {
                crc = (crc >> 1) ^ if crc & 1 != 0 { 0xEDB8_8320 } else { 0 };
            }
        }
        !crc
    }

    // A minimal ZIP containing one stored, correctly checksummed OOXML part.
    fn office_zip(name: &str, content: &[u8]) -> Vec<u8> {
        let mut zip = Vec::new();
        let size = u32::try_from(content.len()).unwrap();
        let len = u16::try_from(name.len()).unwrap();
        let crc = crc32(content);
        zip.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        zip.extend_from_slice(&20u16.to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes());
        zip.extend_from_slice(&0u32.to_le_bytes());
        zip.extend_from_slice(&crc.to_le_bytes());
        zip.extend_from_slice(&size.to_le_bytes());
        zip.extend_from_slice(&size.to_le_bytes());
        zip.extend_from_slice(&len.to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes());
        zip.extend_from_slice(name.as_bytes());
        zip.extend_from_slice(content);
        let cd_start = u32::try_from(zip.len()).unwrap();
        zip.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        zip.extend_from_slice(&20u16.to_le_bytes());
        zip.extend_from_slice(&20u16.to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes());
        zip.extend_from_slice(&0u32.to_le_bytes());
        zip.extend_from_slice(&crc.to_le_bytes());
        zip.extend_from_slice(&size.to_le_bytes());
        zip.extend_from_slice(&size.to_le_bytes());
        zip.extend_from_slice(&len.to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes());
        zip.extend_from_slice(&0u32.to_le_bytes());
        zip.extend_from_slice(&0u32.to_le_bytes());
        zip.extend_from_slice(name.as_bytes());
        let cd_len = u32::try_from(zip.len()).unwrap() - cd_start;
        zip.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes());
        zip.extend_from_slice(&1u16.to_le_bytes());
        zip.extend_from_slice(&1u16.to_le_bytes());
        zip.extend_from_slice(&cd_len.to_le_bytes());
        zip.extend_from_slice(&cd_start.to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes());
        zip
    }

    struct Fixture {
        home: PathBuf,
        root: PathBuf,
        state: StateStore,
        id: String,
        index: DocumentIndex,
    }
    impl Fixture {
        fn new() -> Self {
            let parent =
                PathBuf::from(std::env::var_os("MP_CORPUS_FIXTURES").expect("runner required"));
            assert!(parent.is_absolute());
            let home = tempfile::Builder::new()
                .prefix("corpus-")
                .tempdir_in(parent)
                .unwrap()
                .keep();
            let root = home.join("approved");
            fs::create_dir(&root).unwrap();
            let state = StateStore::open(home.join("authority/state.sqlite3")).unwrap();
            let id = state.register_document_root(&root).unwrap().id;
            state.approve_document_root(&id).unwrap();
            let index = DocumentIndex::open(home.join("document-index.sqlite3")).unwrap();
            Self {
                home,
                root,
                state,
                id,
                index,
            }
        }
        fn uri(&self, name: &str) -> String {
            VirtualDocumentPath::from_segments(&self.id, &[name.to_owned()])
                .unwrap()
                .to_uri()
        }
        fn hits(&self, phrase: &str) -> usize {
            self.index
                .search(&self.state, phrase, 25, || Ok(()))
                .unwrap()
                .len()
        }
        fn object_count(&self) -> i64 {
            Connection::open(self.state.path())
                .unwrap()
                .query_row("SELECT count(*) FROM document_objects", [], |row| {
                    row.get(0)
                })
                .unwrap()
        }
        fn write(&self, name: &str, bytes: impl AsRef<[u8]>) {
            fs::write(self.root.join(name), bytes).unwrap();
        }
    }

    fn make_corpus(f: &Fixture) {
        let shared = b"# Corpus\npaperlotusmd seeded evidence\n";
        f.write("notes.md", shared);
        f.write("duplicate.txt", shared);
        f.write("code.rs", b"fn paperlotusrust() -> usize { 42 }\n");
        f.write("sheet.pdf", b"%PDF-1.4\n1 0 obj\n<</Type /Page>>\nendobj\n2 0 obj\n<</Length 35>>\nstream\nBT (paperlotuspdf) Tj ET\nendstream\nendobj\n%%EOF\n");
        f.write(
            "report.docx",
            office_zip(
                "word/document.xml",
                b"<w:document><w:body><w:p><w:t>paperlotusword</w:t></w:p></w:body></w:document>",
            ),
        );
        f.write(
            "slides.pptx",
            office_zip(
                "ppt/slides/slide1.xml",
                b"<p:sp><a:t>paperlotusslide</a:t></p:sp>",
            ),
        );
        f.write(
            "table.xlsx",
            office_zip(
                "xl/worksheets/sheet1.xml",
                b"<worksheet><row><c><v>paperlotuscell</v></c></row></worksheet>",
            ),
        );
        fs::create_dir(f.root.join(".git")).unwrap();
        fs::write(f.root.join(".git/never-index.md"), b"paperlotussecret").unwrap();
    }

    fn corpus() -> Value {
        let f = Fixture::new();
        make_corpus(&f);
        let mut scanner = DocumentScanner::new(f.state.clone(), ScanPolicy::default()).unwrap();
        let started = Instant::now();
        let first = scanner.rescan(&f.id, || Ok(())).unwrap();
        let initial_scan_ms = started.elapsed().as_secs_f64() * 1000.;
        let indexed_at = Instant::now();
        let initial_index = f.index.refresh_root(&f.state, &f.id, || Ok(())).unwrap();
        let initial_index_ms = indexed_at.elapsed().as_secs_f64() * 1000.;
        assert!(first.complete && first.new_versions == 7 && first.visited_files == 7);
        assert_eq!(initial_index.indexed_files, 7);
        assert_eq!(f.object_count(), 6, "same bytes in two files must dedup");
        for (term, hits) in [
            ("paperlotusmd", 2),
            ("paperlotusrust", 1),
            ("paperlotuspdf", 1),
            ("paperlotusword", 1),
            ("paperlotusslide", 1),
            ("paperlotuscell", 1),
            ("paperlotussecret", 0),
        ] {
            assert_eq!(f.hits(term), hits, "format search {term}");
        }
        let objects = DocumentObjectStore::open(f.state.clone()).unwrap();
        let note_uri = f.uri("notes.md");
        let duplicate_uri = f.uri("duplicate.txt");
        let prior = objects
            .current_version(&note_uri, || Ok(()))
            .unwrap()
            .unwrap();
        let other = objects
            .current_version(&duplicate_uri, || Ok(()))
            .unwrap()
            .unwrap();
        assert_eq!(prior.content_hash, other.content_hash);
        assert_ne!(prior.id, other.id);

        let noop_at = Instant::now();
        let noop = scanner.rescan(&f.id, || Ok(())).unwrap();
        let noop_ms = noop_at.elapsed().as_secs_f64() * 1000.;
        let reindex_at = Instant::now();
        let reindex = f.index.refresh_root(&f.state, &f.id, || Ok(())).unwrap();
        let noop_index_ms = reindex_at.elapsed().as_secs_f64() * 1000.;
        assert!(noop.complete && noop.new_versions == 0 && noop.unchanged_versions == 7);
        assert_eq!(f.object_count(), 6);
        assert_eq!(reindex.indexed_files, 7);

        f.write("notes.md", b"# Changed\npaperlotusrevision live revision\n");
        let changed_at = Instant::now();
        let changed = scanner.rescan(&f.id, || Ok(())).unwrap();
        let changed_ms = changed_at.elapsed().as_secs_f64() * 1000.;
        let changed_index_at = Instant::now();
        f.index.refresh_root(&f.state, &f.id, || Ok(())).unwrap();
        let changed_index_ms = changed_index_at.elapsed().as_secs_f64() * 1000.;
        assert!(changed.complete && changed.new_versions == 1 && changed.unchanged_versions == 6);
        assert_eq!(f.hits("paperlotusmd"), 1);
        assert_eq!(f.hits("paperlotusrevision"), 1);
        assert_eq!(f.object_count(), 7);
        let current = objects
            .current_version(&note_uri, || Ok(()))
            .unwrap()
            .unwrap();
        assert_ne!(current.id, prior.id);
        assert_eq!(
            objects
                .versions(&note_uri, None, 10, || Ok(()))
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            objects
                .read_version(&note_uri, &prior.id, || Ok(()))
                .unwrap(),
            b"# Corpus\npaperlotusmd seeded evidence\n"
        );
        assert_eq!(
            objects
                .read_version(&note_uri, &current.id, || Ok(()))
                .unwrap(),
            b"# Changed\npaperlotusrevision live revision\n"
        );
        json!({"status":"PASS","files":first.visited_files,"formats":
            ["markdown","plain_text","rust","pdf","docx","pptx","xlsx"],
            "excluded_paths":first.excluded_paths,"indexed_chunks":initial_index.indexed_chunks,
            "initial_scan_ms":initial_scan_ms,"initial_index_ms":initial_index_ms,
            "noop_scan_ms":noop_ms,"noop_index_ms":noop_index_ms,
            "changed_scan_ms":changed_ms,"changed_index_ms":changed_index_ms,
            "new_versions_initial":first.new_versions,"unchanged_versions_noop":noop.unchanged_versions,
            "new_versions_after_edit":changed.new_versions,"immutable_history_versions":2,
            "object_count_initial":6,"object_count_after_edit":7,
            "duplicate_source_files_share_object":true,"stale_search_rejected":true})
    }

    fn parser_limits() -> Value {
        let defaults = ExtractionLimits::default();
        let input = ExtractionLimits {
            max_input_bytes: 3,
            ..defaults
        };
        assert_eq!(
            extract_bytes(DocumentFormat::PlainText, b"1234", input),
            Err(ExtractionError::Limit("input_bytes"))
        );
        let output = ExtractionLimits {
            max_output_bytes: 3,
            ..defaults
        };
        assert_eq!(
            extract_bytes(DocumentFormat::Markdown, b"abcdef", output),
            Err(ExtractionError::Limit("output_bytes"))
        );
        let chunks = ExtractionLimits {
            max_chunks: 1,
            ..defaults
        };
        assert_eq!(
            extract_bytes(DocumentFormat::Source, &vec![b'x'; 8192], chunks),
            Err(ExtractionError::Limit("chunks"))
        );
        let zip = office_zip("word/document.xml", &vec![b'x'; 32_768]);
        let expanded = ExtractionLimits {
            max_expanded_bytes: 1024,
            ..defaults
        };
        assert_eq!(
            extract_bytes(DocumentFormat::Docx, &zip, expanded),
            Err(ExtractionError::Limit("expanded_bytes"))
        );
        let depth_xml = b"<a><b><c><w:t>secret</w:t></c></b></a>";
        let depth = ExtractionLimits {
            max_xml_depth: 2,
            ..defaults
        };
        assert_eq!(
            extract_bytes(
                DocumentFormat::Docx,
                &office_zip("word/document.xml", depth_xml),
                depth
            ),
            Err(ExtractionError::Limit("xml_depth"))
        );
        let pages = ExtractionLimits {
            max_pages: 1,
            ..defaults
        };
        assert_eq!(
            extract_bytes(
                DocumentFormat::Pdf,
                b"%PDF-1.4\n/Type /Page\n/Type /Page\nBT (secret) Tj ET",
                pages
            ),
            Err(ExtractionError::Limit("pdf_pages"))
        );
        let unsafe_entry = office_zip("../word/document.xml", b"ignored");
        assert_eq!(
            extract_bytes(DocumentFormat::Docx, &unsafe_entry, defaults),
            Err(ExtractionError::Invalid)
        );
        json!({"status":"PASS","assertions":7,"limits_checked":
            ["input_bytes","output_bytes","chunks","expanded_bytes","xml_depth","pdf_pages","zip_traversal"]})
    }

    pub fn run() {
        let mut cases = Vec::new();
        for (name, test) in [
            ("windows_corpus", corpus as fn() -> Value),
            ("parser_limits", parser_limits as fn() -> Value),
        ] {
            let start = Instant::now();
            let result = std::panic::catch_unwind(test);
            let mut receipt = result.unwrap_or_else(|panic| {
                let reason = panic
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| panic.downcast_ref::<&str>().map(|v| (*v).to_owned()))
                    .unwrap_or_else(|| "native_case_panicked".into());
                json!({"status":"FAIL","reason":reason})
            });
            receipt["case"] = json!(name);
            receipt["elapsed_ms"] = json!(start.elapsed().as_secs_f64() * 1000.);
            println!("{}", receipt);
            cases.push(receipt);
        }
        let status = if cases.iter().all(|case| case["status"] == "PASS") {
            "PASS"
        } else {
            "FAIL"
        };
        let report = json!({"schema":"mind-place/mp-4l2/v1","task_id":"MP-4L2",
            "status":status,"product_sha":std::env::var("MP_CORPUS_PRODUCT_SHA").unwrap(),
            "cases":cases,"scope":"Native Windows NTFS, generated representative document corpus; no owner private files",
            "limits":"Cooperative parser deadline only; PDF is best-effort; MP-4U owner UI pending",
            "fixture_retention":"Outside repo, retained without automatic deletion"});
        fs::write(
            std::env::var_os("MP_CORPUS_RECEIPT").unwrap(),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
        println!("MP-4L2 {status}");
        if status != "PASS" {
            std::process::exit(1);
        }
    }
}

fn main() {
    #[cfg(windows)]
    native::run();
    #[cfg(not(windows))]
    {
        eprintln!("MP-4L2 BLOCKED: native Windows NTFS required");
        std::process::exit(2);
    }
}
