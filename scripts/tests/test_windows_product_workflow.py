"""Contract tests for real Actions control flow; runtime verifier needs only stdlib."""
import json
from pathlib import Path
import re
import unittest

import yaml

ROOT = Path(__file__).resolve().parents[2]


class WorkflowTests(unittest.TestCase):
    def setUp(self):
        self.workflow = yaml.safe_load((ROOT / ".github/workflows/windows-product.yml").read_text())
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
        self.assertEqual(self.jobs["verify"]["if"], "needs.plan.outputs.mode == 'native'")
        self.assertFalse(self.jobs["verify"]["concurrency"]["cancel-in-progress"])
        self.assertIn("outputs.pr", self.jobs["verify"]["concurrency"]["group"])

    def test_trusted_required_steps_exactly_cover_real_native_workflow(self):
        names = json.loads((ROOT / ".github/windows-policy/native-steps.json").read_text())
        self.assertEqual(names, [step["name"] for step in self.steps])
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
        fast = yaml.safe_load((ROOT / ".github/workflows/ci.yml").read_text())
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


if __name__ == "__main__":
    unittest.main()
