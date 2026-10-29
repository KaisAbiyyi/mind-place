"""Production verifier/decision tests over real Git merge objects and fake API wire data."""
import copy
import datetime as dt
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
import urllib.error
import zipfile

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
                shutil.copyfile(ROOT / name, file)
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
        self.run = {"id": 7, "repository": {"full_name": API.repo}, "path": gate.WORKFLOW,
                    "event": "pull_request", "status": "completed", "conclusion": "success", "run_attempt": 1,
                    "updated_at": gate.utc().isoformat(), "head_sha": self.head,
                    "pull_requests": [{"number": 131, "head": {"sha": self.head}, "base": {"sha": self.base}}]}
        self.proof = {"schema": gate.SCHEMA, "policy": gate.POLICY, "repository": API.repo,
                      **self.current, "fingerprint": self.identity, "run_id": 7, "attempt": 1}
        self.steps = [{"name": name, "status": "completed", "conclusion": "success"}
                      for name in json.loads((ROOT / ".github/windows-policy/native-steps.json").read_text())]
        self.artifact = {"id": 8, "name": "windows-product-proof-7-1", "expired": False, "workflow_run": {"id": 7}}
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
        self.config = {"enabled": True, "repository": API.repo, "app_id": 42, "policy": gate.POLICY}
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
            "head": {"sha": self.head}, "base": {"sha": self.base}}}))
        output = self.root / "output"
        env = {"GITHUB_EVENT_PATH": str(event), "GITHUB_EVENT_NAME": "pull_request", "GITHUB_REPOSITORY": API.repo,
               "GITHUB_SHA": self.source, "GITHUB_OUTPUT": str(output), "GITHUB_RUN_ID": "7", "GITHUB_RUN_ATTEMPT": "1",
               "EXPECTED_SOURCE": self.source, "WINDOWS_PRODUCT_APP_ID": "42"}
        previous = os.getcwd()
        self.addCleanup(os.chdir, previous)
        os.chdir(self.repo)
        with patch.dict(os.environ, env), patch.object(gate, "GitHub", return_value=self.api):
            for command in ["plan", "slot", "proof"]:
                with patch.object(sys, "argv", ["gate", command, "--policy-root", str(self.policy)]):
                    gate.main()
        self.assertIn("mode=reuse", output.read_text())
        self.assertEqual(json.loads((self.repo / "qualification/proof.json").read_text())["source"], self.source)


if __name__ == "__main__":
    unittest.main()
