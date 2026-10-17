use mind_place_archive::native_host_manifest;
use std::fs;
use tempfile::tempdir;

#[test]
fn manifest_grants_only_the_dedicated_extension_and_the_exact_installed_host() {
    let dir = tempdir().unwrap();
    let host = dir.path().join("mind-place-capture-host.exe");
    fs::write(&host, b"test executable").unwrap();
    let value = native_host_manifest(&host).unwrap();
    assert_eq!(value["name"], "com.mindplace.capture");
    assert_eq!(value["type"], "stdio");
    assert_eq!(value["path"], host.display().to_string());
    assert_eq!(
        value["allowed_origins"],
        serde_json::json!(["chrome-extension://dninfeidggaaioioficnigfpfjkdicjh/"])
    );
}

#[test]
fn missing_or_relative_host_paths_cannot_be_registered() {
    assert!(native_host_manifest(std::path::Path::new("relative-host.exe")).is_err());
    let dir = tempdir().unwrap();
    assert!(native_host_manifest(&dir.path().join("missing.exe")).is_err());
}
