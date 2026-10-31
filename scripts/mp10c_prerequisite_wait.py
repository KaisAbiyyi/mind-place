#!/usr/bin/env python3
"""Bounded Ubuntu coordination for exact PR/base/head-bound native evidence."""
from __future__ import annotations

import argparse
import os
import time
import urllib.error

import mp10c_ci_gate as gate
import mp10c_native_contract as native_contract

BASE_NATIVE_PATHS = (native_contract.CI_WORKFLOW,)
SEPARATED_PRODUCT_PATHS = (
    native_contract.CI_WORKFLOW,
    native_contract.PRODUCT_WORKFLOW,
)


def required_paths(vault: bool, separated_product: bool = False) -> tuple[str, ...]:
    paths = SEPARATED_PRODUCT_PATHS if separated_product else BASE_NATIVE_PATHS
    return paths + ((native_contract.SQLCIPHER_WORKFLOW,) if vault else ())


def classify(runs, candidate, repo, vault, contract_fingerprint, fetch_jobs, fetch_evidence,
             workflow_sources: dict[str, bytes | str] | None = None):
    """Return ready/pending/failed; only exact candidate runs can count."""
    try:
        gate.require_prerequisites(
            runs, candidate, repo=repo, vault=vault,
            contract_fingerprint=contract_fingerprint,
            fetch_jobs=fetch_jobs, fetch_evidence=fetch_evidence,
            workflow_sources=workflow_sources)
        return "ready", "Exact PR/base/head native jobs and contract evidence succeeded"
    except ValueError as error:
        pending = []
        failed = []
        separated_product = native_contract.requires_separated_product(repo, candidate["pr"])
        for path in required_paths(vault, separated_product):
            matching = []
            for run in runs:
                if run.get("path") != path:
                    continue
                try:
                    gate.validate_native_run_identity(run, candidate, repo, workflow_path=path)
                except (KeyError, TypeError, ValueError):
                    continue
                matching.append(run)
            if not matching:
                pending.append(path)
                continue
            if any(run.get("status") in (
                    "queued", "in_progress", "requested", "waiting", "pending")
                   for run in matching):
                pending.append(path)
            elif any(run.get("status") == "completed" for run in matching):
                failed.append(path)
            else:
                pending.append(path)
        if failed:
            return "failed", (
                "Completed exact-candidate native prerequisite failed or lacked required "
                "jobs/steps/provenance: " + ", ".join(failed) + f" ({error})")
        return "pending", "Exact-candidate native prerequisite queued/running/missing: " + ", ".join(pending)


def snapshot(repo, candidate, token, vault, contract_fingerprint):
    try:
        gate.require_live_candidate(repo, candidate, token)
    except ValueError as error:
        return "failed", str(error)
    runs = []
    expected_total = None
    base = f"https://api.github.com/repos/{repo}/actions/runs"
    for page in range(1, 4):
        response = gate.api_json(
            base + f"?head_sha={candidate['head']}&per_page=100&page={page}", token)
        page_runs = response.get("workflow_runs", [])
        total = response.get("total_count")
        if (not isinstance(page_runs, list) or type(total) is not int
                or (expected_total is not None and total != expected_total)):
            return "failed", "Malformed/inconsistent workflow run listing"
        expected_total = total
        runs.extend(page_runs)
        if len(page_runs) < 100:
            break
        if page == 3:
            return "failed", "Native workflow run pagination exceeded the bounded evidence window"
    if expected_total != len(runs):
        return "failed", "Incomplete workflow run listing"

    def jobs(run):
        return gate.api_json(
            f"https://api.github.com/repos/{repo}/actions/runs/{run['id']}"
            f"/attempts/{run['run_attempt']}/jobs?per_page=100", token)

    def evidence(run):
        return gate.fetch_native_evidence(run, repo=repo, token=token)

    state, reason = classify(
        runs, candidate, repo, vault, contract_fingerprint, jobs, evidence)
    try:
        gate.require_live_candidate(repo, candidate, token)
    except ValueError as error:
        return "failed", str(error)
    return state, reason


def wait(repo, candidate, token, vault, contract_fingerprint, timeout=4800, poll=30,
         clock=time.monotonic, sleep=time.sleep, inspect=snapshot):
    if (not repo or not isinstance(candidate, dict)
            or type(candidate.get("pr")) is not int or candidate["pr"] <= 0
            or not gate.HEX40.fullmatch(candidate.get("head", ""))
            or not gate.HEX40.fullmatch(candidate.get("base_sha", ""))
            or not gate.HEX40.fullmatch(candidate.get("checkout_sha", ""))
            or not gate.HEX64.fullmatch(contract_fingerprint or "")
            or native_contract.requires_sqlcipher(repo, candidate["pr"]) is not vault):
        raise ValueError("Invalid frozen native candidate identity/contract")
    deadline = clock() + timeout
    while True:
        try:
            state, reason = inspect(repo, candidate, token, vault, contract_fingerprint)
        except (OSError, ValueError, KeyError, TypeError, AttributeError, urllib.error.URLError, TimeoutError) as err:
            state, reason = "pending", f"API/evidence verification unavailable: {type(err).__name__}"
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
    repo = os.environ["GITHUB_REPOSITORY"]
    candidate = gate.candidate_from_env()
    vault = native_contract.requires_sqlcipher(repo, candidate["pr"])
    contract_fingerprint = os.environ.get("NATIVE_CONTRACT_FINGERPRINT", "")
    if os.environ.get("NATIVE_CONTRACT_POLICY") != native_contract.POLICY:
        raise ValueError("Missing or unsupported native contract policy")
    wait(repo, candidate, os.environ["GH_TOKEN"], vault, contract_fingerprint,
         timeout=args.timeout_seconds)


if __name__ == "__main__":
    main()
