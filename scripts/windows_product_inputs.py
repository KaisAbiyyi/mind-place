#!/usr/bin/env python3
"""Experimental Windows product input identity; this is not an acceptance gate.

Hash immutable Git tree entries, including the workflow and this policy. Only
Markdown files under docs/ are excluded. Callers must obtain the *tested merge*
commit independently; a PR head alone does not describe a merge build.
"""
import argparse
import hashlib
import re
import subprocess

OID = re.compile(rb"[0-9a-f]{40}\Z")


def fingerprint(commit, root="."):
    if not re.fullmatch(r"[0-9a-f]{40}", commit):
        raise ValueError("expected an immutable full commit SHA")
    actual = subprocess.check_output(
        ["git", "rev-parse", "--verify", f"{commit}^{{commit}}"], cwd=root
    ).strip().decode("ascii")
    if actual != commit:
        raise ValueError("commit identity mismatch")
    raw = subprocess.check_output(
        ["git", "ls-tree", "-r", "-z", "--full-tree", commit], cwd=root
    )
    digest = hashlib.sha256(b"windows-product-inputs/experimental-v1\0")
    count = 0
    previous = None
    entries = []
    for record in raw.split(b"\0"):
        if not record:
            continue
        metadata, path = record.split(b"\t", 1)
        mode, kind, oid = metadata.split(b" ")
        if not path or not OID.fullmatch(oid):
            raise ValueError("invalid tree entry")
        if (mode, kind) not in {(b"100644", b"blob"), (b"100755", b"blob"),
                                (b"120000", b"blob"), (b"160000", b"commit")}:
            raise ValueError("unsupported tree entry")
        entries.append((path, mode, oid))
    for path, mode, oid in sorted(entries):
        if path == previous:
            raise ValueError("duplicate tree entry")
        previous = path
        # Only ordinary Markdown documents are outside the native workload.
        # Symlinks and executable files remain bound.
        if path.startswith(b"docs/") and path.endswith(b".md") and mode == b"100644":
            continue
        digest.update(mode + b"\0" + oid + b"\0" + path + b"\0")
        count += 1
    if not count:
        raise ValueError("empty input tree")
    return digest.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("commit", help="full immutable SHA of the tested commit")
    args = parser.parse_args()
    print(fingerprint(args.commit))


if __name__ == "__main__":
    main()
