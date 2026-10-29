"""Contract tests for real Actions control flow; runtime verifier needs only stdlib."""
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
import re
import unittest

import yaml

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
import mp10c_ci_gate as scale
import mp10c_prerequisite_wait as coordinator


class WorkflowTests(unittest.TestCase):
    def setUp(self):
        self.workflow = yaml.safe_load((ROOT / "policy/contracts/.github/workflows/windows-product.yml").read_text())
        self.jobs = self.workflow["jobs"]
        self.steps = self.jobs["verify"]["steps"]

    def test_preserves_every_native_command_from_previous_ci(self):
        commands = "\n".join(step.get("run", "") for step in self.steps)
        for command in ["npm ci", "cargo fmt --all -- --check", "npm run verify:repo", "npm run typecheck",
                        "node scripts/verify-settings-render.mjs", "cargo test -p mind-place-state bundled_sqlite_version_is_supported -- --nocapture",
                        "npm test", "cargo build -p mind-place-capture-host", "node scripts/tunnel-helper.mjs",
                        "node scripts/generate-notices.mjs", "npm run build:web",
                        "cargo test -p mind-place-integration --test mcp -- --nocapture",
                        "cargo test -p mind-place --bin mind-place hosted_tunnel::tests -- --nocapture",
                        "npm run build", "npm run verify:distribution-license"]:
            self.assertIn(command, commands)

    def test_all_expensive_steps_and_proof_guarded_by_slot_recheck(self):
        self.assertEqual(self.steps[0]["name"], "Checkout")
        self.assertEqual(self.steps[1]["id"], "slot")
        self.assertIn(" slot", self.steps[1]["run"])
        for step in self.steps[2:]:
            self.assertIn("steps.slot.outputs.native == 'true'", step["if"])
        self.assertEqual(self.steps[-2]["name"], "Record native proof")
        self.assertEqual(self.steps[-1]["name"], "Upload native proof")
        self.assertEqual(self.jobs["verify"]["if"], "needs.plan.outputs.mode == 'native' && needs.coordinate.outputs.mode == 'native'")
        self.assertFalse(self.workflow["concurrency"]["cancel-in-progress"])
        self.assertIn("pull_request.number", self.workflow["concurrency"]["group"])
        self.assertIn("inputs.pr_number", self.workflow["concurrency"]["group"])
        self.assertNotIn("concurrency", self.jobs["verify"])
        # This lock includes candidate, so the successor plan cannot start while
        # predecessor proof exists but the containing workflow is still pending.
        self.assertIn("verify", self.jobs["candidate"]["needs"])

    def test_trusted_required_steps_exactly_cover_real_native_workflow(self):
        contract = json.loads((ROOT / "policy/contracts/.github/windows-policy/native-steps.json").read_text())
        names = contract["common"] + contract["vault"]
        self.assertCountEqual(names, [step["name"] for step in self.steps])
        self.assertEqual(len(set(names)), len(names))

    def test_read_only_pinned_actions_no_persisted_checkout_credentials(self):
        self.assertEqual(set(self.workflow["permissions"].values()), {"read"})
        self.assertNotIn("pull_request_target", str(self.workflow))
        for job in self.jobs.values():
            for step in job["steps"]:
                if "uses" in step:
                    self.assertRegex(step["uses"], r"@[0-9a-f]{40}$")
                    if step["uses"].startswith("actions/checkout@"):
                        self.assertIs(step["with"]["persist-credentials"], False)
        self.assertNotIn("secrets.", str(self.workflow))

    def test_draft_and_skipped_native_are_explicitly_pending_or_fail(self):
        self.assertIn("draft == false", self.jobs["plan"]["if"])
        self.assertEqual(self.jobs["candidate"]["if"], "always()")
        report = self.jobs["candidate"]["steps"][0]["run"]
        self.assertIn("PENDING: Draft", report)
        self.assertIn("skipped without proof'; exit 1", report)
        self.assertIn("independent App acceptance is required", report)

    def test_fast_workflow_cannot_trigger_native_build(self):
        fast = yaml.safe_load((ROOT / "policy/contracts/.github/workflows/ci.yml").read_text())
        self.assertEqual(set(fast["jobs"]), {"fast", "rust-fast"})
        self.assertNotIn("windows-latest", str(fast))

    def test_authority_template_never_checks_out_target_or_exposes_app_to_it(self):
        policy = yaml.safe_load((ROOT / ".github/windows-policy/verifier.yml").read_text())
        steps = policy["jobs"]["verify"]["steps"]
        checkout = [step for step in steps if step.get("uses", "").startswith("actions/checkout@")]
        self.assertEqual(len(checkout), 1)
        self.assertNotIn("repository", checkout[0]["with"])
        self.assertEqual(checkout[0]["with"]["ref"], "${{ vars.WINDOWS_POLICY_COMMIT }}")
        self.assertEqual(policy["jobs"]["verify"]["environment"], "windows-policy-authority")
        self.assertNotIn("pull_request", str(policy.get("on", policy.get(True))))

    def test_mp10c_requires_actual_exact_source_native_in_separated_workflow(self):
        source = "a" * 40
        product_path = ".github/workflows/windows-product.yml"
        fast = {"id": 1, "run_attempt": 1, "head_sha": source, "status": "completed", "conclusion": "success",
                "path": ".github/workflows/ci.yml"}
        native = {**fast, "id": 2, "path": product_path}
        rust = yaml.safe_load((ROOT / "policy/contracts/.github/workflows/ci.yml").read_text())["jobs"]["rust-fast"]
        def job(name, steps):
            return {"name": name, "status": "completed", "conclusion": "success", "steps": [
                {"name": step["name"], "conclusion": "success"} for step in steps if "name" in step]}
        jobs = {1: [job(rust["name"], rust["steps"])],
                2: [job(self.jobs["verify"]["name"], self.steps)]}
        def fetch(run):
            return {"total_count": len(jobs[run["id"]]), "jobs": jobs[run["id"]]}
        self.assertEqual(scale.product_workflow(), product_path)
        scale.require_prerequisites([fast, native], source, vault=False, fetch_jobs=fetch, product_path=product_path)
        self.assertEqual(coordinator.classify([fast, native], source, False, fetch, product_path)[0], "ready")
        self.assertEqual(coordinator.classify([fast], source, False, fetch, product_path)[0], "pending")
        self.assertEqual(coordinator.classify([fast, {**native, "status": "in_progress"}], source, False, fetch, product_path)[0], "pending")
        jobs[2][0]["conclusion"] = "skipped"
        self.assertEqual(coordinator.classify([fast, native], source, False, fetch, product_path)[0], "failed")
        with self.assertRaises(ValueError):
            scale.require_prerequisites([fast, native], source, vault=False, fetch_jobs=fetch, product_path=product_path)

    def test_mp9b_vault_integration_keeps_encrypted_native_ipc_prerequisite(self):
        source = "a" * 40
        product_path = ".github/workflows/windows-product.yml"
        def run(identifier, path):
            return {"id": identifier, "run_attempt": 1, "head_sha": source, "status": "completed",
                    "conclusion": "success", "path": path}
        rust = yaml.safe_load((ROOT / "policy/contracts/.github/workflows/ci.yml").read_text())["jobs"]["rust-fast"]
        def job(name, names):
            return {"name": name, "status": "completed", "conclusion": "success",
                    "steps": [{"name": n, "conclusion": "success"} for n in names]}
        jobs = {1: [job(rust["name"], [s["name"] for s in rust["steps"] if "name" in s])],
                2: [job(self.jobs["verify"]["name"], [s["name"] for s in self.steps])],
                3: [job("SQLCipher / " + platform, ["Encrypted state/index and FTS regression",
                      "Keyed MCP synthetic authorization and revocation", "Vault and backup adversarial regression"])
                    for platform in ["ubuntu-24.04", "windows-2022"]]}
        runs = [run(1, ".github/workflows/ci.yml"), run(2, product_path),
                run(3, ".github/workflows/mp9b-sqlcipher-qualification.yml")]
        def fetch(candidate):
            return {"total_count": len(jobs[candidate["id"]]), "jobs": jobs[candidate["id"]]}
        scale.require_prerequisites(runs, source, vault=True, fetch_jobs=fetch, product_path=product_path)
        self.assertEqual(coordinator.classify(runs, source, True, fetch, product_path)[0], "ready")
        ipc_name = "Qualify native capture encrypted IPC and revoked owner epoch"
        ipc = next(step for step in self.steps if step["name"] == ipc_name)
        self.assertIn("cargo test -p mind-place --bin mind-place native_capture_ipc::qualification -- --nocapture", ipc["run"])
        for outcome in ["skipped", "failure", "cancelled"]:
            next(s for s in jobs[2][0]["steps"] if s["name"] == ipc_name)["conclusion"] = outcome
            with self.assertRaises(ValueError):
                scale.require_prerequisites(runs, source, vault=True, fetch_jobs=fetch, product_path=product_path)

    @unittest.skipUnless(shutil.which("pwsh"), "PowerShell unavailable in this cloud shell; exercised on hosted CI")
    def test_native_ipc_powershell_step_cannot_pass_with_zero_missing_or_failing_tests(self):
        ipc = next(step for step in self.steps if step["name"] == "Qualify native capture encrypted IPC and revoked owner epoch")
        names = ["native_capture_ipc::qualification::separate_process_capture_probe: test",
                 "native_capture_ipc::qualification::owner_local_capture_roundtrip_requires_unlocked_epoch: test"]
        stub = '''function cargo {
          [IO.File]::AppendAllText($env:IPC_COMMAND_LOG, ($args -join ' ') + "`n")
          if ($args -contains '--list') {
            $env:IPC_LIST_JSON | ConvertFrom-Json | Write-Output
            $global:LASTEXITCODE = [int]$env:IPC_LIST_EXIT
          } else { $global:LASTEXITCODE = [int]$env:IPC_TEST_EXIT }
        }
        '''
        for listing, list_exit, test_exit, passed, count in [
            (names, 0, 0, True, 2), (names[:1], 0, 0, False, 1), ([], 0, 0, False, 1),
            (names, 1, 0, False, 1), (names, 0, 1, False, 2)]:
            with self.subTest(listing=listing, list_exit=list_exit, test_exit=test_exit), tempfile.TemporaryDirectory() as directory:
                script, log = Path(directory) / "qualify.ps1", Path(directory) / "commands.log"
                # Match GitHub's pwsh wrapper while executing the actual workflow
                # script. Only Cargo transport is substituted; no Rust is built.
                script.write_text("$ErrorActionPreference = 'Stop'\n" + stub + ipc["run"] + "\nexit $LASTEXITCODE\n")
                env = {**os.environ, "IPC_COMMAND_LOG": str(log), "IPC_LIST_JSON": json.dumps(listing),
                       "IPC_LIST_EXIT": str(list_exit), "IPC_TEST_EXIT": str(test_exit)}
                result = subprocess.run(["pwsh", "-NoProfile", "-NonInteractive", "-File", str(script)],
                                        env=env, capture_output=True, text=True, timeout=30)
                self.assertEqual(result.returncode == 0, passed, result.stdout + result.stderr)
                commands = log.read_text().splitlines()
                self.assertEqual(len(commands), count)
                if count == 2:
                    # PowerShell functions consume the '--' argument separator;
                    # the real native-command spelling is checked above. This
                    # substitute exercises qualification control flow only.
                    self.assertEqual(commands[-1], "test -p mind-place --bin mind-place native_capture_ipc::qualification --nocapture")


if __name__ == "__main__":
    unittest.main()
