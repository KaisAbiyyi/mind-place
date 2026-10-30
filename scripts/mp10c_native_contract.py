#!/usr/bin/env python3
"""Deterministic, separately versioned native execution contract and run evidence."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
from pathlib import Path
import subprocess
import sys

POLICY = "mp-native-contract/v2"
EVIDENCE_SCHEMA = "mind-place-native-evidence/v1"
CI_WORKFLOW = ".github/workflows/ci.yml"
SQLCIPHER_WORKFLOW = ".github/workflows/mp9b-sqlcipher-qualification.yml"

# Explicit candidate policy: PR 127 is MP-9B vault; PR 129 is CI infrastructure.
CANDIDATE_REQUIREMENTS = {
    ("KaisAbiyyi/mind-place", 127): ("mp9b-vault", True),
    ("KaisAbiyyi/mind-place", 129): ("mp-ci-100k-infrastructure", False),
}
DEFAULT_REQUIREMENT = ("ordinary-non-vault-pr", False)


def _yaml_strip_comment(line: str) -> str:
    """Remove YAML comments without touching quoted text or block-scalar bodies."""
    quote = None
    escaped = False
    index = 0
    while index < len(line):
        char = line[index]
        if quote == "'":
            if char == "'" and index + 1 < len(line) and line[index + 1] == "'":
                index += 2
                continue
            if char == "'":
                quote = None
        elif quote == '"':
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == '"':
                quote = None
        else:
            if char in {"'", '"'} and (
                    index == 0 or line[index - 1].isspace() or line[index - 1] in ":[,{"):
                quote = char
            elif char == "#" and (index == 0 or line[index - 1].isspace()):
                return line[:index]
        index += 1
    return line


def _yaml_pair(text: str) -> tuple[str, str] | None:
    """Split one YAML mapping entry at its structural colon."""
    quote = None
    escaped = False
    depth = 0
    index = 0
    while index < len(text):
        char = text[index]
        if quote == "'":
            if char == "'" and index + 1 < len(text) and text[index + 1] == "'":
                index += 2
                continue
            if char == "'":
                quote = None
        elif quote == '"':
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == '"':
                quote = None
        else:
            if char in {"'", '"'} and (
                    index == 0 or text[index - 1].isspace() or text[index - 1] in ":[,{"):
                quote = char
            elif char in "[{(":
                depth += 1
            elif char in "]})":
                depth -= 1
            elif char == ":" and depth == 0 and (
                    index + 1 == len(text) or text[index + 1].isspace()):
                key = _yaml_scalar(text[:index].strip())
                if not isinstance(key, str) or not key:
                    raise ValueError("Invalid YAML mapping key")
                return key, text[index + 1:].strip()
        index += 1
    return None


def _yaml_split_inline(text: str) -> list[str]:
    parts = []
    start = 0
    quote = None
    escaped = False
    depth = 0
    index = 0
    while index < len(text):
        char = text[index]
        if quote == "'":
            if char == "'" and index + 1 < len(text) and text[index + 1] == "'":
                index += 2
                continue
            if char == "'":
                quote = None
        elif quote == '"':
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == '"':
                quote = None
        else:
            if char in {"'", '"'} and (
                    index == 0 or text[index - 1].isspace() or text[index - 1] in ":[,{"):
                quote = char
            elif char in "[{(":
                depth += 1
            elif char in "]})":
                depth -= 1
            elif char == "," and depth == 0:
                parts.append(text[start:index].strip())
                start = index + 1
        index += 1
    parts.append(text[start:].strip())
    return [part for part in parts if part]


def _yaml_scalar(value: str):
    value = value.strip()
    if not value:
        return ""
    if value.startswith("[") and value.endswith("]"):
        inner = value[1:-1].strip()
        return [] if not inner else [_yaml_scalar(item) for item in _yaml_split_inline(inner)]
    if value.startswith("{") and value.endswith("}"):
        inner = value[1:-1].strip()
        result = {}
        if not inner:
            return result
        for item in _yaml_split_inline(inner):
            pair = _yaml_pair(item)
            if pair is None or pair[0] in result:
                raise ValueError("Invalid or duplicate inline YAML mapping key")
            result[pair[0]] = _yaml_scalar(pair[1])
        return result
    if len(value) >= 2 and value[0] == "'" and value[-1] == "'":
        return value[1:-1].replace("''", "'")
    if len(value) >= 2 and value[0] == '"' and value[-1] == '"':
        try:
            return json.loads(value)
        except json.JSONDecodeError:
            return value[1:-1]
    lowered = value.lower()
    if lowered == "true":
        return True
    if lowered == "false":
        return False
    if lowered in {"null", "~"}:
        return None
    if re.fullmatch(r"-?(?:0|[1-9][0-9]*)", value):
        try:
            return int(value)
        except ValueError:
            pass
    if re.fullmatch(r"-?(?:[0-9]+\.[0-9]*|[0-9]*\.[0-9]+)", value):
        try:
            return float(value)
        except ValueError:
            pass
    return value


def _yaml_tokens(source: str):
    raw_lines = source.splitlines()
    tokens = []
    for number, raw in enumerate(raw_lines):
        if "\t" in raw[:len(raw) - len(raw.lstrip(" \t"))]:
            raise ValueError("Tabs are not valid YAML indentation")
        clean = _yaml_strip_comment(raw).rstrip()
        if not clean.strip():
            continue
        indent = len(clean) - len(clean.lstrip(" "))
        tokens.append({"line": number, "indent": indent, "text": clean[indent:]})
    return raw_lines, tokens


def _yaml_block_scalar(raw_lines: list[str], header_line: int,
                       parent_indent: int, marker: str) -> tuple[str, int]:
    cursor = header_line + 1
    while cursor < len(raw_lines):
        raw = raw_lines[cursor]
        if not raw.strip():
            cursor += 1
            continue
        indent = len(raw) - len(raw.lstrip(" "))
        if indent <= parent_indent:
            break
        cursor += 1
    body = raw_lines[header_line + 1:cursor]
    nonempty_indents = [
        len(line) - len(line.lstrip(" ")) for line in body if line.strip()
    ]
    content_indent = min(nonempty_indents) if nonempty_indents else parent_indent + 1
    content = [line[content_indent:] if line.strip() else "" for line in body]
    value = "\n".join(content)
    if marker.startswith(">"):
        value = re.sub(r"(?<!\n)\n(?!\n)", " ", value)
    if marker.endswith("-"):
        value = value.rstrip("\n")
    elif marker.endswith("+"):
        if body and not value.endswith("\n"):
            value += "\n"
    elif value:
        value = value.rstrip("\n") + "\n"
    return value, cursor


def _yaml_parse_entry(value_text: str, pos: int, key_indent: int,
                      line_number: int, raw_lines: list[str], tokens: list[dict]):
    if value_text in {"|", "|-", "|+", ">", ">-", ">+"}:
        value, end_line = _yaml_block_scalar(
            raw_lines, line_number, key_indent, value_text)
        while pos < len(tokens) and tokens[pos]["line"] < end_line:
            pos += 1
        return value, pos
    if value_text == "":
        if pos < len(tokens) and tokens[pos]["indent"] > key_indent:
            return _yaml_parse_block(raw_lines, tokens, pos, tokens[pos]["indent"])
        return None, pos
    return _yaml_scalar(value_text), pos


def _yaml_parse_mapping(raw_lines: list[str], tokens: list[dict], pos: int,
                        indent: int, initial=None):
    result = {}
    if initial is not None:
        key, value_text, line_number = initial
        if key in result:
            raise ValueError("Duplicate YAML mapping key: " + key)
        result[key], pos = _yaml_parse_entry(
            value_text, pos, indent, line_number, raw_lines, tokens)
    while pos < len(tokens) and tokens[pos]["indent"] == indent:
        token = tokens[pos]
        if token["text"] == "-" or token["text"].startswith("- "):
            break
        pair = _yaml_pair(token["text"])
        if pair is None:
            raise ValueError("Unsupported YAML mapping syntax")
        key, value_text = pair
        if key in result:
            raise ValueError("Duplicate YAML mapping key: " + key)
        pos += 1
        result[key], pos = _yaml_parse_entry(
            value_text, pos, indent, token["line"], raw_lines, tokens)
    return result, pos


def _yaml_parse_sequence(raw_lines: list[str], tokens: list[dict],
                         pos: int, indent: int):
    result = []
    while pos < len(tokens) and tokens[pos]["indent"] == indent and (
            tokens[pos]["text"] == "-" or tokens[pos]["text"].startswith("- ")):
        token = tokens[pos]
        item_text = token["text"][1:].strip()
        pos += 1
        if not item_text:
            if pos < len(tokens) and tokens[pos]["indent"] > indent:
                item, pos = _yaml_parse_block(
                    raw_lines, tokens, pos, tokens[pos]["indent"])
            else:
                item = None
        else:
            pair = _yaml_pair(item_text)
            if pair is not None:
                item, pos = _yaml_parse_mapping(
                    raw_lines, tokens, pos, indent + 2,
                    initial=(pair[0], pair[1], token["line"]))
            else:
                item = _yaml_scalar(item_text)
                if pos < len(tokens) and tokens[pos]["indent"] > indent:
                    raise ValueError("Unexpected nested YAML content after scalar sequence item")
        result.append(item)
    return result, pos


def _yaml_parse_block(raw_lines: list[str], tokens: list[dict],
                      pos: int, indent: int):
    if pos >= len(tokens) or tokens[pos]["indent"] != indent:
        raise ValueError("Invalid YAML indentation")
    text = tokens[pos]["text"]
    if text == "-" or text.startswith("- "):
        return _yaml_parse_sequence(raw_lines, tokens, pos, indent)
    return _yaml_parse_mapping(raw_lines, tokens, pos, indent)


def parse_workflow_yaml(contents: bytes | str) -> dict:
    """Parse GitHub Actions workflow YAML without adding a runtime dependency.

    Mapping/list structure, quoted and inline scalars, comments, and literal/folded
    block scalars are supported. Ambiguous indentation and duplicate keys fail closed.
    """
    source = contents.decode("utf-8") if isinstance(contents, bytes) else contents
    if not isinstance(source, str):
        raise ValueError("Workflow source must be UTF-8 text")
    raw_lines, tokens = _yaml_tokens(source)
    if not tokens:
        raise ValueError("Empty workflow YAML")
    if tokens[0]["indent"] != 0:
        raise ValueError("Workflow YAML must start at indentation zero")
    document, pos = _yaml_parse_block(raw_lines, tokens, 0, 0)
    if pos != len(tokens) or not isinstance(document, dict):
        raise ValueError("Incomplete or invalid workflow YAML document")
    return document


def canonical_workflow_signature(contents: bytes | str) -> bytes:
    """Return a formatting/comment-independent native execution contract."""
    document = parse_workflow_yaml(contents)
    jobs = document.get("jobs")
    if not isinstance(jobs, dict) or not jobs:
        raise ValueError("Native workflow has no jobs mapping")
    for job_id, job in jobs.items():
        if not isinstance(job, dict) or not ("runs-on" in job or "uses" in job):
            raise ValueError("Invalid native workflow job: " + str(job_id))
        if "steps" in job and not isinstance(job["steps"], list):
            raise ValueError("Invalid native workflow steps: " + str(job_id))
    selected = {
        key: document[key]
        for key in ("on", "permissions", "defaults", "env", "concurrency")
        if key in document
    }
    payload = {
        "schema": "mp-native-workflow-execution/v1",
        "workflow": selected,
        "jobs": jobs,
    }
    return json.dumps(
        payload, sort_keys=True, separators=(",", ":"), ensure_ascii=False
    ).encode("utf-8")


def _runner_platform(label: str) -> str | None:
    value = label.strip().lower()
    if value == "windows" or value.startswith("windows-"):
        return "windows"
    if value in {"linux", "ubuntu"} or value.startswith("ubuntu-") or value.startswith("linux-"):
        return "linux"
    if value == "macos" or value.startswith("macos-"):
        return "macos"
    return None


def workflow_job_runner_requirements(contents: bytes | str, job_name: str) -> dict:
    """Derive configured runner labels/platform from a named workflow job."""
    document = parse_workflow_yaml(contents)
    jobs = document.get("jobs")
    if not isinstance(jobs, dict):
        raise ValueError("Workflow has no jobs mapping")
    matches = [
        (job_id, job) for job_id, job in jobs.items()
        if isinstance(job, dict) and job.get("name", job_id) == job_name
    ]
    if len(matches) != 1:
        raise ValueError("Workflow job name is missing or ambiguous: " + job_name)
    job_id, config = matches[0]
    runs_on = config.get("runs-on")
    group = None
    if isinstance(runs_on, str):
        labels = [runs_on]
    elif isinstance(runs_on, list):
        labels = runs_on
    elif isinstance(runs_on, dict):
        group = runs_on.get("group")
        labels = runs_on.get("labels")
        if isinstance(labels, str):
            labels = [labels]
    else:
        raise ValueError("Unverifiable runs-on contract for " + job_name)
    if (not isinstance(labels, list) or not labels
            or any(not isinstance(label, str) or not label.strip() for label in labels)):
        raise ValueError("Malformed runs-on labels for " + job_name)
    normalized = [label.strip().lower() for label in labels]
    if len(set(normalized)) != len(normalized):
        raise ValueError("Duplicate configured runner label for " + job_name)
    platforms = {_runner_platform(label) for label in normalized}
    platforms.discard(None)
    if len(platforms) != 1:
        raise ValueError("Workflow runner platform is missing or ambiguous for " + job_name)
    if group is not None and (not isinstance(group, str) or not group.strip()):
        raise ValueError("Malformed runner group for " + job_name)
    return {
        "job_id": job_id, "labels": normalized,
        "platform": next(iter(platforms)), "group": group,
    }


def validate_runner_labels(job: dict, expected: dict) -> None:
    """Fail closed unless GitHub's actual job labels match workflow requirements."""
    labels = job.get("labels")
    if (not isinstance(labels, list) or not labels
            or any(not isinstance(label, str) or not label.strip() for label in labels)):
        raise ValueError("Missing or malformed GitHub runner labels")
    normalized = [label.strip().lower() for label in labels]
    if len(set(normalized)) != len(normalized):
        raise ValueError("Duplicate GitHub runner labels")
    if not set(expected["labels"]).issubset(set(normalized)):
        raise ValueError("Actual runner labels do not satisfy workflow runs-on requirements")
    platforms = {_runner_platform(label) for label in normalized}
    platforms.discard(None)
    if len(platforms) != 1 or expected["platform"] not in platforms:
        raise ValueError("Actual runner platform conflicts with workflow runs-on requirements")
    if expected.get("group") is not None and job.get("runner_group_name") != expected["group"]:
        raise ValueError("Actual runner group does not match workflow runs-on requirements")


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
        if path in {CI_WORKFLOW, SQLCIPHER_WORKFLOW}:
            content = canonical_workflow_signature(content)
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
