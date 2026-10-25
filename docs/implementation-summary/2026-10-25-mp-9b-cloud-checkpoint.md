# MP-9B cloud implementation checkpoint — 2026-10-25

Issue: https://github.com/KaisAbiyyi/mind-place/issues/126
Draft PR: https://github.com/KaisAbiyyi/mind-place/pull/127
Resource lock: crypto-migration. Owner policy #125 accepted.

## Implemented in cloud (isolated security primitive)

- Random 256-bit vault DEK, independent generated 256-bit offline recovery secret, unique vault ID. Existing PIN/verifier is never key material.
- AES-256-GCM immutable archive/document/backup envelopes with vault, role and canonical identity AAD, random 96-bit nonce, versioned framing, size bounds, fail-closed authentication and best-effort key zeroization.
- Distinct Windows DPAPI convenience wrap, independent portable recovery wrap, unlock-lease fences before and after encryption/decryption, new-vault-only no-clobber manifest write with reparse/symlink ancestor rejection.
- Synthetic tests: wrong recovery secret, tamper, cross-vault/cross-kind/cross-identity attacks, locked stale lease, malformed format/replay, existing vault non-adoption and symlink ancestry. Hosted Ubuntu 24.04 Rust 1.90 test and formatting workflow PASS: https://github.com/KaisAbiyyi/mind-place/actions/runs/40174682598.

## Mandatory gates NOT IMPLEMENTED / NOT QUALIFIED

1. SQLCipher Rust SQLite build, encrypted state/index/document-index + WAL/temp and FTS qualification, key-before-read throughout product, native Windows packaging.
2. Production archive/object writer-reader integration, keyed object IDs, durability/interruption and identity readback; current app storage remains plaintext.
3. First-run mandatory encryption onboarding, recovery UX, key lifecycle integration and secure deletion of unlocked ephemeral handles.
4. Generation-consistent portable encrypted backup/restore with explicit external root exclusions and configurable retention.
5. Owner-consented legacy migration staging and rollback, consistency and non-destructive preflight. Policy approval is not authorization to modify real owner archives/keys/backups.
6. Product Windows/Rust/web CI PASS on final PR HEAD, measured performance and independent code/security review.

## Handoff

This is a bounded cloud checkpoint. MP-9B stays In Progress and PR #127 stays draft. MP-9C/9D/9E remain blocked until all required gates pass. Do not merge as production encryption. No owner data or local Windows workspace was accessed.
