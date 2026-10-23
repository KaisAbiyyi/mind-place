# MP-6E — Graphify prompt templates and synthetic graph qualification

Cloud implementor: ChatGPT GPT-6 (claim `mp6e-20261023-2303-wib`).
Issue: [#72](https://github.com/KaisAbiyyi/mind-place/issues/72).
Resource lock: `graphify-integration`. Prerequisites MP-6B and MP-6D merged.

## Supported scope

- `graphify_prompt(GraphifyPromptInput)`: deterministic ready-to-paste Codebase, Documents, or Mixed guidance. Call only with trusted approved-root catalog identity, human-readable alias, current 64-character source revision and relative include/exclude globs. Rejects native path / drive / newline / traversal-shaped selectors; does not read any file or produce authorization.
- `qualify_graphify_batch` and `submit_graphify_batch`: bounded producer-side rejection of missing provenance, duplicate proposals, unsupported speculative relation kinds and inferred-edge confidence below 8,000 basis points. `GraphMaintenanceJob::accept_batch` independently rejects low-confidence inferred edges so bypassing the wrapper cannot silently publish them.
- `GraphOrigin::Inferred { confidence_bps }` or `GraphOrigin::Deterministic` is retained as native derived-graph metadata. Inference is never mislabeled as deterministic. Existing evidence locators stay intact; graph neighbors return origin and confidence.
- `index.sqlite3` v4 upgrades the **disposable** graph edge table only. Existing v1–v3 FTS/index data and deterministic edge identities survive migration. No archive JSONL, state database, root permission or native file is modified by Graphify. Rebuildability and current lease/revision/generation fences remain with MP-6D.

## Trust boundary

A prompt is not a write permission. The public Mind Place MCP remains **read-only**. Model-produced batches must be reviewed by the trusted, unlocked local desktop producer and passed to its **in-process** `GraphMaintenanceJob`. Never expose the job or a long-lived bearer token through HTTP/MCP. Begin with an approved root and live source fingerprint; batch size ≤512 nodes and ≤512 edges; total ≤20,000 of either kind; source revision and permission rechecked for each submission and finish. Aborted jobs are invisible.

Confidence means an **annotated model estimate**, not ground truth: a value of 9,250 is not a calibrated probability. Structural facts should use `Deterministic` only when independently established from exact source. Inferred claims are limited to `References` and `Contains` and must be evidence-linked.

## Synthetic acceptance corpus

`crates/index-core/tests/graphify.rs` checks three prompt profiles; hostile alias and absolute path rejection; low-confidence/duplicate/stale/unsupported relation denial; accepted producer batch and atomic scope publication; preserved true source locator and stored inferred confidence in neighborhood query; and v3→v4 migration with a deterministic edge still present. Existing MP-6A–6D regressions also run under Windows product CI and Phase 1E storage matrix.

**Boundaries:** No live model/provider inference, native Windows approved-root walkthrough or UI button is claimed here; Phase 8 UI owns presentation and later project/owner gates own actual source coverage. No Graphify autonomous inference loop, model-network client, file scanner or remote write endpoint is added. Coverage/precision evaluation here is a reproducible synthetic protocol gate, not real-world semantic recall certification.

Review receipt and exact-head CI outcome belong on PR #73 (or the resulting PR number) and [the Notion task](https://app.notion.com/p/400c4a6aaf738190842eda84a4ea797d).
