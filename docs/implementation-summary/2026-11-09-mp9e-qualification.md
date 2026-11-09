# MP-9E reclaimed qualification and native handoff

Claim: `mp9e-reclaim-20261109-01`, Codex Cloud, exclusive `backup` resource.
PR: [#152](https://github.com/KaisAbiyyi/mind-place/pull/152), kept Draft.
Reconciled by non-force merge with main
`320f711cb9a8dc8a13b83bd5ad99a39a9ee3bb70`.
The PR handoff supplies the final immutable head, source tree and run receipts;
older-head receipts are historical and must not qualify a changed head.

## Executed-path qualification contract

The application restore entrypoint is `VaultOnlineBackup::restore`, which
authenticates the portable inventory and validates keyed SQLCipher state,
archive catalog, FTS, document bytes/hashes and inverse object inventory before
publishing the new directory. PortableBackup alone does not qualify this path.

Private no-op checkpoints in the production coordinator allow unit tests to
terminate a child process without destructors at these boundaries:

| Operation | Process-exit checkpoints |
| --- | --- |
| Backup | scratch manifest, first SQLCipher copy, ciphertext copy, coherent verification, before package call, after package publication |
| Restore | authenticated portable copy, coherent verification, before final directory publication, after publication |

The parent requires the exact injected exit code, checks publication visibility,
preserves interrupted staging and retries without replacing it. A second fresh
process restores the separately published good generation. Archive readback,
FTS search, encrypted state header and vault recovery are checked after restart.
The after-publication case must already be usable even when acknowledgment was
lost. These are **process exits with the host OS still running**, not power cuts.

Additional application tests cover final destination creation by a competitor,
cross-process participating writer blocking, truncated SQLCipher payload,
unexpected package roots, unlisted payload and Unix symlink substitution. Existing
tests retain missing/foreign authority, orphan archive/document ciphertext,
tampered manifest/payload, stale staging, unlock revocation and WAL/FTS readback.
Small injected budgets test oversized empty-directory trees without allocating
an unbounded fixture. Windows document writer/state/index integration has its
own OS-specific test. Windows symlink privilege is not assumed by Unix tests.

The existing `mp9b-sqlcipher-qualification.yml` dispatch now validates frozen
head/base and the exact two-parent merge checkout using the existing native
contract verifier before executing tests. Its OS artifacts retain source-tree
identity, run/attempt, contract fingerprint and application/portable test logs.
The generic contract's `sqlcipher_required` policy field is not an acceptance
verdict for MP-9E; actual execution logs establish the tested feature profile.
Tests receive no token or repository secret; the short provenance step has only
the workflow's read-only GitHub token. No `pull_request_target` or write grant.

## MP-9C authority boundary — proposed, not implemented acceptance

Coordination is posted to [MP-9C PR #151](https://github.com/KaisAbiyyi/mind-place/pull/151#issuecomment-6744286667).
No MP-9C-owned file is edited by this reclaim. In particular, source tombstones,
privacy fences, archive disclosure, migration and trusted owner confirmation
remain with `privacy-deletion`. Historical MP-9E archive/legacy inventory changes
are retained without further edits to those shared files.

A coherent old backup can roll back state-local tombstones. Neither a valid
manifest nor the caller-supplied backup generation proves monotonic privacy.
The following interface requires agreement and joint qualification before
destructive-forget or anti-resurrection acceptance:

1. MP-9C owns a per-vault authenticated monotonic epoch/head digest and canonical
   source tombstone log, durable outside every rollback-able vault/backup root.
   Owner custody/recovery enrollment must detect missing/tampered/regressed
   authority; restore must never silently create an empty authority.
2. MP-9E records each retained generation's vault/epoch/head binding and
   disposition. Missing or stale authority/disposition blocks publication.
   Retention changes require explicit owner authorization, not automatic cleanup.
3. Restore holds a stable authority read/fence from preflight through publication;
   stage state and every disclosure/rebuild path must replay retained tombstones.
   Revalidate epoch/head immediately before atomic publication and validate the
   reopened result before exposing it. If replay is unavailable, reject stale
   generations. A copied state snapshot is insufficient authority.
4. Joint tests must cover stale coherent backup after tombstone, authority
   removal/tamper/rollback/cross-vault substitution, concurrent epoch advance,
   crash before/after authority commit, restore publication and fresh reopen,
   shared-source custody and retained-generation disposition.

This PR does not implement or certify that external authority. The current
backup/restore API alone must not be advertised as preventing redaction
resurrection. MP-9C and MP-9F remain blocked on the agreed integration; no
retained backup, interrupted staging or owner data is deleted here.

## Exact-head owner-native handoff

Status is **NOT_RUN** for installed-owner qualification. After independent review
and explicit owner scheduling, use an isolated Windows x64 checkout at the full
SHA published in the final PR handoff. Verify `git rev-parse HEAD`, a clean tree,
and the source tree; record OS, NTFS volume, Rust/MSVC, WebView2 and installed
binary hashes. Do not implement fixes in the qualification checkout.

Run the synthetic source tests first, saving complete output and exit codes:

```powershell
cargo test --locked -p mind-place-index --features sqlcipher-preview --lib vault_online_backup -- --nocapture
cargo test --locked -p mind-place-index --features sqlcipher-preview --test vault_online_backup --test vault_backup_consistency --test sqlcipher_index_backup --test vault_capture_encrypted -- --nocapture
cargo test --locked -p mind-place-security vault_backup -- --nocapture
```

These source commands do not prove an installed application journey. Separately
qualify the owner-approved installed runtime on disposable copies only: initial
locked denial, explicit unlock/recovery enrollment, online committed-WAL backup,
fresh empty destination restore, archive/document/FTS/hash/count readback,
restart/relock/fresh unlock, wrong recovery and corruption rejection, existing
destination/staging preservation, competing writer behavior, and recovery from
a previously verified good generation. Never pass recovery material on command
lines or include it in receipts. Report each case PASS/FAIL/PARTIAL/BLOCKED with
exact source/build identity; unsupported installed entrypoints are BLOCKED.

Real private corpus access, deletion/retention, different-account key custody,
physical power-loss and subjective owner recovery acceptance require their
separate explicit gates. Hosted Windows PASS cannot satisfy them. Physical
power-loss acceptance is **NOT_RUN and not claimed**. MP-9L2/MP-9U2 remain behind
MP-9F; this handoff does not start or accept those downstream tasks.

Recovery: fetch the remote task branch and verify the handoff SHA before resuming.
Revalidate this claim generation and both resource owners before a new push.
Keep failed staging for explicit disposition. New fixes require new exact-head
CI and independent review. The human owner alone may approve and merge.
