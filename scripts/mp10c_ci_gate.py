#!/usr/bin/env python3
"""MP-CI-100K-01: fail-closed, source-bound qualification reuse for MP-10C.

Only the original Windows 100,000-document job may mint an accepted provenance
receipt. An unrelated PR push may reuse that proof, but is never itself called a
new full-scale PASS. No owner files are accessed.
"""
from __future__ import annotations

import argparse
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

SCHEMA = "mind-place-mp10c-qualification/v1"
ARTIFACT = "mp10c-qualified-100000"
WORKFLOW = "mp-10c-scale.yml"
FULL_JOB = "Windows approved documents / 100000 files"
HEX64 = re.compile(r"^[0-9a-f]{64}$")
HEX40 = re.compile(r"^[0-9a-f]{40}$")


def excluded(path: str) -> bool:
    # Deliberately conservative. ALL tracked project files are dependencies
    # unless explicitly proven inert to the Windows Rust scale qualification.
    # Never exclude crates, Rust workspace/dependency/toolchain, test fixtures,
    # security/authority code, build scripts, .cargo config or this gate.
    return (
        path.startswith("docs/")
        or path in {"README.md", "LICENSE", "LICENSE.md", "CHANGELOG.md"}
        or path.startswith(".github/ISSUE_TEMPLATE/")
        or path.startswith(".github/DISCUSSION_TEMPLATE/")
        or path in {".github/PULL_REQUEST_TEMPLATE.md", ".github/workflows/ci.yml"}
        or (path.startswith(".github/workflows/") and path != ".github/workflows/mp-10c-scale.yml")
    )


def fingerprint(root: str = ".") -> str:
    """Hash all effective tracked file mode/blob tuples, including deletions.

    git index is the checkout's canonical tree; this also includes gitlinks,
    symlink mode, Cargo.lock and the runner/test workflow. Excluded paths are
    narrowly documented non-inputs, not an allowlist of possible dependencies.
    """
    records = subprocess.check_output(["git", "ls-files", "--stage", "-z"], cwd=root)
    digest = hashlib.sha256(b"mp10c-effective-inputs/v1\0")
    included = 0
    for record in records.split(b"\0"):
        if not record:
            continue
        meta, path_bytes = record.split(b"\t", 1)
        mode, oid, stage = meta.split(b" ")
        if stage != b"0":
            raise ValueError("Unmerged index: refusing fingerprint")
        path = path_bytes.decode("utf-8", errors="surrogateescape")
        if excluded(path):
            continue
        digest.update(mode + b"\0" + oid + b"\0" + path_bytes + b"\0")
        included += 1
    if not included:
        raise ValueError("Empty effective-input set: refusing fingerprint")
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
    source = proof.get("checkout_sha")
    if not isinstance(source, str) or not HEX40.fullmatch(source):
        raise ValueError("Invalid qualified checkout SHA")
    if proof.get("run_id") != run.get("id") or proof.get("run_attempt") != run.get("run_attempt"):
        raise ValueError("Provenance does not belong to successful run attempt")
    if proof.get("repo") != repo or proof.get("pr_number") != pr or proof.get("event") != "pull_request":
        raise ValueError("Proof belongs to another repository, PR, or event")
    if proof.get("checkout_sha") != run.get("head_sha"):
        raise ValueError("Checkout SHA != GitHub's qualified run")
    if proof.get("tier") != 100000 or proof.get("qualified") is not True:
        raise ValueError("Proof does not attest completed full tier")
    if proof.get("input_fingerprint") != desired or not HEX64.fullmatch(desired):
        raise ValueError("Relevant effective sources changed")
    if proof.get("raw_receipt_sha256") != hashlib.sha256(raw_bytes).hexdigest():
        raise ValueError("Raw acceptance receipt does not match provenance hash")
    require_raw_receipt(json.loads(raw_bytes))
    if artifact.get("expired") is not False or artifact.get("name") != ARTIFACT:
        raise ValueError("Artifact expired/misnamed")
    if artifact.get("workflow_run", {}).get("id") != run.get("id"):
        raise ValueError("Artifact belongs to different workflow run")


def make_proof(*, receipt: bytes, source: str, run_id: int, attempt: int,
               pr: int, repo: str, event: str, effective: str) -> dict:
    require_raw_receipt(json.loads(receipt))
    if not HEX40.fullmatch(source) or not HEX64.fullmatch(effective):
        raise ValueError("Invalid source binding")
    if not all(type(x) is int and x > 0 for x in (run_id, attempt)):
        raise ValueError("Missing workflow identity")
    if not repo or event not in {"pull_request", "workflow_dispatch", "push"}:
        raise ValueError("Missing workflow identity")
    return {
        "schema": SCHEMA, "tier": 100000, "qualified": True,
        "repo": repo, "event": event, "pr_number": pr,
        "checkout_sha": source, "input_fingerprint": effective,
        "run_id": run_id, "run_attempt": attempt,
        "raw_receipt_sha256": hashlib.sha256(receipt).hexdigest(),
    }


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


def validated_candidate(run: dict, *, token: str, repo: str, pr: int, desired: str) -> dict | None:
    if (run.get("event") != "pull_request" or run.get("status") != "completed"
            or run.get("conclusion") != "success" or run.get("path") != ".github/workflows/" + WORKFLOW):
        return None
    run_id = run["id"]
    api = f"https://api.github.com/repos/{repo}/actions/runs/{run_id}"
    jobs = api_json(api + "/jobs?per_page=100", token).get("jobs", [])
    # A completed workflow containing only a previously-skipped heavyweight
    # job is NOT an authoritative qualification, even when overall run green.
    proven = [j for j in jobs if j.get("name") == FULL_JOB
              and j.get("conclusion") == "success"
              and all(any(s.get("name") == step and s.get("conclusion") == "success"
                          for s in j.get("steps", [])) for step in (
                  "Qualify real authorized document scale", "Validate exact-tier receipt",
                  "Mint source-bound 100K provenance"))]
    if len(proven) != 1:
        return None
    artifacts = api_json(api + "/artifacts?per_page=100", token).get("artifacts", [])
    for item in artifacts:
        if item.get("name") != ARTIFACT or item.get("expired") is not False:
            continue
        try:
            zip_bytes = artifact_bytes(item["archive_download_url"], token)
            archive_digest = item.get("digest", "")
            if not re.fullmatch(r"sha256:[0-9a-f]{64}", archive_digest or ""):
                continue
            if hashlib.sha256(zip_bytes).hexdigest() != archive_digest.removeprefix("sha256:"):
                continue
            with zipfile.ZipFile(io.BytesIO(zip_bytes)) as z:
                expected_files = {"mp10c-provenance.json", "mp10c-100000.json"}
                if set(z.namelist()) != expected_files:
                    continue
                if any(z.getinfo(name).file_size > 65536 for name in expected_files):
                    continue
                proof = json.loads(z.read("mp10c-provenance.json"))
                receipt = z.read("mp10c-100000.json")
            validate_proof(proof, receipt, run=run, artifact=item,
                           pr=pr, repo=repo, desired=desired)
            return {"run_id": run_id, "artifact_id": item["id"],
                    "qualified_sha": proof["checkout_sha"], "artifact_digest": archive_digest,
                    "proof": proof, "run_url": run["html_url"]}
        except (KeyError, ValueError, TypeError, OSError, zipfile.BadZipFile, json.JSONDecodeError):
            continue
    return None


def find_accepted(*, token: str, repo: str, pr: int, desired: str) -> dict | None:
    base = f"https://api.github.com/repos/{repo}/actions/workflows/{WORKFLOW}/runs"
    # Bounded lookup. Exhaustion, network errors, revoked artifacts => run FULL,
    # never silently accept an unverifiable qualification.
    for page in range(1, 4):
        url = base + f"?event=pull_request&status=success&per_page=100&page={page}"
        runs = api_json(url, token).get("workflow_runs", [])
        for run in runs:
            if not any(x.get("number") == pr for x in run.get("pull_requests", [])):
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


def write_output(path: str, values: dict[str, str]) -> None:
    with open(path, "a", encoding="utf-8") as f:
        for k, v in values.items():
            if "\n" in v:
                raise ValueError("Multiline GitHub output")
            f.write(f"{k}={v}\n")


def main() -> int:
    p = argparse.ArgumentParser()
    p.add_argument("command", choices=("decide", "attest", "fingerprint"))
    p.add_argument("--output")
    p.add_argument("--receipt")
    args = p.parse_args()
    fp = fingerprint()
    if args.command == "fingerprint":
        print(fp)
        return 0
    if args.command == "attest":
        if not args.receipt or not args.output:
            p.error("attest requires --receipt and --output")
        receipt = Path(args.receipt).read_bytes()
        proof = make_proof(
            receipt=receipt, source=subprocess.check_output(
                ["git", "rev-parse", "HEAD"], text=True).strip(),
            run_id=int(os.environ["GITHUB_RUN_ID"]),
            attempt=int(os.environ["GITHUB_RUN_ATTEMPT"]),
            pr=int(os.environ.get("PR_NUMBER") or 0),
            repo=os.environ["GITHUB_REPOSITORY"],
            event=os.environ["GITHUB_EVENT_NAME"],
            effective=fp)
        expected = os.environ.get("EXPECTED_FINGERPRINT")
        if expected != fp:
            raise ValueError("Checkout/input TOCTOU mismatch: gate and full job differ")
        Path(args.output).write_text(json.dumps(proof, indent=2, sort_keys=True) + "\n")
        print("Verified full 100K receipt and minted provenance:", json.dumps(proof, sort_keys=True))
        return 0
    if not args.output:
        p.error("decide requires --output")
    event = os.environ["GITHUB_EVENT_NAME"]
    mode = os.environ.get("QUALIFICATION_MODE", "auto")
    previous = None
    fault = ""
    if event == "pull_request" and mode == "auto":
        try:
            previous = find_accepted(
                token=os.environ["GH_TOKEN"],
                repo=os.environ["GITHUB_REPOSITORY"],
                pr=int(os.environ["PR_NUMBER"]), desired=fp)
        except (KeyError, ValueError, OSError, urllib.error.URLError, TimeoutError) as exc:
            fault = f"Unable to verify cached proof: {type(exc).__name__}; fail closed."
    run_full, reason = decision(event, mode, previous)
    reason = fault or reason
    write_output(args.output, {
        "run-heavy": str(run_full).lower(),
        "fingerprint": fp,
        "qualified-run": str(previous["run_id"] if previous else ""),
        "qualified-artifact": str(previous["artifact_id"] if previous else ""),
    })
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        lines = [
            "## MP-10C 100K input gate",
            f"- Effective input fingerprint: \`{fp}\`",
            f"- Full 100K test on this run: **{'REQUIRED' if run_full else 'NOT RUN'}**",
            f"- Decision: {reason}",
        ]
        if previous and not run_full:
            lines.extend([
                f"- Previously qualified checkout SHA: \`{previous['qualified_sha']}\`",
                f"- Prior successful Windows 100K run: {previous['run_url']}",
                f"- Artifact ID: \`{previous['artifact_id']}\`; SHA256: \`{previous['artifact_digest']}\`",
                "- **This head is not newly 100K-qualified.** The proof covers identical effective inputs only.",
            ])
        Path(summary).open("a", encoding="utf-8").write("\n".join(lines) + "\n")
    print(json.dumps({"run_heavy": run_full, "fingerprint": fp,
                      "reuse": previous, "reason": reason}, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
