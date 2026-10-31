#!/usr/bin/env python3
"""Windows evidence verifier. Run the controller ONLY in an independent policy repo.

The target checkout is an object database, never imported or executed here.
Worker decisions are advisory; only the separately installed App check accepts.
"""
import argparse
import datetime as dt
import hashlib
import io
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
import zipfile

from windows_product_inputs import POLICY, fingerprint

CHECK = "Windows Product / trusted acceptance"
WORKFLOW = ".github/workflows/windows-product.yml"
SCHEMA = "windows-product-proof/v1"
CONTRACT_FILES = (WORKFLOW, "scripts/windows_product_gate.py", "scripts/windows_product_inputs.py",
                  ".github/windows-policy/native-steps.json")
VAULT_WORKFLOW = ".github/workflows/mp9b-sqlcipher-qualification.yml"
MAX_AGE = 14 * 86400
AUTHORITY_WAIT = 420  # One five-minute controller period plus scheduling slack.


class Rejected(ValueError):
    pass


def require(condition, reason):
    if not condition:
        raise Rejected(reason)


def sha(value):
    require(isinstance(value, str) and re.fullmatch(r"[0-9a-f]{40}", value), "invalid source SHA")
    return value


def utc():
    return dt.datetime.now(dt.timezone.utc)


class SafeRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        require(newurl.startswith("https://"), "insecure API redirect")
        redirected = super().redirect_request(req, fp, code, msg, headers, newurl)
        redirected.remove_header("Authorization")
        return redirected


class GitHub:
    def __init__(self, repo, token=None):
        require(re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repo), "invalid repository")
        self.repo = repo
        self.token = token or os.environ.get("GH_TOKEN", "")

    def request(self, path, data=None, method=None, binary=False):
        require(path.startswith("/") and not path.startswith("//"), "invalid API path")
        headers = {"Authorization": "Bearer " + self.token,
                   "Accept": "application/vnd.github+json", "Content-Type": "application/json",
                   "X-GitHub-Api-Version": "2022-11-28"}
        request = urllib.request.Request("https://api.github.com/repos/" + self.repo + path,
                                         data=None if data is None else json.dumps(data).encode(),
                                         headers=headers, method=method)
        with urllib.request.build_opener(SafeRedirect()).open(request, timeout=30) as response:
            payload = response.read(2 * 1024 * 1024 + 1)
        require(len(payload) <= 2 * 1024 * 1024, "oversized API response")
        return payload if binary else json.loads(payload)

    def iter_pages(self, path, key=None):
        expected_total = None
        seen = 0
        for page in range(1, 21):
            data = self.request(path + ("&" if "?" in path else "?") + f"per_page=100&page={page}")
            if key is None:
                items = data
            else:
                require(isinstance(data, dict), "malformed API listing")
                items = data.get(key)
                total = data.get("total_count")
                require(type(total) is int and total >= 0, "missing API listing count")
                require(expected_total is None or total == expected_total, "API listing changed during pagination")
                expected_total = total
            require(isinstance(items, list), "malformed API listing entries")
            seen += len(items)
            require(expected_total is None or seen <= expected_total, "API listing count mismatch")
            yield from items
            if len(items) < 100:
                require(expected_total is None or seen == expected_total, "incomplete API listing")
                return
        raise Rejected("API pagination limit; no partial evidence accepted")

    def pages(self, path, key=None):
        return list(self.iter_pages(path, key))


class Sources:
    def __init__(self, root, repo):
        self.root = str(root)
        self.repo = repo

    def git(self, *args):
        return subprocess.check_output(["git", *args], cwd=self.root, stderr=subprocess.DEVNULL)

    def fetch(self, commit):
        sha(commit)
        try:
            self.git("cat-file", "-e", commit + "^{commit}")
        except subprocess.CalledProcessError:
            self.git("fetch", "--quiet", "--no-tags", "https://github.com/" + self.repo + ".git", commit)

    def blob(self, commit, path):
        self.fetch(commit)
        return self.git("show", sha(commit) + ":" + path)

    def parents(self, commit):
        self.fetch(commit)
        return self.git("rev-list", "--parents", "-n", "1", sha(commit)).decode().split()[1:]

    def merge(self, source, base, head):
        require(self.parents(source) == [base, head], "stale merge parents")
        # A receipt cannot invent a different tree with plausible merge parents.
        # Reconstruct GitHub's clean merge without checking out or executing code.
        tree = self.git("merge-tree", "--write-tree", sha(base), sha(head)).decode().splitlines()[0]
        require(self.git("rev-parse", sha(source) + "^{tree}").decode().strip() == tree,
                "tested tree is not the reconstructed PR merge")

    def fingerprint(self, commit):
        self.fetch(commit)
        return fingerprint(commit, self.root)


def snapshot(api, number):
    pr = api.request(f"/pulls/{int(number)}")
    require(pr["state"] == "open" and not pr["draft"], "PR is not open and Ready")
    require(pr["head"]["repo"]["full_name"] == api.repo == pr["base"]["repo"]["full_name"], "foreign PR")
    return {"pr": int(number), "head": sha(pr["head"]["sha"]), "base": sha(pr["base"]["sha"]),
            "source": sha(pr["merge_commit_sha"]), "base_ref": pr["base"]["ref"]}


def validate_contract(sources, commit, policy_root):
    for path in CONTRACT_FILES:
        require(sources.git("ls-tree", sha(commit), "--", path).split()[:2] == [b"100644", b"blob"],
                "execution contract must be a regular file: " + path)
        require(sources.blob(commit, path) == (Path(policy_root) / path).read_bytes(),
                "execution contract changed: " + path)


def validate_source(sources, current, policy_root):
    sources.fetch(current["source"])
    sources.merge(current["source"], current["base"], current["head"])
    validate_contract(sources, current["source"], policy_root)
    validate_contract(sources, current["head"], policy_root)
    return sources.fingerprint(current["source"])


def enforcement(api, current, config):
    """Only the independent App can read protection; worker tokens need no admin access."""
    require(config.get("enabled") is True, "trust anchor is disabled")
    require(type(config.get("app_id")) is int and config["app_id"] > 0, "missing independent App ID")
    require(config.get("repository") == api.repo and config.get("policy") == POLICY, "wrong authority policy")
    # URL-encode branch names (including stacked branches with slashes).
    from urllib.parse import quote
    protection = api.request("/branches/" + quote(current["base_ref"], safe="") + "/protection")
    checks = protection.get("required_status_checks") or {}
    require(checks.get("strict") is True and protection.get("enforce_admins", {}).get("enabled") is True,
            "strict protection including administrators is required")
    require(any(c.get("context") == CHECK and c.get("app_id") == config["app_id"]
                for c in checks.get("checks", [])), "required check must be bound to the independent App")


def unpack_proof(payload):
    with zipfile.ZipFile(io.BytesIO(payload)) as archive:
        require(archive.namelist() == ["proof.json"], "invalid proof archive members")
        entry = archive.getinfo("proof.json")
        require(entry.file_size <= 16384 and entry.compress_size <= 16384, "oversized proof")
        return json.loads(archive.read(entry))


def verify_run(api, sources, run_id, current, identity, policy_root, now=None):
    """All assertions use GitHub-owned data + independently pinned execution bytes."""
    now = now or utc()
    run = api.request(f"/actions/runs/{int(run_id)}")
    require(run["id"] == int(run_id) and run["repository"]["full_name"] == api.repo
            and run["head_repository"]["full_name"] == api.repo, "foreign run")
    require(run["path"] == WORKFLOW and run["event"] in {"pull_request", "workflow_dispatch"}, "wrong execution origin")
    require(run["status"] == "completed" and run["conclusion"] == "success", "native run incomplete or failed")
    require(0 <= (now - dt.datetime.fromisoformat(run["updated_at"].replace("Z", "+00:00"))).total_seconds() <= MAX_AGE,
            "expired or future run")
    attempt = run["run_attempt"]
    require(type(attempt) is int and attempt > 0, "invalid run attempt")
    artifacts = api.pages(f"/actions/runs/{run_id}/artifacts", "artifacts")
    artifacts = [a for a in artifacts if a["name"] == f"windows-product-proof-{run_id}-{attempt}"]
    require(len(artifacts) == 1 and not artifacts[0]["expired"], "missing or ambiguous native proof")
    artifact = artifacts[0]
    origin = artifact.get("workflow_run", {})
    require(origin.get("id") == run_id and origin.get("head_sha") == run["head_sha"]
            and origin.get("repository_id") == run["repository"]["id"]
            and origin.get("head_repository_id") == run["repository"]["id"], "foreign artifact provenance")
    digest = artifact.get("digest")
    require(isinstance(digest, str) and re.fullmatch(r"sha256:[0-9a-f]{64}", digest), "missing or malformed artifact digest")
    payload = api.request(f"/actions/artifacts/{artifact['id']}/zip", binary=True)
    require(hashlib.sha256(payload).hexdigest() == digest.removeprefix("sha256:"), "artifact archive digest mismatch")
    proof = unpack_proof(payload)
    require(proof.get("schema") == SCHEMA and proof.get("policy") == POLICY, "unknown proof policy")
    require(proof.get("repository") == api.repo and proof.get("pr") == current["pr"], "foreign proof")
    require(proof.get("run_id") == run_id and proof.get("attempt") == attempt, "proof attempt mismatch")
    for field in ("head", "base", "source"):
        sha(proof.get(field))
    sources.merge(proof["source"], proof["base"], proof["head"])
    validate_contract(sources, proof["source"], policy_root)
    validate_contract(sources, run["head_sha"], policy_root)
    if run["event"] == "pull_request":
        require(run["head_sha"] == proof["head"], "run head mismatch")
        require(any(p["number"] == current["pr"] and p["head"]["sha"] == proof["head"]
                    and p["base"]["sha"] == proof["base"] for p in run["pull_requests"]), "foreign or stale PR run")
    else:
        # Dispatch must use the PR head branch, never a different workflow ref.
        require(run["head_sha"] == proof["head"], "dispatch workflow/source mismatch")
        require(proof["source"] == current["source"] and proof["base"] == current["base"],
                "manual proof is not the current merge source")
    require(proof.get("fingerprint") == identity == sources.fingerprint(proof["source"]), "Windows inputs changed")
    jobs = api.pages(f"/actions/runs/{run_id}/attempts/{attempt}/jobs", "jobs")
    jobs = [job for job in jobs if job["name"] == "Verify Windows product"]
    require(len(jobs) == 1, "native job missing or ambiguous")
    job = jobs[0]
    require(type(job.get("run_id")) is int and job["run_id"] == run_id
            and type(job.get("run_attempt")) is int and job["run_attempt"] == attempt
            and job.get("status") == "completed" and job.get("conclusion") == "success",
            "native job did not complete successfully on the exact run attempt")
    require("windows-latest" in jobs[0]["labels"], "wrong native runner")
    contract = json.loads((Path(policy_root) / ".github/windows-policy/native-steps.json").read_text())
    require(contract.get("schema") == "windows-product-native-steps/v2", "unknown native step policy")
    required = list(contract["common"])
    if sources.git("ls-tree", proof["source"], "--", VAULT_WORKFLOW):
        required += contract["vault"]
    steps = jobs[0]["steps"]
    for name in required:
        matching = [s for s in steps if s["name"] == name]
        require(len(matching) == 1 and matching[0]["status"] == "completed"
                and matching[0]["conclusion"] == "success", "native step did not execute: " + name)
    return {"run_id": run_id, "attempt": attempt, "artifact_id": artifact["id"], "artifact_digest": digest, "source": proof["source"],
            "event": run["event"], "head": proof["head"]}


def find_proof(api, sources, current, identity, policy_root, fresh=False, fresh_after=None):
    runs = api.iter_pages("/actions/workflows/windows-product.yml/runs?status=success", "workflow_runs")
    for run in runs:
        if fresh and (run["event"] != "workflow_dispatch" or run["head_sha"] != current["head"]):
            continue
        if fresh_after and run.get("created_at", "") < fresh_after:
            continue
        try:
            evidence = verify_run(api, sources, run["id"], current, identity, policy_root)
            if fresh and evidence["source"] != current["source"]:
                continue
            return evidence
        except (Rejected, KeyError, TypeError, zipfile.BadZipFile, json.JSONDecodeError):
            continue
    return None


def lease_state(api, current, identity, app_id):
    """Return pending/authorized/native without trusting any PR-generated check."""
    if not app_id:
        return "native"
    checks = api.pages(f"/commits/{current['head']}/check-runs?check_name=Windows%20Product%20%2F%20trusted%20acceptance", "check_runs")
    for check in checks:
        if check["app"]["id"] != int(app_id) or check["name"] != CHECK or check["head_sha"] != current["head"]:
            continue
        if check["status"] != "in_progress" and check.get("conclusion") != "success":
            return "native"
        try:
            lease = json.loads(check["output"]["text"])
            require(lease["schema"] == "windows-product-acceptance/v1" and lease["policy"] == POLICY, "invalid lease")
            require(lease["current"] == current and lease["fingerprint"] == identity, "stale lease")
            require(lease["anchor_enforced"] is True and 0 <= lease["expires"] - utc().timestamp() <= 600, "expired lease")
            return "native" if lease.get("require_fresh") else "authorized"
        except (Rejected, ValueError, KeyError, TypeError):
            continue
    return "pending"


def authorized_reuse(api, sources, current, identity, app_id, policy_root):
    if lease_state(api, current, identity, app_id) == "authorized":
        return find_proof(api, sources, current, identity, policy_root)
    return None


def coordinate(api, sources, current, app_id, policy_root, fresh=False,
               timeout=AUTHORITY_WAIT, poll=15, clock=None, sleep=None):
    """Cheap Ubuntu coordination only; never wait while occupying a Windows runner.

    Wait only when verified matching native evidence exists but current-head
    authority has not arrived. Missing authority at deadline schedules native;
    a superseded source aborts. The native slot still performs its own recheck.
    """
    require(0 <= timeout <= AUTHORITY_WAIT and 0 < poll <= 30, "invalid authority wait budget")
    clock, sleep = clock or time.monotonic, sleep or time.sleep
    deadline = clock() + timeout
    identity = validate_source(sources, current, policy_root)
    fallback = ("native", identity, None)
    if fresh or not app_id:
        return fallback
    try:
        candidate = find_proof(api, sources, current, identity, policy_root)
    except (OSError, Rejected, KeyError, TypeError, subprocess.CalledProcessError):
        return fallback
    if candidate is None:
        return fallback
    while True:
        try:
            require(snapshot(api, current["pr"]) == current, "source superseded during authority wait")
            state = lease_state(api, current, identity, app_id)
        except OSError:
            state = "pending"  # Bounded retry; lookup failure cannot authorize reuse.
        if state == "native":
            return fallback
        if state == "authorized":
            try:
                proof = verify_run(api, sources, candidate["run_id"], current, identity, policy_root)
            except (OSError, ValueError, KeyError, TypeError, zipfile.BadZipFile, subprocess.CalledProcessError):
                return fallback
            try:
                require(snapshot(api, current["pr"]) == current, "source superseded during authority wait")
            except OSError:
                return fallback
            return "reuse", identity, proof
        remaining = deadline - clock()
        if remaining <= 0:
            print("Windows authority wait expired; native qualification required", flush=True)
            return fallback
        print("Windows authority pending for exact current source; waiting on Ubuntu", flush=True)
        sleep(min(poll, remaining))


def decision(api, sources, current, app_id, policy_root, fresh=False):
    identity = validate_source(sources, current, policy_root)
    if fresh:
        return "native", identity, None
    try:
        proof = authorized_reuse(api, sources, current, identity, app_id, policy_root)
    except (urllib.error.URLError, Rejected, KeyError, ValueError, subprocess.CalledProcessError):
        proof = None  # API/authority uncertainty always schedules real native work.
    return ("reuse" if proof else "native"), identity, proof


def controller(api, sources, policy_root, config, fresh=False):
    for pr in api.pages("/pulls?state=open"):
        if pr["draft"]:
            continue
        check_id = None
        try:
            current = snapshot(api, pr["number"])
            existing = api.pages(f"/commits/{current['head']}/check-runs?check_name=Windows%20Product%20%2F%20trusted%20acceptance", "check_runs")
            owned = [c for c in existing if c["name"] == CHECK and c["app"]["id"] == config["app_id"] and "id" in c]
            check_id = owned[0]["id"] if owned else None
            enforcement(api, current, config)
            identity = validate_source(sources, current, policy_root)
            # Native commands execute PR build/test code. An unreviewed input
            # cannot promote its own successful run into independent authority,
            # even if it rewrites/restores the workspace or forges a receipt.
            require(identity in config.get("approved_fingerprints", []),
                    "native inputs need independent policy review and fingerprint approval")
            require_fresh = fresh or config.get("require_fresh", False)
            fresh_after = config.get("fresh_after") if require_fresh else None
            require(not require_fresh or (isinstance(fresh_after, str) and
                    re.fullmatch(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z", fresh_after)),
                    "fresh policy requires a pinned UTC fresh_after cutoff")
            lease = {"schema": "windows-product-acceptance/v1", "policy": POLICY, "current": current,
                     "fingerprint": identity, "anchor_enforced": True, "expires": int(utc().timestamp()) + 600,
                     "require_fresh": require_fresh, "fresh_after": fresh_after}
            # Token must belong to the App whose ID is pinned by required checks.
            payload = {"name": CHECK, "status": "in_progress",
                                 "output": {"title": "Native evidence pending", "summary": "Independent policy verification",
                                            "text": json.dumps(lease)}}
            if check_id is None:
                payload["head_sha"] = current["head"]
            check = api.request("/check-runs" + (f"/{check_id}" if check_id else ""), payload,
                                "PATCH" if check_id else "POST")
            check_id = check["id"]
            require(check["app"]["id"] == config["app_id"], "wrong publishing App")
            evidence = find_proof(api, sources, current, identity, policy_root, require_fresh, fresh_after)
            require(snapshot(api, current["pr"]) == current, "source superseded during verification")
            enforcement(api, current, config)
            if evidence:
                lease["evidence"] = evidence
                lease["outcome"] = "NATIVE_PASS" if evidence["source"] == current["source"] else "VERIFIED_REUSE"
                api.request(f"/check-runs/{check_id}", {"status": "completed", "conclusion": "success",
                            "output": {"title": lease["outcome"], "summary": "Verified actual native execution and immutable inputs",
                                       "text": json.dumps(lease)}}, "PATCH")
            print(json.dumps({"pr": current["pr"], "check_id": check_id, "evidence": evidence}))
        except (Rejected, urllib.error.URLError, KeyError, ValueError, TypeError, subprocess.CalledProcessError) as error:
            if check_id is not None:
                api.request(f"/check-runs/{check_id}", {"status": "completed", "conclusion": "failure",
                            "output": {"title": "Windows acceptance rejected", "summary": str(error)}}, "PATCH")
            print(json.dumps({"pr": pr["number"], "rejected": str(error)}))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["plan", "coordinate", "slot", "proof", "controller"])
    parser.add_argument("--policy-root", default=".")
    parser.add_argument("--config")
    parser.add_argument("--fresh", action="store_true")
    parser.add_argument("--wait-seconds", type=int, default=AUTHORITY_WAIT)
    args = parser.parse_args()
    if args.command == "controller":
        config = json.loads(Path(args.config).read_text())
        api = GitHub(config["repository"])
        with tempfile.TemporaryDirectory() as directory:
            subprocess.run(["git", "init", "--bare", "-q", directory], check=True)
            controller(api, Sources(directory, api.repo), args.policy_root, config, args.fresh)
        return
    api = GitHub(os.environ["GITHUB_REPOSITORY"])
    sources = Sources(".", api.repo)
    event = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text())
    number = event.get("number") or event.get("inputs", {}).get("pr_number")
    fresh = os.environ["GITHUB_EVENT_NAME"] != "pull_request"
    if not number:
        # Main pushes keep all native checks; there is no same-PR proof reuse.
        require(args.command != "proof", "main builds do not mint PR evidence")
        current = {"pr": 0, "head": sha(os.environ["GITHUB_SHA"]), "base": "", "base_ref": "", "source": sha(os.environ["GITHUB_SHA"])}
        mode, identity = "native", sources.fingerprint(current["source"])
    else:
        if args.command == "proof" and not fresh:
            # Finish an already-running, valid older qualification. It may be
            # reused only after the controller compares current effective inputs.
            current = {"pr": int(number), "head": sha(event["pull_request"]["head"]["sha"]),
                       "base": sha(event["pull_request"]["base"]["sha"]), "source": sha(os.environ["GITHUB_SHA"]),
                       "base_ref": event["pull_request"]["base"]["ref"]}
        else:
            current = snapshot(api, int(number))
            if os.environ["GITHUB_EVENT_NAME"] == "workflow_dispatch":
                inputs = event.get("inputs", {})
                require(sha(inputs.get("head_sha")) == current["head"]
                        and sha(inputs.get("base_sha")) == current["base"],
                        "dispatch PR head/base does not match the live frozen candidate")
        if not fresh:
            require(event["pull_request"]["head"]["sha"] == current["head"]
                    and event["pull_request"]["base"]["sha"] == current["base"]
                    and os.environ["GITHUB_SHA"] == current["source"], "superseded PR event")
        else:
            require(os.environ["GITHUB_SHA"] == current["head"], "dispatch on current PR head branch")
        expected = os.environ.get("EXPECTED_SOURCE")
        require(not expected or expected == current["source"], "superseded queued source")
        # In workers the contract is advisory. The independent controller uses its
        # own pinned files, so editing this verifier cannot self-qualify a PR.
        if args.command == "coordinate":
            mode, identity, _ = coordinate(api, sources, current, os.environ.get("WINDOWS_PRODUCT_APP_ID"),
                                           args.policy_root, fresh, timeout=args.wait_seconds)
        else:
            mode, identity, _ = decision(api, sources, current, os.environ.get("WINDOWS_PRODUCT_APP_ID"), args.policy_root, fresh)
    if args.command == "proof":
        require(sources.git("rev-parse", "HEAD").decode().strip() == current["source"], "native checkout moved")
        sources.git("diff", "--exit-code", current["source"], "--")
        proof = {"schema": SCHEMA, "policy": POLICY, "repository": api.repo, **current, "fingerprint": identity,
                 "run_id": int(os.environ["GITHUB_RUN_ID"]), "attempt": int(os.environ["GITHUB_RUN_ATTEMPT"])}
        Path("qualification").mkdir(exist_ok=True)
        Path("qualification/proof.json").write_text(json.dumps(proof), encoding="utf-8")
    else:
        values = {**current, "mode": mode, "fingerprint": identity, "native": str(mode == "native").lower()}
        with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
            for key, value in values.items():
                require("\n" not in str(value) and "\r" not in str(value), "invalid output")
                output.write(f"{key}={value}\n")
        print(json.dumps(values))


if __name__ == "__main__":
    main()
