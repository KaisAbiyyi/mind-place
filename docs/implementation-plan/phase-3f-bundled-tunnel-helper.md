# MP-3F — bundled official Windows tunnel helper

Priority: **TOP PRIORITY in Phase 3**, requested by the owner on 2026-10-20. Execute before MP-3L1/3L2/3U; those gates still require real client/account evidence. Scope: remove manual helper download/copy from the Windows installation experience. Tunnel/workspace provisioning and owner consent remain explicit.

CLAIM: mind-place/MP-3F; agent=Codex local; session=20261020-bundled-helper; generation=1; status=RUNNING; base/checkpoint=55465041cf9f2e2df05eeeccec9ac3249df1cc0c; branch=feature/phase-3f-bundled-tunnel-helper; locks=windows-packaging,hosted-helper-resolution,hosted-setup-copy; dependencies=MP-3E merged (#14). No open PRs or claims were found before this claim. Local implementation is explicitly authorized by this request. Existing Cargo.lock changes belong to prior local work and are excluded from this unit except for its own new direct dependency edge.

Acceptance:

- Build-time fetch of pinned official OpenAI Windows x64 helper, SHA-256 verification, bounded download/extraction and cache revalidation; no generated binaries in Git.
- NSIS installer and portable payload contain helper plus upstream licenses and receipt. No Cloudflare helper or unrelated source trees are added.
- App checks the adjacent helper against the compiled pin before launch; missing/tampered helper fails closed with repair instructions. No runtime download, PATH lookup or arbitrary executable selection.
- Setup says the helper is included. Unlock, explicit remote consent, ephemeral read-only credential, stop/lock/revoke and secret isolation remain intact.
- Tests cover packaging inclusion, missing/tampered files, narrow payload allowlist and native helper resolution. Run full repo/typecheck/tests/format/build plus `npm run dist:win`.
- Report package proof separately from installed GUI, live tunnel, local MCP and owner acceptance. Do not claim 3L1/3L2/3U PASS.

Implementation: pin the existing v0.0.16 release and published asset checksums, stage the verified executable as a Windows Tauri sidecar, include notices as resources, and reuse the adjacent-executable lifecycle. Production archive/state/index formats and capture behavior are outside scope.

## Completion receipt

Code checkpoint `d27fa0987a4ad626b741321ae67ccb0c2767f72a`, PR #17. Status: **IMPLEMENTED / LOCAL PACKAGING PASS / REVIEW**, not merged. Full baseline, Windows NSIS, portable ZIP integrity and native helper tests passed. Independent review found no Critical/Important issues. Latest GitHub CI/integration and 3L1/3L2/3U remain pending. [Dated implementation summary](../implementation-summary/2026-10-20-phase-3f-bundled-tunnel-helper.md) records artifact hashes, test counts, minor limitations and preserved unrelated lockfile work. Claim moves to REVIEW; no overlapping writer is authorized by this receipt.
