//! Windows native messaging registration: configuration, not installed-browser proof.
//! Chrome and Brave discover the same Google Chrome key. Never redirect a
//! foreign registration or replace a manifest owned by another installation.
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};
use winreg::{
    enums::{
        HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WRITE, KEY_WOW64_32KEY,
        KEY_WOW64_64KEY, REG_CREATED_NEW_KEY, REG_SZ,
    },
    RegKey,
};

use mind_place_archive::NATIVE_HOST_NAME;

static REGISTRATION_GATE: Mutex<()> = Mutex::new(());
const REPARSE_POINT: u32 = 0x400;
const REGISTRY_VIEWS: [u32; 2] = [KEY_WOW64_64KEY, KEY_WOW64_32KEY];

pub fn registry_path(browser: &str) -> Option<String> {
    let prefix = match browser {
        "chrome" | "brave" => r"Software\Google\Chrome",
        "edge" => r"Software\Microsoft\Edge",
        _ => return None,
    };
    Some(format!(r"{prefix}\NativeMessagingHosts\{NATIVE_HOST_NAME}"))
}

struct RegistryRoots<'a> {
    user: &'a RegKey,
    machine: &'a RegKey,
}

fn permitted_paths(target: &Path, browser: &str) -> Result<Vec<PathBuf>, String> {
    let parent = target
        .parent()
        .ok_or("Native manifest directory is missing")?;
    let files: &[&str] = match browser {
        "chrome" | "brave" => &["chrome.json", "brave.json"],
        "edge" => &["edge.json"],
        _ => return Err("Choose Chrome, Edge or Brave".into()),
    };
    if !target.is_absolute() || target != parent.join(format!("{browser}.json")) {
        return Err("Native manifest path is not a recognized same-install path".into());
    }
    Ok(files.iter().map(|file| parent.join(file)).collect())
}

fn no_reparse(path: &Path, file: bool) -> Result<(), String> {
    use std::os::windows::fs::MetadataExt;
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if metadata.file_attributes() & REPARSE_POINT != 0
        || (file && !metadata.is_file())
        || (!file && !metadata.is_dir())
    {
        return Err("Native registration file or directory is not a regular path".into());
    }
    Ok(())
}

fn validate_manifest(path: &Path, expected: &serde_json::Value) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or("Native manifest directory is missing")?;
    no_reparse(parent, false)?;
    no_reparse(path, true)?;
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    let actual: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|_| "Native registration manifest contains invalid JSON".to_owned())?;
    if &actual != expected {
        return Err(
            "Native registration manifest does not match the approved executable and origin".into(),
        );
    }
    Ok(())
}

fn inspect(
    roots: &RegistryRoots<'_>,
    registry: &str,
    allowed: &[PathBuf],
    expected: &serde_json::Value,
) -> Result<Option<PathBuf>, String> {
    let mut configured = None;
    // Windows 32/64 views and both installation scopes must all be inspected.
    // A matching HKCU pointer never excuses a foreign or broken HKLM pointer.
    for (scope, hive) in [("HKCU", roots.user), ("HKLM", roots.machine)] {
        for view in REGISTRY_VIEWS {
            let key = match hive.open_subkey_with_flags(registry, KEY_READ | view) {
                Ok(key) => key,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(_) => return Err(format!("{scope} native registration is inaccessible")),
            };
            let raw = key.get_raw_value("").map_err(|_| {
                format!("{scope} native registration has a missing or inaccessible default value")
            })?;
            if raw.vtype != REG_SZ {
                return Err(format!(
                    "{scope} native registration default value is not REG_SZ"
                ));
            }
            let path: String = key
                .get_value("")
                .map_err(|_| format!("{scope} native registration string is malformed"))?;
            if path.is_empty() || !allowed.iter().any(|item| item == Path::new(&path)) {
                return Err(format!(
                    "{scope} native registration belongs to a foreign or unknown manifest"
                ));
            }
            validate_manifest(Path::new(&path), expected)
                .map_err(|error| format!("{scope} native registration conflict: {error}"))?;
            if configured.is_none() {
                configured = Some(PathBuf::from(path));
            }
        }
    }
    Ok(configured)
}

fn publish_manifest(target: &Path, expected: &serde_json::Value) -> Result<(), String> {
    let parent = target
        .parent()
        .ok_or("Native manifest directory is missing")?;
    if fs::symlink_metadata(parent).is_ok() {
        no_reparse(parent, false)?;
    } else {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        no_reparse(parent, false)?;
    }
    match fs::symlink_metadata(target) {
        Ok(_) => return validate_manifest(target, expected),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    let mut staged = tempfile::NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
    staged
        .write_all(&serde_json::to_vec_pretty(expected).map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())?;
    staged
        .as_file()
        .sync_all()
        .map_err(|error| error.to_string())?;
    // persist() would replace existing files. A concurrent/foreign file must
    // never be clobbered; only an independently validated existing one is safe.
    if let Err(error) = staged.persist_noclobber(target) {
        if error.error.kind() != io::ErrorKind::AlreadyExists {
            return Err(error.error.to_string());
        }
        validate_manifest(target, expected)?;
    }
    Ok(())
}

fn register_at(
    roots: &RegistryRoots<'_>,
    registry: &str,
    target: &Path,
    host: &Path,
    browser: &str,
) -> Result<(), String> {
    let allowed = permitted_paths(target, browser)?;
    let expected =
        mind_place_archive::native_host_manifest(host).map_err(|error| error.to_string())?;
    if inspect(roots, registry, &allowed, &expected)?.is_some() {
        return Ok(());
    }
    publish_manifest(target, &expected)?;
    // Recheck after file creation; never use the file operation as authority
    // to overwrite a concurrently appearing or machine-scoped registration.
    if inspect(roots, registry, &allowed, &expected)?.is_some() {
        return Ok(());
    }
    let (key, disposition) = roots
        .user
        .create_subkey_with_flags(registry, KEY_READ | KEY_WRITE | KEY_WOW64_64KEY)
        .map_err(|error| error.to_string())?;
    if disposition != REG_CREATED_NEW_KEY {
        // The key appeared since preflight. Even if its default is absent, do
        // not take ownership of it or change an existing registration.
        return match inspect(roots, registry, &allowed, &expected)? {
            Some(_) => Ok(()),
            None => {
                Err("Native registration appeared concurrently without an approved pointer".into())
            }
        };
    }
    match key.get_raw_value("") {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        _ => return Err("Native registration changed or cannot be inspected before publication".into()),
    }
    key.set_value("", &target.display().to_string())
        .map_err(|error| error.to_string())?;
    // A parallel machine-scope or other-view conflict invalidates success.
    if inspect(roots, registry, &allowed, &expected)?.is_none() {
        return Err("Native registration was not observable after publication".into());
    }
    Ok(())
}

pub fn is_registered(target: &Path, host: &Path, browser: &str) -> bool {
    let Some(registry) = registry_path(browser) else {
        return false;
    };
    let Ok(allowed) = permitted_paths(target, browser) else {
        return false;
    };
    let Ok(expected) = mind_place_archive::native_host_manifest(host) else {
        return false;
    };
    let user = RegKey::predef(HKEY_CURRENT_USER);
    let machine = RegKey::predef(HKEY_LOCAL_MACHINE);
    let roots = RegistryRoots {
        user: &user,
        machine: &machine,
    };
    inspect(&roots, &registry, &allowed, &expected)
        .ok()
        .flatten()
        .is_some()
}

pub fn register(target: &Path, host: &Path, browser: &str) -> Result<(), String> {
    let registry = registry_path(browser).ok_or("Choose Chrome, Edge or Brave")?;
    let _guard = REGISTRATION_GATE
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let user = RegKey::predef(HKEY_CURRENT_USER);
    let machine = RegKey::predef(HKEY_LOCAL_MACHINE);
    let roots = RegistryRoots {
        user: &user,
        machine: &machine,
    };
    register_at(&roots, &registry, target, host, browser)
}

#[cfg(test)]
mod browser_registration_tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    const SHARED: &str = r"NativeMessagingHosts\com.mindplace.capture";
    const EDGE: &str = r"EdgeNativeMessagingHosts\com.mindplace.capture";

    struct Fixture {
        _dir: tempfile::TempDir,
        base: String,
        user: RegKey,
        machine: RegKey,
        host: PathBuf,
        manifest_dir: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let host = dir.path().join("bundled-capture-host.exe");
            fs::write(&host, b"synthetic executable only").unwrap();
            let base = format!(
                r"Software\MindPlace_BRAVE_REG_01_Tests\{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            );
            let hive = RegKey::predef(HKEY_CURRENT_USER);
            let (user, _) = hive.create_subkey(format!(r"{base}\user")).unwrap();
            let (machine, _) = hive.create_subkey(format!(r"{base}\machine")).unwrap();
            Self {
                manifest_dir: dir.path().join("native-host"),
                _dir: dir,
                base,
                user,
                machine,
                host,
            }
        }
        fn roots(&self) -> RegistryRoots<'_> {
            RegistryRoots {
                user: &self.user,
                machine: &self.machine,
            }
        }
        fn target(&self, browser: &str) -> PathBuf {
            self.manifest_dir.join(format!("{browser}.json"))
        }
        fn register(&self, browser: &str) -> Result<(), String> {
            let registry = if browser == "edge" { EDGE } else { SHARED };
            register_at(
                &self.roots(),
                registry,
                &self.target(browser),
                &self.host,
                browser,
            )
        }
        fn visible(&self, browser: &str) -> bool {
            let registry = if browser == "edge" { EDGE } else { SHARED };
            let expected = mind_place_archive::native_host_manifest(&self.host).unwrap();
            inspect(
                &self.roots(),
                registry,
                &permitted_paths(&self.target(browser), browser).unwrap(),
                &expected,
            )
            .ok()
            .flatten()
            .is_some()
        }
        fn put(&self, machine: bool, registry: &str, path: &Path) {
            let root = if machine { &self.machine } else { &self.user };
            let (key, _) = root.create_subkey(registry).unwrap();
            key.set_value("", &path.display().to_string()).unwrap();
        }
        fn pointer(&self, registry: &str) -> String {
            self.user
                .open_subkey(registry)
                .unwrap()
                .get_value("")
                .unwrap()
        }
        fn expected_file(&self, path: &Path) {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            let expected = mind_place_archive::native_host_manifest(&self.host).unwrap();
            fs::write(path, serde_json::to_vec_pretty(&expected).unwrap()).unwrap();
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(&self.base);
        }
    }

    #[test]
    fn shared_chrome_brave_lookup_and_independent_edge_key() {
        assert_eq!(registry_path("chrome"), registry_path("brave"));
        assert_ne!(registry_path("chrome"), registry_path("edge"));
        assert_eq!(registry_path("opera"), None);
    }

    #[test]
    fn chrome_then_brave_retains_pointer_and_manifest_bytes() {
        let fixture = Fixture::new();
        fixture.register("chrome").unwrap();
        let original = fs::read(fixture.target("chrome")).unwrap();
        let pointer = fixture.pointer(SHARED);
        fixture.register("brave").unwrap();
        assert!(fixture.visible("brave") && fixture.visible("chrome"));
        assert_eq!(fixture.pointer(SHARED), pointer);
        assert_eq!(fs::read(fixture.target("chrome")).unwrap(), original);
        assert!(!fixture.target("brave").exists());
    }

    #[test]
    fn brave_then_chrome_retains_original_pointer() {
        let fixture = Fixture::new();
        fixture.register("brave").unwrap();
        let pointer = fixture.pointer(SHARED);
        fixture.register("chrome").unwrap();
        assert!(fixture.visible("brave") && fixture.visible("chrome"));
        assert_eq!(fixture.pointer(SHARED), pointer);
        assert!(!fixture.target("chrome").exists());
    }

    #[test]
    fn valid_preexisting_manifest_preserved_and_unregistration_reversible() {
        let fixture = Fixture::new();
        let target = fixture.target("chrome");
        fixture.expected_file(&target);
        let bytes = fs::read(&target).unwrap();
        fixture.register("chrome").unwrap();
        assert_eq!(fs::read(&target).unwrap(), bytes);
        fixture.user.delete_subkey_all(SHARED).unwrap();
        assert!(!fixture.visible("brave"));
        fixture.register("brave").unwrap();
        assert_eq!(
            fixture.pointer(SHARED),
            fixture.target("brave").display().to_string()
        );
        assert_eq!(fs::read(&target).unwrap(), bytes);
    }

    #[test]
    fn refuse_foreign_pointer_even_if_json_matches_without_mutation() {
        let fixture = Fixture::new();
        let foreign = fixture._dir.path().join("foreign.json");
        fixture.expected_file(&foreign);
        fixture.put(false, SHARED, &foreign);
        let old = fs::read(&foreign).unwrap();
        assert!(fixture.register("brave").is_err());
        assert!(!fixture.visible("brave"));
        assert_eq!(fixture.pointer(SHARED), foreign.display().to_string());
        assert_eq!(fs::read(&foreign).unwrap(), old);
        assert!(!fixture.target("brave").exists());
    }

    #[test]
    fn machine_conflict_blocks_matching_user_registration() {
        let fixture = Fixture::new();
        fixture.register("chrome").unwrap();
        let pointer = fixture.pointer(SHARED);
        fixture.put(
            true,
            SHARED,
            &fixture._dir.path().join("foreign-machine.json"),
        );
        assert!(fixture.register("brave").is_err());
        assert!(!fixture.visible("chrome"));
        assert_eq!(fixture.pointer(SHARED), pointer);
    }

    #[test]
    fn absent_or_malformed_or_wrong_origin_or_wrong_executable_fails_closed() {
        for mutation in ["missing", "malformed", "origin", "binary"] {
            let fixture = Fixture::new();
            let target = fixture.target("chrome");
            fixture.expected_file(&target);
            if mutation == "missing" {
                fs::remove_file(&target).unwrap();
            }
            if mutation == "malformed" {
                fs::write(&target, b"{not-json").unwrap();
            }
            if mutation == "origin" || mutation == "binary" {
                let mut value: serde_json::Value =
                    serde_json::from_slice(&fs::read(&target).unwrap()).unwrap();
                let field = if mutation == "origin" {
                    "allowed_origins"
                } else {
                    "path"
                };
                value[field] = if mutation == "origin" {
                    serde_json::json!(["chrome-extension://foreign/"])
                } else {
                    serde_json::json!("C:\\foreign.exe")
                };
                fs::write(&target, serde_json::to_vec(&value).unwrap()).unwrap();
            }
            fixture.put(false, SHARED, &target);
            assert!(!fixture.visible("brave"), "{mutation}");
            assert!(fixture.register("brave").is_err(), "{mutation}");
            assert!(!fixture.target("brave").exists(), "{mutation}");
        }
    }

    #[test]
    fn non_string_and_empty_registry_values_are_conflicts() {
        let fixture = Fixture::new();
        let (key, _) = fixture.user.create_subkey(SHARED).unwrap();
        key.set_value("", &7u32).unwrap();
        assert!(fixture.register("chrome").is_err());
        assert!(!fixture.visible("chrome"));

        let fixture = Fixture::new();
        fixture.put(false, SHARED, Path::new(""));
        assert!(fixture.register("brave").is_err());
        assert!(!fixture.visible("brave"));
    }

    #[test]
    fn preexisting_malformed_target_is_never_overwritten_and_edge_is_independent() {
        let fixture = Fixture::new();
        let target = fixture.target("chrome");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, b"foreign-missing-json").unwrap();
        assert!(fixture.register("chrome").is_err());
        assert_eq!(fs::read(&target).unwrap(), b"foreign-missing-json");
        fixture.register("edge").unwrap();
        assert!(fixture.visible("edge"));
        assert!(!fixture.visible("chrome"));
    }
}
