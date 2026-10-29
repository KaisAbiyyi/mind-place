#!/usr/bin/env python3
"""Staging-only policy-repository payload validator; no GitHub credentials or execution.

Policy contracts are Git blobs stored as inert data. The active verifier takes
--policy-root policy/contracts and compares ORIGINAL TARGET paths byte for byte.
This validator MUST fail after deliberate Checkpoint 2B activation.
"""
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import sys

INVENTORY = "policy/contract-inventory.json"
HEX40 = re.compile(r"^[0-9a-f]{40}$")


def git_blob_id(content):
    return hashlib.sha1(b"blob " + str(len(content)).encode("ascii") + b"\0" + content).hexdigest()


def safe_file(root, relative):
    path = PurePosixPath(relative)
    if not relative or path.is_absolute() or ".." in path.parts or "." in path.parts or str(path) != relative:
        raise ValueError("ambiguous or traversing contract path: " + relative)
    entry = root.joinpath(*path.parts)
    if not entry.is_file() or entry.is_symlink():
        raise ValueError("missing or unsafe contract: " + relative)
    for directory in (root, *entry.parents):
        if directory == root.parent:
            break
        if directory.is_symlink():
            raise ValueError("symlink directory in contract path: " + relative)
    return entry


def validate_layout(root):
    root = Path(root).resolve(strict=True)
    inventory = json.loads(safe_file(root, INVENTORY).read_text())
    if inventory.get("schema") != "mp-authority-contract-inventory/v1" or inventory.get("staging_only") is not True or inventory.get("independent_approval") is not False:
        raise ValueError("invalid staging inventory")
    if inventory.get("contract_root") != "policy/contracts":
        raise ValueError("contract root cannot be redirected")
    expected = inventory.get("target_blobs")
    if not isinstance(expected, dict) or len(expected) != 6 or set(expected) != {
            ".github/workflows/ci.yml",
            ".github/workflows/windows-product.yml",
            ".github/workflows/mp9b-sqlcipher-qualification.yml",
            ".github/windows-policy/native-steps.json",
            "scripts/windows_product_gate.py",
            "scripts/windows_product_inputs.py"}:
        raise ValueError("unexpected contract manifest")
    contract_root = root / "policy/contracts"
    if not contract_root.is_dir() or contract_root.is_symlink():
        raise ValueError("missing contract directory")
    discovered = set()
    for file in contract_root.rglob("*"):
        if file.is_symlink():
            raise ValueError("symlink in contract inventory")
        if file.is_file():
            discovered.add(file.relative_to(contract_root).as_posix())
    if discovered != set(expected) or len({p.lower() for p in discovered}) != len(discovered):
        raise ValueError("missing, extra, or ambiguous contract inventory")
    for target_path, expected_blob in expected.items():
        if not isinstance(expected_blob, str) or not HEX40.fullmatch(expected_blob):
            raise ValueError("unreviewed target blob ID")
        actual = git_blob_id(safe_file(contract_root, target_path).read_bytes())
        if actual != expected_blob:
            raise ValueError("frozen target contract changed: " + target_path)
    template = inventory.get("controller_template")
    if not isinstance(template, dict) or template.get("path") != ".github/windows-policy/verifier.yml":
        raise ValueError("invalid controller template")
    if not HEX40.fullmatch(str(template.get("blob", ""))):
        raise ValueError("invalid template pin")
    if git_blob_id(safe_file(root, template["path"]).read_bytes()) != template["blob"]:
        raise ValueError("controller template differs from reviewed copy")
    for original in ("scripts/windows_product_gate.py", "scripts/windows_product_inputs.py"):
        if safe_file(root, original).read_bytes() != safe_file(contract_root, original).read_bytes():
            raise ValueError("active policy code differs from frozen source snapshot: " + original)
    registered = root / ".github/workflows"
    if registered.exists():
        if registered.is_symlink() or not registered.is_dir():
            raise ValueError("unsafe active workflow path")
        if any(registered.iterdir()):
            raise ValueError("STAGING ONLY: policy repo would activate .github/workflows")
    controller = safe_file(root, template["path"]).read_text()
    if "--policy-root policy/contracts" not in controller:
        raise ValueError("controller would use wrong immutable contract root")
    if "scripts/windows_product_gate.py controller" not in controller:
        raise ValueError("controller command mismatch")
    return sorted(discovered)


if __name__ == "__main__":
    print("INERT STAGING CONTRACTS VERIFIED:", ", ".join(validate_layout(Path(sys.argv[1] if len(sys.argv) > 1 else "."))))
