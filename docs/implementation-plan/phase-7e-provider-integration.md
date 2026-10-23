# MP-7E — cross-provider capability and privacy regression

**Task:** MP-7E · Cloud-only · GitHub issue #74 · lock `capture-integration`. Prerequisites MP-7B, MP-7C, MP-7D are merged. The matrix below is based on shipped adapter descriptors and native-import implementation, not conjectured capabilities of provider websites.

## Capability and authority matrix

| Provider | Entry | Saved identity | Manual loaded DOM | Manual history scroll | Automatic loaded-only | Raw evidence |
| --- | --- | --- | --- | --- | --- | --- |
| ChatGPT | activeTab on official `chatgpt.com` | saved UUID path or project UUID route | yes | explicit bounded job | opt-in, ChatGPT-only | immutable Capture V1 JSONL |
| Gemini | activeTab on `gemini.google.com` | `/app/<id>` or `/u/<n>/app/<id>` | yes | no | no | immutable Capture V1 JSONL |
| Claude | activeTab on `claude.ai` | saved chat UUID or project/chat UUID | yes | no | no | immutable Capture V1 JSONL |
| Generic | unlocked-owner selected file, **not browser** | SHA-256 of raw source under `mindplace-import://generic/` | no | no | no | exact `.raw` UTF-8 source + derived immutable Capture V1 JSONL |

Generic accepts only bounded, explicitly structured JSON, Markdown or HTML transcripts. It is not an arbitrary website scraper and cannot claim a native DOM capture. `PROVIDER_CAPABILITIES` in `extension/providers/capability-matrix.mjs` projects the bundled browser descriptors and a distinct generic import entry. The object is informational; enforcement stays in the adapter URL parser, per-tab worker and popup checks, native capture host, archive core and explicit Tauri owner import gate. No runtime permission is derived from this matrix.

## Privacy and regression invariants

1. **No privilege expansion:** Manifest v3 has only activeTab, scripting, nativeMessaging and storage. No unconditional host permissions or manifest content scripts. `https://chatgpt.com/*` is the **only** optional persistent origin and only for explicit ChatGPT automatic opt-in. Claude/Gemini manual capture uses activeTab and does not install observers.
2. **Identity fail closed:** exact official HTTPS origin, supported saved route, no credentials, browser top-frame admission where implemented; worker checks the tab again before submit. Browser V1 must match the selected provider, source URL, conversation ID and capture mode. Native independently rejects cross-provider spoofing and invalid mode/identity.
3. **Private data:** proven Temporary Chat, private/incognito modes are excluded. No browsing history or other tabs are imported. DOM readers only use visible conversation turns; never read sidebar or hidden thinking. A provider UI change must fail closed when no recognized turns remain rather than fabricate an archive.
4. **Observed truth only:** `completeness: partial` and warnings always accompany DOM and generic captures. Missing authored timestamps, models, attachments, unseen branches, tool actions and hidden chain-of-thought are not inferred. Claude preserves exposed branch/regeneration IDs; Gemini preserves observed revision differences.
5. **Immutable provenance:** capture revisions use content-addressed JSONL; later partial windows and regenerations cannot rewrite an earlier snapshot. Generic source bytes are stored first, even if parsing is rejected. No provider APIs, remote media fetches or cross-origin scraping.
6. **Synthetic qualification only:** node/JSDOM and native framed-host/archive regression test fixtures contain no signed-in or personally identifying provider data. Cloud CI does not certify real DOM variants or installed Windows browser accounts; MP-7L1/7L2 and MP-7U1/7U2 remain explicit local/owner gates.

## Qualification and handoff

- `test/provider-integration.test.mjs` pins the matrix, manifest permissions, serializer admission and fail-closed hostile cases across the bundled ChatGPT, Claude and Gemini DOM readers.
- `crates/capture-host/tests/provider_integration.rs` exercises the real native host with multiple provider captures and the archive with a generic source, including identity isolation and immutable revisions.
- Existing single-provider tests and generic source-byte tests remain required; product Windows and Phase 1E 10K/100K/1M CI must be checked **on the final PR SHA**.
- Results belong to PR #TBD and [Notion MP-7E](https://app.notion.com/p/400c4a6aaf7381749e52ee14b5dd482a). Mark CI as NOT_RUN while pending; mark cloud complete only after truthful self-review and checks. No release/device qualification is implied.
