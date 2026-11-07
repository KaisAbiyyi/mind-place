# MP-9E — Atomic portable backup / restore integrity hardening

**Task:** MP-9E · [#149](https://github.com/KaisAbiyyi/mind-place/issues/149). **Execution:** ChatGPT GPT-6 Cloud, exclusive `backup` lock. **Source baseline:** `main` at `24648c74f3f59e176974d3878049b882478db69b`.

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
