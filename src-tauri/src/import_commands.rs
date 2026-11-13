//! Owner import of a ChatGPT export or a Gemini Takeout from the desktop app (MP-IMPORT-APP-01).
//!
//! Backend commands only (owner decision: option A). The Archive's Import button arrives with
//! MP-UI2-06. Design: `docs/implementation-plan/2026-11-14-mp-import-app-01-design.md`, section 4.
//!
//! - Every command needs an unlocked owner session and the legacy plaintext archive; the RAW-01
//!   import path refuses an encrypted vault. None of them is reachable from MCP or the extension.
//! - `start_import` re-authenticates the owner, authorizes a capability bound to the measured
//!   source, and always revokes it when the job ends.
//! - The credential is wrapped in `Zeroizing` on arrival and is never logged or echoed. Events
//!   carry counts only, never paths or content.

use super::{begin_unlocked, finish_unlocked, legacy_archive, security_error, AppState};
use mind_place_archive::historical::{
    member_extension_allowed, ImportJobReceipt, ImportMember, ImportPoint, ImportProgress,
    ManifestLimits, MemberKind, SourceImportRequest, SourceManifest,
};
use mind_place_archive::{ArchiveError, ArchiveStore};
use mind_place_security::{SecurityGate, UnlockLease};
use serde::Serialize;
use std::cell::Cell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};
use zeroize::Zeroizing;

/// The cancel flag of the import that is running in this process, if any.
pub(super) type RunningImport = Arc<Mutex<Option<Arc<AtomicBool>>>>;

pub(super) const PROGRESS_EVENT: &str = "import-progress";
const CAPABILITY_TTL: Duration = Duration::from_secs(3600);
const MAX_PATH_CHARS: usize = 4096;
const MAX_EXPLICIT_MEMBERS: usize = 1000;
/// Emit at most one staging/publishing event per this many records.
const EVENT_EVERY: u64 = 64;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CandidateMember {
    pub path: String,
    pub bytes: u64,
}

/// What the owner is shown before authorizing. `source_token` is the manifest hash; it is
/// opaque to the UI and only lets `start_import` refuse a source that changed since.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SourceInspection {
    pub provider: String,
    pub kind: String,
    pub files: usize,
    pub bytes: u64,
    pub stored_bytes: u64,
    pub candidates: Vec<CandidateMember>,
    pub source_token: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProgressEvent {
    /// `staging`, `verified`, `publishing` or `finishing`.
    pub phase: &'static str,
    pub done: u64,
    pub total: Option<u64>,
}

fn coded(code: &str, detail: &str) -> String {
    format!("{code}: {detail}")
}

fn archive_error(error: ArchiveError) -> String {
    match error {
        // Coded, content-free messages from the import path.
        ArchiveError::Invalid(message) => message,
        // Never forward an OS error text (it may name a path).
        ArchiveError::Io(_) => coded("E_STORAGE_IO", "Archive storage could not be written"),
        ArchiveError::Json(_) => coded("E_STORAGE_IO", "Archive storage could not be read"),
    }
}

fn check_provider(provider: &str) -> Result<MemberKind, String> {
    match provider {
        "chatgpt" => Ok(MemberKind::ChatgptConversations),
        "gemini" => Ok(MemberKind::GeminiTakeoutHtml),
        _ => Err(coded("E_IMPORT_PROVIDER", "Unknown import provider")),
    }
}

fn check_path(path: &str) -> Result<PathBuf, String> {
    if path.is_empty() || path.chars().count() > MAX_PATH_CHARS || path.contains('\0') {
        return Err(coded("E_IMPORT_PATH", "Invalid import source path"));
    }
    // Network shares and device namespaces (`\\host\share`, `\\?\UNC\..`, `\\.\..`) are never a
    // source: touching them could send the owner's network credentials to another host.
    if path.starts_with("\\\\") || path.starts_with("//") {
        return Err(coded(
            "E_IMPORT_PATH",
            "Import sources must be on this computer",
        ));
    }
    let path = PathBuf::from(path);
    if !path.is_absolute() {
        return Err(coded("E_IMPORT_PATH", "Invalid import source path"));
    }
    Ok(path)
}

fn measure(path: &Path) -> Result<SourceManifest, String> {
    SourceManifest::measure(path, ManifestLimits::default()).map_err(archive_error)
}

pub(crate) fn inspect_source(provider: &str, path: &str) -> Result<SourceInspection, String> {
    check_provider(provider)?;
    let manifest = measure(&check_path(path)?)?;
    Ok(SourceInspection {
        provider: provider.to_owned(),
        kind: manifest.source_kind().to_owned(),
        files: manifest.total_files(),
        bytes: manifest.total_bytes(),
        stored_bytes: manifest.stored_bytes(),
        candidates: manifest
            .candidate_members(provider)
            .into_iter()
            .map(|entry| CandidateMember {
                path: entry.path.clone(),
                bytes: entry.size,
            })
            .collect(),
        source_token: manifest.sha256_hex(),
    })
}

/// Explicit members must be measured entries with the provider's extension; without a list,
/// the documented-layout candidates are used.
fn choose_members(
    manifest: &SourceManifest,
    provider: &str,
    kind: MemberKind,
    explicit: Option<&[String]>,
) -> Result<Vec<ImportMember>, String> {
    let paths: Vec<String> = match explicit {
        Some(list) => {
            if list.len() > MAX_EXPLICIT_MEMBERS {
                return Err(coded("E_IMPORT_MEMBERS", "Too many import members"));
            }
            let mut seen = HashSet::new();
            for path in list {
                if manifest.entry(path).is_none()
                    || !member_extension_allowed(path, provider)
                    || !seen.insert(path.as_str())
                {
                    return Err(coded(
                        "E_IMPORT_MEMBERS",
                        "An import member is not a measured file of this provider",
                    ));
                }
            }
            list.to_vec()
        }
        None => manifest
            .candidate_members(provider)
            .into_iter()
            .map(|entry| entry.path.clone())
            .collect(),
    };
    if paths.is_empty() {
        return Err(coded(
            "E_IMPORT_NO_MEMBERS",
            "No conversations file was found in this export",
        ));
    }
    Ok(paths
        .into_iter()
        .map(|path| ImportMember { path, kind })
        .collect())
}

/// Bridges job checkpoints to throttled count-only events and honours cancel at points where
/// stopping is clean (while staging, after verification, before an item).
struct CommandProgress<'a> {
    cancel: &'a AtomicBool,
    sink: &'a dyn Fn(ProgressEvent),
    total: Cell<Option<u64>>,
    /// Staging is not fenced by the capability (only publication is), so the lease is checked
    /// at every safe point: Lock stops staging too.
    gate: &'a SecurityGate,
    lease: UnlockLease,
}

impl ImportProgress for CommandProgress<'_> {
    fn checkpoint(&self, point: ImportPoint) -> mind_place_archive::Result<()> {
        let cancellable = matches!(
            point,
            ImportPoint::Staged { .. } | ImportPoint::Verified | ImportPoint::BeforeItem { .. }
        );
        if cancellable && self.gate.validate_lease(self.lease).is_err() {
            return Err(ArchiveError::Invalid(coded(
                "E_AUTHORIZATION",
                "The owner session was locked",
            )));
        }
        if cancellable && self.cancel.load(Ordering::Acquire) {
            return Err(ArchiveError::Invalid(coded(
                "E_IMPORT_CANCELLED",
                "Import stopped by the owner",
            )));
        }
        let event = match point {
            ImportPoint::Staged { seq } if seq % EVENT_EVERY == 0 => Some(ProgressEvent {
                phase: "staging",
                done: seq + 1,
                total: None,
            }),
            ImportPoint::Verified => Some(ProgressEvent {
                phase: "verified",
                done: 0,
                total: None,
            }),
            ImportPoint::AfterItem { seq } => {
                let total = self.total.get();
                let last = total.is_some_and(|total| seq + 1 == total);
                (seq % EVENT_EVERY == 0 || last).then_some(ProgressEvent {
                    phase: "publishing",
                    done: seq + 1,
                    total,
                })
            }
            ImportPoint::BeforeComplete => Some(ProgressEvent {
                phase: "finishing",
                done: self.total.get().unwrap_or(0),
                total: self.total.get(),
            }),
            _ => None,
        };
        if let Some(event) = event {
            (self.sink)(event);
        }
        Ok(())
    }

    fn items_total(&self, total: u64) {
        self.total.set(Some(total));
        (self.sink)(ProgressEvent {
            phase: "publishing",
            done: 0,
            total: Some(total),
        });
    }
}

pub(crate) struct StartRequest<'a> {
    pub provider: &'a str,
    pub path: &'a str,
    pub source_token: &'a str,
    pub members: Option<&'a [String]>,
}

/// Re-authenticates, re-measures, authorizes and runs one import. The capability is revoked
/// whatever the outcome.
pub(crate) fn run_import(
    store: &ArchiveStore,
    gate: &SecurityGate,
    lease: UnlockLease,
    credential: &[u8],
    request: StartRequest<'_>,
    sink: &dyn Fn(ProgressEvent),
    cancel: &AtomicBool,
) -> Result<ImportJobReceipt, String> {
    run_import_with_ttl(
        store,
        gate,
        lease,
        credential,
        request,
        sink,
        cancel,
        CAPABILITY_TTL,
    )
}

#[allow(clippy::too_many_arguments)]
fn run_import_with_ttl(
    store: &ArchiveStore,
    gate: &SecurityGate,
    lease: UnlockLease,
    credential: &[u8],
    request: StartRequest<'_>,
    sink: &dyn Fn(ProgressEvent),
    cancel: &AtomicBool,
    ttl: Duration,
) -> Result<ImportJobReceipt, String> {
    let kind = check_provider(request.provider)?;
    let path = check_path(request.path)?;
    // Verify the owner before reading the source again (fail fast, same backoff as unlock).
    gate.verify_owner_credential(credential)
        .map_err(security_error)?;
    let manifest = measure(&path)?;
    if manifest.sha256_hex() != request.source_token {
        return Err(coded(
            "E_SOURCE_CHANGED",
            "The export changed since it was inspected; inspect it again",
        ));
    }
    let members = choose_members(&manifest, request.provider, kind, request.members)?;
    let capability = gate
        .authorize_import_session(
            lease,
            credential,
            request.provider,
            &path,
            *manifest.sha256(),
            ttl,
        )
        .map_err(security_error)?;
    // Revoked when this function returns or unwinds.
    let _revoke = RevokeOnDrop {
        gate,
        job_id: capability.job_id(),
    };
    let progress = CommandProgress {
        cancel,
        sink,
        total: Cell::new(None),
        gate,
        lease,
    };
    let mut job = SourceImportRequest::new(gate, lease, &capability, &members);
    job.progress = &progress;
    let result = store.import_authorized_source(&job);
    result.map_err(|error| {
        let message = archive_error(error);
        // Still unlocked, so the capability itself ran out (it lives at most an hour) or was
        // revoked. Published items are kept; running the import again resumes the job.
        if message.starts_with("E_AUTHORIZATION") && gate.validate_lease(lease).is_ok() {
            coded(
                "E_IMPORT_AUTH_EXPIRED",
                "The import authorization ended before the job finished; run the import again to continue",
            )
        } else {
            message
        }
    })
}

struct RevokeOnDrop<'a> {
    gate: &'a SecurityGate,
    job_id: &'a str,
}

impl Drop for RevokeOnDrop<'_> {
    fn drop(&mut self) {
        let _ = self.gate.revoke_import_job(self.job_id);
    }
}

/// Holds the process-wide import slot; released on drop.
struct SlotGuard {
    slot: RunningImport,
}

impl Drop for SlotGuard {
    fn drop(&mut self) {
        *self.slot.lock().unwrap_or_else(|error| error.into_inner()) = None;
    }
}

fn claim_slot(slot: &RunningImport) -> Result<(SlotGuard, Arc<AtomicBool>), String> {
    let mut current = slot.lock().unwrap_or_else(|error| error.into_inner());
    if current.is_some() {
        return Err(coded("E_IMPORT_BUSY", "Another import is running"));
    }
    let flag = Arc::new(AtomicBool::new(false));
    *current = Some(flag.clone());
    Ok((SlotGuard { slot: slot.clone() }, flag))
}

/// Asks the running import, if any, to stop at its next safe point. Returns whether one ran.
pub(super) fn cancel_running(slot: &RunningImport) -> bool {
    match slot
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .as_ref()
    {
        Some(flag) => {
            flag.store(true, Ordering::Release);
            true
        }
        None => false,
    }
}

/// Measures an export folder or zip so the owner can review it. Read-only.
#[tauri::command]
pub(super) async fn inspect_import_source(
    state: State<'_, AppState>,
    provider: String,
    path: String,
) -> Result<SourceInspection, String> {
    legacy_archive(&state)?;
    let lease = begin_unlocked(&state)?;
    let report = tauri::async_runtime::spawn_blocking(move || inspect_source(&provider, &path))
        .await
        .map_err(|_| "E_IMPORT_WORKER: import worker failed".to_owned())??;
    finish_unlocked(&state, lease)?;
    Ok(report)
}

/// Imports the inspected export after re-authenticating the owner. Emits `import-progress`.
#[tauri::command]
pub(super) async fn start_import(
    app: AppHandle,
    state: State<'_, AppState>,
    provider: String,
    path: String,
    source_token: String,
    credential: String,
    members: Option<Vec<String>>,
) -> Result<ImportJobReceipt, String> {
    let credential = Zeroizing::new(credential);
    let store = legacy_archive(&state)?;
    let lease = begin_unlocked(&state)?;
    let (guard, cancel) = claim_slot(&state.running_import)?;
    let security = state.security.clone();
    let receipt = tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        let sink = |event: ProgressEvent| {
            let _ = app.emit(PROGRESS_EVENT, event);
        };
        run_import(
            &store,
            &security,
            lease,
            credential.as_bytes(),
            StartRequest {
                provider: &provider,
                path: &path,
                source_token: &source_token,
                members: members.as_deref(),
            },
            &sink,
            &cancel,
        )
    })
    .await
    .map_err(|_| "E_IMPORT_WORKER: import worker failed".to_owned())??;
    finish_unlocked(&state, lease)?;
    Ok(receipt)
}

/// Stops the running import at its next safe point. A verified, partly published job is kept
/// and finished by running `start_import` on the same export again.
#[tauri::command]
pub(super) fn cancel_import(state: State<'_, AppState>) -> bool {
    cancel_running(&state.running_import)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const CRED: &[u8] = b"synthetic-owner-credential";

    fn conv(id: &str, text: &str) -> String {
        serde_json::to_string(&serde_json::json!({
            "id": id, "title": "Synthetic", "create_time": 1700000000.5,
            "update_time": 1700000100.5, "current_node": "n2",
            "mapping": {
                "n1": {"id":"n1","parent":null,"children":["n2"],"message":{
                    "id":"m1","author":{"role":"user"},"create_time":1700000001.0,
                    "content":{"content_type":"text","parts":[text]}}},
                "n2": {"id":"n2","parent":"n1","children":[],"message":{
                    "id":"m2","author":{"role":"assistant"},"create_time":1700000002.0,
                    "content":{"content_type":"text","parts":["reply"]}}}
            }
        }))
        .unwrap()
    }

    fn export(n: usize) -> Vec<u8> {
        let records: Vec<String> = (0..n)
            .map(|i| {
                conv(
                    &format!("{:08x}-aaaa-4aaa-8aaa-aaaaaaaaaaaa", i),
                    "question",
                )
            })
            .collect();
        format!("[{}]", records.join(",")).into_bytes()
    }

    /// Minimal stored-only zip writer for synthetic fixtures (the hostile-archive tests live in
    /// archive-core).
    fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut central = Vec::new();
        for (name, data) in entries {
            let crc = flate2_crc(data);
            let offset = out.len() as u32;
            for part in [
                &0x0403_4b50u32.to_le_bytes()[..],
                &20u16.to_le_bytes(),
                &(1u16 << 11).to_le_bytes(),
                &0u16.to_le_bytes(),
                &0u32.to_le_bytes(),
                &crc.to_le_bytes(),
                &(data.len() as u32).to_le_bytes(),
                &(data.len() as u32).to_le_bytes(),
                &(name.len() as u16).to_le_bytes(),
                &0u16.to_le_bytes(),
                name.as_bytes(),
                data,
            ] {
                out.extend_from_slice(part);
            }
            for part in [
                &0x0201_4b50u32.to_le_bytes()[..],
                &20u16.to_le_bytes(),
                &20u16.to_le_bytes(),
                &(1u16 << 11).to_le_bytes(),
                &0u16.to_le_bytes(),
                &0u32.to_le_bytes(),
                &crc.to_le_bytes(),
                &(data.len() as u32).to_le_bytes(),
                &(data.len() as u32).to_le_bytes(),
                &(name.len() as u16).to_le_bytes(),
                &[0u8; 8],
                &0u32.to_le_bytes(),
                &offset.to_le_bytes(),
                name.as_bytes(),
            ] {
                central.extend_from_slice(part);
            }
        }
        let cd_offset = out.len() as u32;
        out.extend_from_slice(&central);
        for part in [
            &0x0605_4b50u32.to_le_bytes()[..],
            &[0u8; 4],
            &(entries.len() as u16).to_le_bytes(),
            &(entries.len() as u16).to_le_bytes(),
            &(central.len() as u32).to_le_bytes(),
            &cd_offset.to_le_bytes(),
            &0u16.to_le_bytes(),
        ] {
            out.extend_from_slice(part);
        }
        out
    }

    fn flate2_crc(data: &[u8]) -> u32 {
        // CRC-32 (IEEE), bitwise; fixtures are tiny.
        let mut crc = 0xFFFF_FFFFu32;
        for byte in data {
            crc ^= *byte as u32;
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    struct Fx {
        home: tempfile::TempDir,
        store: ArchiveStore,
        gate: SecurityGate,
    }

    impl Fx {
        fn new() -> Self {
            let home = tempfile::tempdir().unwrap();
            let store = ArchiveStore::open(home.path().join("archive-v1")).unwrap();
            let gate = SecurityGate::open(home.path().join("security-v1.json")).unwrap();
            gate.initialize(CRED).unwrap();
            Self { home, store, gate }
        }

        fn write_zip(&self, entries: &[(&str, &[u8])]) -> String {
            let path = self.home.path().join("export.zip");
            let mut file = std::fs::File::create(&path).unwrap();
            file.write_all(&zip(entries)).unwrap();
            path.to_string_lossy().into_owned()
        }

        fn start(
            &self,
            path: &str,
            token: &str,
            credential: &[u8],
            members: Option<&[String]>,
            cancel: &AtomicBool,
            events: &Mutex<Vec<ProgressEvent>>,
        ) -> Result<ImportJobReceipt, String> {
            let sink = |event: ProgressEvent| events.lock().unwrap().push(event);
            run_import(
                &self.store,
                &self.gate,
                self.gate.require_unlocked().unwrap(),
                credential,
                StartRequest {
                    provider: "chatgpt",
                    path,
                    source_token: token,
                    members,
                },
                &sink,
                cancel,
            )
        }

        fn conversations(&self) -> usize {
            let dir = self.store.root().join("conversations");
            if dir.exists() {
                std::fs::read_dir(dir).unwrap().count()
            } else {
                0
            }
        }
    }

    #[test]
    fn inspect_then_start_imports_a_zip_and_reports_count_only_progress() {
        let fx = Fx::new();
        let json = export(3);
        let path = fx.write_zip(&[("conversations.json", &json), ("chat.html", b"<html/>")]);
        let report = inspect_source("chatgpt", &path).unwrap();
        assert_eq!(report.kind, "zip");
        assert_eq!(report.files, 2);
        assert_eq!(
            report.candidates,
            vec![CandidateMember {
                path: "conversations.json".into(),
                bytes: json.len() as u64
            }]
        );
        let events = Mutex::new(Vec::new());
        let receipt = fx
            .start(
                &path,
                &report.source_token,
                CRED,
                None,
                &AtomicBool::new(false),
                &events,
            )
            .unwrap();
        assert_eq!(receipt.imported, 3);
        assert_eq!(fx.conversations(), 3);
        let events = events.into_inner().unwrap();
        assert!(events.iter().any(|e| e.phase == "staging"));
        assert!(events
            .iter()
            .any(|e| e.phase == "publishing" && e.total == Some(3) && e.done == 3));
        assert_eq!(events.last().unwrap().phase, "finishing");
        // Events never carry paths or content: they are counts by construction.
        let serialized = serde_json::to_string(&events).unwrap();
        assert!(!serialized.contains("conversations"));
        // Re-running the same export is idempotent.
        let again = fx
            .start(
                &path,
                &report.source_token,
                CRED,
                None,
                &AtomicBool::new(false),
                &Mutex::new(Vec::new()),
            )
            .unwrap();
        assert!(again.already_complete);
        assert_eq!(fx.conversations(), 3);
    }

    #[test]
    fn a_wrong_credential_writes_nothing_and_backs_off() {
        let fx = Fx::new();
        let path = fx.write_zip(&[("conversations.json", &export(2))]);
        let report = inspect_source("chatgpt", &path).unwrap();
        let message = fx
            .start(
                &path,
                &report.source_token,
                b"wrong-owner-credential",
                None,
                &AtomicBool::new(false),
                &Mutex::new(Vec::new()),
            )
            .unwrap_err();
        assert!(message.starts_with("MP_"), "{message}");
        assert!(!message.contains("wrong-owner-credential"));
        assert_eq!(fx.conversations(), 0);
    }

    #[test]
    fn a_source_changed_since_inspection_is_refused() {
        let fx = Fx::new();
        let path = fx.write_zip(&[("conversations.json", &export(2))]);
        let report = inspect_source("chatgpt", &path).unwrap();
        fx.write_zip(&[("conversations.json", &export(3))]);
        let message = fx
            .start(
                &path,
                &report.source_token,
                CRED,
                None,
                &AtomicBool::new(false),
                &Mutex::new(Vec::new()),
            )
            .unwrap_err();
        assert!(message.starts_with("E_SOURCE_CHANGED"), "{message}");
        assert_eq!(fx.conversations(), 0);
    }

    #[test]
    fn explicit_members_must_be_measured_files_of_the_provider() {
        let fx = Fx::new();
        let json = export(1);
        let path = fx.write_zip(&[("renamed-by-owner.json", &json), ("chat.html", b"<html/>")]);
        let report = inspect_source("chatgpt", &path).unwrap();
        assert!(report.candidates.is_empty());
        let run = |members: Option<&[String]>| {
            fx.start(
                &path,
                &report.source_token,
                CRED,
                members,
                &AtomicBool::new(false),
                &Mutex::new(Vec::new()),
            )
        };
        assert!(run(None).unwrap_err().starts_with("E_IMPORT_NO_MEMBERS"));
        for bad in [
            vec!["chat.html".to_owned()],
            vec!["missing.json".to_owned()],
            vec!["../renamed-by-owner.json".to_owned()],
            vec![
                "renamed-by-owner.json".to_owned(),
                "renamed-by-owner.json".to_owned(),
            ],
        ] {
            assert!(
                run(Some(&bad)).unwrap_err().starts_with("E_IMPORT_MEMBERS"),
                "{bad:?}"
            );
        }
        let receipt = run(Some(&["renamed-by-owner.json".to_owned()])).unwrap();
        assert_eq!(receipt.imported, 1);
    }

    #[test]
    fn cancel_while_staging_leaves_nothing_and_cancel_while_publishing_resumes() {
        let fx = Fx::new();
        let path = fx.write_zip(&[("conversations.json", &export(200))]);
        let report = inspect_source("chatgpt", &path).unwrap();
        // Cancelled before the first record is staged: nothing visible, no job left.
        let message = fx
            .start(
                &path,
                &report.source_token,
                CRED,
                None,
                &AtomicBool::new(true),
                &Mutex::new(Vec::new()),
            )
            .unwrap_err();
        assert!(message.starts_with("E_IMPORT_CANCELLED"), "{message}");
        assert_eq!(fx.conversations(), 0);
        assert!(fx.store.incomplete_jobs().unwrap().is_empty());

        // Cancelled mid-publication: some items published, the verified job is kept and a
        // second run finishes it.
        let cancel = AtomicBool::new(false);
        let events = Mutex::new(Vec::new());
        let sink_cancel = |event: ProgressEvent| {
            if event.phase == "publishing" && event.done >= 65 {
                cancel.store(true, Ordering::Release);
            }
            events.lock().unwrap().push(event);
        };
        let message = run_import(
            &fx.store,
            &fx.gate,
            fx.gate.require_unlocked().unwrap(),
            CRED,
            StartRequest {
                provider: "chatgpt",
                path: &path,
                source_token: &report.source_token,
                members: None,
            },
            &sink_cancel,
            &cancel,
        )
        .unwrap_err();
        assert!(message.starts_with("E_IMPORT_CANCELLED"), "{message}");
        let partial = fx.conversations();
        assert!(partial > 0 && partial < 200, "{partial}");
        assert_eq!(fx.store.incomplete_jobs().unwrap().len(), 1);
        let receipt = fx
            .start(
                &path,
                &report.source_token,
                CRED,
                None,
                &AtomicBool::new(false),
                &Mutex::new(Vec::new()),
            )
            .unwrap();
        assert!(receipt.resumed);
        assert_eq!(fx.conversations(), 200);
    }

    #[test]
    fn an_authorization_that_ends_mid_job_asks_for_a_rerun_that_resumes() {
        let fx = Fx::new();
        let path = fx.write_zip(&[("conversations.json", &export(5))]);
        let report = inspect_source("chatgpt", &path).unwrap();
        // The capability expires while the owner waits between staging and publication.
        let sink = |event: ProgressEvent| {
            if event.phase == "publishing" && event.done == 0 {
                std::thread::sleep(Duration::from_millis(1500));
            }
        };
        let message = run_import_with_ttl(
            &fx.store,
            &fx.gate,
            fx.gate.require_unlocked().unwrap(),
            CRED,
            StartRequest {
                provider: "chatgpt",
                path: &path,
                source_token: &report.source_token,
                members: None,
            },
            &sink,
            &AtomicBool::new(false),
            Duration::from_secs(1),
        )
        .unwrap_err();
        assert!(message.starts_with("E_IMPORT_AUTH_EXPIRED"), "{message}");
        assert_eq!(fx.store.incomplete_jobs().unwrap().len(), 1);
        let receipt = fx
            .start(
                &path,
                &report.source_token,
                CRED,
                None,
                &AtomicBool::new(false),
                &Mutex::new(Vec::new()),
            )
            .unwrap();
        assert!(receipt.resumed);
        assert_eq!(fx.conversations(), 5);
    }

    #[test]
    fn locking_while_staging_stops_before_anything_is_published() {
        let fx = Fx::new();
        let path = fx.write_zip(&[("conversations.json", &export(10))]);
        let report = inspect_source("chatgpt", &path).unwrap();
        let gate = fx.gate.clone();
        let sink = |event: ProgressEvent| {
            if event.phase == "staging" {
                gate.lock();
            }
        };
        let message = run_import(
            &fx.store,
            &fx.gate,
            fx.gate.require_unlocked().unwrap(),
            CRED,
            StartRequest {
                provider: "chatgpt",
                path: &path,
                source_token: &report.source_token,
                members: None,
            },
            &sink,
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert!(message.starts_with("E_AUTHORIZATION"), "{message}");
        assert_eq!(fx.conversations(), 0);
        assert!(fx.store.incomplete_jobs().unwrap().is_empty());
    }

    #[test]
    fn locking_mid_import_stops_publication() {
        let fx = Fx::new();
        let path = fx.write_zip(&[("conversations.json", &export(200))]);
        let report = inspect_source("chatgpt", &path).unwrap();
        let gate = fx.gate.clone();
        let sink = |event: ProgressEvent| {
            if event.phase == "publishing" && event.done >= 65 {
                gate.lock();
            }
        };
        let lease = fx.gate.require_unlocked().unwrap();
        let message = run_import(
            &fx.store,
            &fx.gate,
            lease,
            CRED,
            StartRequest {
                provider: "chatgpt",
                path: &path,
                source_token: &report.source_token,
                members: None,
            },
            &sink,
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert!(message.starts_with("E_AUTHORIZATION"), "{message}");
        let published = fx.conversations();
        assert!(published < 200, "{published}");
        // Locked: no new lease, and the old lease cannot authorize another run.
        assert!(fx.gate.require_unlocked().is_err());
        let stale = run_import(
            &fx.store,
            &fx.gate,
            lease,
            CRED,
            StartRequest {
                provider: "chatgpt",
                path: &path,
                source_token: &report.source_token,
                members: None,
            },
            &|_| {},
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert!(stale.starts_with("MP_"), "{stale}");
        assert_eq!(fx.conversations(), published);
    }

    #[test]
    fn inputs_are_validated_before_any_read() {
        assert!(inspect_source("claude", "/tmp/x.zip")
            .unwrap_err()
            .starts_with("E_IMPORT_PROVIDER"));
        for path in [
            "",
            "relative/export.zip",
            "bad\0path",
            r"\\attacker-host\share\export.zip",
            r"\\?\UNC\attacker-host\share\export.zip",
            r"\\.\pipe\export",
            "//attacker-host/share/export.zip",
        ] {
            assert!(
                inspect_source("chatgpt", path)
                    .unwrap_err()
                    .starts_with("E_IMPORT_PATH"),
                "{path:?}"
            );
        }
        let long = format!("/{}", "a".repeat(MAX_PATH_CHARS + 1));
        assert!(inspect_source("chatgpt", &long)
            .unwrap_err()
            .starts_with("E_IMPORT_PATH"));
    }

    #[test]
    fn only_one_import_runs_at_a_time_and_cancel_reaches_it() {
        let slot: RunningImport = Arc::new(Mutex::new(None));
        assert!(!cancel_running(&slot));
        let (guard, flag) = claim_slot(&slot).unwrap();
        assert!(claim_slot(&slot)
            .err()
            .unwrap()
            .starts_with("E_IMPORT_BUSY"));
        assert!(cancel_running(&slot));
        assert!(flag.load(Ordering::Acquire));
        drop(guard);
        assert!(claim_slot(&slot).is_ok());
    }

    #[test]
    fn os_error_text_is_never_forwarded() {
        let error = ArchiveError::Io(std::io::Error::other("C:\\Users\\someone\\secret path"));
        let message = archive_error(error);
        assert!(!message.contains("someone"), "{message}");
    }
}
