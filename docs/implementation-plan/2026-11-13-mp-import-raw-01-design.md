# MP-IMPORT-RAW-01 — design note: atomic raw-evidence and projection publication

Status: **DESIGN, for independent review before implementation is accepted.** Not acceptance, not a merge request. Only the repository owner merges.

- Card: Notion "MP-IMPORT-RAW-01 · Atomic raw-evidence and projection publication with independent source manifest".
- Predecessors: MP-IMPORT-FIX-01 (PR #166) and MP-IMPORT-SANITIZE-01 (PR #167), both merged into `main`.
- Exclusive logical resources: `archive-core historical write path`, `security-core import authorization`.
- Scope: plaintext profile only. Out of scope: zip adapter, Tauri commands, UI, encrypted vault import, MCP write authority, asset (image) ingestion.

## 1. Problem

After MP-IMPORT-FIX-01 the parsers return `ReconciledItem`s (`Conversation { capture, raw_evidence }` or `PreservedRawOnly`), but the only authorized write API (`save_authorized_historical_import`) accepts a `Capture` and nothing else:

1. **P1-3.** The raw bytes (full ChatGPT DAG with alternate branches, raw-only Gemini cards, rejected projections) are never stored. The repository invariant "preserve immutable raw revisions before normalization" would be violated by importing a real export.
2. **P1-2.** `ImportSourceInventory.manifest_sha256` is supplied by the caller and is then passed back to the gate as the "independently computed" hash, so the gate only compares a value with itself. Nothing measures the source, so a file can be replaced between authorization and publication, and no stored record is tied to any source byte.

## 2. Design summary

One new authorized entry point on `ArchiveStore` (`import_authorized_source`) that:

1. **Measures** the source folder itself into a `SourceManifest` (every regular file: relative path, size, SHA-256) and derives `manifest_sha256` from a domain-separated, length-framed encoding. Callers cannot construct a manifest or supply its hash. The same function is what the owner flow calls **before** `authorize_import_session`, so the capability pins an honestly measured value.
2. **Re-measures at job start** and refuses to proceed unless the result equals the hash pinned in the capability (`E_SOURCE_CHANGED`).
3. **Binds every record to verified bytes**: each declared member file is read once through a hashing reader; every item records `(member, offset, length, sha256)` computed from the bytes actually extracted, and the member's whole-file SHA-256 must equal its manifest entry before anything becomes durable-visible. The bytes that are parsed are the bytes that are hashed, so there is no read/verify gap for members.
4. **Stages** items (raw bytes + projected `Capture` + disposition) in a private job directory, then writes a **verified marker** carrying a root hash over all staged items.
5. **Re-measures the whole source again** (after staging, before publication) and requires the same pinned hash. Only then does it publish.
6. **Publishes item by item** under the archive lock and a fresh gate verification per item, in a fixed order: raw blob, then evidence record, then projection (`meta.json` last, the existing visibility point). Every step is idempotent and content-addressed, so a crash at any point is repaired by running the same import again.
7. Writes a **per-job receipt** (imported / deduplicated / raw-only with reasons / rejected with reasons, plus counts of raw blobs and evidence records written or reused) that is checked against the disk in tests.

## 3. Storage layout (under the archive root, plaintext profile)

```
imports/
  raw/<aa>/<sha256>.raw            immutable, content-addressed raw payloads (aa = first two hex chars)
  evidence/<aa>/<evidence_id>.json immutable provenance + disposition records
  jobs/<job_key>/
    .job.lock                      exclusive per-job lock
    state.json                     phase (staging | verified | complete), counts, root hash, published_through
    manifest.json                  the pinned manifest (entries, hash)
    items/<seq:08>.json            staged item (envelope, span, disposition, validated Capture)
    items/<seq:08>.raw             staged raw payload
    receipt.json                   final receipt
conversations/<id>/...             existing projection layout, unchanged
```

All file names are derived from hashes or integers, never from source-controlled strings. `job_key = sha256(domain || provider || manifest_sha256 || canonical member list)`. The gate `job_id` (random per authorization) is **recorded** in state, evidence and receipt but is not the directory key, so a restart can resume the same job under a new authorization.

`imports/generic/` (the pre-existing generic importer) is untouched.

### Evidence record

`evidence_id = sha256(domain || manifest_sha256 || member path || offset || length || payload_sha256)`. The record states provider, envelope domain, source identifier, payload hash and length, the span (`member`, `member_sha256`, `offset`, `length`, `record_index`), the raw `metadata_json` as an opaque string, the disposition (`projected` with `archive_id` and `snapshot_id`, `raw-only` or `rejected` with a reason) and the recording job. It is immutable (`persist_noclobber`); an existing file is compared byte for byte and a difference is `E_EVIDENCE_CONFLICT`. The same bytes imported from a different export give a new evidence record that points at the **same** raw blob (byte dedup, distinct provenance).

## 4. Publication protocol and failure model

Per item, in this order, inside `archive lock → gate.with_verified_import_capability_bound(...)`:

1. **Raw blob.** Absent: move the verified staged file into `imports/raw/..` (same volume, archive lock held). Present: hash it and require equality; delete the staged copy. Present but different: `E_RAW_CORRUPT`, nothing is replaced, ever.
2. **Evidence record** (immutable; the snapshot id is computed before the projection from the same identity function the store uses, so the link is durable before the projection exists).
3. **Projection**, only for `Conversation` items that validate at this boundary: the existing `save_under_lock` (snapshot, markdown, search text, `meta.json` last). If the returned snapshot id differs from the precomputed one, fail (`E_SNAPSHOT_MISMATCH`). A capture that fails `validate_historical_capture`, carries an inline `data_url`, or is too large is **rejected**: raw and evidence are still preserved, the reason is recorded.

Invariants:

- A projection is never visible without a durable raw blob and an evidence record for it (raw and evidence are written first; `meta.json` is the single visibility point).
- Raw-only and rejected items are complete outcomes: raw plus evidence, no projection.
- Crash windows: after raw (blob present, no evidence): re-run finds the blob, verifies, writes evidence. After evidence: re-run reuses it. During projection: the existing writer converges because snapshot files are content-addressed and derived files are rewritten. Between items: `published_through` is checkpointed in `state.json` every 64 items and at the end; re-running redoes at most 64 idempotent items.
- A lock, revoke, expiry or foreign gate between items fails the next gate verification: publication stops with `E_AUTHORIZATION` (a content-free message), state stays `verified` with `published_through` recorded, and **nothing partial is described as complete**. Re-authorizing and re-running resumes.
- Staging failures (bad source, hash mismatch, parse error, limits) delete the job directory: nothing was visible and the source is still there to redo.
- Publication failures keep the job directory (verified spool) so the job can resume; `discard_staged_job(job_key)` removes it explicitly.
- A completed job re-run (same manifest) returns the stored receipt (`already_complete = true`) after the source has been re-measured; nothing is written.
- The staged root hash (sha256 over the ordered hashes of the item files, plus count) is verified **before** the first publication step; staged raw files are hash-checked when consumed. Spool tampering fails closed (`E_STAGING_CORRUPT`) and the owner re-stages from the source.

## 5. Independent source manifest

- **Measure:** walk the canonical source root. Reject symlinks, reparse points, devices, non-UTF-8 names, `..`/empty components, depth over 32, paths over 512 bytes, more than 100,000 files, or more than a configurable total (default 64 GiB) as typed errors. Per file: `lstat` must be a regular file; open without following links (Windows `FILE_FLAG_OPEN_REPARSE_POINT` plus an attribute check on the handle; Unix: handle identity equals `lstat` identity); hash in 64 KiB chunks; the bytes read must equal the size and the handle metadata (size, mtime) must be unchanged after hashing, otherwise `E_SOURCE_CHANGING`.
- **Hash:** `sha256("mind-place/import-source-manifest/v1\0" || u64be(count) || for each entry sorted by path bytes: u32be(len(path)) || path || u64be(size) || sha256)`.
- **Pin:** `authorize_import_session(.., manifest.sha256(), ..)` stays the gate's API; the manifest value now comes from `SourceManifest::measure`, whose fields are private. The import function takes **no** inventory or hash from the caller: it uses the capability's pinned root and hash and compares them with its own measurement.
- **Members:** the caller lists `(relative path, kind)`; every path must be an entry of the measured manifest and kind must match the authorized provider (`chatgpt` ↔ conversations JSON, `gemini` ↔ Takeout HTML). Choosing members from a real export layout is MP-IMPORT-APP-01's job; the layout is unverified.
- **Binding:** ChatGPT records come from `stream_bounded_top_level_array` extended (additively) to report each record's byte offset, length and SHA-256; Gemini cards report the slice they were cut from. The raw payload must equal that slice by length and hash. Whole-member hash must equal the manifest entry, otherwise `E_SOURCE_MISMATCH`.

## 6. Threat model (what is defended, what is not)

| # | Threat | Defense | Residual |
| --- | --- | --- | --- |
| T1 | Source replaced between measurement/authorization and import | Job-start re-measure vs the gate-pinned hash; member hash checked while parsing; whole-tree re-measure before publication | Same-size, same-content swaps are not changes. An attacker who can also forge the gate-pinned value is out of model (the gate is in-process) |
| T2 | TOCTOU during read | Hash the stream that is parsed; publish only spooled, verified data, never re-read the source | None for members |
| T3 | Symlink / junction / reparse substitution or traversal | Reject at walk, open with no-follow, check handle; names never used to build output paths | Intermediate-directory swaps are not prevented, but content hash equality bounds what can be read |
| T4 | Caller forges a manifest or hash | Manifest type is not constructible; import takes no hash | None |
| T5 | Spool tampering | Root hash before publication; raw hash on consumption | A local attacker with archive write access can already alter the archive |
| T6 | Crash or power loss | Atomic writes, content addressing, idempotent order, journaled progress | Filesystems that lie about fsync |
| T7 | Lock/revoke/expiry mid-job | Per-item gate verification inside the archive lock | An item in flight completes (bounded by one item) |
| T8 | Resource exhaustion (huge/many files, huge records, deep paths, endless streams) | Manifest limits, per-record 64 MiB, ID table and item caps, Gemini 64 MiB, staging bytes cap | Disk use is about one copy of the member bytes while staging |
| T9 | Raw replacement or silent loss | Noclobber writes, hash compare on reuse, `E_RAW_CORRUPT`; no delete path for raw | Offline disk corruption is detected on reuse, not repaired |
| T10 | Encrypted vault profile | `ensure_not_encrypted_vault` before any write and inside `lock()`; the import never opens a vault | Encrypted import is a later card |
| T11 | Content or path leakage in errors/logs | Typed codes, member index not path, no content | |

## 7. What is not claimed

No claim of lossless import for real exports (none was read; real exports remain `NOT_RUN`). Claims are limited to what the tests prove for synthetic fixtures: raw payload bytes equal the source slice, the receipt equals the disk, and the failure and recovery paths above. Parse errors still abort the job (an unparseable ChatGPT record is not downgraded to raw-only here; recorded as a follow-up). Assets (`DiscoveredAsset`) are refused when non-empty; the parsers never produce them today. Encrypted vault, zip, UI and Tauri wiring are not included. Windows-native behaviour (true reparse points, NTFS locking) is `NOT_RUN` until the Windows qualification.

## 8. Test plan (maps to the card)

Source mutated, replaced, truncated or extended after authorization; swap between stages (fault hook); symlink/junction members; crash after raw, after evidence, after projection, between checkpoints; re-import idempotent and a re-import from a different export (raw reuse, new evidence); duplicate content in one job; lock and revoke mid-job; oversized, deep and many-file abuse with shrunken limits; receipt equals disk counts; raw blob equals the independently re-read source slice; no raw replacement on corrupt existing blob; vault marker refuses before any write; foreign gate; wrong provider/kind; ledger test updated for the new entry point.

## 9. Design review round 1 (independent AI reviewer, advisory) — decisions

The review found no P0 and eight P1 items. **Where this section differs from sections 2 to 6, this section wins.**

| Finding | Decision |
| --- | --- |
| D-F01 evidence not replay-safe, could claim a missing projection | The evidence record is **provenance only**: provider, domain, source identifier, payload hash/length, span, member hash, manifest hash, opaque `metadata_json`. No job id, no disposition, no time. Identical bytes on every replay; a difference is `E_EVIDENCE_CONFLICT`. The disposition (projected with snapshot, raw-only, rejected, with reason) goes into a separate replaceable link record `imports/dispositions/<aa>/<evidence_id>.json` written **after** the projection, and into the per-item done marker. Order per item: raw, evidence, projection, disposition link, done marker |
| D-F02 projection not idempotent / can displace a live capture | Items with a done marker are never replayed. Only the one in-flight item can replay. For historical imports a **live** (manual/automatic DOM) latest capture is never displaced: the existing keep-prior rule is extended from `legacy-import` to `export-import` and `takeout-import`. A newer import of the same conversation still becomes latest. Ordering of two exports is the owner's choice and is documented |
| D-F03 moving the staged raw file breaks resume | Staged raw is **copied** (fsynced temp in the raw directory, then noclobber persist) and deleted only after the done marker. The staged root hash covers the item JSON only (which carries the raw hash and length); a missing staged raw is fine when the final blob exists and hashes equal |
| D-F04 durability on Windows | Data files are fsynced; directory fsync exists only on Unix. Crash durability on Windows is **not claimed** and is `NOT_RUN` |
| D-F05 gate mutex held across heavy I/O | All hashing, spool reads, existing-blob verification, evidence/link serialization and raw temp-file writes happen before the gate action (under the archive lock). Inside the action only: noclobber persist of the prepared raw temp file, the small evidence write and the projection write (the same I/O the existing authorized entry already does inside the gate). The action returns a nested `Result` so storage errors are not reported as authorization failures |
| D-F06 missing item bounds | `max_items` (250,000) is enforced in staging for every item including raw-only; the Gemini parser takes a card cap before parsing cards |
| D-F07 old public `save_authorized_historical_import` | **Not changed in this card** (rewriting its callers is a separate decision). It is documented as a capture-only primitive without raw evidence; MP-IMPORT-APP-01 must use `import_authorized_source`. Residual risk, reported to the owner |
| D-F08 path-based walk | Defense in depth only (lstat, no-follow open, handle checks, Unix identity). The earlier statement that hashing bounds reads holds **after** a manifest is pinned; the first measurement pins whatever it read. Not defended and documented: swapped intermediate directories, FIFO open before the handle check on Unix, hard links and NTFS alternate streams. Requires write access to the source folder |
| D-F09 crash during staging | State `staging` found at start: wipe the spool and restage. A resume of a `verified` job that finds the source changed keeps the spool and published state and fails with `E_SOURCE_CHANGED` |
| D-F10 vault fence and ancestors | Marker check and `no_link` on `imports`, `imports/jobs` and the job directory at job-lock time and before every item (via `lock()`); the spool `items/` directory is deleted when the job completes |
| D-F11 receipt vs disk after a crash | The receipt is derived from per-item done markers (final dispositions). After a resumed run the single in-flight item may be reported `deduplicated` instead of `imported`; receipts carry `resumed` so counts are never over-claimed |
| D-F12 capability TTL | The post-staging check is a cheap file-set and size check (no third full hash pass); member bytes are already hash-bound while parsing. Resume under a new authorization is the normal path for very large exports |
| D-F13 gate comparison | The hash passed to the gate at publication is the one measured at job start (verified equal to the pinned one), not the capability's own |
| D-F14 job key | Includes an importer version and the stream limits |
| D-F15 one bad record aborts the job | A ChatGPT record that cannot be deserialized becomes raw-only with a generic reason (no content echoed). Tracker overflow and Gemini invalid UTF-8 still abort |
| D-F16 duplicate-ID tracking across shards/case | One tracker per job, keyed by lowercased id, members processed in sorted path order |
| D-F17 snapshot identity | One shared `snapshot_identity` function, computed after validation at publication time; `data_url` and `asset_path` attachments are **rejected** (raw preserved) |
| D-F18 coverage | Each member receipt records record bytes preserved versus member bytes, so framing and non-record bytes are visible; no lossless claim |
| D-F19 job lock | One `fs2` lock file `imports/.jobs.lock` outside any deletable directory; `discard_staged_job` takes it |
| P3 | Staged items carry a bytes-free envelope; processing time is frozen in the spool and never enters evidence; receipts carry codes and generic reasons only; the staged hash detects corruption, it is not a MAC; unrelated files added to the folder change the manifest and therefore evidence ids (stated) |
