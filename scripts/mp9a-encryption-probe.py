#!/usr/bin/env python3
"""MP-9A isolated, synthetic encryption/backup cost probe.

NOT a product encryption implementation, SQLCipher benchmark, or security proof.
Requires Python 3.12+ and cryptography. Runs solely in a temporary directory.
Receipts contain aggregated measurements only; no keys, payloads or temp paths.
"""
from __future__ import annotations

import argparse
import hashlib
import hmac
import json
import os
from pathlib import Path
import platform
import sqlite3
import tempfile
from time import perf_counter

from cryptography.exceptions import InvalidTag
from cryptography.hazmat.primitives.ciphers.aead import AESGCM

AAD_BACKUP = b"mind-place/mp9a/backup-envelope/v1"
AAD_OBJECT = b"mind-place/mp9a/object/v1/"
AAD_ROW = b"mind-place/mp9a/row/v1/"
MARKER = b"needle_unique_9a"
MAX_ROWS = 10000
MAX_OBJECTS = 2000
MAX_PAYLOAD = 65536


def elapsed(task):
    start = perf_counter()
    result = task()
    return round((perf_counter() - start) * 1000, 3), result


def seal(cipher: AESGCM, message: bytes, aad: bytes) -> bytes:
    return (nonce := os.urandom(12)) + cipher.encrypt(nonce, message, aad)


def unseal(cipher: AESGCM, payload: bytes, aad: bytes) -> bytes:
    if len(payload) < 29:
        raise ValueError("invalid envelope")
    return cipher.decrypt(payload[:12], payload[12:], aad)


def assert_rejects(cipher: AESGCM, blob: bytes, aad: bytes) -> None:
    damaged = bytearray(blob)
    damaged[-1] ^= 1
    try:
        unseal(cipher, bytes(damaged), aad)
    except (InvalidTag, ValueError):
        pass
    else:
        raise AssertionError("modified authenticated payload accepted")
    try:
        unseal(AESGCM(os.urandom(32)), blob, aad)
    except (InvalidTag, ValueError):
        pass
    else:
        raise AssertionError("wrong key accepted")
    try:
        unseal(cipher, blob, aad + b"foreign-context")
    except (InvalidTag, ValueError):
        pass
    else:
        raise AssertionError("wrong context accepted")


def fixture(i: int, length: int, marker_id: int) -> bytes:
    # Synthetic/repeatable non-private plaintext. No user files read.
    seed = hashlib.sha256(f"MP-9A-synthetic-record-{i}".encode()).hexdigest().encode()
    prefix = (MARKER + b" " if i == marker_id else b"ordinary_fixture ")
    return (prefix + seed * ((length // len(seed)) + 2))[:length]


def db_open(path: Path) -> sqlite3.Connection:
    conn = sqlite3.connect(path)
    conn.execute("PRAGMA journal_mode=WAL")
    conn.execute("PRAGMA synchronous=FULL")
    conn.execute("CREATE TABLE docs(id INTEGER PRIMARY KEY, payload BLOB NOT NULL)")
    return conn


def db_close(conn: sqlite3.Connection) -> None:
    conn.execute("PRAGMA wal_checkpoint(TRUNCATE)")
    conn.close()


def db_size(path: Path) -> int:
    return sum(
        p.stat().st_size
        for p in (path, Path(str(path) + "-wal"), Path(str(path) + "-shm"))
        if p.exists()
    )


def probe(rows: int, objects: int, bytes_per_row: int, object_bytes: int, root: Path) -> dict:
    cipher = AESGCM(os.urandom(32))
    dedup_key = os.urandom(32)  # prototype only; never part of receipt
    target = rows // 2
    plain_path, enc_path = root / "plain.sqlite3", root / "app-aead.sqlite3"
    plain = db_open(plain_path)
    encrypted = db_open(enc_path)
    plain.execute("CREATE VIRTUAL TABLE docs_fts USING fts5(payload)")
    inputs = [fixture(i, bytes_per_row, target) for i in range(rows)]

    def load_plain():
        with plain:
            for i, record in enumerate(inputs):
                plain.execute("INSERT INTO docs(id,payload) VALUES(?,?)", (i + 1, record))
                plain.execute("INSERT INTO docs_fts(rowid,payload) VALUES(?,?)", (i + 1, record))

    def load_encrypted():
        with encrypted:
            for i, record in enumerate(inputs):
                aad = AAD_ROW + str(i + 1).encode()
                encrypted.execute(
                    "INSERT INTO docs(id,payload) VALUES(?,?)",
                    (i + 1, seal(cipher, record, aad)),
                )

    insert_plain_ms, _ = elapsed(load_plain)
    insert_encrypted_ms, _ = elapsed(load_encrypted)

    def search_plain():
        return plain.execute(
            "SELECT rowid FROM docs_fts WHERE docs_fts MATCH ?", (MARKER.decode(),)
        ).fetchall()

    def scan_encrypted():
        return [
            key for key, payload in encrypted.execute("SELECT id,payload FROM docs")
            if MARKER in unseal(cipher, payload, AAD_ROW + str(key).encode())
        ]

    search_plain_ms, hits_plain = elapsed(search_plain)
    search_encrypted_ms, hits_encrypted = elapsed(scan_encrypted)
    assert [x[0] for x in hits_plain] == [target + 1], hits_plain
    assert hits_encrypted == [target + 1], hits_encrypted
    assert_rejects(cipher, encrypted.execute("SELECT payload FROM docs WHERE id=?", (target + 1,)).fetchone()[0], AAD_ROW + str(target + 1).encode())
    assert encrypted.execute("PRAGMA integrity_check").fetchone()[0] == "ok"
    assert plain.execute("PRAGMA integrity_check").fetchone()[0] == "ok"
    assert plain.execute("INSERT INTO docs_fts(docs_fts) VALUES('integrity-check')") is not None

    # Backup through SQLite's consistent online backup API, never raw-copy a live WAL.
    def snapshot(source: sqlite3.Connection, target_file: Path):
        target_conn = sqlite3.connect(target_file)
        try:
            source.backup(target_conn)
        finally:
            target_conn.close()

    plain_snap = root / "plain-snapshot.sqlite3"
    enc_snap = root / "encrypted-record-snapshot.sqlite3"
    snap_plain_ms, _ = elapsed(lambda: snapshot(plain, plain_snap))
    snap_enc_ms, _ = elapsed(lambda: snapshot(encrypted, enc_snap))
    db_close(plain)
    db_close(encrypted)
    plain_bytes, enc_bytes = db_size(plain_path), db_size(enc_path)

    backup_payload = enc_snap.read_bytes()
    wrap_ms, portable = elapsed(lambda: seal(cipher, backup_payload, AAD_BACKUP))
    assert not portable.startswith(b"SQLite format 3")
    assert_rejects(cipher, portable, AAD_BACKUP)

    def recover():
        restored = root / "restored.sqlite3"
        restored.write_bytes(unseal(cipher, portable, AAD_BACKUP))
        conn = sqlite3.connect(restored)
        try:
            assert conn.execute("PRAGMA integrity_check").fetchone()[0] == "ok"
            count = conn.execute("SELECT COUNT(*) FROM docs").fetchone()[0]
            assert count == rows
            key, payload = conn.execute("SELECT id,payload FROM docs WHERE id=?", (target + 1,)).fetchone()
            assert MARKER in unseal(cipher, payload, AAD_ROW + str(key).encode())
        finally:
            conn.close()

    recover_ms, _ = elapsed(recover)

    # Object contents are synthetic and never copied outside this temp directory.
    object_inputs = [fixture(i, object_bytes, -1) for i in range(objects)]
    plain_dir, protected_dir = root / "plain-objects", root / "aead-objects"
    plain_dir.mkdir()
    protected_dir.mkdir()
    def plain_object_write():
        for i, blob in enumerate(object_inputs):
            (plain_dir / str(i)).write_bytes(blob)

    def encrypted_object_write():
        for i, blob in enumerate(object_inputs):
            # Keyed names avoid public raw-SHA guessing; same plaintext deduplicates
            # within one vault. Real store integration is reserved for MP-9B.
            name = hmac.digest(dedup_key, blob, "sha256").hex()
            path = protected_dir / name
            if not path.exists():
                path.write_bytes(seal(cipher, blob, AAD_OBJECT + bytes.fromhex(name)))

    plain_objects_ms, _ = elapsed(plain_object_write)
    encrypted_objects_ms, _ = elapsed(encrypted_object_write)
    def read_objects():
        for blob in protected_dir.iterdir():
            result = unseal(cipher, blob.read_bytes(), AAD_OBJECT + bytes.fromhex(blob.name))
            assert hmac.digest(dedup_key, result, "sha256").hex() == blob.name

    object_read_ms, _ = elapsed(read_objects)
    sample = next(protected_dir.iterdir())
    assert_rejects(cipher, sample.read_bytes(), AAD_OBJECT + bytes.fromhex(sample.name))
    plain_object_size = sum(f.stat().st_size for f in plain_dir.iterdir())
    enc_object_size = sum(f.stat().st_size for f in protected_dir.iterdir())
    assert len(list(protected_dir.iterdir())) == objects
    assert len(portable) == len(backup_payload) + 28

    return {
        "task": "MP-9A",
        "probe": "synthetic application-level AES-256-GCM vs SQLite plaintext FTS5",
        "scope": "microbenchmark ONLY; not production SQLite codec or SQLCipher",
        "platform": platform.system(),
        "python": platform.python_version(),
        "sqlite_version": sqlite3.sqlite_version,
        "cryptography": __import__("cryptography").__version__,
        "fixture": {"rows": rows, "row_bytes": bytes_per_row, "objects": objects, "object_bytes": object_bytes},
        "sqlite": {
            "plaintext_fts": {"write_ms": insert_plain_ms, "indexed_lookup_ms": search_plain_ms, "disk_bytes": plain_bytes, "consistent_snapshot_ms": snap_plain_ms},
            "encrypted_per_row_no_fts": {"write_ms": insert_encrypted_ms, "full_decrypt_scan_ms": search_encrypted_ms, "disk_bytes": enc_bytes, "consistent_snapshot_ms": snap_enc_ms},
        },
        "objects": {
            "plaintext_write_ms": plain_objects_ms, "aead_write_ms": encrypted_objects_ms,
            "aead_read_verify_ms": object_read_ms, "plaintext_bytes": plain_object_size,
            "aead_bytes": enc_object_size,
        },
        "backup": {
            "sqlite_snapshot_bytes": len(backup_payload), "portable_aead_bytes": len(portable),
            "encrypt_ms": wrap_ms, "decrypt_restore_integrity_ms": recover_ms,
        },
        "assertions": {
            "identical_target_hit": True, "sqlite_integrity": True,
            "wrong_key_rejected": True, "tamper_rejected": True, "aad_swap_rejected": True,
            "portable_restore_pass": True, "keyed_object_id": True,
        },
        "not_measured": ["SQLCipher full-database encryption", "real product query/index behavior",
            "Windows DPAPI key lifecycle", "cold-start on owner machine",
            "real-size backup and incremental backup", "per-user privacy/permission UX"],
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--rows", type=int, default=1000)
    parser.add_argument("--objects", type=int, default=128)
    parser.add_argument("--row-bytes", type=int, default=1024)
    parser.add_argument("--object-bytes", type=int, default=16384)
    parser.add_argument("--receipt", type=Path, required=True)
    args = parser.parse_args()
    if not 2 <= args.rows <= MAX_ROWS or not 1 <= args.objects <= MAX_OBJECTS:
        parser.error("rows/objects outside synthetic probe safety bounds")
    if not 64 <= args.row_bytes <= MAX_PAYLOAD or not 64 <= args.object_bytes <= MAX_PAYLOAD:
        parser.error("row/object payload outside synthetic probe safety bounds")
    with tempfile.TemporaryDirectory(prefix="mp9a-synthetic-") as scratch:
        result = probe(args.rows, args.objects, args.row_bytes, args.object_bytes, Path(scratch))
    args.receipt.parent.mkdir(parents=True, exist_ok=True)
    args.receipt.write_text(json.dumps(result, sort_keys=True, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(result, sort_keys=True))


if __name__ == "__main__":
    main()
