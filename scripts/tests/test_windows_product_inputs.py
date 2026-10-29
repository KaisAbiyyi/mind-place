import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest

MODULE = Path(__file__).resolve().parents[1] / "windows_product_inputs.py"
spec = importlib.util.spec_from_file_location("windows_product_inputs", MODULE)
inputs = importlib.util.module_from_spec(spec)
spec.loader.exec_module(inputs)


class InputIdentityTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.git("init", "-q")
        self.git("config", "user.name", "Fixture")
        self.git("config", "user.email", "fixture@example.invalid")
        self.write("crates/core/src/lib.rs", "initial")
        self.write("docs/guide.md", "initial")
        self.write(".github/workflows/ci.yml", "initial")
        self.base = self.commit()

    def git(self, *args):
        return subprocess.check_output(["git", *args], cwd=self.root).decode().strip()

    def write(self, path, content):
        file = self.root / path
        file.parent.mkdir(parents=True, exist_ok=True)
        file.write_text(content)

    def commit(self):
        self.git("add", "-A")
        self.git("commit", "-qm", "fixture")
        return self.git("rev-parse", "HEAD")

    def fp(self, sha):
        return inputs.fingerprint(sha, str(self.root))

    def test_docs_only_commit_preserves_identity(self):
        self.write("docs/guide.md", "revised")
        self.assertEqual(self.fp(self.base), self.fp(self.commit()))

    def test_mixed_history_uses_tree_not_latest_diff(self):
        self.write("crates/core/src/lib.rs", "changed")
        changed = self.commit()
        self.write("docs/guide.md", "revised")
        self.assertEqual(self.fp(changed), self.fp(self.commit()))
        self.assertNotEqual(self.fp(self.base), self.fp(changed))

    def test_sensitive_paths_are_bound(self):
        for path in ("src-tauri/src/main.rs", "test/ipc.test.mjs", "package-lock.json",
                     "rust-toolchain.toml", "scripts/package.mjs", ".github/workflows/ci.yml",
                     "scripts/windows_product_inputs.py", "docs/build-config.json"):
            with self.subTest(path=path):
                self.write(path, "changed")
                new = self.commit()
                self.assertNotEqual(self.fp(self.base), self.fp(new))
                self.base = new

    def test_rejects_mutable_refs(self):
        for ref in ("HEAD", self.base[:12], "--help", "a" * 39):
            with self.subTest(ref=ref), self.assertRaises(ValueError):
                self.fp(ref)

    def test_working_tree_does_not_change_identity(self):
        expected = self.fp(self.base)
        self.write("crates/core/src/lib.rs", "uncommitted")
        self.assertEqual(expected, self.fp(self.base))

    def test_executable_document_is_bound(self):
        document = self.root / "docs/guide.md"
        document.chmod(0o755)
        self.assertNotEqual(self.fp(self.base), self.fp(self.commit()))


if __name__ == "__main__":
    unittest.main()
