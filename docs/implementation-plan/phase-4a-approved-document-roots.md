# MP-4A — Approved document root identity and virtual namespace

Task: MP-4A, GitHub issue #22. Base: `149cb18b4eea903606d73874d4d0b7e49ae6600b`. Claim: ChatGPT cloud session `2026-10-21-cloud-mp4a-01`; lock `folder-identity/state-schema`. Prerequisite: MP-3U owner PASS.

## Scope and fail-closed contract

- Extend the authoritative `state.sqlite3` to v4 with an **opt-in** `document_root_registry` keyed to existing `source_roots`. No migration elevates any legacy `source_roots` grant. Already revoked roots remain revoked; provenance/tombstones remain immutable.
- Explicit staging API accepts an existing directory, obtains its absolute canonical representation without reading contents, rejects symlink/reparse components at staging, and creates deterministic `root:v1:<sha256>` identity with a versioned path framing. Same path is idempotent; distinct roots cannot alias. A staged request is **pending**, not authorization.
- The virtual namespace `mpdoc://root/<root-id>/<percent-encoded-segments>` is a **logical identifier**, not a file system path or token. Canonical parser/encoder must reject traversal, encoded separators, backslashes, Windows drive syntax, malformed/overlong encodings, reserved device names, and double decoding.
- Registry read and virtual binding resolve fail closed: only an explicitly approved **and still live** root can resolve. MP-4A does not expose a public approval operation, arbitrary file reads, scanning, MCP operations, or a path-to-handle conversion. The explicit owner authorization/UI and handle-level NTFS race-free open will be completed by MP-4B.
- Keep raw archive immutable and indices rebuildable; revoke operations preserve tombstones and invalidate future access immediately at authoritative state lookup. Existing `add_pending_root` stays backward compatible but does not auto-enroll in the new document namespace.
- Desktop adds a bounded **pending root registration** UI accessible only after unlock; it must explicitly state that staging does not index/read or grant permission. No manual grant or admin MCP surface in MP-4A.

## Validation / acceptance

1. Fresh v4 install, v3 -> v4 transaction/backup, identity/checksum and legacy-root non-adoption tests.
2. Deterministic round trips, unicode/boundary checks and adversarial traversal/malformed URI cases.
3. Idempotent registration, no roots outside registry, no active roots before explicit future approval, revocation/tombstone and locked UI/Tauri boundary tests.
4. GitHub cloud CI: `npm run verify:repo`, `npm run typecheck`, `npm test`, `cargo fmt --all -- --check`, `npm run build`. Review exact final head, merge on PASS.

## Explicit MP-4B handoff

MP-4B owns actual owner approval workflow, NTFS handle-level containment, symlink/junction/reparse/root replacement validation and read authorization. Phase 4C owns scanner/watcher revocation reconciliation; 4D object store; 4E extractors. No Windows-only certification is claimed here.
