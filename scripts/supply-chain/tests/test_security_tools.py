import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tarfile
import tomllib
import unittest

ROOT = Path(__file__).resolve().parents[3]
TOOLS = ROOT / "scripts/supply-chain"


def module(name, filename):
    spec = importlib.util.spec_from_file_location(name, TOOLS / filename)
    loaded = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(loaded)
    return loaded


installer = module("installer", "install-tool.py")
sbom = module("sbom", "sbom.py")
rust_audit = module("rust_audit", "rust-audit.py")


class RustTriageTests(unittest.TestCase):
    def diagnostic(self, version="1.0.0", code="vulnerability", term=""):
        return {"type": "diagnostic", "fields": {"severity": "error", "code": code, "message": "test finding",
                "advisory": {"id": "RUSTSEC-2099-0001"}, "graphs": [{"Krate": {"name": "example", "version": version}}],
                "labels": [{"message": "rejected: license is not explicitly allowed", "span": term}] if term else []}}

    def parsed(self, diagnostic):
        return rust_audit.diagnostic_findings(json.dumps(diagnostic), 5)[1]

    def test_same_id_with_new_package_version_is_not_existing(self):
        base = self.parsed(self.diagnostic())
        head = self.parsed(self.diagnostic(version="1.0.1"))
        result = rust_audit.compare_findings(head, base)
        self.assertEqual(len(result["introducedOrChanged"]), 1)
        self.assertEqual(result["existing"], [])

    def test_license_term_and_dependency_graph_changes_are_visible(self):
        base = self.parsed(self.diagnostic(code="rejected", term="MPL-2.0"))
        changed = self.diagnostic(code="rejected", term="WTFPL")
        self.assertEqual(len(rust_audit.compare_findings(self.parsed(changed), base)["introducedOrChanged"]), 1)
        changed = self.diagnostic(code="rejected", term="MPL-2.0")
        changed["fields"]["graphs"][0]["parents"] = [{"Krate": {"name": "new-consumer", "version": "2.0.0"}}]
        self.assertEqual(len(rust_audit.compare_findings(self.parsed(changed), base)["introducedOrChanged"]), 1)

    def test_errors_cannot_become_clean_audits(self):
        self.assertEqual(rust_audit.diagnostic_findings("registry unavailable", 2), ("BLOCKED", []))
        with self.assertRaisesRegex(ValueError, "contradicts"):
            rust_audit.diagnostic_findings(json.dumps(self.diagnostic()), 0)
        malformed = self.diagnostic()
        malformed["fields"]["graphs"] = [{"Krate": {"name": "missing-version"}}]
        with self.assertRaisesRegex(ValueError, "identity"):
            self.parsed(malformed)

    def test_existing_failures_and_target_absence_never_waive_gate(self):
        raw = json.dumps(self.diagnostic())
        status, findings = rust_audit.diagnostic_findings(raw, 5)
        self.assertEqual(status, "FAIL")
        self.assertEqual(len(rust_audit.compare_findings(findings, findings)["existing"]), 1)
        metadata = {"packages": [{"id": "root", "name": "app", "version": "1"}, {"id": "dep", "name": "example", "version": "1.0.0"}],
                    "workspace_members": ["root"], "resolve": {"nodes": [{"id": "root", "deps": []}, {"id": "dep", "deps": []}]}}
        presence = rust_audit.target_presence(metadata, findings)
        self.assertFalse(presence["packages"][0]["resolvedForTarget"])
        self.assertTrue(presence["notAnExploitabilityOrNativeCertificate"])
        metadata["resolve"]["nodes"][0]["deps"] = [{"pkg": "dep"}]
        self.assertTrue(rust_audit.target_presence(metadata, findings)["packages"][0]["resolvedForTarget"])


class ToolIntegrityTests(unittest.TestCase):
    def archive(self, name="bin/cargo-deny", kind=tarfile.REGTYPE, duplicate=False):
        output = io.BytesIO()
        with tarfile.open(fileobj=output, mode="w:gz") as archive:
            for _ in range(2 if duplicate else 1):
                entry = tarfile.TarInfo(name)
                entry.type = kind
                entry.linkname = "elsewhere"
                entry.size = 4 if kind == tarfile.REGTYPE else 0
                archive.addfile(entry, io.BytesIO(b"test") if entry.size else None)
        payload = output.getvalue()
        return payload, {"binary": "cargo-deny", "sha256": hashlib.sha256(payload).hexdigest()}

    def test_valid_binary(self):
        payload, pin = self.archive()
        self.assertEqual(installer.verified_binary(payload, pin), b"test")

    def test_checksum_tampering(self):
        payload, pin = self.archive()
        with self.assertRaisesRegex(ValueError, "checksum"):
            installer.verified_binary(payload + b"modified", pin)

    def test_unsafe_archive_members(self):
        for name, kind, duplicate in [("../cargo-deny", tarfile.REGTYPE, False), ("/cargo-deny", tarfile.REGTYPE, False),
                                      ("cargo-deny", tarfile.SYMTYPE, False), ("cargo-deny", tarfile.LNKTYPE, False),
                                      ("cargo-deny", tarfile.REGTYPE, True), ("other", tarfile.REGTYPE, False)]:
            with self.subTest(name=name, kind=kind, duplicate=duplicate):
                payload, pin = self.archive(name, kind, duplicate)
                with self.assertRaises(ValueError):
                    installer.verified_binary(payload, pin)

    def test_nonempty_cyclonedx_required(self):
        valid = {"bomFormat": "CycloneDX", "specVersion": "1.5", "version": 1,
                 "metadata": {"component": {"name": "root"}}, "components": [{"name": "dependency", "version": "1.0.0"}]}
        self.assertEqual(sbom.validate_bom(valid), 1)
        for change in [{"components": []}, {"bomFormat": "SPDX"}, {"components": [{"name": "unversioned"}]}, {"metadata": {}}]:
            with self.assertRaises(ValueError):
                sbom.validate_bom({**valid, **change})

    def test_policy_has_only_owner_approved_exceptions(self):
        policy = tomllib.loads((ROOT / "deny.toml").read_text())
        ignores = policy["advisories"]["ignore"]
        self.assertEqual({entry["id"] for entry in ignores}, {"RUSTSEC-2024-0429", "RUSTSEC-2024-0370"})
        self.assertTrue(all(isinstance(entry.get("reason"), str) and len(entry["reason"].strip()) >= 20 for entry in ignores))
        self.assertEqual(policy["advisories"]["unmaintained"], "all")
        self.assertEqual(policy["advisories"]["yanked"], "deny")
        self.assertEqual(policy["licenses"]["exceptions"], [{"allow": ["WTFPL"], "name": "sqlcipher-src"}])
        self.assertFalse(policy["licenses"]["private"]["ignore"])
        self.assertTrue(policy["graph"]["all-features"])
        self.assertNotIn("exclude", policy["graph"])
        self.assertEqual(policy["sources"]["unknown-registry"], "deny")
        self.assertEqual(policy["sources"]["unknown-git"], "deny")
        self.assertEqual(policy["sources"]["allow-git"], [])
        self.assertEqual(policy["sources"]["allow-registry"], ["https://github.com/rust-lang/crates.io-index"])

    def test_toolchain_matches_existing_ci_and_msrv(self):
        pin = tomllib.loads((ROOT / "rust-toolchain.toml").read_text())["toolchain"]
        self.assertEqual(pin["channel"], "1.90.0")
        self.assertEqual(pin["profile"], "minimal")
        self.assertEqual(pin["components"], ["rustfmt"])
        self.assertIn("rustup toolchain install 1.90.0", (ROOT / ".github/workflows/ci.yml").read_text())

    def test_upstream_tool_pins_are_immutable_digests(self):
        for tool, pin in json.loads((TOOLS / "tools.json").read_text()).items():
            with self.subTest(tool=tool):
                self.assertRegex(pin["sha256"], r"^[0-9a-f]{64}$")
                self.assertTrue(pin["url"].startswith("https://github.com/"))
                self.assertIn(pin["version"], pin["url"])
                self.assertNotIn("/latest/", pin["url"])


if __name__ == "__main__":
    unittest.main()
