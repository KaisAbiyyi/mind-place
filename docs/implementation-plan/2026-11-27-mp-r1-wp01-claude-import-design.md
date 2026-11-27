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

- `users.json` (account name, e-mail, phone) is **never** a candidate member and is never staged. It is measured (hashed) as part of the source manifest like every other file, nothing more.
- Both kinds are top-level JSON arrays read through the same bounded stream (`stream_bounded_top_level_array_spanned`, 64 MiB per record) as ChatGPT, so every record is bound to `(member, offset, length, sha256)`.
- Explicit members: a `.json` entry of the manifest; its kind comes from its file name (`projects.json` → projects, otherwise conversations). The Tauri layer now maps each member path to a kind instead of assuming one kind per provider.

## 3. Projection rules (never invent)

- **Identity.** `uuid` must be a UUID; otherwise raw-only. Duplicates in one job (across shards, case-insensitive) get a deterministic disambiguated UUID and the `mindplace-import://claude-export/<uuid>` source URL, exactly like ChatGPT. The first occurrence uses `https://claude.ai/chat/<uuid>`.
- **Capture method** `export-import`, provider `claude`. `validate_historical_capture(.., "claude")` accepts only those two URL shapes and requires the identity to match. The stored-capture rule now accepts `export-import` for Claude; the ordinary `save` path still refuses it, and the keep-prior rule (an export never displaces a live capture) already covers `export-import`.
- **Order and branches.** Messages are projected in `chat_messages` order. If any message has `parent_message_uuid`, the links must form exactly one chain in array order (one root whose parent is absent, the null-UUID sentinel, or not a message of the conversation; each next message's parent is the previous message). Anything else (a fork, a cycle, an out-of-order link, a duplicate message uuid) is **refused**: the record stays raw-only, because the active branch cannot be chosen without guessing. Without any parent links the projection is `partial` with the warning that branch structure is not proved by the export.
- **Content.** With `content` present, `text` blocks are projected; every other block type (`thinking`, `tool_use`, `tool_result`, images, unknown) is not shown and makes the conversation `partial` with a bounded "Completeness not proved" warning; the block stays in raw evidence. `thinking` text is never shown as message text. Without `content`, `text` is projected. A message with neither is a gap.
- **Roles.** `human` → `user`, `assistant` → `assistant`; anything else is shown as `tool` and is a gap.
- **Model.** Always `None`: the export does not carry it.
- **Timestamps.** Message `authored_at` is the message's own `created_at` when it parses as RFC 3339, else `None` plus a gap warning. Conversation created/updated come from the record. `captured_at` uses created, then updated, then import time with an explicit warning (same rule as ChatGPT).
- **Attachments.** Each `attachments[]` and `files[]` entry becomes metadata only: name (bounded), MIME guessed from `file_type`/extension or `application/octet-stream`, no `data_url`, no `asset_path`, an `unavailable_reason` saying the file body is not in the export (extracted text stays in raw evidence). Attachments do not lower completeness.
- **Title.** `name` trimmed and clipped to 120 characters, else "Claude Export Conversation".
- **Raw metadata** (opaque `metadata_json`): message count, whether parent links exist, whether a project uuid exists, the project uuid if it is a UUID. No content.

## 4. Raw-first path

Unchanged from RAW-01: measure → stage (raw bytes + validated projection) → verify root hash → publish raw blob, evidence, projection, disposition link, done marker. The importer version in the job key becomes `mp-import-raw-01/2` so a spool staged by the previous version (which had no Claude kinds) is never reused.

## 5. Sanitizer and fixtures

The MP-IMPORT-SANITIZE-01 tool learns the Claude vocabulary (default-deny stays): `uuid`-style id keys (`uuid`, `parent_message_uuid`, `file_uuid`, `project_uuid`), the `sender` enum, Claude content block types, the `claude.ai` host, `_at` timestamp keys, and detection of a Claude export in the report label. Fixtures:

- `test/fixtures/import-sanitizer/parser-compat/claude-{synthetic,sanitized}.json`, generated like the other two (invented data, fixed key), with a drift test.
- Rust tests prove sanitized and synthetic files parse to the same shapes for all three providers, and an end-to-end zip import (folder and zip) for Claude through `import_authorized_source`.

## 6. UI copy (gap protocol)

The signed-off guide's Import dialog lists ChatGPT, Gemini (Takeout) and a transcript file; Claude is not shown. Gap ledger entry:

1. Job: "I want to add the conversations from my Claude export to my archive."
2. Classification: Add, same destination (Archive → Import, kind list). No new destination.
3. Pattern: one more row in the existing kind list (title + one-line hint), same tokens.
4. Copy through i18n, EN + ID together: `import.kind.claude`, `import.kind.claudeHint`; the existing subtitle and choose-file strings name Claude too.
5. Screenshots: **NOT_RUN** — the Import dialog itself is not built yet (it lands with the picker after MP-UI2-06), so only catalog strings change here.

## 7. Local runbook

`docs/runbooks/MP-R1-WP01-import-trio-local.md`: disposable archive, real ChatGPT/Gemini/Claude exports, inspect → import → re-import (idempotent) → receipt checks, optional sanitizer pass to share structure. Result is owner-reported `PASS`/`FAIL`/`PARTIAL`/`BLOCKED`; until then the local gate is `NOT_RUN`.

## 8. Residual risks

1. Real Claude export layout unverified (keys, sharding, presence of `parent_message_uuid`, extra files such as memories). The explicit member list is the escape hatch; unexpected keys stay in raw evidence.
2. Most real Claude conversations will probably be `partial` (no parent links, tool/thinking blocks). That is the honest outcome, not a defect.
3. `projects.json` documents (`docs[].content`) are preserved only as raw evidence, not shown in the reader.
4. Windows-native behaviour (antivirus sharing violations, long paths) stays NOT_RUN until the runbook.
