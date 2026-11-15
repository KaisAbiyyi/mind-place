# MP-9C Slice 1: owner-facing forget UI — decision record (BLOCKED-DESIGN, owner decision pending)

Status: **open**. Recorded 15 November 2026 on PR #151. No UI has been built for this. Nothing in this note authorizes building one.

## What exists in the backend (PR #151)

The trusted native desktop has three Tauri commands. They work only with the encrypted vault, and none of them is in the MCP tool catalog:

- `preview_source_forget(source_id, reason, secret)` returns the impact summary and the preview fingerprint. It never returns the one-time token.
- `confirm_source_forget(source_id, preview_sha256, secret)` commits a **logical** tombstone and a *pending* receipt (`physical_erase_verified = false`).
- `get_source_forget_progress(source_id)` reads the status.

Both preview and confirm require the owner to re-enter the unlock secret. The one-time token lives only in native memory, allows a single attempt, and is dropped on Lock. Preview and confirm commits are serialized with Lock. No archive, object or backup bytes are removed; that is MP-9C2.

## What does not exist

There is no user-facing entry point, preview display, explicit confirmation step or progress presentation. The UI2 backend capability audit (`docs/ui/MP-UI2-01-backend-capability-audit.md`, gap `G5`) predates these commands and keeps Memory read-only. This note does not change that audit or the UI2 design lock.

## Why this is blocked on the owner

The MP-9C Slice 1 acceptance text includes "owner preview and one-time confirmation". The signed-off guide does not show a forget flow. Under `docs/ui/UI-PRINCIPLES.md` a capability the guide does not show needs:
1. the gap protocol (job, class, destination, catalog pattern, EN + ID copy, design-gap ledger with screenshots);
2. an owner decision when no catalog pattern clearly fits a destructive, irreversible action.

An agent must not improvise this flow.

**The person's job:** "Make Mind Place stop showing and using one saved conversation, after I see what that affects and confirm it once."

## Options for the owner (text only)

1. **Accept a reduced Slice 1 contract.** MP-9C Slice 1 accepts the trusted native commands, with the backend preview and one-time confirmation. A separately named, gated successor (for example under the UI2 lane) owns the user-facing entry, preview display, confirmation step and progress. It would run under the UI gap protocol with its own UI locks and design-gap ledger. MP-9C could then be reviewed for Done on the backend contract once the owner records this decision.
2. **Keep the full contract in MP-9C.** A minimal UI entry is added inside MP-9C under the gap protocol. That requires coordinated UI resource locks and a design-gap ledger entry, and it may stay `BLOCKED-DESIGN` until a catalog pattern for a destructive confirmation is approved.

## Design-gap ledger entry (pending)

```text
capability | person's job | class | destination | pattern used | needs owner review
owner source forget (preview + one-time confirm + progress) | stop Mind Place from showing/using one saved conversation after seeing the impact and confirming once | destructive owner action | not chosen (no new top-level destination allowed) | none (no catalog pattern selected) | y — BLOCKED-DESIGN
```

Until the owner decides, MP-9C Slice 1 must not be marked Done/PASS against the original acceptance text.
