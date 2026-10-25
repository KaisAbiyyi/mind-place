//! MP-9B: FTS5/graph derived indexes cannot remain visible at rest.
use mind_place_index::IndexStore;
use mind_place_index::documents::DocumentIndex;
use std::fs;

#[test]
fn search_fts_is_ciphered_and_key_checked() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("index.sqlite3");
    let key = [0x9au8; 32];
    let index = IndexStore::open_encrypted(&db, key).unwrap();
    let _ = index.stats().unwrap();
    drop(index);
    let disk = fs::read(&db).unwrap();
    assert!(!disk.starts_with(b"SQLite format 3\0"));
    assert!(!disk.windows(7).any(|w| w == b"fts5("));
    assert!(IndexStore::open_encrypted(&db, [0x9bu8; 32]).is_err());
    assert!(IndexStore::open(&db).is_err());
    IndexStore::open_encrypted(&db, key).unwrap();
}
#[test]
fn document_fts_is_ciphered() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("document-index.sqlite3");
    let key = [0x8au8; 32];
    DocumentIndex::open_encrypted(&db, key).unwrap();
    let bytes = fs::read(&db).unwrap();
    assert!(!bytes.starts_with(b"SQLite format 3\0"));
    assert!(DocumentIndex::open_encrypted(&db, [0x8bu8; 32]).is_err());
    DocumentIndex::open_encrypted(&db, key).unwrap();
}
