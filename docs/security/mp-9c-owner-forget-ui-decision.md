# MP-9C Slice 1: owner-facing forget UI — decision record

Status: **DECIDED (owner, 15 November 2026): Option 1, backend/native-only Slice 1 acceptance.** The UI is moved to the separately governed successor **MP-UI2-FORGET-01**. No UI is implemented in PR #151.

## Owner decision

- The revised MP-9C Slice 1 contract is the existing trusted native preview, one-time confirmation, logical redaction, and pending receipt.
- A separately governed successor, **MP-UI2-FORGET-01**, owns the owner-facing forget workflow. It must follow the approved UI2 design system and the design-gap protocol. It is not implemented in PR #151.
- MP-9C2 (physical erasure) and MP-9C3 (restore protection) remain separate tasks.
- The revised scope is approved, but **merge is not yet authorized**. Required native Windows qualification and independent final acceptance must be completed first. No real-data deletion, self-approval, or release publication.

## Revised MP-9C Slice 1 acceptance criteria

1. **Private-scope isolation.** Admin-gated scope management. Scoped writes need Write + Admin on the same live credential. Global inspection and ranking exclude scoped claims.
2. **Logical redaction enforced on every read path.**
   - Covered paths: archive read/list/snapshots/export/assets, index sync/rebuild, retrieval/inspection/context, the graph, memory, desktop commands and MCP.
   - Read-time filtering applies to stale index generations.
   - There is no existence oracle.
   - Reimport of a forgotten source is refused.
3. **Serialization.**
   - Every vault disclosure is serialized against redaction commits by the shared/exclusive privacy fence.
   - Owner preview/confirm commits are serialized with Lock revocation on the Lock gate.
   - Plaintext legacy state refuses redaction and forget.
4. **Trusted native owner preview and one-time confirmation.**
   - Implemented as `preview_source_forget`, `confirm_source_forget` and `get_source_forget_progress`, vault-only and never exposed to MCP.
   - The owner re-enters the unlock secret for both preview and confirm.
   - The token lives in native memory only, allows a single attempt, and is dropped on Lock.
5. **Logical tombstone with a pending receipt.** `physical_erase_verified = false`, and no bytes are removed.
6. **Restart and SQLCipher persistence** are covered by synthetic encrypted regressions.
7. **Gates before merge:**
   - required native Windows qualification;
   - independent final acceptance review at the exact head;
   - the owner's manual merge.

## Explicitly out of MP-9C

- **MP-UI2-FORGET-01:** the user-facing entry, preview display, explicit confirmation step and progress presentation, under the UI2 design system and the gap protocol.
- **MP-9C2:** physical erasure, crypto-erase, shared-object custody and erasure receipts.
- **MP-9C3:** restore/tombstone anti-rollback and backup-generation disposition.

## Background (kept for the successor)

**The person's job:** "Make Mind Place stop showing and using one saved conversation, after I see what that affects and confirm it once."

The signed-off guide does not show a forget flow. The UI2 backend capability audit (`docs/ui/MP-UI2-01-backend-capability-audit.md`, gap `G5`) predates the native commands. This record does not edit that audit or the UI2 design lock. MP-UI2-FORGET-01 must:
1. choose an existing destination (no new top-level destination);
2. use only catalog patterns and tokens;
3. write EN + ID copy through i18n;
4. keep technical detail (source IDs, fingerprints) behind Details/Info;
5. never send the one-time token to the renderer or to MCP;
6. record its design-gap ledger row with screenshots.

If no catalog pattern fits a destructive confirmation, it stays `BLOCKED-DESIGN` until the owner approves one.

Design-gap ledger row (handed to MP-UI2-FORGET-01):

```text
capability | person's job | class | destination | pattern used | needs owner review
owner source forget (preview + one-time confirm + progress) | stop Mind Place from showing/using one saved conversation after seeing the impact and confirming once | destructive owner action | to be chosen in MP-UI2-FORGET-01 | to be chosen from the UI2 catalog | y
```
