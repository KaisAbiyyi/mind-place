//! Trusted desktop owner controls; never exposed through MCP.
use super::{begin_unlocked, finish_unlocked, AppState};
use mind_place_integration::{transport::LocalMcpServer, ClientAuthority};
use mind_place_state::{ClientCapability, StateStore};
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
    let server = LocalMcpServer::start(
        state.store.clone(),
        state.security.clone(),
        state.state_path.clone(),
        state.index_path.clone(),
    )
    .await
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
    state.mcp.lock().unwrap_or_else(|e| e.into_inner()).take();
    finish_unlocked(&state, lease)?;
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
    let authority = StateStore::open(&state.state_path)
        .map_err(|_| "MP_MCP_AUTHORITY:Unavailable".to_string())?;
    let issued = ClientAuthority::new(&authority, &state.security)
        .issue(&display_name, &[ClientCapability::Read])
        .map_err(|_| "MP_MCP_CLIENT:Cannot issue credential".to_string())?;
    finish_unlocked(&state, lease)?;
    Ok(McpReaderCredential {
        client_id: issued.client.id,
        display_name: issued.client.display_name.clone(),
        credential: issued.credential().to_owned(),
        endpoint: ENDPOINT,
    })
}
#[tauri::command]
pub(super) fn revoke_local_mcp_client(
    state: State<'_, AppState>,
    client_id: String,
) -> Result<(), String> {
    let lease = begin_unlocked(&state)?;
    let authority = StateStore::open(&state.state_path)
        .map_err(|_| "MP_MCP_AUTHORITY:Unavailable".to_string())?;
    ClientAuthority::new(&authority, &state.security)
        .revoke(&client_id)
        .map_err(|_| "MP_MCP_CLIENT:Cannot revoke client".to_string())?;
    finish_unlocked(&state, lease)
}
