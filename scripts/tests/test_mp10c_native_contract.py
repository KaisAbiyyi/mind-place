"""Adversarial MP-CI native evidence identity and execution-contract tests."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import mp10c_ci_gate as gate
import mp10c_native_contract as native_contract
import mp10c_prerequisite_wait as coordinator

REPO = "KaisAbiyyi/mind-place"
HEAD = "a" * 40
BASE = "b" * 40
CHECKOUT = "c" * 40
CONTRACT = "d" * 64


def git(root, *args):
    return subprocess.check_output(["git", *args], cwd=root, stderr=subprocess.DEVNULL)


def candidate(pr=129, **changes):
    value = {
        "pr": pr, "head": HEAD, "branch": "feature/native",
        "head_repo": "owner/repo", "head_repo_id": 1, "base_repo": "owner/repo", "base_repo_id": 1,
        "base_branch": "main", "base_sha": BASE, "checkout_sha": CHECKOUT,
    }
    value.update(changes)
    return value


def native_run(cand=None, *, run_id=10, attempt=1, path=".github/workflows/ci.yml",
               event="pull_request", status="completed", conclusion="success"):
    c = cand or candidate()
    return {
        "id": run_id, "run_attempt": attempt, "path": path, "event": event,
        "status": status, "conclusion": conclusion,
        "head_sha": c["head"], "head_branch": c["branch"],
        "repository": {"full_name": "owner/repo", "id": 1},
        "head_repository": {"full_name": c["head_repo"], "id": c["head_repo_id"]},
        "pull_requests": [{
            "number": c["pr"],
            "head": {"ref": c["branch"], "sha": c["head"],
                     "repo": {"full_name": c["head_repo"], "id": c["head_repo_id"]}},
            "base": {"ref": c["base_branch"], "sha": c["base_sha"],
                     "repo": {"full_name": c["base_repo"], "id": c["base_repo_id"]}},
        }],
    }


def job(name, steps, conclusion="success"):
    return {
        "name": name, "status": "completed", "conclusion": conclusion,
        "steps": [{"name": step, "status": "completed", "conclusion": "success"}
                  for step in steps],
    }


def ci_jobs():
    return [
        job("Fast developer feedback", (
            "Install dependencies", "Verify repository contract", "Typecheck",
            "Test reader and browser contracts", "Build reader",
            "Validate document qualification gate policy")),
        job("Fast Rust compile and vault feedback", (
            "Install pinned Rust 1.90", "Rust format", "Compile production core crates",
            "Compile available SQLCipher and vault features", "Focused security tests")),
        job("Verify Windows product", (
            "Setup Node.js", "Setup Rust 1.90", "Install dependencies",
            "Check Rust formatting", "Verify repository contract", "Typecheck",
            "Visual Settings scroll and footer regression", "Qualify bundled SQLite runtime",
            "Test", "Stage capture sidecar for native desktop tests",
            "Qualify Phase 3E simulated MCP and revocation boundary",
            "Test hosted tunnel desktop boundaries", "Build desktop app",
            "Verify portable MIT license and third-party notices")),
        job("Native execution evidence", (
            "Capture native execution provenance", "Upload native execution evidence")),
    ]


def native_evidence(cand, run, fingerprint=CONTRACT, vault=False):
    kind, _ = native_contract.candidate_requirement("owner/repo", cand["pr"])
    return {
        "schema": native_contract.EVIDENCE_SCHEMA,
        "repository": "owner/repo", "event": "pull_request",
        "workflow_path": ".github/workflows/ci.yml", "pr_number": cand["pr"],
        "head_repository": cand["head_repo"], "head_branch": cand["branch"],
        "head_sha": cand["head"], "base_repository": cand["base_repo"],
        "base_branch": cand["base_branch"], "base_sha": cand["base_sha"],
        "checkout_sha": cand["checkout_sha"], "candidate_kind": kind,
        "sqlcipher_required": vault, "native_contract_policy": native_contract.POLICY,
        "native_contract_fingerprint": fingerprint,
        "workflow_run_id": run["id"], "workflow_run_attempt": run["run_attempt"],
        "head_repository_id": 1, "base_repository_id": 1,
    }


class NativeContractFingerprintTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        git(self.root, "init", "-q")
        git(self.root, "config", "user.name", "Synthetic CI")
        git(self.root, "config", "user.email", "ci@example.invalid")
        files = {
            ".github/workflows/ci.yml": "jobs:\n  verify:\n    runs-on: windows-latest\n    steps:\n      - uses: actions/checkout@v5\n      - run: cargo test --locked\n",
            "Cargo.toml": "[workspace]\nmembers = []\n",
            "Cargo.lock": "dependency-v1\n",
            "package.json": '{"scripts":{"build":"tauri build"}}\n',
            "package-lock.json": '{"lockfileVersion":3}\n',
            "src-tauri/Cargo.toml": "[package]\nname='app'\n",
            "src-tauri/tauri.conf.json": '{"build":{}}\n',
            "scripts/mp10c_native_contract.py": "native fingerprint implementation\n",
            "scripts/mp10c_ci_gate.py": "gate implementation\n",
            "scripts/mp10c_prerequisite_wait.py": "coordinator implementation\n",
            "scripts/tests/test_mp10c_ci_gate.py": "native regression tests\n",
            "crates/security-core/src/lib.rs": "pub fn security() {}\n",
            "crates/security-core/tests/security.rs": "assert!(true);\n",
            "src/main.ts": "export const app = true;\n",
            "test/native.test.mjs": "assert.ok(true);\n",
            ".github/workflows/mp-10c-scale.yml": "100k workload scheduler\n",
            "docs/qualification.md": "documentation\n",
        }
        for path, text in files.items():
            self.write(path, text)
        git(self.root, "add", ".")
        git(self.root, "commit", "-qm", "synthetic native baseline")

    def write(self, path, text):
        target = self.root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text, encoding="utf-8")

    def commit(self, message="synthetic update"):
        git(self.root, "add", "-A")
        git(self.root, "commit", "-qm", message)

    def fp(self, pr=129):
        return native_contract.native_contract_fingerprint(str(self.root), REPO, pr)

    def test_native_contract_is_deterministic_and_sha256(self):
        first = self.fp()
        self.assertEqual(first, self.fp())
        self.assertEqual(len(first), 64)
        self.assertEqual(first, first.lower())

    def test_workflow_command_runner_and_action_changes_invalidate_contract(self):
        before = self.fp()
        path = self.root / ".github/workflows/ci.yml"
        path.write_text(path.read_text().replace("cargo test --locked", "cargo test --release --locked"))
        self.commit()
        self.assertNotEqual(before, self.fp())
        before = self.fp()
        path.write_text(path.read_text().replace("actions/checkout@v5", "actions/checkout@v4"))
        self.commit()
        self.assertNotEqual(before, self.fp())

    def test_native_test_and_dependency_lock_changes_invalidate_contract(self):
        before = self.fp()
        path = self.root / "crates/security-core/tests/security.rs"
        path.write_text("assert!(false);\n")
        self.commit()
        self.assertNotEqual(before, self.fp())
        before = self.fp()
        path = self.root / "Cargo.lock"
        path.write_text("dependency-v2\n")
        self.commit()
        self.assertNotEqual(before, self.fp())

    def test_toolchain_and_build_configuration_changes_invalidate_contract(self):
        before = self.fp()
        self.write("rust-toolchain.toml", "[toolchain]\nchannel = '1.90.0'\n")
        self.commit()
        self.assertNotEqual(before, self.fp())
        before = self.fp()
        self.write(".cargo/config.toml", "[build]\njobs = 2\n")
        self.commit()
        self.assertNotEqual(before, self.fp())

    def test_docs_and_mp10c_scheduling_workflow_are_excluded(self):
        before = self.fp()
        self.write("docs/qualification.md", "rewritten docs\n")
        self.write(".github/workflows/mp-10c-scale.yml", "new scheduling-only definition\n")
        self.commit()
        self.assertEqual(before, self.fp())

    def test_sqlcipher_requirement_is_explicit_and_candidate_specific(self):
        self.assertTrue(native_contract.requires_sqlcipher(REPO, 127))
        self.assertFalse(native_contract.requires_sqlcipher(REPO, 129))
        self.assertFalse(native_contract.requires_sqlcipher(REPO, 9001))
        # PR 129 is not a vault candidate: absence of the optional workflow is valid.
        self.assertEqual(len(self.fp(129)), 64)
        # PR 127 is a vault candidate: absence of its mandatory workflow fails closed.
        with self.assertRaises(ValueError):
            self.fp(127)
        self.write(native_contract.SQLCIPHER_WORKFLOW, "jobs:\n  sqlcipher: {}\n")
        self.commit()
        before = self.fp(127)
        self.write(native_contract.SQLCIPHER_WORKFLOW, "jobs:\n  sqlcipher:\n    runs-on: windows-2022\n")
        self.commit()
        self.assertNotEqual(before, self.fp(127))

    def test_native_evidence_captures_exact_base_head_and_run_attempt(self):
        base = git(self.root, "rev-parse", "HEAD").decode().strip()
        git(self.root, "branch", "base")
        git(self.root, "checkout", "-qb", "feature")
        self.write("crates/security-core/src/lib.rs", "pub fn security() { assert!(true); }\n")
        self.commit("head")
        head = git(self.root, "rev-parse", "HEAD").decode().strip()
        git(self.root, "checkout", "-q", "base")
        git(self.root, "merge", "--no-ff", "-qm", "PR merge", "feature")
        checkout = git(self.root, "rev-parse", "HEAD").decode().strip()
        payload = {
            "number": 129,
            "pull_request": {
                "number": 129,
                "head": {"sha": head, "ref": "feature", "repo": {"full_name": REPO, "id": 77}},
                "base": {"sha": base, "ref": "main", "repo": {"full_name": REPO, "id": 77}},
            },
        }
        evidence = native_contract.capture_native_evidence(
            str(self.root), payload, repo=REPO, run_id=700, attempt=2,
            event_name="pull_request", github_sha=checkout)
        self.assertEqual((evidence["pr_number"], evidence["head_sha"], evidence["base_sha"]),
                         (129, head, base))
        self.assertEqual((evidence["workflow_run_id"], evidence["workflow_run_attempt"]), (700, 2))
        self.assertEqual(evidence["native_contract_policy"], "mp-native-contract/v1")
        bad = json.loads(json.dumps(payload))
        bad["pull_request"]["base"]["sha"] = "e" * 40
        with self.assertRaises(ValueError):
            native_contract.capture_native_evidence(
                str(self.root), bad, repo=REPO, run_id=700, attempt=2)
        with self.assertRaises(ValueError):
            native_contract.capture_native_evidence(
                str(self.root), payload, repo=REPO, run_id=700, attempt=2,
                event_name="workflow_dispatch")
        forked = json.loads(json.dumps(payload))
        forked["pull_request"]["head"]["repo"] = {
            "full_name": "foreign/repo", "id": 88}
        with self.assertRaises(ValueError):
            native_contract.capture_native_evidence(
                str(self.root), forked, repo=REPO, run_id=700, attempt=2)


class NativeIdentityAndPrerequisiteTests(unittest.TestCase):
    def setUp(self):
        self.candidate = candidate()
        self.run = native_run(self.candidate)
        self.jobs = ci_jobs()
        self.evidence = native_evidence(self.candidate, self.run)
        self.args = {
            "repo": "owner/repo", "vault": False, "contract_fingerprint": CONTRACT,
            "fetch_jobs": lambda _: {"total_count": len(self.jobs), "jobs": self.jobs},
            "fetch_evidence": lambda _: self.evidence,
        }

    def require(self, runs=None, candidate_value=None):
        gate.require_prerequisites(
            [self.run] if runs is None else runs,
            self.candidate if candidate_value is None else candidate_value,
            **self.args)

    def test_exact_run_and_same_run_fingerprint_evidence_are_accepted(self):
        self.require()

    def test_foreign_pr_and_same_head_different_pr_are_rejected(self):
        for altered in (
            dict(self.run, pull_requests=[dict(self.run["pull_requests"][0], number=130)]),
            dict(self.run, pull_requests=[dict(self.run["pull_requests"][0], number=127)]),
        ):
            with self.subTest(altered=altered), self.assertRaises(ValueError):
                self.require([altered])

    def test_foreign_head_repo_and_fork_pr_are_rejected(self):
        for altered in (
            dict(self.run, head_repository={"full_name": "foreign/repo"}),
            dict(self.run, pull_requests=[{
                "number": 129,
                "head": {"ref": "feature/native", "sha": HEAD, "repo": {"full_name": "foreign/repo", "id": 2}},
                "base": {"ref": "main", "sha": BASE, "repo": {"full_name": "owner/repo"}},
            }]),
        ):
            with self.subTest(altered=altered), self.assertRaises(ValueError):
                self.require([altered])

    def test_foreign_base_repo_wrong_base_branch_and_stale_base_are_rejected(self):
        for altered in (
            dict(self.run, pull_requests=[{
                "number": 129,
                "head": self.run["pull_requests"][0]["head"],
                "base": {"ref": "main", "sha": BASE, "repo": {"full_name": "foreign/repo", "id": 2}},
            }]),
            dict(self.run, pull_requests=[{
                "number": 129,
                "head": self.run["pull_requests"][0]["head"],
                "base": {"ref": "release", "sha": BASE, "repo": {"full_name": "owner/repo", "id": 1}},
            }]),
            dict(self.run, pull_requests=[{
                "number": 129,
                "head": self.run["pull_requests"][0]["head"],
                "base": {"ref": "main", "sha": "f"*40, "repo": {"full_name": "owner/repo", "id": 1}},
            }]),
        ):
            with self.subTest(altered=altered), self.assertRaises(ValueError):
                self.require([altered])

    def test_stale_head_dispatch_and_ambiguous_pr_context_are_rejected(self):
        for altered in (
            dict(self.run, head_sha="e"*40),
            dict(self.run, event="workflow_dispatch"),
            dict(self.run, pull_requests=[self.run["pull_requests"][0], self.run["pull_requests"][0]]),
        ):
            with self.subTest(altered=altered), self.assertRaises(ValueError):
                self.require([altered])

    def test_missing_or_incorrect_native_fingerprint_is_rejected(self):
        original = self.evidence["native_contract_fingerprint"]
        self.evidence.pop("native_contract_fingerprint")
        with self.assertRaises(ValueError):
            self.require()
        self.evidence["native_contract_fingerprint"] = "0"*64
        with self.assertRaises(ValueError):
            self.require()
        self.evidence["native_contract_fingerprint"] = original
        self.evidence["workflow_run_attempt"] = 2
        with self.assertRaises(ValueError):
            self.require()

    def test_skipped_native_job_and_skipped_critical_step_are_rejected(self):
        self.jobs[2]["conclusion"] = "skipped"
        with self.assertRaises(ValueError):
            self.require()
        self.jobs[2]["conclusion"] = "success"
        self.jobs[2]["steps"][0]["conclusion"] = "skipped"
        with self.assertRaises(ValueError):
            self.require()

    def test_successful_run_with_wrong_identity_never_satisfies_prerequisite(self):
        wrong = dict(self.run, pull_requests=[dict(self.run["pull_requests"][0], number=128)])
        with self.assertRaises(ValueError):
            self.require([wrong])

    def test_candidate_policy_does_not_infer_sqlcipher_from_file_presence(self):
        self.assertFalse(native_contract.requires_sqlcipher("owner/repo", 129))
        self.assertTrue(native_contract.requires_sqlcipher(REPO, 127))
        with self.assertRaises(ValueError):
            gate.require_prerequisites(
                [self.run], candidate(127, head_repo="owner/repo", base_repo="owner/repo"),
                repo=REPO, vault=False, contract_fingerprint=CONTRACT,
                fetch_jobs=lambda _: {"total_count": len(self.jobs), "jobs": self.jobs},
                fetch_evidence=lambda _: self.evidence)

    def test_optional_sqlcipher_workflow_absence_does_not_disable_non_vault_native_check(self):
        self.require()
        # No SQLCipher run is supplied; the explicit PR 129 policy makes it optional.

    def test_coordinator_pending_then_success_and_failed_or_missing_prerequisites(self):
        in_progress = dict(self.run, status="in_progress", conclusion=None)
        fetch_jobs = self.args["fetch_jobs"]
        fetch_evidence = self.args["fetch_evidence"]
        self.assertEqual(coordinator.classify(
            [in_progress], self.candidate, "owner/repo", False, CONTRACT,
            fetch_jobs, fetch_evidence)[0], "pending")
        self.assertEqual(coordinator.classify(
            [self.run], self.candidate, "owner/repo", False, CONTRACT,
            fetch_jobs, fetch_evidence)[0], "ready")
        failed = dict(self.run, conclusion="failure")
        self.assertEqual(coordinator.classify(
            [failed], self.candidate, "owner/repo", False, CONTRACT,
            fetch_jobs, fetch_evidence)[0], "failed")
        self.assertEqual(coordinator.classify(
            [], self.candidate, "owner/repo", False, CONTRACT,
            fetch_jobs, fetch_evidence)[0], "pending")
        self.jobs[2]["steps"][0]["conclusion"] = "skipped"
        self.assertEqual(coordinator.classify(
            [self.run], self.candidate, "owner/repo", False, CONTRACT,
            fetch_jobs, fetch_evidence)[0], "failed")

    def test_candidate_head_or_base_mutation_during_wait_fails_closed(self):
        mutations = (
            {
                "number": 129, "state": "open",
                "head": {"sha": "e"*40, "ref": "feature/native",
                         "repo": {"full_name": "owner/repo", "id": 1}},
                "base": {"sha": BASE, "ref": "main",
                         "repo": {"full_name": "owner/repo", "id": 1}},
            },
            {
                "number": 129, "state": "open",
                "head": {"sha": HEAD, "ref": "feature/native",
                         "repo": {"full_name": "owner/repo", "id": 1}},
                "base": {"sha": "f"*40, "ref": "main",
                         "repo": {"full_name": "owner/repo", "id": 1}},
            },
        )
        for changed in mutations:
            with self.subTest(changed=changed), patch.object(gate, "api_json", return_value=changed):
                state, reason = coordinator.snapshot(
                    "owner/repo", self.candidate, "token", False, CONTRACT)
                self.assertEqual(state, "failed")
                self.assertIn("identity", reason.lower())

    def test_wait_is_bounded_and_api_errors_never_become_acceptance(self):
        results = iter([("pending", "queued"), ("ready", "verified")])
        sleeps = []
        coordinator.wait(
            "owner/repo", self.candidate, "token", False, CONTRACT,
            timeout=60, poll=10, clock=lambda: len(sleeps)*10,
            sleep=lambda duration: sleeps.append(duration),
            inspect=lambda *_: next(results))
        self.assertEqual(sleeps, [10])
        with self.assertRaises(TimeoutError):
            coordinator.wait(
                "owner/repo", self.candidate, "token", False, CONTRACT,
                timeout=0, inspect=lambda *_: (_ for _ in ()).throw(OSError("API unavailable")))
        with self.assertRaises(TimeoutError):
            coordinator.wait(
                "owner/repo", self.candidate, "token", False, CONTRACT,
                timeout=0, inspect=lambda *_: ("pending", "missing"))


if __name__ == "__main__":
    unittest.main()
