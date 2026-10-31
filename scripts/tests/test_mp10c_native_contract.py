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
        "repository": {"full_name": c["base_repo"], "id": c["base_repo_id"]},
        "head_repository": {"full_name": c["head_repo"], "id": c["head_repo_id"]},
        "pull_requests": [{
            "number": c["pr"],
            "head": {"ref": c["branch"], "sha": c["head"],
                     "repo": {"full_name": c["head_repo"], "id": c["head_repo_id"]}},
            "base": {"ref": c["base_branch"], "sha": c["base_sha"],
                     "repo": {"full_name": c["base_repo"], "id": c["base_repo_id"]}},
        }],
    }


def job(name, steps, conclusion="success", labels=("ubuntu-latest",), run_attempt=1):
    return {
        "name": name, "status": "completed", "conclusion": conclusion,
        "run_attempt": run_attempt, "labels": list(labels),
        "steps": [{"name": step, "status": "completed", "conclusion": "success"}
                  for step in steps],
    }


def ci_jobs():
    return [
        job("Fast developer feedback", (
            "Install dependencies", "Verify repository contract", "Typecheck",
            "Test reader and browser contracts", "Build reader",
            "Validate document qualification gate policy"), labels=("ubuntu-latest",)),
        job("Fast Rust compile and vault feedback", (
            "Install pinned Rust 1.90", "Rust format", "Compile production core crates",
            "Compile available SQLCipher and vault features", "Focused security tests"),
            labels=("ubuntu-24.04",)),
        job("Verify Windows product", (
            "Setup Node.js", "Setup Rust 1.90", "Install dependencies",
            "Check Rust formatting", "Verify repository contract", "Typecheck",
            "Visual Settings scroll and footer regression", "Qualify bundled SQLite runtime",
            "Test", "Stage capture sidecar for native desktop tests",
            "Qualify Phase 3E simulated MCP and revocation boundary",
            "Test hosted tunnel desktop boundaries", "Build desktop app",
            "Verify portable MIT license and third-party notices"), labels=("windows-latest",)),
        job("Native execution evidence", (
            "Capture native execution provenance", "Upload native execution evidence"),
            labels=("ubuntu-latest",)),
    ]


def native_evidence(cand, run, fingerprint=CONTRACT, vault=False):
    kind, _ = native_contract.candidate_requirement(cand["base_repo"], cand["pr"])
    return {
        "schema": native_contract.EVIDENCE_SCHEMA,
        "repository": cand["base_repo"], "event": run["event"],
        "workflow_path": ".github/workflows/ci.yml", "pr_number": cand["pr"],
        "head_repository": cand["head_repo"], "head_branch": cand["branch"],
        "head_sha": cand["head"], "base_repository": cand["base_repo"],
        "base_branch": cand["base_branch"], "base_sha": cand["base_sha"],
        "checkout_sha": cand["checkout_sha"], "candidate_kind": kind,
        "sqlcipher_required": vault, "native_contract_policy": native_contract.POLICY,
        "native_contract_fingerprint": fingerprint,
        "workflow_run_id": run["id"], "workflow_run_attempt": run["run_attempt"],
        "head_repository_id": cand["head_repo_id"], "base_repository_id": cand["base_repo_id"],
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


    def test_native_workflow_fingerprint_binds_timeout_dependencies_env_and_required_steps(self):
        path = self.root / ".github/workflows/ci.yml"
        before = self.fp()
        text = path.read_text()
        path.write_text(text.replace(
            "    runs-on: windows-latest\n",
            "    runs-on: windows-latest\n    timeout-minutes: 30\n"))
        self.commit()
        self.assertNotEqual(before, self.fp())

        before = self.fp()
        text = path.read_text()
        path.write_text(text.replace(
            "    timeout-minutes: 30\n",
            "    timeout-minutes: 30\n    needs: prepare\n"))
        self.commit()
        self.assertNotEqual(before, self.fp())

        before = self.fp()
        text = path.read_text()
        path.write_text(text.replace(
            "    needs: prepare\n",
            "    needs: prepare\n    env:\n      BUILD_PROFILE: release\n"))
        self.commit()
        self.assertNotEqual(before, self.fp())

        before = self.fp()
        text = path.read_text()
        path.write_text(text.replace(
            "      - run: cargo test --locked\n",
            "      - name: Required native verification step\n        run: cargo test --locked\n"))
        self.commit()
        self.assertNotEqual(before, self.fp())

    def test_native_workflow_fingerprint_ignores_yaml_formatting_and_comments(self):
        before = self.fp()
        self.write(".github/workflows/ci.yml", (
            "# formatting-only rewrite\n"
            "jobs: # job map\n"
            " verify:\n"
            "  runs-on: 'windows-latest' # same runner\n"
            "  steps:\n"
            "   - uses: 'actions/checkout@v5'\n"
            "   - run: 'cargo test --locked'\n"
        ))
        self.commit()
        self.assertEqual(before, self.fp())

    def test_identity_metadata_inside_command_is_not_globally_excluded(self):
        path = self.root / ".github/workflows/ci.yml"
        text = path.read_text().replace(
            "      - run: cargo test --locked\n",
            "      - run: |\n          echo PR_HEAD_REPOSITORY: original/repo\n")
        self.write(".github/workflows/ci.yml", text)
        self.commit()
        before = self.fp()
        self.write(".github/workflows/ci.yml", path.read_text().replace(
            "PR_HEAD_REPOSITORY: original/repo",
            "PR_HEAD_REPOSITORY: changed/repo"))
        self.commit()
        self.assertNotEqual(before, self.fp())

    def test_identity_metadata_in_unrelated_job_is_not_globally_excluded(self):
        path = self.root / ".github/workflows/ci.yml"
        text = path.read_text() + (
            "\n  unrelated:\n"
            "    name: Unrelated job\n"
            "    runs-on: ubuntu-latest\n"
            "    env:\n"
            "      PR_HEAD_REPOSITORY: original/repo\n"
            "    steps:\n"
            "      - run: echo unrelated\n"
        )
        self.write(".github/workflows/ci.yml", text)
        self.commit()
        before = self.fp()
        self.write(".github/workflows/ci.yml", path.read_text().replace(
            "PR_HEAD_REPOSITORY: original/repo",
            "PR_HEAD_REPOSITORY: changed/repo"))
        self.commit()
        self.assertNotEqual(before, self.fp())

    def test_runner_contract_is_derived_from_authoritative_workflow(self):
        source = (
            "jobs:\n"
            "  linux:\n"
            "    name: Linux native\n"
            "    runs-on: ubuntu-24.04\n"
            "    steps:\n"
            "      - run: cargo test\n"
            "  windows:\n"
            "    name: Windows native\n"
            "    runs-on: windows-2022\n"
            "    steps:\n"
            "      - run: cargo test\n"
        )
        linux = native_contract.workflow_job_runner_requirements(source, "Linux native")
        windows = native_contract.workflow_job_runner_requirements(source, "Windows native")
        self.assertEqual((linux["labels"], linux["platform"]), (["ubuntu-24.04"], "linux"))
        self.assertEqual((windows["labels"], windows["platform"]), (["windows-2022"], "windows"))
        native_contract.validate_runner_labels(
            {"labels": ["ubuntu-24.04", "linux", "x64"]}, linux)
        native_contract.validate_runner_labels(
            {"labels": ["windows-2022", "windows", "x64"]}, windows)
        for labels in (["ubuntu-24.04", "windows"], None, "ubuntu-24.04", ["windows-2022", 7]):
            with self.subTest(labels=labels), self.assertRaises(ValueError):
                native_contract.validate_runner_labels({"labels": labels}, windows)


    def test_matrix_runner_requirements_resolve_each_expanded_sqlcipher_job(self):
        source = (
            "jobs:\n"
            "  sqlcipher:\n"
            "    name: SQLCipher / ${{ matrix.os }}\n"
            "    strategy:\n"
            "      fail-fast: false\n"
            "      matrix:\n"
            "        os: [ubuntu-24.04, windows-2022]\n"
            "    runs-on: ${{ matrix.os }}\n"
            "    steps:\n"
            "      - run: cargo test\n"
        )
        linux = native_contract.workflow_job_runner_requirements(
            source, "SQLCipher / ubuntu-24.04")
        windows = native_contract.workflow_job_runner_requirements(
            source, "SQLCipher / windows-2022")
        self.assertEqual((linux["labels"], linux["platform"], linux["matrix"]["os"]),
                         (["ubuntu-24.04"], "linux", "ubuntu-24.04"))
        self.assertEqual((windows["labels"], windows["platform"], windows["matrix"]["os"]),
                         (["windows-2022"], "windows", "windows-2022"))

        mismatched = source.replace(
            "    runs-on: ${{ matrix.os }}\n",
            "    runs-on: ubuntu-24.04\n")
        with self.assertRaises(ValueError):
            native_contract.workflow_job_runner_requirements(
                mismatched, "SQLCipher / windows-2022")


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
        self.write(native_contract.SQLCIPHER_WORKFLOW, "jobs:\n  sqlcipher:\n    runs-on: ubuntu-24.04\n    steps:\n      - run: cargo test\n")
        self.commit()
        before = self.fp(127)
        self.write(native_contract.SQLCIPHER_WORKFLOW, "jobs:\n  sqlcipher:\n    runs-on: windows-2022\n    steps:\n      - run: cargo test\n")
        self.commit()
        self.assertNotEqual(before, self.fp(127))

    def test_manual_native_dispatch_requires_exact_live_pr_inputs_and_merge_checkout(self):
        base = git(self.root, "rev-parse", "HEAD").decode().strip()
        git(self.root, "branch", "fixture-base", base)
        git(self.root, "checkout", "-qb", "feature/native-dispatch")
        path = self.root / "crates/security-core/src/lib.rs"
        path.write_text("pub fn security() { assert!(true); }\n")
        git(self.root, "add", ".")
        git(self.root, "commit", "-qm", "manual dispatch candidate")
        head = git(self.root, "rev-parse", "HEAD").decode().strip()
        git(self.root, "checkout", "-q", "fixture-base")
        git(self.root, "merge", "--no-ff", "-qm", "synthetic PR merge", "feature/native-dispatch")
        checkout = git(self.root, "rev-parse", "HEAD").decode().strip()
        pull = {
            "number": 129, "state": "open",
            "head": {"sha": head, "ref": "feature/native-dispatch",
                     "repo": {"full_name": REPO, "id": 77}},
            "base": {"sha": base, "ref": "fixture-base",
                     "repo": {"full_name": REPO, "id": 77}},
        }
        payload = {"inputs": {"pr_number": "129", "head_sha": head, "base_sha": base}}
        with patch.object(native_contract, "api_json", return_value=pull):
            evidence = native_contract.capture_native_evidence(
                str(self.root), payload, repo=REPO, run_id=701, attempt=1,
                event_name="workflow_dispatch", github_sha=head,
                github_ref="refs/heads/feature/native-dispatch", token="synthetic")
        self.assertEqual(evidence["event"], "workflow_dispatch")
        self.assertEqual((evidence["pr_number"], evidence["head_sha"], evidence["base_sha"]), (129, head, base))
        self.assertEqual(evidence["checkout_sha"], checkout)
        self.assertEqual(evidence["workflow_run_id"], 701)

        invalid_payloads = (
            {"inputs": {"pr_number": "129", "head_sha": "e"*40, "base_sha": base}},
            {"inputs": {"pr_number": "129", "head_sha": head, "base_sha": "f"*40}},
            {"inputs": {"pr_number": "0", "head_sha": head, "base_sha": base}},
            {"inputs": {"pr_number": "129", "head_sha": head}},
        )
        for invalid in invalid_payloads:
            with self.subTest(inputs=invalid), patch.object(native_contract, "api_json", return_value=pull):
                with self.assertRaises(ValueError):
                    native_contract.capture_native_evidence(
                        str(self.root), invalid, repo=REPO, run_id=701, attempt=1,
                        event_name="workflow_dispatch", github_sha=head,
                        github_ref="refs/heads/feature/native-dispatch", token="synthetic")
        with patch.object(native_contract, "api_json", return_value=pull):
            with self.assertRaises(ValueError):
                native_contract.capture_native_evidence(
                    str(self.root), payload, repo=REPO, run_id=701, attempt=1,
                    event_name="workflow_dispatch", github_sha=head,
                    github_ref="refs/heads/main", token="synthetic")
        closed = dict(pull, state="closed")
        with patch.object(native_contract, "api_json", return_value=closed):
            with self.assertRaises(ValueError):
                native_contract.capture_native_evidence(
                    str(self.root), payload, repo=REPO, run_id=701, attempt=1,
                    event_name="workflow_dispatch", github_sha=head,
                    github_ref="refs/heads/feature/native-dispatch", token="synthetic")

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
        self.assertEqual(evidence["native_contract_policy"], "mp-native-contract/v2")
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
        ci_source = Path(__file__).resolve().parents[2].joinpath(
            ".github/workflows/ci.yml").read_bytes()
        self.workflow_sources = {native_contract.CI_WORKFLOW: ci_source}
        self.args = {
            "repo": "owner/repo", "vault": False, "contract_fingerprint": CONTRACT,
            "fetch_jobs": lambda _: {"total_count": len(self.jobs), "jobs": self.jobs},
            "fetch_evidence": lambda _: self.evidence,
            "workflow_sources": self.workflow_sources,
        }

    def require(self, runs=None, candidate_value=None):
        gate.require_prerequisites(
            [self.run] if runs is None else runs,
            self.candidate if candidate_value is None else candidate_value,
            **self.args)

    def test_exact_run_and_same_run_fingerprint_evidence_are_accepted(self):
        self.require()

    def test_exact_manual_dispatch_native_run_with_same_run_evidence_is_accepted(self):
        dispatched = native_run(self.candidate, event="workflow_dispatch")
        dispatched.pop("pull_requests")
        evidence = native_evidence(self.candidate, dispatched)
        self.args["fetch_evidence"] = lambda _: evidence
        self.require([dispatched])

    def test_manual_dispatch_run_with_stale_head_or_unbound_evidence_is_rejected(self):
        dispatched = native_run(self.candidate, event="workflow_dispatch")
        dispatched.pop("pull_requests")
        dispatched["head_sha"] = "e" * 40
        with self.assertRaises(ValueError):
            self.require([dispatched])
        dispatched["head_sha"] = self.candidate["head"]
        self.args["fetch_evidence"] = lambda _: {}
        with self.assertRaises(ValueError):
            self.require([dispatched])

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


    def test_wrong_windows_runner_platform_is_rejected(self):
        for labels in (["ubuntu-latest"], ["macos-latest"]):
            with self.subTest(labels=labels):
                self.jobs[2]["labels"] = labels
                with self.assertRaises(ValueError):
                    self.require()
        self.jobs[2]["labels"] = ["windows-latest"]

    def test_missing_or_malformed_runner_labels_are_rejected(self):
        original = self.jobs[2].get("labels")
        for labels in (None, "windows-latest", [], ["windows-latest", None], ["windows-latest", "windows-latest"]):
            with self.subTest(labels=labels):
                self.jobs[2]["labels"] = labels
                with self.assertRaises(ValueError):
                    self.require()
        self.jobs[2]["labels"] = original

    def test_duplicate_required_jobs_are_rejected_even_with_one_success(self):
        original = self.jobs[2]
        for conclusion in ("failure", "skipped", "success"):
            duplicate = json.loads(json.dumps(original))
            duplicate["conclusion"] = conclusion
            self.jobs.append(duplicate)
            with self.subTest(duplicate_conclusion=conclusion), self.assertRaises(ValueError):
                self.require()
            self.jobs.pop()

    def test_duplicate_critical_step_is_rejected(self):
        duplicate = json.loads(json.dumps(self.jobs[2]["steps"][0]))
        self.jobs[2]["steps"].append(duplicate)
        with self.assertRaises(ValueError):
            self.require()

    def test_native_job_attempt_must_match_workflow_run_attempt(self):
        self.jobs[2]["run_attempt"] = 2
        with self.assertRaises(ValueError):
            self.require()

    def test_incomplete_github_job_listing_is_rejected(self):
        self.args["fetch_jobs"] = lambda _: {
            "total_count": len(self.jobs) + 1, "jobs": self.jobs,
        }
        with self.assertRaises(ValueError):
            self.require()

    def _require_sqlcipher_with_runner_labels(self, ubuntu_labels, windows_labels,
                                              ipc_conclusion="success", duplicate_ipc=False):
        cand = candidate(127, head_repo=REPO, base_repo=REPO)
        ci_run = native_run(cand, run_id=110)
        sql_run = native_run(cand, run_id=111, path=native_contract.SQLCIPHER_WORKFLOW)
        sql_steps = (
            "Check Rust formatting", "Encrypted state/index and FTS regression",
            "Keyed MCP synthetic authorization and revocation",
            "Vault and backup adversarial regression",
        )
        sql_jobs = [
            job("SQLCipher / ubuntu-24.04", sql_steps, labels=ubuntu_labels),
            job("SQLCipher / windows-2022", sql_steps, labels=windows_labels),
        ]
        if ipc_conclusion is not None:
            self.jobs[2]["steps"].append({
                "name": "Qualify native capture encrypted IPC and revoked owner epoch",
                "status": "completed", "conclusion": ipc_conclusion,
            })
        if duplicate_ipc:
            self.jobs[2]["steps"].append(dict(self.jobs[2]["steps"][-1], conclusion="skipped"))
        by_run = {ci_run["id"]: self.jobs, sql_run["id"]: sql_jobs}
        sql_source = (
            "jobs:\n"
            "  sqlcipher:\n"
            "    name: SQLCipher / ${{ matrix.os }}\n"
            "    strategy:\n"
            "      fail-fast: false\n"
            "      matrix:\n"
            "        os: [ubuntu-24.04, windows-2022]\n"
            "    runs-on: ${{ matrix.os }}\n"
            "    steps:\n"
            "      - name: Check Rust formatting\n"
            "        run: cargo fmt --all -- --check\n"
            "      - name: Encrypted state/index and FTS regression\n"
            "        run: cargo test\n"
            "      - name: Keyed MCP synthetic authorization and revocation\n"
            "        run: cargo test\n"
            "      - name: Vault and backup adversarial regression\n"
            "        run: cargo test\n"
        )
        sources = dict(self.workflow_sources)
        sources[native_contract.SQLCIPHER_WORKFLOW] = sql_source
        args = {
            "repo": REPO, "vault": True, "contract_fingerprint": CONTRACT,
            "fetch_jobs": lambda run: {
                "total_count": len(by_run[run["id"]]), "jobs": by_run[run["id"]],
            },
            "fetch_evidence": lambda _: native_evidence(cand, ci_run, vault=True),
            "workflow_sources": sources,
        }
        gate.require_prerequisites([ci_run, sql_run], cand, **args)

    def test_vault_native_capture_ipc_success_is_accepted(self):
        self._require_sqlcipher_with_runner_labels(
            ["ubuntu-24.04"], ["windows-2022"])

    def test_vault_native_capture_ipc_cannot_be_missing_or_skipped(self):
        for conclusion in (None, "skipped", "failure", "cancelled"):
            with self.subTest(conclusion=conclusion), self.assertRaisesRegex(
                    ValueError, "native capture encrypted IPC"):
                self.setUp()
                self._require_sqlcipher_with_runner_labels(
                    ["ubuntu-24.04"], ["windows-2022"], ipc_conclusion=conclusion)

    def test_duplicate_vault_ipc_step_does_not_satisfy_native_prerequisite(self):
        with self.assertRaisesRegex(ValueError, "native capture encrypted IPC"):
            self._require_sqlcipher_with_runner_labels(
                ["ubuntu-24.04"], ["windows-2022"], duplicate_ipc=True)

    def test_sqlcipher_windows_job_on_ubuntu_is_rejected(self):
        with self.assertRaises(ValueError):
            self._require_sqlcipher_with_runner_labels(
                ["ubuntu-24.04"], ["ubuntu-24.04"])

    def test_sqlcipher_ubuntu_job_on_windows_is_rejected(self):
        with self.assertRaises(ValueError):
            self._require_sqlcipher_with_runner_labels(
                ["windows-2022"], ["windows-2022"])

    def test_skipped_failed_cancelled_native_product_and_skipped_step_are_rejected(self):
        for conclusion in ("skipped", "failure", "cancelled"):
            self.jobs[2]["conclusion"] = conclusion
            with self.subTest(conclusion=conclusion), self.assertRaises(ValueError):
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

    def test_cached_proof_selection_still_requires_current_exact_native_evidence(self):
        cached = {"run_id": 77, "artifact_id": 12}
        self.require()
        accepted, fresh = gate.select_acceptance_evidence(
            native_result="success", fresh=None, previous=cached,
            event="pull_request", mode="auto")
        self.assertIs(accepted, cached)
        self.assertFalse(fresh)

        self.jobs[2]["labels"] = ["ubuntu-latest"]
        with self.assertRaises(ValueError):
            self.require()
        with self.assertRaises(ValueError):
            gate.select_acceptance_evidence(
                native_result="failure", fresh=None, previous=cached,
                event="pull_request", mode="auto")

        self.jobs[2]["labels"] = ["windows-latest"]
        self.evidence["native_contract_fingerprint"] = "e" * 64
        with self.assertRaises(ValueError):
            self.require()
        with self.assertRaises(ValueError):
            gate.select_acceptance_evidence(
                native_result="failure", fresh=None, previous=cached,
                event="pull_request", mode="auto")

    def test_missing_native_workflow_times_out_and_cannot_accept_cached_proof(self):
        state, reason = coordinator.classify(
            [], self.candidate, "owner/repo", False, CONTRACT,
            self.args["fetch_jobs"], self.args["fetch_evidence"])
        self.assertEqual(state, "pending")
        with self.assertRaises(TimeoutError):
            coordinator.wait(
                "owner/repo", self.candidate, "token", False, CONTRACT,
                timeout=0, inspect=lambda *_: (state, reason))
        with self.assertRaises(ValueError):
            gate.select_acceptance_evidence(
                native_result="skipped", fresh=None,
                previous={"run_id": 77}, event="pull_request", mode="auto")

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
