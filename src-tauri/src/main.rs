#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use mind_place_archive::{
    ArchiveStore, AssetResult, Conversation, ConversationSummary, ExportResult, ImportReceipt,
    SnapshotSummary, NATIVE_HOST_NAME,
};
use mind_place_security::{SecurityError, SecurityGate, SecurityStatus, UnlockLease};
use mind_place_state::StateStore;
use serde::Serialize;
use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::{atomic::{AtomicBool, Ordering}, Arc},
};
use tauri::{Manager, State};
use zeroize::Zeroizing;

#[derive(Clone)]
struct AppState {
    store: ArchiveStore,
    security: SecurityGate,
    state_path: PathBuf,
    authority_ready: Arc<AtomicBool>,
    extension_path: PathBuf,
    host_path: PathBuf,
    legacy_path: PathBuf,
}

#[derive(Serialize)]
struct SetupInfo {
    archive_path: String,
    extension_path: String,
    host_path: String,
    extension_id: String,
    native_host_name: String,
    registered_browsers: Vec<String>,
    platform: String,
    legacy_available: bool,
}

fn security_error(error: SecurityError) -> String {
    match error.retry_after_ms() {
        Some(retry_after_ms) => format!("{}:{retry_after_ms}:{}", error.code(), error),
        None => format!("{}:{}", error.code(), error),
    }
}

fn begin_unlocked(state: &AppState) -> Result<UnlockLease, String> {
    if !state.authority_ready.load(Ordering::Acquire) {
        return Err("MP_STATE_NOT_READY:authoritative state not ready".into());
    }
    state.security.require_unlocked().map_err(security_error)
}

fn finish_unlocked(state: &AppState, lease: UnlockLease) -> Result<(), String> {
    if !state.authority_ready.load(Ordering::Acquire) {
        return Err("MP_STATE_NOT_READY:authoritative state not ready".into());
    }
    state.security.validate_lease(lease).map_err(security_error)
}

async fn blocking<T: Send + 'static>(
    operation: impl FnOnce() -> mind_place_archive::Result<T> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(operation)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

async fn gated_blocking<T: Send + 'static>(
    state: &AppState,
    operation: impl FnOnce() -> mind_place_archive::Result<T> + Send + 'static,
) -> Result<T, String> {
    let lease = begin_unlocked(state)?;
    let result = blocking(operation).await?;
    finish_unlocked(state, lease)?;
    Ok(result)
}

#[tauri::command]
fn get_security_status(state: State<'_, AppState>) -> SecurityStatus {
    state.security.status()
}

#[tauri::command]
async fn initialize_security(
    state: State<'_, AppState>,
    secret: String,
) -> Result<SecurityStatus, String> {
    let security = state.security.clone();
    let state_path = state.state_path.clone();
    let authority_ready = state.authority_ready.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let secret = Zeroizing::new(secret);
        let status = security.initialize(secret.as_bytes()).map_err(security_error)?;
        if let Err(error) = StateStore::open(&state_path) {
            security.lock();
            return Err(format!("{}:{}", error.code(), error));
        }
        authority_ready.store(true, Ordering::Release);
        Ok(status)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn unlock_security(
    state: State<'_, AppState>,
    secret: String,
) -> Result<SecurityStatus, String> {
    let security = state.security.clone();
    let state_path = state.state_path.clone();
    let authority_ready = state.authority_ready.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let secret = Zeroizing::new(secret);
        let status = security.unlock(secret.as_bytes()).map_err(security_error)?;
        if let Err(error) = StateStore::open(&state_path) {
            security.lock();
            return Err(format!("{}:{}", error.code(), error));
        }
        authority_ready.store(true, Ordering::Release);
        Ok(status)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
fn lock_security(state: State<'_, AppState>) -> SecurityStatus {
    state.authority_ready.store(false, Ordering::Release);
    state.security.lock()
}

#[tauri::command]
async fn list_conversations(
    state: State<'_, AppState>,
    query: Option<String>,
) -> Result<Vec<ConversationSummary>, String> {
    let store = state.store.clone();
    gated_blocking(&state, move || store.list(query.as_deref().unwrap_or(""))).await
}

#[tauri::command]
async fn read_conversation(
    state: State<'_, AppState>,
    id: String,
    snapshot_id: Option<String>,
) -> Result<Conversation, String> {
    let store = state.store.clone();
    gated_blocking(&state, move || store.read(&id, snapshot_id.as_deref())).await
}

#[tauri::command]
async fn list_snapshots(
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<SnapshotSummary>, String> {
    let store = state.store.clone();
    gated_blocking(&state, move || store.snapshots(&id)).await
}

#[tauri::command]
async fn export_markdown(
    state: State<'_, AppState>,
    id: String,
    snapshot_id: Option<String>,
) -> Result<ExportResult, String> {
    let store = state.store.clone();
    gated_blocking(&state, move || {
        store.export_snapshot(&id, snapshot_id.as_deref())
    })
    .await
}

#[tauri::command]
async fn get_asset(
    state: State<'_, AppState>,
    id: String,
    asset_path: String,
) -> Result<AssetResult, String> {
    let store = state.store.clone();
    gated_blocking(&state, move || store.asset(&id, &asset_path)).await
}

#[tauri::command]
async fn import_legacy(state: State<'_, AppState>) -> Result<ImportReceipt, String> {
    let store = state.store.clone();
    let source = state.legacy_path.clone();
    gated_blocking(&state, move || store.import_legacy(&source)).await
}

fn native_manifest_path(state: &AppState, browser: &str) -> PathBuf {
    state
        .store
        .root()
        .join("native-host")
        .join(format!("{browser}.json"))
}

fn setup_info(state: &AppState) -> SetupInfo {
    let registered_browsers = ["chrome", "edge"]
        .into_iter()
        .filter(|browser| is_registered(state, browser))
        .map(str::to_owned)
        .collect();
    SetupInfo {
        archive_path: state.store.root().display().to_string(),
        extension_path: state.extension_path.display().to_string(),
        host_path: state.host_path.display().to_string(),
        extension_id: mind_place_archive::extension_id(),
        native_host_name: NATIVE_HOST_NAME.into(),
        registered_browsers,
        platform: std::env::consts::OS.into(),
        legacy_available: state.legacy_path.is_dir(),
    }
}

#[tauri::command]
fn get_setup(state: State<'_, AppState>) -> Result<SetupInfo, String> {
    let lease = begin_unlocked(&state)?;
    let info = setup_info(&state);
    finish_unlocked(&state, lease)?;
    Ok(info)
}

#[cfg(windows)]
fn registry_path(browser: &str) -> Option<String> {
    let prefix = match browser {
        "chrome" => "Software\\Google\\Chrome",
        "edge" => "Software\\Microsoft\\Edge",
        _ => return None,
    };
    Some(format!(
        "{prefix}\\NativeMessagingHosts\\{NATIVE_HOST_NAME}"
    ))
}

#[cfg(windows)]
fn is_registered(state: &AppState, browser: &str) -> bool {
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};
    let Some(key) = registry_path(browser) else {
        return false;
    };
    let Ok(key) = RegKey::predef(HKEY_CURRENT_USER).open_subkey(key) else {
        return false;
    };
    let Ok(path) = key.get_value::<String, _>("") else {
        return false;
    };
    if Path::new(&path) != native_manifest_path(state, browser) {
        return false;
    }
    let Ok(expected) = mind_place_archive::native_host_manifest(&state.host_path) else {
        return false;
    };
    std::fs::read(&path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .is_some_and(|value| value == expected)
}

#[cfg(not(windows))]
fn is_registered(_state: &AppState, _browser: &str) -> bool {
    false
}

#[tauri::command]
fn register_extension(state: State<'_, AppState>, browser: String) -> Result<SetupInfo, String> {
    let lease = begin_unlocked(&state)?;
    if !matches!(browser.as_str(), "chrome" | "edge") {
        return Err("Choose Chrome or Edge".into());
    }
    #[cfg(windows)]
    {
        use winreg::{enums::HKEY_CURRENT_USER, RegKey};
        let manifest = mind_place_archive::native_host_manifest(&state.host_path)
            .map_err(|e| e.to_string())?;
        let target = native_manifest_path(&state, &browser);
        let parent = target.parent().unwrap();
        if parent.symlink_metadata().is_ok_and(|m| {
            use std::os::windows::fs::MetadataExt;
            m.file_attributes() & 0x400 != 0
        }) {
            return Err("Native registration directory cannot be a reparse point".into());
        }
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
        file.write_all(&serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        file.as_file().sync_all().map_err(|e| e.to_string())?;
        file.persist(&target).map_err(|e| e.to_string())?;
        let (key, _) = RegKey::predef(HKEY_CURRENT_USER)
            .create_subkey(registry_path(&browser).unwrap())
            .map_err(|e| e.to_string())?;
        key.set_value("", &target.display().to_string())
            .map_err(|e| e.to_string())?;
        let info = setup_info(&state);
        finish_unlocked(&state, lease)?;
        Ok(info)
    }
    #[cfg(not(windows))]
    {
        let _ = lease;
        Err(
            "Browser native-host registration is currently implemented for Windows Chrome and Edge"
                .into(),
        )
    }
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let root = mind_place_archive::data_root()?;
            let security_path = root.parent().unwrap_or(&root).join("security-v1.json");
            let state_path = root.parent().unwrap_or(&root).join("state.sqlite3");
            let security = SecurityGate::open(security_path)?;
            let legacy_path = root.parent().unwrap_or(&root).join("sessions");
            let store = ArchiveStore::open(root)?;
            let host_path = std::env::current_exe()?
                .parent()
                .unwrap()
                .join(if cfg!(windows) {
                    "mind-place-capture-host.exe"
                } else {
                    "mind-place-capture-host"
                });
            let extension_path = if cfg!(debug_assertions) {
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../extension")
            } else {
                app.path().resource_dir()?.join("extension")
            };
            app.manage(AppState {
                store,
                security,
                state_path,
                authority_ready: Arc::new(AtomicBool::new(false)),
                extension_path,
                host_path,
                legacy_path,
            });
            tauri::WebviewWindowBuilder::from_config(app, &app.config().app.windows[0])?
                .on_navigation(|url| {
                    (url.scheme() == "tauri" && url.host_str() == Some("localhost"))
                        || matches!(
                            url.origin().ascii_serialization().as_str(),
                            "http://tauri.localhost" | "https://tauri.localhost"
                        )
                        || (cfg!(debug_assertions)
                            && url.origin().ascii_serialization() == "http://127.0.0.1:1420")
                })
                .build()?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_security_status,
            initialize_security,
            unlock_security,
            lock_security,
            list_conversations,
            read_conversation,
            list_snapshots,
            export_markdown,
            get_asset,
            get_setup,
            register_extension,
            import_legacy
        ])
        .run(tauri::generate_context!())
        .expect("Mind Place failed to start");
}
