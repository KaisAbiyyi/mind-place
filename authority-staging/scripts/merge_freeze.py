#!/usr/bin/env python3
"""Read-only, fail-closed preflight for a DEFAULT-CLOSED merge freeze.

This is a staging inspection tool, NOT a merge-admission controller. A green
inspection is not proof of GitHub-native enforcement; a real otherwise-green
merge attempt must fail before owner/admin may attest enforcement.
"""
import argparse
import json
from pathlib import Path
from urllib.parse import quote

from windows_product_gate import CHECK, POLICY, GitHub, Rejected, require


SCHEMA = "mp-p0-merge-freeze/v1"


def _enabled(obj, key):
    return isinstance(obj, dict) and isinstance(obj.get(key), dict) and obj[key].get("enabled") is True


def audit(api, config):
    """Inspect *every* protected base; no API writes or automatic unfreeze."""
    require(type(config) is dict and config.get("schema") == SCHEMA, "invalid freeze schema")
    require(config.get("enabled") is True, "freeze plan disabled (not an enforcement claim)")
    require(config.get("repository") == api.repo, "wrong target repository")
    require(config.get("policy") == POLICY, "wrong policy revision")
    app_id = config.get("app_id")
    require(type(app_id) is int and app_id > 0, "missing independent App ID")
    bases = config.get("protected_bases")
    require(type(bases) is list and bool(bases), "missing protected bases")
    require(all(isinstance(b, str) and b and b != "." and
                not b.startswith("/") and not b.endswith("/") and
                ".." not in b.split("/") for b in bases), "invalid protected base")
    require(len(bases) == len(set(bases)), "duplicate protected base")
    require("main" in bases, "main must remain in the freeze scope")

    # Inherited active rulesets with bypass grants require separate review.
    # Treat uninspectable or potentially bypassable state as NOT VERIFIED.
    rulesets = api.request("/rulesets?includes_parents=true")
    require(type(rulesets) is list, "ruleset inventory unavailable")
    for summary in rulesets:
        require(type(summary) is dict, "malformed ruleset")
        if summary.get("enforcement") == "active":
            ruleset_id = summary.get("id")
            require(type(ruleset_id) is int and ruleset_id > 0,
                    "uninspectable active ruleset")
            detail = api.request("/rulesets/" + str(ruleset_id) + "?includes_parents=true")
            require(type(detail) is dict and detail.get("id") == ruleset_id and
                    detail.get("enforcement") == "active",
                    "incomplete active ruleset detail")
            # GitHub may omit bypass_actors unless the token has the
            # required ruleset write/inspection permission. Missing != [].
            require(detail.get("bypass_actors") == [],
                    "active ruleset bypass is present or not independently visible")
    results = []
    for base in bases:
        protection = api.request("/branches/" + quote(base, safe="") + "/protection")
        require(type(protection) is dict, "missing branch protection: " + base)
        require(_enabled(protection, "lock_branch"), "branch is NOT locked: " + base)
        require(_enabled(protection, "enforce_admins"), "administrator enforcement absent: " + base)
        # GitHub can permit fork syncing even on a locked branch. An
        # uninspectable bypass setting is not acceptable evidence of a freeze.
        for bypass_flag in ("allow_fork_syncing", "allow_force_pushes", "allow_deletions"):
            state = protection.get(bypass_flag)
            require(type(state) is dict and state.get("enabled") is False,
                    "unsafe or unknown branch update bypass (" + bypass_flag + "): " + base)
        checks = protection.get("required_status_checks")
        require(isinstance(checks, dict) and checks.get("strict") is True,
                "strict required checks absent: " + base)
        required = checks.get("checks")
        require(type(required) is list, "App-bound required checks absent: " + base)
        require(any(isinstance(c, dict) and c.get("context") == CHECK and
                    type(c.get("app_id")) is int and c["app_id"] == app_id for c in required),
                "wrong or missing required independent App check: " + base)
        results.append({"base": base, "lock_branch_read": True, "admin_enforced_read": True,
                        "app_check_read": True})
    return {"schema": SCHEMA, "repository": api.repo, "read_only": True,
            "configuration_snapshot_only": True, "live_negative_merge_probe": "NOT_YET_LIVE_VALIDATED",
            "automatic_unfreeze": False, "bases": results}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", required=True, type=Path)
    args = parser.parse_args()
    try:
        config = json.loads(args.config.read_text(encoding="utf-8"))
        repo = config.get("repository") if type(config) is dict else None
        require(isinstance(repo, str) and repo == "KaisAbiyyi/mind-place",
                "unexpected repository in staging freeze plan")
        print(json.dumps(audit(GitHub(repo), config), sort_keys=True))
    except (Rejected, KeyError, TypeError, ValueError, OSError) as exc:
        print(json.dumps({"result": "NOT_VERIFIED", "reason": str(exc),
                          "no_merge_authorization": True}, sort_keys=True))
        raise SystemExit(2) from exc


if __name__ == "__main__":
    main()
