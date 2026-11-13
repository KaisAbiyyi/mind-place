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

use super::manifest::{
    coded, hex, open_regular_no_follow, HashingReader, ManifestLimits, SourceManifest,
};
use crate::importers::chatgpt_export::{ChatGptExportParser, ConversationIdTracker};
use crate::importers::gemini_takeout::{GeminiTakeoutParser, MAX_TAKEOUT_HTML_BYTES};
use crate::importers::{BoundedArrayLimits, ReconciledItem, RecordSpan};
use crate::store::{
    archive_id_for, atomic_write, bounded_read, checked_dir, hex_id, no_link, snapshot_identity,
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
    outcome: ItemOutcome,
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

        let existing = if job_dir.exists() {
            Some(read_state(&job_dir)?)
        } else {
            None
        };
        if let Some(state) = &existing {
            if state.job_key != job_key || state.manifest_sha256 != manifest.sha256_hex() {
                return Err(coded("E_STATE_CORRUPT", "Import job state does not match"));
            }
            if state.phase == JobPhase::Complete {
                let mut receipt: ImportJobReceipt = serde_json::from_slice(&bounded_read(
                    &job_dir.join(RECEIPT_FILE),
                    MAX_STATE_BYTES,
                )?)
                .map_err(|_| coded("E_STATE_CORRUPT", "Import receipt is unreadable"))?;
                let _ = fs::remove_dir_all(job_dir.join("items"));
                receipt.already_complete = true;
                return Ok(receipt);
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
                    fs::remove_dir_all(&job_dir)?;
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
                let _ = fs::remove_dir_all(&job_dir);
                return Err(error);
            }
            // The member bytes are already bound by hash; this only re-checks the file set.
            if let Err(error) = manifest.verify_layout_unchanged(limits.manifest) {
                let _ = fs::remove_dir_all(&job_dir);
                return Err(error);
            }
            write_state(&job_dir, &state)?;
            request.progress.checkpoint(ImportPoint::Verified)?;
        } else {
            write_state(&job_dir, &state)?;
        }

        verify_staged_root(&job_dir, &state)?;
        self.publish_items(request, &manifest, &job_dir, &state)?;
        request.progress.checkpoint(ImportPoint::BeforeComplete)?;
        let receipt = self.finish_job(&job_dir, &mut state, cap.job_id())?;
        Ok(receipt)
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

    /// Removes a job's spool, state and receipt. Published raw, evidence and projections are
    /// never touched. The same import can be run again afterwards.
    pub fn discard_staged_job(&self, job_key: &str) -> Result<()> {
        let dir = self.job_dir(job_key)?;
        let _lock = self.jobs_lock()?;
        no_link(&dir)?;
        if dir.exists() {
            fs::remove_dir_all(&dir)?;
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
            let (file, meta) = open_regular_no_follow(&manifest.root().join(&member.path))?;
            if meta.len() != entry.size {
                return Err(coded(
                    "E_SOURCE_MISMATCH",
                    "An import member differs from its measured size",
                ));
            }
            let mut reader = HashingReader::new(file);
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
                .map_err(|_| coded("E_SOURCE_IO", "Import source file cannot be read"))?;
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
            evidence_exists,
            link_path,
        } = prepared;
        let raw_blob_new = match install {
            Some(temp) => {
                temp.persist_noclobber(&final_raw).map_err(|e| e.error)?;
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
            outcome,
            reason: reason.as_deref(),
            archive_id: archive_id.as_deref(),
            snapshot_id: snapshot_id.as_deref(),
        };
        atomic_write(&link_path, &serde_json::to_vec(&link)?, false)?;
        Ok(ItemReceipt {
            seq,
            outcome,
            reason,
            payload_sha256: item.payload_sha256,
            archive_id,
            snapshot_id,
            raw_blob_new,
            evidence_new,
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
        for seq in 0..state.item_count {
            let done: ItemReceipt = serde_json::from_slice(&bounded_read(
                &items_dir.join(format!("{seq:08}.done.json")),
                MAX_STATE_BYTES,
            )?)
            .map_err(|_| coded("E_STATE_CORRUPT", "An item result is unreadable"))?;
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
                    receipt.exceptions.push(done);
                } else {
                    receipt.exceptions_omitted += 1;
                }
            }
        }
        receipt.raw_bytes_preserved = state.members.iter().map(|m| m.record_bytes).sum();
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
    if error.downcast_ref::<std::io::Error>().is_some() {
        return coded("E_SOURCE_IO", "Import source file cannot be read");
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
    hasher.update((members.len() as u32).to_be_bytes());
    for member in members {
        frame(&mut hasher, member.path.as_bytes());
        frame(&mut hasher, member.kind.label().as_bytes());
    }
    hex(&hasher.finalize())
}

fn evidence_id_for(manifest: &SourceManifest, item: &StagedItem) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"mind-place/import-evidence/v1\0");
    hasher.update(manifest.sha256());
    frame(&mut hasher, item.source.member_path.as_bytes());
    hasher.update(item.source.span.offset.to_be_bytes());
    hasher.update(item.source.span.length.to_be_bytes());
    frame(&mut hasher, item.payload_sha256.as_bytes());
    hex(&hasher.finalize())
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
