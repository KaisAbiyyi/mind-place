"""Independent security staging invariants; nonactivated policy repository payload."""
import json
from pathlib import Path
import unittest
import yaml

ROOT = Path(__file__).resolve().parents[2]
CONTRACT = ROOT / "policy/contracts"


class StagingBoundaryTests(unittest.TestCase):
    def test_disabled_config(self):
        config = json.loads((ROOT / ".github/windows-policy/config.json").read_text())
        self.assertIs(config["enabled"], False)
        self.assertEqual(config["app_id"], 0)
        self.assertEqual(config["approved_fingerprints"], [])
        self.assertIs(config["require_fresh"], False)

    def test_fingerprint_is_only_proposed(self):
        p = json.loads((ROOT / "policy/proposed-fingerprint.json").read_text())
        self.assertIs(p["approved"], False)
        self.assertIs(p["enabled"], False)
        self.assertRegex(p["input_fingerprint"], r"^[0-9a-f]{64}$")
        self.assertEqual(p["reviewed_head"], "3ab2516ae468e2dfe03bac3b26395db44df2db03")
        self.assertEqual(p["effective_merge_tree"], "4aada364dfa9575d0b74736ab5773e9690c1ecf3")

    def test_controller_template_is_inert(self):
        template = ROOT / ".github/windows-policy/verifier.yml"
        data = yaml.safe_load(template.read_text())
        triggers = data.get("on", data.get(True, {}))
        self.assertNotIn("pull_request", triggers)
        self.assertNotIn("pull_request_target", triggers)
        self.assertEqual(data["jobs"]["verify"]["environment"], "windows-policy-authority")
        self.assertNotIn("repository:", template.read_text())
        self.assertIn("--policy-root policy/contracts", template.read_text())
        self.assertFalse((ROOT / ".github/workflows/verify-windows.yml").exists())

    def test_nonactivation_does_not_remove_required_checks(self):
        doc = (ROOT / "ACTIVATION.md").read_text()
        for word in ("app_id", "strict", "administrators", "stale", "merge-freeze"):
            self.assertIn(word, doc)

    def test_exact_native_ipc_guards(self):
        wf = (CONTRACT / ".github/workflows/windows-product.yml").read_text()
        self.assertIn("native_capture_ipc::qualification::separate_process_capture_probe: test", wf)
        self.assertIn("native_capture_ipc::qualification::owner_local_capture_roundtrip_requires_unlocked_epoch: test", wf)
        self.assertIn("-- --list", wf)

    def test_controller_uses_independent_app(self):
        wf = (ROOT / ".github/windows-policy/verifier.yml").read_text()
        for term in ("permission-checks: write", "permission-administration: read",
                     "WINDOWS_PRODUCT_APP_PRIVATE_KEY", "vars.WINDOWS_POLICY_COMMIT"):
            self.assertIn(term, wf)
        self.assertNotIn("pull_request_target:", wf)

    def test_stale_success_risk_must_be_documented(self):
        sec = (ROOT / "SECURITY.md").read_text()
        self.assertIn("previously successful check", sec)
        self.assertIn("do not honor the lease timestamp", sec)
        self.assertIn("controller outage", sec)
        self.assertIn("P0 OPEN", sec)

    def test_no_deployed_credential(self):
        for path in (ROOT / ".github/windows-policy/config.json", ROOT / "policy/proposed-fingerprint.json"):
            data = path.read_text()
            self.assertNotIn("-----BEGIN", data)
            self.assertNotIn("ghp_", data)
            self.assertNotIn("github_pat_", data)


if __name__ == "__main__":
    unittest.main()
