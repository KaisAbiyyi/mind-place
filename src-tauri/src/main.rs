#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod hosted_tunnel;
mod mcp_commands;

use mind_place_archive::{
    ArchiveStore, AssetResult, Conversation, ConversationSummary, ExportResult, ImportReceipt,
    SnapshotSummary, NATIVE_HOST_NAME,
};
use mind_place_index::{
    ContextInspection, ContextPack, ContextRequest, FetchInspection, FetchRequest, FetchResponse,
    IndexError, IndexStats, IndexStore, RetrievalRequest, RetrievalResponse, SearchHit,
    SearchInspection,
};
use mind_place_integration::transport::LocalMcpServer;
use mind_place_security::{SecurityError, SecurityGate, SecurityStatus, UnlockLease};
use mind_place_state::{DocumentRoot, StateStore};
use serde::Serialize;
use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use tauri::{Manager, State};
use zeroize::Zeroizing;

#[derive(Clone)]
struct AppState {
    store: ArchiveStore,
    security: SecurityGate,
    state_path: PathBuf,
    index_path: PathBuf,
    authority_ready: Arc<AtomicBool>,
    // Serializes owner filesystem grants against Lock; prevents late approvals.
    document_authority_gate: Arc<Mutex<()>>,
    mcp: Arc<Mutex<Option<LocalMcpServer>>>,
    hosted: Arc<Mutex<Option<hosted_tunnel::HostedTunnel>>>,
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

async fn index_task<T: Send + 'static>(
    state: &AppState,
    operation: impl FnOnce(UnlockLease, SecurityGate) -> mind_place_index::Result<T> + Send + 'static,
) -> Result<T, String> {
    let lease = begin_unlocked(state)?;
    let security = state.security.clone();
    let result = tauri::async_runtime::spawn_blocking(move || operation(lease, security))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| format!("{}:{}", error.code(), error))?;
    finish_unlocked(state, lease)?;
    Ok(result)
}

#[tauri::command]
async fn refresh_archive_index(state: State<'_, AppState>) -> Result<IndexStats, String> {
    let archive = state.store.clone();
    let state_path = state.state_path.clone();
    let index_path = state.index_path.clone();
    index_task(&state, move |lease, security| {
        let authority = StateStore::open(state_path)?;
        let index = IndexStore::open(index_path)?;
        index.sync_with_check(&archive, &authority, || {
            security
                .validate_lease(lease)
                .map_err(|_| IndexError::Revoked)
        })
    })
    .await
}

#[tauri::command]
async fn rebuild_archive_index(state: State<'_, AppState>) -> Result<IndexStats, String> {
    let archive = state.store.clone();
    let state_path = state.state_path.clone();
    let index_path = state.index_path.clone();
    index_task(&state, move |lease, security| {
        let authority = StateStore::open(state_path)?;
        let index = IndexStore::open(index_path)?;
        index.rebuild_with_check(&archive, &authority, || {
            security
                .validate_lease(lease)
                .map_err(|_| IndexError::Revoked)
        })
    })
    .await
}

#[tauri::command]
async fn get_archive_index_stats(state: State<'_, AppState>) -> Result<IndexStats, String> {
    let index_path = state.index_path.clone();
    index_task(&state, move |_, _| IndexStore::open(index_path)?.stats()).await
}

#[tauri::command]
async fn search_archive_index(
    state: State<'_, AppState>,
    query: String,
    include_history: Option<bool>,
    limit: Option<usize>,
) -> Result<Vec<SearchHit>, String> {
    let index_path = state.index_path.clone();
    index_task(&state, move |_, _| {
        IndexStore::open(index_path)?.search(
            &query,
            include_history.unwrap_or(false),
            limit.unwrap_or(20),
        )
    })
    .await
}

#[tauri::command]
async fn search_retrieval(
    state: State<'_, AppState>,
    request: RetrievalRequest,
) -> Result<RetrievalResponse, String> {
    let index_path = state.index_path.clone();
    index_task(&state, move |_, _| {
        IndexStore::open(index_path)?.retrieve(&request)
    })
    .await
}

#[tauri::command]
async fn fetch_retrieval(
    state: State<'_, AppState>,
    request: FetchRequest,
) -> Result<FetchResponse, String> {
    let archive = state.store.clone();
    let index_path = state.index_path.clone();
    index_task(&state, move |lease, security| {
        IndexStore::open(index_path)?.fetch_with_check(&archive, &request, || {
            security
                .validate_lease(lease)
                .map_err(|_| IndexError::Revoked)
        })
    })
    .await
}

#[tauri::command]
async fn build_retrieval_context(
    state: State<'_, AppState>,
    request: ContextRequest,
) -> Result<ContextPack, String> {
    let archive = state.store.clone();
    let index_path = state.index_path.clone();
    index_task(&state, move |lease, security| {
        IndexStore::open(index_path)?.build_context_with_check(&archive, &request, || {
            security
                .validate_lease(lease)
                .map_err(|_| IndexError::Revoked)
        })
    })
    .await
}

#[tauri::command]
async fn inspect_retrieval(
    state: State<'_, AppState>,
    request: RetrievalRequest,
) -> Result<SearchInspection, String> {
    let index_path = state.index_path.clone();
    index_task(&state, move |lease, security| {
        IndexStore::open(index_path)?.inspect_retrieval_with_check(&request, || {
            security
                .validate_lease(lease)
                .map_err(|_| IndexError::Revoked)
        })
    })
    .await
}

#[tauri::command]
async fn inspect_retrieval_context(
    state: State<'_, AppState>,
    request: ContextRequest,
) -> Result<ContextInspection, String> {
    let archive = state.store.clone();
    let index_path = state.index_path.clone();
    index_task(&state, move |lease, security| {
        IndexStore::open(index_path)?.inspect_context_with_check(&archive, &request, || {
            security
                .validate_lease(lease)
                .map_err(|_| IndexError::Revoked)
        })
    })
    .await
}

#[tauri::command]
async fn inspect_retrieval_fetch(
    state: State<'_, AppState>,
    request: FetchRequest,
) -> Result<FetchInspection, String> {
    let archive = state.store.clone();
    let index_path = state.index_path.clone();
    index_task(&state, move |lease, security| {
        IndexStore::open(index_path)?.inspect_fetch_with_check(&archive, &request, || {
            security
                .validate_lease(lease)
                .map_err(|_| IndexError::Revoked)
        })
    })
    .await
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
        let status = security
            .initialize(secret.as_bytes())
            .map_err(security_error)?;
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
    // Lock cannot complete while a prior authorized grant is committing.
    // Conversely, a queued grant revalidates its lease after acquiring gate.
    let status = {
        let _gate = state
            .document_authority_gate
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.authority_ready.store(false, Ordering::Release);
        state.security.lock()
    };
    // Invalidate all read leases before killing the outbound helper and local listener.
    // Revocation is best-effort while locked; all in-flight evidence is already blocked.
    let _ = hosted_tunnel::stop_and_revoke(&state);
    state.mcp.lock().unwrap_or_else(|e| e.into_inner()).take();
    status
}

/// Owner-only document root inventory. Never part of the MCP tool catalog.
#[tauri::command]
async fn list_document_roots(state: State<'_, AppState>) -> Result<Vec<DocumentRoot>, String> {
    let lease = begin_unlocked(&state)?;
    let db_path = state.state_path.clone();
    let roots = tauri::async_runtime::spawn_blocking(move || {
        let db = StateStore::open(db_path).map_err(|e| format!("{}:{}", e.code(), e))?;
        db.list_document_roots()
            .map_err(|e| format!("{}:{}", e.code(), e))
    })
    .await
    .map_err(|e| e.to_string())??;
    finish_unlocked(&state, lease)?;
    Ok(roots)
}

/// Stages an existing directory as pending. No approval, scan, or document read.
#[tauri::command]
async fn stage_document_root(
    state: State<'_, AppState>,
    path: String,
) -> Result<DocumentRoot, String> {
    let lease = begin_unlocked(&state)?;
    if path.is_empty() || path.len() > 4096 || path.contains('\0') {
        return Err("MP_STATE_INVALID_VALUE:invalid directory path".into());
    }
    let db_path = state.state_path.clone();
    let root = tauri::async_runtime::spawn_blocking(move || {
        let db = StateStore::open(db_path).map_err(|e| format!("{}:{}", e.code(), e))?;
        db.register_document_root(Path::new(&path))
            .map_err(|e| format!("{}:{}", e.code(), e))
    })
    .await
    .map_err(|e| e.to_string())??;
    finish_unlocked(&state, lease)?;
    Ok(root)
}

/// Explicit authenticated desktop owner grant. NEVER expose via MCP.
#[tauri::command]
async fn approve_document_root(
    state: State<'_, AppState>,
    root_id: String,
) -> Result<DocumentRoot, String> {
    let lease = begin_unlocked(&state)?;
    let db_path = state.state_path.clone();
    let gate = state.document_authority_gate.clone();
    let security = state.security.clone();
    let authority_ready = state.authority_ready.clone();
    let root = tauri::async_runtime::spawn_blocking(move || {
        let _gate = gate.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if !authority_ready.load(Ordering::Acquire) {
            return Err("MP_STATE_NOT_READY:authoritative state not ready".to_owned());
        }
        security.validate_lease(lease).map_err(security_error)?;
        let db = StateStore::open(db_path).map_err(|e| format!("{}:{}", e.code(), e))?;
        let root = db.approve_document_root(&root_id)
            .map_err(|e| format!("{}:{}", e.code(), e))?;
        security.validate_lease(lease).map_err(security_error)?;
        Ok(root)
    })
    .await
    .map_err(|e| e.to_string())??;
    finish_unlocked(&state, lease)?;
    Ok(root)
}

/// Immediate, terminal owner revocation. Tombstones deny future reads.
#[tauri::command]
async fn revoke_document_root(state: State<'_, AppState>, root_id: String) -> Result<(), String> {
    let lease = begin_unlocked(&state)?;
    let db_path = state.state_path.clone();
    let gate = state.document_authority_gate.clone();
    let security = state.security.clone();
    let authority_ready = state.authority_ready.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _gate = gate.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if !authority_ready.load(Ordering::Acquire) {
            return Err("MP_STATE_NOT_READY:authoritative state not ready".to_owned());
        }
        security.validate_lease(lease).map_err(security_error)?;
        let db = StateStore::open(db_path).map_err(|e| format!("{}:{}", e.code(), e))?;
        let result = db.revoke_document_root(&root_id)
            .map_err(|e| format!("{}:{}", e.code(), e))?;
        security.validate_lease(lease).map_err(security_error)?;
        Ok(result)
    })
    .await
    .map_err(|e| e.to_string())??;
    finish_unlocked(&state, lease)
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
            let index_path = root.parent().unwrap_or(&root).join("index.sqlite3");
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
                index_path,
                authority_ready: Arc::new(AtomicBool::new(false)),
                document_authority_gate: Arc::new(Mutex::new(())),
                mcp: Arc::new(Mutex::new(None)),
                hosted: Arc::new(Mutex::new(None)),
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
            mcp_commands::get_local_mcp_status,
            mcp_commands::start_local_mcp,
            mcp_commands::stop_local_mcp,
            mcp_commands::issue_local_mcp_reader,
            mcp_commands::revoke_local_mcp_client,
            mcp_commands::list_local_mcp_clients,
            hosted_tunnel::get_hosted_tunnel_status,
            hosted_tunnel::start_hosted_tunnel,
            hosted_tunnel::stop_hosted_tunnel,
            initialize_security,
            unlock_security,
            lock_security,
            list_document_roots,
            stage_document_root,
            approve_document_root,
            revoke_document_root,
            list_conversations,
            read_conversation,
            list_snapshots,
            export_markdown,
            get_asset,
            get_setup,
            register_extension,
            import_legacy,
            refresh_archive_index,
            rebuild_archive_index,
            get_archive_index_stats,
            search_archive_index,
            search_retrieval,
            fetch_retrieval,
            build_retrieval_context,
            inspect_retrieval,
            inspect_retrieval_context,
            inspect_retrieval_fetch
        ])
        .run(tauri::generate_context!())
        .expect("Mind Place failed to start");
}
