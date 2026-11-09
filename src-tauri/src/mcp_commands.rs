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

// ---- MP-7MCP-02 per-client / per-collection read grants (owner only) ----
// A collection is an explicit set of canonical source IDs. A reader sees only
// the union of the collections granted to it; no grant means no evidence.
// None of these are reachable through MCP or with a reader credential.

#[tauri::command]
pub(super) fn create_local_mcp_collection(
    state: State<'_, AppState>,
    name: String,
    source_ids: Vec<String>,
) -> Result<String, String> {
    let lease = begin_unlocked(&state)?;
    let authority = super::open_owner_state(&owner_vault(&state), &state.state_path)
        .map_err(|_| "MP_MCP_AUTHORITY:Unavailable".to_string())?;
    let id = ClientAuthority::new(&authority, &state.security)
        .create_collection(&name, &source_ids)
        .map_err(|_| "MP_MCP_COLLECTION:Cannot create collection".to_string())?;
    finish_unlocked(&state, lease)?;
    Ok(id)
}

#[tauri::command]
pub(super) fn list_local_mcp_collections(
    state: State<'_, AppState>,
) -> Result<Vec<mind_place_state::CollectionRecord>, String> {
    let lease = begin_unlocked(&state)?;
    let authority = super::open_owner_state(&owner_vault(&state), &state.state_path)
        .map_err(|_| "MP_MCP_AUTHORITY:Unavailable".to_string())?;
    let collections = ClientAuthority::new(&authority, &state.security)
        .list_collections()
        .map_err(|_| "MP_MCP_COLLECTION:Could not list collections".to_string())?;
    finish_unlocked(&state, lease)?;
    Ok(collections)
}

#[tauri::command]
pub(super) fn list_local_mcp_client_collections(
    state: State<'_, AppState>,
    client_id: String,
) -> Result<Vec<String>, String> {
    let lease = begin_unlocked(&state)?;
    let authority = super::open_owner_state(&owner_vault(&state), &state.state_path)
        .map_err(|_| "MP_MCP_AUTHORITY:Unavailable".to_string())?;
    let granted = authority
        .client_collections(&client_id)
        .map_err(|_| "MP_MCP_COLLECTION:Could not list grants".to_string())?;
    finish_unlocked(&state, lease)?;
    Ok(granted)
}

#[tauri::command]
pub(super) fn grant_local_mcp_collection(
    state: State<'_, AppState>,
    client_id: String,
    collection_id: String,
) -> Result<(), String> {
    let lease = begin_unlocked(&state)?;
    let authority = super::open_owner_state(&owner_vault(&state), &state.state_path)
        .map_err(|_| "MP_MCP_AUTHORITY:Unavailable".to_string())?;
    ClientAuthority::new(&authority, &state.security)
        .grant_collection(&client_id, &collection_id)
        .map_err(|_| "MP_MCP_COLLECTION:Cannot grant collection".to_string())?;
    finish_unlocked(&state, lease)
}

#[tauri::command]
pub(super) fn revoke_local_mcp_collection_grant(
    state: State<'_, AppState>,
    client_id: String,
    collection_id: String,
) -> Result<(), String> {
    let lease = begin_unlocked(&state)?;
    let authority = super::open_owner_state(&owner_vault(&state), &state.state_path)
        .map_err(|_| "MP_MCP_AUTHORITY:Unavailable".to_string())?;
    ClientAuthority::new(&authority, &state.security)
        .revoke_collection_grant(&client_id, &collection_id)
        .map_err(|_| "MP_MCP_COLLECTION:Cannot revoke grant".to_string())?;
    finish_unlocked(&state, lease)
}

#[tauri::command]
pub(super) fn delete_local_mcp_collection(
    state: State<'_, AppState>,
    collection_id: String,
) -> Result<(), String> {
    let lease = begin_unlocked(&state)?;
    let authority = super::open_owner_state(&owner_vault(&state), &state.state_path)
        .map_err(|_| "MP_MCP_AUTHORITY:Unavailable".to_string())?;
    ClientAuthority::new(&authority, &state.security)
        .delete_collection(&collection_id)
        .map_err(|_| "MP_MCP_COLLECTION:Cannot delete collection".to_string())?;
    finish_unlocked(&state, lease)
}
