# MP-9B offline owner runtime and encrypted writer inventory

Date: 2026-10-28. Scope: source implementation and synthetic qualification; PR #127 remains Draft. Independent security review, actual different-Windows-account denial and owner-installed acceptance are separate gates. Never run these procedures on the owner's private archive during agent qualification.

## Explicit owner action

The source maintenance binary `mp-vault-migrate` provides `begin`, `resume`, and `rollback`. It is never called at startup, through MCP, or by capture/folder association. On Windows, after independently reviewing this candidate, the owner can build/run it using:

```powershell
cargo run --locked -p mind-place-index --features sqlcipher-preview --bin mp-vault-migrate -- begin
# After interruption, use the previously saved recovery code:
cargo run --locked -p mind-place-index --features sqlcipher-preview --bin mp-vault-migrate -- resume
cargo run --locked -p mind-place-index --features sqlcipher-preview --bin mp-vault-migrate -- rollback
```

This is a source maintenance entry point, not an added installer shortcut or a packaged graphical migration screen. The owner must close Mind Place, its daemons and capture processes first. Interactive Windows console input/output is mandatory; redirected secret input/output is rejected. Only the action verb is accepted as an argument. Password and recovery input have console echo disabled, bounded input and zeroizing buffers. The fresh independent recovery code is displayed only by the explicitly requested `begin` ceremony; never record/transcript that console or copy it to logs. Correct re-entry and the exact phrase `ENCRYPT MY LEGACY VAULT` are required before durable migration artifacts are created. The existing security verifier must unlock; its password is never used as an encryption key.

The root is the existing configured application root, parent of `archive-v1`; it is not supplied by a project/folder association. Qualification must override the existing `MIND_PLACE_DATA_DIR` configuration to a disposable synthetic `.../archive-v1`. The takeover agent never invokes a mutation command against the default owner root.

The offline guard opens existing state/derived SQLite files with read-only, read-sharing-only Windows handles. Existing or new write/replacement handles prevent acquisition or are refused while it is held. It also holds the legacy `.archive.lock` exclusive OS lock used by capture/import. Missing/nonempty archive locks, active WAL/SHM, reparse points, unknown source files and source inventory changes fail closed. This guard is supplemental to closing all app processes; it is not protection against a compromised unlocked owner session or unknown old binaries that ignore application coordination.

The fixed destination is `encrypted-vault-v1` beneath that root, which the desktop already discovers on restart. Hashed source/destination-bound journal, encrypted staging, operation lock, wrapped authorization and rollback quarantine live outside the legacy root. Only the exact published child is excluded from legacy inventory, and admitting it during resume requires the authenticated journal and verified output digest. The recovery-wrapped manifest is durable only after correct confirmation; it contains no plaintext DEK or recovery code. An interruption before journal creation can resume only if no conflicting journal/staging exists. Existing/tampered destinations are never adopted. Rollback quarantines staging and refuses a published destination; it never deletes plaintext. Retained plaintext remains unencrypted until a separate owner disposition; source migration is reversible, not secure erasure.

Successful conversion returns with the maintenance security gate locked and no native/MCP/tunnel service started. Restart the application: it starts locked and selects the encrypted child. Do not resume an old migration after subsequently using/mutating that encrypted vault; the original immutable output digest will intentionally fail. No automatic cleanup of recovery artifacts or old source is implemented.

## Production encrypted writers and snapshot cut

All entries below refer to encrypted mode. Plaintext legacy mode has a separate offline migration boundary.

| Writer / entry point | Shared fence lifetime | Snapshot/restore validation |
| --- | --- | --- |
| `StateStore::open_encrypted`, schema setup/migrations, writable `connection()` | Before SQLite open/migration through connection close; memory/permission/client/revocation/document metadata mutations use this wrapper | Keyed schema/application identity, SQLite integrity/FK; raw state values and row identities checked by migration |
| `IndexStore` encrypted init/connections, capture indexing, graph generation/maintenance | Before keyed connection open through FTS/graph/source-registration transaction completion; capture index connection spans source registration | Online Backup captures committed WAL pages; keyed schema and FTS external-content integrity |
| `DocumentIndex::refresh_root` | **Whole approved-root generation**, from scanner/object publication through state reconciliation and FTS commit, plus fenced connections | Deterministically paused after real object publication; backup must wait until the generation completes; restored search/preview reopens |
| `DocumentObjectStore::preserve` | Fenced state transaction surrounds immutable ciphertext publication and object/reference metadata commit; whole scan has the outer generation fence | State hash/length references must decrypt and match stored bytes |
| `ImmutableVaultStorage::write_immutable` | Shared fence through AEAD chunks/manifest and no-clobber publication | Authenticated readback; raw-map hash/size verification; no plaintext temporary object |
| `VaultCaptureStore` capture/import/catalog and reads that may open WAL | Outer shared fence spans encrypted raw payload publication and catalog commit; all historical catalog revisions validated | Canonical archive/snapshot identity, full metadata and decryptable payload; migrated assets read via encrypted raw mapping |
| Native capture IPC | Delegates commits to `VaultCaptureStore` under current unlock lease and document authority gate | Descriptor owner/DACL/handle checked; HMAC session, nonce replay, origin, bounds and old-epoch denial in separate-process synthetic tests |
| Local/hosted MCP and desktop background operations | Keyed `VaultCaptureStore`, `StateStore`, `IndexStore`, `DocumentIndex`; no independent raw SQLite write path | MCP keyed credential/revocation suite; locked epochs deny authority; no remote backup/migration command |
| Online backup | **Exclusive** vault fence for the complete multi-domain snapshot/ciphertext copy/verification/publication | Four SQLCipher databases copied via SQLite Online Backup; independent WAL/SHM excluded; raw mapping, capture revisions, document references and FTS checked **before publication** |
| Portable backup / restore | Offline consistent snapshot input; create holds exclusive fence. Restore writes only a new staging root | AEAD manifest, exact file receipts, bounded paths/size, source recheck, durable atomic no-replace publication; online restore adds product coherence before final publication |
| Provisioning / migration | New destination only; confirmed wrapped manifest, separate migration operation lock, offline source guards | Journal v2, final output hash, real SQLCipher row/value/rowid + raw/archive/media/document/manifest equivalence; restart and rollback tests |

An index can legitimately lag an earlier capture commit because derived indexing is rebuildable. A backup must preserve authority and object references coherently; it must not manufacture a missing derived generation or silently drop raw data. The new approved-root generation fence closes a measured snapshot cut that previously restored document authority but zero search hits. Nested locks are shared; snapshot verification works on separate scratch/staging roots and never upgrades a live shared source lock to exclusive.

## Evidence boundaries

Local synthetic checks cover seven actual abrupt converter process exits, source preservation, values/row IDs, raw generic imports/missing timestamps, attachment retrieval with plaintext source offline, wrong key/journal/output mutation, concurrent generation backup, invalid schema/missing object rejection, restored keyed search/preview, Windows sharing and owner/anonymous-token ACL probes. Exact logs/SHAs and hosted receipts are recorded in the implementation summary.

The anonymous impersonation child is **not a different installed Windows account**. Actual hostile-account access denial, installed-owner recovery/migration experience and independent review remain NOT RUN/PENDING. The 100K gate measures its existing physical synthetic document workload; it is not an encrypted-vault performance or private-corpus/owner acceptance certificate. Future writes, native account changes, packaging, broader crash qualification and release gates must not inherit synthetic PASS without their own evidence.
