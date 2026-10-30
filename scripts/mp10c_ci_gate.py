#!/usr/bin/env python3
"""MP-CI-100K-01: fail-closed, source-bound qualification reuse for MP-10C.

Only the original Windows 100,000-document job may mint an accepted provenance
receipt. An unrelated PR push may reuse that proof, but is never itself called a
new full-scale PASS. No owner files are accessed.
"""
from __future__ import annotations

import argparse
import base64
import hashlib
import io
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import urllib.error
import urllib.parse
import urllib.request
import zipfile

import mp10c_native_contract as native_contract

SCHEMA = "mind-place-mp10c-qualification/v2"
WORKLOAD_POLICY = "mp10c-workload/v2"  # Verified native scale semantics + production inputs.
ARTIFACT = "mp10c-qualified-100000"
WORKFLOW = "mp-10c-scale.yml"
FULL_JOB = "Windows approved documents / 100000 files"
PROOF_JOB = "Source-bound 100K provenance (no indexing)"
RAW_ARTIFACT = "mp10c-raw-100000"
HEX64 = re.compile(r"^[0-9a-f]{64}$")
HEX40 = re.compile(r"^[0-9a-f]{40}$")


def excluded(path: str, *, legacy: bool = False) -> bool:
    """Non-workload paths are policy inputs, not excuses to trust old evidence.

    Legacy v1 included the validator, gate tests and the entire scale workflow.
    v2 instead separately commits to immutable scale runner/test job semantics.
    """
    common = (path.startswith("docs/")
              or path in {"README.md", "LICENSE", "LICENSE.md", "CHANGELOG.md"}
              or path.startswith(".github/ISSUE_TEMPLATE/")
              or path.startswith(".github/DISCUSSION_TEMPLATE/")
              or path in {".github/PULL_REQUEST_TEMPLATE.md", ".github/workflows/ci.yml"}
              or (path.startswith(".github/workflows/") and
                  path != ".github/workflows/mp-10c-scale.yml"))
    return common or (not legacy and path in {
        "scripts/mp10c_ci_gate.py", "scripts/tests/test_mp10c_ci_gate.py",
        "scripts/tests/test_mp10c_native_contract.py",
        "scripts/mp10c_prerequisite_wait.py", "scripts/mp10c_native_contract.py",
        ".github/workflows/mp-10c-scale.yml"})


def scale_workflow_signature(contents: bytes) -> bytes:
    """Hash the existing mp10c-workload/v2 Windows smoke + full workload contract.

    This preserves the v2 workload boundary. Native workflow-wide execution
    semantics (timeouts, dependencies, environment and evidence jobs) are
    separately bound by mp10c_native_contract and are not mixed into workload
    identity. Only approved identity keys in document-scale's job-level env
    mapping are excluded from workload inputs.
    """
    source = contents.decode("utf-8")
    lines = source.splitlines()
    digest = hashlib.sha256(b"mp10c-windows-scale-steps/v1\0")
    identity_only = {
        "PR_HEAD_REPOSITORY", "PR_HEAD_REPOSITORY_ID",
        "PR_BASE_REPOSITORY", "PR_BASE_REPOSITORY_ID",
        "PR_BASE_REF", "PR_BASE_SHA", "PR_NATIVE_CHECKOUT_SHA",
        "NATIVE_CONTRACT_POLICY", "NATIVE_CONTRACT_FINGERPRINT",
    }
    for name in ("document-smoke", "document-scale"):
        job_start = next((
            index for index, line in enumerate(lines)
            if re.fullmatch(r"  " + re.escape(name) + r":\s*", line)
        ), None)
        if job_start is None:
            raise ValueError("Scale workflow missing required Windows tier: " + name)
        job_end = next((
            index for index in range(job_start + 1, len(lines))
            if re.fullmatch(r"  [a-z][a-z0-9-]*:\s*", lines[index])
        ), len(lines))
        body_lines = lines[job_start + 1:job_end]
        runner_index = next((
            index for index, line in enumerate(body_lines)
            if re.fullmatch(r"    runs-on: windows-latest\s*", line)
        ), None)
        if runner_index is None:
            raise ValueError("Scale workflow runner contract changed: " + name)
        if not any(re.fullmatch(r"    steps:\s*", line) for line in body_lines[runner_index:]):
            raise ValueError("Missing Windows scale steps: " + name)
        contract_lines = body_lines[runner_index:]
        if name == "document-scale":
            env_headers = [
                index for index, line in enumerate(body_lines)
                if re.fullmatch(r"    env:\s*", line)
            ]
            if len(env_headers) > 1:
                raise ValueError("Ambiguous document-scale job-level env mapping")
            if env_headers:
                env_index = env_headers[0]
                removed = set()
                for index in range(env_index + 1, len(body_lines)):
                    line = body_lines[index]
                    if not line.strip() or line.lstrip().startswith("#"):
                        continue
                    indent = len(line) - len(line.lstrip(" "))
                    if indent <= 4:
                        break
                    if indent != 6:
                        continue
                    match = re.match(r"^([A-Za-z_][A-Za-z0-9_-]*)\s*:", line.strip())
                    if match and match.group(1) in identity_only:
                        removed.add(index - runner_index)
                contract_lines = [
                    line for index, line in enumerate(contract_lines)
                    if index not in removed
                ]
        contract = "\n".join(contract_lines).rstrip()
        digest.update(name.encode() + b"\0" + contract.encode() + b"\0")
    return digest.digest()


def fingerprint(root: str = ".") -> str:
    """Hash Git's staged tree, not mutable working-directory file contents."""
    records = subprocess.check_output(["git", "ls-files", "--stage", "-z"], cwd=root)
    entries = []
    scale_sha = None
    for record in records.split(b"\0"):
        if not record:
            continue
        meta, path_bytes = record.split(b"\t", 1)
        mode, oid, stage = meta.split(b" ")
        if stage != b"0":
            raise ValueError("Unmerged index: refusing fingerprint")
        entries.append((mode, oid, path_bytes))
        if path_bytes == b".github/workflows/mp-10c-scale.yml":
            scale_sha = oid
    scale = (subprocess.check_output(["git", "cat-file", "blob", scale_sha], cwd=root)
             if scale_sha else None)
    return fingerprint_entries(entries, scale_workflow=scale)


def fingerprint_entries(entries: list[tuple[bytes, bytes, bytes]], *,
                        legacy: bool = False, scale_workflow: bytes | None = None) -> str:
    digest = hashlib.sha256(
        b"mp10c-effective-inputs/v1\0" if legacy else b"mp10c-workload-inputs/v2\0")
    included = 0
    seen = set()
    for mode, oid, path_bytes in sorted(entries, key=lambda item: item[2]):
        if path_bytes in seen or mode not in {b"100644", b"100755", b"120000", b"160000"}:
            raise ValueError("Invalid or duplicate effective tree entry")
        seen.add(path_bytes)
        if not HEX40.fullmatch(oid.decode("ascii")):
            raise ValueError("Invalid effective tree object")
        if excluded(path_bytes.decode("utf-8", errors="surrogateescape"), legacy=legacy):
            continue
        digest.update(mode + b"\0" + oid + b"\0" + path_bytes + b"\0")
        included += 1
    if not included:
        raise ValueError("Empty workload set: refusing fingerprint")
    if not legacy and scale_workflow is not None:
        digest.update(b"mp10c-scale-execution-contract\0" + scale_workflow_signature(scale_workflow))
    return digest.hexdigest()


def require_raw_receipt(obj: dict, tier: int = 100000) -> None:
    if not isinstance(obj, dict):
        raise ValueError("Receipt must be an object")
    expected = {
        "schema": "mind-place-mp10c-document-scale/v1",
        "fixture": "synthetic-only",
        "mode": "real-approved-root-index",
        "platform": "windows",
        "target_files": tier,
        "indexed_files": tier,
        "extracted_chunks": tier * 10,
        "approved_roots": (tier + 9999) // 10000,
        "production_files_per_root_cap": 10000,
        "raw_bytes_unchanged": True,
        "sqlite_and_fts_integrity": "pass",
    }
    for name, value in expected.items():
        if type(obj.get(name)) is not type(value) or obj.get(name) != value:
            raise ValueError(f"Invalid raw 100K acceptance: {name}")
    if not isinstance(obj.get("fixture_sha256"), str) or not HEX64.fullmatch(obj["fixture_sha256"]):
        raise ValueError("Invalid fixture digest")


def validate_proof(proof: dict, raw_bytes: bytes, *, run: dict, artifact: dict,
                   pr: int, repo: str, desired: str) -> None:
    if not isinstance(proof, dict) or proof.get("schema") != SCHEMA:
        raise ValueError("Missing/invalid provenance schema")
    policy = proof.get("fingerprint_policy", "legacy/v1")
    if policy not in {"legacy/v1", WORKLOAD_POLICY}:
        raise ValueError("Unknown/untrusted fingerprint policy")
    source = proof.get("checkout_sha")
    if not isinstance(source, str) or not HEX40.fullmatch(source):
        raise ValueError("Invalid qualified checkout SHA")
    if (type(proof.get("run_id")) is not int or type(proof.get("run_attempt")) is not int
            or proof.get("run_id") != run.get("id") or proof.get("run_attempt") != run.get("run_attempt")):
        raise ValueError("Provenance does not belong to successful run attempt")
    event = run.get("event")
    if (proof.get("repo") != repo or type(pr) is not int
            or (pr != 0 if event == "push" else pr <= 0)
            or type(proof.get("pr_number")) is not int or proof.get("pr_number") != pr
            or event not in {"pull_request", "workflow_dispatch", "push"} or proof.get("event") != event):
        raise ValueError("Proof belongs to another repository, PR, or event")
    # PR run metadata records branch HEAD; checkout tests a synthetic merge.
    if proof.get("source_sha") != run.get("head_sha"):
        raise ValueError("PR branch HEAD SHA != GitHub qualified run")
    if event in {"workflow_dispatch", "push"} and source != proof["source_sha"]:
        raise ValueError("Manual qualification must test the exact dispatched PR head")
    if proof.get("source_ref") != run.get("head_branch") or not proof.get("source_ref"):
        raise ValueError("Qualification branch does not match workflow event")
    scale_attempt = proof.get("scale_run_attempt")
    if type(scale_attempt) is not int or not 0 < scale_attempt <= run["run_attempt"]:
        raise ValueError("Invalid original full-scale attempt")
    if proof.get("tier") != 100000 or proof.get("qualified") is not True:
        raise ValueError("Proof does not attest completed full tier")
    if (not HEX64.fullmatch(desired)
            or not isinstance(proof.get("input_fingerprint"), str)
            or not HEX64.fullmatch(proof["input_fingerprint"])
            or (policy == WORKLOAD_POLICY and proof["input_fingerprint"] != desired)):
        raise ValueError("Relevant workload policy/source mismatch")
    if proof.get("raw_receipt_sha256") != hashlib.sha256(raw_bytes).hexdigest():
        raise ValueError("Raw acceptance receipt does not match provenance hash")
    require_raw_receipt(json.loads(raw_bytes))
    if artifact.get("expired") is not False or artifact.get("name") != artifact_name(ARTIFACT, run["run_attempt"]):
        raise ValueError("Artifact expired/misnamed")
    if artifact.get("workflow_run", {}).get("id") != run.get("id"):
        raise ValueError("Artifact belongs to different workflow run")


def check_merge_source(*, repo_root: str = ".", expected_source: str,
                       expected_base: str | None = None, revision: str = "HEAD") -> None:
    """Verify exact base/head parents from the immutable PR merge object."""
    if not HEX40.fullmatch(expected_source):
        raise ValueError("Invalid expected PR source SHA")
    if expected_base is not None and not HEX40.fullmatch(expected_base):
        raise ValueError("Invalid expected PR base SHA")
    head = subprocess.check_output(
        ["git", "rev-parse", revision], cwd=repo_root, text=True).strip()
    data = subprocess.check_output(
        ["git", "cat-file", "-p", head], cwd=repo_root, text=True)
    headers = data.split("\n\n", 1)[0].splitlines()
    parents = [line[7:] for line in headers if line.startswith("parent ")]
    if len(parents) != 2 or any(not HEX40.fullmatch(p) for p in parents):
        raise ValueError("Expected a two-parent PR checkout merge")
    if parents[0] == parents[1] or parents[1] != expected_source:
        raise ValueError("PR checkout merge does not match branch source SHA")
    if expected_base is not None and parents[0] != expected_base:
        raise ValueError("PR checkout merge does not match frozen base SHA")


def make_proof(*, receipt: bytes, source: str, run_id: int, attempt: int,
               pr: int, repo: str, event: str, effective: str,
               head_sha: str | None = None, head_ref: str = "",
               scale_attempt: int | None = None) -> dict:
    require_raw_receipt(json.loads(receipt))
    head_sha = head_sha or source
    if not HEX40.fullmatch(source) or not HEX40.fullmatch(head_sha) or not HEX64.fullmatch(effective):
        raise ValueError("Invalid source binding")
    if not all(type(x) is int and x > 0 for x in (run_id, attempt)):
        raise ValueError("Missing workflow identity")
    if not repo or event not in {"pull_request", "workflow_dispatch", "push"}:
        raise ValueError("Missing workflow identity")
    if (type(pr) is not int or (pr != 0 if event == "push" else pr <= 0) or not head_ref):
        raise ValueError("Qualification requires a positive PR identity and branch")
    scale_attempt = attempt if scale_attempt is None else scale_attempt
    if type(scale_attempt) is not int or not 0 < scale_attempt <= attempt:
        raise ValueError("Invalid original full-scale attempt")
    return {
        "schema": SCHEMA, "tier": 100000, "qualified": True,
        "repo": repo, "event": event, "pr_number": pr,
        "checkout_sha": source, "source_sha": head_sha,
        "source_ref": head_ref, "scale_run_attempt": scale_attempt,
        "input_fingerprint": effective, "fingerprint_policy": WORKLOAD_POLICY,
        "run_id": run_id, "run_attempt": attempt,
        "raw_receipt_sha256": hashlib.sha256(receipt).hexdigest(),
    }


def artifact_name(name: str, attempt: int) -> str:
    if type(attempt) is not int or attempt <= 0:
        raise ValueError("Invalid artifact attempt")
    return f"{name}-attempt-{attempt}"


def resolve_target(*, event: str, payload: dict, repo: str, sha: str,
                   ref: str, token: str) -> dict:
    """Freeze every PR identity dimension from the event/API snapshot."""
    if event == "push":
        if not ref.startswith("refs/tags/") or not HEX40.fullmatch(sha):
            raise ValueError("Only a release tag may qualify without a PR")
        return {
            "pr": 0, "head": sha, "branch": ref.removeprefix("refs/tags/"),
            "head_repo": repo, "head_repo_id": None, "base_repo": repo,
            "base_repo_id": None, "base_branch": "", "base_sha": "0" * 40,
        }
    if event == "pull_request":
        pull = payload.get("pull_request")
        if not isinstance(pull, dict) or type(payload.get("number")) is not int:
            raise ValueError("Missing or ambiguous pull_request event identity")
        if pull.get("number", payload["number"]) != payload["number"]:
            raise ValueError("Pull request event number mismatch")
    elif event == "workflow_dispatch":
        inputs = payload.get("inputs", {})
        number = inputs.get("pr_number", "")
        expected_head = inputs.get("head_sha", "")
        expected_base = inputs.get("base_sha", "")
        if not isinstance(number, str) or not re.fullmatch(r"[1-9][0-9]*", number):
            raise ValueError("Manual/final qualification requires pr_number")
        if not HEX40.fullmatch(expected_head or "") or not HEX40.fullmatch(expected_base or ""):
            raise ValueError("Manual/final qualification requires explicit head_sha and base_sha")
        pull = api_json(f"https://api.github.com/repos/{repo}/pulls/{number}", token)
        if pull.get("number") != int(number) or pull.get("state") != "open":
            raise ValueError("Manual target PR is not open")
        if (pull.get("head", {}).get("sha") != expected_head
                or pull.get("base", {}).get("sha") != expected_base
                or sha != expected_head
                or ref != "refs/heads/" + pull.get("head", {}).get("ref", "")):
            raise ValueError("Dispatch must select the exact current PR head and base")
    else:
        raise ValueError("Unsupported qualification event")

    head, base = pull.get("head", {}), pull.get("base", {})
    head_repo, base_repo = head.get("repo") or {}, base.get("repo") or {}
    head_repo_name, base_repo_name = head_repo.get("full_name"), base_repo.get("full_name")
    number, head_sha, head_ref = pull.get("number"), head.get("sha"), head.get("ref")
    base_sha, base_ref = base.get("sha"), base.get("ref")
    if (head_repo_name != repo or base_repo_name != repo
            or type(head_repo.get("id")) is not int or head_repo["id"] <= 0
            or type(base_repo.get("id")) is not int or base_repo["id"] <= 0
            or head_repo["id"] != base_repo["id"]
            or type(number) is not int or number <= 0
            or not HEX40.fullmatch(head_sha or "") or not HEX40.fullmatch(base_sha or "")
            or not isinstance(head_ref, str) or not head_ref
            or not isinstance(base_ref, str) or not base_ref):
        raise ValueError("Invalid, forked, or cross-repository PR identity")
    return {
        "pr": number, "head": head_sha, "branch": head_ref,
        "head_repo": head_repo_name, "head_repo_id": head_repo["id"],
        "base_repo": base_repo_name, "base_repo_id": base_repo["id"],
        "base_branch": base_ref, "base_sha": base_sha,
    }


def candidate_from_env() -> dict:
    def required(name: str) -> str:
        value = os.environ.get(name, "")
        if not value:
            raise ValueError("Missing frozen candidate field: " + name)
        return value
    pr = int(required("PR_NUMBER"))
    candidate = {
        "pr": pr, "head": required("PR_HEAD_SHA"),
        "branch": required("PR_HEAD_REF"),
        "head_repo": required("PR_HEAD_REPOSITORY"),
        "head_repo_id": int(required("PR_HEAD_REPOSITORY_ID")),
        "base_repo": required("PR_BASE_REPOSITORY"),
        "base_repo_id": int(required("PR_BASE_REPOSITORY_ID")),
        "base_branch": required("PR_BASE_REF"),
        "base_sha": required("PR_BASE_SHA"),
        "checkout_sha": required("PR_NATIVE_CHECKOUT_SHA"),
    }
    if (pr <= 0 or candidate["head_repo_id"] <= 0 or candidate["base_repo_id"] <= 0
            or candidate["head_repo_id"] != candidate["base_repo_id"]
            or any(not HEX40.fullmatch(candidate[k]) for k in ("head", "base_sha", "checkout_sha"))):
        raise ValueError("Invalid frozen candidate SHA/PR/repository identity")
    if (candidate["head_repo"] != os.environ.get("GITHUB_REPOSITORY")
            or candidate["base_repo"] != os.environ.get("GITHUB_REPOSITORY")):
        raise ValueError("Frozen candidate repository identity mismatch")
    return candidate


def require_live_candidate(repo: str, candidate: dict, token: str) -> None:
    pull = api_json(f"https://api.github.com/repos/{repo}/pulls/{candidate['pr']}", token)
    head, base = pull.get("head", {}), pull.get("base", {})
    if (pull.get("number") != candidate["pr"] or pull.get("state") != "open"
            or head.get("repo", {}).get("full_name") != candidate["head_repo"]
            or head.get("repo", {}).get("id") != candidate["head_repo_id"]
            or head.get("ref") != candidate["branch"] or head.get("sha") != candidate["head"]
            or base.get("repo", {}).get("full_name") != candidate["base_repo"]
            or base.get("repo", {}).get("id") != candidate["base_repo_id"]
            or base.get("ref") != candidate["base_branch"] or base.get("sha") != candidate["base_sha"]):
        raise ValueError("Frozen PR candidate head/base identity was superseded or closed")


def validate_native_run_identity(run: dict, candidate: dict, repo: str,
                                 workflow_path: str = ".github/workflows/ci.yml") -> None:
    if (run.get("event") != "pull_request"
            or run.get("path") != workflow_path
            or run.get("repository", {}).get("full_name") != repo
            or run.get("head_repository", {}).get("full_name") != candidate["head_repo"]
            or run.get("head_repository", {}).get("id") != candidate["head_repo_id"]
            or run.get("repository", {}).get("id") != candidate["base_repo_id"]
            or run.get("head_branch") != candidate["branch"]
            or run.get("head_sha") != candidate["head"]):
        raise ValueError("Native run repository/event/head identity mismatch")
    associated = run.get("pull_requests")
    if not isinstance(associated, list) or len(associated) != 1:
        raise ValueError("Native run has missing or ambiguous PR association")
    context = associated[0]
    head, base = context.get("head", {}), context.get("base", {})
    if (context.get("number") != candidate["pr"]
            or head.get("repo", {}).get("id") != candidate["head_repo_id"]
            or head.get("ref") != candidate["branch"] or head.get("sha") != candidate["head"]
            or base.get("repo", {}).get("id") != candidate["base_repo_id"]
            or base.get("ref") != candidate["base_branch"] or base.get("sha") != candidate["base_sha"]):
        raise ValueError("Native run is associated with another PR or base/head revision")


def fetch_native_evidence(run: dict, *, repo: str, token: str) -> dict:
    run_id, attempt = run.get("id"), run.get("run_attempt")
    if type(run_id) is not int or type(attempt) is not int or run_id <= 0 or attempt <= 0:
        raise ValueError("Invalid native workflow run identity")
    response = api_json(
        f"https://api.github.com/repos/{repo}/actions/runs/{run_id}/artifacts?per_page=100", token)
    artifacts = response.get("artifacts", [])
    if not isinstance(artifacts, list) or response.get("total_count") != len(artifacts):
        raise ValueError("Incomplete native evidence artifact listing")
    name = f"mp-native-contract-attempt-{attempt}"
    matches = [item for item in artifacts if item.get("name") == name]
    if len(matches) != 1:
        raise ValueError("Missing or ambiguous same-run native evidence artifact")
    bundle = read_verified_artifact(matches[0], {"native-contract.json"}, run_id=run_id, token=token)
    return json.loads(bundle["native-contract.json"])


def validate_native_evidence(evidence: dict, run: dict, candidate: dict, *,
                             repo: str, contract_fingerprint: str, vault: bool) -> None:
    kind, policy_vault = native_contract.candidate_requirement(repo, candidate["pr"])
    expected = {
        "schema": native_contract.EVIDENCE_SCHEMA, "repository": repo,
        "event": "pull_request", "workflow_path": ".github/workflows/ci.yml",
        "pr_number": candidate["pr"],
        "head_repository": candidate["head_repo"],
        "head_repository_id": candidate["head_repo_id"],
        "head_branch": candidate["branch"], "head_sha": candidate["head"],
        "base_repository": candidate["base_repo"],
        "base_repository_id": candidate["base_repo_id"],
        "base_branch": candidate["base_branch"], "base_sha": candidate["base_sha"],
        "checkout_sha": candidate["checkout_sha"], "candidate_kind": kind,
        "sqlcipher_required": vault, "native_contract_policy": native_contract.POLICY,
        "native_contract_fingerprint": contract_fingerprint,
        "workflow_run_id": run["id"], "workflow_run_attempt": run["run_attempt"],
    }
    if policy_vault is not vault or not HEX64.fullmatch(contract_fingerprint or ""):
        raise ValueError("Invalid candidate SQLCipher policy or native contract fingerprint")
    for key, value in expected.items():
        if evidence.get(key) != value:
            raise ValueError("Native provenance mismatch: " + key)
    if evidence.get("head_repository_id") is not None:
        if evidence["head_repository_id"] != run.get("head_repository", {}).get("id"):
            raise ValueError("Native provenance head repository ID mismatch")
    if evidence.get("base_repository_id") is not None:
        base = run["pull_requests"][0].get("base", {}).get("repo", {})
        if evidence["base_repository_id"] != base.get("id"):
            raise ValueError("Native provenance base repository ID mismatch")


def api_json(url: str, token: str) -> dict:
    req = urllib.request.Request(url, headers={
        "Authorization": "Bearer " + token,
        "Accept": "application/vnd.github+json",
        "X-GitHub-Api-Version": "2022-11-28",
        "User-Agent": "mind-place-mp10c-gate",
    })
    with urllib.request.urlopen(req, timeout=30) as response:
        return json.load(response)


class StripAuthOnRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        next_request = super().redirect_request(req, fp, code, msg, headers, newurl)
        if next_request and urllib.parse.urlsplit(req.full_url).netloc != urllib.parse.urlsplit(newurl).netloc:
            next_request.remove_header("Authorization")
        return next_request


def artifact_bytes(url: str, token: str, maximum: int = 4 * 1024 * 1024) -> bytes:
    opener = urllib.request.build_opener(StripAuthOnRedirect)
    req = urllib.request.Request(url, headers={
        "Authorization": "Bearer " + token,
        "Accept": "application/vnd.github+json",
        "X-GitHub-Api-Version": "2022-11-28",
    })
    with opener.open(req, timeout=60) as r:
        payload = r.read(maximum + 1)
    if len(payload) > maximum:
        raise ValueError("Oversized provenance artifact")
    return payload


def read_verified_artifact(item: dict, files: set[str], *, run_id: int, token: str) -> dict[str, bytes]:
    if item.get("expired") is not False or item.get("workflow_run", {}).get("id") != run_id:
        raise ValueError("Expired or foreign artifact")
    zipped = artifact_bytes(item["archive_download_url"], token)
    digest = item.get("digest", "")
    if (not re.fullmatch(r"sha256:[0-9a-f]{64}", digest or "")
            or hashlib.sha256(zipped).hexdigest() != digest.removeprefix("sha256:")):
        raise ValueError("Artifact archive digest mismatch")
    with zipfile.ZipFile(io.BytesIO(zipped)) as archive:
        if (len(archive.infolist()) != len(files) or set(archive.namelist()) != files
                or any(archive.getinfo(name).file_size > 65536 for name in files)):
            raise ValueError("Unexpected or oversized artifact entries")
        return {name: archive.read(name) for name in files}


def verify_source_tree(proof: dict, run: dict, *, repo: str, pr: int, desired: str, token: str) -> None:
    base = f"https://api.github.com/repos/{repo}"
    commit = api_json(base + "/git/commits/" + proof["checkout_sha"], token)
    if commit.get("sha") != proof["checkout_sha"]:
        raise ValueError("Qualified commit object mismatch")
    if run["event"] == "pull_request":
        parents = [p.get("sha") for p in commit.get("parents", [])]
        if len(parents) != 2 or parents[0] == parents[1] or parents[1] != proof["source_sha"]:
            raise ValueError("Qualified merge does not contain the run's PR head")
    elif run["event"] == "workflow_dispatch":
        pull = api_json(base + f"/pulls/{pr}", token)
        if (pull.get("number") != pr or pull.get("head", {}).get("ref") != proof["source_ref"]
                or pull.get("head", {}).get("repo", {}).get("full_name") != repo
                or pull.get("base", {}).get("repo", {}).get("full_name") != repo):
            raise ValueError("Dispatched source is not bound to this repository's PR")
        head = pull["head"].get("sha", "")
        if not HEX40.fullmatch(head):
            raise ValueError("Invalid current PR head")
        if head != proof["source_sha"]:
            relation = api_json(base + f"/compare/{proof['source_sha']}...{head}", token)
            if relation.get("status") not in {"ahead", "identical"}:
                raise ValueError("Dispatched source is no longer in PR history")
    tree_sha = commit.get("tree", {}).get("sha", "")
    if not HEX40.fullmatch(tree_sha):
        raise ValueError("Missing qualified tree object")
    tree = api_json(base + f"/git/trees/{tree_sha}?recursive=1", token)
    if tree.get("truncated") is not False or tree.get("sha") != tree_sha:
        raise ValueError("Incomplete qualified source tree")
    entries = [(e["mode"].encode("ascii"), e["sha"].encode("ascii"), e["path"].encode("utf-8"))
               for e in tree["tree"] if e["type"] in {"blob", "commit"}]
    source_scale = next((e["sha"] for e in tree["tree"]
                         if e["type"] == "blob" and e["path"] == ".github/workflows/mp-10c-scale.yml"), None)
    if not source_scale or not HEX40.fullmatch(source_scale):
        raise ValueError("Qualified workflow missing from source tree")
    blob = api_json(base + "/git/blobs/" + source_scale, token)
    if blob.get("sha") != source_scale or blob.get("encoding") != "base64" or blob.get("size", 0) > 65536:
        raise ValueError("Invalid GitHub-hosted workflow blob")
    scale_bytes = base64.b64decode(blob["content"], validate=False)
    if hashlib.sha1(b"blob " + str(len(scale_bytes)).encode() + b"\0" + scale_bytes).hexdigest() != source_scale:
        raise ValueError("Workflow blob differs from its Git object identity")
    policy = proof.get("fingerprint_policy", "legacy/v1")
    original = fingerprint_entries(entries, legacy=(policy == "legacy/v1"),
                                   scale_workflow=scale_bytes if policy != "legacy/v1" else None)
    if original != proof["input_fingerprint"]:
        raise ValueError("Original attested fingerprint does not match GitHub tree")
    if fingerprint_entries(entries, scale_workflow=scale_bytes) != desired:
        raise ValueError("Qualified production workload/Windows execution semantics changed")


def validated_candidate(run: dict, *, token: str, repo: str, pr: int, desired: str,
                        active_run_id: int | None = None) -> dict | None:
    completed = run.get("status") == "completed" and run.get("conclusion") == "success"
    active = run.get("status") == "in_progress" and run.get("id") == active_run_id
    if (run.get("event") not in {"pull_request", "workflow_dispatch", "push"}
            or not (completed or active)
            or run.get("path") != ".github/workflows/" + WORKFLOW):
        return None
    run_id, attempt = run["id"], run["run_attempt"]
    api = f"https://api.github.com/repos/{repo}/actions/runs/{run_id}"
    artifacts = api_json(api + "/artifacts?per_page=100", token).get("artifacts", [])
    qualified = [a for a in artifacts if a.get("name") == artifact_name(ARTIFACT, attempt)]
    if len(qualified) != 1:
        return None
    try:
        item = qualified[0]
        bundle = read_verified_artifact(item, {"mp10c-provenance.json", "mp10c-100000.json"},
                                        run_id=run_id, token=token)
        proof, receipt = json.loads(bundle["mp10c-provenance.json"]), bundle["mp10c-100000.json"]
        validate_proof(proof, receipt, run=run, artifact=item, pr=pr, repo=repo, desired=desired)
        scale_attempt = proof["scale_run_attempt"]
        raw_items = [a for a in artifacts if a.get("name") == artifact_name(RAW_ARTIFACT, scale_attempt)]
        if len(raw_items) != 1:
            return None
        raw = read_verified_artifact(raw_items[0], {"mp10c-100000.json"}, run_id=run_id, token=token)
        if raw["mp10c-100000.json"] != receipt:
            return None

        def succeeded(job: dict, name: str, required_steps: tuple[str, ...]) -> bool:
            return job.get("name") == name and job.get("conclusion") == "success" and all(
                sum(s.get("name") == step and s.get("conclusion") == "success"
                    for s in job.get("steps", [])) == 1 for step in required_steps)

        # The indexing attempt remains independently verifiable when a failed
        # lightweight provenance job is rerun in a later attempt of this run.
        scale_jobs = api_json(api + f"/attempts/{scale_attempt}/jobs?per_page=100", token).get("jobs", [])
        proof_jobs = scale_jobs if scale_attempt == attempt else api_json(
            api + f"/attempts/{attempt}/jobs?per_page=100", token).get("jobs", [])
        full = [j for j in scale_jobs if succeeded(j, FULL_JOB, (
            "Qualify real authorized document scale", "Validate exact-tier receipt",
            "Upload validated raw 100K receipt"))]
        attested = [j for j in proof_jobs if succeeded(j, PROOF_JOB, (
            "Mint source-bound 100K provenance", "Upload qualified receipt and provenance"))]
        if len(full) != 1 or len(attested) != 1:
            return None
        verify_source_tree(proof, run, repo=repo, pr=pr, desired=desired, token=token)
        return {"run_id": run_id, "artifact_id": item["id"],
                "qualified_sha": proof["source_sha"], "artifact_digest": item["digest"],
                "raw_artifact_id": raw_items[0]["id"], "raw_artifact_digest": raw_items[0]["digest"],
                "proof": proof, "run_url": run["html_url"]}
    except (KeyError, ValueError, TypeError, OSError, zipfile.BadZipFile, json.JSONDecodeError):
        return None


def find_accepted(*, token: str, repo: str, pr: int, desired: str) -> dict | None:
    base = f"https://api.github.com/repos/{repo}/actions/workflows/{WORKFLOW}/runs"
    # Bounded lookup. Exhaustion, network errors, revoked artifacts => run FULL,
    # never silently accept an unverifiable qualification.
    for page in range(1, 4):
        url = base + f"?status=success&per_page=100&page={page}"
        runs = api_json(url, token).get("workflow_runs", [])
        for run in runs:
            if run.get("event") == "pull_request" and not any(x.get("number") == pr for x in run.get("pull_requests", [])):
                continue
            candidate = validated_candidate(run, token=token, repo=repo, pr=pr, desired=desired)
            if candidate:
                return candidate
        if len(runs) < 100:
            break
    return None


def decision(event: str, mode: str, previous: dict | None) -> tuple[bool, str]:
    if event != "pull_request" or mode in {"manual", "final"}:
        return True, "Explicit manual/final/release qualification"
    if mode != "auto":
        return True, "Unknown mode: fail closed"
    if previous is None:
        return True, "No verifiable successful 100K proof for identical effective inputs"
    return False, "Identical effective inputs; prior full qualification reused (NOT a new PASS)"


def require_prerequisites(runs: list[dict], candidate: dict, *, repo: str,
                          vault: bool, contract_fingerprint: str, fetch_jobs,
                          fetch_evidence, workflow_sources: dict[str, bytes | str] | None = None) -> None:
    """Accept only exact PR/base/head-bound native runs with same-run provenance."""
    if native_contract.requires_sqlcipher(repo, candidate["pr"]) is not vault:
        raise ValueError("Candidate SQLCipher policy mismatch")
    required = {
        ".github/workflows/ci.yml": {
            "Fast developer feedback": (
                "Install dependencies", "Verify repository contract", "Typecheck",
                "Test reader and browser contracts", "Build reader",
                "Validate document qualification gate policy"),
            "Fast Rust compile and vault feedback": (
                "Install pinned Rust 1.90", "Rust format", "Compile production core crates",
                "Compile available SQLCipher and vault features", "Focused security tests"),
            "Verify Windows product": (
                "Setup Node.js", "Setup Rust 1.90", "Install dependencies",
                "Check Rust formatting", "Verify repository contract", "Typecheck",
                "Visual Settings scroll and footer regression", "Qualify bundled SQLite runtime",
                "Test", "Stage capture sidecar for native desktop tests",
                "Qualify Phase 3E simulated MCP and revocation boundary",
                "Test hosted tunnel desktop boundaries", "Build desktop app",
                "Verify portable MIT license and third-party notices"),
            "Native execution evidence": (
                "Capture native execution provenance", "Upload native execution evidence"),
        }
    }
    if vault:
        required[".github/workflows/mp9b-sqlcipher-qualification.yml"] = {
            "SQLCipher / ubuntu-24.04": (
                "Check Rust formatting", "Encrypted state/index and FTS regression",
                "Keyed MCP synthetic authorization and revocation",
                "Vault and backup adversarial regression"),
            "SQLCipher / windows-2022": (
                "Check Rust formatting", "Encrypted state/index and FTS regression",
                "Keyed MCP synthetic authorization and revocation",
                "Vault and backup adversarial regression"),
        }

    for path, expected_jobs in required.items():
        if workflow_sources is not None:
            source = workflow_sources.get(path)
            if source is None:
                raise ValueError("Missing authoritative workflow source: " + path)
        else:
            source = Path(path).read_bytes()
        runner_contracts = {
            name: native_contract.workflow_job_runner_requirements(source, name)
            for name in expected_jobs
        }
        accepted = False
        for run in runs:
            if run.get("path") != path:
                continue
            try:
                validate_native_run_identity(run, candidate, repo, workflow_path=path)
            except (KeyError, TypeError, ValueError):
                continue
            if (run.get("status") != "completed" or run.get("conclusion") != "success"
                    or type(run.get("id")) is not int or run["id"] <= 0
                    or type(run.get("run_attempt")) is not int or run["run_attempt"] <= 0):
                continue
            response = fetch_jobs(run)
            jobs = response.get("jobs") if isinstance(response, dict) else None
            total_count = response.get("total_count") if isinstance(response, dict) else None
            if (not isinstance(jobs, list) or type(total_count) is not int
                    or total_count < 0 or total_count != len(jobs)
                    or any(not isinstance(job, dict) for job in jobs)):
                raise ValueError("Incomplete or malformed native prerequisite job listing")

            for name, critical_steps in expected_jobs.items():
                matching_jobs = [job for job in jobs if job.get("name") == name]
                if len(matching_jobs) != 1:
                    raise ValueError("Required native job must exist exactly once: " + name)
                job = matching_jobs[0]
                if (job.get("run_attempt") != run["run_attempt"]
                        or job.get("status") != "completed"
                        or job.get("conclusion") != "success"):
                    raise ValueError("Required native job did not complete successfully on exact attempt: " + name)
                native_contract.validate_runner_labels(job, runner_contracts[name])
                steps = job.get("steps")
                if not isinstance(steps, list) or any(not isinstance(step, dict) for step in steps):
                    raise ValueError("Missing or malformed critical steps for native job: " + name)
                for step_name in critical_steps:
                    matching_steps = [step for step in steps if step.get("name") == step_name]
                    if len(matching_steps) != 1:
                        raise ValueError("Required critical step must exist exactly once: " + step_name)
                    step = matching_steps[0]
                    if step.get("status") != "completed" or step.get("conclusion") != "success":
                        raise ValueError("Required critical step did not complete successfully: " + step_name)
            if path == ".github/workflows/ci.yml":
                evidence = fetch_evidence(run)
                validate_native_evidence(
                    evidence, run, candidate, repo=repo,
                    contract_fingerprint=contract_fingerprint, vault=vault)
            accepted = True
            break
        if not accepted:
            raise ValueError("Frozen candidate lacks exact successful native workflow jobs/steps: " + path)


def write_output(path: str, values: dict[str, str]) -> None:
    with open(path, "a", encoding="utf-8") as f:
        for k, v in values.items():
            if "\n" in v:
                raise ValueError("Multiline GitHub output")
            f.write(f"{k}={v}\n")


def main() -> int:
    p = argparse.ArgumentParser()
    p.add_argument("command", choices=("decide", "attest", "accept", "preflight", "fingerprint"))
    p.add_argument("--output")
    p.add_argument("--receipt")
    args = p.parse_args()
    fp = fingerprint()
    if args.command == "fingerprint":
        print(fp)
        return 0
    event = os.environ["GITHUB_EVENT_NAME"]
    repo = os.environ["GITHUB_REPOSITORY"]
    checkout = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()

    if args.command == "preflight":
        if os.environ.get("EXPECTED_FINGERPRINT") != fp:
            raise ValueError("Preflight workload fingerprint changed")
        token = os.environ["GH_TOKEN"]
        candidate = candidate_from_env()
        if event == "pull_request":
            check_merge_source(expected_source=candidate["head"], expected_base=candidate["base_sha"])
            native_revision = "HEAD"
            native_checkout = checkout
        elif event == "workflow_dispatch":
            payload = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_bytes())
            target = resolve_target(event=event, payload=payload, repo=repo,
                                    sha=os.environ["GITHUB_SHA"], ref=os.environ["GITHUB_REF"],
                                    token=token)
            if any(target[k] != candidate[v] for k, v in (
                ("pr", "pr"), ("head", "head"), ("branch", "branch"),
                ("head_repo", "head_repo"), ("base_repo", "base_repo"),
                ("base_branch", "base_branch"), ("base_sha", "base_sha"))):
                raise ValueError("Dispatch candidate differs from frozen PR identity")
            subprocess.check_call(["git", "fetch", "--no-tags", "origin",
                                   f"refs/pull/{candidate['pr']}/merge"])
            check_merge_source(expected_source=candidate["head"],
                               expected_base=candidate["base_sha"], revision="FETCH_HEAD")
            native_revision = "FETCH_HEAD"
            native_checkout = subprocess.check_output(
                ["git", "rev-parse", native_revision], text=True).strip()
        else:
            raise ValueError("Native prerequisites require an explicitly bound PR candidate")
        if native_checkout != candidate["checkout_sha"]:
            raise ValueError("Native merge checkout differs from frozen candidate")
        vault = native_contract.requires_sqlcipher(repo, candidate["pr"])
        contract_fp = native_contract.native_contract_fingerprint(
            ".", repo, candidate["pr"], revision=native_revision)
        if (os.environ.get("NATIVE_CONTRACT_FINGERPRINT") != contract_fp
                or os.environ.get("NATIVE_CONTRACT_POLICY") != native_contract.POLICY):
            raise ValueError("Native execution contract fingerprint changed")
        require_live_candidate(repo, candidate, token)
        runs = []
        expected_total = None
        for page in range(1, 4):
            response = api_json(
                f"https://api.github.com/repos/{repo}/actions/runs?head_sha={candidate['head']}&per_page=100&page={page}",
                token)
            page_runs = response.get("workflow_runs", [])
            total = response.get("total_count")
            if (not isinstance(page_runs, list) or type(total) is not int
                    or (expected_total is not None and total != expected_total)):
                raise ValueError("Malformed/inconsistent native workflow run listing")
            expected_total = total
            runs.extend(page_runs)
            if len(page_runs) < 100:
                break
            if page == 3:
                raise ValueError("Native workflow run pagination exceeded bounded evidence window")
        if expected_total != len(runs):
            raise ValueError("Incomplete native workflow run listing")
        def fetch_prerequisite_jobs(run):
            return api_json(
                f"https://api.github.com/repos/{repo}/actions/runs/{run['id']}"
                f"/attempts/{run['run_attempt']}/jobs?per_page=100", token)
        def fetch_evidence(run):
            return fetch_native_evidence(run, repo=repo, token=token)
        require_prerequisites(
            runs, candidate, repo=repo, vault=vault, contract_fingerprint=contract_fp,
            fetch_jobs=fetch_prerequisite_jobs, fetch_evidence=fetch_evidence)
        require_live_candidate(repo, candidate, token)
        print("Exact frozen PR/base/head identity and native execution prerequisites verified; full qualification may start")
        return 0

    if args.command == "accept":
        if os.environ.get("EXPECTED_FINGERPRINT") != fp:
            raise ValueError("Acceptance checkout differs from gate inputs")
        token, pr, run_id = os.environ["GH_TOKEN"], int(os.environ["PR_NUMBER"]), int(os.environ["GITHUB_RUN_ID"])
        current = api_json(f"https://api.github.com/repos/{repo}/actions/runs/{run_id}", token)
        accepted = validated_candidate(current, token=token, repo=repo, pr=pr, desired=fp,
                                       active_run_id=run_id)
        fresh = accepted is not None
        if not fresh and event == "pull_request" and os.environ.get("QUALIFICATION_MODE", "auto") == "auto":
            accepted = find_accepted(token=token, repo=repo, pr=pr, desired=fp)
        if not accepted:
            raise ValueError("Final 100K qualification REQUIRED: no authenticated matching evidence")
        report = ("Fresh full 100K qualification accepted" if fresh else
                  "Prior full 100K proof reused for identical inputs; NOT a new 100K PASS")
        print(report, json.dumps(accepted, sort_keys=True))
        summary = os.environ.get("GITHUB_STEP_SUMMARY")
        if summary:
            with open(summary, "a", encoding="utf-8") as stream:
                stream.write(f"## {report}\n\nRun: {accepted['run_url']}\n")
        return 0

    if args.command == "attest":
        if not args.receipt or not args.output:
            p.error("attest requires --receipt and --output")
        receipt = Path(args.receipt).read_bytes()
        candidate = candidate_from_env()
        token = os.environ["GH_TOKEN"]
        require_live_candidate(repo, candidate, token)
        if event == "pull_request":
            check_merge_source(expected_source=candidate["head"],
                               expected_base=candidate["base_sha"])
            native_revision = "HEAD"
            native_checkout = checkout
        elif event == "workflow_dispatch":
            payload = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_bytes())
            target = resolve_target(event=event, payload=payload, repo=repo,
                                    sha=os.environ["GITHUB_SHA"], ref=os.environ["GITHUB_REF"],
                                    token=token)
            if any(target[k] != candidate[v] for k, v in (
                ("pr", "pr"), ("head", "head"), ("branch", "branch"),
                ("head_repo", "head_repo"), ("base_repo", "base_repo"),
                ("base_branch", "base_branch"), ("base_sha", "base_sha"))):
                raise ValueError("Attestation candidate differs from frozen PR identity")
            subprocess.check_call(["git", "fetch", "--no-tags", "origin",
                                   f"refs/pull/{candidate['pr']}/merge"])
            check_merge_source(expected_source=candidate["head"],
                               expected_base=candidate["base_sha"], revision="FETCH_HEAD")
            native_revision = "FETCH_HEAD"
            native_checkout = subprocess.check_output(
                ["git", "rev-parse", native_revision], text=True).strip()
        else:
            raise ValueError("100K provenance requires an explicitly bound PR candidate")
        if native_checkout != candidate["checkout_sha"]:
            raise ValueError("Attestation native merge checkout differs from frozen candidate")
        contract_fp = native_contract.native_contract_fingerprint(
            ".", repo, candidate["pr"], revision=native_revision)
        if (os.environ.get("NATIVE_CONTRACT_FINGERPRINT") != contract_fp
                or os.environ.get("NATIVE_CONTRACT_POLICY") != native_contract.POLICY):
            raise ValueError("Native execution contract changed before provenance attestation")
        proof = make_proof(
            receipt=receipt, source=checkout,
            run_id=int(os.environ["GITHUB_RUN_ID"]),
            attempt=int(os.environ["GITHUB_RUN_ATTEMPT"]),
            pr=int(os.environ.get("PR_NUMBER") or 0),
            repo=repo, event=event,
            effective=fp, head_sha=os.environ["PR_HEAD_SHA"], head_ref=os.environ["PR_HEAD_REF"],
            scale_attempt=int(os.environ["SCALE_RUN_ATTEMPT"]))
        if os.environ.get("EXPECTED_FINGERPRINT") != fp:
            raise ValueError("Checkout/input TOCTOU mismatch: gate and full job differ")
        require_live_candidate(repo, candidate, token)
        Path(args.output).write_text(json.dumps(proof, indent=2, sort_keys=True) + "\n")
        print("Verified full 100K receipt and minted provenance:", json.dumps(proof, sort_keys=True))
        return 0

    if not args.output:
        p.error("decide requires --output")
    payload = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_bytes())
    target = resolve_target(event=event, payload=payload, repo=repo,
                            sha=os.environ["GITHUB_SHA"], ref=os.environ["GITHUB_REF"],
                            token=os.environ["GH_TOKEN"])
    native_contract_fp = ""
    native_checkout = ""
    if event == "pull_request":
        check_merge_source(expected_source=target["head"], expected_base=target["base_sha"])
        native_checkout = checkout
        native_revision = "HEAD"
    elif event == "workflow_dispatch":
        if checkout != target["head"]:
            raise ValueError("Manual qualification checkout is not the exact PR head")
        subprocess.check_call(["git", "fetch", "--no-tags", "origin",
                               f"refs/pull/{target['pr']}/merge"])
        check_merge_source(expected_source=target["head"],
                           expected_base=target["base_sha"], revision="FETCH_HEAD")
        native_revision = "FETCH_HEAD"
        native_checkout = subprocess.check_output(
            ["git", "rev-parse", native_revision], text=True).strip()
    elif event == "push":
        native_revision = "HEAD"
    else:
        raise ValueError("Unsupported qualification event")
    if target["pr"] > 0:
        native_contract_fp = native_contract.native_contract_fingerprint(
            ".", repo, target["pr"], revision=native_revision)
    mode = os.environ.get("QUALIFICATION_MODE", "auto")
    previous = None
    fault = ""
    if event == "pull_request" and mode == "auto":
        try:
            previous = find_accepted(
                token=os.environ["GH_TOKEN"],
                repo=repo, pr=target["pr"], desired=fp)
        except (KeyError, ValueError, OSError, urllib.error.URLError, TimeoutError) as exc:
            fault = f"Unable to verify cached proof: {type(exc).__name__}; fail closed."
    run_full, reason = decision(event, mode, previous)
    reason = fault or reason
    write_output(args.output, {
        "run-heavy": str(run_full).lower(),
        "fingerprint": fp,
        "qualified-run": str(previous["run_id"] if previous else ""),
        "qualified-artifact": str(previous["artifact_id"] if previous else ""),
        "pr-number": str(target["pr"]), "source-sha": target["head"],
        "source-ref": target["branch"], "checkout-sha": checkout,
        "head-repository": target["head_repo"],
        "head-repository-id": str(target["head_repo_id"] or ""),
        "base-repository": target["base_repo"],
        "base-repository-id": str(target["base_repo_id"] or ""),
        "base-ref": target["base_branch"], "base-sha": target["base_sha"],
        "native-contract-policy": native_contract.POLICY if native_contract_fp else "",
        "native-contract-fingerprint": native_contract_fp,
        "native-checkout-sha": native_checkout,
    })
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        lines = [
            "## MP-10C 100K input gate",
            f"- Effective workload fingerprint: {fp}",
            f"- Native execution contract: {native_contract_fp or 'NOT APPLICABLE'}",
            f"- Full 100K test on this run: {'REQUIRED' if run_full else 'NOT RUN'}",
            f"- Decision: {reason}",
        ]
        if previous and not run_full:
            lines.extend([
                f"- Previously qualified checkout SHA: {previous['qualified_sha']}",
                f"- Prior successful Windows 100K run: {previous['run_url']}",
                f"- Artifact ID: {previous['artifact_id']}; SHA256: {previous['artifact_digest']}",
                "- **This head is not newly 100K-qualified.** The proof covers identical effective inputs only.",
            ])
        Path(summary).open("a", encoding="utf-8").write("\n".join(lines) + "\n")
    print(json.dumps({"run_heavy": run_full, "fingerprint": fp,
                      "reuse": previous, "reason": reason}, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
