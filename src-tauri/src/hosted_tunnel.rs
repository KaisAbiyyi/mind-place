//! Owner-operated outbound-only hosted MCP bridge via the official tunnel-client.
//! Never expose the local MCP port publicly or run a tunnel while locked.
use super::{begin_unlocked, finish_unlocked, AppState};
use mind_place_integration::ClientAuthority;
use mind_place_state::{ClientCapability, StateStore};
use serde::Serialize;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::Ordering;
use tauri::State;
use zeroize::Zeroizing;

const MCP_ORIGIN: &str = "http://127.0.0.1:43731/mcp";
const HEADER_REF: &str = "Authorization: env:MIND_PLACE_HOSTED_BEARER";
const CLIENT_NAME: &str = "Mind Place hosted tunnel (ephemeral read-only)";

/// Process and identity are owned by the unlocked desktop, not by remote MCP.
pub(crate) struct HostedTunnel {
    child: Child,
    client_id: String,
    tunnel_id: String,
}
impl HostedTunnel {
    fn running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}
impl Drop for HostedTunnel {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Serialize)]
pub(super) struct HostedTunnelStatus {
    /// Means the local helper process is alive, not that ChatGPT is connected.
    running: bool,
    tunnel_id: Option<String>,
    connection_verified: bool,
}

fn stopped() -> HostedTunnelStatus {
    HostedTunnelStatus {
        running: false,
        tunnel_id: None,
        connection_verified: false,
    }
}

fn valid_segment(segment: &str, len: usize) -> bool {
    segment.len() == len
        && segment
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

/// tunnel-client IDs have either a bare 32-character body or a four-character namespace.
fn valid_tunnel_id(id: &str) -> bool {
    let Some(suffix) = id.strip_prefix("tunnel_") else {
        return false;
    };
    valid_segment(suffix, 32)
        || suffix
            .split_once('_')
            .is_some_and(|(ns, rest)| valid_segment(ns, 4) && valid_segment(rest, 32))
}

fn valid_runtime_key(key: &str) -> bool {
    (16..=2048).contains(&key.len())
        && key.starts_with("sk-")
        && key.bytes().all(|b| b.is_ascii_graphic())
}

/// Fixed adjacent binary, never a shell command, URL, user-controlled executable path
/// or auto-downloaded code. Refuse symlinks escaping the app directory.
fn trusted_client_binary() -> Result<PathBuf, String> {
    let app = std::env::current_exe().map_err(|_| "MP_TUNNEL_BINARY:Cannot locate app")?;
    let parent = app
        .parent()
        .ok_or("MP_TUNNEL_BINARY:Cannot locate app directory")?;
    let expected = parent.join(if cfg!(windows) {
        "tunnel-client.exe"
    } else {
        "tunnel-client"
    });
    let actual = expected
        .canonicalize()
        .map_err(|_| "MP_TUNNEL_BINARY:Install official tunnel-client beside Mind Place")?;
    let trusted_parent = parent
        .canonicalize()
        .map_err(|_| "MP_TUNNEL_BINARY:Cannot verify app directory")?;
    if actual.parent() != Some(trusted_parent.as_path()) || !actual.is_file() {
        return Err("MP_TUNNEL_BINARY:Untrusted helper path".into());
    }
    Ok(actual)
}

fn build_client_command(binary: PathBuf, id: &str, api_key: &str, credential: &str) -> Command {
    // Sensitive values go only into the child environment (and are never echoed).
    // Static MCP headers are resolved ONLY at the local origin by tunnel-client.
    // An upstream-provided Authorization header may override the static header;
    // Phase 3B still authenticates every actual bearer and denies unknown grants.
    let bearer = Zeroizing::new(format!("Bearer {credential}"));
    let mut command = Command::new(binary);
    command
        .arg("run")
        .arg("--health.listen-addr=127.0.0.1:0")
        .arg("--log.level=warn")
        .env("CONTROL_PLANE_TUNNEL_ID", id)
        .env("CONTROL_PLANE_API_KEY", api_key)
        .env("MCP_SERVER_URL", MCP_ORIGIN)
        .env("MCP_EXTRA_HEADERS", HEADER_REF)
        .env("MCP_DISCOVERY_EXTRA_HEADERS", HEADER_REF)
        .env("MIND_PLACE_HOSTED_BEARER", bearer.as_str())
        .env("HEALTH_LISTEN_ADDR", "127.0.0.1:0")
        .env("ADMIN_UI_OPEN_BROWSER", "false")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    command
}

fn launch_client(id: &str, api_key: &str, credential: &str) -> Result<Child, String> {
    let binary = trusted_client_binary()?;
    build_client_command(binary, id, api_key, credential)
        .spawn()
        .map_err(|_| "MP_TUNNEL_SPAWN:Could not start installed tunnel-client".into())
}

fn revoke_ephemeral(state: &AppState, client_id: &str) -> Result<(), String> {
    StateStore::open(&state.state_path)
        .and_then(|store| store.revoke_client(client_id))
        .map_err(|_| "MP_TUNNEL_REVOKE:Review hosted client in local client inventory".into())
}

/// Drop the child before revoking, so no newly admitted request can use it.
/// On unexpected termination, the next status read will revoke its client.
pub(crate) fn stop_and_revoke(state: &AppState) -> Result<(), String> {
    let previous = state
        .hosted
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take();
    if let Some(tunnel) = previous {
        let client_id = tunnel.client_id.clone();
        drop(tunnel);
        revoke_ephemeral(state, &client_id)?;
    }
    Ok(())
}

pub(crate) fn is_active_client(state: &AppState, id: &str) -> bool {
    state
        .hosted
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .is_some_and(|hosted| hosted.client_id == id)
}

#[tauri::command]
pub(super) fn get_hosted_tunnel_status(state: State<'_, AppState>) -> HostedTunnelStatus {
    if state.security.status().locked || !state.authority_ready.load(Ordering::Acquire) {
        return stopped();
    }
    let mut guard = state.hosted.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(tunnel) = guard.as_mut() {
        if tunnel.running() {
            return HostedTunnelStatus {
                running: true,
                tunnel_id: Some(tunnel.tunnel_id.clone()),
                connection_verified: false,
            };
        }
    }
    // A dead helper must not leave a live automatic read credential.
    let previous = guard.take();
    drop(guard);
    if let Some(tunnel) = previous {
        let id = tunnel.client_id.clone();
        drop(tunnel);
        let _ = revoke_ephemeral(&state, &id);
    }
    stopped()
}

#[tauri::command]
pub(super) fn start_hosted_tunnel(
    state: State<'_, AppState>,
    tunnel_id: String,
    api_key: String,
    acknowledged_remote_access: bool,
) -> Result<HostedTunnelStatus, String> {
    let api_key = Zeroizing::new(api_key);
    let lease = begin_unlocked(&state)?;
    if !acknowledged_remote_access {
        return Err("MP_TUNNEL_CONSENT:Confirm remote read access first".into());
    }
    if !valid_tunnel_id(&tunnel_id) || !valid_runtime_key(&api_key) {
        return Err("MP_TUNNEL_CONFIG:Invalid tunnel ID or runtime API key".into());
    }
    let mut guard = state.hosted.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_some() {
        return Err("MP_TUNNEL_ACTIVE:Stop the previous tunnel first".into());
    }
    if state
        .mcp
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .is_none()
    {
        return Err("MP_TUNNEL_LOCAL:Start local MCP before starting the tunnel".into());
    }
    let authority = StateStore::open(&state.state_path)
        .map_err(|_| "MP_TUNNEL_AUTHORITY:Cannot open authority".to_string())?;
    let issued = ClientAuthority::new(&authority, &state.security)
        .issue(CLIENT_NAME, &[ClientCapability::Read])
        .map_err(|_| "MP_TUNNEL_AUTHORITY:Cannot create read-only client".to_string())?;
    let client_id = issued.client.id.clone();
    let child = match launch_client(&tunnel_id, &api_key, issued.credential()) {
        Ok(child) => child,
        Err(error) => {
            let _ = revoke_ephemeral(&state, &client_id);
            return Err(error);
        }
    };
    let tunnel = HostedTunnel {
        child,
        client_id: client_id.clone(),
        tunnel_id: tunnel_id.clone(),
    };
    if let Err(error) = finish_unlocked(&state, lease) {
        drop(tunnel);
        let _ = revoke_ephemeral(&state, &client_id);
        return Err(error);
    }
    *guard = Some(tunnel);
    Ok(HostedTunnelStatus {
        running: true,
        tunnel_id: Some(tunnel_id),
        connection_verified: false,
    })
}

#[tauri::command]
pub(super) fn stop_hosted_tunnel(state: State<'_, AppState>) -> Result<HostedTunnelStatus, String> {
    let lease = begin_unlocked(&state)?;
    stop_and_revoke(&state)?;
    finish_unlocked(&state, lease)?;
    Ok(stopped())
}

#[cfg(test)]
mod tests {
    use super::{valid_runtime_key, valid_tunnel_id, HEADER_REF, MCP_ORIGIN};
    #[test]
    fn accepts_only_documented_tunnel_identifiers() {
        assert!(valid_tunnel_id(&format!("tunnel_{}", "a".repeat(32))));
        assert!(valid_tunnel_id(&format!("tunnel_ab12_{}", "9".repeat(32))));
        for invalid in [
            "tunnel_test",
            "tunnel_",
            "tunnel_AA",
            "tunnel_abc_123",
            "tunnel_ab12_1111",
            "../tunnel",
            "tunnel_abc\n",
            "tunnel_ABC",
        ] {
            assert!(!valid_tunnel_id(invalid), "{invalid}");
        }
    }
    #[test]
    fn runtime_key_cannot_contain_shell_or_header_controls() {
        assert!(valid_runtime_key("sk-proj_long_runtime_key"));
        for invalid in [
            "sk-short",
            "Bearer abcdefghijklmnop",
            "sk-key\r\ninjected: true",
            "sk-hello world123",
        ] {
            assert!(!valid_runtime_key(invalid));
        }
    }
    #[test]
    fn bridge_configuration_is_loopback_only_and_secret_indirect() {
        assert_eq!(MCP_ORIGIN, "http://127.0.0.1:43731/mcp");
        assert_eq!(HEADER_REF, "Authorization: env:MIND_PLACE_HOSTED_BEARER");
        assert!(!HEADER_REF.contains("mpc_v1_"));
    }

    #[test]
    fn subprocess_argv_has_no_runtime_key_or_archive_token() {
        let key = "sk-proj_private_example_key_123";
        let credential = "mpc_v1_private_example";
        let command = super::build_client_command(
            std::path::PathBuf::from("tunnel-client"),
            &format!("tunnel_{}", "a".repeat(32)),
            key,
            credential,
        );
        let argv = command
            .get_args()
            .map(|s| s.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        assert_eq!(argv[0], "run");
        assert!(argv
            .iter()
            .all(|part| !part.contains(key) && !part.contains(credential)));
        let env = command
            .get_envs()
            .filter_map(|(k, v)| {
                v.map(|v| {
                    (
                        k.to_string_lossy().to_string(),
                        v.to_string_lossy().to_string(),
                    )
                })
            })
            .collect::<std::collections::HashMap<_, _>>();
        assert_eq!(
            env.get("CONTROL_PLANE_API_KEY").map(String::as_str),
            Some(key)
        );
        assert_eq!(
            env.get("MIND_PLACE_HOSTED_BEARER").map(String::as_str),
            Some("Bearer mpc_v1_private_example")
        );
        assert_eq!(
            env.get("MCP_SERVER_URL").map(String::as_str),
            Some(MCP_ORIGIN)
        );
        assert_eq!(
            env.get("MCP_EXTRA_HEADERS").map(String::as_str),
            Some(HEADER_REF)
        );
        assert_eq!(
            env.get("MCP_DISCOVERY_EXTRA_HEADERS").map(String::as_str),
            Some(HEADER_REF)
        );
        assert_eq!(
            env.get("HEALTH_LISTEN_ADDR").map(String::as_str),
            Some("127.0.0.1:0")
        );
    }
}
