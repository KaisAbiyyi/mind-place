//! Journaled, resumable import job: measure, stage, verify, publish (MP-IMPORT-RAW-01).
//!
//! Protocol (see the design note, section 9 for the review decisions that supersede earlier text):
//! 1. verify the capability, measure the source, require the pinned manifest hash;
//! 2. stage every record (raw bytes + validated projection) in a private spool while hashing the
//!    member bytes that are parsed; the member hash must equal its manifest entry;
//! 3. record a root hash over the staged items, re-check the file layout;
//! 4. publish item by item: raw blob, evidence (provenance only), projection, disposition link,
//!    done marker. Every step is content-addressed and idempotent, so running the same import
//!    again repairs any crash.

use super::manifest::{coded, hex, unhex32, HashingReader, ManifestLimits, SourceManifest};
use super::zip::archive_error_from_io;
use crate::importers::chatgpt_export::{ChatGptExportParser, ConversationIdTracker};
use crate::importers::gemini_takeout::{GeminiTakeoutParser, MAX_TAKEOUT_HTML_BYTES};
use crate::importers::{BoundedArrayLimits, ReconciledItem, RecordSpan};
use crate::store::{
    archive_id_for, atomic_write, bounded_read, checked_dir, hex_id, is_contained_real_dir,
    is_transient_sharing, no_link, persist_retry, retry_transient, snapshot_identity,
    sweep_conversation_temp_files, sweep_temp_files,
};
use crate::{ArchiveError, ArchiveStore, Capture, Result, MAX_CAPTURE_BYTES};
use fs2::FileExt;
use mind_place_security::{ImportAuthorizationCapability, SecurityGate, UnlockLease};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Part of the job key: a spool staged by another importer version is never reused.
pub const IMPORTER_VERSION: &str = "mp-import-raw-01/1";

const STATE_FILE: &str = "state.json";
const MANIFEST_FILE: &str = "manifest.json";
const RECEIPT_FILE: &str = "receipt.json";
const MAX_ITEM_JSON_BYTES: usize = MAX_CAPTURE_BYTES + 8 * 1024 * 1024;
const MAX_STATE_BYTES: usize = 8 * 1024 * 1024;
const MAX_RECEIPT_EXCEPTIONS: usize = 1000;
const MAX_SOURCE_IDENTIFIER_BYTES: usize = 1024;
const MAX_METADATA_JSON_BYTES: usize = 64 * 1024;
const MAX_AUTHORIZED_IDS_KEPT: usize = 16;
const LEDGER_FILE: &str = "ledger.jsonl";
const MAX_LEDGER_LINE: u64 = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MemberKind {
    /// A ChatGPT `conversations.json` (or shard): a top-level JSON array of conversations.
    ChatgptConversations,
    /// A Gemini Takeout activity HTML file.
    GeminiTakeoutHtml,
}

impl MemberKind {
    fn provider(self) -> &'static str {
        match self {
            Self::ChatgptConversations => "chatgpt",
            Self::GeminiTakeoutHtml => "gemini",
        }
    }
    fn domain(self) -> &'static str {
        match self {
            Self::ChatgptConversations => "chatgpt-export-dag",
            Self::GeminiTakeoutHtml => "gemini-takeout-html",
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::ChatgptConversations => "chatgpt-conversations",
            Self::GeminiTakeoutHtml => "gemini-takeout-html",
        }
    }
}

/// One source file the job reads. `path` is relative to the authorized source root, `/`
/// separated, and must be an entry of the measured manifest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportMember {
    pub path: String,
    pub kind: MemberKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImportJobLimits {
    pub manifest: ManifestLimits,
    pub max_members: usize,
    /// Most records (including raw-only) one job stages.
    pub max_items: usize,
    /// Most raw bytes one job stages.
    pub max_staged_raw_bytes: u64,
    pub chatgpt_stream: BoundedArrayLimits,
    pub max_takeout_html_bytes: u64,
}

impl Default for ImportJobLimits {
    fn default() -> Self {
        Self {
            manifest: ManifestLimits::default(),
            max_members: 1000,
            max_items: 250_000,
            max_staged_raw_bytes: 64 * 1024 * 1024 * 1024,
            chatgpt_stream: BoundedArrayLimits {
                max_record_bytes: crate::importers::chatgpt_export::MAX_CHATGPT_RECORD_BYTES,
                max_separator_bytes: crate::importers::chatgpt_export::MAX_CHATGPT_SEPARATOR_BYTES,
            },
            max_takeout_html_bytes: MAX_TAKEOUT_HTML_BYTES,
        }
    }
}

/// Named points of the job, for progress display and for fault injection in tests.
///
/// A hook that returns `Err` aborts the job exactly as a crash at that point would leave the
/// disk. Hooks run inside the archive lock and, for the three `After*` publication points, inside
/// the security gate's action: they **must not call back into the gate** from those points.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportPoint {
    Staged {
        seq: u64,
    },
    /// Staging finished and the root hash was recorded; nothing is published yet.
    Verified,
    BeforeItem {
        seq: u64,
    },
    AfterRaw {
        seq: u64,
    },
    AfterEvidence {
        seq: u64,
    },
    AfterProjection {
        seq: u64,
    },
    AfterItem {
        seq: u64,
    },
    BeforeComplete,
}

pub trait ImportProgress {
    fn checkpoint(&self, point: ImportPoint) -> Result<()>;
    /// Called once before publication starts with the number of staged items, so a progress
    /// display can show `seq / total`. Informational; never fails the job.
    fn items_total(&self, _total: u64) {}
}

pub struct NoProgress;

impl ImportProgress for NoProgress {
    fn checkpoint(&self, _point: ImportPoint) -> Result<()> {
        Ok(())
    }
}

/// Everything an authorized import needs. The source root, provider and manifest hash come from
/// the capability the owner authorized; nothing about the source is taken from the caller.
pub struct SourceImportRequest<'a> {
    pub gate: &'a SecurityGate,
    pub lease: UnlockLease,
    pub capability: &'a ImportAuthorizationCapability,
    pub members: &'a [ImportMember],
    pub limits: ImportJobLimits,
    pub progress: &'a dyn ImportProgress,
    /// When a finished job no longer matches what is on disk (a published raw blob, evidence,
    /// link or projection is missing), re-stage and re-publish it instead of failing. The old
    /// receipt and ledger are kept as `*.superseded-N` files. Off by default.
    pub repair: bool,
}

static NO_PROGRESS: NoProgress = NoProgress;

impl<'a> SourceImportRequest<'a> {
    pub fn new(
        gate: &'a SecurityGate,
        lease: UnlockLease,
        capability: &'a ImportAuthorizationCapability,
        members: &'a [ImportMember],
    ) -> Self {
        Self {
            gate,
            lease,
            capability,
            members,
            limits: ImportJobLimits::default(),
            progress: &NO_PROGRESS,
            repair: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ItemOutcome {
    /// A new projection snapshot was published.
    Imported,
    /// The projection snapshot already existed; nothing new was published for it.
    Deduplicated,
    /// Raw evidence preserved; the record has no projection (reason recorded).
    RawOnly,
    /// A projection was offered but refused at this boundary; raw evidence preserved.
    Rejected,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemReceipt {
    pub seq: u64,
    pub outcome: ItemOutcome,
    pub reason: Option<String>,
    pub payload_sha256: String,
    pub archive_id: Option<String>,
    pub snapshot_id: Option<String>,
    pub raw_blob_new: bool,
    pub evidence_new: bool,
    pub evidence_id: String,
    pub payload_bytes: u64,
    /// SHA-256 of the evidence and link records as written, so a deep check detects any change.
    pub evidence_sha256: String,
    pub link_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemberReceipt {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
    pub records: u64,
    /// Sum of the preserved record lengths.
    pub record_bytes: u64,
    /// Member bytes outside any preserved record (array framing, whitespace, text between
    /// Takeout cards). Never zero-sum with the member: this is what a reader must not assume
    /// was preserved as evidence.
    pub unpreserved_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportJobReceipt {
    pub schema_version: u32,
    pub job_key: String,
    pub importer_version: String,
    pub provider: String,
    pub manifest_sha256: String,
    pub staged_root_sha256: String,
    /// Gate job id of the authorization that finished the job (audit only).
    pub authorized_job_id: String,
    /// True when this call only returned the receipt of an already finished job.
    pub already_complete: bool,
    /// `published-now` for a job this call finished; `presence-verified` when a finished job was
    /// returned after checking that every published artifact it lists still exists.
    pub published_check: String,
    /// True when the job needed more than one run. After a resume the single item that was in
    /// flight may be counted `deduplicated` rather than `imported`.
    pub resumed: bool,
    pub members: Vec<MemberReceipt>,
    pub records_scanned: u64,
    pub imported: u64,
    pub deduplicated: u64,
    pub raw_only: u64,
    pub rejected: u64,
    pub raw_blobs_new: u64,
    pub raw_blobs_reused: u64,
    pub evidence_new: u64,
    pub evidence_existing: u64,
    pub raw_bytes_preserved: u64,
    /// Raw-only and rejected items (capped); the full truth is the evidence and link records.
    pub exceptions: Vec<ItemReceipt>,
    pub exceptions_omitted: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IncompleteJob {
    pub job_key: String,
    pub provider: String,
    pub items: u64,
    pub items_published: u64,
    pub staged: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum JobPhase {
    Staging,
    Verified,
    Complete,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct JobState {
    schema_version: u32,
    job_key: String,
    importer_version: String,
    provider: String,
    manifest_sha256: String,
    phase: JobPhase,
    item_count: u64,
    staged_root_sha256: Option<String>,
    members: Vec<MemberReceipt>,
    authorized_job_ids: Vec<String>,
    attempts: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StagedSpan {
    member_index: u32,
    member_path: String,
    member_sha256: String,
    member_bytes: u64,
    record_index: u64,
    span: RecordSpan,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
enum StagedDisposition {
    Projected {
        capture: Box<Capture>,
        snapshot_id: String,
        archive_id: String,
    },
    RawOnly {
        reason: String,
    },
    Rejected {
        reason: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StagedItem {
    schema_version: u32,
    seq: u64,
    source: StagedSpan,
    provider: String,
    envelope_domain: String,
    source_identifier: String,
    payload_sha256: String,
    payload_bytes: u64,
    metadata_json: String,
    disposition: StagedDisposition,
}

/// Provenance only: identical bytes on every replay of the same item, no job, time or
/// disposition (those live in the link record and the done marker).
#[derive(Serialize)]
struct EvidenceRecord<'a> {
    schema_version: u32,
    evidence_id: &'a str,
    provider: &'a str,
    envelope_domain: &'a str,
    source_identifier: &'a str,
    payload_sha256: &'a str,
    payload_bytes: u64,
    manifest_sha256: &'a str,
    member_path: &'a str,
    member_sha256: &'a str,
    member_bytes: u64,
    offset: u64,
    length: u64,
    record_index: u64,
    metadata_json: &'a str,
}

#[derive(Serialize)]
struct LinkRecord<'a> {
    schema_version: u32,
    evidence_id: &'a str,
    importer_version: &'a str,
    /// `projected`, `raw-only` or `rejected`. Not imported-versus-deduplicated: that distinction
    /// depends on whether this run or an earlier one first published the snapshot, so it would
    /// differ between a clean run and a resumed one. It lives in the per-job receipt.
    disposition: &'static str,
    reason: Option<&'a str>,
    archive_id: Option<&'a str>,
    snapshot_id: Option<&'a str>,
}

fn frame(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u32).to_be_bytes());
    hasher.update(bytes);
}

fn sha_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

struct Prepared {
    item: StagedItem,
    evidence_id: String,
    final_raw: PathBuf,
    install: Option<tempfile::NamedTempFile>,
    evidence_path: PathBuf,
    evidence_bytes: Vec<u8>,
    evidence_sha256: String,
    evidence_exists: bool,
    link_path: PathBuf,
}

/// Validates a staged capture at the write boundary; the reason is generic and carries no content.
fn classify_capture(
    mut capture: Capture,
    provider: &str,
) -> std::result::Result<(Capture, String, String), String> {
    crate::validate_historical_capture(&mut capture, provider)
        .map_err(|error| format!("projection rejected: {error}"))?;
    let stored_paths = capture.messages.iter().any(|message| {
        message
            .attachments
            .iter()
            .any(|a| a.data_url.is_some() || a.asset_path.is_some())
    });
    if stored_paths {
        return Err("projection rejected: attachment storage is not supported here".into());
    }
    let snapshot_id = snapshot_identity(&capture).map_err(|_| "projection rejected".to_owned())?;
    let archive_id = archive_id_for(&capture.provider, &capture.conversation_id);
    Ok((capture, snapshot_id, archive_id))
}

impl ArchiveStore {
    fn imports_dir(&self) -> PathBuf {
        self.root.join("imports")
    }

    fn job_dir(&self, job_key: &str) -> Result<PathBuf> {
        if !hex_id(job_key) {
            return Err(coded("E_JOB_KEY", "Invalid import job identity"));
        }
        Ok(self.imports_dir().join("jobs").join(job_key))
    }

    fn sharded(&self, kind: &str, id: &str, extension: &str) -> Result<PathBuf> {
        if !hex_id(id) {
            return Err(coded("E_ID", "Invalid import record identity"));
        }
        Ok(self
            .imports_dir()
            .join(kind)
            .join(&id[..2])
            .join(format!("{id}.{extension}")))
    }

    fn jobs_lock(&self) -> Result<File> {
        self.ensure_not_encrypted_vault()?;
        let imports = self.imports_dir();
        no_link(&imports)?;
        checked_dir(&imports)?;
        checked_dir(&imports.join("jobs"))?;
        let path = imports.join(".jobs.lock");
        no_link(&path)?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)?;
        match file.try_lock_exclusive() {
            Ok(()) => Ok(file),
            Err(error) if error.raw_os_error() == fs2::lock_contended_error().raw_os_error() => {
                Err(coded("E_JOB_BUSY", "Another import is running"))
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Runs the authorized import described in the module documentation.
    ///
    /// Failure model: a failure while staging deletes the spool (nothing was visible); a failure
    /// while publishing keeps the verified spool so the same call can be repeated to finish the
    /// job, under the same or a new authorization. A lock, revoke or expiry stops publication at
    /// the next item with `E_AUTHORIZATION`.
    pub fn import_authorized_source(
        &self,
        request: &SourceImportRequest<'_>,
    ) -> Result<ImportJobReceipt> {
        self.run_authorized_import(request).map_err(|error| match error {
            // A file held open by a virus scanner or similar outlasted the retries: a stable,
            // content-free code instead of a raw OS error. The job stays recoverable.
            ArchiveError::Io(io) if is_transient_sharing(&io) => coded(
                "E_STORAGE_BUSY",
                "A file is in use by another program (for example a virus scanner); run the import again",
            ),
            other => other,
        })
    }

    fn run_authorized_import(&self, request: &SourceImportRequest<'_>) -> Result<ImportJobReceipt> {
        let cap = request.capability;
        let limits = request.limits;
        self.ensure_not_encrypted_vault()?;
        self.require_same_owner(request.gate)?;
        let members = normalize_members(request, cap.provider())?;
        // Fail early when locked, revoked, expired or foreign.
        self.verify_authorization(request, cap.manifest_hash())?;
        let _job_lock = self.jobs_lock()?;

        // Independent measurement at job start, pinned against the capability.
        let manifest = SourceManifest::measure(cap.source_root(), limits.manifest)?;
        if manifest.sha256() != cap.manifest_hash() {
            return Err(coded(
                "E_SOURCE_CHANGED",
                "Import source changed since it was authorized",
            ));
        }
        for member in &members {
            if manifest.entry(&member.path).is_none() {
                return Err(coded(
                    "E_MEMBER_UNKNOWN",
                    "An import member is not part of the measured source",
                ));
            }
        }
        let job_key = job_key_for(cap.provider(), &manifest, &members, &limits);
        let job_dir = self.job_dir(&job_key)?;
        no_link(&job_dir)?;
        // A process killed between creating a temporary file and moving it into place leaves it
        // behind: remove those before this run writes anything.
        self.sweep_import_temp_files(&job_dir)?;

        let mut existing = if job_dir.exists() {
            Some(read_state(&job_dir)?)
        } else {
            None
        };
        if let Some(state) = &existing {
            if state.job_key != job_key || state.manifest_sha256 != manifest.sha256_hex() {
                return Err(coded("E_STATE_CORRUPT", "Import job state does not match"));
            }
        }
        if existing
            .as_ref()
            .is_some_and(|state| state.phase == JobPhase::Complete)
        {
            // A stored receipt is a claim, not proof: check what it lists is still published.
            match self.verify_published(&job_dir, existing.as_ref().unwrap(), false) {
                Ok(mut receipt) => {
                    let _ = fs::remove_dir_all(job_dir.join("items"));
                    receipt.already_complete = true;
                    receipt.published_check = "presence-verified".into();
                    return Ok(receipt);
                }
                Err(error) if !request.repair => return Err(error),
                Err(_) => {
                    // Explicit repair: keep the old audit files, then stage and publish again
                    // (every step is idempotent, so only what is missing is rewritten).
                    supersede_receipt(&job_dir)?;
                    let mut state = existing.take().unwrap();
                    state.phase = JobPhase::Staging;
                    existing = Some(state);
                }
            }
        }

        let mut state = match existing {
            Some(mut state) if state.phase == JobPhase::Verified => {
                state.attempts += 1;
                state
            }
            other => {
                // Never staged, or interrupted while staging: nothing was published; restage.
                if other.is_some() {
                    reset_job_dir(&job_dir)?;
                }
                checked_dir(&job_dir)?;
                let state = JobState {
                    schema_version: 1,
                    job_key: job_key.clone(),
                    importer_version: IMPORTER_VERSION.into(),
                    provider: cap.provider().to_owned(),
                    manifest_sha256: manifest.sha256_hex(),
                    phase: JobPhase::Staging,
                    item_count: 0,
                    staged_root_sha256: None,
                    members: Vec::new(),
                    authorized_job_ids: Vec::new(),
                    attempts: 1,
                };
                atomic_write(
                    &job_dir.join(MANIFEST_FILE),
                    &serde_json::to_vec(manifest.entries())?,
                    false,
                )?;
                write_state(&job_dir, &state)?;
                state
            }
        };
        if !state.authorized_job_ids.iter().any(|id| id == cap.job_id()) {
            state.authorized_job_ids.push(cap.job_id().to_owned());
            if state.authorized_job_ids.len() > MAX_AUTHORIZED_IDS_KEPT {
                state.authorized_job_ids.remove(0);
            }
        }

        if state.phase == JobPhase::Staging {
            if let Err(error) = self.stage_job(request, &manifest, &members, &job_dir, &mut state) {
                let _ = discard_job_dir(&job_dir);
                return Err(error);
            }
            // The member bytes are already bound by hash; this only re-checks the file set.
            if let Err(error) = manifest.verify_layout_unchanged(limits.manifest) {
                let _ = discard_job_dir(&job_dir);
                return Err(error);
            }
            write_state(&job_dir, &state)?;
            request.progress.checkpoint(ImportPoint::Verified)?;
        } else {
            write_state(&job_dir, &state)?;
        }

        verify_staged_root(&job_dir, &state)?;
        request.progress.items_total(state.item_count);
        self.publish_items(request, &manifest, &job_dir, &state)?;
        request.progress.checkpoint(ImportPoint::BeforeComplete)?;
        let receipt = self.finish_job(&job_dir, &mut state, cap.job_id())?;
        Ok(receipt)
    }

    /// Removes leftover temporary files of this job's directories and of the raw, evidence and
    /// disposition shards. Holds the archive lock, so no writer has a live temporary file there.
    fn sweep_import_temp_files(&self, job_dir: &Path) -> Result<usize> {
        let _archive_lock = self.lock()?;
        let mut removed = sweep_temp_files(&self.root, job_dir)
            + sweep_temp_files(&self.root, &job_dir.join("items"));
        for kind in ["raw", "evidence", "dispositions"] {
            removed += self.sweep_shards(kind);
        }
        Ok(removed)
    }

    /// Sweeps every shard directory under `imports/<kind>`; does nothing unless `imports/<kind>`
    /// is reached through real directories only.
    fn sweep_shards(&self, kind: &str) -> usize {
        let base = self.imports_dir().join(kind);
        if !is_contained_real_dir(&self.root, &base) {
            return 0;
        }
        let Ok(shards) = fs::read_dir(&base) else {
            return 0;
        };
        shards
            .flatten()
            .map(|shard| sweep_temp_files(&self.root, &shard.path()))
            .sum()
    }

    /// Maintenance: removes leftover temporary files (named exactly like the ones this crate
    /// creates: `.tmp` plus six letters or digits; regular files only) that a killed process left
    /// in conversation directories, the raw/evidence/disposition shards and job directories.
    /// Returns how many were removed. Fails with `E_JOB_BUSY` while an import runs, and waits for
    /// the archive lock like any writer, so it never races a live write.
    pub fn sweep_orphaned_temp_files(&self) -> Result<usize> {
        self.ensure_not_encrypted_vault()?;
        let _jobs_lock = self.jobs_lock()?;
        let _archive_lock = self.lock()?;
        let mut removed = 0;
        let conversations = self.root.join("conversations");
        if is_contained_real_dir(&self.root, &conversations) {
            if let Ok(entries) = fs::read_dir(&conversations) {
                for entry in entries.flatten() {
                    removed += sweep_conversation_temp_files(&self.root, &entry.path());
                }
            }
        }
        for kind in ["raw", "evidence", "dispositions"] {
            removed += self.sweep_shards(kind);
        }
        let jobs = self.imports_dir().join("jobs");
        if is_contained_real_dir(&self.root, &jobs) {
            if let Ok(entries) = fs::read_dir(&jobs) {
                for job in entries.flatten() {
                    removed += sweep_temp_files(&self.root, &job.path());
                    removed += sweep_temp_files(&self.root, &job.path().join("items"));
                }
            }
        }
        Ok(removed)
    }

    /// Lists jobs that were staged or partly published and not finished.
    pub fn incomplete_jobs(&self) -> Result<Vec<IncompleteJob>> {
        self.ensure_not_encrypted_vault()?;
        let jobs = self.imports_dir().join("jobs");
        if !jobs.exists() {
            return Ok(Vec::new());
        }
        no_link(&jobs)?;
        let mut out = Vec::new();
        for entry in fs::read_dir(&jobs)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if !hex_id(&name) {
                continue;
            }
            let dir = entry.path();
            let Ok(state) = read_state(&dir) else {
                continue;
            };
            if state.phase == JobPhase::Complete {
                continue;
            }
            let published = (0..state.item_count)
                .filter(|seq| {
                    dir.join("items")
                        .join(format!("{seq:08}.done.json"))
                        .exists()
                })
                .count() as u64;
            out.push(IncompleteJob {
                job_key: name,
                provider: state.provider,
                items: state.item_count,
                items_published: published,
                staged: state.phase == JobPhase::Verified,
            });
        }
        out.sort_by(|a, b| a.job_key.cmp(&b.job_key));
        Ok(out)
    }

    /// Removes the spool and state of an **unfinished** job. Needs the owner's unlocked gate for
    /// this archive. A finished job (state `complete` or a receipt on disk) is an audit record and
    /// is refused (`E_JOB_COMPLETE`); published raw, evidence and projections are never touched.
    /// Superseded receipts from an earlier repair are kept. The same import can be run again.
    pub fn discard_staged_job(
        &self,
        gate: &SecurityGate,
        lease: UnlockLease,
        job_key: &str,
    ) -> Result<()> {
        self.ensure_not_encrypted_vault()?;
        self.require_same_owner(gate)?;
        gate.validate_lease(lease).map_err(|_| {
            coded(
                "E_AUTHORIZATION",
                "Owner authorization is required to discard an import job",
            )
        })?;
        let dir = self.job_dir(job_key)?;
        let _lock = self.jobs_lock()?;
        no_link(&dir)?;
        if !dir.exists() {
            return Ok(());
        }
        let complete = dir.join(RECEIPT_FILE).exists()
            || read_state(&dir).is_ok_and(|state| state.phase == JobPhase::Complete);
        if complete {
            return Err(coded(
                "E_JOB_COMPLETE",
                "A finished import job is an audit record and cannot be discarded",
            ));
        }
        discard_job_dir(&dir)
    }

    /// Checks that everything a finished job lists is still published: the raw blob (size), the
    /// evidence record, the disposition link and, for a projected item, its snapshot and
    /// `meta.json`. With `deep`, raw blobs are re-hashed and evidence is parsed. Read-only.
    pub fn verify_published_job(&self, job_key: &str, deep: bool) -> Result<()> {
        self.ensure_not_encrypted_vault()?;
        let dir = self.job_dir(job_key)?;
        let _lock = self.jobs_lock()?;
        no_link(&dir)?;
        let state = read_state(&dir)?;
        if state.phase != JobPhase::Complete {
            return Err(coded("E_JOB_INCOMPLETE", "The import job is not finished"));
        }
        self.verify_published(&dir, &state, deep).map(|_| ())
    }

    /// Reconciles a finished job with the disk: the receipt must exist, describe this job and
    /// equal what the ledger adds up to; every ledger item must still be published. Returns the
    /// validated receipt. Missing, unreadable or foreign receipts are `E_PUBLISHED_MISMATCH`.
    fn verify_published(
        &self,
        job_dir: &Path,
        state: &JobState,
        deep: bool,
    ) -> Result<ImportJobReceipt> {
        let mismatch = |what: &str| coded("E_PUBLISHED_MISMATCH", what);
        let receipt_path = job_dir.join(RECEIPT_FILE);
        no_link(&receipt_path)?;
        let receipt: ImportJobReceipt = serde_json::from_slice(
            &bounded_read(&receipt_path, MAX_STATE_BYTES)
                .map_err(|_| mismatch("The job receipt is missing"))?,
        )
        .map_err(|_| mismatch("The job receipt is unreadable"))?;
        if receipt.job_key != state.job_key
            || receipt.manifest_sha256 != state.manifest_sha256
            || receipt.importer_version != state.importer_version
            || receipt.provider != state.provider
            || Some(&receipt.staged_root_sha256) != state.staged_root_sha256.as_ref()
            || receipt.records_scanned != state.item_count
            || receipt.members != state.members
        {
            return Err(mismatch("The job receipt does not describe this job"));
        }

        let mut expected = receipt.clone();
        expected.imported = 0;
        expected.deduplicated = 0;
        expected.raw_only = 0;
        expected.rejected = 0;
        expected.raw_blobs_new = 0;
        expected.raw_blobs_reused = 0;
        expected.evidence_new = 0;
        expected.evidence_existing = 0;
        expected.exceptions = Vec::new();
        expected.exceptions_omitted = 0;

        let ledger = job_dir.join(LEDGER_FILE);
        no_link(&ledger)?;
        let file = File::open(&ledger).map_err(|_| mismatch("The job ledger is missing"))?;
        let mut reader = std::io::BufReader::new(file);
        let mut seen = 0u64;
        loop {
            let mut line = Vec::new();
            let read = std::io::BufRead::read_until(
                &mut Read::take(&mut reader, MAX_LEDGER_LINE + 1),
                b'\n',
                &mut line,
            )
            .map_err(|_| mismatch("The job ledger cannot be read"))?;
            if read == 0 {
                break;
            }
            if line.len() as u64 > MAX_LEDGER_LINE {
                return Err(mismatch("The job ledger is corrupt"));
            }
            let item: ItemReceipt =
                serde_json::from_slice(&line).map_err(|_| mismatch("The job ledger is corrupt"))?;
            if item.seq != seen {
                return Err(mismatch("The job ledger is out of order"));
            }
            self.check_published_item(&item, state, deep)?;
            tally(&mut expected, &item);
            seen += 1;
        }
        if seen != state.item_count {
            return Err(mismatch("The job ledger is incomplete"));
        }
        expected.raw_bytes_preserved = state.members.iter().map(|m| m.record_bytes).sum();
        if receipt.imported != expected.imported
            || receipt.deduplicated != expected.deduplicated
            || receipt.raw_only != expected.raw_only
            || receipt.rejected != expected.rejected
            || receipt.raw_blobs_new != expected.raw_blobs_new
            || receipt.raw_blobs_reused != expected.raw_blobs_reused
            || receipt.evidence_new != expected.evidence_new
            || receipt.evidence_existing != expected.evidence_existing
            || receipt.raw_bytes_preserved != expected.raw_bytes_preserved
            || receipt.exceptions != expected.exceptions
            || receipt.exceptions_omitted != expected.exceptions_omitted
        {
            return Err(mismatch("The job receipt does not match its ledger"));
        }
        Ok(receipt)
    }

    /// Shallow: presence and size. Deep: the raw blob is re-hashed, and the evidence and link
    /// records must equal the bytes the job wrote (hash recorded in the ledger) and agree with
    /// the ledger and the job (evidence id re-derived from its fields, manifest, member span,
    /// disposition, archive and snapshot ids).
    fn check_published_item(&self, item: &ItemReceipt, state: &JobState, deep: bool) -> Result<()> {
        let mismatch = |what: &str| coded("E_PUBLISHED_MISMATCH", what);
        let raw = self.sharded("raw", &item.payload_sha256, "raw")?;
        no_link(&raw)?;
        match fs::metadata(&raw) {
            Ok(meta) if meta.is_file() && meta.len() == item.payload_bytes => {}
            _ => {
                return Err(mismatch(
                    "A published raw blob is missing or has the wrong size",
                ))
            }
        }
        let evidence = self.sharded("evidence", &item.evidence_id, "json")?;
        no_link(&evidence)?;
        if !evidence.is_file() {
            return Err(mismatch("A provenance record is missing"));
        }
        let link = self.sharded("dispositions", &item.evidence_id, "json")?;
        no_link(&link)?;
        if !link.is_file() {
            return Err(mismatch("A disposition link is missing"));
        }
        if let (Some(archive_id), Some(snapshot_id)) = (&item.archive_id, &item.snapshot_id) {
            if !hex_id(snapshot_id) {
                return Err(mismatch("A projection identity is invalid"));
            }
            let dir = self.conversation_dir(archive_id)?;
            if !dir.join("meta.json").is_file()
                || !dir
                    .join("captures")
                    .join(format!("{snapshot_id}.jsonl"))
                    .is_file()
            {
                return Err(mismatch("A projection is missing"));
            }
        }
        if !deep {
            return Ok(());
        }

        if verify_blob(&raw, &item.payload_sha256, item.payload_bytes).is_err() {
            return Err(mismatch("A published raw blob does not match its hash"));
        }

        let evidence_bytes = bounded_read(&evidence, MAX_STATE_BYTES)?;
        if sha_hex(&evidence_bytes) != item.evidence_sha256 {
            return Err(mismatch("A provenance record was altered"));
        }
        let record: serde_json::Value = serde_json::from_slice(&evidence_bytes)
            .map_err(|_| mismatch("A provenance record is unreadable"))?;
        let manifest_bytes = unhex32(&state.manifest_sha256)
            .ok_or_else(|| mismatch("The job manifest hash is invalid"))?;
        let member_path = record["member_path"].as_str().unwrap_or_default();
        let (offset, length) = (
            record["offset"].as_u64().unwrap_or(u64::MAX),
            record["length"].as_u64().unwrap_or(u64::MAX),
        );
        let member = state.members.iter().find(|m| m.path == member_path);
        let within_member = member.is_some_and(|m| {
            record["member_sha256"] == m.sha256.as_str()
                && record["member_bytes"].as_u64() == Some(m.bytes)
                && offset.checked_add(length).is_some_and(|end| end <= m.bytes)
        });
        if record["evidence_id"] != item.evidence_id.as_str()
            || record["payload_sha256"] != item.payload_sha256.as_str()
            || record["payload_bytes"].as_u64() != Some(item.payload_bytes)
            || length != item.payload_bytes
            || record["manifest_sha256"] != state.manifest_sha256.as_str()
            || record["provider"] != state.provider.as_str()
            || !within_member
            || evidence_id_from_parts(
                &manifest_bytes,
                member_path,
                offset,
                length,
                &item.payload_sha256,
            ) != item.evidence_id
        {
            return Err(mismatch("A provenance record does not match its job"));
        }

        let link_bytes = bounded_read(&link, MAX_STATE_BYTES)?;
        if sha_hex(&link_bytes) != item.link_sha256 {
            return Err(mismatch("A disposition link was altered"));
        }
        let value: serde_json::Value = serde_json::from_slice(&link_bytes)
            .map_err(|_| mismatch("A disposition link is unreadable"))?;
        let disposition = match item.outcome {
            ItemOutcome::Imported | ItemOutcome::Deduplicated => "projected",
            ItemOutcome::RawOnly => "raw-only",
            ItemOutcome::Rejected => "rejected",
        };
        if value["evidence_id"] != item.evidence_id.as_str()
            || value["importer_version"] != state.importer_version.as_str()
            || value["disposition"] != disposition
            || value["reason"] != serde_json::json!(item.reason)
            || value["archive_id"] != serde_json::json!(item.archive_id)
            || value["snapshot_id"] != serde_json::json!(item.snapshot_id)
        {
            return Err(mismatch(
                "A disposition link does not match its ledger entry",
            ));
        }
        Ok(())
    }

    fn verify_authorization(
        &self,
        request: &SourceImportRequest<'_>,
        manifest_hash: &[u8; 32],
    ) -> Result<()> {
        let cap = request.capability;
        request
            .gate
            .verify_import_capability_bound(
                request.lease,
                cap,
                cap.provider(),
                cap.job_id(),
                cap.source_root(),
                manifest_hash,
            )
            .map_err(|_| {
                coded(
                    "E_AUTHORIZATION",
                    "Owner import authorization is invalid or revoked",
                )
            })
    }

    fn stage_job(
        &self,
        request: &SourceImportRequest<'_>,
        manifest: &SourceManifest,
        members: &[ImportMember],
        job_dir: &Path,
        state: &mut JobState,
    ) -> Result<()> {
        let items_dir = job_dir.join("items");
        checked_dir(&items_dir)?;
        let mut stager = Stager {
            request,
            items_dir: &items_dir,
            provider: request.capability.provider(),
            seq: 0,
            raw_total: 0,
            root: Sha256::new(),
            tracker: ConversationIdTracker::default(),
        };
        stager.root.update(ROOT_DOMAIN);
        let mut member_receipts = Vec::new();
        for (index, member) in members.iter().enumerate() {
            self.ensure_not_encrypted_vault()?;
            let entry = manifest
                .entry(&member.path)
                .ok_or_else(|| coded("E_MEMBER_UNKNOWN", "An import member is not measured"))?;
            // A folder file or a zip member (decompressed, size and CRC-32 enforced). Either
            // way the bytes the parser reads are hashed and compared with the manifest below.
            let mut reader = HashingReader::new(manifest.open_member(&member.path)?);
            let ctx = MemberCtx {
                index: index as u32,
                path: member.path.clone(),
                sha256: entry.sha256.clone(),
                bytes: entry.size,
                kind: member.kind,
            };
            let mut records = 0u64;
            let mut record_bytes = 0u64;
            match member.kind {
                MemberKind::ChatgptConversations => {
                    let mut tracker = std::mem::take(&mut stager.tracker);
                    let result = ChatGptExportParser::stream_conversations_json_tracked(
                        &mut reader,
                        request.limits.chatgpt_stream,
                        &mut tracker,
                        |span, item| {
                            records += 1;
                            record_bytes += span.length;
                            stager
                                .add(&ctx, span, item, records - 1)
                                .map_err(|error| Box::new(error) as Box<dyn std::error::Error>)
                        },
                    );
                    stager.tracker = tracker;
                    if let Err(error) = result {
                        return Err(map_stream_error(error));
                    }
                }
                MemberKind::GeminiTakeoutHtml => {
                    let cards = GeminiTakeoutParser::parse_cards_html_spanned(
                        &mut reader,
                        request.limits.max_takeout_html_bytes,
                        request.limits.max_items,
                    )
                    .map_err(map_stream_error)?;
                    for (span, item) in cards {
                        records += 1;
                        record_bytes += span.length;
                        stager.add(&ctx, span, item, records - 1)?;
                    }
                }
            }
            let (bytes, digest) = reader
                .finish_to_eof()
                .map_err(|error| archive_error_from_io(&error))?;
            if bytes != entry.size || hex(&digest) != entry.sha256 {
                return Err(coded(
                    "E_SOURCE_MISMATCH",
                    "An import member differs from the bytes that were measured",
                ));
            }
            member_receipts.push(MemberReceipt {
                path: member.path.clone(),
                bytes,
                sha256: entry.sha256.clone(),
                records,
                record_bytes,
                unpreserved_bytes: bytes.saturating_sub(record_bytes),
            });
        }
        stager.root.update(stager.seq.to_be_bytes());
        state.item_count = stager.seq;
        state.staged_root_sha256 = Some(hex(&stager.root.finalize()));
        state.members = member_receipts;
        state.phase = JobPhase::Verified;
        Ok(())
    }

    fn publish_items(
        &self,
        request: &SourceImportRequest<'_>,
        manifest: &SourceManifest,
        job_dir: &Path,
        state: &JobState,
    ) -> Result<()> {
        let cap = request.capability;
        let items_dir = job_dir.join("items");
        for seq in 0..state.item_count {
            let done_path = items_dir.join(format!("{seq:08}.done.json"));
            if done_path.exists() {
                continue;
            }
            request
                .progress
                .checkpoint(ImportPoint::BeforeItem { seq })?;
            let _archive_lock = self.lock()?;
            // Heavy reads, hashing and temp-file writes happen here, outside the gate.
            let prepared = self.prepare_item(job_dir, seq, state, manifest)?;
            let outcome = request
                .gate
                .with_verified_import_capability_bound(
                    request.lease,
                    cap,
                    cap.provider(),
                    cap.job_id(),
                    cap.source_root(),
                    manifest.sha256(),
                    || self.apply_item(request, prepared, seq),
                )
                .map_err(|_| {
                    coded(
                        "E_AUTHORIZATION",
                        "Owner import authorization is invalid or revoked",
                    )
                })??;
            atomic_write(&done_path, &serde_json::to_vec(&outcome)?, true)?;
            // The staged raw copy is only needed until the item is done.
            let _ = fs::remove_file(items_dir.join(format!("{seq:08}.raw")));
            request
                .progress
                .checkpoint(ImportPoint::AfterItem { seq })?;
        }
        Ok(())
    }

    fn prepare_item(
        &self,
        job_dir: &Path,
        seq: u64,
        state: &JobState,
        manifest: &SourceManifest,
    ) -> Result<Prepared> {
        let items_dir = job_dir.join("items");
        let item: StagedItem = serde_json::from_slice(&bounded_read(
            &items_dir.join(format!("{seq:08}.json")),
            MAX_ITEM_JSON_BYTES,
        )?)
        .map_err(|_| coded("E_STAGING_CORRUPT", "A staged import item is unreadable"))?;
        if item.seq != seq || item.provider != state.provider {
            return Err(coded(
                "E_STAGING_CORRUPT",
                "A staged import item does not match",
            ));
        }
        // A kill during this item's projection write may have left a temporary file in its
        // conversation directory. The caller holds the archive lock, so no writer owns one.
        if let StagedDisposition::Projected { archive_id, .. } = &item.disposition {
            sweep_conversation_temp_files(&self.root, &self.conversation_dir(archive_id)?);
        }
        let evidence_id = evidence_id_for(manifest, &item);
        let final_raw = self.sharded("raw", &item.payload_sha256, "raw")?;
        let spool_raw = items_dir.join(format!("{seq:08}.raw"));

        let install = if final_raw.exists() {
            no_link(&final_raw)?;
            // Existing raw evidence is verified, never replaced.
            verify_blob(&final_raw, &item.payload_sha256, item.payload_bytes).map_err(|_| {
                coded(
                    "E_RAW_CORRUPT",
                    "Existing raw evidence does not match its hash",
                )
            })?;
            None
        } else {
            if !spool_raw.exists() {
                return Err(coded(
                    "E_STAGING_CORRUPT",
                    "A staged raw payload is missing",
                ));
            }
            verify_blob(&spool_raw, &item.payload_sha256, item.payload_bytes)
                .map_err(|_| coded("E_STAGING_CORRUPT", "A staged raw payload is corrupt"))?;
            let parent = final_raw
                .parent()
                .ok_or_else(|| coded("E_ID", "Invalid raw location"))?;
            no_link(&self.imports_dir().join("raw"))?;
            checked_dir(parent)?;
            let mut temp = tempfile::NamedTempFile::new_in(parent)?;
            let mut source = File::open(&spool_raw)?;
            std::io::copy(&mut source, &mut temp)?;
            temp.flush()?;
            temp.as_file().sync_all()?;
            Some(temp)
        };

        let evidence = EvidenceRecord {
            schema_version: 1,
            evidence_id: &evidence_id,
            provider: &item.provider,
            envelope_domain: &item.envelope_domain,
            source_identifier: &item.source_identifier,
            payload_sha256: &item.payload_sha256,
            payload_bytes: item.payload_bytes,
            manifest_sha256: &state.manifest_sha256,
            member_path: &item.source.member_path,
            member_sha256: &item.source.member_sha256,
            member_bytes: item.source.member_bytes,
            offset: item.source.span.offset,
            length: item.source.span.length,
            record_index: item.source.record_index,
            metadata_json: &item.metadata_json,
        };
        let evidence_bytes = serde_json::to_vec(&evidence)?;
        let evidence_sha256 = sha_hex(&evidence_bytes);
        let evidence_path = self.sharded("evidence", &evidence_id, "json")?;
        let evidence_exists = if evidence_path.exists() {
            no_link(&evidence_path)?;
            let existing = bounded_read(&evidence_path, MAX_STATE_BYTES)?;
            if existing != evidence_bytes {
                return Err(coded(
                    "E_EVIDENCE_CONFLICT",
                    "An evidence record exists with different provenance",
                ));
            }
            true
        } else {
            false
        };
        let link_path = self.sharded("dispositions", &evidence_id, "json")?;
        Ok(Prepared {
            item,
            evidence_id,
            final_raw,
            install,
            evidence_path,
            evidence_bytes,
            evidence_sha256,
            evidence_exists,
            link_path,
        })
    }

    /// Runs inside the gate action and the archive lock: only noclobber installs and the
    /// projection write. Hooks here must not call the gate.
    fn apply_item(
        &self,
        request: &SourceImportRequest<'_>,
        prepared: Prepared,
        seq: u64,
    ) -> Result<ItemReceipt> {
        let Prepared {
            item,
            evidence_id,
            final_raw,
            install,
            evidence_path,
            evidence_bytes,
            evidence_sha256,
            evidence_exists,
            link_path,
        } = prepared;
        let raw_blob_new = match install {
            Some(temp) => {
                persist_retry(temp, &final_raw, true)?;
                #[cfg(unix)]
                if let Some(parent) = final_raw.parent() {
                    File::open(parent)?.sync_all()?;
                }
                true
            }
            None => false,
        };
        request.progress.checkpoint(ImportPoint::AfterRaw { seq })?;

        let evidence_new = if evidence_exists {
            false
        } else {
            atomic_write(&evidence_path, &evidence_bytes, true)?;
            true
        };
        request
            .progress
            .checkpoint(ImportPoint::AfterEvidence { seq })?;

        let (outcome, reason, archive_id, snapshot_id) = match item.disposition {
            StagedDisposition::Projected {
                capture,
                snapshot_id,
                archive_id,
            } => match classify_capture(*capture, &item.provider) {
                Ok((capture, expected_snapshot, expected_archive)) => {
                    if expected_snapshot != snapshot_id || expected_archive != archive_id {
                        return Err(coded(
                            "E_STAGING_CORRUPT",
                            "A staged projection identity does not match",
                        ));
                    }
                    let saved = self.save_under_lock(capture)?;
                    if saved.snapshot_id != snapshot_id || saved.id != archive_id {
                        return Err(coded(
                            "E_SNAPSHOT_MISMATCH",
                            "The stored snapshot differs from the staged identity",
                        ));
                    }
                    let outcome = if saved.unchanged {
                        ItemOutcome::Deduplicated
                    } else {
                        ItemOutcome::Imported
                    };
                    (outcome, None, Some(archive_id), Some(snapshot_id))
                }
                Err(reason) => (ItemOutcome::Rejected, Some(reason), None, None),
            },
            StagedDisposition::RawOnly { reason } => {
                (ItemOutcome::RawOnly, Some(reason), None, None)
            }
            StagedDisposition::Rejected { reason } => {
                (ItemOutcome::Rejected, Some(reason), None, None)
            }
        };
        request
            .progress
            .checkpoint(ImportPoint::AfterProjection { seq })?;

        let link = LinkRecord {
            schema_version: 1,
            evidence_id: &evidence_id,
            importer_version: IMPORTER_VERSION,
            disposition: match outcome {
                ItemOutcome::Imported | ItemOutcome::Deduplicated => "projected",
                ItemOutcome::RawOnly => "raw-only",
                ItemOutcome::Rejected => "rejected",
            },
            reason: reason.as_deref(),
            archive_id: archive_id.as_deref(),
            snapshot_id: snapshot_id.as_deref(),
        };
        let link_bytes = serde_json::to_vec(&link)?;
        atomic_write(&link_path, &link_bytes, false)?;
        Ok(ItemReceipt {
            seq,
            outcome,
            reason,
            payload_sha256: item.payload_sha256,
            archive_id,
            snapshot_id,
            raw_blob_new,
            evidence_new,
            evidence_id,
            payload_bytes: item.payload_bytes,
            evidence_sha256,
            link_sha256: sha_hex(&link_bytes),
        })
    }

    fn finish_job(
        &self,
        job_dir: &Path,
        state: &mut JobState,
        authorized_job_id: &str,
    ) -> Result<ImportJobReceipt> {
        let items_dir = job_dir.join("items");
        let mut receipt = ImportJobReceipt {
            schema_version: 1,
            job_key: state.job_key.clone(),
            importer_version: state.importer_version.clone(),
            provider: state.provider.clone(),
            manifest_sha256: state.manifest_sha256.clone(),
            staged_root_sha256: state.staged_root_sha256.clone().unwrap_or_default(),
            authorized_job_id: authorized_job_id.to_owned(),
            already_complete: false,
            published_check: "published-now".into(),
            resumed: state.attempts > 1,
            members: state.members.clone(),
            records_scanned: state.item_count,
            imported: 0,
            deduplicated: 0,
            raw_only: 0,
            rejected: 0,
            raw_blobs_new: 0,
            raw_blobs_reused: 0,
            evidence_new: 0,
            evidence_existing: 0,
            raw_bytes_preserved: 0,
            exceptions: Vec::new(),
            exceptions_omitted: 0,
        };
        // One line per item: the ledger lets a later call check the job against the disk.
        let mut ledger = tempfile::NamedTempFile::new_in(job_dir)?;
        for seq in 0..state.item_count {
            let done: ItemReceipt = serde_json::from_slice(&bounded_read(
                &items_dir.join(format!("{seq:08}.done.json")),
                MAX_STATE_BYTES,
            )?)
            .map_err(|_| coded("E_STATE_CORRUPT", "An item result is unreadable"))?;
            ledger.write_all(&serde_json::to_vec(&done)?)?;
            ledger.write_all(b"\n")?;
            tally(&mut receipt, &done);
        }
        receipt.raw_bytes_preserved = state.members.iter().map(|m| m.record_bytes).sum();
        ledger.flush()?;
        ledger.as_file().sync_all()?;
        persist_retry(ledger, &job_dir.join(LEDGER_FILE), false)?;
        atomic_write(
            &job_dir.join(RECEIPT_FILE),
            &serde_json::to_vec(&receipt)?,
            false,
        )?;
        state.phase = JobPhase::Complete;
        write_state(job_dir, state)?;
        let _ = fs::remove_dir_all(&items_dir);
        Ok(receipt)
    }
}

/// Adds one item's result to a receipt's counters. Used when a job finishes and again when a
/// finished job is reconciled with its ledger, so both always agree on the rules.
fn tally(receipt: &mut ImportJobReceipt, done: &ItemReceipt) {
    match done.outcome {
        ItemOutcome::Imported => receipt.imported += 1,
        ItemOutcome::Deduplicated => receipt.deduplicated += 1,
        ItemOutcome::RawOnly => receipt.raw_only += 1,
        ItemOutcome::Rejected => receipt.rejected += 1,
    }
    if done.raw_blob_new {
        receipt.raw_blobs_new += 1;
    } else {
        receipt.raw_blobs_reused += 1;
    }
    if done.evidence_new {
        receipt.evidence_new += 1;
    } else {
        receipt.evidence_existing += 1;
    }
    if matches!(done.outcome, ItemOutcome::RawOnly | ItemOutcome::Rejected) {
        if receipt.exceptions.len() < MAX_RECEIPT_EXCEPTIONS {
            receipt.exceptions.push(done.clone());
        } else {
            receipt.exceptions_omitted += 1;
        }
    }
}

const ROOT_DOMAIN: &[u8] = b"mind-place/import-staged-root/v1\0";

struct MemberCtx {
    index: u32,
    path: String,
    sha256: String,
    bytes: u64,
    kind: MemberKind,
}

struct Stager<'a> {
    request: &'a SourceImportRequest<'a>,
    items_dir: &'a Path,
    provider: &'a str,
    seq: u64,
    raw_total: u64,
    root: Sha256,
    tracker: ConversationIdTracker,
}

impl Stager<'_> {
    fn add(
        &mut self,
        member: &MemberCtx,
        span: RecordSpan,
        item: ReconciledItem,
        record_index: u64,
    ) -> Result<()> {
        let limits = &self.request.limits;
        if self.seq >= limits.max_items as u64 {
            return Err(coded("E_SOURCE_LIMIT", "Import has too many records"));
        }
        let (envelope, capture, reason, assets_empty) = match item {
            ReconciledItem::Conversation {
                capture,
                raw_evidence,
                assets,
            } => (raw_evidence, Some(capture), None, assets.is_empty()),
            ReconciledItem::PreservedRawOnly {
                evidence,
                disposition_reason,
                assets,
            } => (evidence, None, Some(disposition_reason), assets.is_empty()),
        };
        if !assets_empty {
            return Err(coded(
                "E_ASSETS_UNSUPPORTED",
                "Asset ingestion is not part of this import path",
            ));
        }
        let payload_len = envelope.payload_bytes.len() as u64;
        if envelope.provider != self.provider
            || envelope.envelope_domain != member.kind.domain()
            || payload_len != span.length
            || sha_hex(&envelope.payload_bytes) != span.sha256
            || envelope.payload_sha256 != span.sha256
            || envelope.source_identifier.len() > MAX_SOURCE_IDENTIFIER_BYTES
            || envelope.metadata_json.len() > MAX_METADATA_JSON_BYTES
            || serde_json::from_str::<serde_json::Value>(&envelope.metadata_json).is_err()
        {
            return Err(coded(
                "E_ENVELOPE",
                "A parsed record does not match the bytes it was cut from",
            ));
        }
        self.raw_total = self.raw_total.saturating_add(payload_len);
        if self.raw_total > limits.max_staged_raw_bytes {
            return Err(coded("E_SOURCE_LIMIT", "Import raw bytes exceed the limit"));
        }
        let disposition = match capture {
            Some(capture) => match classify_capture(capture, self.provider) {
                Ok((capture, snapshot_id, archive_id)) => StagedDisposition::Projected {
                    capture: Box::new(capture),
                    snapshot_id,
                    archive_id,
                },
                Err(reason) => StagedDisposition::Rejected { reason },
            },
            None => StagedDisposition::RawOnly {
                reason: reason.unwrap_or_default(),
            },
        };
        let staged = StagedItem {
            schema_version: 1,
            seq: self.seq,
            source: StagedSpan {
                member_index: member.index,
                member_path: member.path.clone(),
                member_sha256: member.sha256.clone(),
                member_bytes: member.bytes,
                record_index,
                span,
            },
            provider: envelope.provider.clone(),
            envelope_domain: envelope.envelope_domain.clone(),
            source_identifier: envelope.source_identifier.clone(),
            payload_sha256: envelope.payload_sha256.clone(),
            payload_bytes: payload_len,
            metadata_json: envelope.metadata_json.clone(),
            disposition,
        };
        atomic_write(
            &self.items_dir.join(format!("{:08}.raw", self.seq)),
            &envelope.payload_bytes,
            true,
        )?;
        let bytes = serde_json::to_vec(&staged)?;
        if bytes.len() > MAX_ITEM_JSON_BYTES {
            return Err(coded("E_SOURCE_LIMIT", "A staged import item is too large"));
        }
        atomic_write(
            &self.items_dir.join(format!("{:08}.json", self.seq)),
            &bytes,
            true,
        )?;
        self.root.update(Sha256::digest(&bytes));
        self.request
            .progress
            .checkpoint(ImportPoint::Staged { seq: self.seq })?;
        self.seq += 1;
        Ok(())
    }
}

fn map_stream_error(error: Box<dyn std::error::Error>) -> ArchiveError {
    let error = match error.downcast::<ArchiveError>() {
        Ok(error) => return *error,
        Err(error) => error,
    };
    if let Some(io) = error.downcast_ref::<std::io::Error>() {
        // A zip member reader reports corruption as an io::Error carrying a coded error.
        return archive_error_from_io(io);
    }
    let text = error.to_string();
    if text.contains("limit") || text.contains("exceeds") || text.contains("more cards") {
        coded("E_SOURCE_LIMIT", "Import source exceeds a safety limit")
    } else {
        coded("E_SOURCE_PARSE", "Import source file could not be parsed")
    }
}

fn normalize_members(
    request: &SourceImportRequest<'_>,
    provider: &str,
) -> Result<Vec<ImportMember>> {
    let mut members: Vec<ImportMember> = request.members.to_vec();
    if members.is_empty() || members.len() > request.limits.max_members {
        return Err(coded(
            "E_MEMBERS",
            "Import member list is empty or too long",
        ));
    }
    if members.iter().any(|m| m.kind.provider() != provider) {
        return Err(coded(
            "E_MEMBERS",
            "An import member does not belong to the authorized provider",
        ));
    }
    members.sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));
    if members.windows(2).any(|pair| pair[0].path == pair[1].path) {
        return Err(coded("E_MEMBERS", "An import member is listed twice"));
    }
    Ok(members)
}

fn job_key_for(
    provider: &str,
    manifest: &SourceManifest,
    members: &[ImportMember],
    limits: &ImportJobLimits,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"mind-place/import-job/v1\0");
    frame(&mut hasher, IMPORTER_VERSION.as_bytes());
    frame(&mut hasher, provider.as_bytes());
    hasher.update(manifest.sha256());
    hasher.update((limits.chatgpt_stream.max_record_bytes as u64).to_be_bytes());
    hasher.update((limits.chatgpt_stream.max_separator_bytes as u64).to_be_bytes());
    hasher.update(limits.max_takeout_html_bytes.to_be_bytes());
    hasher.update((limits.max_items as u64).to_be_bytes());
    hasher.update(limits.max_staged_raw_bytes.to_be_bytes());
    hasher.update((members.len() as u32).to_be_bytes());
    for member in members {
        frame(&mut hasher, member.path.as_bytes());
        frame(&mut hasher, member.kind.label().as_bytes());
    }
    hex(&hasher.finalize())
}

fn evidence_id_for(manifest: &SourceManifest, item: &StagedItem) -> String {
    evidence_id_from_parts(
        manifest.sha256(),
        &item.source.member_path,
        item.source.span.offset,
        item.source.span.length,
        &item.payload_sha256,
    )
}

fn evidence_id_from_parts(
    manifest_sha256: &[u8; 32],
    member_path: &str,
    offset: u64,
    length: u64,
    payload_sha256: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"mind-place/import-evidence/v1\0");
    hasher.update(manifest_sha256);
    frame(&mut hasher, member_path.as_bytes());
    hasher.update(offset.to_be_bytes());
    hasher.update(length.to_be_bytes());
    frame(&mut hasher, payload_sha256.as_bytes());
    hex(&hasher.finalize())
}

/// Removes everything in a job directory except superseded receipts and ledgers (audit files).
fn reset_job_dir(job_dir: &Path) -> Result<()> {
    for entry in fs::read_dir(job_dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("receipt.superseded-") || name.starts_with("ledger.superseded-") {
            continue;
        }
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            retry_transient(|| fs::remove_dir_all(&path))?;
        } else {
            retry_transient(|| fs::remove_file(&path))?;
        }
    }
    Ok(())
}

/// As [`reset_job_dir`], then removes the directory itself when nothing is left to keep.
fn discard_job_dir(job_dir: &Path) -> Result<()> {
    if job_dir.exists() {
        reset_job_dir(job_dir)?;
        let _ = fs::remove_dir(job_dir);
    }
    Ok(())
}

/// Keeps the receipt and ledger of a finished job under `*.superseded-N` before it is repaired.
fn supersede_receipt(job_dir: &Path) -> Result<()> {
    let mut n = 1u32;
    while job_dir
        .join(format!("receipt.superseded-{n}.json"))
        .exists()
    {
        n += 1;
    }
    for (from, to) in [
        (
            RECEIPT_FILE.to_owned(),
            format!("receipt.superseded-{n}.json"),
        ),
        (
            LEDGER_FILE.to_owned(),
            format!("ledger.superseded-{n}.jsonl"),
        ),
    ] {
        if job_dir.join(&from).exists() {
            retry_transient(|| fs::rename(job_dir.join(&from), job_dir.join(&to)))?;
        }
    }
    Ok(())
}

fn read_state(job_dir: &Path) -> Result<JobState> {
    serde_json::from_slice(&bounded_read(&job_dir.join(STATE_FILE), MAX_STATE_BYTES)?)
        .map_err(|_| coded("E_STATE_CORRUPT", "Import job state is unreadable"))
}

fn write_state(job_dir: &Path, state: &JobState) -> Result<()> {
    atomic_write(
        &job_dir.join(STATE_FILE),
        &serde_json::to_vec(state)?,
        false,
    )
}

/// Re-hashes every staged item file and compares with the root recorded when staging finished.
fn verify_staged_root(job_dir: &Path, state: &JobState) -> Result<()> {
    let mut root = Sha256::new();
    root.update(ROOT_DOMAIN);
    for seq in 0..state.item_count {
        let bytes = bounded_read(
            &job_dir.join("items").join(format!("{seq:08}.json")),
            MAX_ITEM_JSON_BYTES,
        )
        .map_err(|_| coded("E_STAGING_CORRUPT", "A staged import item is missing"))?;
        root.update(Sha256::digest(&bytes));
    }
    root.update(state.item_count.to_be_bytes());
    if Some(hex(&root.finalize())) != state.staged_root_sha256 {
        return Err(coded(
            "E_STAGING_CORRUPT",
            "The staged import does not match its recorded hash",
        ));
    }
    Ok(())
}

/// Streams a file and requires the expected hash and length.
fn verify_blob(path: &Path, sha256: &str, length: u64) -> Result<()> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let read = match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error.into()),
        };
        hasher.update(&buffer[..read]);
        total += read as u64;
    }
    if total != length || hex(&hasher.finalize()) != sha256 {
        return Err(coded("E_BLOB", "Blob does not match its hash"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Attachment, Message};

    fn capture(attachment: Option<Attachment>) -> Capture {
        Capture {
            schema_version: 1,
            provider: "chatgpt".into(),
            conversation_id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa".into(),
            source_url: "mindplace-import://chatgpt-export/aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"
                .into(),
            title: "t".into(),
            conversation_created_at: None,
            conversation_updated_at: None,
            captured_at: "2026-11-13T00:00:00Z".into(),
            capture_method: "export-import".into(),
            completeness: "partial".into(),
            warnings: vec!["w".into()],
            messages: vec![Message {
                id: "m".into(),
                role: "user".into(),
                text: "hello".into(),
                format: "plain".into(),
                authored_at: None,
                model: None,
                attachments: attachment.into_iter().collect(),
            }],
        }
    }

    fn attachment(data_url: Option<&str>, asset_path: Option<&str>) -> Attachment {
        Attachment {
            id: "a".into(),
            name: "n".into(),
            mime_type: "image/png".into(),
            source_url: None,
            data_url: data_url.map(Into::into),
            asset_path: asset_path.map(Into::into),
            unavailable_reason: None,
        }
    }

    #[test]
    fn captures_that_would_store_assets_are_rejected_with_a_generic_reason() {
        assert!(classify_capture(capture(None), "chatgpt").is_ok());
        for bad in [
            attachment(Some("data:image/png;base64,AAAA"), None),
            attachment(None, Some("x.png")),
        ] {
            let reason = classify_capture(capture(Some(bad)), "chatgpt").unwrap_err();
            assert!(reason.starts_with("projection rejected"));
            assert!(!reason.contains("AAAA") && !reason.contains("x.png"));
        }
        // A provider mismatch is a rejection too, never a silent re-labelling.
        assert!(classify_capture(capture(None), "gemini").is_err());
    }

    #[test]
    fn the_snapshot_id_is_the_one_the_store_computes_and_ignores_the_observation_time() {
        let (_, one, archive_one) = classify_capture(capture(None), "chatgpt").unwrap();
        let mut later = capture(None);
        later.captured_at = "2030-01-01T00:00:00Z".into();
        let (_, two, archive_two) = classify_capture(later, "chatgpt").unwrap();
        assert_eq!(one, two);
        assert_eq!(archive_one, archive_two);
        let mut changed = capture(None);
        changed.title = "different".into();
        assert_ne!(one, classify_capture(changed, "chatgpt").unwrap().1);
    }

    #[test]
    fn job_keys_depend_on_provider_manifest_members_and_limits() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("c.json"), b"[]").unwrap();
        let manifest = SourceManifest::measure(dir.path(), ManifestLimits::default()).unwrap();
        let member = |kind| {
            vec![ImportMember {
                path: "c.json".into(),
                kind,
            }]
        };
        let limits = ImportJobLimits::default();
        let base = job_key_for(
            "chatgpt",
            &manifest,
            &member(MemberKind::ChatgptConversations),
            &limits,
        );
        assert_eq!(base.len(), 64);
        assert_ne!(
            base,
            job_key_for(
                "gemini",
                &manifest,
                &member(MemberKind::ChatgptConversations),
                &limits
            )
        );
        assert_ne!(
            base,
            job_key_for(
                "chatgpt",
                &manifest,
                &member(MemberKind::GeminiTakeoutHtml),
                &limits
            )
        );
        let mut other = limits;
        other.max_items = 7;
        assert_ne!(
            base,
            job_key_for(
                "chatgpt",
                &manifest,
                &member(MemberKind::ChatgptConversations),
                &other
            )
        );
    }
}
