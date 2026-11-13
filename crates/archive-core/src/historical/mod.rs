//! Authorized historical import with raw-evidence preservation (MP-IMPORT-RAW-01).
//!
//! `ArchiveStore::import_authorized_source` publishes the raw bytes of every provider-export
//! record together with its projection, bound to an independently measured source manifest.
//! Design, failure model and threat model: `docs/implementation-plan/2026-11-13-mp-import-raw-01-design.md`.
//! Plaintext profile only; the encrypted vault marker fence is enforced before any write.

mod job;
pub mod manifest;

pub use job::{
    ImportJobLimits, ImportJobReceipt, ImportMember, ImportPoint, ImportProgress, IncompleteJob,
    ItemOutcome, ItemReceipt, MemberKind, MemberReceipt, NoProgress, SourceImportRequest,
    IMPORTER_VERSION,
};
pub use manifest::{ManifestEntry, ManifestLimits, SourceManifest};
