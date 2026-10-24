# MP-8A — Knowledge OS shell, navigation and large-list data contract

**Scope:** Cloud 8A. Baseline `main` d6a690b31d18d3a30cba5165493c5dae1cb5a982. Task [#76](https://github.com/KaisAbiyyi/mind-place/issues/76), implementation [PR #78](https://github.com/KaisAbiyyi/mind-place/pull/78). Owner ChatGPT GPT-6 Cloud, session `mp8a-20261024-0954-wib`.

## Outcome and boundaries

Phase 8A establishes a consistent, keyboard-accessible workspace entry point and reusable data primitives. It does **not** claim to implement Phase 8B–8E panels, a new backend search endpoint, filesystem authority, graph mutation, or source-of-truth changes.

The complete product navigation contract is `WORKSPACES` in `src/knowledge-os.ts`:

| Workspace | UI status / intended owner |
| --- | --- |
| Conversations | Available now; existing archive reader |
| Memories | Available now; provenance/conflict inspector from MP-5D |
| Documents | Available now; approved-root-bound document search |
| Connections | Available now; legacy capture and MCP setup |
| Projects, People, Topics | Planned MP-8C; **not clickable** now |
| Hive graph | Planned MP-8E; **not clickable** now |
| Health | Planned MP-8D; **not clickable** now |

Conversations preserve existing transcript/snapshot/copy/export behavior. Memories and Documents call their existing implementations; Connections invokes existing setup. The reader remains under `mountApplication`'s unlock-epoch lifetime. Workspace selection is **not** an authorization grant, folder approval or source of truth.

## Layout and visual system

- Semantic tokens live in `src/knowledge-os.css`: spacing (4/8/12/16/24), type scale (11/12/14/19), focus visibility, surface/interactive states, a fixed 76px virtual row density.
- Keyboard-operable native buttons inside labeled workspace navigation. Exactly one active workspace via `aria-current=page`. Unbuilt routes do not render as dead controls.
- Wide shell retains scrollable sidebar and transcript. On narrow WebView2, navigation becomes horizontally scrollable tabs; small conversation sets retain legacy horizontally scrollable cards; large sets use vertical virtualization.
- No visual overhaul to open Settings/Documents PR #45/#46, no new global markup injection, no external fonts/network calls. Contrast and subjective installed Windows UX still need product acceptance later (8L/8U).
- Loading, empty, retry and error content stays with the current reader; further feature work should reuse these states rather than silently showing a blank screen.

## Large-data behavior

`getVirtualWindow` is pure, bounded by row height, viewport and overscan; for large lists the DOM only contains visible rows plus overscan/spacers, not every saved conversation. Native `list_conversations` still loads matching summary data into the UI at once; **backend pagination is not yet implemented and must not be claimed**. The Phase 8B follow-up should add native cursor support for archive/FTS/graph sources before presenting truly million-item remote browsing.

`createKnowledgeCollection` specifies a generic cursor-based loader for later panels:

- explicit `{cursor, limit, signal}` request; 1–200 page limit;
- observable `idle/loading/ready/error` state and immutable snapshots;
- a single in-flight load for duplicate requests;
- cancellation/generation fencing on reset/lock/workspace replacement;
- cursor loop and malformed-page error detection;
- explicit terminal `nextCursor = null`; no implicit collection reads.

Each future workspace owns its own instance and must pass the live native unlock/permission guard into its loader. Stale fetches may not repopulate a destroyed or reset collection. Do not cache source text, credentials or authorization across unlock epochs.

## Acceptance checks

- `test/knowledge-os.test.ts`: 100K range windows, bounds, navigation semantics, stale async response fencing, page duplication/loop retry, large archive DOM count and focus arrow navigation.
- Existing `test/reader.test.ts` remains applicable for transcript sanitization, snapshots, lock/unlock, setup and refresh.
- Required exact-head Windows product CI, repository guard, TypeScript, web tests, Rust tests and desktop build; GitHub runners are **cloud tests**, not installed Windows/device/owner acceptance.
- No state/index migration, no raw capture/archive mutation, no new native command, no change to privilege scope.

## Phase handoff

After MP-8A qualified merge, MP-8B (libraries), MP-8C (memory/entity workspace), MP-8D (retrieval health), MP-8E (graph) can branch in parallel using separate `ui-library`, `ui-memory`, `ui-health`, `ui-graph` locks. Each feature plugs into the registry and paged collection contract; avoid changing the shared shell without coordinating the `ui-shell` lock. Phase 8F is the eventual desktop accessibility/visual integration gate.
