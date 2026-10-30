"""Synthetic MP-CI-100K-01 regression matrix; no network and no owner corpus."""
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
import zipfile

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import mp10c_ci_gate as gate


def raw_receipt(tier=100000):
    return {
        "schema": "mind-place-mp10c-document-scale/v1",
        "fixture": "synthetic-only", "mode": "real-approved-root-index",
        "platform": "windows", "target_files": tier,
        "indexed_files": tier, "extracted_chunks": tier * 10,
        "approved_roots": (tier + 9999) // 10000,
        "production_files_per_root_cap": 10000,
        "raw_bytes_unchanged": True, "sqlite_and_fts_integrity": "pass",
        "fixture_sha256": "c" * 64,
    }


def git(root, *args):
    return subprocess.check_output(["git", *args], cwd=root, stderr=subprocess.DEVNULL)


class FingerprintTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        git(self.root, "init", "-q")
        self.write("crates/index-core/src/documents.rs", "original")
        self.write("Cargo.lock", "dependency-A")
        self.write(".github/workflows/mp-10c-scale.yml", "jobs:\n  document-smoke:\n    runs-on: windows-latest\n    steps:\n      - run: cargo test smoke\n  document-scale:\n    runs-on: windows-latest\n    steps:\n      - run: cargo test full\n  provenance:\n    runs-on: ubuntu-latest\n")
        self.write(".github/workflows/ci.yml", "timeout 30")
        self.write("docs/history.md", "docs")
        git(self.root, "add", ".")

    def write(self, path, content):
        file = self.root / path
        file.parent.mkdir(parents=True, exist_ok=True)
        file.write_text(content)
        git(self.root, "add", "-A")

    def fp(self):
        return gate.fingerprint(str(self.root))

    def test_unrelated_ci_timeout_and_docs_do_not_requalify(self):
        before = self.fp()
        self.write(".github/workflows/ci.yml", "timeout 120")
        self.write("docs/history.md", "completely rewritten")
        self.assertEqual(before, self.fp())

    def test_relevant_index_scanner_object_authority_test_and_dependency_force(self):
        for path in (
            "crates/index-core/src/documents.rs",
            "crates/state-core/src/document_scanner.rs",
            "crates/document-core/src/lib.rs",
            "crates/security-core/src/lib.rs",
            "crates/state-core/src/cipher.rs",
            "crates/index-core/tests/document_scale_qualification.rs",
            "Cargo.lock", "Cargo.toml", ".cargo/config.toml",
            "rust-toolchain.toml",
        ):
            with self.subTest(path=path):
                before = self.fp()
                self.write(path, (self.root.joinpath(path).read_text() if self.root.joinpath(path).exists() else "") + "!")
                self.assertNotEqual(before, self.fp())

    def test_native_candidate_identity_environment_does_not_reindex(self):
        workflow = self.root.joinpath(".github/workflows/mp-10c-scale.yml").read_text()
        workflow = workflow.replace(
            "  document-scale:\n    runs-on: windows-latest\n",
            "  document-scale:\n    runs-on: windows-latest\n    env:\n      CARGO_INCREMENTAL: '0'\n")
        self.write(".github/workflows/mp-10c-scale.yml", workflow)
        before = self.fp()
        workflow = workflow.replace(
            "      CARGO_INCREMENTAL: '0'\n",
            "      CARGO_INCREMENTAL: '0'\n"
            "      PR_HEAD_REPOSITORY: owner/repo\n"
            "      PR_BASE_SHA: " + "b"*40 + "\n"
            "      NATIVE_CONTRACT_FINGERPRINT: " + "c"*64 + "\n")
        self.write(".github/workflows/mp-10c-scale.yml", workflow)
        self.assertEqual(before, self.fp())


    def test_metadata_text_inside_scale_execution_command_is_not_excluded(self):
        workflow = self.root.joinpath(".github/workflows/mp-10c-scale.yml").read_text()
        workflow = workflow.replace(
            "      - run: cargo test full\n",
            "      - run: |\n          echo PR_HEAD_REPOSITORY: original/repo\n")
        self.write(".github/workflows/mp-10c-scale.yml", workflow)
        before = self.fp()
        workflow = self.root.joinpath(".github/workflows/mp-10c-scale.yml").read_text()
        self.write(".github/workflows/mp-10c-scale.yml", workflow.replace(
            "PR_HEAD_REPOSITORY: original/repo",
            "PR_HEAD_REPOSITORY: changed/repo"))
        self.assertNotEqual(before, self.fp())

    def test_verified_original_workload_v2_fingerprint_is_preserved(self):
        repository_root = Path(__file__).resolve().parents[2]
        self.assertEqual(
            gate.fingerprint(str(repository_root)),
            "58b9293787c3d1782e05ed346bb1a1bc539827d6ba45b887cc39be4a36ea8649",
        )

    def test_validator_only_and_coordination_only_edits_do_not_reindex(self):
        before = self.fp()
        self.write("scripts/mp10c_ci_gate.py", "new strict parser")
        self.write("scripts/tests/test_mp10c_ci_gate.py", "new tests")
        self.write("scripts/mp10c_prerequisite_wait.py", "bounded wait")
        self.write("scripts/mp10c_native_contract.py", "native-only verifier")
        self.write("scripts/tests/test_mp10c_native_contract.py", "native contract tests")
        text = self.root.joinpath(".github/workflows/mp-10c-scale.yml").read_text()
        self.write(".github/workflows/mp-10c-scale.yml", text.replace(
            "  document-scale:", "  coordinator:\n    runs-on: ubuntu-latest\n  document-scale:"))
        self.assertEqual(before, self.fp())
        self.write(".github/workflows/mp-10c-scale.yml",
                   self.root.joinpath(".github/workflows/mp-10c-scale.yml").read_text().replace(
                       "cargo test full", "cargo test different"))
        self.assertNotEqual(before, self.fp())

    def test_deletion_of_dependency_forces(self):
        before = self.fp()
        (self.root / "Cargo.lock").unlink()
        git(self.root, "add", "-A")
        self.assertNotEqual(before, self.fp())

    def test_reverted_relevant_change_has_same_effective_fingerprint(self):
        before = self.fp()
        self.write("Cargo.lock", "temporary")
        self.assertNotEqual(before, self.fp())
        self.write("Cargo.lock", "dependency-A")
        self.assertEqual(before, self.fp())


class ShallowMergeTests(unittest.TestCase):
    def test_real_depth_one_checkout_retains_parent_object_headers(self):
        """Real shallow clone reproduces the failed rev-list behavior; no mocks."""
        with tempfile.TemporaryDirectory() as tmp:
            root, clone = Path(tmp)/"origin", Path(tmp)/"clone"
            root.mkdir()
            git(root, "init", "-q")
            git(root, "config", "user.name", "CI Fixture")
            git(root, "config", "user.email", "ci-fixture@example.invalid")
            git(root, "commit", "--allow-empty", "-q", "-m", "base")
            base = git(root, "rev-parse", "HEAD").decode().strip()
            git(root, "checkout", "-q", "-b", "source")
            git(root, "commit", "--allow-empty", "-q", "-m", "branch")
            source = git(root, "rev-parse", "HEAD").decode().strip()
            git(root, "checkout", "-q", "-b", "synthetic", base)
            git(root, "merge", "-q", "--no-ff", "-m", "PR checkout merge", "source")
            subprocess.check_call(["git", "clone", "-q", "--depth=1", "--branch",
                                   "synthetic", root.resolve().as_uri(), str(clone)])
            self.assertEqual(len(git(clone, "rev-list", "--parents", "-n", "1",
                                     "HEAD").split()), 1)
            gate.check_merge_source(repo_root=str(clone), expected_source=source)
            with self.assertRaises(ValueError):
                gate.check_merge_source(repo_root=str(clone), expected_source=base)
            with self.assertRaises(ValueError):
                gate.check_merge_source(repo_root=str(clone), expected_source="0"*40)
            with self.assertRaises(ValueError):
                gate.check_merge_source(repo_root=str(clone), expected_source=source,
                                        expected_base="f"*40)
            gate.check_merge_source(repo_root=str(clone), expected_source=source,
                                    expected_base=base)


class DecisionTests(unittest.TestCase):
    def test_unrelated_push_reuses_verified_acceptance(self):
        self.assertEqual(gate.decision("pull_request", "auto", {"run_id": 1})[0], False)

    def test_missing_or_failed_receipt_triggers_full(self):
        self.assertEqual(gate.decision("pull_request", "auto", None)[0], True)

    def test_explicit_manual_final_release(self):
        for event, mode in (("pull_request", "final"), ("pull_request", "manual"),
                            ("workflow_dispatch", "manual"), ("push", "auto"),
                            ("pull_request", "invalid")):
            self.assertTrue(gate.decision(event, mode, {"run_id": 1})[0])

    def test_raw_receipt_rejects_invalid_and_reduced_tier(self):
        good = raw_receipt()
        gate.require_raw_receipt(good)
        for k, bad in (("target_files", 100), ("extracted_chunks", 999999),
                       ("approved_roots", 1), ("raw_bytes_unchanged", False),
                       ("sqlite_and_fts_integrity", "not-proven"),
                       ("fixture_sha256", "bad"), ("platform", "linux")):
            with self.subTest(k=k):
                changed = dict(good, **{k: bad})
                with self.assertRaises(ValueError):
                    gate.require_raw_receipt(changed)

    def test_attestation_does_not_accept_forged_raw_receipt(self):
        raw = json.dumps(raw_receipt()).encode()
        p = gate.make_proof(receipt=raw, source="a"*40, run_id=2, attempt=1,
                            pr=128, repo="owner/repo", event="pull_request", effective="d"*64,
                            head_ref="qualification")
        self.assertTrue(p["qualified"])
        with self.assertRaises(ValueError):
            gate.make_proof(receipt=b"{}", source="a"*40, run_id=2, attempt=1,
                            pr=128, repo="owner/repo", event="pull_request", effective="d"*64)

    def test_candidate_sqlcipher_requirements_are_explicit_not_file_inferred(self):
        self.assertTrue(gate.native_contract.requires_sqlcipher("KaisAbiyyi/mind-place", 127))
        self.assertFalse(gate.native_contract.requires_sqlcipher("KaisAbiyyi/mind-place", 129))
        self.assertFalse(gate.native_contract.requires_sqlcipher("KaisAbiyyi/mind-place", 9001))


class AcceptancePrerequisiteTests(unittest.TestCase):
    def test_cached_100k_proof_reuse_requires_current_native_coordination_success(self):
        cached = {"run_id": 77, "artifact_id": 12}
        accepted, fresh = gate.select_acceptance_evidence(
            native_result="success", fresh=None, previous=cached,
            event="pull_request", mode="auto")
        self.assertIs(accepted, cached)
        self.assertFalse(fresh)
        for result in ("skipped", "failure", "cancelled", "neutral", "", None):
            with self.subTest(native_result=result), self.assertRaises(ValueError):
                gate.select_acceptance_evidence(
                    native_result=result, fresh=None, previous=cached,
                    event="pull_request", mode="auto")

    def test_fresh_100k_proof_also_requires_current_native_coordination_success(self):
        fresh = {"run_id": 88, "artifact_id": 22}
        accepted, is_fresh = gate.select_acceptance_evidence(
            native_result="success", fresh=fresh, previous=None,
            event="pull_request", mode="final")
        self.assertIs(accepted, fresh)
        self.assertTrue(is_fresh)
        with self.assertRaises(ValueError):
            gate.select_acceptance_evidence(
                native_result="skipped", fresh=fresh, previous=None,
                event="pull_request", mode="final")

    def test_workflow_keeps_cached_reuse_separate_from_native_coordination(self):
        root = Path(__file__).resolve().parents[2]
        source = (root / ".github/workflows/mp-10c-scale.yml").read_text()
        coordinator = source.split("  prerequisite-coordination:\n", 1)[1].split(
            "\n  document-scale:\n", 1)[0]
        coordinator_if = next(
            line.strip() for line in coordinator.splitlines()
            if line.strip().startswith("if:"))
        self.assertNotIn("run-heavy", coordinator_if)
        self.assertIn("workflow_dispatch", coordinator_if)
        self.assertIn("draft == false", coordinator_if)

        scale = source.split("  document-scale:\n", 1)[1].split(
            "\n  provenance:\n", 1)[0]
        self.assertIn("run-heavy == 'true'", scale)
        self.assertIn("needs.prerequisite-coordination.result == 'success'", scale)

        accepted = source.split("  qualification-accepted:\n", 1)[1]
        self.assertIn("prerequisite-coordination", accepted.splitlines()[1])
        self.assertIn("NATIVE_PREREQUISITE_RESULT: ${{ needs.prerequisite-coordination.result }}", accepted)
        self.assertIn('test "$PREREQUISITE_RESULT" = success', accepted)

        draft = source.split("  draft-qualification-pending:\n", 1)[1].split(
            "\n  qualification-accepted:\n", 1)[0]
        draft_if = next(line.strip() for line in draft.splitlines() if line.strip().startswith("if:"))
        self.assertIn("draft == true", draft_if)
        self.assertNotIn("run-heavy", draft_if)

    def test_manual_operator_sequence_requires_ci_dispatch_before_mp10c(self):
        root = Path(__file__).resolve().parents[2]
        ci = (root / ".github/workflows/ci.yml").read_text()
        dispatch = ci.split("  workflow_dispatch:\n", 1)[1].split("\npermissions:", 1)[0]
        for field in ("pr_number", "head_sha", "base_sha"):
            with self.subTest(field=field):
                self.assertIn(field + ":", dispatch)
                self.assertIn("required: true", dispatch)
        self.assertIn("refs/pull/{0}/merge", ci)
        self.assertIn("github.event_name == 'workflow_dispatch'", ci)

        docs = (root / "docs/implementation-plan/mp-ci-100k-01-qualification-gate.md").read_text()
        sequence = docs.split("### Mandatory operator sequence: native CI first, MP-10C second", 1)[1]
        self.assertLess(sequence.index("Dispatch the CI workflow"), sequence.index("Dispatch MP-10C"))
        self.assertIn("does not automatically dispatch CI", sequence)
        self.assertIn("native contract fingerprint", sequence)
        self.assertIn("explicitly PR-bound workflow_dispatch", docs)
        self.assertNotIn("workflow_dispatch runs are never accepted", docs)
        self.assertIn("manual", sequence)
        self.assertIn("final", sequence)


class DispatchTargetTests(unittest.TestCase):
    def setUp(self):
        self.pull = {"number": 127, "state": "open",
                     "head": {"sha": "a"*40, "ref": "feature/target", "repo": {"full_name": "owner/repo", "id": 1}},
                     "base": {"sha": "b"*40, "ref": "main", "repo": {"full_name": "owner/repo", "id": 1}}}

    def resolve(self, number="127", sha="a"*40, ref="refs/heads/feature/target"):
        with patch.object(gate, "api_json", return_value=self.pull):
            return gate.resolve_target(
                event="workflow_dispatch",
                payload={"inputs": {"pr_number": number, "head_sha": "a"*40, "base_sha": "b"*40}},
                repo="owner/repo", sha=sha, ref=ref, token="synthetic")

    def test_open_pr_exact_head_branch_is_required(self):
        self.assertEqual(self.resolve(), {
            "pr": 127, "head": "a"*40, "branch": "feature/target",
            "head_repo": "owner/repo", "head_repo_id": 1,
            "base_repo": "owner/repo", "base_repo_id": 1,
            "base_branch": "main", "base_sha": "b"*40,
        })
        for number in ("", "0", "-1", "127\n", 127):
            with self.subTest(number=number), self.assertRaises(ValueError):
                self.resolve(number=number)
        for sha, ref in (("b"*40, "refs/heads/feature/target"), ("a"*40, "refs/heads/main")):
            with self.subTest(sha=sha, ref=ref), self.assertRaises(ValueError):
                self.resolve(sha=sha, ref=ref)
        with patch.object(gate, "api_json", return_value=self.pull):
            with self.assertRaises(ValueError):
                gate.resolve_target(
                    event="workflow_dispatch",
                    payload={"inputs": {"pr_number": "127", "head_sha": "a"*40, "base_sha": "c"*40}},
                    repo="owner/repo", sha="a"*40, ref="refs/heads/feature/target", token="synthetic")

    def test_foreign_or_closed_pr_is_never_adopted(self):
        self.pull["head"]["repo"]["full_name"] = "foreign/repo"
        with self.assertRaises(ValueError):
            self.resolve()
        self.pull["head"]["repo"]["full_name"] = "owner/repo"
        self.pull["state"] = "closed"
        with self.assertRaises(ValueError):
            self.resolve()


class AttestationCliTests(unittest.TestCase):
    def test_real_cli_rejects_branch_only_dispatch_attestation(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            git(root, "init", "-q")
            git(root, "config", "user.name", "Synthetic CI fixture")
            git(root, "config", "user.email", "synthetic@example.invalid")
            (root / "Cargo.lock").write_text("synthetic dependencies")
            git(root, "add", ".")
            git(root, "commit", "-qm", "synthetic source")
            source = git(root, "rev-parse", "HEAD").decode().strip()
            fp = gate.fingerprint(str(root))
            receipt, event, output = root/"raw.json", root/"event.json", root/"proof.json"
            receipt.write_text(json.dumps(raw_receipt()))
            event.write_text(json.dumps({"inputs": {"pr_number": "127"}}))
            env = dict(os.environ, GITHUB_EVENT_NAME="workflow_dispatch",
                       GITHUB_REPOSITORY="owner/repo", GITHUB_SHA=source,
                       GITHUB_REF="refs/heads/qualification", GITHUB_EVENT_PATH=str(event),
                       GITHUB_RUN_ID="77", GITHUB_RUN_ATTEMPT="2",
                       SCALE_RUN_ATTEMPT="1", PR_NUMBER="127",
                       PR_HEAD_SHA=source, PR_HEAD_REF="qualification",
                       EXPECTED_FINGERPRINT=fp)
            command = [sys.executable, str(Path(gate.__file__).resolve()), "attest",
                       "--receipt", str(receipt), "--output", str(output)]
            rejected = subprocess.run(command, cwd=root, env=env, capture_output=True, text=True)
            self.assertNotEqual(rejected.returncode, 0)
            self.assertIn("PR_HEAD_REPOSITORY", rejected.stderr)
            self.assertFalse(output.exists())


class PriorProofTests(unittest.TestCase):
    def setUp(self):
        self.repo, self.pr = "owner/repo", 128
        self.tree = [{"type": "blob", "mode": "100644", "sha": "e"*40, "path": "Cargo.lock"}]
        self.scale_bytes = b"jobs:\n  document-smoke:\n    runs-on: windows-latest\n    steps:\n      - run: cargo test smoke\n  document-scale:\n    runs-on: windows-latest\n    steps:\n      - run: cargo test full\n  provenance:\n    runs-on: ubuntu-latest\n"
        self.scale_sha = hashlib.sha1(b"blob " + str(len(self.scale_bytes)).encode() + b"\0" + self.scale_bytes).hexdigest()
        self.tree.append({"type": "blob", "mode": "100644", "sha": self.scale_sha,
                          "path": ".github/workflows/mp-10c-scale.yml"})
        self.fp = gate.fingerprint_entries([(x["mode"].encode(), x["sha"].encode(), x["path"].encode()) for x in self.tree], scale_workflow=self.scale_bytes)
        self.raw = json.dumps(raw_receipt(), sort_keys=True).encode()
        self.run = {
            "id": 77, "run_attempt": 1, "head_sha": "a"*40, "event": "pull_request",
            "head_branch": "qualification",
            "path": ".github/workflows/mp-10c-scale.yml",
            "status": "completed", "conclusion": "success",
            "pull_requests": [{"number": 128}], "html_url": "https://github.com/owner/repo/actions/runs/77",
        }
        self.proof = gate.make_proof(
            receipt=self.raw, source="b"*40, head_sha="a"*40, run_id=77, attempt=1, pr=self.pr,
            repo=self.repo, event="pull_request", effective=self.fp, head_ref="qualification")
        stream = io.BytesIO()
        with zipfile.ZipFile(stream, "w") as z:
            z.writestr("mp10c-provenance.json", json.dumps(self.proof))
            z.writestr("mp10c-100000.json", self.raw)
        self.archive = stream.getvalue()
        self.artifact = {
            "id": 12, "name": gate.artifact_name(gate.ARTIFACT, 1), "expired": False,
            "archive_download_url": "https://api.github.com/artifact.zip",
            "digest": "sha256:" + hashlib.sha256(self.archive).hexdigest(),
            "workflow_run": {"id": 77}
        }
        raw_stream = io.BytesIO()
        with zipfile.ZipFile(raw_stream, "w") as z:
            z.writestr("mp10c-100000.json", self.raw)
        self.raw_archive = raw_stream.getvalue()
        self.raw_artifact = {
            "id": 13, "name": gate.artifact_name(gate.RAW_ARTIFACT, 1), "expired": False,
            "archive_download_url": "https://api.github.com/raw.zip",
            "digest": "sha256:" + hashlib.sha256(self.raw_archive).hexdigest(),
            "workflow_run": {"id": 77}
        }
        self.jobs = {"jobs": [
            {"name": gate.FULL_JOB, "conclusion": "success",
             "steps": [{"name": n, "conclusion": "success"} for n in (
                 "Qualify real authorized document scale", "Validate exact-tier receipt",
                 "Upload validated raw 100K receipt")]},
            {"name": gate.PROOF_JOB, "conclusion": "success",
             "steps": [{"name": n, "conclusion": "success"} for n in (
                 "Mint source-bound 100K provenance",
                 "Upload qualified receipt and provenance")]}
        ]}

    def repack(self):
        stream = io.BytesIO()
        with zipfile.ZipFile(stream, "w") as z:
            z.writestr("mp10c-provenance.json", json.dumps(self.proof))
            z.writestr("mp10c-100000.json", self.raw)
        self.archive = stream.getvalue()
        self.artifact["digest"] = "sha256:" + hashlib.sha256(self.archive).hexdigest()

    def check(self):
        def fake_api(url, _token):
            if "/jobs?" in url:
                return self.jobs
            if "/artifacts?" in url:
                return {"artifacts": [self.artifact, self.raw_artifact]}
            if "/runs?" in url:
                return {"workflow_runs": [self.run]}
            if "/git/commits/" in url:
                return {"sha": self.proof["checkout_sha"], "parents": [{"sha": "c"*40}, {"sha": "a"*40}],
                        "tree": {"sha": "f"*40}}
            if "/git/trees/" in url:
                return {"sha": "f"*40, "truncated": False, "tree": self.tree}
            if "/git/blobs/" in url:
                return {"sha": self.scale_sha, "encoding": "base64", "size": len(self.scale_bytes),
                        "content": __import__("base64").b64encode(self.scale_bytes).decode()}
            if "/pulls/128" in url:
                return {"number": self.pr, "head": {"sha": "a"*40, "ref": "qualification",
                                                       "repo": {"full_name": self.repo}},
                        "base": {"repo": {"full_name": self.repo}}}
            raise AssertionError(url)
        def fake_artifact(url, _token):
            if url == self.artifact["archive_download_url"]:
                return self.archive
            if url == self.raw_artifact["archive_download_url"]:
                return self.raw_archive
            raise AssertionError(url)
        with patch.object(gate, "api_json", side_effect=fake_api), patch.object(
            gate, "artifact_bytes", side_effect=fake_artifact):
            return gate.find_accepted(token="dummy", repo=self.repo, pr=self.pr, desired=self.fp)

    def test_valid_previous_receipt_and_successful_full_job(self):
        result = self.check()
        self.assertEqual(result["run_id"], 77)
        self.assertEqual(result["artifact_id"], 12)


    def test_legacy_authenticated_proof_can_revalidate_without_reindex(self):
        entries = [(x["mode"].encode(), x["sha"].encode(), x["path"].encode()) for x in self.tree]
        self.proof.pop("fingerprint_policy", None)
        self.proof["input_fingerprint"] = gate.fingerprint_entries(entries, legacy=True)
        self.repack()
        self.assertIsNotNone(self.check())  # Legacy proof bound to original v1 Git tree.
        self.scale_bytes = self.scale_bytes.replace(b"cargo test full", b"cargo test changed")
        self.scale_sha = hashlib.sha1(b"blob " + str(len(self.scale_bytes)).encode() + b"\0" + self.scale_bytes).hexdigest()
        self.tree[-1]["sha"] = self.scale_sha
        self.assertIsNone(self.check())  # Changed scale semantics refuse proof reuse.

    def test_stricter_parser_rejects_legacy_evidence_when_raw_invalid(self):
        entries = [(x["mode"].encode(), x["sha"].encode(), x["path"].encode()) for x in self.tree]
        self.proof.pop("fingerprint_policy", None)
        self.proof["input_fingerprint"] = gate.fingerprint_entries(entries, legacy=True)
        self.raw = b'{"schema": "invalid"}'
        self.proof["raw_receipt_sha256"] = hashlib.sha256(self.raw).hexdigest()
        self.repack()
        self.assertIsNone(self.check())

    def test_mixed_pr_history_fingerprint_not_latest_commit(self):
        self.assertIsNotNone(self.check())  # same inputs, independent of unrelated SHA
        changed = self.fp
        self.fp = "f" * 64
        self.assertIsNone(self.check())  # changed effective input since qualification
        self.fp = changed

    def test_reject_missing_corrupted_or_foreign_raw_artifact(self):
        self.raw_artifact["expired"] = True
        self.assertIsNone(self.check())
        self.raw_artifact["expired"] = False
        self.raw_artifact["digest"] = "sha256:" + "0"*64
        self.assertIsNone(self.check())
        self.raw_artifact["digest"] = "sha256:" + hashlib.sha256(self.raw_archive).hexdigest()
        self.raw_artifact["workflow_run"]["id"] = 999
        self.assertIsNone(self.check())

    def test_attestation_must_succeed_separately_from_indexing(self):
        self.jobs["jobs"][1]["conclusion"] = "failure"
        self.assertIsNone(self.check())
        self.jobs["jobs"][1]["conclusion"] = "success"
        self.jobs["jobs"][1]["steps"][0]["conclusion"] = "failure"
        self.assertIsNone(self.check())

    def test_reject_missing_expired_or_tampered_artifact(self):
        self.artifact["expired"] = True
        self.assertIsNone(self.check())
        self.artifact["expired"] = False
        self.artifact["digest"] = "sha256:" + "0" * 64
        self.assertIsNone(self.check())
        self.artifact["digest"] = "sha256:" + hashlib.sha256(self.archive).hexdigest()
        self.artifact["name"] = "mp10c-document-100"  # smoke is not full
        self.assertIsNone(self.check())

    def test_reject_failed_cancelled_skipped_100k_job(self):
        self.jobs["jobs"][0]["conclusion"] = "skipped"
        self.assertIsNone(self.check())
        self.jobs["jobs"][0]["conclusion"] = "success"
        self.jobs["jobs"][0]["steps"][0]["conclusion"] = "failure"
        self.assertIsNone(self.check())
        self.jobs["jobs"][0]["steps"][0]["conclusion"] = "success"
        self.run["conclusion"] = "cancelled"
        self.assertIsNone(self.check())

    def test_reject_other_pr_attempt_stale_source_or_invalid_raw(self):
        with self.assertRaises(ValueError):
            gate.validate_proof(self.proof, self.raw, run=dict(self.run, run_attempt=2),
                                artifact=self.artifact, pr=self.pr, repo=self.repo, desired=self.fp)
        with self.assertRaises(ValueError):
            gate.validate_proof(self.proof, self.raw, run=self.run, artifact=self.artifact,
                                pr=127, repo=self.repo, desired=self.fp)
        with self.assertRaises(ValueError):
            gate.validate_proof(self.proof, b"{}", run=self.run, artifact=self.artifact,
                                pr=self.pr, repo=self.repo, desired=self.fp)


    def test_pr_run_head_differs_from_synthetic_merge_checkout(self):
        # GitHub Actions pull_request runs record branch HEAD but checkout
        # executes the merge ref. Both must be preserved, not equated.
        self.proof["checkout_sha"] = "b" * 40
        self.proof["source_sha"] = "a" * 40
        gate.validate_proof(self.proof, self.raw, run=self.run, artifact=self.artifact,
                            pr=self.pr, repo=self.repo, desired=self.fp)
        self.proof["source_sha"] = "b" * 40
        with self.assertRaises(ValueError):
            gate.validate_proof(self.proof, self.raw, run=self.run, artifact=self.artifact,
                                pr=self.pr, repo=self.repo, desired=self.fp)

    def test_only_same_pr_can_reuse(self):
        self.run["pull_requests"] = [{"number": 127}]
        self.assertIsNone(self.check())

    def test_dispatch_proof_is_reusable_for_its_bound_pr(self):
        self.run["event"] = "workflow_dispatch"
        self.proof["event"] = "workflow_dispatch"
        self.proof["checkout_sha"] = self.proof["source_sha"]
        gate.validate_proof(self.proof, self.raw, run=self.run, artifact=self.artifact,
                            pr=self.pr, repo=self.repo, desired=self.fp)
        self.run["pull_requests"] = []
        self.repack()
        self.assertIsNotNone(self.check())

    def test_recompute_github_tree_rejects_a_forged_fingerprint(self):
        self.tree[0]["sha"] = "0"*40
        self.assertIsNone(self.check())

    def test_failed_attestation_retry_preserves_original_scale_attempt(self):
        self.run["run_attempt"] = 2
        self.proof["run_attempt"] = 2
        self.artifact["name"] = gate.artifact_name(gate.ARTIFACT, 2)
        self.repack()
        self.assertIsNotNone(self.check())
        self.raw_artifact["name"] = gate.artifact_name(gate.RAW_ARTIFACT, 2)
        self.assertIsNone(self.check())

    def test_event_mismatch_and_unbound_dispatch_are_rejected(self):
        for event, pr in (("workflow_dispatch", self.pr), ("pull_request", 0)):
            with self.subTest(event=event, pr=pr):
                with self.assertRaises(ValueError):
                    gate.validate_proof(dict(self.proof, event=event, pr_number=pr), self.raw,
                                        run=self.run, artifact=self.artifact,
                                        pr=pr, repo=self.repo, desired=self.fp)

    def test_failed_api_lookup_must_not_grant_proof(self):
        with patch.object(gate, "api_json", side_effect=OSError("unavailable")):
            with self.assertRaises(OSError):
                gate.find_accepted(token="dummy", repo=self.repo, pr=self.pr, desired=self.fp)


if __name__ == "__main__":
    unittest.main()

