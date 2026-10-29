# Mind Place Windows Product — Security lifecycle and threat model (Checkpoint 2A)

**P0 OPEN / ACTIVATION BLOCKED.** The staging payload is neither independently administered nor production-authoritative.

## Frozen contract protection and independent identity

The target is untrusted; the policy repository and dedicated App must be separately controlled. The policy must not import/run PR-controlled Python or checkout/execute target code. Native Git objects, immutable proof zip SHA-256 and GitHub-owned actual run/job/step/attempt evidence are verified with App-ID-bound required checks. Approved fingerprints cover immutable Git input paths, source merge tree, workflow and verifier bytes; source-equivalent reuse only after independent approval. Both IPC tests are mandatory:

- `native_capture_ipc::qualification::separate_process_capture_probe`
- `native_capture_ipc::qualification::owner_local_capture_roundtrip_requires_unlocked_epoch`

A successful Cargo zero-test filter does not count as native qualification. The original source verifier/inputs are unchanged. The controller's only `policy_root` is `policy/contracts`; all target contract copies remain immutable source Git blob bytes with original relative target paths. Extra files, traversal, symlinks, modified snapshots, unapproved active workflows or different controller template fail the 2A staging validator.

## P0: persistent previously successful check

GitHub required checks evaluate GitHub-owned completed check runs; they **do not honor the lease timestamp** encoded in a check output. A previously successful check may continue to satisfy branch protection during its platform-valid window (GitHub documents a past-seven-days requirement), even if its internal 600-second lease expires or policy says revoked. The future worker verifier rejects an expired lease for *new reuse decisions*; that does not automatically invalidate the old SUCCESS. A controller can PATCH a check to failure only while reachable with valid credentials and API access. Merely stopping the controller, disabling `enabled`, rotating keys or revoking App credentials does not retroactively invalidate a completed success.

Relevant official GitHub documentation:
- https://docs.github.com/en/pull-requests/how-tos/merge-and-close-pull-requests/troubleshooting-required-status-checks
- https://docs.github.com/en/rest/checks/runs
- https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue

### Lifecycle cases and evidence status

| Event | Existing completed SUCCESS | What is enforceable here |
| --- | --- | --- |
| Existing same HEAD; internal lease expiry | Remains GitHub SUCCESS | New worker reuse rejected; old success **not** auto revoked |
| Fingerprint or policy revision revoked | Existing SUCCESS not auto revoked | Online authorized App may PATCH failure; outage prevents guarantee |
| Controller stopped/restarted | SUCCESS remains until GitHub changes it | Recovery MUST reconcile current checks; no immediate outage guarantee |
| App key rotated/installation revoked | Old SUCCESS remains possible | New issuance needs valid App; revocation alone cannot retract old check |
| New head or changed merge source | Old head proof not enough | Source SHA and reconstructed merge must match; rerun/acceptance for new head |
| Stacked PR base changed | Prior base/tree proof superseded | Revalidate base, merge tree and protection on actual base |
| GitHub API fails during revocation | Old SUCCESS may remain | Report P0 incident, externally freeze merges, do not claim revocation |
| Policy repository compromised | Could forge check with stolen App | Separate repo ownership/protected environment/independent review required |

### Operational fail-safe: emergency merge-freeze

The owner/admin must **block merges using an independently enforced branch restriction/ruleset** that cannot be bypassed by ordinary writers, including stacked PR bases and `main` when applicable. Do not remove an existing required check as the freeze. Record the API/UI state and independently verify an otherwise-green PR is blocked before reporting a freeze effective. Freeze must be established and tested *before* enabling acceptance, not first invented during an incident. If the owner/admin, settings permission or GitHub API is unavailable, immediate freeze is **not guaranteed**; fail operationally (stop all release/merge activity) and keep activation blocked.

During a revocation/incident: (1) invoke verified merge freeze; (2) halt controller publications; (3) inspect all relevant PR and check-run IDs on each current SHA; (4) with trusted App, PATCH affected success to failure and verify actual GitHub result; (5) revoke/rotate compromised keys, revoke approval in independently protected policy; (6) verify rule enforcement and evidence on real GitHub; (7) owner/admin may restore only after new independent approval and green real enforcement demonstration. Never infer freeze from the runbook, a controller success log or a synthetic mocked status.

### Candidate future design: merge queue / per-merge fresh App check

GitHub merge queues create new `merge_group` SHAs and require checks there, with a queue timeout for missing checks. A policy controller deliberately issuing a fresh, independently App-bound acceptance check **only to the current queue SHA** could avoid relying solely on old PR-head checks for *new* queue entries while the controller is down. However this implementation does **not** support merge_group authority, and an already approved queue entry or controller failure after issuance still needs a fully specified revocation/incident treatment. Queue requirement, App-origin binding, webhook scope, inability to bypass queue, race behaviour, and outage tests require separate design and **live GitHub enforcement demonstration in Checkpoint 2B**. No queue policy is enabled now.

### Activation criteria / P0 exit

Do not close P0 based on periodical sweep, short lease, output expiry, synthetic unit tests, stale-check PATCH alone, or key rotation. Independent reviewer must validate an enforceable admission/merge control with actual GitHub settings under: expired successful same-head proof, revoked approved fingerprint, controller outage/restart, wrong App, policy revision, stale merge, and API failure. Until then **NO-GO for Checkpoint 2B activation**.

## P1: isolated staged workflow snapshots

The reviewed policy repo root must contain **no `.github/workflows/*.yml`** in Checkpoint 2A. Its product workflow snapshots are only in `policy/contracts/.github/workflows/` (GitHub cannot register nested contract-data YAMLs as Actions workflows). The controller template is `.github/windows-policy/verifier.yml`, *outside* the active workflow directory. During 2B only, authorized owner may promote the single reviewed controller into `.github/workflows/verify-windows.yml`. No target workflow may be promoted. `python3 scripts/staging_layout.py` verifies the intended copied payload before activation, but is not itself a GitHub enforcement mechanism.

## Not performed

No private policy repo, protected env, App/key installation, production required status checks, real merge freeze or queue, fingerprint approval, trusted acceptance, release, merge, native requalification, 100K indexing or owner-local tests.

## Option B decision: permanent lock, no merge admission

**Temporary unlocking is PROHIBITED until a stronger enforcement design is independently reviewed and demonstrated live.** Option B remains viable only as permanent-lock containment with no authorized merges. It is not viable as a singleton admission service. P1 is **MITIGATED by prohibiting the unsafe operation**; P0 is **OPEN**, and Checkpoint 2B/production are **NO-GO**. This supersedes the prior manual-exception recommendation.

GitHub documents a locked branch as read-only. Administrator enforcement, disabled fork syncing/force pushes/deletion, App-ID-bound strict checks and complete bypass visibility must all be independently inspected and then challenged live. These are proposed platform controls, **not live-proven in this checkpoint**. Operational serialization is an owner/admin agreement to prevent concurrent actions; it is not a GitHub transaction and cannot close the unlock race. Settings administrators retain the power to change enforcement.

### Official platform semantics and admission tuple

- [Branch protection](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/about-protected-branches): Lock branch makes a branch read-only; administrators/bypass roles normally have exceptions unless enforcement covers them. Push restrictions identify users/teams/Apps, not a one-use PR/tree authorization; availability depends on repository/organization plan.
- [Repository rulesets](https://docs.github.com/en/rest/repos/rules#get-all-repository-rulesets) with `includes_parents=true` include higher-level rulesets applying to this repository. [Ruleset detail](https://docs.github.com/en/rest/repos/rules#get-a-repository-ruleset) discloses `bypass_actors` **only with write access to the ruleset**. Repository or organization access alone does not prove that visibility. Bypass actors are grants to identities/roles, not a transaction binding an exact PR/source/base/tree/window.
- [Effective branch rules](https://docs.github.com/en/rest/repos/rules#get-rules-for-a-branch) returns active rules for the specified branch, including inherited rules. Disabled/evaluate rules are excluded. The inspector crosschecks these authoritative results against separately fetched active details; it does not emulate GitHub ref globs or infer inherited applicability from a name.
- [PR merge API](https://docs.github.com/en/rest/pulls/pulls#merge-a-pull-request): repository and PR identify the request, and `sha` guards the PR **head**. The documented call has no expected-base-SHA, expected-effective-tree, one-use unlock token, or admission-window parameter. A pre-read of those values plus unlock plus merge is not atomic. Actor authentication does not exclude every other authorized actor.
- [Auto-merge](https://docs.github.com/en/pull-requests/collaborating-with-pull-requests/incorporating-changes-from-a-pull-request/automatically-merging-a-pull-request) may merge when requirements pass. On queue-required branches, **Merge when ready** can enqueue even when the repository auto-merge setting is disabled. Disabling that repository setting alone is insufficient; enumerate both PR requests and queue entries.
- [Merge queues](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue) validate merge groups but do not implement this selected branch-lock mechanism's exact PR/source/base/approved-tree/actor/window exception. An already successful entry retains the outage/revocation race. No merge-group controller is implemented here.

No supported operation in these documented interfaces enforces the entire requested tuple atomically while opening a branch lock. This conclusion is scoped to the selected mechanism, not a claim that all possible GitHub architectures have been exhausted. No replacement architecture is introduced.

### Read-only inspection contract

`scripts/merge_freeze.py` validates the actual repository identity, enumerates inventory and effective branch rules in bounded 100-item pages (maximum 20; reaching the limit rejects all partial evidence), and inspects **every active inventory detail**, conservatively even when unrelated to a destination. Per-page type checks reject malformed lists, duplicate IDs, unknown enforcement, inconsistent summary/detail/source/target identities, missing conditions/rules and hidden bypass actors. Linked short pages, malformed/jumping/foreign continuation links, non-200 responses, API errors and oversized/malformed bodies fail closed. An empty inventory is accepted only after successful complete enumeration and effective-branch crosschecks.

Repository and inherited Organization sources are independently checked against repository identity/owner. Unsupported Enterprise/unknown sources or targets remain **NOT VERIFIED** and require owner/admin inspection; do not silently skip them. GitHub determines effective branch applicability via its branch-rules endpoint; all active details require `bypass_actors=[]`. A read-only HTTP operation does not mean a low-privilege read-only token has sensitive visibility. Use a separately authorized owner/admin inspection identity, without granting write authority to the policy App or changing its permissions.

All successful output remains a **configuration snapshot only**, with `no_merge_authorization=true`, `temporary_unlock_permitted=false`, `operating_mode=PERMANENT_LOCK_NO_ADMISSION` and `NOT_YET_LIVE_VALIDATED`. The inspector never writes settings, invalidates checks, authorizes an exception, attests current source/base approval, or proves exclusive administration. Auto-merge/queue and identity inspection in `ACTIVATION.md` are additional owner evidence; no such evidence can turn this output into permission to unlock.

### Residual risks and live gates

A completed SUCCESS does not expire with the controller lease. Permanent-lock containment is useful only while actually enforced and not bypassed; a concurrent settings administrator can change it. Controller outage before admission cannot authorize a merge because **no admission exists in this mode**. Outage after an unauthorized unlock or relock API failure is an incident with no guaranteed automatic closure. Stop merges/releases, escalate immediately to the exclusive owner/admin and independent reviewer, restore and read back every protection and branch SHA through an available authorized channel, and keep P0 OPEN. An attempted relock, monitor, or successful controller restart is not evidence of closure.

The full owner/admin scenario matrix, exclusive procedure, competing-request inventory, source/base/tree recheck, branch readback and incident response are in `ACTIVATION.md`. **LIVE VERIFICATION REQUIRED** means exactly that; Python fixtures test inspection/rejection behavior only. Option B cannot regain manual exceptions merely by passing these tests. A separately reviewed stronger admission design and new owner authorization are required.
