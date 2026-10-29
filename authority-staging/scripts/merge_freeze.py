#!/usr/bin/env python3
"""Read-only, fail-closed preflight for a DEFAULT-CLOSED merge freeze.

This is a staging inspection tool, NOT a merge-admission controller. A green
inspection is not proof of GitHub-native enforcement; a real otherwise-green
merge attempt must fail before owner/admin may attest enforcement.
"""
import argparse
from http.client import HTTPException
import json
from pathlib import Path
import re
import urllib.request
from urllib.parse import quote, urlsplit, parse_qsl

from windows_product_gate import CHECK, POLICY, GitHub, Rejected, SafeRedirect, require


SCHEMA = "mp-p0-merge-freeze/v1"


class InspectionGitHub(GitHub):
    """GET-only transport; reject short/truncated or inconsistent linked pages.

    The frozen verifier transport intentionally remains byte-identical. Its
    paginator does not validate list types or expose Link headers, so this
    inspector needs stricter page validation rather than silently flattening {}.
    """
    def request(self, path):
        require(path.startswith("/") and not path.startswith("//"), "invalid API path")
        url = "https://api.github.com/repos/" + self.repo + path
        request = urllib.request.Request(url, headers={
            "Authorization": "Bearer " + self.token,
            "Accept": "application/vnd.github+json",
            "X-GitHub-Api-Version": "2022-11-28"})
        with urllib.request.build_opener(SafeRedirect()).open(request, timeout=30) as response:
            require(response.status == 200, "incomplete HTTP API response")
            payload = response.read(2 * 1024 * 1024 + 1)
            links = response.headers.get("Link", "")
        require(len(payload) <= 2 * 1024 * 1024, "oversized API response")
        data = json.loads(payload)
        if "per_page=100&page=" in path:
            seen = set()
            for link in links.split(",") if links else []:
                match = re.fullmatch(r'\s*<([^>]+)>;\s*rel="(next|prev|first|last)"\s*', link)
                require(match is not None, "malformed API pagination Link")
                href, rel = match.groups()
                require(rel not in seen, "duplicate API pagination Link")
                seen.add(rel)
                if rel == "next":
                    expected = url.rsplit("=", 1)[0] + "=" + str(int(path.rsplit("=", 1)[1]) + 1)
                    actual_parts, expected_parts = urlsplit(href), urlsplit(expected)
                    require(actual_parts[:3] == expected_parts[:3] and
                            sorted(parse_qsl(actual_parts.query)) == sorted(parse_qsl(expected_parts.query)) and
                            not actual_parts.fragment, "inconsistent API pagination Link")
                    require(type(data) is list and len(data) == 100,
                            "truncated API page with continuation")
        return data


def _pages(api, path):
    """Same 20x100 bound as the frozen paginator, with per-page type checks."""
    result = []
    for page in range(1, 21):
        items = api.request(path + ("&" if "?" in path else "?") +
                            f"per_page=100&page={page}")
        require(type(items) is list and len(items) <= 100, "ruleset inventory unavailable or malformed page")
        result.extend(items)
        if len(items) < 100:
            return result
    raise Rejected("API pagination limit; no partial evidence accepted")


def _rulesets(api):
    repository = api.request("/")
    require(type(repository) is dict and repository.get("full_name") == api.repo and
            type(repository.get("id")) is int and repository["id"] > 0,
            "wrong or uninspectable repository identity")
    owner = repository.get("owner")
    require(type(owner) is dict and owner.get("login") == api.repo.split("/")[0] and
            owner.get("type") in ("User", "Organization"), "unknown repository owner")
    summaries = _pages(api, "/rulesets?includes_parents=true")
    inventory = {}
    for summary in summaries:
        require(type(summary) is dict, "malformed ruleset")
        rid = summary.get("id")
        require(type(rid) is int and rid > 0, "uninspectable ruleset ID")
        require(rid not in inventory, "duplicate or inconsistent ruleset entry")
        state = summary.get("enforcement")
        require(state in ("active", "disabled", "evaluate"), "unknown effective enforcement")
        kind, source = summary.get("source_type"), summary.get("source")
        require((kind == "Repository" and source == api.repo) or
                (kind == "Organization" and owner["type"] == "Organization" and source == owner["login"]),
                "unknown or inconsistent ruleset source; owner/admin inspection required")
        require(summary.get("target") in ("branch", "tag", "push") and
                isinstance(summary.get("name"), str) and summary["name"], "incomplete ruleset summary")
        inventory[rid] = summary
        if state != "active":
            continue
        detail = api.request(f"/rulesets/{rid}?includes_parents=true")
        require(type(detail) is dict and type(detail.get("id")) is int and all(
                detail.get(key) == summary[key] for key in
                ("id", "name", "enforcement", "source_type", "source", "target")),
                "incomplete or inconsistent active ruleset detail")
        require(detail.get("bypass_actors") == [],
                "active ruleset bypass is present or not independently visible")
        conditions, rules = detail.get("conditions"), detail.get("rules")
        require(type(conditions) is dict and type(rules) is list and all(
                type(rule) is dict and isinstance(rule.get("type"), str) and rule["type"] for rule in rules),
                "incomplete active ruleset conditions or rules")
        if detail["target"] in ("branch", "tag"):
            refs = conditions.get("ref_name")
            require(type(refs) is dict and all(type(refs.get(k)) is list and
                    all(isinstance(v, str) and v for v in refs[k]) for k in ("include", "exclude")),
                    "incomplete ruleset ref conditions")
        inventory[rid] = detail
    return inventory


def _effective_rules(api, base, inventory):
    """GitHub evaluates inherited repository/ref conditions; no local glob guess."""
    effective = _pages(api, "/rules/branches/" + quote(base, safe=""))
    seen, applicable = set(), set()
    for rule in effective:
        require(type(rule) is dict, "malformed effective branch rule")
        rid = rule.get("ruleset_id")
        require(type(rid) is int and rid > 0 and rid in inventory,
                "effective ruleset missing from complete inventory")
        detail = inventory[rid]
        require(detail["enforcement"] == "active" and detail["target"] in ("branch", "push") and
                rule.get("ruleset_source_type") == detail["source_type"] and
                rule.get("ruleset_source") == detail["source"] and
                type(rule.get("type")) is str and any(r.get("type") == rule["type"] for r in detail["rules"]),
                "inconsistent effective ruleset applicability")
        identity = (rid, rule["type"])
        require(identity not in seen, "duplicate effective branch rule")
        seen.add(identity)
        applicable.add(rid)
    return sorted(applicable)


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

    # All active inventory entries are inspected, even those not applying to
    # a destination. This deliberately rejects bypass more conservatively.
    inventory = _rulesets(api)
    results = []
    for base in bases:
        effective_ids = _effective_rules(api, base, inventory)
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
                        "app_check_read": True, "effective_ruleset_ids": effective_ids})
    return {"schema": SCHEMA, "repository": api.repo, "read_only": True,
            "configuration_snapshot_only": True, "live_negative_merge_probe": "NOT_YET_LIVE_VALIDATED",
            "automatic_unfreeze": False, "no_merge_authorization": True,
            "temporary_unlock_permitted": False,
            "operating_mode": "PERMANENT_LOCK_NO_ADMISSION", "bases": results}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", required=True, type=Path)
    args = parser.parse_args()
    try:
        config = json.loads(args.config.read_text(encoding="utf-8"))
        repo = config.get("repository") if type(config) is dict else None
        require(isinstance(repo, str) and repo == "KaisAbiyyi/mind-place",
                "unexpected repository in staging freeze plan")
        print(json.dumps(audit(InspectionGitHub(repo), config), sort_keys=True))
    except (Rejected, KeyError, TypeError, ValueError, OSError, HTTPException) as exc:
        print(json.dumps({"result": "NOT_VERIFIED", "reason": str(exc),
                          "no_merge_authorization": True}, sort_keys=True))
        raise SystemExit(2) from exc


if __name__ == "__main__":
    main()
