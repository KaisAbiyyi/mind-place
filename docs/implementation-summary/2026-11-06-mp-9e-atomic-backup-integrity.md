# MP-9E — Atomic portable backup / restore integrity hardening

**Task:** MP-9E · [#149](https://github.com/KaisAbiyyi/mind-place/issues/149). **Execution:** ChatGPT GPT-6 Cloud, exclusive `backup` lock. **Original source baseline:** `main` at `24648c74f3f59e176974d3878049b882478db69b`; **reconciled with main** `428cb2df6c931024ff13fe58494f33e2640a787e` without rewriting prior MP-9E commits.

## Architecture and boundaries

The existing MP-9B design is the source of truth: `VaultOnlineBackup::create` takes the exclusive `VaultWriterFence`, makes SQLCipher Online Backup snapshots (including committed WAL pages) and copies encrypted immutable objects to an isolated scratch vault. It checks schema, FTS, foreign-key, source identity and object/catalog coherence, then calls `PortableBackup::create` to create an authenticated generation manifest and publish a **new** bundle directory. `VaultOnlineBackup::restore` authenticates the bundle with the independent recovery secret, verifies encrypted DB health and source/catalog coherence in a separate staging generation, then non-replacing-renames it into an owner-chosen new vault path.

This task **does not** make an incomplete raw SQLite/file copy safe or authorize additional roots. External approved source roots are excluded and require fresh authorization. No owner archive, key, recovery secret, backup location, retention policy or real device is touched. No background backup/cleanup or silent overwrite is implemented. The caller still must use `VaultOnlineBackup` (not `PortableBackup` alone) for full application data validation.

## MP-9E changes

1. Reject a backup when package-level files or payload files differ from the authenticated manifest, including hidden/unlisted ciphertext entries, malformed or symlinked entries and unknown root metadata. The check runs before restore staging is created and also before a new backup bundle is published. All declared payload hashes continue to be checked.
2. Sync nested staging directories bottom-up on Unix after every file flush, before atomic nonreplacing directory publication; file handles are closed first. On Windows, file flushes plus the existing MoveFileExW WRITE_THROUGH publication remain the durability mechanism. This is not proof of arbitrary Windows power-loss atomicity on physical storage.
3. Preserve partial staging and existing destinations on failure for explicit owner disposition, never delete or adopt them automatically. Existing authenticated v1 backup package format stays unchanged.
4. Add synthetic tests for unmanifested payload and root data, ambiguous pre-existing partial/destination, corrupted authenticated manifest, stale owner unlock and package payload symlink.

## Security limitations and gates

- **Rollback/freshness:** Authenticated `generation` does not establish an independent monotonic trusted counter. Restore of an older valid backup requires an explicit owner decision upstream.
- **Writer coordination:** The cross-process fence only covers participating native application writers. The owner-installed native Windows runtime/writers inventory and unexpected external mutation are separate gates.
- **Path races:** Symlink/reparse-point checks and exclusive files are not a universal defense against hostile same-user processes racing directory handles. The threat model excludes compromise of an already-unlocked Windows session.
- **Native durability:** Hosted Windows tests cannot certify power-loss ordering, BitLocker/DPAPI behavior on the owner's machine, real credential recovery or hardware crash replay.
- **Acceptance:** Exact-head Rust format and security tests, hosted Windows/Ubuntu product + SQLCipher qualification, independent source review, owner-specific recovery/backup acceptance where applicable. Do not claim DONE/PASS until receipts exist; owner-only merge.

## Focused regression commands (for GitHub-hosted or other cloud runner)

```sh
cargo fmt --all -- --check
cargo test -p mind-place-security vault_backup --lib
cargo test -p mind-place-index --features sqlcipher-preview --test vault_online_backup
```

These are suggested verification commands, not claims that the cloud connector executed them.


## Reclaim implementation — 2026-11-07 (existing PR #152)

### Bounded inventory correction

The recursive inventory now shares one explicit budget across the package-root and payload walks. Every encountered directory entry increments the total-entry counter before path/type processing; directories also increment their own counter and are checked against the preserved depth limit.

| Budget | Production limit | Purpose |
|---|---:|---|
| Payload regular files | 100,000 | Preserves the existing \`MAX_FILES\` limit |
| Total entries | 200,000 | Bounds files plus directories and package metadata |
| Directories | 100,000 | Prevents empty-directory fan-out from bypassing file limits |
| Payload directory depth | 5 | Preserves the existing depth boundary |

The total-entry and directory limits bound worst-case enumeration independently of file count. Tests inject small limits; they do not create production-scale directory trees. The same bounded accounting is applied to the online encrypted-object copier (200,000 total entries, 100,000 directories, 100,000 files, depth 12). Symlink/reparse-point and special-file rejection remains fail-closed.

Backup publication revalidates the package inventory after directory sync. Restore performs bounded preflight before creating staging, rechecks the source package after payload copy, compares the restored file inventory with the authenticated manifest, and only then syncs and publishes the new directory.

### Interruption and recovery evidence added

Deterministic checkpoints cover package staging creation, payload-copy interruption, partial encrypted-manifest publication, restore-copy interruption, and a destination collision introduced immediately before final backup publication. Regression cases also cover existing destination collisions, restart against preserved partial backup/restore staging, empty-directory fan-out, directory-count overflow, depth overflow, mixed file/directory total-entry overflow, exactly-at-limit acceptance, symlink and special-file rejection, corrupted payload digest, corrupted encrypted manifest, stale unlock lease, and valid published-backup recovery.

Failed online encrypted scratch snapshots are detached from automatic temporary-directory cleanup immediately. Failed or interrupted scratch/staging paths remain for explicit owner disposition; successful online backup removes only its temporary duplicate after the portable package has been published.

### Full application integration scope

Synthetic SQLCipher integration coverage now exercises the real encrypted state, archive catalog, archive FTS/index, document-index database, immutable archive objects, vault manifest metadata, and online create/restore flow. The Windows-only approved-document-root integration additionally verifies a real encrypted document object, document FTS search, and preview after restore. No fake encrypted-looking fixture substitutes for these SQLCipher integration tests.

### Qualification status and boundaries

- Exact-head hosted CI: record the run ID and executed job/test counts in the PR and this page after the branch update.
- \`mp9b-sqlcipher-qualification.yml\` is skipped for \`pull_request\` events while the PR is Draft. Its \`workflow_dispatch\` path checks out the PR merge ref and can qualify a Draft PR without changing its Draft state. The dispatch was not available through the connected GitHub actions in this execution; SQLCipher integration execution therefore remains **NOT RUN** until an owner dispatches it.
- Native installed-Windows backup/recovery acceptance, owner-vault recovery, and physical power-loss durability remain **NOT RUN**. Hosted Windows jobs are not native owner acceptance.
- PR #152 remains Draft. No merge, release, owner-data access, or main-branch change is authorized.


### Additional bounded traversal and interruption regressions (2026-11-07)

Following the reconciliation of PR #152 with current main, the bounded `sync_directory_tree` walk also increments and enforces the payload-file budget. Package-root metadata is included in the total-entry budget but not counted toward the 100,000-file payload cap. Synthetic injected-limit regressions cover exact-boundary file acceptance and rejection during directory sync. New checkpoint regressions cover an interrupted backup before publication, restore staging interruption, interruption immediately before final restore publication, rejection of conflicting partial staging on retry, and successful recovery to a distinct new destination. Source added to the existing Draft PR. Execution and exact-HEAD qualification remain NOT RUN until recorded by CI; these source additions do not certify SQLCipher/native Windows or real hardware durability.


### Two-way immutable object inventory and valid recovery (2026-11-07)

The online snapshot validator now requires **inverse object/catalog correspondence** in addition to opening every referenced object. The archive catalog derives expected encrypted object filenames from every immutable revision; the state SQLCipher `document_objects` table derives expected document names. Legacy raw-file inventory and its encrypted raw-map object are included, rather than silently dropped. The archive and document object roots share a checked entry/directory/file budget and reject stray names, unexpected shard layouts, missing referenced objects and extra ciphertext objects before backup or restore publication. No encryption key, backup v1 schema or existing ciphertext format changes are made.

A synthetic SQLCipher regression injects both a physically orphaned archive ciphertext and an independently encrypted but unreferenced document object; `VaultOnlineBackup::create()` must reject either without publishing the backup destination. The full-application recovery regression additionally preserves a good published encrypted generation while damaging other packages and injecting staging conflicts, then requires that the valid generation restores readable archive content and the original vault identity. An inserted object does not become accepted merely because its `MPOS\x01` header is syntactically plausible.

**Unverified limits:** These SQLCipher integration tests are code-added but require actual feature-enabled execution on hosted Windows and Ubuntu; ordinary source compilation is not proof they ran. The `backups` opaque ciphertext subtree is preserved under the MP-9B contract and has no separately authenticated logical object catalog in this task. Native owner-installed recovery and physical power-loss replay remain a distinct NOT RUN gate. 100K/1M workflow dispatch remains unauthorized. Keep PR Draft with no merge or release.
