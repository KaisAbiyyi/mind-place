# MP Import Second Correction — cloud implementation handoff


> **Amended 2026-11-11 by MP-IMPORT-FIX-01 (see [2026-11-11-mp-import-fix-01.md](2026-11-11-mp-import-fix-01.md)).** This branch provides **parsing and capture-only primitives** for ChatGPT exports and Gemini Takeout cards. It does **not** include raw-evidence persistence, independent source-manifest measurement, zip handling, Tauri commands or UI (MP-IMPORT-RAW-01 and later cards). It is **not** true that every historical write requires `HistoricalImportAuthorization`: takeout/export captures can be persisted only through `save_authorized_historical_import`, while the legacy-import and generic-import migration paths are documented exceptions listed in the FIX-01 handoff. Statements below that say otherwise are superseded.
Status: **READY FOR INDEPENDENT RE-REVIEW**. This is neither acceptance nor merge/release authorization.

- Repository: `KaisAbiyyi/mind-place`.
- Task: `MP-IMPORT-SECOND-CORRECTION`, P1, owner-assigned bounded corrective implementation.
- Executor: Codex, cloud Linux x86_64, `/workspace/mind-place`; synthetic fixtures only.
- Branch: `feat/mp-import-core-01`; no new PR or remote branch.
- Required and actual starting HEAD: `13f3ba59843bed4bc06ad5fa00d7cfc7c48c19cc`.
- Starting tree: `3c8580398340343348f80527b3d20c977358c6e9`.
- Implementation commit and remote checkpoint: `aa505bb858f196129a2b4d97e82a215664c00384`.
- Implementation tree: `3b8087a7da094bc02f96eb9ccc3545425ce10da7`.
- The following documentation commit adds only this handoff and its machine-readable evidence. Its final SHA/tree are supplied in the chat handoff and can be obtained with `git rev-parse HEAD HEAD^{tree}`.
- Existing commits are preserved as ancestors. No main merge, release, production data, Windows files, vault integration, or P0 branch/PR changes.
- Claim: the explicit owner assignment serialized this correction. No global claim-controller generation or wider resource exclusivity is asserted. Logical resources: historical archive write authorization, ChatGPT export projection, Gemini Takeout projection.
- Commit attribution: owner noreply identity verified against GitHub account ID `93252311`; actual executor is Codex. No attribution exception.

## Corrections

| Defect | Result |
| --- | --- |
| 1. Unauthenticated historical writes | The old two-argument `save_historical_import` is a rejecting compatibility shim. `save_authorized_historical_import` requires `HistoricalImportAuthorization`: owner gate, unlock lease, owner-reauthenticated import capability, independently established source inventory, and expected job. The gate's persisted config must resolve to the archive profile's sibling `security-v1.json`. Provider/job/source/manifest, gate instance, epoch, expiry and revocation are checked before acquiring the archive lock and again at publication. |
| 2. Nonreciprocal ChatGPT DAG | Validate mapping keys/node IDs, reciprocal edges, unique child references, one root, acyclicity and full root connectivity before branch selection. Inconsistent DAGs become raw-only with an explicit reason. Graph checking is iterative and linear in nodes plus edges. |
| 3. Duplicate ChatGPT message IDs | Validate the final reader projection using `validate_historical_capture`. Duplicate projected IDs produce raw-only evidence with an explicit duplicate-identity reason. Duplicate IDs in unselected alternatives remain in raw evidence without invalidating an otherwise valid selected projection. |
| 4. Gemini card boundaries | HTML5 tokenization recognizes class tokens, all valid attribute quoting, attribute order and case-insensitive HTML tag/attribute names. Matched div boundaries select exact original card byte slices; distinct occurrences retain independent identities and occurrence indices. Comments and script text cannot manufacture card boundaries. |
| 5. Fabricated Gemini user turns | Prompt labels must occur as direct block text outside response/quoted/code contexts. Further user turns require distinct peer `content-cell` containers. Nested or repeated flat labels do not create users; ambiguous labels remain in raw evidence and force a partial projection. Assistant paragraphs preserve literal `Prompted` text. |
| 6. Truncated Gemini URLs | Parse and validate the entire candidate and compare its original spelling with the exact canonical `https://gemini.google.com/app/<id>` route. Reject traversal, encoded traversal/separators, extra suffixes, query/fragment, credentials and noncanonical authorities. Assemble visible text across inline markup before validating textual candidates. Invalid URLs remain untouched in raw evidence and use the content-addressed Canvas identity. |

`SecurityGate::with_verified_import_capability_bound` holds the existing runtime mutex through a bounded write action, so lock/revocation serialize with publication. Actions must not call back into the same gate. Existing capability verification delegates to the same checks; credential backoff, registration, gate-instance binding and revocation protections remain in place.

The legacy archive vault-marker fence remains active, including for already-open handles. The new writer supports the existing plaintext archive profile layout; it does not route imports into encrypted vault storage. Source inventory establishment and manifest calculation remain the import coordinator's responsibility; they are not inferred from capability fields.

The only new external dependency is pinned `html5gum = 0.8.0` (MIT, default features disabled, no transitive dependencies). Archive core also gains the existing workspace security crate as a path dependency. `Cargo.lock` changes only for these additions; existing package resolutions remain unchanged.

## Six independent negative regressions

Each fixture is in `crates/archive-core/tests/fixtures/import-second-correction/`; each test is in `crates/archive-core/tests/import_second_correction.rs`.

| Fixture | Regression test |
| --- | --- |
| `unauthenticated-historical.json` | `unauthenticated_historical_write_is_rejected_without_side_effects` |
| `chatgpt-nonreciprocal.json` | `nonreciprocal_selected_leaf_is_not_complete` |
| `chatgpt-duplicate-message.json` | `duplicate_message_ids_never_emit_an_invalid_capture` |
| `gemini-single-quoted-cards.html` | `single_quoted_html_cards_remain_separate_occurrences` |
| `gemini-prompted-in-response.html` | `prompted_inside_assistant_text_cannot_fabricate_a_user_turn` |
| `gemini-traversal-url.html` | `traversal_suffix_cannot_supply_an_official_gemini_identity` |

Before implementation, the initial six tests were run against the required starting implementation with `cargo test --locked -p mind-place-archive --test import_second_correction`: **0 passed, 6 failed, exit 101**. Each failed on its reported defective behavior. The final integration suite contains **18 passing tests**, including these six and additional positive/negative boundary checks.

Supplemental tests cover owner-authorized write/read/restart/deduplication; incorrect provider/job/source/manifest; foreign gate/profile; revoked, stale and expired authority; vault-marker rejection; other inconsistent graph edges and node IDs; valid alternate branches; HTML quoting/case/comments/raw-text/entities; response-only/quoted text; complete canonical URLs; inline-hidden suffixes; and ambiguous nested/flat labels. A new security test confirms the write action is fenced and a revoked action never runs.

The read-only review used the `superpowers:requesting-code-review` skill. It found nested/flat assistant-label ambiguity and inline URL-prefix acceptance; focused regressions were observed failing and then passing after corrections. Its final verdict found no remaining actionable defect within this bounded scope. A separate independent acceptance review is still required.

## Verification

Environment: Linux x86_64, Rust `1.90.0`, Cargo `1.90.0`. Commands used the committed lockfile and default crate features. Final runs tested the implementation contents committed as `aa505bb858f196129a2b4d97e82a215664c00384`; later changes are documentation/evidence only.

| Exact command | Passed | Failed | Ignored | Exit |
| --- | ---: | ---: | ---: | ---: |
| `cargo test --locked -p mind-place-security` | 45 | 0 | 0 | 0 |
| `cargo test --locked -p mind-place-archive` | 88 | 0 | 0 | 0 |
| `cargo test --locked -p mind-place-index` | 61 | 0 | 1 | 0 |
| `cargo fmt --all -- --check` | N/A | N/A | N/A | 0 |

Archive breakdown: 28 unit, 19 archive, 5 generic, 18 second-correction, 7 legacy, 2 registration, 9 validation-boundary tests. All three crates have zero doc-tests. The ignored index test is the existing `qualifies_archive_scale` test, marked `explicit Windows CI/storage qualification only`.

Positive and backwards-compatibility evidence includes all legacy and generic tests, unchanged stored-reader validation, ordinary live capture behavior, authorized historical round trips, existing ChatGPT active-branch/raw/attachment fixtures, and Gemini prompt-only/response-only/timestamp/nested-markup fixtures. The multi-turn Gemini positive now uses explicit peer `content-cell` wrappers. The previous ambiguous flat shape is retained as a negative regression requiring no inferred later users and partial completeness.

Additional checks: `node scripts/verify-repo.mjs` and `git diff --check`, both exit 0. No frontend build or full-workspace/npm test claim is made for this bounded Rust correction. Existing unused-mut/import/dead-field compiler warnings remain in security, state and index sources outside this correction.

Machine-readable counts, suite breakdowns, regression names and final-run output digests are in `evidence/mp-import-second-correction-20261105.json`.

## Remaining limits and next reviewer

1. **Windows SQLCipher qualification: NOT RUN.** Default-feature Linux tests do not establish Windows SQLCipher, installed-app, hostile-account, real-provider, or production-vault qualification. Feature-gated index tests report zero tests under this run; the scale gate remains ignored.
2. Gemini HTML extraction uses an HTML5 tokenizer plus conservative Takeout structure rules, not a browser DOM tree builder. Arbitrary malformed/misnested markup is not qualified. All source card bytes remain evidence; the input limit remains 64 MiB.
3. Repeated flat labels cannot distinguish a real later prompt from assistant prose. They remain raw evidence with a partial projection. Additional projected turns require peer provider prompt cells. Entity-encoded whitespace before a label can conservatively yield an assistant-only partial projection, with the original label retained raw.
4. Duplicate ChatGPT IDs on a projected path are raw-only rather than guessed/renamed. Any inconsistent graph is raw-only, even when a narrower branch might be recoverable. Existing stored readers remain compatible.
5. Authorized plaintext historical callers must migrate to the new authorization-bearing API. The former unauthenticated method always rejects. The profile must have its genuine sibling `security-v1.json`; encrypted vault import integration is a separate task.

Next: independently review the final feature-branch SHA against the starting SHA, rerun the six fixtures and mandated commands, and assess raw fidelity, conservative completeness, and owner-write authority. No merge or release follows automatically. Local qualification requires a separately scoped owner instruction; no local gate is marked PASS here. Recover from the pushed feature-branch checkpoint and verify its SHA before continuing. This task makes no changes to P0 foundation branches or PRs.
