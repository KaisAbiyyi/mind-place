# MP-R1-WP01 — design note: Claude.ai export importer (import trio)

Status: design for the claimed work package. Synthetic fixtures only. Not an acceptance claim; only the owner merges.

- Card: Notion "MP-R1-WP01 · Import trio: ChatGPT, Gemini and Claude exports". Lock: `capture-import`. Gate: C + L.
- Builds on MP-IMPORT-RAW-01 (raw-first publication), MP-IMPORT-APP-01 (zip sources, Tauri commands) and MP-IMPORT-SANITIZE-01 (sanitizer). Absorbs MP-IMPORT-L1 as one local runbook.
- Scope: archive-core importer and job wiring, `src-tauri` provider check, sanitizer vocabulary and fixtures, EN/ID copy, runbook. Out of scope: the native picker (`G6`), the Import dialog UI, encrypted-vault import, asset (file body) ingestion, `users.json` and any other account file.

## 1. Documented layout (unverified)

No real Claude export was read (repository rule). The layout below is the publicly described one and stays **NOT_RUN** until the owner runs the runbook (section 7).

- Settings → Privacy → Export data produces a `.zip` with `conversations.json`, `projects.json` and `users.json` at the root (newer exports may add more files).
- `conversations.json` is a top-level array. A conversation has `uuid`, `name`, `created_at`, `updated_at` (ISO 8601), optionally `summary`, `account`, `project_uuid`, and `chat_messages`.
- A message has `uuid`, `sender` (`human` or `assistant`), `text`, optionally `content` (typed blocks: `text`, `thinking`, `tool_use`, `tool_result`, others), `created_at`, `updated_at`, `attachments` (`file_name`, `file_size`, `file_type`, `extracted_content`) and `files` (`file_name`, sometimes `file_uuid`), and in some exports `parent_message_uuid`.
- The export does **not** name the model of a message. It carries no explicit "current leaf" pointer.
- `projects.json` is a top-level array of project records (`uuid`, `name`, `description`, `prompt_template`, `docs`, timestamps).

## 2. Members and kinds

| Kind (`MemberKind`) | Provider | Member | Envelope domain | Outcome |
| --- | --- | --- | --- | --- |
| `claude-conversations` | `claude` | `conversations.json` or `conversations-*.json`, any depth | `claude-export-conversation` | projection or raw-only |
| `claude-projects` | `claude` | `projects.json`, any depth | `claude-export-project` | always raw-only (a project is not a conversation) |

- `users.json` (account name, e-mail, phone) is **never** a candidate member and is never staged. It is measured (hashed) as part of the source manifest like every other file, nothing more. Review round 1: account files (`users.json`, and ChatGPT's `user.json`, any depth, any case) are also refused when named explicitly, both by `member_extension_allowed`/`MemberKind::for_member` and at the job boundary (`E_MEMBERS`), whatever kind a caller claims.
- Both kinds are top-level JSON arrays read through the same bounded stream (`stream_bounded_top_level_array_spanned`, 64 MiB per record) as ChatGPT, so every record is bound to `(member, offset, length, sha256)`.
- Explicit members: a `.json` entry of the manifest; its kind comes from its file name (`projects.json` → projects, otherwise conversations). The Tauri layer now maps each member path to a kind instead of assuming one kind per provider.

## 3. Projection rules (never invent)

- **Identity.** `uuid` must be a UUID; otherwise raw-only. Duplicates in one job (across shards, case-insensitive) get a deterministic disambiguated UUID and the `mindplace-import://claude-export/<uuid>` source URL, exactly like ChatGPT. The first occurrence uses `https://claude.ai/chat/<uuid>`.
- **Capture method** `export-import`, provider `claude`. `validate_historical_capture(.., "claude")` accepts only those two URL shapes and requires the identity to match. The stored-capture rule now accepts `export-import` for Claude; the ordinary `save` path still refuses it, and the keep-prior rule (an export never displaces a live capture) already covers `export-import`.
- **Order and branches.** Messages are projected in `chat_messages` order. If any message has `parent_message_uuid`, the links must form exactly one chain in array order (each next message's parent is the previous message). The history is proved complete only when the first message's parent is an explicit root (absent, `null`, the `00000000-0000-4000-8000-000000000000` sentinel or the nil UUID). A first parent that is any other value not in the conversation (for example a truncated export) keeps the order but makes the projection `partial` with the warning that earlier history may be missing (review round 1). Anything else (a fork, a cycle, an out-of-order link, a non-string link, a duplicate message uuid) is **refused**: the record stays raw-only, because the active branch cannot be chosen without guessing. Without any parent links the projection is `partial` with the warning that branch structure is not proved by the export.
- **Content.** With `content` present, `text` blocks are projected; every other block type (`thinking`, `tool_use`, `tool_result`, images, unknown) is not shown and makes the conversation `partial` with a bounded "Completeness not proved" warning; the block stays in raw evidence. `thinking` text is never shown as message text. The top-level `text` is projected only when `content` is absent, null or an empty array (review round 1: with only non-text blocks it is not proved to be the shown answer; review round 3: a `content` of any other shape does not fall back to `text` either). A message with neither is a gap.
- **Roles.** `human` → `user`, `assistant` → `assistant`; anything else is shown as `tool` and is a gap.
- **Model.** Always `None`: the export does not carry it.
- **Timestamps.** Message `authored_at` is the message's own `created_at` when it parses as RFC 3339, else `None` plus a gap warning. Conversation created/updated come from the record. `captured_at` uses created, then updated, then import time with an explicit warning (same rule as ChatGPT).
- **Attachments.** Each `attachments[]` and `files[]` entry becomes metadata only: name (bounded), MIME guessed from `file_type`/extension or `application/octet-stream`, no `data_url`, no `asset_path`, an `unavailable_reason` saying the file body is not in the export (extracted text stays in raw evidence). Attachments do not lower completeness.
- **Title.** `name` trimmed and clipped to 120 characters, else "Claude Export Conversation".
- **Raw metadata** (opaque `metadata_json`): message count, whether parent links exist, whether a project uuid exists, the project uuid if it is a UUID. No content.

## 4. Raw-first path

Unchanged from RAW-01: measure → stage (raw bytes + validated projection) → verify root hash → publish raw blob, evidence, projection, disposition link, done marker. The importer version (`mp-import-raw-01/1`) is **not** bumped: ChatGPT and Gemini staging is byte-for-byte unchanged for accepted jobs (they only gain new refusals: an account file, a member whose kind does not match its file, and the provider-shape mismatch), and the job key already frames each member's kind label, so a Claude job can never reuse a spool of another kind. Both Claude kinds read through the same bounded JSON array limits as ChatGPT (`ImportJobLimits::chatgpt_stream`).

## 5. Sanitizer and fixtures

The MP-IMPORT-SANITIZE-01 tool learns the Claude vocabulary (default-deny stays): `uuid`-style id keys (`uuid`, `parent_message_uuid`, `file_uuid`, `project_uuid`), the null-UUID root parent as a kept literal, the `sender` and `file_type` enums, Claude content block types, the `claude.ai` host, ISO 8601 strings under time-like keys (including `_at`) kept with the time of day zeroed, `users.json` as a structural name, and detection of a Claude export (`users.json` beside `conversations.json`) in the report label. The committed ChatGPT and Gemini fixtures are unchanged by this. Fixtures:

- `test/fixtures/import-sanitizer/parser-compat/claude-{synthetic,sanitized}.json` and `claude-projects-{synthetic,sanitized}.json`, generated like the other two (invented data, fixed key), covered by the drift test.
- `import_sanitizer_compat.rs`: sanitized and synthetic files parse to the same shapes for all three providers.
- `import_trio.rs`: all three **sanitized** fixtures import end to end through `import_authorized_source` (counts, raw blob equals the source slice, idempotent re-run, `users.json` never a member).
- `src-tauri` `import_commands` test: a Claude `.zip` through inspect and start (zip path, candidates, explicit-member refusal).

## 6. UI copy (gap protocol)

The signed-off guide's Import dialog lists ChatGPT, Gemini (Takeout) and a transcript file; Claude is not shown. Gap ledger entry:

1. Job: "I want to add the conversations from my Claude export to my archive."
2. Classification: Add, same destination (Archive → Import, kind list). No new destination.
3. Pattern: one more row in the existing kind list (title + one-line hint), same tokens.
4. Copy through i18n, EN + ID together: the kind list keys `import.kind.{chatgpt,claude,gemini}` and their `…Hint` lines (ChatGPT and Gemini copied verbatim from the guide's `imp.k.*` strings, Claude written in the same pattern), plus `import.kind.claudeNote` (the honest-limits line, same voice as the guide's `imp.note`). The existing subtitle and choose-file strings name Claude too. Ledger: `docs/ui/evidence/mp-r1-wp01/design-gap-ledger.md`.
5. Screenshots: **NOT_RUN** — the Import dialog itself is not built yet (it lands with the picker after MP-UI2-06), so only catalog strings change here.

## 7. Local runbook

`docs/qualification/MP-R1-WP01-import-trio-local-runbook.md` with the owner-run harness `crates/archive-core/examples/import_trio_local.rs`: real ChatGPT/Gemini/Claude exports (zip and folder) into a new, empty, disposable directory under a throwaway gate; import, re-import (idempotent), receipt and raw-blob hash checks; prints counts and content-free categories only; optional sanitizer pass to share structure. The in-app path cannot be qualified yet (no `G6` picker, so the commands refuse with `E_IMPORT_SELECTION`); the harness runs the same archive-core job the commands call. Result is owner-reported `PASS`/`FAIL`/`PARTIAL`/`BLOCKED`; until then the local gate is `NOT_RUN`.

## 8. Review round 1 (Antigravity, advisory, PR #219 comment) — decisions

| Item | Decision |
| --- | --- |
| R1 unknown first parent marked `complete` | Fixed: only an explicit root proves completeness; other first parents give `partial` + warning. Test `a_first_parent_outside_the_export_is_not_a_proved_root` |
| R2 explicit `users.json` could be staged | Fixed at both layers (see section 2). Tests in `import_trio.rs` and the Tauri Claude zip test |
| R3 sanitizer `KEPT_STRUCTURAL` missed `users.json` | Fixed (`users?`). No duplicate-entry test: the sanitizer's zip reader and writer already refuse duplicate names, so the path is defense in depth |
| N1 ID pronoun (`kamu` vs `Anda`) | Kept the guide's wording for now; reconcile with the Import dialog PR together with placement-matrix row 22 |
| N2 fallback to top-level `text` | Tightened (section 3, Content) |
| N3 reader label/date for `export-import` | Pre-existing, outside WP01 |

### Review round 2 (independent reviewer Claude Sonnet 5.5 High, read-only, advisory; APPROVE-ADVISORY at `4226f14`)

| Item | Decision |
| --- | --- |
| P2 non-array `content` (and P3 non-array `attachments`/`files`) could still be `complete` | Fixed: a present, non-null, non-array value is a completeness gap ("unexpected shape") |
| P2 an export imported under the other provider is stored with a wrong, immutable provider label | Fixed: while staging, a ChatGPT/Claude conversations member whose records have the other provider's top-level shape (`mapping` vs `chat_messages`) and none of its own fails the job with `E_PROVIDER_MISMATCH` before anything is published. Records of no known shape stay raw-only as before |
| P2 Claude note understated fork handling | Reworded EN+ID: chats with edited messages are kept safely but may not appear yet; ledger updated |
| P3 non-string parent link read as "no links" | Fixed: any present non-null link counts, so a wrong-typed link makes the chain `Broken` (raw-only), as section 3 says |
| P3 sanitizer pseudonymized the nil UUID root | Fixed: nil UUID kept as a structural literal |
| P3 harness category filter could print quoted text | Fixed: output stops at the first quote character |
| P3 `projects.json`-only source; kind not checked against file name | Fixed: the commands refuse a projects-only selection (`E_IMPORT_NO_MEMBERS`); the job refuses a member whose kind is not the one its file name implies (`E_MEMBERS`) |
| P3 no shard-duplicate test | Added (`duplicate_claude_conversations_across_shards_get_distinct_identities`) |

### Review round 3 (independent reviewer Claude Sonnet 5.5 High, read-only, advisory; APPROVE-ADVISORY at `434d413`, P3 only)

| Item | Decision |
| --- | --- |
| P3-1 a non-array `content` still let the top-level `text` be shown | Fixed: only an absent, null or empty `content` falls back to `text` (section 3 and the unit test say so) |
| P3-2 harness: a bare relative run directory was compared against canonical paths | Fixed: the current directory is canonicalized too |
| P3-3 docs: "nothing was written" and "byte-for-byte unchanged" overstated | Fixed: runbook says nothing was published and to use a new run directory; section 4 names the new refusals |
| P3-4 account-file refusal unreachable after the kind check; future error copy | Fixed: the account-file check runs first so the refusal names the real reason; the ledger records that the Import dialog needs human EN/ID copy for `E_PROVIDER_MISMATCH` and `E_IMPORT_NO_MEMBERS` |

## 9. Residual risks

1. Real Claude export layout unverified (keys, sharding, presence of `parent_message_uuid`, extra files such as memories). The explicit member list is the escape hatch; unexpected keys stay in raw evidence.
2. Most real Claude conversations will probably be `partial` (no parent links, tool/thinking blocks). That is the honest outcome, not a defect.
3. `projects.json` documents (`docs[].content`) are preserved only as raw evidence, not shown in the reader.
4. Windows-native behaviour (antivirus sharing violations, long paths) stays NOT_RUN until the runbook.
5. Local-gate follow-up (PR #223): the ChatGPT raw-only reason for a refused DAG now names the violated rules, so its bytes differ from earlier binaries while `IMPORTER_VERSION` stays `mp-import-raw-01/1`. Disposition links are keyed by evidence id and shared across jobs, so a later job over the same source in an archive that already holds an older job would rewrite the link and that older job's deep verification would report it as altered. Acceptable now: no persistent owner archive is imported into yet (in-app import refuses with `E_IMPORT_SELECTION`; the runbook uses a new disposable directory per run). Before in-app import ships, either make disposition links job-scoped or make the deep check tolerate reason-text changes; a version bump alone does not fix it.
6. Local-gate follow-up (PR #224, stacked on #223): the owner's 2026 ChatGPT export lists no `children` (every refusal was `child-not-listed, unreachable-or-cyclic` only). A DAG in which **no node lists any child**, that breaks nothing but those listing rules, and whose parent links form one acyclic rooted tree, is now projected with the warning that its branch structure was derived from parent links. Mixed exports (some parents list children, others leave one out) stay raw-only: they contradict themselves. A repeated `mapping` key is now refused at parse time (raw-only, own reason) instead of silently keeping the last entry. This deliberately reverses one case of the `aa505bb` hardening (an empty `children` list under a valid parent link was raw-only). Completeness is not lowered for the uniform shape: Case A has always read the active branch from `current_node` and `parent` only, so `children` was a redundant cross-check there; Case B (no usable `current_node`) used to walk the listed children and now walks children derived from parent links (identical for a fully valid DAG) and stays `partial`. The same `IMPORTER_VERSION` now projects records an earlier binary kept raw-only; the caveats of item 5 (same-version provenance, shared disposition links) apply, and the runbook's new-directory-per-run rule avoids stale results in the gate.
