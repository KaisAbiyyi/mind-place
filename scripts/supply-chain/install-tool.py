"""Install only a checksum-verified regular binary from a pinned upstream archive."""
import argparse
import hashlib
import io
import json
from pathlib import Path, PurePosixPath
import tarfile
import urllib.request

MAX_ARCHIVE = 64 * 1024 * 1024
MAX_BINARY = 128 * 1024 * 1024


def verified_binary(payload, pin):
    if len(payload) > MAX_ARCHIVE or hashlib.sha256(payload).hexdigest() != pin["sha256"]:
        raise ValueError("Tool archive checksum/size mismatch")
    with tarfile.open(fileobj=io.BytesIO(payload), mode="r:*") as archive:
        matches = [m for m in archive if PurePosixPath(m.name).name == pin["binary"]]
        if len(matches) != 1:
            raise ValueError("Expected exactly one tool binary")
        member = matches[0]
        if (not member.isfile() or member.size > MAX_BINARY or member.size <= 0
                or PurePosixPath(member.name).is_absolute() or ".." in PurePosixPath(member.name).parts):
            raise ValueError("Unsafe tool binary member")
        return archive.extractfile(member).read(MAX_BINARY + 1)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("tool", choices=["cargo-deny", "cargo-cyclonedx", "scorecard"])
    parser.add_argument("--destination", required=True, type=Path)
    args = parser.parse_args()
    pin = json.loads(Path(__file__).with_name("tools.json").read_text())[args.tool]
    request = urllib.request.Request(pin["url"], headers={"User-Agent": "MindPlace-supply-chain"})
    with urllib.request.urlopen(request, timeout=120) as response:
        payload = response.read(MAX_ARCHIVE + 1)
    binary = verified_binary(payload, pin)
    args.destination.mkdir(parents=True, exist_ok=True)
    target = args.destination / pin["binary"]
    # Exclusive creation avoids replacing a pre-existing/symlinked executable.
    with target.open("xb") as output:
        output.write(binary)
    target.chmod(0o755)
    print(f"Installed {args.tool} {pin['version']}; verified sha256:{pin['sha256']}")


if __name__ == "__main__":
    main()
