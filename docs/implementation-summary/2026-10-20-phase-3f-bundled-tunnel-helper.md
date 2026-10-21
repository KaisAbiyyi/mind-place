# MP-3F — bundled official Windows tunnel helper

Owner request: urgent top priority in the current Phase 3, 2026-10-20 (Asia/Jakarta). Scope: users install Mind Place and receive the official helper with it, without a separate download/copy step. Account provisioning, runtime key entry and explicit remote consent remain required.

## Implementation

- Code checkpoint: `d27fa0987a4ad626b741321ae67ccb0c2767f72a`; branch `feature/phase-3f-bundled-tunnel-helper`; [PR #17](https://github.com/KaisAbiyyi/mind-place/pull/17).
- Claim: `mind-place/MP-3F`, session `20261020-bundled-helper`, generation 1, Codex local implementer; dependency MP-3E merged (#14, `5546504`). Locks: Windows packaging, hosted helper resolution, hosted setup copy.
- Pinned official `openai/tunnel-client` v0.0.16, Windows amd64. ZIP SHA-256 verified against the upstream release checksum; executable and resource digests match the published release SPDX/asset metadata. Pin is tracked in `scripts/tunnel-client.lock.json`.
- Build-time bounded download and strict member extraction; cached executable/notices/receipt revalidated on reuse. No runtime downloader, PATH lookup, arbitrary binary selector or executable committed in Git.
- Windows Tauri sidecar configuration supplies the helper to NSIS. Portable staging includes the same verified helper plus upstream LICENSE, NOTICE, dependency license report, original release SBOM and an exact included-file receipt under `tunnel/`.
- The original upstream SBOM includes its Cloudflare companion; the Mind Place receipt explicitly records that `cloudflared.exe` and its manifest are excluded. No Cloudflare companion is bundled here. Executable size: 22,591,488 bytes.
- Pre-launch verification uses the compiled checksum/size pin and a retained Windows read-only file handle that prevents helper writes/deletion while running. Missing/tampered bytes fail closed; UI gives repair/reinstall guidance without disclosing key contents.
- Existing unlock, owner consent, ephemeral read-only identity, environment-only secrets, stop/lock/revoke and watchdog behavior remain intact. Raw archive/state/index formats and capture permissions are unchanged.
- Roadmap/current phase/board now prioritize MP-3F before 3L1/3L2/3U. Historical 3D helper-install instructions are explicitly superseded by this packaging change.

## Verification

PASS:

- `npm run verify:repo`.
- `npm run typecheck`.
- `npm test`: 103 web + 95 Rust tests passed; 1 pre-existing storage qualification test intentionally ignored.
- `cargo fmt --all -- --check`.
- Focused hosted native suite: 9 passed, 1 intentionally ignored subprocess fixture. Positive-path test uses the actual pinned upstream Windows binary, exercises `--version`, demonstrates write-lock enforcement, and rejects same-size corruption. Negative-path test initially reproduced the old acceptance of an arbitrary adjacent executable (RED), then passed with verification (GREEN).
- Portable regression initially reproduced the omitted helper (RED), then passed (GREEN); download/cache corruption and oversized payload tests also pass.
- UI regression initially reproduced manual-install guidance (RED), then passed for included-helper/repair guidance and key clearing (GREEN).
- Independent read-only review by gpt-6-luna (max): no Critical or Important findings in `5546504..d27fa09`. No weakening of unlock, consent or secret handling found.

PASS package/build verification:

- `npm run build`: release app and portable staging succeeded, 32.89 MB / 31.36 MiB, 20 files.
- `npm run dist:win`: NSIS succeeded. Generated installer script explicitly embeds `tunnel-client.exe` adjacent to the app plus `tunnel/` notices and receipt.
- Final portable executable and notice hashes match the pin. ZIP independently read through .NET ZipFile: 20 entries, correct helper SHA-256, no Cloudflare executable.
- Installer: `release/Mind-Place-MP-3F-windows-x64-setup.exe`, 9,638,494 bytes, SHA-256 `b4018d6c1f8274a38fb13b4577299e092b3a5c6b2ab024ab1a3fac0eccd890f0`.
- Portable ZIP: `release/Mind-Place-MP-3F-windows-x64-portable.zip`, 12,848,765 bytes, SHA-256 `b29dd77923d5310cff246a7868b15e8b836e9d1caffd58f2b32b5a09ea40b85e`.
- Local generated proof: `outputs/phase-3f-package-proof.json`, including app/helper hashes and the preserved working-tree Cargo.lock digest. Binaries, ZIP and generated proof/logs are intentionally untracked.

MERGED: PR #17 squash-merged to `main` at `f9d1313ac3301e69ccb3fba5d2426c0985940fb8` on 2026-10-21 after final PR-head [Windows CI PASS](https://github.com/KaisAbiyyi/mind-place/actions/runs/39305453977). Local package gates above are independently observed; installed-app qualification remains pending. Installer compilation/resource mapping and portable ZIP contents are verified; installed GUI behavior is not inferred from them.

NOT RUN: installed-app GUI acceptance, real local Codex/Claude MCP qualification (3L1), real official tunnel/account/network calls (3L2), owner ChatGPT continuity/revocation acceptance (3U). Those gates remain mandatory; no public endpoint or account resource was created.

## Review limitations

Deferred minor findings:

- An unexpected extra file in the generated developer helper-cache resources directory fails verification and requires removing that generated cache before rebuilding; re-extraction does not clear unknown leftovers. This does not affect installed users.
- Only Windows x64 bundles the helper. Other platforms remain unqualified; their shared repair/reinstall error copy is less specific than their manual development setup requires.

Existing unrelated `Cargo.lock` edits (21 additions/1 deletion) were preserved and excluded from commits; only the new desktop `sha2` dependency edge was committed. Local tests/builds use that preserved working-tree dependency resolution. Artifacts and live-account acceptance must not be described as an installed release or merged milestone merely because source tests pass.

## Handoff

PROJECT=Mind Place; TASK_ID=MP-3F; ROLE=local implementer; BASE_SHA=55465041cf9f2e2df05eeeccec9ac3249df1cc0c; BRANCH=feature/phase-3f-bundled-tunnel-helper; REMOTE_CODE_CHECKPOINT_SHA=d27fa0987a4ad626b741321ae67ccb0c2767f72a; PR=17; STATUS=MERGED at f9d1313 / LOCAL BUILD AND PACKAGING PASS / PR-HEAD WINDOWS CI PASS; NEXT=3L1 local MCP qualification, then 3L2 and 3U; LOCAL_REQUIRED=3L1/3L2; HUMAN_REQUIRED=3U. Preserve the unrelated lockfile changes and private archives when resuming. Product defects found in account qualification return to a bounded fix unit with redacted evidence.
