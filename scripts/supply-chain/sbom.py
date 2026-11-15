"""Generate and validate release-source CycloneDX documents; never publish a release."""
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys


def validate_bom(bom):
    if (bom.get("bomFormat") != "CycloneDX" or bom.get("specVersion") not in ["1.5", "1.6", "1.7"]
            or not isinstance(bom.get("version"), int) or bom["version"] < 1
            or not bom.get("metadata", {}).get("component", {}).get("name")
            or not isinstance(bom.get("components"), list) or not bom["components"]):
        raise ValueError("Missing/empty/invalid CycloneDX inventory")
    if any(not c.get("name") or not c.get("version") for c in bom["components"]):
        raise ValueError("Unversioned SBOM component")
    return len(bom["components"])


def main():
    root = Path.cwd()
    output = Path(sys.argv[1]).resolve()
    output.mkdir(parents=True, exist_ok=True)
    original_locks = {name: (root / name).read_bytes() for name in ["Cargo.lock", "package-lock.json"]}
    metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"]))
    subprocess.run(["cargo", "cyclonedx", "--format", "json", "--spec-version", "1.5", "--target", "x86_64-pc-windows-msvc"], check=True, timeout=600)
    files = []
    for package in metadata["packages"]:
        source = Path(package["manifest_path"]).with_name("bom.json")
        if not source.is_file():
            raise ValueError(f"Missing Rust SBOM for {package['name']}")
        validate_bom(json.loads(source.read_text()))
        target = output / f"cargo-{package['name']}.cdx.json"
        shutil.copyfile(source, target)
        files.append(target)
    # Built-in npm SBOM avoids installing a separate dependency generator.
    npm = subprocess.run(["npm", "sbom", "--sbom-format", "cyclonedx", "--package-lock-only", "--ignore-scripts", "--include=dev", "--include=optional", "--include=peer"], capture_output=True, text=True, check=True, timeout=180)
    validate_bom(json.loads(npm.stdout))
    npm_file = output / "npm.cdx.json"
    npm_file.write_text(npm.stdout)
    files.append(npm_file)
    pin = json.loads((root / "scripts/tunnel-client.lock.json").read_text())
    helper = {"bomFormat": "CycloneDX", "specVersion": "1.5", "version": 1,
              "metadata": {"component": {"type": "application", "name": "mind-place-bundled-helper", "version": metadata["packages"][0]["version"]}},
              "components": [{"type": "application", "name": "openai/tunnel-client", "version": pin["version"],
                              "hashes": [{"alg": "SHA-256", "content": pin["executable"]["sha256"]}],
                              "licenses": [{"license": {"id": "Apache-2.0"}}],
                              "externalReferences": [{"type": "distribution", "url": pin["repository"] + "/releases/tag/v" + pin["version"]}],
                              "properties": [{"name": "mind-place:upstream-spdx-resource", "value": json.dumps({k: v for k, v in pin["resources"].items() if k.endswith(".spdx.json")}, sort_keys=True)},
                                             {"name": "mind-place:excluded-archive-members", "value": json.dumps(pin["excluded_archive_members"]) }]}]}
    helper_file = output / "bundled-helper.cdx.json"
    helper_file.write_text(json.dumps(helper, indent=2) + "\n")
    files.append(helper_file)
    for name, before in original_locks.items():
        if (root / name).read_bytes() != before:
            raise ValueError(f"SBOM generator modified {name}")
    manifest = {"schema": 1, "sourceSha": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(),
                "ref": subprocess.check_output(["git", "describe", "--always", "--tags"], text=True).strip(),
                "target": "x86_64-pc-windows-msvc", "rustFeatures": "workspace defaults, matching release manifests",
                "scope": "Source dependency inventories, including npm development/build tooling. Helper is a pinned binary component; its transitive dependencies remain in the upstream SPDX resource already shipped by existing packaging. OS/WebView2 and final installer bytes are not inventoried/attested here.",
                "lockfiles": {name: hashlib.sha256(data).hexdigest() for name, data in original_locks.items()},
                "tools": json.loads((root / "scripts/supply-chain/tools.json").read_text()),
                "documents": [{"file": file.name, "sha256": hashlib.sha256(file.read_bytes()).hexdigest(), "components": validate_bom(json.loads(file.read_text()))} for file in files]}
    (output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(json.dumps(manifest, indent=2))


if __name__ == "__main__":
    main()
