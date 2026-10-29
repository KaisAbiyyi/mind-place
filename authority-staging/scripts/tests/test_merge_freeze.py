"""Negative-only synthetic tests: cannot prove GitHub actually blocks a merge."""
import copy
import json
from pathlib import Path
import sys
import unittest
from urllib.error import URLError

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
from merge_freeze import audit, InspectionGitHub
from windows_product_gate import CHECK, POLICY, Rejected, GitHub


class MockGitHub(GitHub):
    repo = "KaisAbiyyi/mind-place"

    def __init__(self):
        self.calls = []
        self.rulesets = []
        self.ruleset_detail = {}
        self.ruleset_pages = None
        self.effective = []
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
        if path == "/":
            return {"id": 1397406746, "full_name": self.repo,
                    "owner": {"login": "KaisAbiyyi", "type": "User"}}
        if path.startswith("/rulesets?includes_parents=true&per_page=100&page="):
            page = int(path.rsplit("=", 1)[1])
            if self.ruleset_pages is not None:
                return copy.deepcopy(self.ruleset_pages.get(page, []))
            return copy.deepcopy(self.rulesets if page == 1 else [])
        if path.startswith("/rules/branches/"):
            return copy.deepcopy(self.effective)
        if path == "/rulesets?includes_parents=true":
            return copy.deepcopy(self.rulesets)
        if path.startswith("/rulesets/") and path.endswith("?includes_parents=true"):
            return copy.deepcopy(self.ruleset_detail.get(int(path.split("/")[2].split("?")[0]), self.ruleset_detail))
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

    def ruleset(self, rid=100, state="active", source_type="Repository", source=None):
        summary = {"id": rid, "name": "freeze", "enforcement": state,
                   "source_type": source_type, "source": source or self.api.repo,
                   "target": "branch"}
        detail = {**summary, "bypass_actors": [],
                  "conditions": {"ref_name": {"include": ["~ALL"], "exclude": []}},
                  "rules": [{"type": "update"}]}
        self.api.ruleset_detail[rid] = detail
        return summary

    def test_empty_complete_inventory_and_inactive_rulesets(self):
        self.api.rulesets = [self.ruleset(state="disabled"), self.ruleset(101, "evaluate")]
        audit(self.api, self.cfg)
        self.assertFalse(any(p.startswith("/rulesets/") for p, _, _ in self.api.calls))

    def multipage(self):
        first = [self.ruleset(rid) for rid in range(1, 101)]
        second = [self.ruleset(101)]
        self.api.ruleset_pages = {1: first, 2: second}

    def test_multiple_pages_inspect_every_active_detail(self):
        self.multipage()
        audit(self.api, self.cfg)
        for rid in range(1, 102):
            self.assertIn((f"/rulesets/{rid}?includes_parents=true", None, None), self.api.calls)

    def test_bypass_on_page_two_rejected(self):
        self.multipage()
        self.api.ruleset_detail[101]["bypass_actors"] = [{"actor_type": "RepositoryRole"}]
        self.reject("bypass")

    def test_malformed_pages_and_pagination_truncation(self):
        for bad in (None, {}, "", [None], [self.ruleset()] * 101):
            with self.subTest(bad=bad):
                self.api.rulesets = bad
                with self.assertRaises(Rejected):
                    audit(self.api, self.cfg)
        # Twenty full, individually valid pages must never count as complete.
        self.api.ruleset_pages = {page: [self.ruleset((page-1)*100+i)
            for i in range(1, 101)] for page in range(1, 21)}
        self.reject("pagination limit")

    def test_missing_bypass_or_incomplete_detail_rejected(self):
        for key in ("bypass_actors", "id", "enforcement", "source", "source_type",
                    "target", "conditions", "rules", "name"):
            with self.subTest(key=key):
                summary = self.ruleset()
                self.api.rulesets = [summary]
                self.api.ruleset_detail[100].pop(key)
                with self.assertRaises(Rejected):
                    audit(self.api, self.cfg)

    def test_malformed_ids_unknown_enforcement_and_duplicates(self):
        for rid in (None, True, "100", 0, -1):
            self.api.rulesets = [{**self.ruleset(), "id": rid}]
            with self.assertRaises(Rejected):
                audit(self.api, self.cfg)
        for state in (None, "unknown", "ACTIVE"):
            self.api.rulesets = [self.ruleset(state=state)]
            self.reject("enforcement")
        for state in ("active", "disabled"):
            first = self.ruleset()
            self.api.rulesets = [first, {**first, "enforcement": state}]
            self.reject("duplicate")

    def test_rate_limit_authorization_and_later_page_api_failures(self):
        from urllib.error import HTTPError
        from unittest.mock import patch
        original = self.api.request
        self.multipage()
        for code in (403, 429, 500):
            def request(path, code=code):
                if "page=2" in path:
                    raise HTTPError(path, code, "unavailable", {}, None)
                return original(path)
            with patch.object(self.api, "request", side_effect=request):
                with self.assertRaises(HTTPError):
                    audit(self.api, self.cfg)

    def test_repository_and_effective_branch_applicability_crosscheck(self):
        self.api.rulesets = [self.ruleset()]
        self.api.effective = [{"type": "update", "ruleset_id": 100,
                               "ruleset_source_type": "Repository",
                               "ruleset_source": self.api.repo}]
        result = audit(self.api, self.cfg)
        self.assertEqual(result["bases"][0]["effective_ruleset_ids"], [100])
        for field, value in (("ruleset_id", 999), ("ruleset_source", "wrong/repo"),
                             ("ruleset_source_type", "Organization"), ("type", "deletion")):
            with self.subTest(field=field):
                old = self.api.effective[0][field]
                self.api.effective[0][field] = value
                with self.assertRaises(Rejected):
                    audit(self.api, self.cfg)
                self.api.effective[0][field] = old
        self.api.rulesets[0]["source"] = "wrong/repo"
        self.reject("source")

    def test_inherited_active_ruleset_inspected_and_visibility_required(self):
        from unittest.mock import patch
        original = self.api.request
        def request(path):
            if path == "/":
                return {"id": 1, "full_name": self.api.repo,
                        "owner": {"login": "KaisAbiyyi", "type": "Organization"}}
            return original(path)
        self.api.rulesets = [self.ruleset(source_type="Organization", source="KaisAbiyyi")]
        with patch.object(self.api, "request", side_effect=request):
            audit(self.api, self.cfg)
            self.api.ruleset_detail[100].pop("bypass_actors")
            self.reject("bypass")

    def test_snapshot_always_prohibits_temporary_unlock_and_merge_authorization(self):
        result = audit(self.api, self.cfg)
        self.assertTrue(result["no_merge_authorization"])
        self.assertFalse(result["temporary_unlock_permitted"])
        self.assertEqual(result["operating_mode"], "PERMANENT_LOCK_NO_ADMISSION")

    def test_unprotected_stacked_base_rejected(self):
        self.api.overrides["/branches/codex%2Fmp-ci-100k-01-20261027/protection"] = {
            **self.api.protection, "lock_branch": {"enabled": False}}
        self.reject("NOT locked")

    def test_concurrent_protection_change_requires_new_audit(self):
        audit(self.api, self.cfg)
        self.api.protection["lock_branch"]["enabled"] = False
        self.reject("NOT locked")

    def test_config_cannot_grant_exception_even_with_approval_and_exact_identities(self):
        self.cfg.update({"temporary_unlock_permitted": True, "no_merge_authorization": False,
                         "admission": {"pr": 132, "source_sha": "a" * 40,
                                       "base_sha": "b" * 40, "merge_tree": "c" * 40,
                                       "independent_approval": True}})
        result = audit(self.api, self.cfg)
        self.assertTrue(result["no_merge_authorization"])
        self.assertFalse(result["temporary_unlock_permitted"])
        self.assertTrue(all(data is None and method is None for _, data, method in self.api.calls))

    def test_cli_reports_api_failures_as_not_verified(self):
        from contextlib import redirect_stdout
        from http.client import IncompleteRead
        from io import StringIO
        import tempfile
        from unittest.mock import patch
        from urllib.error import HTTPError
        from merge_freeze import main
        for failure in (HTTPError("api", 429, "rate limit", {}, None),
                        URLError("authorization unavailable"), IncompleteRead(b"[", 3)):
            with self.subTest(failure=type(failure).__name__), tempfile.TemporaryDirectory() as temp:
                path = Path(temp) / "config.json"
                path.write_text(json.dumps(self.cfg))
                output = StringIO()
                with patch.object(sys, "argv", ["merge_freeze.py", "--config", str(path)]), \
                     patch("merge_freeze.InspectionGitHub.request", side_effect=failure), \
                     redirect_stdout(output), self.assertRaises(SystemExit) as error:
                    main()
                self.assertEqual(error.exception.code, 2)
                result = json.loads(output.getvalue())
                self.assertEqual(result["result"], "NOT_VERIFIED")
                self.assertTrue(result["no_merge_authorization"])


class InspectionTransportTests(unittest.TestCase):
    def response(self, payload, links="", status=200):
        from unittest.mock import MagicMock
        response = MagicMock()
        response.read.return_value = payload
        response.headers = {"Link": links}
        response.status = status
        response.__enter__.return_value = response
        return response

    def call(self, response, path="/rulesets?includes_parents=true&per_page=100&page=1"):
        from unittest.mock import patch
        with patch("merge_freeze.urllib.request.build_opener") as opener:
            opener.return_value.open.return_value = response
            return InspectionGitHub("KaisAbiyyi/mind-place", token="test").request(path)

    def test_short_page_with_next_link_is_not_complete_evidence(self):
        link = '<https://api.github.com/repos/KaisAbiyyi/mind-place/rulesets?includes_parents=true&per_page=100&page=2>; rel="next"'
        with self.assertRaisesRegex(Rejected, "truncated"):
            self.call(self.response(b"[]", link))

    def test_malformed_foreign_or_skipped_next_links_rejected(self):
        for link in ('garbage', '<https://api.github.com/repos/wrong/repo/rulesets?page=2>; rel="next"',
                     '<https://api.github.com/repos/KaisAbiyyi/mind-place/rulesets?includes_parents=true&per_page=100&page=3>; rel="next"'):
            with self.subTest(link=link), self.assertRaises(Rejected):
                self.call(self.response(json.dumps([{}]*100).encode(), link))

    def test_valid_page_continuation_and_reordered_query(self):
        link = '<https://api.github.com/repos/KaisAbiyyi/mind-place/rulesets?page=2&per_page=100&includes_parents=true>; rel="next"'
        self.assertEqual(len(self.call(self.response(json.dumps([{}]*100).encode(), link))), 100)
        self.assertEqual(self.call(self.response(b"[]")), [])

    def test_partial_http_response_rejected(self):
        with self.assertRaisesRegex(Rejected, "incomplete HTTP"):
            self.call(self.response(b"[]", status=206))

    def test_malformed_json_and_oversized_body_rejected(self):
        with self.assertRaises(ValueError):
            self.call(self.response(b"["))
        with self.assertRaisesRegex(Rejected, "oversized"):
            self.call(self.response(b" " * (2*1024*1024+1)))


if __name__ == "__main__":
    unittest.main()
