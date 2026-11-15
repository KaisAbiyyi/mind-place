"""Same policy and advisory DB for base/head; neither baseline failures nor errors are waived."""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys


def diagnostic_findings(raw, exit_code):
    """Preserve policy failures; distinguish a tool failure from a clean audit."""
    findings = []
    for line in raw.splitlines():
        try:
            entry = json.loads(line)
        except json.JSONDecodeError:
            continue  # Cargo/tool stderr is retained verbatim, never treated as a finding waiver.
        if not isinstance(entry, dict) or entry.get("type") != "diagnostic":
            continue
        fields = entry.get("fields", {})
        if fields.get("severity") != "error":
            continue
        code = fields.get("code")
        graphs = fields.get("graphs")
        if not isinstance(code, str) or not isinstance(graphs, list):
            raise ValueError("RUST_AUDIT_BLOCKED: malformed error diagnostic")
        packages = []
        for graph in graphs:
            crate = graph.get("Krate", {})
            if not isinstance(crate.get("name"), str) or not isinstance(crate.get("version"), str):
                raise ValueError("RUST_AUDIT_BLOCKED: missing affected package identity")
            packages.append({"name": crate["name"], "version": crate["version"]})
        finding = {"code": code, "advisoryId": fields.get("advisory", {}).get("id"),
                   "packages": sorted(packages, key=lambda p: (p["name"], p["version"])),
                   "terms": sorted({label.get("span", "") for label in fields.get("labels", [])
                                    if label.get("message", "").startswith("rejected:")}),
                   "message": fields.get("message"),
                   "inclusionGraphSha256": hashlib.sha256(json.dumps(graphs, sort_keys=True).encode()).hexdigest()}
        findings.append(finding)
    if exit_code == 0 and findings:
        raise ValueError("RUST_AUDIT_BLOCKED: successful exit contradicts error diagnostics")
    status = "FAIL" if findings else ("PASS" if exit_code == 0 else "BLOCKED")
    return status, sorted(findings, key=lambda f: json.dumps(f, sort_keys=True))


def compare_findings(head, base):
    # Package version, license/source code and inclusion graph are part of identity;
    # the same RUSTSEC ID must not hide a new affected version or dependency path.
    identity = lambda finding: json.dumps(finding, sort_keys=True)
    previous = {identity(f) for f in base}
    current = {identity(f) for f in head}
    return {"existing": [f for f in head if identity(f) in previous],
            "introducedOrChanged": [f for f in head if identity(f) not in previous],
            "resolved": [f for f in base if identity(f) not in current]}


def target_presence(metadata, findings):
    packages = {p["id"]: p for p in metadata["packages"]}
    nodes = {n["id"]: n for n in metadata["resolve"]["nodes"]}
    previous = {package: None for package in metadata["workspace_members"]}
    queue = list(previous)
    for node in queue:
        for dependency in nodes.get(node, {}).get("deps", []):
            package = dependency["pkg"]
            if package not in previous:
                previous[package] = node
                queue.append(package)
    observed = []
    wanted = {(p["name"], p["version"]) for f in findings for p in f["packages"]}
    for name, version in sorted(wanted):
        matches = [p for p in packages.values() if p["name"] == name and p["version"] == version and p["id"] in previous]
        path = []
        current = matches[0]["id"] if matches else None
        while current is not None:
            package = packages[current]
            path.append({"name": package["name"], "version": package["version"]})
            current = previous[current]
        observed.append({"name": name, "version": version, "resolvedForTarget": bool(matches), "examplePath": list(reversed(path))})
    return {"target": "x86_64-pc-windows-msvc", "features": "all workspace features; metadata resolution only",
            "notAnExploitabilityOrNativeCertificate": True, "packages": observed}


def run(root, output):
    command = ["cargo", "deny", "--locked", "--format", "json", "check", "--disable-fetch", "advisories", "licenses", "sources"]
    result = subprocess.run(command, cwd=root, capture_output=True, text=True, timeout=600)
    output.write_text(result.stdout + result.stderr)
    print(f"=== {output.name}: cargo-deny exit {result.returncode} ===", flush=True)
    for line in (result.stdout + result.stderr).splitlines():
        try:
            diagnostic = json.loads(line)
            if diagnostic.get("level") == "error" or diagnostic.get("type") == "diagnostic":
                print(json.dumps(diagnostic), flush=True)
        except json.JSONDecodeError:
            if "error" in line.lower():
                print(line, flush=True)
    status, findings = diagnostic_findings(result.stdout + result.stderr, result.returncode)
    return {"status": status, "exitCode": result.returncode, "findings": findings,
            "rawSha256": hashlib.sha256(output.read_bytes()).hexdigest(),
            "advisories": sorted(set(re.findall(r"RUSTSEC-\d{4}-\d{4}", result.stdout + result.stderr)))}


def main():
    head, base, destination = map(Path, sys.argv[1:])
    destination.mkdir(parents=True, exist_ok=True)
    # Fetch once, then forbid refetch during comparison. Failure cannot become a PASS.
    subprocess.run(["cargo", "deny", "fetch"], cwd=head, check=True, timeout=300)
    cargo_home = Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo"))
    databases = {}
    for repo in (cargo_home / "advisory-dbs").glob("*"):
        if (repo / ".git").exists():
            databases[repo.name] = subprocess.check_output(["git", "-C", str(repo), "rev-parse", "HEAD"], text=True).strip()
    if not databases:
        raise RuntimeError("No advisory database commit evidence")
    base_report = run(base, destination / "base.jsonl")
    head_report = run(head, destination / "head.jsonl")
    metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--locked", "--all-features", "--format-version", "1", "--filter-platform", "x86_64-pc-windows-msvc"], cwd=head, timeout=300))
    receipt = {"schema": 2, "headSha": os.environ.get("SOURCE_HEAD"), "baseSha": os.environ.get("SOURCE_BASE"),
               "advisoryDatabases": databases, "head": head_report, "base": base_report,
               "comparison": compare_findings(head_report["findings"], base_report["findings"]),
               "targetPresence": target_presence(metadata, head_report["findings"]),
               "introducedAdvisoryIds": sorted(set(head_report["advisories"]) - set(base_report["advisories"])),
               "existingAdvisoryIds": sorted(set(head_report["advisories"]) & set(base_report["advisories"])),
               "classificationLimit": "Unchanged tool diagnostics and target resolution are evidence for triage, not an exception or proof of unreachability. Review source/runtime behavior independently."}
    (destination / "comparison.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps(receipt, indent=2))
    return head_report["exitCode"] or (2 if head_report["status"] == "BLOCKED" or base_report["status"] == "BLOCKED" else 0)


if __name__ == "__main__":
    sys.exit(main())
