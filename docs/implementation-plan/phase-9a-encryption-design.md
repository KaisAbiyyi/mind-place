# MP-9A — measured encryption-at-rest architecture and backup impact

Status: **cloud-only prototype / architecture proposal; NOT a production encryption migration**. Owner issue [#102](https://github.com/KaisAbiyyi/mind-place/issues/102). Resource lock: `security-architecture`. Prerequisite MP-8F merged [#95](https://github.com/KaisAbiyyi/mind-place/pull/95). Direct successor: MP-9B; production changes require a separate authorization and migration gate.

## Current verified boundaries (read from source)

- `crates/security-core/src/lib.rs` uses an Argon2id **unlock verifier**, which is not yet an encryption-at-rest key.
- `crates/state-core/src/lib.rs` opens plaintext `state.sqlite3` with bundled `rusqlite`, `WAL`, `synchronous=FULL`, identity/schema/integrity checks, and verified pre-migration SQLite snapshots under `state-backups`. As of this investigation state schema is v9; do not change it here.
- `index.sqlite3` is a disposable/rebuildable SQLite/FTS5 projection. Content-bearing FTS columns and WAL must not remain plaintext after the encryption migration.
- Raw `archive-v1` JSONL is immutable canonical evidence; `document-objects/v1/sha256/<prefix>/<hash>` contains SHA-256-addressed immutable source bytes. Encrypted storage cannot cover only SQLite and ignore these plaintext evidence/object paths.
- Approved external document roots are **not automatically copied into the vault** and root authorization is separate from file encryption. Backup must distinguish app-owned captured bytes from external documents it cannot portably restore.

This is an inventory, not a claim that encryption is already present.

## Threat and non-threat boundaries

| Threat | Proposed mitigation | Not claimed |
| --- | --- | --- |
| Lost/offline disk, copied app data, untrusted backup destination | Encrypt all canonical archive revisions, state/index databases, derived/object bytes and portable backups; store no plaintext keys alongside encrypted data | Protection from a logged-in compromised OS |
| Cross-user or copied vault directory | Random per-vault DEK wrapped with OS user-bound protection and independently recoverable passphrase/recovery secret | DPAPI by itself is cross-device portable |
| Stolen backup | Authenticated, versioned manifest + AEAD-wrapped backup key, explicit recovery secret, integrity checks | Upload / sync / host access without owner permission |
| Malicious modification/truncation/substitution | AEAD AAD binding (vault ID, object/version ID, format), keyed object IDs and authenticated manifest with version, counts, hashes, schema | Rollback detection without separate trusted monotonic state |
| File names, sizes, SQLite WAL/SHM, temp, deleted sectors, logs | Encrypted container contents, guarded secure temporary staging, no plaintext dumps/backups, memory hygiene and limited metadata | Complete metadata privacy, crypto-erase of old copies, secure deletion from SSD |
| Malicious same-user process while vault unlocked | Lock epochs/permission checks, least-privilege boundary, session expiry | Resistance to keyloggers, memory dump malware, or admin compromise |

**Owner consent contract:** No automatic encryption migration; no reapproval of external folders, hidden reads or background imports; do not infer consent to changing retention, backup destination, recovery UX or password policy. Lock must stop plaintext index access and prevent new privileged disclosures. Deny access to revoked roots regardless of encryption state.

## Options to measure and decision matrix

| Approach | State SQLite | FTS index | Canonical raw + object bytes | Recovery and portability | Evaluation |
| --- | --- | --- | --- | --- | --- |
| Full disk encryption only (BitLocker/device encryption) | OS-level, dependent on settings | OS-level, dependent on settings | OS-level, dependent on settings | Machine/recovery-key dependent | Useful defense in depth, not an app-controlled portable vault |
| Application AES-256-GCM **per record** | Possible with row serialization | Loses normal plaintext FTS; full decrypt scan or special index required | Suitable for immutable objects | Vault DEK + independent backup wrap | **Synthetic measured**; not recommended for FTS-heavy query DB without a separate encrypted index |
| SQLCipher-compatible whole-file SQLite + per-object AEAD | Encrypts pages incl. content under appropriate deployment | Preserves SQL/FTS feature semantics while encrypting pages | Needs separate authenticated object/archive envelope | Portable if database key is independently recoverable | **Preferred candidate for MP-9B**, but SQLCipher product build, WAL/temp, migration, platform packaging **NOT measured in MP-9A** |
| Encrypt only backups | Live state/index still plaintext | Live index still plaintext | Live evidence remains plaintext | Improves transport/offsite backup | Insufficient for local at-rest privacy |

**Provisional choice (requires owner approval and SQLCipher qualification):** independent random vault data encryption key(s), SQLCipher for authoritative and disposable SQLite, AEAD stream/chunk format for archive/object bytes, keyed content IDs for dedup with no cross-vault equality leak, separately AEAD-protected backup. Use vetted libraries, versioned envelopes, 96-bit random nonces with collision prevention by design, unique per-vault key domains and authenticated associated data. Evaluate misuse-resistant construction if lifetime/volume makes random-nonce safety unacceptable. Do not invent a homegrown cipher.

## Proposed key/recovery lifecycle for MP-9B (not implemented here)

1. After explicit owner setup and unlock, generate cryptographically random 256-bit vault DEK; do **not** use the current verifier hash or a 4-digit PIN as encryption material. If deriving a wrap key from a password, benchmark and pin a separate Argon2id policy (version, salt, memory, time, parallelism).
2. Windows Credential Manager/DPAPI should wrap a key for convenience only and bind it to a user/machine. Test what profile changes, Windows reinstallation, other machines and device loss mean; never assume Windows-hosted CI is proof on owner hardware.
3. Require an explicit, tested **independent recovery method** (e.g., high-entropy recovery secret encrypting/wrapping the DEK), separated from any local DPAPI blob. Present loss warning, verification and rekey/rotation UX. Do not quietly store recovery material in app data, GitHub artifacts or telemetry.
4. While locked, no plaintext SQLite open, search, indexing, extracted previews, API disclosure or connector operations; rotate/invalidate leases on lock and replace ephemeral keys with best-effort zeroization. No claim against host compromise or pagefile/crash leakage.
5. Keep vault identity and envelope schema version; authenticated context binds record ID, document root/provenance ID or object hash where appropriate; add a tested rekey + interruption/rollback state machine **only in MP-9B**.

## Migration and backup/recovery sequence required before shipping

- **Preflight:** confirm owner approval, supported SQLite cipher ABI, provenance and durable source checks, adequate free disk for at least full duplicate snapshots plus WAL/temp overhead; record exact bytes/DB schema and backup objects. Do not overwrite existing `state.sqlite3` or mutate canonical JSONL in place.
- **Quiesce + snapshot:** fence writers and readers (including agent connections); use SQLite's consistent online backup API, not naïve copying of live WAL; snapshot state, raw JSONL, object store and security metadata to the **same generation**, with an integrity-checked manifest. Derived index can be rebuilt, but must not be exposed plaintext during migration.
- **Stage:** migrate into a new encrypted location with a deterministic manifest, AEAD-verified content and source/provenance hashes, FTS consistency, SQLite identity/checksum/foreign keys, full decrypt/readback; verify all key wraps and recovery before atomically switching pointers.
- **Commit + rollback:** double-buffer generation pointers; fsync files and parent dirs; test power loss at every staging/promotion boundary; preserve prior bytes on failure and lock/fail closed. Never silently delete plaintext legacy backups/orphans: provide an explicit owner-approved retirement plan and caution about immutable SSD/snapshot copies.
- **Portable backup:** authenticate a format-versioned manifest containing vault generation, schema, canonical snapshot, object/archive content set, hashes, exclusion semantics, and key recovery metadata; encrypt the archive with a separate backup-encryption envelope. Restore into a new isolated vault and validate before activation. External source roots require explicit re-grant on the new device; backup does not claim to contain external files.
- **Threat of rollback:** AEAD alone does not prove freshness. For backups, expose generation/timestamp and owner-confirmed rollback acceptance; a high-assurance monotonic anchor is future work.

## Isolated reproducible measurement (synthetic only)

`scripts/mp9a-encryption-probe.py` writes bounded generated fixture data to an OS temporary directory, runs Python SQLite/FTS5 baseline versus AES-256-GCM app-level row encryption (no FTS), tests indexed rare-token lookup versus **full decrypted scan**, compares object read/write/bytes, creates live-consistent SQLite snapshots via `sqlite3.Connection.backup`, and times a portable AEAD backup envelope/restore. Wrong key, corrupted tag, wrong associated context, sqlite integrity and recovery assertions are **hard gates**. Object names use per-vault keyed HMAC-SHA256 in the prototype; this differs intentionally from today's plaintext global SHA-256 addressing and is **not** a migration implementation.

Cloud qualification: `.github/workflows/mp9a-security-probe.yml` runs Python 3.12, cryptography 46.0.3 on GitHub-hosted Ubuntu 24.04 + Windows Server 2022, with exactly 1000 rows × 1024 bytes and 128 objects × 16384 bytes. JSON receipts omit source payload, keys and temporary file paths; CI artifacts retained 14 days.

**Interpretation rules:**
- These observations are one synthetic microbenchmark per OS, **not** p95, user hardware, product `rusqlite`, or a production SQLite encryption implementation. App-row full scan and FTS index lookup are deliberately not equivalent search strategies; the comparison highlights FTS loss, not an apples-to-apples cryptographic speedup.
- SQLCipher full-file encrypted index, app open/restart, WAL checkpoint, journal + temp safety, actual Rust object store, incremental backup and Windows DPAPI remain **NOT_MEASURED** until dedicated instrumented product tests. Never label these PASS from this workflow.
- Always record exact CI head SHA, platform, fixture, artifacts and failures before assigning a review verdict. No fabricated performance numbers; GitHub Actions receipts are the authoritative measured outputs.

## Acceptance and owner decisions

MP-9A can close its **design/measurement unit** only after: (a) successful Linux + Windows receipts at the final PR HEAD, (b) critical self-review of code, security boundaries, fixtures, disclosure rules and diff, (c) documented unknowns and 9B follow-on gates, and (d) the owner sees and approves which *proposed* encryption/recovery/permission policy may proceed to a production migration. The present document records **proposal**, not owner acceptance.

Owner choices needed before MP-9B changes data: **(1)** whether to enforce mandatory local vault encryption or opt-in (and legacy behavior), **(2)** independent recovery format, loss/rotation semantics and minimum-strength unlock UX, **(3)** portable backup scope/retention/location and whether old plaintext snapshots may be retired, **(4)** whether the threat model explicitly excludes compromised unlocked Windows sessions.

No private archive, actual Windows owner credential store, installed app, native screenshots or local workspace is accessed in MP-9A.
