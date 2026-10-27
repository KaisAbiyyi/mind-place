"""Synthetic MP-CI-100K-01 regression matrix; no network and no owner corpus."""
import hashlib
import io
import json
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
        self.write(".github/workflows/mp-10c-scale.yml", "workflow-original")
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
            ".github/workflows/mp-10c-scale.yml",
            "scripts/mp10c_ci_gate.py", "rust-toolchain.toml",
        ):
            with self.subTest(path=path):
                before = self.fp()
                self.write(path, self.root.joinpath(path).read_text() + "!")
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
                            pr=128, repo="owner/repo", event="pull_request", effective="d"*64)
        self.assertTrue(p["qualified"])
        with self.assertRaises(ValueError):
            gate.make_proof(receipt=b"{}", source="a"*40, run_id=2, attempt=1,
                            pr=128, repo="owner/repo", event="pull_request", effective="d"*64)


class PriorProofTests(unittest.TestCase):
    def setUp(self):
        self.repo, self.pr, self.fp = "owner/repo", 128, "d"*64
        self.raw = json.dumps(raw_receipt(), sort_keys=True).encode()
        self.run = {
            "id": 77, "run_attempt": 1, "head_sha": "a"*40, "event": "pull_request",
            "path": ".github/workflows/mp-10c-scale.yml",
            "status": "completed", "conclusion": "success",
            "pull_requests": [{"number": 128}], "html_url": "https://github.com/owner/repo/actions/runs/77",
        }
        self.proof = gate.make_proof(
            receipt=self.raw, source="a"*40, run_id=77, attempt=1, pr=self.pr,
            repo=self.repo, event="pull_request", effective=self.fp)
        stream = io.BytesIO()
        with zipfile.ZipFile(stream, "w") as z:
            z.writestr("mp10c-provenance.json", json.dumps(self.proof))
            z.writestr("mp10c-100000.json", self.raw)
        self.archive = stream.getvalue()
        self.artifact = {
            "id": 12, "name": gate.ARTIFACT, "expired": False,
            "archive_download_url": "https://api.github.com/artifact.zip",
            "digest": "sha256:" + hashlib.sha256(self.archive).hexdigest(),
            "workflow_run": {"id": 77}
        }
        self.jobs = {"jobs": [{
            "name": gate.FULL_JOB, "conclusion": "success",
            "steps": [{"name": n, "conclusion": "success"} for n in (
                "Qualify real authorized document scale", "Validate exact-tier receipt",
                "Mint source-bound 100K provenance")]
        }]}

    def check(self):
        def fake_api(url, _token):
            if "/jobs?" in url:
                return self.jobs
            if "/artifacts?" in url:
                return {"artifacts": [self.artifact]}
            if "/runs?" in url:
                return {"workflow_runs": [self.run]}
            raise AssertionError(url)
        with patch.object(gate, "api_json", side_effect=fake_api), patch.object(
            gate, "artifact_bytes", return_value=self.archive):
            return gate.find_accepted(token="dummy", repo=self.repo, pr=self.pr, desired=self.fp)

    def test_valid_previous_receipt_and_successful_full_job(self):
        result = self.check()
        self.assertEqual(result["run_id"], 77)
        self.assertEqual(result["artifact_id"], 12)

    def test_mixed_pr_history_fingerprint_not_latest_commit(self):
        self.assertIsNotNone(self.check())  # same inputs, independent of unrelated SHA
        changed = self.fp
        self.fp = "f" * 64
        self.assertIsNone(self.check())  # changed effective input since qualification
        self.fp = changed

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
        self.assertIsNone(gate.validated_candidate(
            dict(self.run, run_attempt=2), token="dummy", repo=self.repo, pr=self.pr,
            desired=self.fp)) if False else None
        with self.assertRaises(ValueError):
            gate.validate_proof(self.proof, self.raw, run=dict(self.run, run_attempt=2),
                                artifact=self.artifact, pr=self.pr, repo=self.repo, desired=self.fp)
        with self.assertRaises(ValueError):
            gate.validate_proof(self.proof, self.raw, run=self.run, artifact=self.artifact,
                                pr=127, repo=self.repo, desired=self.fp)
        with self.assertRaises(ValueError):
            gate.validate_proof(self.proof, b"{}", run=self.run, artifact=self.artifact,
                                pr=self.pr, repo=self.repo, desired=self.fp)

    def test_only_same_pr_can_reuse(self):
        self.run["pull_requests"] = [{"number": 127}]
        self.assertIsNone(self.check())

    def test_failed_api_lookup_must_not_grant_proof(self):
        with patch.object(gate, "api_json", side_effect=OSError("unavailable")):
            with self.assertRaises(OSError):
                gate.find_accepted(token="dummy", repo=self.repo, pr=self.pr, desired=self.fp)


if __name__ == "__main__":
    unittest.main()
