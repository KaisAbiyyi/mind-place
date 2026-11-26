//! Trusted desktop owner controls; never exposed through MCP.
use super::{begin_unlocked, finish_unlocked, legacy_archive, owner_vault, AppState};
use mind_place_integration::{transport::LocalMcpServer, ClientAuthority};
use mind_place_security::{SecurityGate, UnlockLease};
use mind_place_state::{ClientCapability, ClientRecord, StateError, StateStore};
use serde::Serialize;
use std::time::{Duration, Instant};
use tauri::State;

const ENDPOINT: &str = "http://127.0.0.1:43731/mcp";
/// Longest an owner access command waits for the state schema lock. Short
/// enough that a stuck holder is reported, long enough for a busy MCP client.
const AUTHORITY_WAIT: Duration = Duration::from_secs(3);
const AUTHORITY_POLL: Duration = Duration::from_millis(25);

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

/// Opening the state store takes a short exclusive schema lock that fails at
/// once with `Busy`, and every MCP request takes the same lock. Wait for `Busy`
/// only, within `wait`; every other failure is immediate and opaque. The
/// original unlock lease is validated before each attempt, so Lock (or a new
/// unlock epoch) ends the wait and is never reopened under a later epoch.
fn open_with_bounded_wait<T>(
    security: &SecurityGate,
    lease: UnlockLease,
    wait: Duration,
    mut open: impl FnMut() -> mind_place_index::Result<T>,
) -> Result<T, String> {
    let deadline = Instant::now() + wait;
    loop {
        security
            .validate_lease(lease)
            .map_err(super::security_error)?;
        match open() {
            Ok(authority) => return Ok(authority),
            Err(mind_place_index::IndexError::State(StateError::Busy)) => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Err("MP_MCP_AUTHORITY:Unavailable".into());
                }
                std::thread::sleep(remaining.min(AUTHORITY_POLL));
            }
            Err(_) => return Err("MP_MCP_AUTHORITY:Unavailable".into()),
        }
    }
}

/// Runs one owner access operation on a blocking worker, so the wait never
/// holds the UI/async thread. The caller takes `lease` once, before the wait.
/// No document-authority or Lock gate is held while waiting or while the
/// operation runs, so Lock stays available; the lease is revalidated after the
/// operation and once more on the async side before the result is returned.
async fn with_owner_authority<T: Send + 'static>(
    state: &AppState,
    lease: UnlockLease,
    operation: impl FnOnce(&StateStore, &SecurityGate) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let security = state.security.clone();
    let vault = owner_vault(state);
    let path = state.state_path.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let authority = open_with_bounded_wait(&security, lease, AUTHORITY_WAIT, || {
            super::open_owner_state(&vault, &path)
        })?;
        let result = operation(&authority, &security)?;
        security
            .validate_lease(lease)
            .map_err(super::security_error)?;
        Ok(result)
    })
    .await
    .map_err(|_| "MP_MCP_AUTHORITY:Unavailable".to_owned())??;
    finish_unlocked(state, lease)?;
    Ok(result)
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
pub(super) async fn issue_local_mcp_reader(
    state: State<'_, AppState>,
    display_name: String,
) -> Result<McpReaderCredential, String> {
    let lease = begin_unlocked(&state)?;
    with_owner_authority(&state, lease, move |authority, security| {
        let issued = ClientAuthority::new(authority, security)
            .issue(&display_name, &[ClientCapability::Read])
            .map_err(|_| "MP_MCP_CLIENT:Cannot issue credential".to_string())?;
        let credential = issued.credential().to_owned();
        Ok(McpReaderCredential {
            client_id: issued.client.id,
            display_name: issued.client.display_name,
            credential,
            endpoint: ENDPOINT,
        })
    })
    .await
}

#[tauri::command]
pub(super) async fn list_local_mcp_clients(
    state: State<'_, AppState>,
    after: Option<String>,
) -> Result<Vec<ClientRecord>, String> {
    let lease = begin_unlocked(&state)?;
    with_owner_authority(&state, lease, move |authority, _| {
        authority
            .list_credentialed_clients(after.as_deref(), 50)
            .map_err(|_| "MP_MCP_CLIENT:Could not list clients".to_string())
    })
    .await
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
    with_owner_authority(&state, lease, move |authority, security| {
        ClientAuthority::new(authority, security)
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
pub(super) async fn grant_local_mcp_client_sources(
    state: State<'_, AppState>,
    client_id: String,
    source_ids: Vec<String>,
) -> Result<usize, String> {
    let lease = begin_unlocked(&state)?;
    with_owner_authority(&state, lease, move |authority, security| {
        ClientAuthority::new(authority, security)
            .grant_read_sources(&client_id, &source_ids)
            .map_err(|_| "MP_MCP_GRANT:Cannot grant read access".to_string())
    })
    .await
}

#[tauri::command]
pub(super) async fn revoke_local_mcp_client_sources(
    state: State<'_, AppState>,
    client_id: String,
    source_ids: Vec<String>,
) -> Result<usize, String> {
    let lease = begin_unlocked(&state)?;
    with_owner_authority(&state, lease, move |authority, security| {
        ClientAuthority::new(authority, security)
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
    with_owner_authority(&state, lease, move |authority, security| {
        ClientAuthority::new(authority, security)
            .revoke_all_read_sources(&client_id)
            .map_err(|_| "MP_MCP_GRANT:Cannot revoke read access".to_owned())
    })
    .await
}

/// Sorted canonical source ids this client may read (at most 10,000).
#[tauri::command]
pub(super) async fn list_local_mcp_client_sources(
    state: State<'_, AppState>,
    client_id: String,
) -> Result<Vec<String>, String> {
    let lease = begin_unlocked(&state)?;
    with_owner_authority(&state, lease, move |authority, security| {
        ClientAuthority::new(authority, security)
            .read_sources(&client_id)
            .map_err(|_| "MP_MCP_GRANT:Could not list read access".to_string())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use mind_place_index::IndexError;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn unlocked_gate(root: &std::path::Path) -> (SecurityGate, UnlockLease, &'static [u8]) {
        let secret: &'static [u8] = b"disposable owner wait test";
        let security = SecurityGate::open(root.join("security.json")).unwrap();
        security.initialize(secret).unwrap();
        let lease = security.require_unlocked().unwrap();
        (security, lease, secret)
    }

    fn held_schema_lock(root: &std::path::Path) -> std::fs::File {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(root.join(".state-schema.lock"))
            .unwrap();
        file.try_lock().unwrap();
        file
    }

    #[test]
    fn owner_wait_retries_only_schema_contention() {
        let temp = tempfile::tempdir().unwrap();
        let (security, lease, _) = unlocked_gate(temp.path());
        let mut attempts = 0;
        let opened = open_with_bounded_wait(&security, lease, AUTHORITY_WAIT, || {
            attempts += 1;
            match attempts {
                1 => Err(StateError::Busy.into()),
                _ => Ok(()),
            }
        });
        assert_eq!(opened, Ok(()));
        assert_eq!(attempts, 2);
    }

    #[test]
    fn only_busy_is_retried() {
        let temp = tempfile::tempdir().unwrap();
        let (security, lease, _) = unlocked_gate(temp.path());
        let mut attempts = 0;
        let error = open_with_bounded_wait::<()>(&security, lease, AUTHORITY_WAIT, || {
            attempts += 1;
            Err(StateError::UnsafePath.into())
        });
        assert_eq!(error.unwrap_err(), "MP_MCP_AUTHORITY:Unavailable");
        assert_eq!(attempts, 1);

        let mut attempts = 0;
        let error = open_with_bounded_wait::<()>(&security, lease, AUTHORITY_WAIT, || {
            attempts += 1;
            Err(IndexError::State(StateError::Corrupt))
        });
        assert_eq!(error.unwrap_err(), "MP_MCP_AUTHORITY:Unavailable");
        assert_eq!(attempts, 1);
    }

    #[test]
    fn owner_wait_does_not_reopen_on_a_new_unlock_epoch() {
        let temp = tempfile::tempdir().unwrap();
        let (security, lease, secret) = unlocked_gate(temp.path());
        let mut attempts = 0;
        let error = open_with_bounded_wait::<()>(&security, lease, AUTHORITY_WAIT, || {
            attempts += 1;
            security.lock();
            security.unlock(secret).unwrap();
            Err(StateError::Busy.into())
        });
        assert_ne!(error.unwrap_err(), "MP_MCP_AUTHORITY:Unavailable");
        assert_eq!(attempts, 1);
    }

    #[test]
    fn owner_command_waits_for_a_busy_schema_lock() {
        let temp = tempfile::tempdir().unwrap();
        let (security, lease, _) = unlocked_gate(temp.path());
        let path = temp.path().join("state.sqlite3");
        StateStore::open(&path).unwrap();
        let lock = held_schema_lock(temp.path());
        // Without the wait this is exactly the failure the owner saw.
        assert!(matches!(StateStore::open(&path), Err(StateError::Busy)));
        let holder = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            drop(lock);
        });
        let opened = open_with_bounded_wait(&security, lease, AUTHORITY_WAIT, || {
            Ok(StateStore::open(&path)?)
        });
        assert!(opened.is_ok());
        holder.join().unwrap();
    }

    #[test]
    fn owner_command_wait_is_bounded_and_fails_visibly() {
        let temp = tempfile::tempdir().unwrap();
        let (security, lease, _) = unlocked_gate(temp.path());
        let path = temp.path().join("state.sqlite3");
        StateStore::open(&path).unwrap();
        let _lock = held_schema_lock(temp.path());
        let started = Instant::now();
        let error = open_with_bounded_wait(&security, lease, Duration::from_millis(250), || {
            Ok(StateStore::open(&path)?)
        })
        .unwrap_err();
        assert_eq!(error, "MP_MCP_AUTHORITY:Unavailable");
        assert!(started.elapsed() >= Duration::from_millis(250));
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn lock_ends_the_wait() {
        let temp = tempfile::tempdir().unwrap();
        let (security, lease, _) = unlocked_gate(temp.path());
        let attempts = Arc::new(AtomicUsize::new(0));
        let counter = attempts.clone();
        let waiter_security = security.clone();
        let waiter = std::thread::spawn(move || {
            open_with_bounded_wait::<()>(&waiter_security, lease, Duration::from_secs(60), || {
                counter.fetch_add(1, Ordering::AcqRel);
                Err(StateError::Busy.into())
            })
        });
        while attempts.load(Ordering::Acquire) < 3 {
            std::thread::yield_now();
        }
        let started = Instant::now();
        security.lock();
        let error = waiter.join().unwrap().unwrap_err();
        assert_ne!(error, "MP_MCP_AUTHORITY:Unavailable");
        assert!(started.elapsed() < Duration::from_secs(10));
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
    let granted: std::collections::HashSet<String> =
        with_owner_authority(&state, lease, move |authority, security| {
            Ok(ClientAuthority::new(authority, security)
                .read_sources(&client_id)
                .map_err(|_| "MP_MCP_GRANT:Could not list read access".to_string())?
                .into_iter()
                .collect())
        })
        .await?;
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
