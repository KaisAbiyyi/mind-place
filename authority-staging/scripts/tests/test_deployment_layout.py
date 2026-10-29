"""Adversarial Checkpoint 2A deployment tests; no network or trusted checks."""
import json
from pathlib import Path
import shutil
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
from staging_layout import git_blob_id, safe_file, validate_layout


class DeployLayoutTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.deploy = Path(self.tmp.name) / "policy-repository-root"
        shutil.copytree(ROOT, self.deploy)

    def edit(self, path, text):
        file = self.deploy / path
        file.parent.mkdir(parents=True, exist_ok=True)
        file.write_text(text)

    def test_deployment_payload_is_inert_and_complete(self):
        self.assertEqual(len(validate_layout(self.deploy)), 6)
        self.assertFalse((self.deploy / ".github/workflows").exists())

    def test_product_workflows_are_data_not_registered_actions(self):
        paths = list((self.deploy / "policy/contracts/.github/workflows").glob("*.yml"))
        self.assertEqual(sorted(p.name for p in paths),
                         ["ci.yml", "mp9b-sqlcipher-qualification.yml", "windows-product.yml"])
        self.assertFalse(list((self.deploy / ".github/workflows").glob("*.yml")))

    def test_copy_to_independent_root_cannot_register_product_workflows(self):
        self.assertEqual(len(validate_layout(self.deploy)), 6)
        active = self.deploy / ".github/workflows"
        active.mkdir()
        (active / "windows-product.yml").write_text("name: ACCIDENTALLY ACTIVE\n")
        with self.assertRaisesRegex(ValueError, "activate"):
            validate_layout(self.deploy)

    def test_modified_native_command_fails_closed(self):
        path = "policy/contracts/.github/workflows/windows-product.yml"
        content = (self.deploy / path).read_text()
        self.edit(path, content.replace("npm ci", "npm install", 1))
        with self.assertRaisesRegex(ValueError, "contract changed"):
            validate_layout(self.deploy)

    def test_modified_ipc_requirement_fails_closed(self):
        path = "policy/contracts/.github/windows-policy/native-steps.json"
        content = (self.deploy / path).read_text()
        self.edit(path, content.replace("Qualify native capture encrypted IPC and revoked owner epoch", "skip IPC"))
        with self.assertRaisesRegex(ValueError, "contract changed"):
            validate_layout(self.deploy)

    def test_missing_contract_fails_closed(self):
        (self.deploy / "policy/contracts/scripts/windows_product_gate.py").unlink()
        with self.assertRaisesRegex(ValueError, "missing"):
            validate_layout(self.deploy)

    def test_extra_or_ambiguous_contract_fails_closed(self):
        self.edit("policy/contracts/.github/workflows/WINDOWS-PRODUCT.yml", "name: bad")
        with self.assertRaisesRegex(ValueError, "extra, or ambiguous"):
            validate_layout(self.deploy)

    def test_contract_traversal_fails_closed(self):
        root = self.deploy / "policy/contracts"
        for path in ("../stolen", "/tmp/stolen", "scripts/../windows_product_gate.py",
                     "scripts//windows_product_gate.py"):
            with self.subTest(path=path), self.assertRaises(ValueError):
                safe_file(root, path)

    def test_contract_symlink_fails_closed(self):
        path = self.deploy / "policy/contracts/.github/workflows/ci.yml"
        path.unlink()
        try:
            path.symlink_to(self.deploy / "policy/contracts/.github/workflows/windows-product.yml")
        except (OSError, NotImplementedError):
            self.skipTest("symlinks unavailable")
        with self.assertRaisesRegex(ValueError, "symlink"):
            validate_layout(self.deploy)

    def test_controller_promotion_requires_explicit_checkpoint_2b(self):
        template = (self.deploy / ".github/windows-policy/verifier.yml").read_text()
        self.assertIn("--policy-root policy/contracts", template)
        self.edit(".github/workflows/verify-windows.yml", template)
        with self.assertRaisesRegex(ValueError, "activate"):
            validate_layout(self.deploy)

    def test_wrong_active_verifier_or_template_rejected(self):
        self.edit("scripts/windows_product_gate.py", "print('unreviewed controller')")
        with self.assertRaisesRegex(ValueError, "active policy code"):
            validate_layout(self.deploy)

    def test_byte_exact_git_blob_instead_of_unreviewed_text(self):
        manifest = json.loads((self.deploy / "policy/contract-inventory.json").read_text())
        for name, oid in manifest["target_blobs"].items():
            self.assertEqual(git_blob_id((self.deploy / "policy/contracts" / name).read_bytes()), oid)

    def test_runbook_matches_real_layout_and_binds_activation_boundary(self):
        runbook = (self.deploy / "ACTIVATION.md").read_text()
        for phrase in ("policy/contracts", ".github/windows-policy/verifier.yml",
                       ".github/workflows/verify-windows.yml", "P0 OPEN", "DO NOT MERGE",
                       "merge-freeze", "app_id"):
            self.assertIn(phrase, runbook)


if __name__ == "__main__":
    unittest.main()
