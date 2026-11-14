# MP-IMPORT-APP-01 — design note: zip sources and Tauri import commands

Status: design for the claimed card. Synthetic fixtures only. Not an acceptance claim.

## Owner decisions (2026-11-14, in chat)

1. Build against synthetic fixtures and the documented layout. Real-export layout verification stays **NOT_RUN** and moves to MP-IMPORT-L1 (residual risk below).
2. Extend the RAW-01 source manifest and the security-core import authorization so a **single zip file** is a source. Members are streamed from the zip; nothing is extracted to disk.
3. Entry point option **A**: backend Tauri commands only. The Import button arrives with MP-UI2-06.

## 1. Zip source in the manifest (archive-core `historical`)

`SourceManifest::measure(path, limits)` now accepts a folder (unchanged) or a regular file that is a zip archive. A zip source is measured by:

1. no-follow open (the same checks as folder members), metadata kept (length, modification time);
2. one sequential pass computing the SHA-256 of the whole archive;
3. a strict parse of the end-of-central-directory record (zip64 supported, single disk only) and of every central directory entry;
4. for every file entry: validating its local header against the central directory, decompressing it (stored or deflate) and hashing the decompressed bytes.

`ManifestEntry { path, size, sha256 }` keeps its meaning: the member name, its decompressed size and the SHA-256 of its decompressed content. Raw-evidence offsets are offsets into the decompressed member, exactly as for a folder file. The manifest hash for a zip uses a separate domain (`mind-place/import-source-manifest/zip/v1`) and also binds the archive length and archive SHA-256, so any byte change (including bytes outside members) changes the pinned hash.

Staging opens a member with `SourceManifest::open_member`: for a zip it reopens the archive no-follow, requires the measured length, re-validates the local header, and streams `Take(compressed) → inflate → exact-length + CRC-32 check`. The job's existing `HashingReader` then requires the measured decompressed SHA-256, so a zip swapped after measurement is refused (`E_SOURCE_MISMATCH`). `verify_layout_unchanged` for a zip re-parses the central directory and requires the same names and sizes, plus the same archive length and modification time.

### Zip safety rules (all typed errors, nothing skipped silently)

| Threat | Rule | Code |
| --- | --- | --- |
| Path traversal / odd names | member names must be UTF-8, `/`-separated, no leading `/`, no `\`, no drive prefix, no `.`/`..`/empty components, no control characters, at most `max_path_bytes`. Names are identifiers only; nothing is written to disk under them | `E_SOURCE_NAME` |
| Duplicate names | an exact duplicate name is refused (which entry is "the" member would be ambiguous) | `E_ZIP_DUPLICATE` |
| Zip bombs (ratio) | a member larger than 1 MiB may not exceed `max_ratio` (default 200) decompressed bytes per compressed byte | `E_ZIP_BOMB` |
| Zip bombs (overlap) | local entries may not overlap each other or the central directory (the overlapping-file bomb) | `E_ZIP_BOMB` |
| Unaccounted bytes (prefix, gaps, polyglots) | the members must **tile the file**: the first local header is at byte 0, each next one starts exactly where the previous member (plus its data descriptor) ends, and the last one ends exactly where the central directory begins. A prefix with rebased offsets, bytes between members and bytes before the directory are all refused, so every byte is a measured member or a parsed record. Checked on the offsets *after* sorting, so a consistently rebased archive is caught as well as a broken one | `E_ZIP_MALFORMED` |
| Data descriptors (policy) | allowed only when the member's local header sets bit 3, and then exactly one descriptor must follow the data, whose CRC-32 and sizes (32 or 64 bit, signature optional) equal the central directory's. A declared descriptor that is absent or disagrees is corrupt; descriptor-shaped bytes without the bit, or extra bytes after the descriptor, are unaccounted bytes | `E_ZIP_CORRUPT`, `E_ZIP_MALFORMED` |
| Zip bombs (total) | the sum of declared sizes, and the bytes actually inflated, are bounded by `max_total_bytes`; entry count by `max_entries` | `E_SOURCE_LIMIT` |
| Lying headers | inflated length must equal the declared size exactly, CRC-32 must match, local header must match the central directory | `E_ZIP_CORRUPT` |
| Unsupported features | encryption, compression other than stored/deflate, multi-disk/spanned archives, a zip64 record that is missing or inconsistent, data after the end record (data before the archive is covered by the tiling rule above) | `E_ZIP_ENCRYPTED`, `E_ZIP_METHOD`, `E_ZIP_MULTIDISK`, `E_ZIP_MALFORMED` |

Directory entries count toward `max_entries` but are not manifest entries. The reader is written in-tree on top of `flate2` (already a pinned workspace dependency through document-core), so every rule above is explicit and testable with hand-crafted hostile archives; no new crate is added.

## 2. Authorization (security-core)

`authorize_import_session` accepts a source that is either a safe directory (unchanged) or a regular file (`validate_import_source`): absolute path, no link or reparse point in any existing ancestor, the final component a regular file. Everything else in the capability (canonical path binding, manifest hash, lease epoch, TTL, revoke, lock fence) is unchanged; the canonical path of a file binds the same way as a folder's.

## 3. Member selection (documented layout, unverified)

`SourceManifest::candidate_members(provider)`:

- `chatgpt`: every entry whose last path component is `conversations.json` or matches `conversations-*.json` (shards), at any depth.
- `gemini`: every `.html` entry whose path contains a component with `gemini` (case-insensitive), at any depth (the documented Takeout path is `Takeout/My Activity/Gemini Apps/MyActivity.html`; localized folder names are expected to keep the product name).

The start command takes an optional explicit member list; each one must be a manifest entry with an extension that matches the provider (`.json` for ChatGPT, `.html` for Gemini). Real layouts (localized Takeout names, sharding) are **NOT_RUN** until MP-IMPORT-L1.

## 4. Tauri commands (`src-tauri/src/import_commands.rs`)

All three need an unlocked owner session (`begin_unlocked`) and the legacy plaintext archive (the RAW-01 path refuses an encrypted vault; encrypted vault import is a later card). None is reachable from MCP or the extension.

**The webview never names a path.** A source is named by a *selection grant*: an opaque 128-bit token the backend mints (`grant_selection`, not a command) for a path the owner picked in a native dialog. Grants live in memory only, expire after 10 minutes, are capped at 4 (oldest dropped), are forgotten on Lock or window close, and are not consumed by use (inspect, then import the same selection). The picker command that mints them is MP-UI2-06's; **until it exists no command can mint a grant and `inspect_import_source` / `start_import` refuse every request with `E_IMPORT_SELECTION`** (fail closed). The path check (absolute, no UNC/device namespace) still runs when a grant is minted and again when a command uses it.

| Command | Does | Returns |
| --- | --- | --- |
| `inspect_import_source { provider, selection }` | resolves the grant, takes the import slot, measures the source (read-only) | kind, file count, bytes, candidate members, and an opaque `sourceToken` (the manifest hash) |
| `start_import { provider, selection, sourceToken, credential, members? }` | re-authenticates the owner, re-measures, requires the token to equal the new measurement (the owner authorizes what was inspected), authorizes a capability bound to that hash, runs `import_authorized_source` with progress events and cancel, then revokes the capability | the job receipt (counts, raw-only and rejected reasons, `unpreserved_bytes` per member) |
| `cancel_import` | sets the cancel flag of the running import | whether one was running |

- A caller-supplied token can only cause a refusal: the import still measures the source itself and the gate compares against its own measurement.
- The credential arrives as IPC payload (like unlock), is wrapped in `Zeroizing` immediately, and is never logged or put in a URL. Wrong credentials use the gate's existing backoff.
- Progress: `import-progress` events with `{ phase, done, total }` at the staging, verified, per-item and completion points (throttled). Events carry counts only, never paths or content.
- Measuring is cancellable: hashing and decompressing the source polls Cancel and the lease between entries and about once per MiB (`SourceManifest::measure_interruptible`, `ImportProgress::measuring` for the job's own start-of-run measurement). Lock gives `E_AUTHORIZATION`, Cancel gives `E_IMPORT_CANCELLED`, nothing is written. Parsing a zip's central directory is not polled; it is bounded by `max_entries`.
- Cancel and Lock: cancel is honoured at the next staging record or before the next item. While staging, the spool is discarded; after staging, the verified spool is kept and `start_import` on the same source resumes it. Lock revokes the lease, so the next item fails `E_AUTHORIZATION` (RAW-01 fence); the command also stops at its next checkpoint.
- One import **or inspection** at a time per process (`E_IMPORT_BUSY`), on top of the archive's job lock: inspecting claims the same slot, so `cancel_import` and Lock reach it and it never overlaps an import.

## 5. Residual risks

1. Real export layouts are unverified (owner decision 1). Member selection may miss localized or sharded members; the explicit member list is the escape hatch.
2. A zip is read three times (inspect, job-start measurement, staging), the same as a folder. Throughput on multi-gigabyte exports is not measured on the owner's machine.
3. Windows behaviour (sharing violations from antivirus on the zip, very long paths) is NOT_RUN until MP-IMPORT-L1.
4. Images and other assets inside the export are measured (hashed) but not ingested, as in RAW-01.
5. **MP-UI2-06 depends on the grant.** It must mint selection grants from a native picker in the backend and must not add a command that turns a renderer-supplied path into a grant. This is a tracked security dependency, not a convenience.
6. Strict tiling refuses archives with an archive-extra-data record, a digital signature record or other padding between members. Mainstream writers (Info-ZIP, Python, Java, macOS Archive Utility with descriptors) tile; a real export that does not would be refused with `E_ZIP_MALFORMED` and recorded by MP-IMPORT-L1.
