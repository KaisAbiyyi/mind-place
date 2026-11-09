# MP-UI-08 — Settings, recovery, privacy and operational diagnostics

**Task:** MP-UI-08  
**Repository:** `KaisAbiyyi/mind-place`  
**Base:** `main` @ `24648c74f3f59e176974d3878049b882478db69b`  
**Branch:** `work/mp-ui-07-08-local`  
**Exclusive resource lock:** `ui-module/settings-recovery-diagnostics`  
**Execution:** Local Windows Antigravity implementation

## 1. Objective and scope

Implement the dedicated HTML Settings workspace, integrating recovery/backup/restore ceremonies, privacy controls, and connector/index/retrieval operational diagnostics following monochrome Knowledge OS design tokens.

## 2. Settings sections

In `src/settings-workspace.ts` and `src/settings-workspace.css`:

1. **General & Appearance:**
   - Interactive theme toggles (`light` / `dark`) setting `--mp-bg-app`, `--mp-bg-surface`, `--mp-text-primary`.
   - Density preference (`comfortable` / `compact`).
   - Technical provenance mode (`data-technical-mode="true"`), revealing technical IDs and traces across the interface.

2. **Storage & Vault:**
   - Real, factual vault status from `get_security_status` (configured, locked, security epoch).
   - Key recovery ceremony: inputs for secret and recovery code with `unlock_vault_with_recovery`, `begin_vault_setup`, and `confirm_vault_setup` backed by explicit two-click confirmation.
   - Online vault backup: validates absolute path and generation before invoking `create_vault_online_backup`.
   - Data portability & deletion boundaries: reports factual capabilities (Markdown export, generic import) and clarifies that destructive sector zeroing requires backend acceptance (MP-9C), while Curated Memory forget actions apply non-disclosure tombstones without modifying raw archive files.

3. **Privacy & Security:**
   - Instant "Lock Mind Place" session control invoking `lock_security` and invalidating the active unlock epoch.
   - Factual summary of loopback-only MCP listener, outbound-only tunnel, and zero-telemetry architecture.

4. **Operational Diagnostics:**
   - Search index stats from `get_archive_index_stats` (generation, sources, revisions, chunks).
   - "Rebuild Archive Index" action invoking `rebuild_archive_index` behind an irreversible-action confirmation dialog.
   - Local MCP process status (`get_local_mcp_status`) without leaking tokens or secrets.
   - Hosted tunnel status (`get_hosted_tunnel_status`) distinguishing helper process presence from verified remote connectivity.
   - Retrieval query tracer invoking `inspect_retrieval` with metadata-only inspection (elapsed microseconds, candidate counts, token estimates; never private snippets).

5. **Overview Health Coordination:**
   - Exports `fetchHealthSummary` to aggregate live security and connector telemetry for Overview and diagnostics without competing edits.

## 3. Verification

- All 5 dedicated unit tests in `test/settings-workspace.test.ts` pass.
- All 3 health diagnostic tests in `test/health-workspace.test.ts` pass.
- Route navigation and contextual diagnostics in `test/knowledge-os.test.ts` pass.
- Full web test suite (213/213 tests) passes.
