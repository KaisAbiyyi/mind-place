//! MP-9B explicit offline, owner-consented legacy-to-SQLCipher conversion.
//! NEVER auto-run at startup, never delete/overwrite plaintext source.
//! Historical raw archive blobs are also preserved as keyed AEAD ciphertext,
//! including attachments; encrypted readback must succeed before publication.
use crate::vault_archive::VaultCaptureStore;
use mind_place_archive::{validate_capture, Capture, MAX_CAPTURE_BYTES};
use mind_place_security::{
    vault::{ContentKind, CryptoVault, RecoverySecret, VaultManifest},
    vault_migration::{ExplicitMigrationConsent, LegacyMigrationPreflight, MigrationCheckpoint},
    vault_storage::ImmutableVaultStorage,
    Result as SecResult, SecurityError, SecurityGate, UnlockLease,
};
use mind_place_state::{CipherKey, APPLICATION_ID, SCHEMA_VERSION};
use rusqlite::{Connection, OpenFlags};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
use zeroize::Zeroize;

const MAX_RAW_LEGACY_BLOB: u64 = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LegacyConversionCheckpoint {
    StateCopied,
    ArchiveImported,
    RawArchivePreserved,
    DocumentObjectsPreserved,
    Journal(MigrationCheckpoint),
}

pub struct LegacyEncryptedConversion<'a> {
    pub gate: &'a SecurityGate,
    pub lease: UnlockLease,
    pub consent: &'a ExplicitMigrationConsent,
    pub manifest: &'a VaultManifest,
    pub recovery: &'a RecoverySecret,
}

impl LegacyEncryptedConversion<'_> {
    fn crypto(&self) -> SecResult<CryptoVault> {
        CryptoVault::restore_with_recovery(self.gate, self.lease, self.manifest, self.recovery)
    }

    pub fn begin_owner_consented(&self, preflight: &LegacyMigrationPreflight) -> SecResult<()> {
        preflight.begin_owner_consented(self.consent, self.gate, self.lease, &self.crypto()?)
    }

    pub fn resume_owner_consented(
        &self,
        source: &Path,
        destination: &Path,
    ) -> SecResult<LegacyMigrationPreflight> {
        LegacyMigrationPreflight::resume_owner_consented(
            source,
            destination,
            self.consent,
            self.gate,
            self.lease,
            &self.crypto()?,
        )
    }

    pub fn rollback_to_legacy(&self, preflight: &LegacyMigrationPreflight) -> SecResult<()> {
        preflight.rollback_to_legacy(self.consent, self.gate, self.lease, &self.crypto()?)
    }

    /// Requires a separately created durable journal via begin_owner_consented.
    /// Both conversion and verifier run on isolated staging, not live owner data.
    pub fn execute_or_resume(&self, preflight: &LegacyMigrationPreflight) -> SecResult<()> {
        self.execute_or_resume_with_checkpoint(preflight, |_| Ok(()))
    }

    pub fn execute_or_resume_with_checkpoint(
        &self,
        preflight: &LegacyMigrationPreflight,
        checkpoint: impl FnMut(LegacyConversionCheckpoint) -> SecResult<()>,
    ) -> SecResult<()> {
        self.gate.validate_lease(self.lease)?;
        let vault = self.crypto()?;
        let checkpoint = std::cell::RefCell::new(checkpoint);
        let source = preflight.source_root();
        preflight.execute_or_resume_with_checkpoint(
            self.consent,
            self.gate,
            self.lease,
            &vault,
            |source, stage| self.convert(source, stage, &mut *checkpoint.borrow_mut()),
            |stage| self.verify(source, stage),
            |phase| checkpoint.borrow_mut()(LegacyConversionCheckpoint::Journal(phase)),
        )
    }

    fn convert(
        &self,
        source: &Path,
        stage: &Path,
        checkpoint: &mut impl FnMut(LegacyConversionCheckpoint) -> SecResult<()>,
    ) -> SecResult<()> {
        self.gate.validate_lease(self.lease)?;
        self.ensure_manifest(stage)?;
        self.copy_state(source, stage)?;
        checkpoint(LegacyConversionCheckpoint::StateCopied)?;
        self.import_archives(source, stage)?;
        checkpoint(LegacyConversionCheckpoint::ArchiveImported)?;
        self.preserve_raw_archive(source, stage)?;
        checkpoint(LegacyConversionCheckpoint::RawArchivePreserved)?;
        self.preserve_documents(source, stage)?;
        checkpoint(LegacyConversionCheckpoint::DocumentObjectsPreserved)?;
        self.gate.validate_lease(self.lease)
    }

    fn verify(&self, source: &Path, stage: &Path) -> SecResult<()> {
        self.gate.validate_lease(self.lease)?;
        self.ensure_manifest(stage)?;
        self.verify_state(source, stage)?;
        self.verify_archives(source, stage)?;
        self.verify_raw_archive(source, stage)?;
        self.verify_documents(source, stage)?;
        self.verify_layout(source, stage)?;
        self.gate.validate_lease(self.lease)
    }

    fn verify_layout(&self, source: &Path, stage: &Path) -> SecResult<()> {
        use std::collections::BTreeSet;
        let vault = self.crypto()?;
        let mut expected = BTreeSet::from([
            PathBuf::from("vault-v1.json"),
            PathBuf::from("state.sqlite3"),
        ]);
        let mut object = |kind: ContentKind, identity: &str, category: &str| -> SecResult<()> {
            let name = vault.storage_name(self.gate, self.lease, kind, identity.as_bytes())?;
            expected.insert(PathBuf::from(category).join(&name[..2]).join(name));
            Ok(())
        };
        let archive = source.join("archive-v1");
        if archive.exists() {
            for path in all_regular_files(&archive)? {
                object(
                    ContentKind::Archive,
                    &raw_identity(
                        path.strip_prefix(source)
                            .map_err(|_| SecurityError::UnsafePath)?,
                    )?,
                    "archive",
                )?;
            }
        }
        let captures = archive.join("conversations");
        if captures.exists() {
            for (id, snapshot, _) in archive_snapshots(&captures)? {
                object(
                    ContentKind::Archive,
                    &format!("capture/{id}/{snapshot}"),
                    "archive",
                )?;
            }
        }
        let documents = source.join("document-objects");
        if documents.exists() {
            for path in all_regular_files(&documents)? {
                let hash = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .ok_or(SecurityError::InvalidConfig)?;
                object(
                    ContentKind::DocumentObject,
                    &format!("document/{hash}"),
                    "document-objects",
                )?;
            }
        }
        if captures.exists() {
            expected.insert(PathBuf::from("archive-index.sqlite3"));
        }
        for sentinel in [".vault-writer.lock", ".state-schema.lock"] {
            let path = stage.join(sentinel);
            if path.exists() {
                safe_regular(&path)?;
                if fs::metadata(path)?.len() != 0 {
                    return Err(SecurityError::Crypto);
                }
                expected.insert(PathBuf::from(sentinel));
            }
        }
        let actual: BTreeSet<_> = all_regular_files(stage)?
            .into_iter()
            .map(|path| {
                path.strip_prefix(stage)
                    .map(Path::to_owned)
                    .map_err(|_| SecurityError::UnsafePath)
            })
            .collect::<SecResult<_>>()?;
        // Never certify or publish orphan temporary/partial/plaintext bytes.
        // Unknown staging data is retained for explicit rollback/disposition.
        if actual != expected {
            return Err(SecurityError::Crypto);
        }
        Ok(())
    }

    fn ensure_manifest(&self, stage: &Path) -> SecResult<()> {
        let marker = stage.join("vault-v1.json");
        if marker.exists() {
            let actual = VaultManifest::read(stage)?;
            if serde_json::to_vec(&actual).map_err(|_| SecurityError::Crypto)?
                != serde_json::to_vec(self.manifest).map_err(|_| SecurityError::Crypto)?
            {
                return Err(SecurityError::Crypto);
            }
            return Ok(());
        }
        let bytes = serde_json::to_vec(self.manifest).map_err(|_| SecurityError::Crypto)?;
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(marker)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        Ok(())
    }

    fn key(&self) -> SecResult<std::sync::Arc<CipherKey>> {
        let vault = self.crypto()?;
        let derived = vault.derived_key(self.gate, self.lease, b"state/sqlcipher/v1")?;
        let mut raw = [0u8; 32];
        raw.copy_from_slice(derived.as_ref());
        let key = CipherKey::new(raw, self.gate, self.lease);
        raw.zeroize();
        Ok(key)
    }

    fn source_state(&self, source: &Path) -> SecResult<Connection> {
        let file = source.join("state.sqlite3");
        safe_regular(&file)?;
        let mut uri = url::Url::from_file_path(fs::canonicalize(file)?)
            .map_err(|_| SecurityError::UnsafePath)?;
        uri.set_query(Some("mode=ro&immutable=1"));
        let db = Connection::open_with_flags(
            uri.as_str(),
            OpenFlags::SQLITE_OPEN_READ_ONLY
                | OpenFlags::SQLITE_OPEN_NOFOLLOW
                | OpenFlags::SQLITE_OPEN_URI,
        )
        .map_err(|_| SecurityError::Crypto)?;
        sql(db.execute_batch("PRAGMA temp_store = MEMORY"))?;
        let app: i64 = sql(db.query_row("PRAGMA application_id", [], |row| row.get(0)))?;
        let version: i64 = sql(db.query_row("PRAGMA user_version", [], |row| row.get(0)))?;
        if app != APPLICATION_ID || version != SCHEMA_VERSION {
            return Err(SecurityError::UnsupportedSchema);
        }
        sqlite_healthy(&db)?;
        Ok(db)
    }

    fn encrypted_state(&self, stage: &Path) -> SecResult<Connection> {
        let file = stage.join("state.sqlite3");
        safe_regular(&file)?;
        let db = sql(Connection::open_with_flags(
            file,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        ))?;
        self.key()?.unlock(&db).map_err(|_| SecurityError::Crypto)?;
        let app: i64 = sql(db.query_row("PRAGMA application_id", [], |row| row.get(0)))?;
        let version: i64 = sql(db.query_row("PRAGMA user_version", [], |row| row.get(0)))?;
        if app != APPLICATION_ID || version != SCHEMA_VERSION {
            return Err(SecurityError::Crypto);
        }
        sqlite_healthy(&db)?;
        Ok(db)
    }

    fn copy_state(&self, source: &Path, stage: &Path) -> SecResult<()> {
        let original = self.source_state(source)?;
        let target = stage.join("state.sqlite3");
        let partial = stage.join(".state.sqlite3.migration-partial");
        if target.exists() {
            self.verify_state(source, stage)?;
            if partial.exists() {
                safe_regular(&partial)?;
                fs::remove_file(&partial)?;
            }
            return Ok(());
        }
        if partial.exists() {
            safe_regular(&partial)?;
            fs::remove_file(&partial)?;
        }
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&partial)?;
        let dest = sql(Connection::open_with_flags(
            &partial,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_NOFOLLOW
                | OpenFlags::SQLITE_OPEN_URI,
        ))?;
        self.key()?
            .unlock(&dest)
            .map_err(|_| SecurityError::Crypto)?;
        let source_path = fs::canonicalize(source.join("state.sqlite3"))?;
        let mut source_uri =
            url::Url::from_file_path(source_path).map_err(|_| SecurityError::UnsafePath)?;
        source_uri.set_query(Some("mode=ro&immutable=1"));
        sql(dest.execute("ATTACH DATABASE ?1 AS legacy KEY ''", [source_uri.as_str()]))?;
        sql(dest.query_row("SELECT sqlcipher_export('main', 'legacy')", [], |_| Ok(())))?;
        let app: i64 = sql(original.query_row("PRAGMA application_id", [], |r| r.get(0)))?;
        let version: i64 = sql(original.query_row("PRAGMA user_version", [], |r| r.get(0)))?;
        sql(dest.execute_batch(&format!("PRAGMA main.application_id = {app}; PRAGMA main.user_version = {version}; DETACH DATABASE legacy;")))?;
        sqlite_healthy(&dest)?;
        sql(dest.close().map_err(|(_, error)| error))?;
        let mut header = [0u8; 16];
        // Windows FlushFileBuffers requires GENERIC_WRITE. Reopen without
        // truncation, then close this handle before linking/unlinking staging.
        let mut copied = OpenOptions::new().read(true).write(true).open(&partial)?;
        copied.read_exact(&mut header)?;
        if &header == b"SQLite format 3\0" {
            return Err(SecurityError::Crypto);
        }
        copied.sync_all()?;
        drop(copied);
        // Linking is atomic and refuses an existing target on both Unix and
        // Windows; a restart after linking verifies the target before cleanup.
        fs::hard_link(&partial, &target)?;
        fs::remove_file(&partial)?;
        #[cfg(unix)]
        File::open(stage)?.sync_all()?;
        self.verify_state(source, stage)
    }

    fn verify_state(&self, source: &Path, stage: &Path) -> SecResult<()> {
        let original = self.source_state(source)?;
        let migrated = self.encrypted_state(stage)?;
        compare_rows(
            &original,
            &migrated,
            "SELECT type, name, tbl_name, sql FROM sqlite_master ORDER BY type, name",
        )?;
        let mut query = sql(
            original.prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        )?;
        let names = sql(query.query_map([], |row| row.get::<_, String>(0)))?;
        for name in names {
            let name = sql(name)?;
            let escaped = name.replace('"', "\"\"");
            let column_query = format!("SELECT * FROM \"{escaped}\" LIMIT 0");
            let columns = sql(original.prepare(&column_query))?;
            let names: Vec<String> = columns.column_names().map(str::to_owned).collect();
            let without_rowid: i64 = sql(original.query_row(
                "SELECT wr FROM pragma_table_list WHERE schema = 'main' AND name = ?1",
                [&name],
                |row| row.get(0),
            ))?;
            if without_rowid == 0 {
                let rowid = ["_rowid_", "rowid", "oid"]
                    .into_iter()
                    .find(|alias| !names.iter().any(|name| name.eq_ignore_ascii_case(alias)))
                    .ok_or(SecurityError::Crypto)?;
                let statement = format!("SELECT {rowid}, * FROM \"{escaped}\" ORDER BY {rowid}");
                compare_rows(&original, &migrated, &statement)?;
            }
            let order = names.iter().map(|name| {
                let column = format!("\"{}\"", name.replace('"', "\"\""));
                format!("typeof({column}), CASE WHEN typeof({column}) = 'real' THEN printf('%!.26g', {column}) ELSE hex(CAST({column} AS BLOB)) END COLLATE BINARY")
            }).collect::<Vec<_>>().join(", ");
            if order.is_empty() {
                return Err(SecurityError::Crypto);
            }
            let statement = format!("SELECT * FROM \"{escaped}\" ORDER BY {order}");
            compare_rows(&original, &migrated, &statement)?;
        }
        Ok(())
    }

    fn import_archives(&self, source: &Path, stage: &Path) -> SecResult<()> {
        let root = source.join("archive-v1/conversations");
        if !root.exists() {
            return Ok(());
        }
        let vault = self.crypto()?;
        let adapter = VaultCaptureStore::open(stage, vault, self.gate, self.lease)
            .map_err(|_| SecurityError::Crypto)?;
        for (archive_id, snapshot_id, path) in archive_snapshots(&root)? {
            let mut capture = parsed_capture(&path)?;
            validate_capture(&mut capture).map_err(|_| SecurityError::Crypto)?;
            self.check_generic_source(source, &capture)?;
            let receipt = adapter
                .import_legacy_consented(self.consent, capture)
                .map_err(|_| SecurityError::Crypto)?;
            if receipt.id != archive_id || receipt.snapshot_id != snapshot_id {
                return Err(SecurityError::Crypto);
            }
        }
        Ok(())
    }

    fn verify_archives(&self, source: &Path, stage: &Path) -> SecResult<()> {
        let root = source.join("archive-v1/conversations");
        if !root.exists() {
            return Ok(());
        }
        let adapter = VaultCaptureStore::open(stage, self.crypto()?, self.gate, self.lease)
            .map_err(|_| SecurityError::Crypto)?;
        for (archive_id, snapshot_id, path) in archive_snapshots(&root)? {
            let mut expected = parsed_capture(&path)?;
            validate_capture(&mut expected).map_err(|_| SecurityError::Crypto)?;
            let actual = adapter
                .read(&archive_id, Some(&snapshot_id))
                .map_err(|_| SecurityError::Crypto)?;
            if actual.capture != expected {
                return Err(SecurityError::Crypto);
            }
        }
        Ok(())
    }

    fn check_generic_source(&self, source: &Path, capture: &Capture) -> SecResult<()> {
        if capture.provider != "generic" {
            return Ok(());
        }
        let expected = capture.conversation_id.as_str();
        if !hex64(expected) {
            return Err(SecurityError::Crypto);
        }
        let path = source
            .join("archive-v1/imports/generic")
            .join(format!("{expected}.raw"));
        let bytes = bounded_file(&path)?;
        if format!("{:x}", Sha256::digest(&bytes)) != expected {
            return Err(SecurityError::Crypto);
        }
        Ok(())
    }

    fn preserve_raw_archive(&self, source: &Path, stage: &Path) -> SecResult<()> {
        let original = source.join("archive-v1");
        if !original.exists() {
            return Ok(());
        }
        let vault = self.crypto()?;
        let storage = ImmutableVaultStorage::new(stage, &vault, self.gate, self.lease)?;
        for path in all_regular_files(&original)? {
            let bytes = bounded_file(&path)?;
            let relative = path
                .strip_prefix(source)
                .map_err(|_| SecurityError::UnsafePath)?;
            let identity = raw_identity(relative)?;
            storage.write_immutable(
                self.gate,
                self.lease,
                ContentKind::Archive,
                identity.as_bytes(),
                &bytes,
            )?;
        }
        Ok(())
    }

    fn verify_raw_archive(&self, source: &Path, stage: &Path) -> SecResult<()> {
        let original = source.join("archive-v1");
        if !original.exists() {
            return Ok(());
        }
        let vault = self.crypto()?;
        let storage = ImmutableVaultStorage::new(stage, &vault, self.gate, self.lease)?;
        for path in all_regular_files(&original)? {
            let bytes = bounded_file(&path)?;
            let relative = path
                .strip_prefix(source)
                .map_err(|_| SecurityError::UnsafePath)?;
            let identity = raw_identity(relative)?;
            let restored = storage.read(
                self.gate,
                self.lease,
                ContentKind::Archive,
                identity.as_bytes(),
            )?;
            if restored.as_slice() != bytes.as_slice() {
                return Err(SecurityError::Crypto);
            }
        }
        Ok(())
    }

    fn preserve_documents(&self, source: &Path, stage: &Path) -> SecResult<()> {
        self.process_documents(source, stage, false)
    }

    fn verify_documents(&self, source: &Path, stage: &Path) -> SecResult<()> {
        self.process_documents(source, stage, true)
    }

    fn process_documents(&self, source: &Path, stage: &Path, verifying: bool) -> SecResult<()> {
        let original = source.join("document-objects");
        if !original.exists() {
            return Ok(());
        }
        let vault = self.crypto()?;
        let storage = ImmutableVaultStorage::new(stage, &vault, self.gate, self.lease)?;
        for path in all_regular_files(&original)? {
            let relative = path
                .strip_prefix(&original)
                .map_err(|_| SecurityError::UnsafePath)?;
            let parts: Vec<_> = relative.components().collect();
            if parts.len() != 4
                || parts[0].as_os_str().to_str() != Some("v1")
                || parts[1].as_os_str().to_str() != Some("sha256")
            {
                return Err(SecurityError::InvalidConfig);
            }
            let prefix = parts[2]
                .as_os_str()
                .to_str()
                .ok_or(SecurityError::InvalidConfig)?;
            let hash = parts[3]
                .as_os_str()
                .to_str()
                .ok_or(SecurityError::InvalidConfig)?;
            if !hex64(hash) || prefix != &hash[..2] {
                return Err(SecurityError::Crypto);
            }
            let bytes = bounded_file(&path)?;
            if format!("{:x}", Sha256::digest(&bytes)) != hash {
                return Err(SecurityError::Crypto);
            }
            let identity = format!("document/{hash}");
            if verifying {
                let read = storage.read(
                    self.gate,
                    self.lease,
                    ContentKind::DocumentObject,
                    identity.as_bytes(),
                )?;
                if read.as_slice() != bytes.as_slice() {
                    return Err(SecurityError::Crypto);
                }
            } else {
                storage.write_immutable(
                    self.gate,
                    self.lease,
                    ContentKind::DocumentObject,
                    identity.as_bytes(),
                    &bytes,
                )?;
            }
        }
        Ok(())
    }
}

fn compare_rows(left: &Connection, right: &Connection, statement: &str) -> SecResult<()> {
    let mut a = sql(left.prepare(statement))?;
    let mut b = sql(right.prepare(statement))?;
    let count = a.column_count();
    if count != b.column_count() {
        return Err(SecurityError::Crypto);
    }
    let mut a_rows = sql(a.query([]))?;
    let mut b_rows = sql(b.query([]))?;
    loop {
        match (sql(a_rows.next())?, sql(b_rows.next())?) {
            (None, None) => return Ok(()),
            (Some(a_row), Some(b_row)) => {
                for i in 0..count {
                    if sql(a_row.get::<_, rusqlite::types::Value>(i))?
                        != sql(b_row.get::<_, rusqlite::types::Value>(i))?
                    {
                        return Err(SecurityError::Crypto);
                    }
                }
            }
            _ => return Err(SecurityError::Crypto),
        }
    }
}

fn sql<T>(result: rusqlite::Result<T>) -> SecResult<T> {
    result.map_err(|_| SecurityError::Crypto)
}
fn sqlite_healthy(db: &Connection) -> SecResult<()> {
    let health: String = sql(db.query_row("PRAGMA quick_check(1)", [], |r| r.get(0)))?;
    if health != "ok" {
        return Err(SecurityError::Crypto);
    }
    Ok(())
}
fn safe_regular(path: &Path) -> SecResult<()> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        return Err(SecurityError::UnsafePath);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return Err(SecurityError::UnsafePath);
        }
    }
    Ok(())
}
fn bounded_file(path: &Path) -> SecResult<Vec<u8>> {
    safe_regular(path)?;
    let file = File::open(path)?;
    if file.metadata()?.len() > MAX_RAW_LEGACY_BLOB {
        return Err(SecurityError::InvalidConfig);
    }
    let mut bytes = Vec::new();
    file.take(MAX_RAW_LEGACY_BLOB + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_RAW_LEGACY_BLOB {
        return Err(SecurityError::InvalidConfig);
    }
    Ok(bytes)
}
fn all_regular_files(root: &Path) -> SecResult<Vec<PathBuf>> {
    fn descend(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) -> SecResult<()> {
        if depth > 12 || out.len() > 100_000 {
            return Err(SecurityError::InvalidConfig);
        }
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            let meta = fs::symlink_metadata(&path)?;
            if meta.file_type().is_symlink() {
                return Err(SecurityError::UnsafePath);
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if meta.file_attributes() & 0x400 != 0 {
                    return Err(SecurityError::UnsafePath);
                }
            }
            if meta.is_dir() {
                descend(&path, depth + 1, out)?;
            } else if meta.is_file() {
                out.push(path);
            } else {
                return Err(SecurityError::UnsafePath);
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    descend(root, 0, &mut files)?;
    files.sort();
    Ok(files)
}
fn raw_identity(relative: &Path) -> SecResult<String> {
    let name = relative.to_str().ok_or(SecurityError::InvalidConfig)?;
    Ok(format!("legacy-file/v1/{}", name.replace('\\', "/")))
}
fn hex64(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
fn parsed_capture(path: &Path) -> SecResult<Capture> {
    let bytes = bounded_file(path)?;
    if bytes.len() > MAX_CAPTURE_BYTES {
        return Err(SecurityError::InvalidConfig);
    }
    serde_json::from_slice(&bytes).map_err(|_| SecurityError::Crypto)
}
fn archive_snapshots(root: &Path) -> SecResult<Vec<(String, String, PathBuf)>> {
    let mut result = Vec::new();
    for conversation in fs::read_dir(root)? {
        let dir = conversation?.path();
        if !fs::symlink_metadata(&dir)?.is_dir() {
            return Err(SecurityError::UnsafePath);
        }
        let id = dir
            .file_name()
            .and_then(|x| x.to_str())
            .ok_or(SecurityError::InvalidConfig)?;
        if !hex64(id) {
            return Err(SecurityError::InvalidConfig);
        }
        let captures = dir.join("captures");
        if !captures.is_dir() {
            return Err(SecurityError::Crypto);
        }
        for file in fs::read_dir(captures)? {
            let path = file?.path();
            safe_regular(&path)?;
            let snapshot = path
                .file_stem()
                .and_then(|x| x.to_str())
                .ok_or(SecurityError::InvalidConfig)?;
            if path.extension().and_then(|x| x.to_str()) != Some("jsonl") || !hex64(snapshot) {
                return Err(SecurityError::InvalidConfig);
            }
            result.push((id.to_owned(), snapshot.to_owned(), path));
        }
    }
    result.sort();
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mind_place_state::StateStore;

    #[test]
    fn rejects_changed_state_value_with_same_row_count() {
        let home = tempfile::tempdir().unwrap();
        let source = home.path().join("legacy");
        let stage = home.path().join("stage");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&stage).unwrap();
        drop(StateStore::open(source.join("state.sqlite3")).unwrap());
        let db = Connection::open(source.join("state.sqlite3")).unwrap();
        db.execute_batch("CREATE TABLE synthetic_evidence (value TEXT); INSERT INTO synthetic_evidence VALUES ('original');").unwrap();
        drop(db);
        let gate = SecurityGate::open(home.path().join("security.json")).unwrap();
        gate.initialize(b"synthetic owner migration password")
            .unwrap();
        let lease = gate.require_unlocked().unwrap();
        let (_, recovery, manifest) = CryptoVault::provision(&gate, lease).unwrap();
        let consent =
            ExplicitMigrationConsent::from_owner_typed_confirmation("ENCRYPT MY LEGACY VAULT")
                .unwrap();
        let converter = LegacyEncryptedConversion {
            gate: &gate,
            lease,
            consent: &consent,
            manifest: &manifest,
            recovery: &recovery,
        };
        fs::write(
            stage.join(".state.sqlite3.migration-partial"),
            b"interrupted synthetic copy",
        )
        .unwrap();
        converter.copy_state(&source, &stage).unwrap();
        assert!(!stage.join(".state.sqlite3.migration-partial").exists());
        fs::hard_link(
            stage.join("state.sqlite3"),
            stage.join(".state.sqlite3.migration-partial"),
        )
        .unwrap();
        converter.copy_state(&source, &stage).unwrap();
        assert!(!stage.join(".state.sqlite3.migration-partial").exists());
        let db = Connection::open(stage.join("state.sqlite3")).unwrap();
        converter.key().unwrap().unlock(&db).unwrap();
        db.execute("UPDATE synthetic_evidence SET value = 'altered'", [])
            .unwrap();
        drop(db);
        assert!(converter.verify_state(&source, &stage).is_err());
        let db = Connection::open(stage.join("state.sqlite3")).unwrap();
        converter.key().unwrap().unlock(&db).unwrap();
        db.execute(
            "UPDATE synthetic_evidence SET value = 'original', rowid = 900",
            [],
        )
        .unwrap();
        drop(db);
        assert!(converter.verify_state(&source, &stage).is_err());
    }
}
