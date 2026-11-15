"""Same policy and advisory DB for base/head; neither baseline failures nor errors are waived."""
import json
import os
from pathlib import Path
import re
import subprocess
import sys


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
    return {"status": "PASS" if result.returncode == 0 else "FAIL", "exitCode": result.returncode,
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
    receipt = {"schema": 1, "headSha": os.environ.get("SOURCE_HEAD"), "baseSha": os.environ.get("SOURCE_BASE"),
               "advisoryDatabases": databases, "head": head_report, "base": base_report,
               "introducedAdvisoryIds": sorted(set(head_report["advisories"]) - set(base_report["advisories"])),
               "existingAdvisoryIds": sorted(set(head_report["advisories"]) & set(base_report["advisories"])),
               "classificationLimit": "Compare full diagnostics for license/source errors and affected crate/version changes; IDs alone are not vulnerability identity."}
    (destination / "comparison.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps(receipt, indent=2))
    return head_report["exitCode"]


if __name__ == "__main__":
    sys.exit(main())
