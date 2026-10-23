# MP-7B — Claude manual loaded-DOM adapter

- Cloud unit: MP-7B, claim v1, ChatGPT GPT-6 Cloud mp7b-20261023-2047-wib, lock capture-claude, issue #63.
- Base: 70fce6f862a4f92d5a2714326242f97a49472411; predecessor MP-7A Done.

## Security and native admission

The explicit one-shot browser capture uses activeTab and scripting only; no new unconditional origin permission, content script, provider API, scroll or automatic observer. Only official saved conversation URLs are admitted: claude.ai/chat/UUID and claude.ai/project/UUID/chat/UUID. Unknown routes, query strings, fragments, ports, credentials and lookalike hosts are rejected. Incognito is blocked by worker/popup; positively active temporary/private UI markers are rejected by the reader.

The native archive checks the Claude provider, exact URL-to-conversation UUID and manual-dom/partial mode independent of the browser. No migration of V1 raw JSONL, protocol or ChatGPT identities. Previously stored revisions remain immutable. Explicit rendered message IDs, branch IDs and regeneration counters are encoded in the existing V1 message identity, with observed model and timestamp only when a message exposes them. Positional fallback IDs warn that source identity is not verified. Unseen branches, private data, tool internals, attachments or hidden reasoning are never invented or scraped.

## Limitations and acceptance

This is an explicit manual Claude loaded-DOM capture-to-archive path, not a Claude export-file parser (generic JSON/HTML/Markdown imports belong to MP-7D). It does not support automatic saving or history scrolling; those would need separate privacy qualification. DOM role selectors and branch attributes are synthetic-test assumptions, not live-provider evidence. The required MP-7E integration suite, MP-7L1 real-account/browser qualification and MP-7U1 owner acceptance remain distinct gates. CI/test/self-review verdicts belong to the linked PR and Notion evidence fields.
