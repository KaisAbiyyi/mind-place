# MP-10B — Cloud-only retrieval/ranking bakeoff and cost tuning

**Claim:** ChatGPT GPT-6 Cloud mp10b-20261024-1040-wib; lock benchmark-retrieval.
**Prerequisites:** MP-10A, MP-6E, MP-5D DONE.

## Measurement boundary

The Rust harness crates/index-core/tests/continuity_bakeoff.rs constructs a temporary derived SQLite index using only 24 public synthetic MP-10A source records and calls actual production IndexStore::retrieve (SQLite FTS5). It never opens a private archive, real approved root or signed-in provider, and never accesses local user files.

IMPORTANT: test-only direct derived-index seeding bypasses normal StateStore/ArchiveStore source registration and should NOT be presented as a production ingestion route.

| Strategy | Measured system | Deployment status |
| --- | --- | --- |
| lexical_fts5 | Real IndexStore::retrieve, bounded question-derived FTS5 query terms | Actual search engine, temporary fixture only |
| structured_fts5 | Same real FTS5 API with native source_ids filter, selected by fixture project metadata | Real search engine; project-to-source adapter is synthetic |
| project_graph_proxy | Synthetic project-to-source adjacency, newest-first; NOT GraphNeighborhoodRequest | Proxy, NOT production graph |
| hybrid_fts5_project_proxy | Reciprocal rank fusion (RRF k=60) of structured FTS5 and project-graph proxy | Proxy, NOT production hybrid |

The restricted source e20 is excluded before indexing and from proxy retrieval. Graph/project membership never grants authorization; this synthetic test cannot certify real revocation races.

## Reproduce — only inside a cloud CI runner

~~~sh
node --test test/retrieval-bakeoff.test.mjs
cargo test -p mind-place-index --test continuity_bakeoff -- --nocapture
node scripts/benchmark-retrieval.mjs --output mp10b-report.json
~~~

The isolated GitHub workflow at .github/workflows/mp10b-bakeoff.yml runs these tests and publishes mp10b-report.json as an evidence artifact. The runner reuses the unmodified MP-10A scorer to verify every predicted source ID, rank and scope, plus fixture SHA256 and environment metadata. The CI report records exact HEAD via MP10B_HEAD_SHA when provided.

- Metrics: per-case and macro recall@5, precision@5, MRR, nDCG@5, strict restricted evidence leak counts, warm per-query measured elapsed ms, top-five estimated context tokens.
- No gold relevance, expected claims or abstention labels enter retrieval candidate creation or ranking. Gold is consumed only by the independent scorer, never the retriever.
- The runner deliberately emits no generated claims and does not use gold abstention to manufacture correct answers. Hence answer support, natural-language accuracy and abstention remain NOT qualified.
- Latency excludes temp index creation, first-run import, authorization, UI, package startup and live Windows resources. Context cost approximates chars/4, not actual LLM billing.
- Case-level misses must be inspected. A CI-green harness is not equivalent to a qualified accuracy target. No quality results may be claimed before the GitHub Actions artifact is examined.
- The lexical tuning candidate is chosen only from two real-FTS5 variants, by graded nDCG then smaller token estimate. It is an *experimental candidate*, NOT a production rollout authorization.

## Exit and limitations

A full production-real graph/structured/hybrid test still needs a normal temporary authorized ArchiveStore/StateStore projection and the actual graph API; these proxy numbers cannot justify enabling embeddings, vectors or graph default routing. The 24-document set cannot qualify MP-10C's 1M-chunk/100K-file scale. Owner-device/provider and signed-in account validation belongs to later 10L/10U gates.

Before Done: exact-head product CI + dedicated bakeoff CI PASS, non-empty scorer report with zero privacy exposures, implementor self-review of precise SHA/diff and constraints, a documented cost/quality choice, merge readiness and Notion/GitHub evidence. Until all are proven, leave this task Review.

**Status:** implementation submitted on a feature branch; CI/review and final acceptance tracked in PR + Notion, not asserted here.
