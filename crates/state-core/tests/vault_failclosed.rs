//! MP-9B fail-closed compatibility: marker-present vaults cannot write cleartext SQLite.
use mind_place_state::StateStore;
use std::fs;

#[test]
fn plaintext_state_open_never_creates_a_database_inside_a_vault() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("vault-v1.json"), b"synthetic vault marker").unwrap();
    let path = root.path().join("state.sqlite3");
    assert!(StateStore::open(&path).is_err());
    assert!(!path.exists(), "plaintext state was created in a vault");
}
