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
        "scripts/mp10c_prerequisite_wait.py", ".github/workflows/mp-10c-scale.yml"})


def scale_workflow_signature(contents: bytes) -> bytes:
    """Hash the actual Windows smoke + full job execution contract.

    Cross-workflow orchestration and scheduling are deliberately out of scope,
    but Windows runner, timeouts, build flags, test commands, receipt acceptance,
    artifact upload, and critical step guards remain bound. Unparseable YAML
    shape fails closed; changes inside either executed job force reindexing.
    """
    source = contents.decode("utf-8")
    digest = hashlib.sha256(b"mp10c-windows-scale-steps/v1\0")
    for name in ("document-smoke", "document-scale"):
        begin = re.search(r"(?m)^  " + re.escape(name) + r":\s*$", source)
        if not begin:
            raise ValueError("Scale workflow missing required Windows tier: " + name)
        next_job = re.search(r"(?m)^  [a-z][a-z0-9-]*:\s*$", source[begin.end():])
        body = source[begin.end(): begin.end() + next_job.start() if next_job else len(source)]
        runner = re.search(r"(?m)^    runs-on: windows-latest\s*$", body)
        if not runner:
            raise ValueError("Scale workflow runner contract changed: " + name)
        contract = body[runner.start():].rstrip()
        if not re.search(r"(?m)^    steps:\s*$", contract):
            raise ValueError("Missing Windows scale steps: " + name)
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


def check_merge_source(*, repo_root: str = ".", expected_source: str) -> None:
    """Read the actual merge commit object; works even at a shallow boundary.

    'git rev-list --parents' intentionally hides shallow-boundary parents.
    cat-file exposes the signed-by-Git object content, independent of ancestry
    traversal. Reject an absent, non-merge, or differently parented checkout.
    """
    if not HEX40.fullmatch(expected_source):
        raise ValueError("Invalid expected PR source SHA")
    head = subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=repo_root, text=True).strip()
    data = subprocess.check_output(
        ["git", "cat-file", "-p", head], cwd=repo_root, text=True)
    headers = data.split("\n\n", 1)[0].splitlines()
    parents = [line[7:] for line in headers if line.startswith("parent ")]
    if len(parents) != 2 or any(not HEX40.fullmatch(p) for p in parents):
        raise ValueError("Expected a two-parent PR checkout merge")
    if parents[0] == parents[1] or parents[1] != expected_source:
        raise ValueError("PR checkout merge does not match branch source SHA")


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
    """Bind manual qualification to an open, same-repository PR at dispatch.

    A dispatch runs on the PR's own branch, tests its exact immutable head,
    and never silently qualifies the default branch or PR zero.
    """
    if event == "push":
        if not ref.startswith("refs/tags/") or not HEX40.fullmatch(sha):
            raise ValueError("Only a release tag may qualify without a PR")
        return {"pr": 0, "head": sha, "branch": ref.removeprefix("refs/tags/")}
    if event == "pull_request":
        pull = payload["pull_request"]
    elif event == "workflow_dispatch":
        number = payload.get("inputs", {}).get("pr_number", "")
        if not isinstance(number, str) or not re.fullmatch(r"[1-9][0-9]*", number):
            raise ValueError("Manual/final qualification requires pr_number")
        pull = api_json(f"https://api.github.com/repos/{repo}/pulls/{number}", token)
        if pull.get("number") != int(number) or pull.get("state") != "open":
            raise ValueError("Manual target PR is not open")
        if pull.get("head", {}).get("sha") != sha or ref != "refs/heads/" + pull.get("head", {}).get("ref", ""):
            raise ValueError("Dispatch must select the current target PR head branch")
    else:
        raise ValueError("Unsupported qualification event")
    if (pull.get("head", {}).get("repo", {}).get("full_name") != repo
            or pull.get("base", {}).get("repo", {}).get("full_name") != repo):
        raise ValueError("Cross-repository qualification is not authorized")
    number, head, branch = pull.get("number"), pull["head"].get("sha"), pull["head"].get("ref")
    if type(number) is not int or number <= 0 or not HEX40.fullmatch(head or "") or not branch:
        raise ValueError("Invalid PR source identity")
    return {"pr": number, "head": head, "branch": branch}


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


def require_prerequisites(runs: list[dict], source: str, *, vault: bool, fetch_jobs) -> None:
    # A workflow can be green while its native job was skipped on a Draft PR.
    # Inspect the exact run attempt's required jobs AND their critical steps.
    required = {
        ".github/workflows/ci.yml": {
            "Verify Windows product": ("Test", "Test hosted tunnel desktop boundaries", "Build desktop app")
        }
    }
    if vault:
        required[".github/workflows/mp9b-sqlcipher-qualification.yml"] = {
            "SQLCipher / ubuntu-24.04": ("Encrypted state/index and FTS regression",
                                          "Keyed MCP synthetic authorization and revocation",
                                          "Vault and backup adversarial regression"),
            "SQLCipher / windows-2022": ("Encrypted state/index and FTS regression",
                                         "Keyed MCP synthetic authorization and revocation",
                                         "Vault and backup adversarial regression"),
        }
    for path, expected_jobs in required.items():
        verified = False
        for run in runs:
            if (run.get("path") != path or run.get("head_sha") != source
                    or run.get("status") != "completed" or run.get("conclusion") != "success"
                    or type(run.get("id")) is not int or run["id"] <= 0
                    or type(run.get("run_attempt")) is not int or run["run_attempt"] <= 0):
                continue
            response = fetch_jobs(run)
            jobs = response.get("jobs", [])
            if (not isinstance(jobs, list) or type(response.get("total_count")) is not int
                    or response["total_count"] != len(jobs)):
                continue  # incomplete/paginated evidence is not acceptance
            if all(sum(job.get("name") == name and job.get("status") == "completed"
                       and job.get("conclusion") == "success" and all(
                           sum(step.get("name") == step_name and step.get("conclusion") == "success"
                               for step in job.get("steps", [])) == 1 for step_name in steps)
                       for job in jobs) == 1 for name, steps in expected_jobs.items()):
                verified = True
                break
        if not verified:
            raise ValueError("Frozen source lacks a successful native Windows Product or SQLCipher job/step prerequisite")


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
            raise ValueError("Preflight input fingerprint changed")
        source = os.environ["PR_HEAD_SHA"]
        if not HEX40.fullmatch(source):
            raise ValueError("Invalid prerequisite source")
        token = os.environ["GH_TOKEN"]
        pr = int(os.environ.get("PR_NUMBER") or "0")
        if pr:
            pull = api_json(f"https://api.github.com/repos/{repo}/pulls/{pr}", token)
            if (pull.get("state") != "open"
                    or pull.get("head", {}).get("sha") != source
                    or pull.get("head", {}).get("repo", {}).get("full_name") != repo):
                raise ValueError("Frozen PR head was superseded before expensive indexing")
        runs = api_json(f"https://api.github.com/repos/{repo}/actions/runs?head_sha={source}&per_page=100",
                        token).get("workflow_runs", [])
        def fetch_prerequisite_jobs(run):
            return api_json(f"https://api.github.com/repos/{repo}/actions/runs/{run['id']}"
                            f"/attempts/{run['run_attempt']}/jobs?per_page=100", token)
        require_prerequisites(runs, source,
                              vault=Path(".github/workflows/mp9b-sqlcipher-qualification.yml").is_file(),
                              fetch_jobs=fetch_prerequisite_jobs)
        print("Frozen source cheap CI prerequisites verified; full qualification may start")
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
        proof = make_proof(
            receipt=receipt, source=checkout,
            run_id=int(os.environ["GITHUB_RUN_ID"]),
            attempt=int(os.environ["GITHUB_RUN_ATTEMPT"]),
            pr=int(os.environ.get("PR_NUMBER") or 0),
            repo=repo, event=event,
            effective=fp, head_sha=os.environ["PR_HEAD_SHA"], head_ref=os.environ["PR_HEAD_REF"],
            scale_attempt=int(os.environ["SCALE_RUN_ATTEMPT"]))
        if event == "pull_request":
            check_merge_source(expected_source=proof["source_sha"])
        elif event == "workflow_dispatch":
            payload = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_bytes())
            if (payload.get("inputs", {}).get("pr_number") != str(proof["pr_number"])
                    or proof["source_sha"] != os.environ["GITHUB_SHA"]
                    or checkout != proof["source_sha"]
                    or os.environ["GITHUB_REF"] != "refs/heads/" + proof["source_ref"]):
                raise ValueError("Dispatch event, PR and tested checkout disagree")
        expected = os.environ.get("EXPECTED_FINGERPRINT")
        if expected != fp:
            raise ValueError("Checkout/input TOCTOU mismatch: gate and full job differ")
        Path(args.output).write_text(json.dumps(proof, indent=2, sort_keys=True) + "\n")
        print("Verified full 100K receipt and minted provenance:", json.dumps(proof, sort_keys=True))
        return 0
    if not args.output:
        p.error("decide requires --output")
    payload = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_bytes())
    target = resolve_target(event=event, payload=payload, repo=repo,
                            sha=os.environ["GITHUB_SHA"], ref=os.environ["GITHUB_REF"],
                            token=os.environ["GH_TOKEN"])
    if event == "pull_request":
        check_merge_source(expected_source=target["head"])
    elif event == "workflow_dispatch" and checkout != target["head"]:
        raise ValueError("Manual checkout is not the resolved target PR head")
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
    })
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        lines = [
            "## MP-10C 100K input gate",
            f"- Effective input fingerprint: `{fp}`",
            f"- Full 100K test on this run: **{'REQUIRED' if run_full else 'NOT RUN'}**",
            f"- Decision: {reason}",
        ]
        if previous and not run_full:
            lines.extend([
                f"- Previously qualified checkout SHA: `{previous['qualified_sha']}`",
                f"- Prior successful Windows 100K run: {previous['run_url']}",
                f"- Artifact ID: `{previous['artifact_id']}`; SHA256: `{previous['artifact_digest']}`",
                "- **This head is not newly 100K-qualified.** The proof covers identical effective inputs only.",
            ])
        Path(summary).open("a", encoding="utf-8").write("\n".join(lines) + "\n")
    print(json.dumps({"run_heavy": run_full, "fingerprint": fp,
                      "reuse": previous, "reason": reason}, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
