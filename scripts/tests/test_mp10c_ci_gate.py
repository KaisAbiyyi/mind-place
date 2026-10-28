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
        self.write(".github/workflows/mp-10c-scale.yml", "jobs:\\n  document-smoke:\\n    runs-on: windows-latest\\n    steps:\\n      - run: cargo test smoke\\n  document-scale:\\n    runs-on: windows-latest\\n    steps:\\n      - run: cargo test full\\n  provenance:\\n    runs-on: ubuntu-latest\\n")
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

    def test_validator_only_and_coordination_only_edits_do_not_reindex(self):
        before = self.fp()
        self.write("scripts/mp10c_ci_gate.py", "new strict parser")
        self.write("scripts/tests/test_mp10c_ci_gate.py", "new tests")
        self.write("scripts/mp10c_prerequisite_wait.py", "bounded wait")
        text = self.root.joinpath(".github/workflows/mp-10c-scale.yml").read_text()
        self.write(".github/workflows/mp-10c-scale.yml", text.replace(
            "  document-scale:", "  coordinator:\\n    runs-on: ubuntu-latest\\n  document-scale:"))
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

    def test_full_run_waits_for_exact_source_cheap_ci(self):
        source = "a"*40
        passed = [{"id": i, "run_attempt": 1, "path": path, "head_sha": source,
                   "status": "completed", "conclusion": "success"}
                  for i, path in enumerate((".github/workflows/ci.yml",
                      ".github/workflows/mp9b-sqlcipher-qualification.yml"), start=1)]
        def job(name, steps, *, conclusion="success"):
            return {"name": name, "status": "completed", "conclusion": conclusion,
                    "steps": [{"name": s, "conclusion": "success"} for s in steps]}
        product = job("Verify Windows product", ("Test", "Test hosted tunnel desktop boundaries", "Build desktop app"))
        sql_steps = ("Encrypted state/index and FTS regression",
                     "Keyed MCP synthetic authorization and revocation",
                     "Vault and backup adversarial regression")
        sql_ubuntu = job("SQLCipher / ubuntu-24.04", sql_steps)
        sql_windows = job("SQLCipher / windows-2022", sql_steps)
        jobs = {1: [product], 2: [sql_ubuntu, sql_windows]}
        def fetch(run):
            return {"jobs": jobs[run["id"]], "total_count": len(jobs[run["id"]])}
        gate.require_prerequisites(passed, source, vault=True, fetch_jobs=fetch)
        for bad in ([], passed[:1], [dict(r, head_sha="b"*40) for r in passed],
                    [dict(r, conclusion="failure") for r in passed]):
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                gate.require_prerequisites(bad, source, vault=True, fetch_jobs=fetch)
        # A successful Draft workflow can contain a skipped native job. Never
        # treat its overall SUCCESS as proof for a multi-hour qualification.
        for replacement in (dict(product, conclusion="skipped"),
                            dict(product, steps=[{"name": "Build desktop app", "conclusion": "skipped"}]),
                            dict(product, name="Fast developer feedback")):
            with self.subTest(replacement=replacement), self.assertRaises(ValueError):
                jobs[1] = [replacement]
                gate.require_prerequisites(passed, source, vault=False, fetch_jobs=fetch)
        jobs[1] = [product]
        for replacement in (dict(sql_windows, conclusion="skipped"),
                            dict(sql_windows, steps=[]),
                            dict(sql_windows, name="SQLCipher / macos-15")):
            with self.subTest(replacement=replacement), self.assertRaises(ValueError):
                jobs[2] = [sql_ubuntu, replacement]
                gate.require_prerequisites(passed, source, vault=True, fetch_jobs=fetch)
        jobs[2] = [sql_ubuntu, sql_windows]
        with self.assertRaises(ValueError):
            gate.require_prerequisites(passed, source, vault=True,
                                       fetch_jobs=lambda r: {"jobs": jobs[r["id"]], "total_count": 101})


class DispatchTargetTests(unittest.TestCase):
    def setUp(self):
        self.pull = {"number": 127, "state": "open",
                     "head": {"sha": "a"*40, "ref": "feature/target", "repo": {"full_name": "owner/repo"}},
                     "base": {"repo": {"full_name": "owner/repo"}}}

    def resolve(self, number="127", sha="a"*40, ref="refs/heads/feature/target"):
        with patch.object(gate, "api_json", return_value=self.pull):
            return gate.resolve_target(event="workflow_dispatch", payload={"inputs": {"pr_number": number}},
                                        repo="owner/repo", sha=sha, ref=ref, token="synthetic")

    def test_open_pr_exact_head_branch_is_required(self):
        self.assertEqual(self.resolve(), {"pr": 127, "head": "a"*40, "branch": "feature/target"})
        for number in ("", "0", "-1", "127\n", 127):
            with self.subTest(number=number), self.assertRaises(ValueError):
                self.resolve(number=number)
        for sha, ref in (("b"*40, "refs/heads/feature/target"), ("a"*40, "refs/heads/main")):
            with self.subTest(sha=sha, ref=ref), self.assertRaises(ValueError):
                self.resolve(sha=sha, ref=ref)

    def test_foreign_or_closed_pr_is_never_adopted(self):
        self.pull["head"]["repo"]["full_name"] = "foreign/repo"
        with self.assertRaises(ValueError):
            self.resolve()
        self.pull["head"]["repo"]["full_name"] = "owner/repo"
        self.pull["state"] = "closed"
        with self.assertRaises(ValueError):
            self.resolve()


class AttestationCliTests(unittest.TestCase):
    def test_real_cli_binds_dispatch_and_preserves_scale_attempt_without_indexing(self):
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
            env = dict(os.environ, GITHUB_EVENT_NAME="workflow_dispatch", GITHUB_REPOSITORY="owner/repo",
                       GITHUB_SHA=source, GITHUB_REF="refs/heads/qualification", GITHUB_EVENT_PATH=str(event),
                       GITHUB_RUN_ID="77", GITHUB_RUN_ATTEMPT="2", SCALE_RUN_ATTEMPT="1", PR_NUMBER="127",
                       PR_HEAD_SHA=source, PR_HEAD_REF="qualification", EXPECTED_FINGERPRINT=fp)
            command = [sys.executable, str(Path(gate.__file__).resolve()), "attest", "--receipt", str(receipt), "--output", str(output)]
            good = subprocess.run(command, cwd=root, env=env, capture_output=True, text=True)
            self.assertEqual(good.returncode, 0, good.stderr)
            proof = json.loads(output.read_bytes())
            self.assertEqual((proof["event"], proof["pr_number"], proof["source_sha"], proof["checkout_sha"]),
                             ("workflow_dispatch", 127, source, source))
            self.assertEqual((proof["run_attempt"], proof["scale_run_attempt"]), (2, 1))
            for change in ({"PR_NUMBER": "0"}, {"PR_HEAD_SHA": "0"*40},
                           {"PR_HEAD_REF": "main"}, {"EXPECTED_FINGERPRINT": "0"*64}):
                with self.subTest(change=change):
                    output.unlink()
                    failed = subprocess.run(command, cwd=root, env=dict(env, **change), capture_output=True)
                    self.assertNotEqual(failed.returncode, 0)
                    self.assertFalse(output.exists())
                    # Restore a successful output for the next independent case.
                    self.assertEqual(subprocess.run(command, cwd=root, env=env, capture_output=True).returncode, 0)
class PriorProofTests(unittest.TestCase):
    def setUp(self):
        self.repo, self.pr = "owner/repo", 128
        self.tree = [{"type": "blob", "mode": "100644", "sha": "e"*40, "path": "Cargo.lock"}]
        self.scale_bytes = b"jobs:\\n  document-smoke:\\n    runs-on: windows-latest\\n    steps:\\n      - run: cargo test smoke\\n  document-scale:\\n    runs-on: windows-latest\\n    steps:\\n      - run: cargo test full\\n  provenance:\\n    runs-on: ubuntu-latest\\n"
        self.scale_sha = hashlib.sha1(b"blob " + str(len(self.scale_bytes)).encode() + b"\\0" + self.scale_bytes).hexdigest()
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
        self.scale_sha = hashlib.sha1(b"blob " + str(len(self.scale_bytes)).encode() + b"\\0" + self.scale_bytes).hexdigest()
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

class PrerequisiteCoordinationTests(unittest.TestCase):
    def test_pending_success_failure_skipped_and_missing(self):
        import mp10c_prerequisite_wait as coordinator
        sha = "a" * 40
        run = {"id": 2, "run_attempt": 1, "path": ".github/workflows/ci.yml",
               "head_sha": sha, "status": "in_progress", "conclusion": None}
        def jobs(_):
            return {"total_count": 1, "jobs": [{"name": "Verify Windows product",
                "status": "completed", "conclusion": "success", "steps": [
                {"name": n, "conclusion": "success"} for n in
                ("Test", "Test hosted tunnel desktop boundaries", "Build desktop app")]}]}
        self.assertEqual(coordinator.classify([run], sha, False, jobs)[0], "pending")
        success = dict(run, status="completed", conclusion="success")
        self.assertEqual(coordinator.classify([success], sha, False, jobs)[0], "ready")
        for conclusion in ("failure", "cancelled", "skipped"):
            self.assertEqual(coordinator.classify([dict(success, conclusion=conclusion)],
                             sha, False, jobs)[0], "failed")
        self.assertEqual(coordinator.classify([], sha, False, jobs)[0], "pending")
        self.assertEqual(coordinator.classify([dict(run, head_sha="b"*40)], sha,
                         False, jobs)[0], "pending")
        self.assertEqual(coordinator.classify([success], sha, False,
                         lambda _: {"total_count": 0, "jobs": []})[0], "failed")

    def test_waits_then_passes_without_premature_failure_or_loop(self):
        import mp10c_prerequisite_wait as coordinator
        result = iter([("pending", "running"), ("pending", "queued"), ("ready", "good")])
        calls = []
        coordinator.wait("owner/repo", "a"*40, 127, "test", False,
                         timeout=60, poll=10, clock=lambda: len(calls)*10,
                         sleep=lambda n: calls.append(n),
                         inspect=lambda *_: next(result))
        self.assertEqual(calls, [10, 10])
        for state in ("failed",):
            with self.assertRaises(ValueError):
                coordinator.wait("owner/repo", "a"*40, 127, "test", False,
                                 inspect=lambda *_: (state, "native failed"))
        with self.assertRaises(TimeoutError):
            coordinator.wait("owner/repo", "a"*40, 127, "test", False,
                             timeout=0, inspect=lambda *_: ("pending", "missing"))

    def test_superseded_sha_and_api_outage_are_never_accepted(self):
        import mp10c_prerequisite_wait as coordinator
        with patch.object(gate, "api_json", return_value={"state": "open",
                "head": {"sha": "b"*40, "repo": {"full_name": "owner/repo"}}}):
            self.assertEqual(coordinator.snapshot("owner/repo", "a"*40, 127,
                                                  "token", False)[0], "failed")
        with self.assertRaises(TimeoutError):
            coordinator.wait("owner/repo", "a"*40, 127, "token", False,
                             timeout=0,
                             inspect=lambda *_: (_ for _ in ()).throw(OSError("api offline")))
