# MP-6C — Graph generation activation contract

**Environment:** GitHub cloud only. **Owner:** ChatGPT Cloud / `mp6c-20261023-1936-wib-gpt6-cloud`. **Prerequisite:** MP-6A merged `008a5580`. **Lock:** `graph-build`; MP-6B owns graph queries.

## Scope and invariants

- Derived `index.sqlite3` v3 adds `graph_generations`. Preexisting v1/v2 index upgrades atomically and retains FTS + graph rows; no archive or `state.sqlite3` data migrates.
- `GraphInputHashes` provides verified SHA-256 lowercase hex digests of file inventory, source content/revision and structural dependencies. The trusted caller must compute/verify them from authorized sources; this graph layer does not independently certify input content.
- `activate_graph_generation[_with_check]` takes an **expected generation** (including `None` for a never-seen scope), hashes the complete staged candidate deterministically, then swaps edges/nodes and the active manifest inside one `IMMEDIATE` SQLite transaction. Identical hashes + candidate skip rewrites. Mismatch yields `MP_INDEX_STALE_GENERATION`.
- `invalidate_graph_generation[_with_check]` atomically clears graph rows, prunes unreferenced nodes and advances an **inactive tombstone** before a slow source rebuild, protecting against stale writes.
- Permission/revocation callbacks run before and immediately before commit; an error rolls back graph and manifest together. Ordinary legacy graph replacement is rejected for generation-managed scopes; it cannot bypass fencing.
- `archive:chatgpt` remains exclusively managed by the MP-6A archive synchronization path. No changes to raw archives, state authority, or unapproved automatic source scans.
- Graph consumers such as MP-6B should inspect `graph_generation` for managed scopes if they require `active=true` / version-pinned retrieval; this task does not implement MP-6B query interfaces or MP-6D authorized graph mutations.

## Qualification

Dedicated Rust tests cover: deterministic hash and order independence, no-op, file/structure/content hash changes, CAS stale attempts, cancellation rollback, inactive tombstones, rejection of unfenced writes and bad hash input, and lossless v2→v3 FTS/graph migration. Hosted product CI plus storage 10K/100K/1M jobs must be evaluated on the **final PR HEAD**; any pending/nonexecuted checks remain `NOT_RUN`. Real owner/runtime gates are not implied by hosted CI.

See [GitHub issue #56](https://github.com/KaisAbiyyi/mind-place/issues/56) and [Notion MP-6C](https://app.notion.com/p/400c4a6aaf738168bfabc4b8561febce).
