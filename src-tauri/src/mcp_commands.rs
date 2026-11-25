//! Trusted desktop owner controls; never exposed through MCP.
use super::{begin_unlocked, finish_unlocked, legacy_archive, owner_vault, AppState};
use mind_place_integration::{transport::LocalMcpServer, ClientAuthority};
use mind_place_security::{SecurityGate, UnlockLease};
use mind_place_state::{ClientCapability, ClientRecord, StateError};
use serde::Serialize;
use std::time::{Duration, Instant};
use tauri::State;

const ENDPOINT: &str = "http://127.0.0.1:43731/mcp";
const REVOCATION_OPEN_TIMEOUT: Duration = Duration::from_secs(3);
const REVOCATION_OPEN_RETRY: Duration = Duration::from_millis(25);

#[derive(Serialize)]
pub struct McpStatus {
    running: bool,
    endpoint: &'static str,
}
#[derive(Serialize)]
pub struct McpReaderCredential {
    client_id: String,
    display_name: String,
    credential: String,
    endpoint: &'static str,
}

#[tauri::command]
pub(super) fn get_local_mcp_status(state: State<'_, AppState>) -> McpStatus {
    McpStatus {
        running: !state.security.status().locked
            && state
                .mcp
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .is_some(),
        endpoint: ENDPOINT,
    }
}
#[tauri::command]
pub(super) async fn start_local_mcp(state: State<'_, AppState>) -> Result<McpStatus, String> {
    let lease = begin_unlocked(&state)?;
    if state
        .mcp
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .is_some()
    {
        finish_unlocked(&state, lease)?;
        return Ok(McpStatus {
            running: true,
            endpoint: ENDPOINT,
        });
    }
    let server = if let Some(vault) = owner_vault(&state) {
        LocalMcpServer::start_vault(vault, state.security.clone()).await
    } else {
        LocalMcpServer::start(
            legacy_archive(&state)?,
            state.security.clone(),
            state.state_path.clone(),
            state.index_path.clone(),
        )
        .await
    }
    .map_err(|_| "MP_MCP_BIND:Local MCP port is unavailable".to_string())?;
    finish_unlocked(&state, lease)?;
    let mut slot = state.mcp.lock().unwrap_or_else(|e| e.into_inner());
    if slot.is_none() {
        *slot = Some(server);
    }
    Ok(McpStatus {
        running: true,
        endpoint: ENDPOINT,
    })
}

fn open_for_revocation<T>(
    security: &SecurityGate,
    lease: UnlockLease,
    mut open: impl FnMut() -> mind_place_index::Result<T>,
) -> Result<T, String> {
    let deadline = Instant::now() + REVOCATION_OPEN_TIMEOUT;
    loop {
        security
            .validate_lease(lease)
            .map_err(super::security_error)?;
        match open() {
            Ok(authority) => return Ok(authority),
            Err(mind_place_index::IndexError::State(StateError::Busy)) => {
                // Reads take a short exclusive schema lock while opening state.
                // Retry only this pre-mutation contention, with the original lease.
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Err("MP_MCP_AUTHORITY:Unavailable".into());
                }
                std::thread::sleep(remaining.min(REVOCATION_OPEN_RETRY));
            }
            Err(_) => return Err("MP_MCP_AUTHORITY:Unavailable".into()),
        }
    }
}

async fn revoke_with_owner_lease<T: Send + 'static>(
    state: &AppState,
    lease: UnlockLease,
    revoke: impl FnOnce(ClientAuthority<'_>) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let security = state.security.clone();
    let vault = owner_vault(state);
    let path = state.state_path.clone();
    let changed = tauri::async_runtime::spawn_blocking(move || {
        let authority =
            open_for_revocation(&security, lease, || super::open_owner_state(&vault, &path))?;
        // Do not hold the Lock gate across SQLite or the vault writer fence.
        // A revocation already in flight only removes access, and the command
        // still refuses to acknowledge it if the original epoch has ended.
        security
            .validate_lease(lease)
            .map_err(super::security_error)?;
        revoke(ClientAuthority::new(&authority, &security))
    })
    .await
    .map_err(|_| "MP_MCP_AUTHORITY:Unavailable".to_owned())??;
    finish_unlocked(state, lease)?;
    Ok(changed)
}
#[tauri::command]
pub(super) fn stop_local_mcp(state: State<'_, AppState>) -> Result<McpStatus, String> {
    let lease = begin_unlocked(&state)?;
    let tunnel_stop = super::hosted_tunnel::stop_and_revoke(&state);
    state.mcp.lock().unwrap_or_else(|e| e.into_inner()).take();
    finish_unlocked(&state, lease)?;
    tunnel_stop?;
    Ok(McpStatus {
        running: false,
        endpoint: ENDPOINT,
    })
}
#[tauri::command]
pub(super) fn issue_local_mcp_reader(
    state: State<'_, AppState>,
    display_name: String,
) -> Result<McpReaderCredential, String> {
    let lease = begin_unlocked(&state)?;
    let authority = super::open_owner_state(&owner_vault(&state), &state.state_path)
        .map_err(|_| "MP_MCP_AUTHORITY:Unavailable".to_string())?;
    let issued = ClientAuthority::new(&authority, &state.security)
        .issue(&display_name, &[ClientCapability::Read])
        .map_err(|_| "MP_MCP_CLIENT:Cannot issue credential".to_string())?;
    finish_unlocked(&state, lease)?;
    let credential = issued.credential().to_owned();
    Ok(McpReaderCredential {
        client_id: issued.client.id,
        display_name: issued.client.display_name,
        credential,
        endpoint: ENDPOINT,
    })
}

#[tauri::command]
pub(super) fn list_local_mcp_clients(
    state: State<'_, AppState>,
    after: Option<String>,
) -> Result<Vec<ClientRecord>, String> {
    let lease = begin_unlocked(&state)?;
    let authority = super::open_owner_state(&owner_vault(&state), &state.state_path)
        .map_err(|_| "MP_MCP_AUTHORITY:Unavailable".to_string())?;
    let clients = authority
        .list_credentialed_clients(after.as_deref(), 50)
        .map_err(|_| "MP_MCP_CLIENT:Could not list clients".to_string())?;
    finish_unlocked(&state, lease)?;
    Ok(clients)
}

#[tauri::command]
pub(super) async fn revoke_local_mcp_client(
    state: State<'_, AppState>,
    client_id: String,
) -> Result<(), String> {
    let lease = begin_unlocked(&state)?;
    if super::hosted_tunnel::is_active_client(&state, &client_id) {
        super::hosted_tunnel::stop_and_revoke(&state)?;
        return finish_unlocked(&state, lease);
    }
    revoke_with_owner_lease(&state, lease, move |authority| {
        authority
            .revoke(&client_id)
            .map_err(|_| "MP_MCP_CLIENT:Cannot revoke client".to_owned())
    })
    .await
}

// MP-7MCP-02 / MP-ADR-006: owner-only read grants. A read credential discloses
// nothing until the owner grants it individual sources (no wildcard). These are
// trusted desktop commands; MCP clients can never reach them. A malformed id is
// rejected by the state layer, and failures are deliberately opaque.

#[tauri::command]
pub(super) fn grant_local_mcp_client_sources(
    state: State<'_, AppState>,
    client_id: String,
    source_ids: Vec<String>,
) -> Result<usize, String> {
    let lease = begin_unlocked(&state)?;
    let authority = super::open_owner_state(&owner_vault(&state), &state.state_path)
        .map_err(|_| "MP_MCP_AUTHORITY:Unavailable".to_string())?;
    let changed = ClientAuthority::new(&authority, &state.security)
        .grant_read_sources(&client_id, &source_ids)
        .map_err(|_| "MP_MCP_GRANT:Cannot grant read access".to_string())?;
    finish_unlocked(&state, lease)?;
    Ok(changed)
}

#[tauri::command]
pub(super) async fn revoke_local_mcp_client_sources(
    state: State<'_, AppState>,
    client_id: String,
    source_ids: Vec<String>,
) -> Result<usize, String> {
    let lease = begin_unlocked(&state)?;
    revoke_with_owner_lease(&state, lease, move |authority| {
        authority
            .revoke_read_sources(&client_id, &source_ids)
            .map_err(|_| "MP_MCP_GRANT:Cannot revoke read access".to_owned())
    })
    .await
}

#[tauri::command]
pub(super) async fn revoke_all_local_mcp_client_sources(
    state: State<'_, AppState>,
    client_id: String,
) -> Result<usize, String> {
    let lease = begin_unlocked(&state)?;
    revoke_with_owner_lease(&state, lease, move |authority| {
        authority
            .revoke_all_read_sources(&client_id)
            .map_err(|_| "MP_MCP_GRANT:Cannot revoke read access".to_owned())
    })
    .await
}

/// Sorted canonical source ids this client may read (at most 10,000).
#[tauri::command]
pub(super) fn list_local_mcp_client_sources(
    state: State<'_, AppState>,
    client_id: String,
) -> Result<Vec<String>, String> {
    let lease = begin_unlocked(&state)?;
    let authority = super::open_owner_state(&owner_vault(&state), &state.state_path)
        .map_err(|_| "MP_MCP_AUTHORITY:Unavailable".to_string())?;
    let sources = ClientAuthority::new(&authority, &state.security)
        .read_sources(&client_id)
        .map_err(|_| "MP_MCP_GRANT:Could not list read access".to_string())?;
    finish_unlocked(&state, lease)?;
    Ok(sources)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revocation_retries_only_schema_contention() {
        let temp = tempfile::tempdir().unwrap();
        let security = SecurityGate::open(temp.path().join("security.json")).unwrap();
        security.initialize(b"disposable revocation test").unwrap();
        let lease = security.require_unlocked().unwrap();
        let mut attempts = 0;
        let opened = open_for_revocation(&security, lease, || {
            attempts += 1;
            match attempts {
                1 => Err(StateError::Busy.into()),
                _ => Ok(()),
            }
        });
        assert_eq!(opened, Ok(()));
        assert_eq!(attempts, 2);

        let mut attempts = 0;
        let error = open_for_revocation::<()>(&security, lease, || {
            attempts += 1;
            Err(StateError::UnsafePath.into())
        });
        assert_eq!(error.unwrap_err(), "MP_MCP_AUTHORITY:Unavailable");
        assert_eq!(attempts, 1);
    }

    #[test]
    fn revocation_wait_does_not_reopen_on_a_new_unlock_epoch() {
        let temp = tempfile::tempdir().unwrap();
        let security = SecurityGate::open(temp.path().join("security.json")).unwrap();
        let secret = b"disposable revocation test";
        security.initialize(secret).unwrap();
        let lease = security.require_unlocked().unwrap();
        let mut attempts = 0;
        let error = open_for_revocation::<()>(&security, lease, || {
            attempts += 1;
            security.lock();
            security.unlock(secret).unwrap();
            Err(StateError::Busy.into())
        });
        assert!(error.is_err());
        assert_eq!(attempts, 1);
    }
}

/// One saved chat as the owner grant screen needs it. `source_id` is the
/// opaque handle sent back to the grant commands; it is never displayed.
#[derive(Serialize)]
pub struct GrantableChat {
    source_id: String,
    title: String,
    provider: String,
    captured_at: String,
    granted: bool,
}

#[derive(Serialize)]
pub struct GrantableChats {
    total: usize,
    /// How many of the `total` chats this client may currently read.
    allowed: usize,
    rows: Vec<GrantableChat>,
}

const GRANTABLE_PAGE_MAX: usize = 100;

/// Saved chats (optionally filtered by title/text like the Archive list) with
/// whether this client may read each one. Paged; at most 100 rows per call.
#[tauri::command]
pub(super) async fn list_local_mcp_grantable_chats(
    state: State<'_, AppState>,
    client_id: String,
    query: Option<String>,
    offset: Option<usize>,
    limit: Option<usize>,
) -> Result<GrantableChats, String> {
    let q = query.unwrap_or_default();
    let vault_query = q.clone();
    let conversations = if let Some(list) =
        super::vaulted_blocking(&state, move |vault| vault.list(&vault_query)).await?
    {
        list
    } else {
        let store = legacy_archive(&state)?;
        super::gated_blocking(&state, move || store.list(&q)).await?
    };
    let lease = begin_unlocked(&state)?;
    let authority = super::open_owner_state(&owner_vault(&state), &state.state_path)
        .map_err(|_| "MP_MCP_AUTHORITY:Unavailable".to_string())?;
    let granted: std::collections::HashSet<String> =
        ClientAuthority::new(&authority, &state.security)
            .read_sources(&client_id)
            .map_err(|_| "MP_MCP_GRANT:Could not list read access".to_string())?
            .into_iter()
            .collect();
    finish_unlocked(&state, lease)?;
    let limit = limit.unwrap_or(25).clamp(1, GRANTABLE_PAGE_MAX);
    let offset = offset.unwrap_or(0);
    let mut chats = Vec::new();
    for summary in &conversations {
        // Providers without a canonical source namespace are not readable
        // through MCP at all, so they are not offered.
        let Some(source_id) =
            mind_place_index::conversation_source_id(&summary.provider, &summary.conversation_id)
        else {
            continue;
        };
        chats.push(GrantableChat {
            granted: granted.contains(&source_id),
            source_id,
            title: summary.title.clone(),
            provider: summary.provider.clone(),
            captured_at: summary.captured_at.clone(),
        });
    }
    let total = chats.len();
    let allowed = chats.iter().filter(|chat| chat.granted).count();
    let rows = chats.into_iter().skip(offset).take(limit).collect();
    Ok(GrantableChats {
        total,
        allowed,
        rows,
    })
}
