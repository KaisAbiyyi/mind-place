//! Trusted desktop owner controls; never exposed through MCP.
use super::{begin_unlocked, finish_unlocked, legacy_archive, owner_vault, AppState};
use mind_place_integration::{transport::LocalMcpServer, ClientAuthority};
use mind_place_state::{ClientCapability, ClientRecord};
use serde::Serialize;
use tauri::State;

const ENDPOINT: &str = "http://127.0.0.1:43731/mcp";

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
pub(super) fn revoke_local_mcp_client(
    state: State<'_, AppState>,
    client_id: String,
) -> Result<(), String> {
    let lease = begin_unlocked(&state)?;
    if super::hosted_tunnel::is_active_client(&state, &client_id) {
        super::hosted_tunnel::stop_and_revoke(&state)?;
        return finish_unlocked(&state, lease);
    }
    let authority = super::open_owner_state(&owner_vault(&state), &state.state_path)
        .map_err(|_| "MP_MCP_AUTHORITY:Unavailable".to_string())?;
    ClientAuthority::new(&authority, &state.security)
        .revoke(&client_id)
        .map_err(|_| "MP_MCP_CLIENT:Cannot revoke client".to_string())?;
    finish_unlocked(&state, lease)
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
pub(super) fn revoke_local_mcp_client_sources(
    state: State<'_, AppState>,
    client_id: String,
    source_ids: Vec<String>,
) -> Result<usize, String> {
    let lease = begin_unlocked(&state)?;
    let authority = super::open_owner_state(&owner_vault(&state), &state.state_path)
        .map_err(|_| "MP_MCP_AUTHORITY:Unavailable".to_string())?;
    let changed = ClientAuthority::new(&authority, &state.security)
        .revoke_read_sources(&client_id, &source_ids)
        .map_err(|_| "MP_MCP_GRANT:Cannot revoke read access".to_string())?;
    finish_unlocked(&state, lease)?;
    Ok(changed)
}

#[tauri::command]
pub(super) fn revoke_all_local_mcp_client_sources(
    state: State<'_, AppState>,
    client_id: String,
) -> Result<usize, String> {
    let lease = begin_unlocked(&state)?;
    let authority = super::open_owner_state(&owner_vault(&state), &state.state_path)
        .map_err(|_| "MP_MCP_AUTHORITY:Unavailable".to_string())?;
    let changed = ClientAuthority::new(&authority, &state.security)
        .revoke_all_read_sources(&client_id)
        .map_err(|_| "MP_MCP_GRANT:Cannot revoke read access".to_string())?;
    finish_unlocked(&state, lease)?;
    Ok(changed)
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
