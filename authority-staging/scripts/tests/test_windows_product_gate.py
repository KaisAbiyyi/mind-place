"""Production verifier/decision tests over real Git merge objects and fake API wire data."""
import copy
import datetime as dt
import hashlib
import io
import json
import os
from pathlib import Path
import shutil
import shlex
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
import urllib.error
import zipfile
import yaml

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import windows_product_gate as gate

ROOT = Path(__file__).resolve().parents[2]


class API:
    repo = "KaisAbiyyi/mind-place"

    def __init__(self):
        self.data = {}
        self.writes = []

    def request(self, path, data=None, method=None, binary=False):
        if data is not None:
            self.writes.append((path, copy.deepcopy(data), method))
            return {"id": 99, "app": {"id": 42}}
        result = self.data[path]
        if isinstance(result, Exception):
            raise result
        return copy.deepcopy(result)

    def pages(self, path, key=None):
        result = self.request(path)
        return result if key is None else result[key]

    def iter_pages(self, path, key=None):
        return iter(self.pages(path, key))


class GateTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.policy = self.root / "policy"
        self.repo = self.root / "target"
        self.repo.mkdir()
        for name in (*gate.CONTRACT_FILES, ".github/windows-policy/native-steps.json"):
            for target in (self.policy, self.repo):
                file = target / name
                file.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(ROOT / 'policy/contracts' / name, file)
        self.git("init", "-q")
        self.git("config", "user.name", "Fixture")
        self.git("config", "user.email", "fixture@example.invalid")
        self.write("Cargo.toml", "initial")
        self.write("docs/windows-product-gate-handoff.md", "notes")
        self.write(".github/workflows/ci.yml", "fast")
        self.base = self.commit()
        self.write("crates/core/src/lib.rs", "native source")
        self.head = self.commit()
        self.source = self.merge(self.head)
        self.current = {"pr": 131, "head": self.head, "base": self.base, "source": self.source, "base_ref": "main"}
        self.sources = gate.Sources(self.repo, API.repo)
        self.identity = self.sources.fingerprint(self.source)
        self.api = API()
        self.run = {"id": 7, "repository": {"id": 123, "full_name": API.repo}, "head_repository": {"full_name": API.repo}, "path": gate.WORKFLOW,
                    "event": "pull_request", "status": "completed", "conclusion": "success", "run_attempt": 1,
                    "updated_at": gate.utc().isoformat(), "head_sha": self.head,
                    "pull_requests": [{"number": 131, "head": {"sha": self.head}, "base": {"sha": self.base}}]}
        self.proof = {"schema": gate.SCHEMA, "policy": gate.POLICY, "repository": API.repo,
                      **self.current, "fingerprint": self.identity, "run_id": 7, "attempt": 1}
        self.steps = [{"name": name, "status": "completed", "conclusion": "success"}
                      for name in json.loads((ROOT / "policy/contracts/.github/windows-policy/native-steps.json").read_text())["common"]]
        self.artifact = {"id": 8, "name": "windows-product-proof-7-1", "expired": False,
                         "workflow_run": {"id": 7, "head_sha": self.head, "repository_id": 123, "head_repository_id": 123}}
        self.api.data = {
            "/actions/runs/7": self.run,
            "/actions/runs/7/artifacts": {"artifacts": [self.artifact]},
            "/actions/runs/7/attempts/1/jobs": {"jobs": [{"name": "Verify Windows product", "run_id": 7,
                  "conclusion": "success", "labels": ["windows-latest"], "steps": self.steps}]},
            "/actions/workflows/windows-product.yml/runs?status=success": {"workflow_runs": [self.run]},
            "/branches/main/protection": {"required_status_checks": {"strict": True, "checks": [{"context": gate.CHECK, "app_id": 42}]},
                                           "enforce_admins": {"enabled": True}},
            "/pulls?state=open": [{"number": 131, "draft": False}],
        }
        self.config = {"enabled": True, "repository": API.repo, "app_id": 42, "policy": gate.POLICY,
                       "approved_fingerprints": [self.identity]}
        self.publish_current()
        self.pack()

    def git(self, *args):
        return subprocess.check_output(["git", *args], cwd=self.repo).decode().strip()

    def write(self, path, value):
        file = self.repo / path
        file.parent.mkdir(parents=True, exist_ok=True)
        file.write_text(value)

    def commit(self):
        self.git("add", "-A")
        self.git("commit", "-qm", "fixture")
        return self.git("rev-parse", "HEAD")

    def merge(self, head):
        return self.git("commit-tree", self.git("rev-parse", head + "^{tree}"), "-p", self.base, "-p", head, "-m", "merge")

    def publish_current(self):
        self.api.data["/pulls/131"] = {"state": "open", "draft": False, "merge_commit_sha": self.current["source"],
                 "head": {"sha": self.current["head"], "repo": {"full_name": API.repo}},
                 "base": {"sha": self.current["base"], "ref": "main", "repo": {"full_name": API.repo}}}
        self.lease = {"schema": "windows-product-acceptance/v1", "policy": gate.POLICY, "current": self.current.copy(),
                      "fingerprint": self.sources.fingerprint(self.current["source"]), "anchor_enforced": True,
                      "expires": int(gate.utc().timestamp()) + 599}
        self.check = {"app": {"id": 42}, "name": gate.CHECK, "head_sha": self.current["head"],
                      "status": "in_progress", "conclusion": None, "output": {"text": json.dumps(self.lease)}}
        self.api.data[f"/commits/{self.current['head']}/check-runs?check_name=Windows%20Product%20%2F%20trusted%20acceptance"] = {"check_runs": [self.check]}

    def pack(self, members=None):
        archive = io.BytesIO()
        with zipfile.ZipFile(archive, "w") as output:
            for path, value in (members or {"proof.json": json.dumps(self.proof)}).items():
                output.writestr(path, value)
        self.api.data["/actions/artifacts/8/zip"] = archive.getvalue()
        self.artifact["digest"] = "sha256:" + hashlib.sha256(archive.getvalue()).hexdigest()

    def verify(self):
        return gate.verify_run(self.api, self.sources, 7, self.current, self.identity, self.policy)

    def decide(self, fresh=False, app_id=42):
        return gate.decision(self.api, self.sources, self.current, app_id, self.policy, fresh)[0]

    def advance(self, path, value):
        self.write(path, value)
        self.current["head"] = self.commit()
        self.current["source"] = self.merge(self.current["head"])
        self.publish_current()

    def test_actual_native_evidence(self):
        self.assertEqual(self.verify()["run_id"], 7)

    def test_artifact_digest_missing_malformed_or_mismatched_cannot_qualify(self):
        for value in [None, "", "sha256:invalid", "sha256:" + "0" * 64, "sha512:" + "a" * 64, 123]:
            with self.subTest(digest=value):
                self.artifact["digest"] = value
                with self.assertRaisesRegex(gate.Rejected, "digest"):
                    self.verify()
                self.assertEqual(self.decide(), "native")

    def test_modified_archive_envelope_rejected_even_with_unchanged_valid_proof(self):
        payload = io.BytesIO(self.api.data["/actions/artifacts/8/zip"])
        with zipfile.ZipFile(payload, "a") as archive:
            archive.comment = b"changed transport envelope, same valid receipt"
        self.api.data["/actions/artifacts/8/zip"] = payload.getvalue()
        with self.assertRaisesRegex(gate.Rejected, "digest"):
            self.verify()

    def test_evidence_invalidated_during_authority_wait_requires_native(self):
        path = f"/commits/{self.head}/check-runs?check_name=Windows%20Product%20%2F%20trusted%20acceptance"
        self.api.data[path] = {"check_runs": []}
        elapsed = [0]
        def sleep(seconds):
            elapsed[0] += seconds
            self.api.data[path] = {"check_runs": [self.check]}
            self.artifact["expired"] = True
        result = gate.coordinate(self.api, self.sources, self.current, 42, self.policy,
                                 timeout=31, clock=lambda: elapsed[0], sleep=sleep)
        self.assertEqual(result[0], "native")

    def test_vault_source_cannot_qualify_without_native_encrypted_ipc(self):
        self.advance(".github/workflows/mp9b-sqlcipher-qualification.yml", "name: SQLCipher native qualification\n")
        self.head, self.source = self.current["head"], self.current["source"]
        self.identity = self.sources.fingerprint(self.source)
        self.run.update(head_sha=self.head, pull_requests=[{"number": 131, "head": {"sha": self.head}, "base": {"sha": self.base}}])
        self.artifact["workflow_run"]["head_sha"] = self.head
        self.proof.update(**self.current, fingerprint=self.identity)
        self.pack()
        with self.assertRaisesRegex(gate.Rejected, "native capture encrypted IPC"):
            self.verify()
        ipc = {"name": "Qualify native capture encrypted IPC and revoked owner epoch", "status": "completed", "conclusion": "success"}
        self.steps.append(ipc)
        self.assertEqual(self.verify()["run_id"], 7)
        for outcome in ["skipped", "failure", "cancelled"]:
            ipc["conclusion"] = outcome
            with self.assertRaisesRegex(gate.Rejected, "native capture encrypted IPC"):
                self.verify()

    def test_new_unchanged_head_waits_on_ubuntu_then_avoids_windows(self):
        workflow = yaml.safe_load((ROOT / "policy/contracts" / gate.WORKFLOW).read_text())
        self.assertIn("coordinate", workflow["jobs"], "new-head lease race needs a pre-native coordination stage")
        self.advance("docs/windows-product-gate-handoff.md", "unchanged native closure")
        check_path = f"/commits/{self.current['head']}/check-runs?check_name=Windows%20Product%20%2F%20trusted%20acceptance"
        self.api.data[check_path] = {"check_runs": []}
        event = self.root / "race-event.json"
        event.write_text(json.dumps({"number": 131, "pull_request": {
            "head": {"sha": self.current["head"]}, "base": {"sha": self.base, "ref": "main"}}}))
        output = self.root / "race-output"
        env = {"GITHUB_EVENT_PATH": str(event), "GITHUB_EVENT_NAME": "pull_request", "GITHUB_REPOSITORY": API.repo,
               "GITHUB_SHA": self.current["source"], "GITHUB_OUTPUT": str(output), "WINDOWS_PRODUCT_APP_ID": "42",
               "EXPECTED_SOURCE": self.current["source"]}
        elapsed = [0]
        def sleep(seconds):
            elapsed[0] += seconds
            if elapsed[0] >= 30:
                self.api.data[check_path] = {"check_runs": [self.check]}
        previous = os.getcwd()
        try:
            os.chdir(self.repo)
            with patch.dict(os.environ, env), patch.object(gate, "GitHub", return_value=self.api), \
                 patch.object(gate.time, "monotonic", side_effect=lambda: elapsed[0]), patch.object(gate.time, "sleep", side_effect=sleep):
                plan = workflow["jobs"]["plan"]["steps"][-1]["run"]
                with patch.object(sys, "argv", shlex.split(plan)[1:]):
                    gate.main()
                self.assertIn("mode=native", output.read_text())
                output.write_text("")
                coordinator = workflow["jobs"]["coordinate"]
                self.assertEqual(coordinator["runs-on"], "ubuntu-latest")
                with patch.object(sys, "argv", shlex.split(coordinator["steps"][-1]["run"])[1:]):
                    gate.main()
            self.assertIn("mode=reuse", output.read_text())
            self.assertGreaterEqual(elapsed[0], 30)
            self.assertLessEqual(elapsed[0], 420)
            self.assertIn("coordinate", workflow["jobs"]["verify"]["needs"])
            self.assertIn("needs.coordinate.outputs.mode == 'native'", workflow["jobs"]["verify"]["if"])
        finally:
            os.chdir(previous)

    def test_authority_missing_or_unavailable_reaches_bounded_native_fallback(self):
        path = f"/commits/{self.head}/check-runs?check_name=Windows%20Product%20%2F%20trusted%20acceptance"
        for response in [{"check_runs": []}, urllib.error.URLError("controller/API unavailable")]:
            with self.subTest(response=type(response).__name__):
                self.api.data[path] = response
                elapsed = [0]
                def sleep(seconds):
                    elapsed[0] += seconds
                result = gate.coordinate(self.api, self.sources, self.current, 42, self.policy,
                                         timeout=31, clock=lambda: elapsed[0], sleep=sleep)
                self.assertEqual(result[0], "native")
                self.assertEqual(elapsed[0], 31)

    def test_fresh_disabled_or_changed_inputs_do_not_wait_for_authority(self):
        def no_sleep(_):
            self.fail("no reusable candidate or disabled/fresh authority must not wait")
        for kwargs in [{"fresh": True}, {"app_id": None}]:
            self.assertEqual(gate.coordinate(self.api, self.sources, self.current, kwargs.get("app_id", 42),
                                             self.policy, fresh=kwargs.get("fresh", False), sleep=no_sleep)[0], "native")
        self.advance("Cargo.toml", "changed")
        self.assertEqual(gate.coordinate(self.api, self.sources, self.current, 42, self.policy, sleep=no_sleep)[0], "native")

    def test_authority_wait_rejects_superseded_source(self):
        path = f"/commits/{self.head}/check-runs?check_name=Windows%20Product%20%2F%20trusted%20acceptance"
        self.api.data[path] = {"check_runs": []}
        elapsed = [0]
        def sleep(seconds):
            elapsed[0] += seconds
            self.api.data["/pulls/131"]["head"]["sha"] = "f" * 40
        with self.assertRaisesRegex(gate.Rejected, "superseded"):
            gate.coordinate(self.api, self.sources, self.current, 42, self.policy,
                            timeout=31, clock=lambda: elapsed[0], sleep=sleep)

    def test_stale_forged_and_expired_leases_cannot_end_authority_wait(self):
        for mutation in [lambda: self.check["app"].update(id=43), lambda: self.lease.update(expires=0),
                         lambda: self.lease["current"].update(source=self.head)]:
            self.publish_current()
            mutation()
            self.check["output"]["text"] = json.dumps(self.lease)
            elapsed = [0]
            def sleep(seconds):
                elapsed[0] += seconds
            self.assertEqual(gate.coordinate(self.api, self.sources, self.current, 42, self.policy,
                                              timeout=16, clock=lambda: elapsed[0], sleep=sleep)[0], "native")
            self.assertEqual(elapsed[0], 16)

    def test_authority_wait_rejects_unbounded_budget(self):
        with self.assertRaisesRegex(gate.Rejected, "budget"):
            gate.coordinate(self.api, self.sources, self.current, 42, self.policy, timeout=421)

    def test_docs_and_fast_only_changes_reuse(self):
        for path in ["docs/windows-product-gate-handoff.md", ".github/workflows/ci.yml"]:
            self.advance(path, "new comments")
            self.assertEqual(self.decide(), "reuse")

    def test_native_dependency_toolchain_security_distribution_changes_rerun(self):
        for path in ["src-tauri/src/main.rs", "test/ipc.test.mjs", "crates/security/src/lib.rs", "package-lock.json",
                     "rust-toolchain.toml", "scripts/package.mjs", "extension/worker.mjs", "docs/guide.md"]:
            self.advance(path, "new native input")
            self.assertEqual(self.decide(), "native", path)

    def test_expected_step_name_with_altered_command_rejected(self):
        path = self.repo / gate.WORKFLOW
        self.advance(gate.WORKFLOW, path.read_text().replace("npm test", "echo pass"))
        with self.assertRaisesRegex(gate.Rejected, "execution contract"):
            self.decide()

    def test_modified_verifier_cannot_self_qualify(self):
        self.advance("scripts/windows_product_gate.py", "print('success')")
        with self.assertRaisesRegex(gate.Rejected, "execution contract"):
            self.decide()

    def test_required_steps_cannot_skip_fail_cancel_or_duplicate(self):
        for step in self.steps:
            for conclusion in ["skipped", "failure", "cancelled", None]:
                with self.subTest(step=step["name"], conclusion=conclusion):
                    step["conclusion"] = conclusion
                    with self.assertRaises(gate.Rejected):
                        self.verify()
            step["conclusion"] = "success"
        self.steps.append(self.steps[2].copy())
        with self.assertRaises(gate.Rejected):
            self.verify()

    def test_workflow_success_but_windows_job_skipped_rejected(self):
        job = self.api.data["/actions/runs/7/attempts/1/jobs"]["jobs"][0]
        job.update(conclusion="skipped", steps=[])
        with self.assertRaises(gate.Rejected):
            self.verify()

    def test_failed_cancelled_pending_and_old_runs_rejected(self):
        for key, value in [("conclusion", "failure"), ("conclusion", "cancelled"), ("status", "in_progress"),
                           ("updated_at", "2020-01-01T00:00:00Z"), ("event", "push"), ("head_sha", "a" * 40),
                           ("path", ".github/workflows/ci.yml"), ("pull_requests", [])]:
            previous = self.run[key]
            self.run[key] = value
            with self.subTest(key=key), self.assertRaises((gate.Rejected, subprocess.CalledProcessError)):
                self.verify()
            self.run[key] = previous

    def test_artifact_and_attempt_provenance(self):
        for key, value in [("expired", True), ("workflow_run", {"id": 999}), ("name", "wrong")]:
            previous = self.artifact[key]
            self.artifact[key] = value
            with self.subTest(key=key), self.assertRaises(gate.Rejected):
                self.verify()
            self.artifact[key] = previous
        self.run["run_attempt"] = 2
        with self.assertRaises(gate.Rejected):
            self.verify()

    def test_invalid_foreign_and_corrupt_proof(self):
        for key, value in [("schema", "unknown"), ("repository", "attacker/foreign"), ("pr", 132),
                           ("run_id", 8), ("attempt", 2), ("fingerprint", "fake"), ("source", "bad")]:
            previous = self.proof[key]
            self.proof[key] = value
            self.pack()
            with self.subTest(key=key), self.assertRaises(gate.Rejected):
                self.verify()
            self.proof[key] = previous
        self.pack({"../proof.json": "{}"})
        with self.assertRaises(gate.Rejected):
            self.verify()
        self.api.data["/actions/artifacts/8/zip"] = b"corrupt"
        self.assertEqual(self.decide(), "native")

    def test_missing_proof_draft_to_ready_requires_native(self):
        self.api.data["/actions/runs/7/artifacts"] = {"artifacts": []}
        self.assertEqual(self.decide(), "native")

    def test_foreign_pr_and_draft_snapshot_rejected(self):
        self.api.data["/pulls/131"]["head"]["repo"]["full_name"] = "attacker/fork"
        with self.assertRaises(gate.Rejected):
            gate.snapshot(self.api, 131)
        self.publish_current()
        self.api.data["/pulls/131"]["draft"] = True
        with self.assertRaises(gate.Rejected):
            gate.snapshot(self.api, 131)

    def test_stale_merge_and_pr_head_are_not_equivalent(self):
        self.current["source"] = self.head
        with self.assertRaisesRegex(gate.Rejected, "merge parents"):
            self.decide()

    def test_stale_or_foreign_app_lease_never_authorizes_reuse(self):
        for mutate in [lambda: self.lease.update(expires=0), lambda: self.lease.update(anchor_enforced=False),
                       lambda: self.lease["current"].update(source=self.head), lambda: self.check["app"].update(id=43)]:
            self.publish_current()
            mutate()
            self.check["output"]["text"] = json.dumps(self.lease)
            self.assertEqual(self.decide(), "native")

    def test_unenforced_anchor_cannot_reuse_or_accept(self):
        self.assertEqual(self.decide(app_id=None), "native")
        self.config["enabled"] = False
        gate.controller(self.api, self.sources, self.policy, self.config)
        self.assertFalse(self.api.writes)
        self.config["enabled"] = True
        self.api.data["/branches/main/protection"]["required_status_checks"]["checks"][0]["app_id"] = 99
        with self.assertRaises(gate.Rejected):
            gate.enforcement(self.api, self.current, self.config)

    def test_api_failure_schedules_native_and_controller_fails_closed(self):
        path = "/actions/workflows/windows-product.yml/runs?status=success"
        self.api.data[path] = urllib.error.URLError("unavailable")
        self.assertEqual(self.decide(), "native")
        gate.controller(self.api, self.sources, self.policy, self.config)
        self.assertEqual(self.api.writes[-1][1]["conclusion"], "failure")

    def test_manual_fresh_never_reuses_automatic_proof(self):
        self.assertEqual(self.decide(fresh=True), "native")
        self.assertIsNone(gate.find_proof(self.api, self.sources, self.current, self.identity, self.policy, fresh=True))

    def test_concurrent_same_inputs_recheck_after_slot(self):
        self.api.data["/actions/workflows/windows-product.yml/runs?status=success"]["workflow_runs"] = []
        self.assertEqual(self.decide(), "native")
        self.api.data["/actions/workflows/windows-product.yml/runs?status=success"]["workflow_runs"] = [self.run]
        self.assertEqual(self.decide(), "reuse")

    def test_controller_binds_acceptance_to_current_merge(self):
        self.advance("docs/windows-product-gate-handoff.md", "new docs")
        gate.controller(self.api, self.sources, self.policy, self.config)
        payload = self.api.writes[-1][1]
        self.assertEqual(payload["conclusion"], "success")
        receipt = json.loads(payload["output"]["text"])
        self.assertEqual(receipt["outcome"], "VERIFIED_REUSE")
        self.assertEqual(receipt["current"], self.current)

    def test_push_during_independent_verification_rejects(self):
        with patch.object(gate, "snapshot", side_effect=[self.current, {**self.current, "head": "f" * 40}]):
            gate.controller(self.api, self.sources, self.policy, self.config)
        self.assertEqual(self.api.writes[-1][1]["conclusion"], "failure")

    def test_mixed_history_uses_effective_merge_inputs(self):
        self.advance("Cargo.toml", "changed dependency")
        self.advance("docs/windows-product-gate-handoff.md", "docs on changed product")
        self.assertEqual(self.decide(), "native")

    def test_worker_plan_slot_and_proof_execute_production_cli(self):
        # Actual CLI event/output integration, with only transport substituted.
        event = self.root / "event.json"
        event.write_text(json.dumps({"number": 131, "pull_request": {
            "head": {"sha": self.head}, "base": {"sha": self.base, "ref": "main"}}}))
        output = self.root / "output"
        env = {"GITHUB_EVENT_PATH": str(event), "GITHUB_EVENT_NAME": "pull_request", "GITHUB_REPOSITORY": API.repo,
               "GITHUB_SHA": self.source, "GITHUB_OUTPUT": str(output), "GITHUB_RUN_ID": "7", "GITHUB_RUN_ATTEMPT": "1",
               "EXPECTED_SOURCE": self.source, "WINDOWS_PRODUCT_APP_ID": "42"}
        previous = os.getcwd()
        self.addCleanup(os.chdir, previous)
        os.chdir(self.repo)
        self.git("checkout", "-q", self.source)
        with patch.dict(os.environ, env), patch.object(gate, "GitHub", return_value=self.api):
            for command in ["plan", "slot", "proof"]:
                with patch.object(sys, "argv", ["gate", command, "--policy-root", str(self.policy)]):
                    gate.main()
            self.write("Cargo.toml", "unexpected workspace mutation")
            with patch.object(sys, "argv", ["gate", "proof", "--policy-root", str(self.policy)]):
                with self.assertRaises(subprocess.CalledProcessError):
                    gate.main()
            self.git("restore", "Cargo.toml")
            self.advance("docs/windows-product-gate-handoff.md", "push during native execution")
            self.git("checkout", "-q", self.source)
            with patch.object(sys, "argv", ["gate", "plan", "--policy-root", str(self.policy)]):
                with self.assertRaisesRegex(gate.Rejected, "superseded PR event"):
                    gate.main()
            with patch.object(sys, "argv", ["gate", "proof", "--policy-root", str(self.policy)]):
                gate.main()  # A running older qualification can finish safely.
        self.assertIn("mode=reuse", output.read_text())
        self.assertEqual(json.loads((self.repo / "qualification/proof.json").read_text())["source"], self.source)

    def test_unreviewed_native_code_cannot_self_accept_even_with_valid_run(self):
        self.config["approved_fingerprints"] = []
        gate.controller(self.api, self.sources, self.policy, self.config)
        self.assertFalse(self.api.writes)

    def test_foreign_pr_cannot_abort_other_pr_verification(self):
        self.api.data["/pulls?state=open"].insert(0, {"number": 132, "draft": False})
        self.api.data["/pulls/132"] = copy.deepcopy(self.api.data["/pulls/131"])
        self.api.data["/pulls/132"]["head"]["repo"]["full_name"] = "attacker/fork"
        gate.controller(self.api, self.sources, self.policy, self.config)
        self.assertEqual(self.api.writes[-1][1]["conclusion"], "success")

    def test_fake_merge_tree_with_real_parents_rejected(self):
        self.write("Cargo.toml", "forged tested input")
        fake = self.commit()
        self.current["source"] = self.git("commit-tree", self.git("rev-parse", fake + "^{tree}"),
                                            "-p", self.base, "-p", self.head, "-m", "fake merge")
        with self.assertRaisesRegex(gate.Rejected, "reconstructed PR merge"):
            self.decide()

    def test_idempotent_controller_reuses_owned_check(self):
        self.check["id"] = 99
        gate.controller(self.api, self.sources, self.policy, self.config)
        self.assertEqual(self.api.writes[0][0], "/check-runs/99")
        self.assertNotIn("head_sha", self.api.writes[0][1])
        self.assertEqual(self.api.writes[0][2], "PATCH")

    def test_policy_revocation_invalidates_existing_success(self):
        self.check.update(id=99, status="completed", conclusion="success")
        self.config["approved_fingerprints"] = []
        gate.controller(self.api, self.sources, self.policy, self.config)
        self.assertEqual(self.api.writes[-1][0], "/check-runs/99")
        self.assertEqual(self.api.writes[-1][1]["conclusion"], "failure")

    def test_final_policy_rejects_old_manual_proof(self):
        self.config.update(require_fresh=True, fresh_after="2099-01-01T00:00:00Z")
        self.run.update(event="workflow_dispatch", created_at="2026-10-29T01:00:00Z")
        gate.controller(self.api, self.sources, self.policy, self.config)
        self.assertEqual(self.api.writes[-1][1]["status"], "in_progress")

    def test_recent_valid_proof_does_not_require_unbounded_historical_pages(self):
        real = gate.GitHub(API.repo, "fixture")
        calls = []
        def wire(path, *args, **kwargs):
            if path.startswith("/actions/workflows/windows-product.yml/runs?"):
                calls.append(path)
                if "page=1" in path:
                    return {"workflow_runs": [self.run] * 100, "total_count": 2500}
                self.fail("valid recent proof must return before requesting older pages")
            return self.api.request(path.split("?per_page=", 1)[0], *args, **kwargs)
        with patch.object(real, "request", side_effect=wire):
            proof = gate.find_proof(real, self.sources, self.current, self.identity, self.policy)
        self.assertEqual(proof["run_id"], 7)
        self.assertEqual(len(calls), 1)

    def test_finalizing_predecessor_is_not_evidence_until_whole_workflow_completes(self):
        self.run.update(status="in_progress", conclusion=None)
        self.assertEqual(self.decide(), "native")
        self.run.update(status="completed", conclusion="success")
        self.assertEqual(self.decide(), "reuse")


    def test_existing_success_with_expired_lease_remains_green_in_github_model(self):
        self.check.update(id=99, status="completed", conclusion="success")
        self.lease["expires"] = 0
        self.check["output"]["text"] = json.dumps(self.lease)
        self.assertEqual(gate.lease_state(self.api, self.current, self.identity, 42), "pending")
        self.assertEqual(self.check["conclusion"], "success")
        self.assertFalse(self.api.writes)  # Lease evaluation is not GitHub revocation.

    def test_fingerprint_revoked_while_controller_down_cannot_retroactively_patch(self):
        self.check.update(id=99, status="completed", conclusion="success")
        self.config["approved_fingerprints"] = []
        self.assertEqual(self.check["conclusion"], "success")
        self.assertFalse(self.api.writes)  # No controller; native decision != old GitHub status.

    def test_wrong_app_success_cannot_be_revoked_as_own_and_cannot_satisfy_app_anchor(self):
        self.check.update(id=99, status="completed", conclusion="success")
        self.check["app"]["id"] = 43
        self.config["approved_fingerprints"] = []
        gate.controller(self.api, self.sources, self.policy, self.config)
        self.assertEqual(self.check["conclusion"], "success")
        self.assertFalse(self.api.writes)  # Wrong App success remains but App 42 is required.

    def test_failed_api_revocation_leaves_previous_success_unchanged(self):
        self.check.update(id=99, status="completed", conclusion="success")
        self.config["approved_fingerprints"] = []
        original = self.api.request
        def unavailable_write(path, data=None, method=None, binary=False):
            if data is not None:
                raise urllib.error.URLError("GitHub API write unavailable")
            return original(path, data, method, binary)
        with patch.object(self.api, "request", side_effect=unavailable_write):
            with self.assertRaises(urllib.error.URLError):
                gate.controller(self.api, self.sources, self.policy, self.config)
        self.assertEqual(self.check["conclusion"], "success")
        self.assertFalse(self.api.writes)

    def test_policy_revision_without_contract_reapproval_rejects_new_evaluation(self):
        self.advance(".github/workflows/windows-product.yml", "name: modified dangerous execution")
        with self.assertRaisesRegex(gate.Rejected, "execution contract"):
            gate.validate_source(self.sources, self.current, self.policy)

    def test_controller_restart_requires_explicit_reconciliation_not_lease_magic(self):
        self.check.update(id=99, status="completed", conclusion="success")
        self.lease["expires"] = 0
        self.check["output"]["text"] = json.dumps(self.lease)
        self.assertEqual(gate.lease_state(self.api, self.current, self.identity, 42), "pending")
        self.assertFalse(self.api.writes)
        self.assertEqual(self.check["conclusion"], "success")  # Still green until App writes.


if __name__ == "__main__":
    unittest.main()
