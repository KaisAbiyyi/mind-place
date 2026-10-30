#!/usr/bin/env python3
"""Deterministic, separately versioned native execution contract and run evidence."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

POLICY = "mp-native-contract/v1"
EVIDENCE_SCHEMA = "mind-place-native-evidence/v1"
CI_WORKFLOW = ".github/workflows/ci.yml"
SQLCIPHER_WORKFLOW = ".github/workflows/mp9b-sqlcipher-qualification.yml"

# Explicit candidate policy: PR 127 is MP-9B vault; PR 129 is CI infrastructure.
CANDIDATE_REQUIREMENTS = {
    ("KaisAbiyyi/mind-place", 127): ("mp9b-vault", True),
    ("KaisAbiyyi/mind-place", 129): ("mp-ci-100k-infrastructure", False),
}
DEFAULT_REQUIREMENT = ("ordinary-non-vault-pr", False)

REQUIRED_FILES = {
    CI_WORKFLOW, "Cargo.toml", "Cargo.lock", "package.json", "package-lock.json",
    "src-tauri/Cargo.toml", "src-tauri/tauri.conf.json",
    "scripts/mp10c_native_contract.py", "scripts/mp10c_ci_gate.py",
    "scripts/mp10c_prerequisite_wait.py", "scripts/tests/test_mp10c_ci_gate.py",
}
OPTIONAL_FILES = {
    ".cargo/config.toml", ".npmrc", ".nvmrc", "rust-toolchain", "rust-toolchain.toml", "build.rs",
    "index.html", "vite.config.ts", "vite.config.js", "tsconfig.json", "tsconfig.node.json",
}
INPUT_PREFIXES = (
    ".cargo/", ".github/actions/", "crates/", "src/", "src-tauri/", "test/", "scripts/",
    "extension/", "public/",
)
OPTIONAL_ROOT_PREFIXES = ("vite.config.", "tsconfig.", "postcss.config.", "tailwind.config.")


def candidate_requirement(repo: str, pr: int) -> tuple[str, bool]:
    if not isinstance(repo, str) or not repo or type(pr) is not int or pr <= 0:
        raise ValueError("Invalid native candidate identity")
    return CANDIDATE_REQUIREMENTS.get((repo, pr), DEFAULT_REQUIREMENT)


def requires_sqlcipher(repo: str, pr: int) -> bool:
    return candidate_requirement(repo, pr)[1]


def _tree_entries(root: str, revision: str) -> list[tuple[str, str, str]]:
    raw = subprocess.check_output(
        ["git", "ls-tree", "-r", "-z", "--full-tree", revision], cwd=root
    )
    result = []
    for record in raw.split(b"\0"):
        if not record:
            continue
        metadata, path_bytes = record.split(b"\t", 1)
        mode, kind, oid = metadata.split(b" ")
        path = path_bytes.decode("utf-8", errors="strict")
        if kind != b"blob":
            if path.startswith(INPUT_PREFIXES):
                raise ValueError("Unsupported non-blob native input: " + path)
            continue
        result.append((path, mode.decode("ascii"), oid.decode("ascii")))
    return result


def native_contract_manifest(root: str = ".", repo: str = "KaisAbiyyi/mind-place",
                             pr: int = 129, revision: str = "HEAD") -> dict:
    kind, sqlcipher_required = candidate_requirement(repo, pr)
    entries = _tree_entries(root, revision)
    by_path = {path: (mode, oid) for path, mode, oid in entries}
    required = set(REQUIRED_FILES)
    if sqlcipher_required:
        required.add(SQLCIPHER_WORKFLOW)
    missing = sorted(required - set(by_path))
    if missing:
        raise ValueError("Missing required native execution inputs: " + ", ".join(missing))

    selected = []
    for path, (mode, oid) in by_path.items():
        include = (
            path in required or path in OPTIONAL_FILES
            or path.startswith(INPUT_PREFIXES)
            or (path.startswith(OPTIONAL_ROOT_PREFIXES) and "/" not in path)
            or (sqlcipher_required and path == SQLCIPHER_WORKFLOW)
        )
        if not include:
            continue
        content = subprocess.check_output(["git", "cat-file", "blob", oid], cwd=root)
        selected.append({
            "path": path, "mode": mode, "sha256": hashlib.sha256(content).hexdigest(),
        })
    selected.sort(key=lambda item: item["path"])
    if not selected:
        raise ValueError("Empty native execution contract")
    return {
        "policy": POLICY, "candidate_kind": kind,
        "sqlcipher_required": sqlcipher_required, "inputs": selected,
    }


def native_contract_fingerprint(root: str = ".", repo: str = "KaisAbiyyi/mind-place",
                                pr: int = 129, revision: str = "HEAD") -> str:
    manifest = native_contract_manifest(root, repo, pr, revision)
    canonical = json.dumps(
        manifest, sort_keys=True, separators=(",", ":"), ensure_ascii=False
    ).encode("utf-8")
    return hashlib.sha256(POLICY.encode("ascii") + b"\0" + canonical).hexdigest()


def _validate_sha(value: object, name: str) -> str:
    if (not isinstance(value, str) or len(value) != 40
            or any(c not in "0123456789abcdef" for c in value)):
        raise ValueError("Invalid " + name)
    return value


def _verify_merge_checkout(root: str, checkout_sha: str, base_sha: str, head_sha: str) -> None:
    actual = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()
    if actual != checkout_sha:
        raise ValueError("Native evidence checkout SHA changed")
    raw = subprocess.check_output(["git", "cat-file", "-p", checkout_sha], cwd=root, text=True)
    headers = raw.split("\n\n", 1)[0].splitlines()
    parents = [line[7:] for line in headers if line.startswith("parent ")]
    if parents != [base_sha, head_sha]:
        raise ValueError("Native checkout is not the exact frozen base/head merge")


def capture_native_evidence(root: str = ".", payload: dict | None = None, *,
                            repo: str, run_id: int, attempt: int,
                            event_name: str = "pull_request",
                            github_sha: str | None = None) -> dict:
    if event_name != "pull_request" or not isinstance(payload, dict):
        raise ValueError("Only pull_request events can mint native prerequisite evidence")
    pull = payload.get("pull_request")
    if not isinstance(pull, dict):
        raise ValueError("Missing pull_request event identity")
    pr = payload.get("number")
    if type(pr) is not int or pr <= 0 or pull.get("number", pr) != pr:
        raise ValueError("Ambiguous or invalid PR number")
    head, base = pull.get("head"), pull.get("base")
    if not isinstance(head, dict) or not isinstance(base, dict):
        raise ValueError("Missing PR head/base event identity")
    head_repo, base_repo = head.get("repo") or {}, base.get("repo") or {}
    head_repo_name, base_repo_name = head_repo.get("full_name"), base_repo.get("full_name")
    head_repo_id, base_repo_id = head_repo.get("id"), base_repo.get("id")
    if (head_repo_name != repo or base_repo_name != repo
            or type(head_repo_id) is not int or head_repo_id <= 0
            or type(base_repo_id) is not int or base_repo_id <= 0
            or head_repo_id != base_repo_id):
        raise ValueError("Cross-repository or unverifiable native evidence repository identity")
    head_sha = _validate_sha(head.get("sha"), "PR head SHA")
    base_sha = _validate_sha(base.get("sha"), "PR base SHA")
    head_ref, base_ref = head.get("ref"), base.get("ref")
    if (not isinstance(head_ref, str) or not head_ref
            or not isinstance(base_ref, str) or not base_ref):
        raise ValueError("Missing PR head/base branch")
    if type(run_id) is not int or run_id <= 0 or type(attempt) is not int or attempt <= 0:
        raise ValueError("Missing workflow run identity")
    checkout_sha = _validate_sha(
        subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip(),
        "native checkout SHA",
    )
    if github_sha and github_sha != checkout_sha:
        raise ValueError("Native checkout does not match GITHUB_SHA")
    _verify_merge_checkout(root, checkout_sha, base_sha, head_sha)
    kind, sqlcipher_required = candidate_requirement(repo, pr)
    return {
        "schema": EVIDENCE_SCHEMA, "repository": repo, "event": event_name,
        "workflow_path": CI_WORKFLOW, "pr_number": pr,
        "head_repository": head_repo_name, "head_repository_id": head_repo.get("id"),
        "head_branch": head_ref, "head_sha": head_sha,
        "base_repository": base_repo_name, "base_repository_id": base_repo.get("id"),
        "base_branch": base_ref, "base_sha": base_sha, "checkout_sha": checkout_sha,
        "candidate_kind": kind, "sqlcipher_required": sqlcipher_required,
        "native_contract_policy": POLICY,
        "native_contract_fingerprint": native_contract_fingerprint(root, repo, pr),
        "workflow_run_id": run_id, "workflow_run_attempt": attempt,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("command", choices=("fingerprint", "attest"))
    parser.add_argument("--pr-number", type=int)
    parser.add_argument("--output")
    args = parser.parse_args()
    repo = os.environ.get("GITHUB_REPOSITORY", "KaisAbiyyi/mind-place")
    if args.command == "fingerprint":
        pr = args.pr_number or int(os.environ.get("PR_NUMBER") or "0")
        print(native_contract_fingerprint(".", repo, pr))
        return 0
    if not args.output:
        parser.error("attest requires --output")
    payload = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text(encoding="utf-8"))
    evidence = capture_native_evidence(
        ".", payload, repo=repo, run_id=int(os.environ["GITHUB_RUN_ID"]),
        attempt=int(os.environ["GITHUB_RUN_ATTEMPT"]),
        event_name=os.environ.get("GITHUB_EVENT_NAME", ""),
        github_sha=os.environ.get("GITHUB_SHA"),
    )
    output = Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(evidence, sort_keys=True, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({
        "native_contract_policy": evidence["native_contract_policy"],
        "native_contract_fingerprint": evidence["native_contract_fingerprint"],
        "workflow_run_id": evidence["workflow_run_id"],
        "workflow_run_attempt": evidence["workflow_run_attempt"],
        "pr_number": evidence["pr_number"],
        "base_sha": evidence["base_sha"], "head_sha": evidence["head_sha"],
    }, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
