//! MP-9B fail-closed behavior for unkeyed derived search projections.
use mind_place_index::{documents::DocumentIndex, IndexStore};
use std::fs;

#[test]
fn plaintext_index_and_document_index_never_initialize_inside_a_vault() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("vault-v1.json"), b"synthetic vault marker").unwrap();
    let search = root.path().join("index.sqlite3");
    let docs = root.path().join("document-index.sqlite3");
    assert!(IndexStore::open(&search).is_err());
    assert!(DocumentIndex::open(&docs).is_err());
    assert!(!search.exists(), "plaintext search index was created");
    assert!(!docs.exists(), "plaintext document index was created");
}
