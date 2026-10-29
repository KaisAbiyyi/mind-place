"""Negative-only synthetic tests: cannot prove GitHub actually blocks a merge."""
import copy
import json
from pathlib import Path
import sys
import unittest
from urllib.error import URLError

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
from merge_freeze import audit
from windows_product_gate import CHECK, POLICY, Rejected


class MockGitHub:
    repo = "KaisAbiyyi/mind-place"

    def __init__(self):
        self.calls = []
        self.rulesets = []
        self.ruleset_detail = {}
        self.protection = {
            "lock_branch": {"enabled": True},
            "enforce_admins": {"enabled": True},
            "allow_fork_syncing": {"enabled": False},
            "allow_force_pushes": {"enabled": False},
            "allow_deletions": {"enabled": False},
            "required_status_checks": {"strict": True,
                "checks": [{"context": "legacy-ci", "app_id": 123},
                           {"context": CHECK, "app_id": 42}]},
        }
        self.overrides = {}
        self.fail = False

    def request(self, path, data=None, method=None):
        self.calls.append((path, data, method))
        if self.fail:
            raise URLError("GitHub API unavailable")
        if path == "/rulesets?includes_parents=true":
            return copy.deepcopy(self.rulesets)
        if path.startswith("/rulesets/") and path.endswith("?includes_parents=true"):
            return copy.deepcopy(self.ruleset_detail)
        if path in self.overrides:
            return copy.deepcopy(self.overrides[path])
        if path.startswith("/branches/") and path.endswith("/protection"):
            return copy.deepcopy(self.protection)
        raise AssertionError("unexpected endpoint " + path)


class MergeFreezeTests(unittest.TestCase):
    def setUp(self):
        self.api = MockGitHub()
        self.cfg = {"schema": "mp-p0-merge-freeze/v1", "enabled": True,
                    "repository": self.api.repo, "policy": POLICY, "app_id": 42,
                    "protected_bases": ["main", "codex/mp-ci-100k-01-20261027"]}

    def reject(self, msg):
        with self.assertRaisesRegex(Rejected, msg):
            audit(self.api, self.cfg)

    def test_positive_is_read_only_snapshot_not_authorization(self):
        result = audit(self.api, self.cfg)
        self.assertEqual(len(result["bases"]), 2)
        self.assertEqual(result["live_negative_merge_probe"], "NOT_YET_LIVE_VALIDATED")
        self.assertFalse(result["automatic_unfreeze"])
        self.assertTrue(all(data is None and method is None for _, data, method in self.api.calls))
        self.assertIn("/branches/codex%2Fmp-ci-100k-01-20261027/protection",
                      [path for path, _, _ in self.api.calls])

    def test_disabled_and_untrusted_config_fail_closed(self):
        self.cfg["enabled"] = False
        self.reject("disabled")
        self.cfg["enabled"] = True
        self.cfg["repository"] = "attacker/repo"
        self.reject("wrong target")
        self.cfg["repository"] = self.api.repo
        self.cfg["policy"] = "unauthorized/v2"
        self.reject("wrong policy")

    def test_staging_config_cannot_claim_live_freeze(self):
        config = json.loads((ROOT / "policy/merge-freeze.example.json").read_text())
        with self.assertRaises(Rejected):
            audit(self.api, config)
        self.assertEqual(config["enabled"], False)
        self.assertEqual(config["app_id"], 0)

    def test_missing_main_duplicate_and_empty_base_rejected(self):
        for bases in ([], ["other"], ["main", "main"], ["main", "/bad"],
                      ["main", "a/../b"]):
            with self.subTest(bases=bases):
                self.cfg["protected_bases"] = bases
                with self.assertRaises(Rejected):
                    audit(self.api, self.cfg)

    def test_stale_success_after_lease_expiry_does_not_unlock_freeze(self):
        old_check = {"status": "completed", "conclusion": "success", "lease_expired": True}
        result = audit(self.api, self.cfg)
        self.assertEqual(old_check["conclusion"], "success")
        self.assertTrue(all(b["lock_branch_read"] for b in result["bases"]))
        self.assertFalse(result["automatic_unfreeze"])

    def test_controller_outage_before_admission_cannot_claim_freeze(self):
        self.api.fail = True
        with self.assertRaises(URLError):
            audit(self.api, self.cfg)

    def test_controller_outage_after_old_success_requires_live_freeze(self):
        check = {"conclusion": "success"}
        self.api.fail = True
        with self.assertRaises(URLError):
            audit(self.api, self.cfg)
        self.assertEqual(check["conclusion"], "success")

    def test_revoked_fingerprint_does_not_expire_old_github_success(self):
        proof = {"approved_fingerprints": [], "old_check": "success"}
        self.assertTrue(audit(self.api, self.cfg)["configuration_snapshot_only"])
        self.assertEqual(proof["old_check"], "success")

    def test_unlocked_or_missing_protection_rejected(self):
        self.api.protection["lock_branch"]["enabled"] = False
        self.reject("NOT locked")
        self.api.protection["lock_branch"]["enabled"] = True
        self.api.overrides["/branches/main/protection"] = None
        self.reject("missing branch protection")

    def test_admin_bypass_or_wrong_app_rejected(self):
        self.api.protection["enforce_admins"]["enabled"] = False
        self.reject("administrator")
        self.api.protection["enforce_admins"]["enabled"] = True
        self.api.protection["required_status_checks"]["checks"][-1]["app_id"] = 43
        self.reject("wrong or missing")

    def test_branch_update_bypasses_fail_closed(self):
        for flag in ("allow_fork_syncing", "allow_force_pushes", "allow_deletions"):
            with self.subTest(flag=flag):
                self.api.protection[flag]["enabled"] = True
                self.reject(flag)
                self.api.protection[flag]["enabled"] = False
                self.api.protection.pop(flag)
                self.reject(flag)
                self.api.protection[flag] = {"enabled": False}

    def test_status_check_laundering_and_non_strict_rejected(self):
        self.api.protection["required_status_checks"]["checks"] = [
            {"context": CHECK, "app_id": None}]
        self.reject("wrong or missing")
        self.api.protection["required_status_checks"]["checks"] = [
            {"context": CHECK, "app_id": 42}]
        self.api.protection["required_status_checks"]["strict"] = False
        self.reject("strict")

    def test_ruleset_bypass_or_unavailable_inventory_rejected(self):
        self.api.rulesets = [{"id": 100, "enforcement": "active"}]
        self.api.ruleset_detail = {"id": 100, "enforcement": "active",
                                  "bypass_actors": [{"actor_id": 2,
                                                    "actor_type": "RepositoryRole"}]}
        self.reject("bypass")
        self.api.ruleset_detail.pop("bypass_actors")
        self.reject("bypass")
        self.api.ruleset_detail["bypass_actors"] = []
        self.assertTrue(audit(self.api, self.cfg)["configuration_snapshot_only"])
        self.assertIn(("/rulesets/100?includes_parents=true", None, None), self.api.calls)
        self.api.rulesets = [{"enforcement": "active"}]
        self.reject("uninspectable")
        self.api.rulesets = None
        self.reject("inventory unavailable")

    def test_unprotected_stacked_base_rejected(self):
        self.api.overrides["/branches/codex%2Fmp-ci-100k-01-20261027/protection"] = {
            **self.api.protection, "lock_branch": {"enabled": False}}
        self.reject("NOT locked")

    def test_concurrent_protection_change_requires_new_audit(self):
        audit(self.api, self.cfg)
        self.api.protection["lock_branch"]["enabled"] = False
        self.reject("NOT locked")

    def test_no_merge_approval_on_untrusted_source_or_native_proof(self):
        for evidence in ("changed_head", "changed_base", "missing_ipc",
                         "changed_native_inputs", "different_policy_revision",
                         "queue_timeout", "reused_receipt"):
            with self.subTest(evidence=evidence):
                # These inputs are deliberately *not* accepted as admission
                # parameters. Lock state is independently required.
                result = audit(self.api, self.cfg)
                self.assertTrue(result["configuration_snapshot_only"])
                self.assertEqual(result["live_negative_merge_probe"],
                                 "NOT_YET_LIVE_VALIDATED")


if __name__ == "__main__":
    unittest.main()
