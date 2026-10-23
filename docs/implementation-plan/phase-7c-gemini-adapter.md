# MP-7C — Gemini capture/import adapter and immutable revisions

Task: MP-7C. Owner: ChatGPT Cloud / `mp7c-20261023-2047-wib-chatgpt`.
Predecessor MP-7A merged in PR #61. Exclusive lock: `capture-gemini`.

## Cloud implementation

The static browser registry dispatches a Gemini-specific serialized capture for explicit
manual **loaded-only** saves from HTTPS `gemini.google.com/app/<id>` and
`gemini.google.com/u/<account>/app/<id>`. Path identities preserve case.
The adapter scopes extraction to rendered `user-query` / `model-response` turns
under a visible main conversation surface. No provider APIs, scrolling, remote
media fetches, hidden thinking, automatic observer, or broad host permissions
are introduced. Missing source timestamps/models/attachments are not invented.
Unsupported routes, private/temporary markers, navigation and DOM identity mismatch
fail closed. The popup redirects checked history mode to loaded-only for Gemini.

The browser produces the existing capture schema v1 with `manual-dom`/partial
provenance, explicit warnings and bounded text/payload. The native host independently
enforces provider/origin/route/identity and only accepts Gemini manual partial
captures. Immutable content-addressed JSONL snapshots preserve old revisions
when an answer regenerates and de-duplicate identical observations. The derived
index assigns a provider-separated source namespace so ChatGPT source identifiers
do not change, and historical Gemini revisions remain searchable/retrievable.

## Tests and qualification

Synthetic JSDOM serialization, strict URL/DOM/visibility/privacy cases,
regenerated-turn evidence, framed native-host duplicate/revision/spoof rejection,
and cross-provider derived-index search must pass on exact PR HEAD. Existing
ChatGPT regression tests must stay green. Hosted CI is required before merge.
This work does NOT qualify real Gemini DOM variants, provider account state,
Windows extension/native installation or owner acceptance; those belong to
MP-7L2 and MP-7U2 after MP-7E. Gemini automatic saving and historical scrolling
are intentionally disabled until separately specified and safely qualified.
