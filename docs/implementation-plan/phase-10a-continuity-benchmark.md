# MP-10A — Synthetic continuity gold set and reproducible scoring

**Scope:** Cloud benchmark *contract* only. **Status:** implementation pending exact-head CI and self-review. **Prerequisite:** MP-7E merged; **lock:** `benchmark-gold`. This is the data/evaluation handoff for MP-10B and MP-10C, not a claim about their measured results.

## Frozen inputs

- `benchmarks/continuity/v1/corpus.json`: 24 **entirely synthetic**, invented Aster/Beryl evidence records spanning ChatGPT, Claude, Gemini, generic documents, dates/revisions, conflicting decisions, permission boundaries, and a deliberately restricted record. These are not a user's conversations or a dump of an installed archive.
- `benchmarks/continuity/v1/gold.json`: 18 owner-independent synthetic continuity questions, 16 answerable and 2 that **must abstain**. Gold labels reference stable synthetic source IDs and short abstract claim IDs. Relevance is graded 1–3 and evaluated at `top_k=5`. Each question explicitly excludes the restricted source `e20`.
- Both fixtures carry matching dataset and versioned schema IDs. The CLI receipt includes SHA-256 of each literal JSON file, so subsequent bake-offs can prove they used the same frozen fixture.
- A case's required claims must be traceable to at least one *public* relevant gold source. Gold intentionally penalizes stale/superseded assumptions, project confusion, unsupported assertions, fabricated details, false access and private evidence leaks.

## Reproduce (Node 20+; no network or paid LLM)

```sh
node --test test/continuity-benchmark.test.mjs
node scripts/benchmark-continuity.mjs --oracle
node scripts/benchmark-continuity.mjs --predictions ./path/to/predictions.json
```

**CRITICAL:** `--oracle` constructs ideal answers from gold itself. It validates fixture consistency, scoring and determinism only. Oracle scores are **not** measurements of Mind Place, a retriever, model, provider, or user experience. Benchmark quality must be measured with separately collected product predictions in MP-10B (and scaled fixtures in MP-10C).

The command emits structured JSON to stdout; redirect to a JSON file for comparison. Output deliberately omits wall-clock timestamps; the input SHA-256 values uniquely identify this benchmark version. Do not feed a private real corpus into this public fixture scorer.

## Prediction contract

Submit exactly one prediction for every gold case (order arbitrary), with no extra/missing IDs:

```json
{
  "schema": "mind-place-continuity-predictions/v1",
  "dataset": "mp10a-synthetic-v1",
  "cases": [
    {
      "id": "q01",
      "hits": ["e03", "e01"],
      "claims": ["final_lexical_decision"],
      "abstained": false,
      "latency_ms": 4.5,
      "context_estimated_tokens": 124
    }
  ]
}
```

The example contains **only one case for illustration**; it will be rejected until all 18 are provided. `hits` are unique ranked **stable evidence IDs** from the frozen corpus, never temporary index row IDs. `claims` are abstract fact IDs that the caller explicitly asserts in the answer, not automatically inferred from retrieved snippets. The scorer validates IDs, complete case coverage, finite nonnegative latency, nonnegative integer token estimates, array bounds, duplicate-free lists, and source scope. A recorded `latency_ms` is caller-supplied and not independently measured by the scorer. `context_estimated_tokens` is an estimate, not exact model billing.

A retrieval-only experiment can supply empty `claims` and `abstained=true` to inspect ranking metrics, but answer accuracy then deliberately remains low for answerable cases.

## Metrics and interpretation

| Metric | Definition / constraint |
| --- | --- |
| `macro_recall_at_k` | Mean across answerable questions of `relevant retrieved / relevant gold`; binary relevance independent of grade. |
| `macro_precision_at_k` | Mean answerable `relevant retrieved / k`, with `k=5`; an oracle returning only two correct sources therefore has precision 0.4. |
| `mean_reciprocal_rank` | Mean reciprocal position of first relevant source, zero for missing. |
| `macro_ndcg_at_k` | Mean graded DCG normalized by ideal graded ordering, gain `2^grade-1`. |
| `macro_support_fraction` | Fraction of emitted claim IDs backed by *public, non-excluded top-k returned evidence*; abstentions handled explicitly. High support alone does **not** prove the answer is correct. |
| `answer_accuracy` | Strict required-claim coverage, no unapproved/unexpected claims, correct abstention when required, and zero privacy leak for that case. |
| `abstention_accuracy` | Strict correctness on gold unanswerable questions only. |
| `privacy_violation_count` | Restricted or excluded source returned **anywhere in the supplied ranked list**, including below k. Not an authorization system or security certification. |
| `unsupported_claim_count`, `prohibited_claim_count` | Number of ungrounded and explicitly contradicted/forbidden claim IDs. Counts can overlap; not independent probabilities. |
| `mean_latency_ms`, `mean_context_estimated_tokens` | Means of caller-supplied values; no p95, local-device SLA or tokens billed are implied. |

Metric numerators/denominators and individual case diagnostics are included in `cases`; do not optimize for only the macro aggregate. Only the explicit scored candidate set is measured; this benchmark cannot independently prove that no other source exists, that a language model's natural-language answer is faithful, or that a user correctly approved access.

## MP-10B / MP-10C handoff

1. Keep corpus + gold unchanged for comparable baseline runs. If gold corrections are needed, create **v2**, preserving v1 hash and report provenance.
2. Implement an adapter that indexes/queries these exact synthetic records in a temporary authorized test archive or derived index and maps the retriever's **stable provenance identifiers** back to `e01...e24`. Do not silently map disposable SQLite rowids to gold IDs.
3. Produce independently gathered ranked `hits` and (when evaluating synthesis) explicit `claims`; include per-case wall-clock retrieval latency and context token estimate. Run scorer without modifying it. Report corpus hash, scoring version, code SHA, environment, and tested retrieval strategy (lexical, structured, graph, hybrid).
4. Evaluate error classes separately: superseded decisions, as-of history, wrong project, provenance, abstention, revocation/private scope, and unsupported claims. Investigate failures rather than inventing a favorable benchmark.
5. MP-10C must separately exercise the 1M-chunk/100K-file workload and startup, resource and integrity gates; the 24-source gold suite is *not* a scale qualification.
6. Live providers, Windows installed hardware, real archives, owner acceptance, migration and release remain their own later gates. Cloud synthetic PASS cannot substitute for those.

## Acceptance and failure modes

The Node tests assert corpus/gold integrity, all four provider types, reproducible idealized oracle, rank sensitivity, wrong-project/stale hallucination penalties, restricted-source exposure (including abstentions), invalid predictions failing closed, and deterministic output ordering. These tests run through existing `npm test` → `test:web` in GitHub's Windows product CI; the storage matrix is a separate path-filtered workflow and must be reported as **NOT_RUN** if not triggered.

No production code path, archive, database schema, provider permission, remote API, native host, or secret is modified by MP-10A. License of the synthetic fixtures follows the repository's first-party MIT terms.
