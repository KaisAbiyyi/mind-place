#!/usr/bin/env python3
"""Bounded, cheap Ubuntu coordination before MP-10C's expensive Windows tier.

Status is based on exact SHA and successful *jobs and steps*, never overall
green workflow alone. Missing checks are pending until deadline, not an early
false failure. Only immutable synthetic source identities are read.
"""
from __future__ import annotations
import argparse
import os
import time
from pathlib import Path
import urllib.error

import mp10c_ci_gate as gate

NATIVE_PATHS = (".github/workflows/ci.yml",
                ".github/workflows/mp9b-sqlcipher-qualification.yml")


def classify(runs, source, vault, fetch_jobs):
    """Return (ready|pending|failed, reason), without accepting skipped jobs."""
    try:
        gate.require_prerequisites(runs, source, vault=vault, fetch_jobs=fetch_jobs)
        return "ready", "Actual exact-source native jobs and steps succeeded"
    except ValueError:
        pass
    required = NATIVE_PATHS if vault else NATIVE_PATHS[:1]
    missing = []
    pending = []
    failed = []
    for path in required:
        same = [r for r in runs if r.get("path") == path and r.get("head_sha") == source]
        if not same:
            missing.append(path)
        elif any(r.get("status") in ("queued", "in_progress", "requested", "waiting", "pending")
                 for r in same):
            pending.append(path)
        else:
            failed.append(path)
    if failed:
        return "failed", "Completed native prerequisite lacks successful required jobs/steps: " + ", ".join(failed)
    return "pending", "Native prerequisite queued/running/missing: " + ", ".join(pending + missing)


def snapshot(repo, source, pr, token, vault):
    if pr > 0:
        pull = gate.api_json(f"https://api.github.com/repos/{repo}/pulls/{pr}", token)
        if (pull.get("state") != "open" or pull.get("head", {}).get("sha") != source
                or pull.get("head", {}).get("repo", {}).get("full_name") != repo):
            return "failed", "Candidate superseded or PR closed"
    runs = []
    # The first three pages bound API cost while preventing first-page truncation.
    for page in range(1, 4):
        response = gate.api_json(
            f"https://api.github.com/repos/{repo}/actions/runs?head_sha={source}&per_page=100&page={page}", token)
        page_runs = response.get("workflow_runs", [])
        if not isinstance(page_runs, list):
            return "failed", "Malformed workflow list"
        runs.extend(page_runs)
        if len(page_runs) < 100:
            break
    def jobs(run):
        return gate.api_json(
            f"https://api.github.com/repos/{repo}/actions/runs/{run['id']}"
            f"/attempts/{run['run_attempt']}/jobs?per_page=100", token)
    return classify(runs, source, vault, jobs)


def wait(repo, source, pr, token, vault, timeout=4800, poll=30, clock=time.monotonic,
         sleep=time.sleep, inspect=snapshot):
    if not gate.HEX40.fullmatch(source) or not repo or pr < 0:
        raise ValueError("Invalid frozen candidate identity")
    deadline = clock() + timeout
    while True:
        try:
            state, reason = inspect(repo, source, pr, token, vault)
        except (OSError, ValueError, KeyError, TypeError, urllib.error.URLError) as err:
            # API failures cannot certify evidence; bounded recheck while time remains.
            state, reason = "pending", f"API verification unavailable: {type(err).__name__}"
        print(f"MP-10C native coordination: {state} — {reason}", flush=True)
        if state == "ready":
            return
        if state == "failed":
            raise ValueError(reason)
        if clock() >= deadline:
            raise TimeoutError(f"Native prerequisite deadline exceeded: {reason}")
        sleep(min(poll, max(0, deadline - clock())))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--timeout-seconds", type=int, default=4800)
    args = parser.parse_args()
    if not 0 <= args.timeout_seconds <= 4800:
        parser.error("Invalid bounded deadline")
    wait(os.environ["GITHUB_REPOSITORY"], os.environ["PR_HEAD_SHA"],
         int(os.environ.get("PR_NUMBER") or 0), os.environ["GH_TOKEN"],
         Path(".github/workflows/mp9b-sqlcipher-qualification.yml").is_file(),
         timeout=args.timeout_seconds)


if __name__ == "__main__":
    main()
