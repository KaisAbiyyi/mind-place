//! Owner-local MP-9B recovery ceremony for *new synthetic/empty installs*.
//! This does not enable legacy archive, native capture or plaintext fallback.
//! No remote MCP endpoints call these commands.
use super::AppState;
use mind_place_state::vault_bootstrap::PreparedFirstRunVault;
use std::sync::{Arc, Mutex};
use tauri::State;
use zeroize::Zeroizing;

pub(super) type PendingVault = Arc<Mutex<Option<PreparedFirstRunVault>>>;

#[tauri::command]
pub(super) async fn begin_vault_setup(
    state: State<'_, AppState>,
    secret: String,
) -> Result<String, String> {
    if state.store.is_some() {
        return Err("MP_VAULT_LEGACY_MIGRATION_REQUIRED".into());
    }
    let security = state.security.clone();
    let pending = state.pending_vault.clone();
    let vault_root = state.vault_root.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let secret = Zeroizing::new(secret);
        let mut slot = pending.lock().unwrap_or_else(|error| error.into_inner());
        if slot.is_some() {
            return Err("MP_VAULT_CEREMONY_ALREADY_ACTIVE".to_owned());
        }
        if vault_root.exists() {
            return Err("MP_VAULT_EXISTS:never overwrite an existing vault".to_owned());
        }
        // Initializing the verifier never creates legacy state or archive.
        // After process restart, require the existing unlock secret again.
        if security.status().configured {
            security
                .unlock(secret.as_bytes())
                .map_err(|error| format!("{}:{}", error.code(), error))?;
        } else {
            security
                .initialize(secret.as_bytes())
                .map_err(|error| format!("{}:{}", error.code(), error))?;
        }
        let lease = security
            .require_unlocked()
            .map_err(|error| format!("{}:{}", error.code(), error))?;
        let prepared = PreparedFirstRunVault::prepare(&vault_root, &security, lease)
            .map_err(|error| format!("{}:{}", error.code(), error))?;
        let display_only = prepared.recovery_code();
        *slot = Some(prepared);
        Ok(display_only.as_str().to_owned())
    })
    .await
    .map_err(|_| "MP_VAULT_SETUP_WORKER_FAILED".to_owned())?
}

#[tauri::command]
pub(super) async fn confirm_vault_setup(
    state: State<'_, AppState>,
    code: String,
) -> Result<(), String> {
    let security = state.security.clone();
    let pending = state.pending_vault.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let code = Zeroizing::new(code);
        let mut slot = pending.lock().unwrap_or_else(|error| error.into_inner());
        let setup = slot
            .as_ref()
            .ok_or_else(|| "MP_VAULT_CEREMONY_NOT_STARTED".to_owned())?;
        let lease = security
            .require_unlocked()
            .map_err(|error| format!("{}:{}", error.code(), error))?;
        // Wrong code leaves the in-memory proposal intact. Successful
        // publication clears the secret by dropping PreparedFirstRunVault.
        setup
            .confirm_and_provision(code.as_str(), &security, lease)
            .map_err(|error| format!("{}:{}", error.code(), error))?;
        slot.take();
        // Until *all* production adapters are ported to the keyed vault,
        // never signal authority_ready or allow legacy archive access.
        security.lock();
        Ok(())
    })
    .await
    .map_err(|_| "MP_VAULT_SETUP_WORKER_FAILED".to_owned())?
}

#[tauri::command]
pub(super) fn cancel_vault_setup(state: State<'_, AppState>) -> Result<(), String> {
    state
        .pending_vault
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .take();
    state.security.lock();
    Ok(())
}
