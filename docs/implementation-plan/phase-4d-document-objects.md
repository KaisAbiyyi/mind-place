# MP-4D — versioned document object store

Claim: issue #25; Codex/GPT-6 managed cloud session `20261021-cloud-mp4d-01`, generation 1. Base `3b56726a9eadb9e65b46788b900b2edd9be2719f`; branch `feature/mp-4d-document-objects`; locks `object-store/state-schema`. MP-4A (#23) merged; no active conflicting claims found. Explicit owner assignment; serialized single writer, not an atomic claim controller.

## Contract

The store accepts a bounded byte reader supplied by a trusted caller after MP-4B source-handle authorization. It never converts virtual URIs into source filesystem paths. Every operation requires a caller-supplied unlock guard plus authoritative root approval/tombstone checks. No UI/MCP entry point is added.

Raw bytes use SHA-256 with format version 1 under `document-objects/v1/sha256/<prefix>/<hash>` beside state.sqlite3. Stream to a same-directory temporary file, bound size, hash, flush, publish without overwriting, sync the containing directory, then commit authority references. Existing objects must verify before reuse. A crash can leave an unreferenced object, never a committed reference to partially published bytes; startup does not delete evidence or orphans.

Schema v6 adds immutable object/version records and mutable current-document observations. Document identity is canonical MP-4A virtual URI; version identity is domain-separated URI + content hash. Equal bytes share storage across documents/roots; equal bytes at the same URI reuse a version, including A→B→A. Historical versions remain accessible only under a live root grant. Revocation preserves bytes and provenance while denying future store operations.

The metadata signature (stable file identity, byte length, modification/change timestamps) is a caller-observed hint, never proof of content or filesystem authority. An explicit probe may skip content reads when the signature matches and stored object metadata is present. Force verification always reads/hashes bytes. Source changes during reading must be rejected by the future MP-4B/4C handle reader; this store validates the declared length.

## Work and verification

1. Add behavior tests for version/dedup/restart, unchanged hints, root denial/revocation during reads, reader/guard failures, bounds, corruption and publication recovery.
2. Add v5→v6 migration with verified backup and rollback tests; preserve roots, claims and source identities.
3. Implement bounded internal store API and immutable schemas; freeze MP-4A interfaces.
4. Run focused Rust tests and full required baseline. Linux desktop build limitations must be reported; cloud Windows CI is the qualified build check.
5. Push sanitized checkpoints; leave PR for independent critical-boundary review and CI. No merge without independent review and green CI.

Cloud-only unit; no local implementation/qualification. Phase MP-4L1/MP-4L2/MP-4U remain mandatory later. No scanner, extractor, native source authorization, encryption, retention/GC, retrieval/UI, or scale claims in this unit.

## Integration ordering

Concurrent MP-4B PR #26 owns schema v5. This branch stacks on its checkpoint `fc3037b7f3d6b10e7d2b6f92b20bcb282acd9b03`; MP-4D adds v6 only. MP-4B must merge first; rebase onto its final SHA and rerun checks before MP-4D integration. Root approval/NTFS changes in the stack belong to MP-4B, not this unit.
