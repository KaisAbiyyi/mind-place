//! MP-R1-WP01 / MP-IMPORT-L1 local qualification harness (owner-run, disposable archive only).
//!
//! Runs the same raw-first job the app's import commands use (`import_authorized_source`) on a
//! real export folder or `.zip`, into a **new, empty, disposable** archive directory, under a
//! throwaway security gate created inside that directory. It prints **counts and structural names
//! only**: no titles, no message text, no identifiers, no file names other than the selected
//! export members. See `docs/qualification/MP-R1-WP01-import-trio-local-runbook.md`.
//!
//! ```text
//! cargo run --release -p mind-place-archive --example import_trio_local -- \
//!     <chatgpt|gemini|claude> <export folder or .zip> <new empty disposable directory>
//! ```

use mind_place_archive::historical::{
    ImportJobLimits, ImportMember, ManifestLimits, MemberKind, NoProgress, SourceImportRequest,
    SourceManifest,
};
use mind_place_archive::importers::gemini_takeout::GeminiTakeoutParser;
use mind_place_archive::ArchiveStore;
use mind_place_security::SecurityGate;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

type BoxError = Box<dyn std::error::Error>;

fn main() -> Result<(), BoxError> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [provider, source, scratch] = args.as_slice() else {
        return Err(
            "usage: import_trio_local <chatgpt|gemini|claude> <export folder or .zip> <new empty directory>"
                .into(),
        );
    };
    let provider = provider.as_str();
    if !matches!(provider, "chatgpt" | "gemini" | "claude") {
        return Err("provider must be chatgpt, gemini or claude".into());
    }
    let source = std::fs::canonicalize(source)?;
    let scratch = PathBuf::from(scratch);
    refuse_unless_disposable(&scratch, &source)?;
    std::fs::create_dir_all(&scratch)?;
    let scratch = std::fs::canonicalize(&scratch)?;

    // A throwaway gate with a random credential: this harness never touches the owner's gate.
    let mut secret = [0u8; 32];
    getrandom::fill(&mut secret).map_err(|_| "no OS randomness")?;
    let gate = SecurityGate::open(scratch.join("security-v1.json"))?;
    gate.initialize(&secret)?;
    let store = ArchiveStore::open(scratch.join("archive-v1"))?;

    let started = Instant::now();
    let manifest = SourceManifest::measure(&source, ManifestLimits::default())?;
    let members: Vec<ImportMember> = manifest
        .candidate_members(provider)
        .into_iter()
        .filter_map(|entry| {
            MemberKind::for_member(provider, &entry.path).map(|kind| ImportMember {
                path: entry.path.clone(),
                kind,
            })
        })
        .collect();
    println!(
        "source_kind={} files={} bytes={} measure_ms={}",
        manifest.source_kind(),
        manifest.total_files(),
        manifest.total_bytes(),
        started.elapsed().as_millis()
    );
    println!("candidate_members={}", members.len());
    for member in &members {
        println!("  member {:?} kind={:?}", member.path, member.kind);
    }
    if members.is_empty() {
        println!("RESULT=BLOCKED no candidate member found (record the export layout, folder names only)");
        return Ok(());
    }

    let lease = gate.require_unlocked()?;
    let mut receipts = Vec::new();
    for run in 1..=2 {
        let cap = gate.authorize_import_session(
            lease,
            &secret,
            provider,
            &source,
            *manifest.sha256(),
            Duration::from_secs(3600),
        )?;
        let mut request = SourceImportRequest::new(&gate, lease, &cap, &members);
        request.progress = &NoProgress;
        request.limits = ImportJobLimits::default();
        let started = Instant::now();
        let receipt = store.import_authorized_source(&request)?;
        let _ = gate.revoke_import_job(cap.job_id());
        println!(
            "run={run} already_complete={} ms={} scanned={} imported={} deduplicated={} raw_only={} rejected={} raw_blobs_new={} raw_bytes_preserved={}",
            receipt.already_complete,
            started.elapsed().as_millis(),
            receipt.records_scanned,
            receipt.imported,
            receipt.deduplicated,
            receipt.raw_only,
            receipt.rejected,
            receipt.raw_blobs_new,
            receipt.raw_bytes_preserved
        );
        receipts.push(receipt);
    }
    let first = &receipts[0];
    for member in &first.members {
        println!(
            "  member_receipt {:?} bytes={} records={} unpreserved_bytes={}",
            member.path, member.bytes, member.records, member.unpreserved_bytes
        );
    }
    let mut reasons: BTreeMap<String, u64> = BTreeMap::new();
    for item in &first.exceptions {
        *reasons
            .entry(category(item.reason.as_deref().unwrap_or("(none)")))
            .or_default() += 1;
    }
    println!(
        "raw_only_or_rejected_reasons (first {} listed, {} omitted):",
        first.exceptions.len(),
        first.exceptions_omitted
    );
    for (reason, count) in &reasons {
        println!("  {count:>6}  {reason}");
    }

    // Independent checks on the disposable archive.
    let conversations = store.list("")?;
    let mut completeness: BTreeMap<String, u64> = BTreeMap::new();
    let mut warnings: BTreeMap<String, u64> = BTreeMap::new();
    for summary in &conversations {
        *completeness
            .entry(summary.completeness.clone())
            .or_default() += 1;
        for warning in &summary.warnings {
            *warnings.entry(category(warning)).or_default() += 1;
        }
    }
    println!(
        "archive_conversations={} completeness={completeness:?}",
        conversations.len()
    );
    println!("warning_categories:");
    for (warning, count) in &warnings {
        println!("  {count:>6}  {warning}");
    }
    let (blobs, corrupt) = verify_raw_blobs(&scratch.join("archive-v1/imports/raw"))?;
    println!("raw_blobs={blobs} raw_blobs_hash_mismatch={corrupt}");
    if provider == "gemini" {
        // Masked shapes of card-time lines that were not read, computed here from this run's own
        // raw blobs (never stored). Only time-like lines are shaped (see `masked_line_shape`), and
        // a shape seen fewer than MIN_SHAPE_COUNT times is folded into `other`: a real format
        // problem repeats across cards, a one-off line could be private.
        const MIN_SHAPE_COUNT: u64 = 3;
        let shapes = unread_time_line_shapes(&scratch.join("archive-v1/imports/raw"))?;
        let mut other = 0u64;
        println!("gemini_unread_time_line_shapes (distinct cards):");
        for (shape, count) in &shapes {
            if *count >= MIN_SHAPE_COUNT {
                println!("  {count:>6}  {shape}");
            } else {
                other += count;
            }
        }
        if other > 0 {
            println!("  {other:>6}  other (shapes seen fewer than {MIN_SHAPE_COUNT} times)");
        }
    }

    let pass = corrupt == 0
        && receipts[1].already_complete
        && first.records_scanned
            == first.imported + first.deduplicated + first.raw_only + first.rejected
        && conversations.len() as u64 == first.imported;
    println!(
        "RESULT={}",
        if pass {
            "PASS (harness checks only; review the counts)"
        } else {
            "FAIL"
        }
    );
    Ok(())
}

/// The scratch directory must be new or empty, and must not be (inside) the real archive or the
/// export itself.
fn refuse_unless_disposable(scratch: &Path, source: &Path) -> Result<(), BoxError> {
    if scratch.exists() && std::fs::read_dir(scratch)?.next().is_some() {
        return Err("the disposable directory must be new or empty".into());
    }
    // Canonical on every path (a bare relative name's parent is the current directory), so the
    // `starts_with` checks below compare like with like (`\\?\` prefixes on Windows).
    let parent = match scratch.parent().filter(|p| !p.as_os_str().is_empty()) {
        Some(parent) => std::fs::canonicalize(parent)?,
        None => std::fs::canonicalize(std::env::current_dir()?)?,
    };
    if parent.starts_with(source) {
        return Err("the disposable directory must not be inside the export".into());
    }
    if let Ok(real) = mind_place_archive::data_root() {
        if let Some(app) = real.parent().and_then(|p| std::fs::canonicalize(p).ok()) {
            if parent.starts_with(&app) {
                return Err("refusing to write inside the real Mind Place data directory".into());
            }
        }
    }
    Ok(())
}

/// Tallies the unread card-time line shapes of every Gemini card blob in this run's disposable
/// archive (each raw blob is the exact bytes of one card).
fn unread_time_line_shapes(root: &Path) -> Result<BTreeMap<String, u64>, BoxError> {
    let mut tally = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries {
            let path = entry?.path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            let card = std::fs::read(&path)?;
            if let Some(shape) = GeminiTakeoutParser::unread_time_line_shape(&card) {
                *tally.entry(shape).or_default() += 1;
            }
        }
    }
    Ok(tally)
}

/// Every raw blob's content hashes to its own name.
fn verify_raw_blobs(root: &Path) -> Result<(u64, u64), BoxError> {
    let (mut blobs, mut corrupt) = (0, 0);
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries {
            let path = entry?.path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            blobs += 1;
            if format!("{:x}", Sha256::digest(std::fs::read(&path)?)) != name {
                corrupt += 1;
            }
        }
    }
    Ok((blobs, corrupt))
}

/// A content-free category: everything from the first quote character on is dropped (a quoted
/// part may echo a provider value), and digits are masked. Reasons and warnings are fixed texts
/// written by the importers; only their fixed prefix is printed.
fn category(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars().take(160) {
        match c {
            '\'' | '"' | '`' => {
                out.push('…');
                break;
            }
            c if c.is_ascii_digit() => {
                if !out.ends_with('#') {
                    out.push('#');
                }
            }
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn categories_never_print_quoted_text() {
        assert_eq!(
            super::category("Sender 'it's private text' is not a known role"),
            "Sender …"
        );
        assert_eq!(super::category("Content \"secret\" here"), "Content …");
        assert_eq!(
            super::category("More than 100 files on one message"),
            "More than # files on one message"
        );
    }
}
