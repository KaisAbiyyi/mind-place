# MP-CI-WIN-01: Windows qualification reuse

**Status: IMPLEMENTED BUT ENFORCEMENT BLOCKED.** This is a dormant implementation
candidate for independent review, not production acceptance. PR #131 remains
Draft and stacked on #129. Reuse requires an independent App authorization; an
unset `WINDOWS_PRODUCT_APP_ID` always schedules native work for Ready PRs.
No administrative settings, App installations or policy repositories were changed.

## Recovery and durable ownership

Owner-authorized takeover, Codex cloud session 2026-10-29, claim generation 2;
exclusive resource: MP-CI-WIN-01 Windows CI contract. Reclaimed remote
`codex/mp-ci-win-01` at `4de959f9c2aec8d269d019b7900ff9d4664e350e`.
Base #129 remains `7733a91fa7bbfc23504bf4b2f8af09baaa3f8a3f`.
The owner explicitly serialized this takeover; no parallel implementation was
started. All implementation checkpoints are pushed to the existing branch.
Agent identity is recorded here; new commits use the verified owner's GitHub
noreply identity, as required by the repository orchestration policy.

## Authoritative trust boundary

Deploy `.github/windows-policy/verifier.yml` **in a separate owner-controlled
policy repository**. It checks out an administrator-pinned full policy commit,
uses a distinct GitHub App installed only on the target repository, and never
checks out, imports, installs dependencies from, or executes target code.
Target Git trees are fetched into a disposable bare object database. The App
private key exists only in the policy repository's protected environment.
The target repository's Actions token cannot impersonate this App.

The App creates/updates the stable `Windows Product / trusted acceptance`
check on the current PR head. Branch protection must require that exact context
**and App ID**, require up-to-date branches, and include administrators. The
controller independently reads this protection before issuing an authorization
lease and again before acceptance. A missing, disabled, spoofable or unreadable
protection configuration fails closed. `administration: read` is needed only by
the independent App to inspect protection; it cannot change settings.

A PR-controlled check, step name, artifact or fingerprint is never authority.
The independent policy compares the native workflow, input policy and verifier
bytes against its pinned copies, reconstructs the clean PR merge tree, and
validates source parents and GitHub-owned run/head/base/attempt/job/step data.
All required native steps must actually complete successfully. A matching name
with a changed command fails the byte comparison. The artifact's versioned
claims must match independently computed immutable inputs and run provenance.
Skipped native steps, foreign PRs, corrupted/expired artifacts, unavailable API
data, failed/cancelled runs and superseded current sources cannot accept.

The target's `Windows Product / candidate result` is explicitly advisory and
must **never** be configured as the authoritative required acceptance check.
A Draft reports native PENDING; a Ready PR reports native execution or a reuse
candidate. Neither asserts that repository enforcement is active.

## Implementation

- `scripts/windows_product_inputs.py`: immutable Git path/mode/blob closure,
  policy-domain separated; working-tree mutations do not change identity.
- `scripts/windows_product_gate.py`: production decision, post-slot recheck,
  versioned proof emission, independent native verifier and App controller.
- `.github/workflows/windows-product.yml`: fixed native execution contract,
  read-only worker token, SHA-pinned actions, no persisted checkout credentials.
  Preserves all previous formatting, repository, TypeScript, visual Settings,
  bundled SQLite, reader/Rust, capture sidecar, simulated MCP/revocation,
  hosted-tunnel, desktop build and distribution licensing commands.
- `.github/workflows/ci.yml`: fast feedback and policy regressions; native
  commands now have their own execution identity.
- `.github/windows-policy/`: deployment template, disabled example config and
  exact native required-step manifest. This directory is not an active workflow.
- `scripts/tests/test_windows_product*.py`: actual verifier and decision tests
  over real Git merge objects, API wire fixtures, CLI events and workflow flow.

A same-repository Ready PR runs native when there is no independently authorized
matching evidence. A valid App check carries a source-bound authorization lease
of at most ten minutes. Both initial scheduling and the native job after its
per-PR concurrency slot independently reverify available completed evidence.
A proof arriving while queued avoids duplicate compilation. Unrelated PRs use
different slots; running native jobs are not cancelled on new pushes. Queued
superseded events fail before compiling; a superseded completed qualification
cannot mint current acceptance. GitHub can replace pending jobs in a concurrency
group; the surviving latest workflow re-evaluates current source and evidence.
A proof must belong to the same PR and be at most fourteen days old. Reuse of an
older source is intentional only when the effective immutable input closure is
identical; acceptance always binds the current head, base and reconstructed merge.

Main pushes always execute native work and do not mint reusable PR evidence.
Manual dispatch requires an open same-repository Ready PR and its current head
branch; it always executes native tests. For a release/final enforcement window,
set `require_fresh: true` and an immutable UTC `fresh_after` timestamp in the
independent pinned config. Only a successful manual current-merge run created
at/after that cutoff can satisfy acceptance. Workers cannot reuse under that
lease. The template's optional fresh audit also requires this configured cutoff.

## Fingerprint closure and documentation audit

Everything tracked remains bound except two ordinary regular files:
`.github/workflows/ci.yml` and this handoff. Symlinks/executable docs are bound.
The native workflow is independent of fast orchestration and has no dependency
on its outputs or shared Cargo build cache. All other workflows, scripts,
product/native/security/IPC source, frontend/extension/assets, manifests,
lockfiles, configuration, docs and toolchain declarations remain inputs.

The old blanket `docs/**/*.md` exclusion was removed. The audited handoff is
not consumed by any native command: `verify-repo.mjs` reads explicit root
metadata/README/CONTRIBUTING and rejects a fixed set of tracked paths;
`tauri.mjs` recursively stages only product executables, extension, notices,
license and pinned helper resources; `generate-notices.mjs` scans dependency
license files rather than repository docs; `verify-release-license.mjs` scans
packaged output; Settings capture uses the frontend. Tauri resources explicitly
list extension/notices/LICENSE; Rust include_str/include_bytes inputs and test
fixtures do not reference this handoff. Other documentation remains conservative
input because an ongoing universal Markdown exclusion cannot be established.
Future consumers must keep the excluded handoff outside product execution;
changing the closure requires independent policy review and a new qualification.

## Exact administrator handoff (not performed)

1. Independently review the final PR and policy, including the trust boundary.
   Integrate #129 normally before retargeting #131; do not modify #129 here.
2. Create a separate policy repository controlled only by the owner/reviewed
   policy maintainers. Copy the reviewed `scripts/windows_product_gate.py`,
   `scripts/windows_product_inputs.py`, `.github/workflows/windows-product.yml`
   and `.github/windows-policy/native-steps.json` at their current paths. Copy
   `verifier.yml` to `.github/workflows/independent-windows.yml` there.
3. Create a dedicated GitHub App with target-repository permissions:
   contents/read, actions/read, pull_requests/read, administration/read,
   checks/write (metadata/read implicit). Install only on `mind-place`.
   Store its private key only as `WINDOWS_PRODUCT_APP_PRIVATE_KEY` in the
   independent repository environment `windows-policy-authority`.
4. Protect that environment to the policy repository's protected default branch;
   restrict policy writers, require independent review and prevent bypass of
   policy protection. PR workflows there must have no authority environment or
   App secret access. Target writers must have no policy write/secret access.
5. In the independent repository copy `config.example.json` to
   `.github/windows-policy/config.json`, set the real numeric App ID and
   `enabled: true`, commit/review, and pin its **full immutable commit** in
   environment variable `WINDOWS_POLICY_COMMIT`. Set environment variable
   `WINDOWS_PRODUCT_APP_ID` to that App ID. Actions are already SHA-pinned.
6. On every accepted target base (main, and stacked base if testing before
   integration), configure strict branch protection with:
   `required_status_checks.strict=true`, `enforce_admins=true`, and
   `required_status_checks.checks=[{"context":"Windows Product / trusted acceptance","app_id":APP_ID}, ...existing checks]`.
   Preserve existing fast/security/storage/native/100K requirements; this task
   does not waive or rewrite them. Never use a context-only required check.
   If using rulesets instead, extend/review the enforcement reader first: this
   candidate intentionally supports the exact branch-protection API above.
7. Set target repository variable `WINDOWS_PRODUCT_APP_ID` to the installed App
   ID. Enable the independent scheduled workflow and dispatch it once. Verify
   a real changed-input native run, then an excluded-handoff/fast-only Ready PR
   push with `VERIFIED_REUSE`, including exact current-source check provenance.
   Until those GitHub results exist, production savings and activation are NOT
   VERIFIED. Do not merge on the basis of controlled fixtures alone.

No merge, release, deployment, protection change or 100K execution is authorized
by this handoff. Minimum external action is owner/admin provisioning of the
separate App/policy repository and App-bound branch protection after review.

## Validation and performance

Baseline supplied by owner: approximately 43 minutes per native Windows build.
Controlled production verifier/decision/CLI/workflow tests demonstrate validated
reuse and fail-closed fallback, including post-slot discovery. They do not measure
hosted Windows savings. The independent authority polls every five minutes;
activation adds that eventual verification delay, Git/API lookup time and a
small Linux gate. Without a fresh authority lease the native fallback may run
unnecessarily; correctness takes priority. API pagination is bounded (20 pages)
and exhaustion fails closed. Artifact expiration triggers native qualification.

Cloud baseline/native results and exact final SHA are reported in PR #131 and
the final handoff. Existing MP-10C's 34 regressions remain unchanged. No 100K
indexing, private archive access or installed-profile access was performed.
Windows is skipped by Draft policy, not validated reuse. An actual native run
and production reuse measurement remain activation acceptance gates. Independent
review must assess this candidate before any authority is installed.
