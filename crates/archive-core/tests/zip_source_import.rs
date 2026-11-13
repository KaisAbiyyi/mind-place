//! MP-IMPORT-APP-01: a single zip file as an authorized import source, streamed without
//! extraction. Synthetic, hand-built archives only; no real export is read.
//!
//! The zip writer below is deliberately small and explicit so hostile archives (traversal names,
//! overlapping entries, lying sizes, bombs, zip64, multi-disk) can be built byte by byte.

use flate2::write::DeflateEncoder;
use flate2::{Compression, Crc};
use mind_place_archive::historical::{
    ImportJobLimits, ImportJobReceipt, ImportMember, ImportPoint, ImportProgress, ManifestLimits,
    MemberKind, NoProgress, SourceImportRequest, SourceManifest,
};
use mind_place_archive::{ArchiveError, ArchiveStore, Result};
use mind_place_security::{ImportAuthorizationCapability, SecurityGate};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

const CRED: &[u8] = b"synthetic-owner-credential";
const A: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const B: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const C: &str = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";

// ---------- synthetic exports ----------

fn conv(id: &str, title: &str, text: &str) -> String {
    serde_json::to_string(&serde_json::json!({
        "id": id, "title": title, "create_time": 1700000000.5, "update_time": 1700000100.5,
        "current_node": "n2",
        "mapping": {
            "n1": {"id":"n1","parent":null,"children":["n2","n3"],"message":{
                "id":"m1","author":{"role":"user"},"create_time":1700000001.0,
                "content":{"content_type":"text","parts":[text]}}},
            "n2": {"id":"n2","parent":"n1","children":[],"message":{
                "id":"m2","author":{"role":"assistant"},"create_time":1700000002.0,
                "content":{"content_type":"text","parts":[format!("reply to {text}")]}}},
            "n3": {"id":"n3","parent":"n1","children":[],"message":{
                "id":"m3","author":{"role":"assistant"},"create_time":1700000003.0,
                "content":{"content_type":"text","parts":[format!("ALT-BRANCH {text}")]}}}
        }
    }))
    .unwrap()
}

fn export(records: &[String]) -> Vec<u8> {
    format!("[\n  {}\n]\n", records.join(",\n  ")).into_bytes()
}

fn three_conversations() -> Vec<u8> {
    export(&[
        conv(A, "First", "alpha question"),
        conv(B, "Second", "beta question"),
        conv(C, "Third", "gamma question"),
    ])
}

const GEMINI_HTML: &str = concat!(
    "<html><body>PREFIX-NOT-A-CARD\n",
    r#"<div class="outer-cell"><div class="content-cell">Prompted one<br>4 Nov 2026, 21:15:40 WIB<br><p>reply one</p></div><div class="content-cell">https://gemini.google.com/app/c_cardone0001</div></div>"#,
    "\n",
    r#"<div class="outer-cell"><div class="content-cell">Prompted two<br><p>reply two</p></div><div class="content-cell">https://gemini.google.com/app/c_cardtwo0002</div></div>"#,
    "\n</body></html>"
);

// ---------- a small, explicit zip writer ----------

#[derive(Clone)]
struct Z {
    name: Vec<u8>,
    data: Vec<u8>,
    deflate: bool,
    flags: u16,
    method_override: Option<u16>,
}

fn file(name: &str, data: &[u8]) -> Z {
    Z {
        name: name.as_bytes().to_vec(),
        data: data.to_vec(),
        deflate: true,
        flags: 1 << 11,
        method_override: None,
    }
}

fn stored(name: &str, data: &[u8]) -> Z {
    Z {
        deflate: false,
        ..file(name, data)
    }
}

#[derive(Default)]
struct Built {
    bytes: Vec<u8>,
    /// Offset of each local header, in entry order.
    locals: Vec<usize>,
    /// Offset of each central directory header, in entry order.
    centrals: Vec<usize>,
    eocd: usize,
}

fn put16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn put32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn put64(out: &mut Vec<u8>, v: u64) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn compress(data: &[u8]) -> Vec<u8> {
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}

/// Builds a zip. With `zip64`, every size and offset field is saturated and carried in zip64
/// extra fields and records, as a large export would.
fn build(entries: &[Z], zip64: bool) -> Built {
    let mut out = Vec::new();
    let mut built = Built::default();
    let mut meta = Vec::new();
    for entry in entries {
        let body = if entry.deflate {
            compress(&entry.data)
        } else {
            entry.data.clone()
        };
        let mut crc = Crc::new();
        crc.update(&entry.data);
        let method = entry
            .method_override
            .unwrap_or(if entry.deflate { 8 } else { 0 });
        let offset = out.len();
        built.locals.push(offset);
        put32(&mut out, 0x0403_4b50);
        put16(&mut out, if zip64 { 45 } else { 20 });
        put16(&mut out, entry.flags);
        put16(&mut out, method);
        put16(&mut out, 0);
        put16(&mut out, 0x5b21);
        put32(&mut out, crc.sum());
        let local_extra = if zip64 {
            let mut extra = Vec::new();
            put16(&mut extra, 1);
            put16(&mut extra, 16);
            put64(&mut extra, entry.data.len() as u64);
            put64(&mut extra, body.len() as u64);
            extra
        } else {
            Vec::new()
        };
        if zip64 {
            put32(&mut out, 0xFFFF_FFFF);
            put32(&mut out, 0xFFFF_FFFF);
        } else {
            put32(&mut out, body.len() as u32);
            put32(&mut out, entry.data.len() as u32);
        }
        put16(&mut out, entry.name.len() as u16);
        put16(&mut out, local_extra.len() as u16);
        out.extend_from_slice(&entry.name);
        out.extend_from_slice(&local_extra);
        out.extend_from_slice(&body);
        meta.push((offset, crc.sum(), body.len(), method));
    }
    let cd_offset = out.len();
    for (entry, (offset, crc, csize, method)) in entries.iter().zip(&meta) {
        built.centrals.push(out.len());
        put32(&mut out, 0x0201_4b50);
        put16(&mut out, 0x031e);
        put16(&mut out, if zip64 { 45 } else { 20 });
        put16(&mut out, entry.flags);
        put16(&mut out, *method);
        put16(&mut out, 0);
        put16(&mut out, 0x5b21);
        put32(&mut out, *crc);
        let extra = if zip64 {
            let mut extra = Vec::new();
            put16(&mut extra, 1);
            put16(&mut extra, 24);
            put64(&mut extra, entry.data.len() as u64);
            put64(&mut extra, *csize as u64);
            put64(&mut extra, *offset as u64);
            extra
        } else {
            Vec::new()
        };
        if zip64 {
            put32(&mut out, 0xFFFF_FFFF);
            put32(&mut out, 0xFFFF_FFFF);
        } else {
            put32(&mut out, *csize as u32);
            put32(&mut out, entry.data.len() as u32);
        }
        put16(&mut out, entry.name.len() as u16);
        put16(&mut out, extra.len() as u16);
        put16(&mut out, 0);
        put16(&mut out, 0);
        put16(&mut out, 0);
        put32(&mut out, 0);
        put32(&mut out, if zip64 { 0xFFFF_FFFF } else { *offset as u32 });
        out.extend_from_slice(&entry.name);
        out.extend_from_slice(&extra);
    }
    let cd_size = out.len() - cd_offset;
    if zip64 {
        let z64 = out.len();
        put32(&mut out, 0x0606_4b50);
        put64(&mut out, 44);
        put16(&mut out, 45);
        put16(&mut out, 45);
        put32(&mut out, 0);
        put32(&mut out, 0);
        put64(&mut out, entries.len() as u64);
        put64(&mut out, entries.len() as u64);
        put64(&mut out, cd_size as u64);
        put64(&mut out, cd_offset as u64);
        put32(&mut out, 0x0706_4b50);
        put32(&mut out, 0);
        put64(&mut out, z64 as u64);
        put32(&mut out, 1);
    }
    built.eocd = out.len();
    put32(&mut out, 0x0605_4b50);
    put16(&mut out, 0);
    put16(&mut out, 0);
    let count = if zip64 { 0xFFFF } else { entries.len() as u16 };
    put16(&mut out, count);
    put16(&mut out, count);
    put32(&mut out, if zip64 { 0xFFFF_FFFF } else { cd_size as u32 });
    put32(&mut out, if zip64 { 0xFFFF_FFFF } else { cd_offset as u32 });
    put16(&mut out, 0);
    built.bytes = out;
    built
}

fn zip(entries: &[Z]) -> Vec<u8> {
    build(entries, false).bytes
}

// ---------- fixture ----------

struct Fx {
    home: tempfile::TempDir,
    store: ArchiveStore,
    gate: SecurityGate,
    provider: &'static str,
}

impl Fx {
    fn new(provider: &'static str) -> Self {
        let home = tempfile::tempdir().unwrap();
        let store = ArchiveStore::open(home.path().join("archive-v1")).unwrap();
        let gate = SecurityGate::open(home.path().join("security-v1.json")).unwrap();
        gate.initialize(CRED).unwrap();
        Self {
            home,
            store,
            gate,
            provider,
        }
    }

    fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.home.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn authorize(&self, source: &Path) -> ImportAuthorizationCapability {
        let manifest = SourceManifest::measure(source, ManifestLimits::default()).unwrap();
        self.gate
            .authorize_import_session(
                self.gate.require_unlocked().unwrap(),
                CRED,
                self.provider,
                source,
                *manifest.sha256(),
                Duration::from_secs(300),
            )
            .unwrap()
    }

    fn run(
        &self,
        cap: &ImportAuthorizationCapability,
        members: &[ImportMember],
        progress: &dyn ImportProgress,
    ) -> Result<ImportJobReceipt> {
        let mut request = SourceImportRequest::new(
            &self.gate,
            self.gate.require_unlocked().unwrap(),
            cap,
            members,
        );
        request.progress = progress;
        request.limits = ImportJobLimits::default();
        self.store.import_authorized_source(&request)
    }

    fn run_default(&self, source: &Path, members: &[ImportMember]) -> Result<ImportJobReceipt> {
        let cap = self.authorize(source);
        self.run(&cap, members, &NoProgress)
    }

    fn archive(&self) -> &Path {
        self.store.root()
    }
}

struct Hook<F: Fn(ImportPoint) -> Result<()>>(F);

impl<F: Fn(ImportPoint) -> Result<()>> ImportProgress for Hook<F> {
    fn checkpoint(&self, point: ImportPoint) -> Result<()> {
        (self.0)(point)
    }
}

fn chatgpt(path: &str) -> ImportMember {
    ImportMember {
        path: path.into(),
        kind: MemberKind::ChatgptConversations,
    }
}

fn gemini(path: &str) -> ImportMember {
    ImportMember {
        path: path.into(),
        kind: MemberKind::GeminiTakeoutHtml,
    }
}

fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if !dir.exists() {
        return out;
    }
    let mut pending = vec![dir.to_path_buf()];
    while let Some(current) = pending.pop() {
        for entry in std::fs::read_dir(current).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                out.push(path);
            }
        }
    }
    out
}

fn conversations(fx: &Fx) -> usize {
    let dir = fx.archive().join("conversations");
    if !dir.exists() {
        return 0;
    }
    std::fs::read_dir(dir).unwrap().count()
}

fn evidence_records(fx: &Fx) -> Vec<serde_json::Value> {
    files_under(&fx.archive().join("imports").join("evidence"))
        .iter()
        .map(|path| serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap())
        .collect()
}

fn raw_blob(fx: &Fx, sha: &str) -> PathBuf {
    fx.archive()
        .join("imports")
        .join("raw")
        .join(&sha[..2])
        .join(format!("{sha}.raw"))
}

fn assert_nothing_written(fx: &Fx) {
    assert_eq!(conversations(fx), 0);
    for sub in ["imports/raw", "imports/evidence", "imports/dispositions"] {
        assert_eq!(files_under(&fx.archive().join(sub)).len(), 0, "{sub}");
    }
    let jobs = fx.archive().join("imports").join("jobs");
    if jobs.exists() {
        let left: Vec<_> = std::fs::read_dir(jobs)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.is_dir())
            .collect();
        assert!(left.is_empty(), "a job directory was left behind");
    }
}

fn measure_error(bytes: &[u8]) -> String {
    measure_error_with(bytes, ManifestLimits::default())
}

fn measure_error_with(bytes: &[u8], limits: ManifestLimits) -> String {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join("export.zip");
    std::fs::write(&path, bytes).unwrap();
    SourceManifest::measure(&path, limits)
        .expect_err("expected the measurement to fail")
        .to_string()
}

// ---------- end to end ----------

#[test]
fn a_chatgpt_export_zip_imports_without_extraction_and_matches_a_folder_import() {
    let json = three_conversations();
    let image = vec![0x89u8; 4096];
    let archive = zip(&[
        file("conversations.json", &json),
        stored("file-abc-image.png", &image),
        file("chat.html", b"<html>viewer</html>"),
    ]);
    let fx = Fx::new("chatgpt");
    let source = fx.write("export.zip", &archive);
    let manifest = SourceManifest::measure(&source, ManifestLimits::default()).unwrap();
    assert_eq!(manifest.source_kind(), "zip");
    assert_eq!(manifest.total_files(), 3);
    assert_eq!(manifest.stored_bytes(), archive.len() as u64);
    assert_eq!(
        manifest.archive_sha256_hex().unwrap(),
        format!("{:x}", Sha256::digest(&archive))
    );
    let entry = manifest.entry("conversations.json").unwrap();
    assert_eq!(entry.size, json.len() as u64);
    assert_eq!(entry.sha256, format!("{:x}", Sha256::digest(&json)));

    let receipt = fx
        .run_default(&source, &[chatgpt("conversations.json")])
        .unwrap();
    assert_eq!(receipt.records_scanned, 3);
    assert_eq!(receipt.imported, 3);
    assert_eq!(conversations(&fx), 3);
    assert_eq!(receipt.members[0].bytes, json.len() as u64);

    // Raw evidence offsets index the decompressed member, byte for byte.
    let evidence = evidence_records(&fx);
    assert_eq!(evidence.len(), 3);
    for record in &evidence {
        let offset = record["offset"].as_u64().unwrap() as usize;
        let length = record["length"].as_u64().unwrap() as usize;
        let blob =
            std::fs::read(raw_blob(&fx, record["payload_sha256"].as_str().unwrap())).unwrap();
        assert_eq!(blob, json[offset..offset + length]);
        assert_eq!(record["member_path"], "conversations.json");
        assert_eq!(record["member_sha256"], entry.sha256);
    }
    // The alternate branch is preserved in the raw bytes.
    assert!(evidence.iter().any(|record| {
        let blob =
            std::fs::read(raw_blob(&fx, record["payload_sha256"].as_str().unwrap())).unwrap();
        String::from_utf8_lossy(&blob).contains("ALT-BRANCH")
    }));
    // Nothing from the zip was extracted anywhere in the archive or next to the source.
    for path in files_under(fx.archive()) {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        assert!(
            name != "conversations.json" && name != "file-abc-image.png",
            "{name}"
        );
    }
    let siblings: Vec<_> = std::fs::read_dir(fx.home.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .collect();
    assert!(!siblings.iter().any(|name| name == "conversations.json"));

    // The same content imported from a folder gives the same projections and raw blobs.
    let folder_fx = Fx::new("chatgpt");
    let dir = folder_fx.home.path().join("folder");
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("conversations.json"), &json).unwrap();
    let folder_receipt = folder_fx
        .run_default(&dir, &[chatgpt("conversations.json")])
        .unwrap();
    assert_eq!(folder_receipt.imported, receipt.imported);
    assert_eq!(
        folder_receipt.raw_bytes_preserved,
        receipt.raw_bytes_preserved
    );
    let mut zip_blobs: Vec<_> = files_under(&fx.archive().join("imports/raw"))
        .iter()
        .map(|p| p.file_name().unwrap().to_owned())
        .collect();
    let mut folder_blobs: Vec<_> = files_under(&folder_fx.archive().join("imports/raw"))
        .iter()
        .map(|p| p.file_name().unwrap().to_owned())
        .collect();
    zip_blobs.sort();
    folder_blobs.sort();
    assert_eq!(zip_blobs, folder_blobs);
    // A folder and a zip of the same content are different sources (different manifest hash).
    assert_ne!(receipt.manifest_sha256, folder_receipt.manifest_sha256);
}

#[test]
fn reimporting_the_same_zip_is_idempotent() {
    let fx = Fx::new("chatgpt");
    let source = fx.write(
        "export.zip",
        &zip(&[file("conversations.json", &three_conversations())]),
    );
    let first = fx
        .run_default(&source, &[chatgpt("conversations.json")])
        .unwrap();
    let before: Vec<_> = files_under(fx.archive())
        .into_iter()
        .filter(|p| !p.to_string_lossy().contains("jobs"))
        .map(|p| (p.clone(), std::fs::read(&p).unwrap()))
        .collect();
    let second = fx
        .run_default(&source, &[chatgpt("conversations.json")])
        .unwrap();
    assert!(second.already_complete);
    assert_eq!(second.job_key, first.job_key);
    let after: Vec<_> = files_under(fx.archive())
        .into_iter()
        .filter(|p| !p.to_string_lossy().contains("jobs"))
        .map(|p| (p.clone(), std::fs::read(&p).unwrap()))
        .collect();
    assert_eq!(before.len(), after.len());
    for (path, bytes) in &before {
        assert_eq!(std::fs::read(path).unwrap(), *bytes);
    }
}

#[test]
fn a_gemini_takeout_zip_imports_from_its_documented_path() {
    let fx = Fx::new("gemini");
    let path = "Takeout/My Activity/Gemini Apps/MyActivity.html";
    let source = fx.write(
        "takeout.zip",
        &zip(&[
            stored("Takeout/archive_browser.html", b"<html>index</html>"),
            file(path, GEMINI_HTML.as_bytes()),
        ]),
    );
    let manifest = SourceManifest::measure(&source, ManifestLimits::default()).unwrap();
    let candidates: Vec<_> = manifest
        .candidate_members("gemini")
        .into_iter()
        .map(|e| e.path.clone())
        .collect();
    assert_eq!(candidates, vec![path.to_owned()]);
    let receipt = fx.run_default(&source, &[gemini(path)]).unwrap();
    assert_eq!(receipt.imported, 2);
    for record in evidence_records(&fx) {
        let offset = record["offset"].as_u64().unwrap() as usize;
        let length = record["length"].as_u64().unwrap() as usize;
        let blob =
            std::fs::read(raw_blob(&fx, record["payload_sha256"].as_str().unwrap())).unwrap();
        assert_eq!(blob, GEMINI_HTML.as_bytes()[offset..offset + length]);
    }
}

#[test]
fn a_zip64_export_is_read_and_imported() {
    let fx = Fx::new("chatgpt");
    let json = three_conversations();
    let built = build(
        &[
            file("conversations.json", &json),
            stored("notes.txt", b"synthetic"),
        ],
        true,
    );
    let source = fx.write("export64.zip", &built.bytes);
    let manifest = SourceManifest::measure(&source, ManifestLimits::default()).unwrap();
    assert_eq!(
        manifest.entry("conversations.json").unwrap().size,
        json.len() as u64
    );
    let receipt = fx
        .run_default(&source, &[chatgpt("conversations.json")])
        .unwrap();
    assert_eq!(receipt.imported, 3);
}

#[test]
fn candidate_members_follow_the_documented_layouts() {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join("export.zip");
    std::fs::write(
        &path,
        zip(&[
            file("conversations-000.json", b"[]"),
            file("conversations-001.json", b"[]"),
            file("nested/conversations.json", b"[]"),
            file("user.json", b"{}"),
            file("shared_conversations.json", b"[]"),
            file("chat.html", b"<html></html>"),
            file(
                "Takeout/Aktivitas Saya/Aplikasi Gemini/AktivitasSaya.html",
                b"<html></html>",
            ),
            file(
                "Takeout/My Activity/Search/MyActivity.html",
                b"<html></html>",
            ),
        ]),
    )
    .unwrap();
    let manifest = SourceManifest::measure(&path, ManifestLimits::default()).unwrap();
    let names = |provider| -> Vec<String> {
        manifest
            .candidate_members(provider)
            .into_iter()
            .map(|e| e.path.clone())
            .collect()
    };
    assert_eq!(
        names("chatgpt"),
        vec![
            "conversations-000.json",
            "conversations-001.json",
            "nested/conversations.json"
        ]
    );
    assert_eq!(
        names("gemini"),
        vec!["Takeout/Aktivitas Saya/Aplikasi Gemini/AktivitasSaya.html"]
    );
    assert!(names("other").is_empty());
}

// ---------- hostile archives ----------

#[test]
fn unsafe_member_names_are_refused() {
    for name in [
        "../evil.json",
        "/abs.json",
        "dir/../../evil.json",
        "a\\b.json",
        "C:evil.json",
        "a/./b.json",
        "a//b.json",
        "bell\u{7}.json",
    ] {
        let message = measure_error(&zip(&[file(name, b"[]")]));
        assert!(message.contains("E_SOURCE_NAME"), "{name}: {message}");
    }
    // Not UTF-8.
    let mut bad = file("x.json", b"[]");
    bad.name = vec![b'x', 0xff, b'.', b'j'];
    let message = measure_error(&zip(&[bad]));
    assert!(message.contains("E_SOURCE_NAME"), "{message}");
}

#[test]
fn duplicate_member_names_are_refused() {
    let message = measure_error(&zip(&[
        file("conversations.json", b"[]"),
        file("conversations.json", b"[1]"),
    ]));
    assert!(message.contains("E_ZIP_DUPLICATE"), "{message}");
}

#[test]
fn encrypted_and_unknown_methods_are_refused() {
    let mut encrypted = file("conversations.json", b"[]");
    encrypted.flags |= 1;
    assert!(measure_error(&zip(&[encrypted])).contains("E_ZIP_ENCRYPTED"));
    let mut strong = file("conversations.json", b"[]");
    strong.flags |= 1 << 6;
    assert!(measure_error(&zip(&[strong])).contains("E_ZIP_ENCRYPTED"));
    let mut bzip = stored("conversations.json", b"[]");
    bzip.method_override = Some(12);
    assert!(measure_error(&zip(&[bzip])).contains("E_ZIP_METHOD"));
}

#[test]
fn a_high_ratio_member_is_refused_as_a_bomb() {
    let zeros = vec![0u8; 8 * 1024 * 1024];
    let message = measure_error(&zip(&[file("conversations.json", &zeros)]));
    assert!(message.contains("E_ZIP_BOMB"), "{message}");
    // A small member is exempt; a raised ratio limit admits the large one.
    let limits = ManifestLimits {
        max_zip_ratio: 100_000,
        ..ManifestLimits::default()
    };
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join("ok.zip");
    std::fs::write(&path, zip(&[file("conversations.json", &zeros)])).unwrap();
    assert!(SourceManifest::measure(&path, limits).is_ok());
}

#[test]
fn overlapping_members_are_refused_as_a_bomb() {
    // The overlapping-file bomb: entry b's local header and data lie *inside* entry a's data, so
    // every header and checksum is individually valid and only the overlap rule refuses it.
    let inner = build(&[stored("b.json", b"[1,2,3]")], false);
    let inner_local = inner.bytes[..inner.centrals[0]].to_vec();
    let mut built = build(
        &[stored("a.json", &inner_local), stored("b.json", b"[1,2,3]")],
        false,
    );
    let inside_a = (built.locals[0] + 30 + "a.json".len()) as u32;
    let at = built.centrals[1] + 42;
    built.bytes[at..at + 4].copy_from_slice(&inside_a.to_le_bytes());
    let message = measure_error(&built.bytes);
    assert!(message.contains("E_ZIP_BOMB"), "{message}");
}

#[test]
fn bytes_hidden_between_the_directory_and_the_end_record_are_refused() {
    let built = build(&[file("conversations.json", b"[]")], false);
    let mut padded = built.bytes[..built.eocd].to_vec();
    padded.extend_from_slice(b"HIDDEN");
    padded.extend_from_slice(&built.bytes[built.eocd..]);
    let message = measure_error(&padded);
    assert!(message.contains("E_ZIP_MALFORMED"), "{message}");
}

#[test]
fn lying_sizes_and_checksums_are_refused() {
    let data = three_conversations();
    // Central directory declares a smaller decompressed size.
    let mut built = build(&[file("conversations.json", &data)], false);
    let at = built.centrals[0] + 24;
    built.bytes[at..at + 4].copy_from_slice(&((data.len() - 10) as u32).to_le_bytes());
    assert!(measure_error(&built.bytes).contains("E_ZIP_CORRUPT"));
    // Central directory declares a larger decompressed size.
    let mut built = build(&[file("conversations.json", &data)], false);
    let at = built.centrals[0] + 24;
    built.bytes[at..at + 4].copy_from_slice(&((data.len() + 10) as u32).to_le_bytes());
    assert!(measure_error(&built.bytes).contains("E_ZIP_CORRUPT"));
    // Wrong CRC-32.
    let mut built = build(&[file("conversations.json", &data)], false);
    let at = built.centrals[0] + 16;
    built.bytes[at] ^= 0xff;
    assert!(measure_error(&built.bytes).contains("E_ZIP_CORRUPT"));
    // Local header names a different file than the directory.
    let mut built = build(&[stored("conversations.json", &data)], false);
    let at = built.locals[0] + 30;
    built.bytes[at] = b'k';
    assert!(measure_error(&built.bytes).contains("E_ZIP_CORRUPT"));
    // Corrupt deflate stream.
    let mut built = build(&[file("conversations.json", &data)], false);
    let at = built.locals[0] + 30 + "conversations.json".len() + 3;
    for byte in &mut built.bytes[at..at + 16] {
        *byte = 0xff;
    }
    assert!(measure_error(&built.bytes).contains("E_ZIP_CORRUPT"));
}

#[test]
fn malformed_truncated_split_and_padded_archives_are_refused() {
    let good = zip(&[file("conversations.json", &three_conversations())]);
    assert!(measure_error(b"not a zip at all, just text").contains("E_ZIP_MALFORMED"));
    assert!(measure_error(&good[..good.len() / 2]).contains("E_ZIP_MALFORMED"));
    let mut prefixed = b"MZ-self-extractor-stub".to_vec();
    prefixed.extend_from_slice(&good);
    assert!(measure_error(&prefixed).contains("E_ZIP_MALFORMED"));
    let mut appended = good.clone();
    appended.extend_from_slice(b"trailing bytes");
    assert!(measure_error(&appended).contains("E_ZIP_MALFORMED"));
    let built = build(&[file("conversations.json", b"[]")], false);
    let mut split = built.bytes.clone();
    split[built.eocd + 4] = 1;
    assert!(measure_error(&split).contains("E_ZIP_MULTIDISK"));
    // A zip64 end record that disagrees with the classic one.
    let built = build(&[file("conversations.json", b"[]")], true);
    let mut bad64 = built.bytes.clone();
    // The zip64 record starts 56 + 20 bytes before the classic end record; bump its entry count.
    let z64 = built.eocd - 20 - 56;
    bad64[z64 + 32] = 7;
    bad64[z64 + 24] = 7;
    assert!(measure_error(&bad64).contains("E_ZIP_MALFORMED"));
}

#[test]
fn archive_limits_are_enforced() {
    let many: Vec<Z> = (0..20)
        .map(|i| file(&format!("f{i}.json"), b"[]"))
        .collect();
    let limits = ManifestLimits {
        max_entries: 10,
        ..ManifestLimits::default()
    };
    assert!(measure_error_with(&zip(&many), limits).contains("E_SOURCE_LIMIT"));
    let limits = ManifestLimits {
        max_total_bytes: 100,
        ..ManifestLimits::default()
    };
    let big = vec![b'x'; 1000];
    assert!(measure_error_with(&zip(&[stored("a.json", &big)]), limits).contains("E_SOURCE_LIMIT"));
    let limits = ManifestLimits {
        max_path_bytes: 16,
        ..ManifestLimits::default()
    };
    assert!(
        measure_error_with(&zip(&[file("a/very/long/member/name.json", b"[]")]), limits)
            .contains("E_SOURCE_LIMIT")
    );
}

// ---------- substitution and change detection ----------

#[test]
fn a_zip_replaced_after_authorization_is_refused_before_anything_is_written() {
    let fx = Fx::new("chatgpt");
    let source = fx.write(
        "export.zip",
        &zip(&[file("conversations.json", &three_conversations())]),
    );
    let cap = fx.authorize(&source);
    // Same member names, different content.
    std::fs::write(
        &source,
        zip(&[file(
            "conversations.json",
            &export(&[conv(A, "Swapped", "different text")]),
        )]),
    )
    .unwrap();
    let message = fx
        .run(&cap, &[chatgpt("conversations.json")], &NoProgress)
        .expect_err("a swapped zip must be refused")
        .to_string();
    assert!(message.contains("E_SOURCE_CHANGED"), "{message}");
    assert_nothing_written(&fx);
}

#[test]
fn a_zip_changed_while_it_is_staged_is_detected_and_nothing_is_visible() {
    let fx = Fx::new("chatgpt");
    let first = three_conversations();
    let second = export(&[conv(
        "dddddddd-dddd-4ddd-8ddd-dddddddddddd",
        "Fourth",
        "delta",
    )]);
    let original = zip(&[
        stored("conversations-000.json", &first),
        stored("conversations-001.json", &second),
    ]);
    let source = fx.write("export.zip", &original);
    let cap = fx.authorize(&source);
    let path = source.clone();
    let hook = Hook(move |point| {
        if point == (ImportPoint::Staged { seq: 0 }) {
            // Same length, one byte of the second member changed (stored, so it stays parseable
            // or not; either way the hash must catch it).
            let mut changed = std::fs::read(&path).unwrap();
            let needle = b"delta";
            let at = changed
                .windows(needle.len())
                .position(|w| w == needle)
                .unwrap();
            changed[at] = b'D';
            std::fs::write(&path, changed).unwrap();
        }
        Ok(())
    });
    let message = fx
        .run(
            &cap,
            &[
                chatgpt("conversations-000.json"),
                chatgpt("conversations-001.json"),
            ],
            &hook,
        )
        .expect_err("a changed member must be refused")
        .to_string();
    assert!(
        message.contains("E_SOURCE_MISMATCH") || message.contains("E_ZIP_CORRUPT"),
        "{message}"
    );
    assert_nothing_written(&fx);
}

#[test]
fn a_member_outside_the_manifest_or_a_crash_mid_job_behaves_like_a_folder_source() {
    let fx = Fx::new("chatgpt");
    let source = fx.write(
        "export.zip",
        &zip(&[file("conversations.json", &three_conversations())]),
    );
    let cap = fx.authorize(&source);
    let message = fx
        .run(&cap, &[chatgpt("missing.json")], &NoProgress)
        .unwrap_err()
        .to_string();
    assert!(message.contains("E_MEMBER_UNKNOWN"), "{message}");

    // Crash after the second item's raw blob; a rerun under a new authorization repairs it.
    let crash = Hook(|point| {
        if point == (ImportPoint::AfterRaw { seq: 1 }) {
            Err(ArchiveError::Invalid("SIMULATED_CRASH".into()))
        } else {
            Ok(())
        }
    });
    assert!(fx
        .run(&cap, &[chatgpt("conversations.json")], &crash)
        .is_err());
    let receipt = fx
        .run_default(&source, &[chatgpt("conversations.json")])
        .unwrap();
    assert!(receipt.resumed);
    assert_eq!(conversations(&fx), 3);
}

#[test]
fn locking_mid_job_stops_publication_from_a_zip() {
    let fx = Fx::new("chatgpt");
    let source = fx.write(
        "export.zip",
        &zip(&[file("conversations.json", &three_conversations())]),
    );
    let cap = fx.authorize(&source);
    let gate = &fx.gate;
    let hook = Hook(|point| {
        if point == (ImportPoint::BeforeItem { seq: 1 }) {
            gate.lock();
        }
        Ok(())
    });
    let message = fx
        .run(&cap, &[chatgpt("conversations.json")], &hook)
        .unwrap_err()
        .to_string();
    assert!(message.contains("E_AUTHORIZATION"), "{message}");
    assert_eq!(conversations(&fx), 1);
}

#[cfg(unix)]
#[test]
fn a_zip_source_reached_through_a_symlink_is_refused() {
    let fx = Fx::new("chatgpt");
    let source = fx.write("export.zip", &zip(&[file("conversations.json", b"[]")]));
    let link = fx.home.path().join("link.zip");
    std::os::unix::fs::symlink(&source, &link).unwrap();
    let message = SourceManifest::measure(&link, ManifestLimits::default())
        .unwrap_err()
        .to_string();
    assert!(message.contains("E_SOURCE_ROOT"), "{message}");
}

// ---------- large synthetic export (memory probe, run by hand) ----------

/// Streams a large synthetic ChatGPT export zip to disk without holding it in memory, then
/// imports it. Run with `cargo test --release -p mind-place-archive --test zip_source_import --
/// --ignored --nocapture large_synthetic` while sampling the process's peak working set.
/// `MP_IMPORT_PROBE_CONVERSATIONS` (default 20000) and `MP_IMPORT_PROBE_TEXT_BYTES` (default
/// 16384) size the export.
#[test]
#[ignore]
fn large_synthetic_export_memory_probe() {
    let count: usize = std::env::var("MP_IMPORT_PROBE_CONVERSATIONS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20_000);
    let text_bytes: usize = std::env::var("MP_IMPORT_PROBE_TEXT_BYTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(16_384);
    let fx = Fx::new("chatgpt");
    let path = fx.home.path().join("large-export.zip");
    let started = std::time::Instant::now();
    let (uncompressed, compressed) = write_streamed_export(&path, count, text_bytes);
    println!(
        "probe: wrote {count} conversations, {uncompressed} bytes uncompressed, {compressed} bytes zipped in {:?}",
        started.elapsed()
    );
    let started = std::time::Instant::now();
    let receipt = fx
        .run_default(&path, &[chatgpt("conversations.json")])
        .unwrap();
    println!(
        "probe: imported {} (raw-only {}, rejected {}) in {:?}",
        receipt.imported,
        receipt.raw_only,
        receipt.rejected,
        started.elapsed()
    );
    assert_eq!(receipt.imported as usize, count);
}

/// Pseudo-random lowercase text, so the export compresses like prose rather than like zeros.
fn filler(seed: u64, len: usize) -> String {
    let mut state = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    let mut out = String::with_capacity(len);
    while out.len() < len {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let word = (state >> 33) as usize % 9 + 2;
        for i in 0..word {
            out.push((b'a' + ((state >> (i * 5 % 40)) as u8 % 26)) as char);
        }
        out.push(' ');
    }
    out.truncate(len);
    out
}

fn write_streamed_export(path: &Path, count: usize, text_bytes: usize) -> (u64, u64) {
    struct Counting<W: Write> {
        inner: W,
        bytes: u64,
    }
    impl<W: Write> Write for Counting<W> {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let n = self.inner.write(buf)?;
            self.bytes += n as u64;
            Ok(n)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            self.inner.flush()
        }
    }
    let name = b"conversations.json";
    let file = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
    let mut out = Counting {
        inner: file,
        bytes: 0,
    };
    // Local header with a data descriptor (flag bit 3): sizes and CRC follow the data.
    let mut header = Vec::new();
    put32(&mut header, 0x0403_4b50);
    put16(&mut header, 20);
    put16(&mut header, (1 << 11) | (1 << 3));
    put16(&mut header, 8);
    put16(&mut header, 0);
    put16(&mut header, 0x5b21);
    put32(&mut header, 0);
    put32(&mut header, 0);
    put32(&mut header, 0);
    put16(&mut header, name.len() as u16);
    put16(&mut header, 0);
    header.extend_from_slice(name);
    out.write_all(&header).unwrap();
    let data_start = out.bytes;
    let mut crc = Crc::new();
    let mut uncompressed = 0u64;
    let mut encoder = DeflateEncoder::new(out, Compression::fast());
    let mut emit = |encoder: &mut DeflateEncoder<Counting<_>>, bytes: &[u8]| {
        crc.update(bytes);
        uncompressed += bytes.len() as u64;
        encoder.write_all(bytes).unwrap();
    };
    emit(&mut encoder, b"[\n");
    for i in 0..count {
        let id = format!("{:08x}-0000-4000-8000-{:012x}", i, i);
        let record = conv(
            &id,
            &format!("Conversation {i}"),
            &filler(i as u64, text_bytes),
        );
        if i > 0 {
            emit(&mut encoder, b",\n");
        }
        emit(&mut encoder, record.as_bytes());
    }
    emit(&mut encoder, b"\n]\n");
    let mut out = encoder.finish().unwrap();
    let compressed = out.bytes - data_start;
    assert!(compressed < u32::MAX as u64 && uncompressed < u32::MAX as u64);
    let crc = crc.sum();
    let mut tail = Vec::new();
    put32(&mut tail, 0x0807_4b50);
    put32(&mut tail, crc);
    put32(&mut tail, compressed as u32);
    put32(&mut tail, uncompressed as u32);
    let cd_offset = out.bytes + tail.len() as u64;
    put32(&mut tail, 0x0201_4b50);
    put16(&mut tail, 0x031e);
    put16(&mut tail, 20);
    put16(&mut tail, (1 << 11) | (1 << 3));
    put16(&mut tail, 8);
    put16(&mut tail, 0);
    put16(&mut tail, 0x5b21);
    put32(&mut tail, crc);
    put32(&mut tail, compressed as u32);
    put32(&mut tail, uncompressed as u32);
    put16(&mut tail, name.len() as u16);
    put16(&mut tail, 0);
    put16(&mut tail, 0);
    put16(&mut tail, 0);
    put16(&mut tail, 0);
    put32(&mut tail, 0);
    put32(&mut tail, 0);
    tail.extend_from_slice(name);
    let cd_size = 46 + name.len() as u32;
    put32(&mut tail, 0x0605_4b50);
    put16(&mut tail, 0);
    put16(&mut tail, 0);
    put16(&mut tail, 1);
    put16(&mut tail, 1);
    put32(&mut tail, cd_size);
    put32(&mut tail, cd_offset as u32);
    put16(&mut tail, 0);
    out.write_all(&tail).unwrap();
    out.flush().unwrap();
    (uncompressed, compressed)
}
