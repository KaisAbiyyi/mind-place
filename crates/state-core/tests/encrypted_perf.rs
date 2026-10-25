#![cfg(feature = "sqlcipher-preview")]
//! Synthetic SQLCipher vs plaintext Rust StateStore measurement; NOT a native
//! owner-device SLA and not a replacement for independent security review.
use mind_place_security::SecurityGate;
use mind_place_state::{SourceDescriptor, SourceKey, StateStore};
use std::{fs, path::Path, time::Instant};

const COUNT: usize = 160;

fn exercise(store: &StateStore) -> (u128, u128) {
    let mut ids = Vec::new();
    let begin = Instant::now();
    for index in 0..COUNT {
        let key = format!("mp9b-synthetic-source-{index:04}");
        let identity = SourceDescriptor {
            provider: "chatgpt",
            source_namespace: "mp9b-synthetic",
            source_kind: "conversation",
            key: SourceKey::External(&key),
        };
        ids.push(store.register_source(&identity).unwrap());
    }
    let write_ms = begin.elapsed().as_millis();
    let begin = Instant::now();
    for id in ids {
        assert!(store.source(&id).unwrap().is_some());
    }
    (write_ms, begin.elapsed().as_millis())
}

fn disk_bytes(path: &Path) -> u64 {
    let mut total = 0;
    for suffix in ["", "-wal", "-shm"] {
        let candidate = path.with_file_name(format!(
            "{}{suffix}",
            path.file_name().unwrap().to_string_lossy()
        ));
        if let Ok(metadata) = fs::metadata(candidate) {
            total += metadata.len();
        }
    }
    total
}

#[test]
fn emit_synthetic_native_sqlcipher_overhead_receipt() {
    let temp = tempfile::tempdir().unwrap();
    let clear = temp.path().join("plain/state.sqlite3");
    let encrypted = temp.path().join("encrypted/state.sqlite3");
    let gate = SecurityGate::open(temp.path().join("security.json")).unwrap();
    gate.initialize(b"synthetic native benchmark credential")
        .unwrap();
    let lease = gate.require_unlocked().unwrap();
    let clear_store = StateStore::open(&clear).unwrap();
    let encrypted_store = StateStore::open_encrypted(&encrypted, [0x9e; 32], &gate, lease).unwrap();

    // Identical ordered writes and point-lookups on isolated synthetic DBs.
    let (clear_write_ms, clear_read_ms) = exercise(&clear_store);
    let (encrypted_write_ms, encrypted_read_ms) = exercise(&encrypted_store);
    let clear_bytes = disk_bytes(&clear);
    let encrypted_bytes = disk_bytes(&encrypted);
    assert!(clear_bytes > 0 && encrypted_bytes > 0);
    assert!(fs::read(&clear).unwrap().starts_with(b"SQLite format 3\0"));
    assert!(!fs::read(&encrypted)
        .unwrap()
        .starts_with(b"SQLite format 3\0"));
    println!(
        "MP9B_SYNTHETIC_PERF_RECEIPT={}",
        serde_json::json!({
            "records": COUNT,
            "host_os": std::env::consts::OS,
            "metric": "wall_ms",
            "clear_write": clear_write_ms,
            "encrypted_write": encrypted_write_ms,
            "clear_read": clear_read_ms,
            "encrypted_read": encrypted_read_ms,
            "clear_db_wal_shm_bytes": clear_bytes,
            "encrypted_db_wal_shm_bytes": encrypted_bytes,
            "qualification": "synthetic-only: no SLA or owner-installed Windows evidence"
        })
    );
}
