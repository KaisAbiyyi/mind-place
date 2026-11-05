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
