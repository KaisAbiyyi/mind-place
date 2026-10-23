# MP-7A — browser provider adapter contract

Task: MP-7A; claim v1 `mp7a-20261023-1941-wib-codex-cloud`; exclusive logical
resource `capture-contract`; base `008a5580c14c4dad103e58ee59519351cd1fb191`.
Dependency MP-4U is owner PASS / Done in the live execution ledger. The old
current-phase pointer predates that acceptance. GitHub issue #58 records the claim.

## Design and scope

Manual and automatic coordinators previously imported ChatGPT's capture function
and duplicated provider/identity/mode checks. Both now dispatch through the same
statically bundled adapter registry and normalized admission function. ChatGPT's
serialized DOM implementation moves intact to `extension/providers/chatgpt.mjs`;
`extension/capture.mjs` remains a compatibility re-export. Its DOM selection,
privacy checks, manual history state machine and loaded-only automatic path are
unchanged.

An envelope around the existing capture is sufficient for this unit. Rewriting
the DOM collector or adding a new persistent event schema would put current
archive identity and privacy behavior at unnecessary risk. Adapters for Claude,
Gemini and generic imports remain separate dependent units. This is a browser
capture contract, not a new agent-session import format or import UI.

## Adapter interface, version 1

The JSDoc `CaptureAdapter` in `extension/providers/contract.mjs` defines:

- `contractVersion: 1`, persisted `id`, and an exact HTTPS `pageOrigins` allowlist;
- pure `parseConversationUrl(url)` returning `{conversationId, sourceUrl}` or null;
- explicit `manualDom`, `manualHistory`, `automaticLoadedDom` capabilities;
- self-contained `capture(options)`, `control(operationId, action)` and
  `finish(operationId, status)` functions suitable for `executeScript` serialization;
- optional automatic observer metadata: a unique script ID, permission match
  patterns and bundled observer file. This metadata grants no browser authority.

`resolveCaptureAdapter` requires an allowed origin and a valid conversation route.
`captureAdapterForPage` also permits the provider homepage solely so the existing
opt-in observer can follow SPA navigation. Capture admission still requires a
real conversation identity. URLs with credentials, non-HTTPS origins, lookalike
hosts and nonstandard ports cannot select an adapter. Registry entries come only
from bundled modules; messages cannot install adapters or broaden permissions.

Only ChatGPT is registered. Its page origin stays `https://chatgpt.com` and its
optional permission stays `https://chatgpt.com/*`. Existing dynamic observer
registration IDs are retained, including opt-out removal. New providers need
explicit permission, observer, native validation and qualification work before
registration; a descriptor alone does not establish provider support.

## Normalized evidence and trust boundary

The normalized payload remains the existing archive `Capture` schema version 1
and native protocol 1. Capture header: provider, conversation ID, exact source
URL, title, observation time, method, completeness, warnings, optional provider
creation/update times, and ordered messages. Each message preserves ID, role,
text/format, optional source timestamp/model and attachment references or bounded
inline image data. Missing source times/models remain absent. No branch, tool
event or hidden reasoning is invented. Adapter version is not injected into raw
captures, since doing so would alter V1 evidence and hashes.

Shared browser admission checks exact selected provider/identity/URL/mode,
schema version, partial completeness and warnings, timestamp shape, message
roles/formats, unique IDs, and existing byte/count limits. Browser attachments
cannot supply archive-local asset paths. It returns a boolean without rewriting
or normalizing the observation. The native host remains an independent authority
for archive validation, image sniffing, safe paths, immutable writes and receipts.
This browser check does not replace native validation or grant file access.

Both coordinators retain private-tab rejection, current-tab URL checks before
submission, job cancellation and acknowledgement uncertainty behavior. Automatic
saving remains off by default, permission-gated, loaded-only, debounced and
generation-fenced on opt-out. The observer stays ChatGPT-specific and never
scrolls; manual history loading remains explicit, bounded and cancellable.

No archive migration, identity/hash algorithm, source registration, state schema,
retrieval, graph or memory-write code is changed. Future browser adapters must
integrate native provider validation as part of their bounded tasks.

## Verification

`test/fixtures/capture/chatgpt-v1.html` contains synthetic text and public example
URLs. Its expected JSON was captured with the pre-refactor function at the base
SHA and a fixed observation timestamp. The adapter test serializes the production
function into an isolated DOM and compares the entire payload to that fixture.
Existing shell, virtualized-scroll, Stop/Cancel, Temporary Chat and automatic
navigation/streaming tests exercise the same implementation via the compatibility
entry point. Coordinator tests reject malformed normalized captures before a
native submission.

Native `adapter_regression.rs` consumes the same fixture through real framed
`serve_one`, pins the existing conversation and snapshot hashes, verifies exact
readback, time-independent deduplication, immutable prior JSONL bytes after a
smaller automatic window, and native rejection of spoofed provider/identity/mode.
No private corpus or live account is used. Required repository/typecheck/web/Rust
checks and exact-head Windows CI are reported on the PR; do not infer results
from this design document. Hosted CI does not substitute for later Phase 7
provider/device/owner qualification.
