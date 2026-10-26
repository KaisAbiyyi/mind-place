//! Owner-operated outbound-only hosted MCP bridge via the official tunnel-client.
//! Never expose the local MCP port publicly or run a tunnel while locked.
use super::{begin_unlocked, finish_unlocked, AppState};
use mind_place_integration::ClientAuthority;
use mind_place_state::{ClientCapability, StateStore};
use serde::Serialize;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
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
    // Keep the verified Windows image non-writable/non-replaceable while running.
    _binary_guard: File,
    _config: tempfile::NamedTempFile,
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
fn trusted_client_binary() -> Result<(PathBuf, File), String> {
    let app = std::env::current_exe().map_err(|_| "MP_TUNNEL_BINARY:Cannot locate app")?;
    let parent = app
        .parent()
        .ok_or("MP_TUNNEL_BINARY:Cannot locate app directory")?;
    adjacent_client_binary(parent)
}

fn adjacent_client_binary(parent: &Path) -> Result<(PathBuf, File), String> {
    let expected = parent.join(if cfg!(windows) {
        "tunnel-client.exe"
    } else {
        "tunnel-client"
    });
    let actual = expected
        .canonicalize()
        .map_err(|_| "MP_TUNNEL_BINARY:Bundled helper missing; repair or reinstall Mind Place")?;
    let trusted_parent = parent
        .canonicalize()
        .map_err(|_| "MP_TUNNEL_BINARY:Cannot verify app directory")?;
    if actual.parent() != Some(trusted_parent.as_path()) || !actual.is_file() {
        return Err("MP_TUNNEL_BINARY:Untrusted helper path".into());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1); // FILE_SHARE_READ only: no writes/deletes during use.
    }
    let mut file = options
        .open(&actual)
        .map_err(|_| "MP_TUNNEL_BINARY:Cannot open bundled helper; repair Mind Place")?;
    #[cfg(windows)]
    {
        use sha2::{Digest, Sha256};
        let pin: serde_json::Value =
            serde_json::from_str(include_str!("../../scripts/tunnel-client.lock.json"))
                .map_err(|_| "MP_TUNNEL_INTEGRITY:Invalid compiled helper pin")?;
        let expected_size = pin["executable"]["bytes"]
            .as_u64()
            .ok_or("MP_TUNNEL_INTEGRITY:Missing compiled helper size")?;
        let expected_hash = pin["executable"]["sha256"]
            .as_str()
            .ok_or("MP_TUNNEL_INTEGRITY:Missing compiled helper checksum")?;
        let metadata = file
            .metadata()
            .map_err(|_| "MP_TUNNEL_INTEGRITY:Cannot inspect bundled helper")?;
        if !metadata.is_file() || metadata.len() != expected_size {
            return Err(
                "MP_TUNNEL_INTEGRITY:Bundled helper damaged; repair or reinstall Mind Place".into(),
            );
        }
        let mut hasher = Sha256::new();
        let mut buffer = [0_u8; 65536];
        let mut total = 0_u64;
        loop {
            let read = file
                .read(&mut buffer)
                .map_err(|_| "MP_TUNNEL_INTEGRITY:Cannot verify bundled helper")?;
            if read == 0 {
                break;
            }
            total += read as u64;
            if total > expected_size {
                return Err("MP_TUNNEL_INTEGRITY:Bundled helper size changed".into());
            }
            hasher.update(&buffer[..read]);
        }
        if total != expected_size || format!("{:x}", hasher.finalize()) != expected_hash {
            return Err("MP_TUNNEL_INTEGRITY:Bundled helper checksum mismatch; repair or reinstall Mind Place".into());
        }
    }
    Ok((actual, file))
}

fn build_client_command(
    binary: PathBuf,
    config: &Path,
    id: &str,
    api_key: &str,
    credential: &str,
) -> Command {
    // Sensitive values go only into the child environment (and are never echoed).
    // Static MCP headers are resolved ONLY at the local origin by tunnel-client.
    // An upstream-provided Authorization header may override the static header;
    // Phase 3B still authenticates every actual bearer and denies unknown grants.
    let bearer = Zeroizing::new(format!("Bearer {credential}"));
    let mut command = Command::new(binary);
    // Only the application's minimal config may define MCP channels. Inherited
    // user profiles or admin/Harpoon settings must never extend the exposed catalog.
    for name in [
        "TUNNEL_CLIENT_CONFIG",
        "TUNNEL_CLIENT_PROFILE",
        "TUNNEL_CLIENT_PROFILE_FILE",
        "TUNNEL_CLIENT_PROFILE_DIR",
        "CONTROL_PLANE_BASE_URL",
        "CONTROL_PLANE_URL_PATH",
        "CONTROL_PLANE_EXTRA_HEADERS",
        "OPENAI_API_KEY",
        "OPENAI_ADMIN_KEY",
        "MCP_COMMAND",
        "HARPOON_TARGETS",
        "HARPOON_ALLOW_PLAINTEXT_HTTP",
        "CLOUDFLARED_MANAGED",
        "CLOUDFLARED_TUNNEL_TOKEN",
        "CLOUDFLARED_TOKEN",
        "CLOUDFLARED_PATH",
        "HARPOON_ADDITIONAL_TRANSPORTS",
        "HARPOON_CAPTURE_PAYLOADS",
        "HARPOON_HTTP_PROXY",
        "MCP_OAUTH_TRUSTED_ORIGINS",
        "MCP_FORWARD_TRACE_CONTEXT",
        "LOG_FILE",
        "TRACING_FILE",
        "HEALTH_URL_FILE",
    ] {
        command.env_remove(name);
    }
    command
        .arg("run")
        .arg("--config")
        .arg(config)
        .arg("--health.listen-addr=127.0.0.1:0")
        .arg("--log.level=warn")
        .arg("--log.format=struct-text")
        .env("CONTROL_PLANE_TUNNEL_ID", id)
        .env("CONTROL_PLANE_API_KEY", api_key)
        .env("MCP_SERVER_URL", MCP_ORIGIN)
        .env("MCP_EXTRA_HEADERS", HEADER_REF)
        .env("MCP_DISCOVERY_EXTRA_HEADERS", HEADER_REF)
        .env("MIND_PLACE_HOSTED_BEARER", bearer.as_str())
        .env("HEALTH_LISTEN_ADDR", "127.0.0.1:0")
        .env("ALLOW_REMOTE_UI", "false")
        .env("OPEN_WEB_UI", "false")
        .env("LOG_HTTP_RAW_UNSAFE", "false")
        .env("TRACING_ENABLED", "false")
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

fn launch_client(
    id: &str,
    api_key: &str,
    credential: &str,
) -> Result<(Child, tempfile::NamedTempFile, File), String> {
    let (binary, binary_guard) = trusted_client_binary()?;
    // Private temporary file contains NO secrets: settings arrive via child environment.
    // Keeping this handle alive also prevents accidental profile substitution on disk.
    let mut config = tempfile::NamedTempFile::new()
        .map_err(|_| "MP_TUNNEL_CONFIG:Cannot create isolated profile")?;
    config
        .write_all(b"config_version: 1\n")
        .map_err(|_| "MP_TUNNEL_CONFIG:Cannot write isolated profile")?;
    config
        .as_file()
        .sync_all()
        .map_err(|_| "MP_TUNNEL_CONFIG:Cannot sync isolated profile")?;
    let child = build_client_command(binary, config.path(), id, api_key, credential)
        .spawn()
        .map_err(|_| "MP_TUNNEL_SPAWN:Could not start installed tunnel-client".to_string())?;
    Ok((child, config, binary_guard))
}

fn revoke_ephemeral(state: &AppState, client_id: &str) -> Result<(), String> {
    if StateStore::open(&state.state_path)
        .and_then(|store| store.revoke_client(client_id))
        .is_err()
    {
        // The persisted grant is not known to be revoked. Lock the entire
        // reader authority instead of leaving a still-valid hosted token.
        state.authority_ready.store(false, Ordering::Release);
        state.security.lock();
        state.mcp.lock().unwrap_or_else(|e| e.into_inner()).take();
        return Err("MP_TUNNEL_REVOKE:Authority unavailable; Mind Place locked".into());
    }
    Ok(())
}

/// The expected ID prevents an old monitor from closing a newly started tunnel.
/// Drop/kill the child before revoking its credential.
fn stop_client(state: &AppState, expected_id: Option<&str>) -> Result<(), String> {
    let previous = {
        let mut guard = state.hosted.lock().unwrap_or_else(|e| e.into_inner());
        if expected_id
            .is_some_and(|id| guard.as_ref().is_none_or(|current| current.client_id != id))
        {
            return Ok(());
        }
        guard.take()
    };
    if let Some(tunnel) = previous {
        let client_id = tunnel.client_id.clone();
        drop(tunnel);
        revoke_ephemeral(state, &client_id)?;
    }
    Ok(())
}

pub(crate) fn stop_and_revoke(state: &AppState) -> Result<(), String> {
    stop_client(state, None)
}

/// No user status polling is required to revoke a dead helper's credential.
/// A matching client identity is mandatory so a stale monitor cannot kill a
/// later owner-approved session. This never calls the network.
fn watch_helper(state: AppState, expected_id: String) {
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_millis(250));
        let dead = {
            let mut guard = state.hosted.lock().unwrap_or_else(|e| e.into_inner());
            match guard.as_mut() {
                Some(tunnel) if tunnel.client_id == expected_id => !tunnel.running(),
                _ => break,
            }
        };
        if dead {
            let _ = stop_client(&state, Some(&expected_id));
            break;
        }
    });
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
    let dead_id = guard.as_ref().map(|tunnel| tunnel.client_id.clone());
    drop(guard);
    if let Some(id) = dead_id {
        let _ = stop_client(&state, Some(&id));
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
    let (child, config, binary_guard) =
        match launch_client(&tunnel_id, &api_key, issued.credential()) {
            Ok(process) => process,
            Err(error) => {
                let _ = revoke_ephemeral(&state, &client_id);
                return Err(error);
            }
        };
    let tunnel = HostedTunnel {
        child,
        _binary_guard: binary_guard,
        _config: config,
        client_id: client_id.clone(),
        tunnel_id: tunnel_id.clone(),
    };
    if let Err(error) = finish_unlocked(&state, lease) {
        drop(tunnel);
        let _ = revoke_ephemeral(&state, &client_id);
        return Err(error);
    }
    *guard = Some(tunnel);
    drop(guard);
    watch_helper(state.inner().clone(), client_id);
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
    #[cfg(windows)]
    fn phase_3f_rejects_tampered_adjacent_helper() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("tunnel-client.exe"),
            b"not the official executable",
        )
        .unwrap();
        assert!(super::adjacent_client_binary(dir.path()).is_err());
    }
    #[test]
    #[cfg(windows)]
    fn phase_3f_pinned_helper_is_accepted_and_locked_against_replacement() {
        let dir = tempfile::tempdir().unwrap();
        assert!(super::adjacent_client_binary(dir.path()).is_err());
        // Cargo's test executable lives in debug/deps; the real app is in debug.
        let staged = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/debug");
        let (official, _guard) = super::adjacent_client_binary(&staged).unwrap();
        let copy = dir.path().join("tunnel-client.exe");
        std::fs::copy(official, &copy).unwrap();
        let (_, guard) = super::adjacent_client_binary(dir.path()).unwrap();
        let version = std::process::Command::new(&copy)
            .arg("--version")
            .output()
            .unwrap();
        assert!(version.status.success());
        assert!(String::from_utf8_lossy(&version.stdout).contains("0.0.16"));
        assert!(std::fs::OpenOptions::new().write(true).open(&copy).is_err());
        drop(guard);
        // Same-size corruption must be detected by digest, not only metadata.
        let mut altered = std::fs::OpenOptions::new().write(true).open(&copy).unwrap();
        std::io::Write::write_all(&mut altered, b"bad-image").unwrap();
        drop(altered);
        assert!(super::adjacent_client_binary(dir.path()).is_err());
    }
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
            std::path::Path::new("isolated-runtime.yaml"),
            &format!("tunnel_{}", "a".repeat(32)),
            key,
            credential,
        );
        let argv = command
            .get_args()
            .map(|s| s.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        assert_eq!(argv[0], "run");
        assert_eq!(argv[1], "--config");
        assert_eq!(argv[2], "isolated-runtime.yaml");
        // Bundled tunnel-client v0.0.16 exits before polling if warn level
        // is supplied without an explicit supported log format.
        assert!(argv.contains(&"--log.level=warn".to_string()));
        assert!(argv.contains(&"--log.format=struct-text".to_string()));
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
        for denied in [
            "OPENAI_ADMIN_KEY",
            "HARPOON_TARGETS",
            "TUNNEL_CLIENT_PROFILE",
        ] {
            assert!(command
                .get_envs()
                .any(|(key, value)| key == denied && value.is_none()));
        }
    }
    #[test]
    #[ignore = "Only launched as a short-lived subprocess fixture"]
    fn phase_3e_exited_mock_helper() {
        // Intentionally exits at once. Does not connect to OpenAI or bind a socket.
    }

    #[test]
    fn phase_3e_detects_unexpected_helper_exit_without_reporting_remote_connection() {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "hosted_tunnel::tests::phase_3e_exited_mock_helper",
                "--ignored",
            ])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let mut hosted = super::HostedTunnel {
            child,
            _binary_guard: std::fs::File::open(std::env::current_exe().unwrap()).unwrap(),
            _config: tempfile::NamedTempFile::new().unwrap(),
            client_id: "client:v1:simulated".into(),
            tunnel_id: format!("tunnel_{}", "a".repeat(32)),
        };
        assert!(hosted.child.wait().unwrap().success());
        assert!(
            !hosted.running(),
            "dead helper must be reaped by status polling"
        );
        let status = super::stopped();
        assert!(!status.running);
        assert!(!status.connection_verified);
        assert!(status.tunnel_id.is_none());
        // Drop also reaps/kills the child and cannot restart it.
        drop(hosted);
    }

    #[test]
    fn phase_3e_watchdog_revokes_crashed_helpers_read_identity_without_ui_poll() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("state.sqlite3");
        let db = mind_place_state::StateStore::open(&state_path).unwrap();
        let security =
            mind_place_security::SecurityGate::open(dir.path().join("security.json")).unwrap();
        security.initialize(b"phase-3e-test-secret").unwrap();
        let issued = mind_place_integration::ClientAuthority::new(&db, &security)
            .issue(
                "Crash-only hosted identity",
                &[mind_place_state::ClientCapability::Read],
            )
            .unwrap();
        let id = issued.client.id.clone();
        let credential = issued.credential().to_owned();
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "hosted_tunnel::tests::phase_3e_exited_mock_helper",
                "--ignored",
            ])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let state = super::AppState {
            store: Some(
                mind_place_archive::ArchiveStore::open(dir.path().join("archive-v1")).unwrap(),
            ),
            archive_root: dir.path().join("archive-v1"),
            vault_root: dir.path().join("encrypted-vault-v1"),
            pending_vault: std::sync::Arc::new(std::sync::Mutex::new(None)),
            vault: std::sync::Arc::new(std::sync::Mutex::new(None)),
            native_capture: std::sync::Arc::new(std::sync::Mutex::new(None)),
            security: security.clone(),
            state_path,
            index_path: dir.path().join("index.sqlite3"),
            archive_index_write_gate: std::sync::Arc::new(std::sync::Mutex::new(())),
            authority_ready: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)),
            document_authority_gate: std::sync::Arc::new(std::sync::Mutex::new(())),
            document_refresh: std::sync::Arc::new(crate::DocumentRefreshRegistry::default()),
            mcp: std::sync::Arc::new(std::sync::Mutex::new(None)),
            hosted: std::sync::Arc::new(std::sync::Mutex::new(Some(super::HostedTunnel {
                child,
                _binary_guard: std::fs::File::open(std::env::current_exe().unwrap()).unwrap(),
                _config: tempfile::NamedTempFile::new().unwrap(),
                client_id: id,
                tunnel_id: format!("tunnel_{}", "c".repeat(32)),
            }))),
            extension_path: dir.path().join("extension"),
            host_path: dir.path().join("host"),
            legacy_path: dir.path().join("legacy"),
        };
        super::watch_helper(state.clone(), issued.client.id.clone());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            let stopped = state.hosted.lock().unwrap().is_none();
            let revoked = mind_place_integration::ClientAuthority::new(&db, &security)
                .authorize(&credential, mind_place_state::ClientCapability::Read)
                .is_err();
            if stopped && revoked {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(
            state.hosted.lock().unwrap().is_none(),
            "crashed helper was not reaped"
        );
        assert!(
            mind_place_integration::ClientAuthority::new(&db, &security)
                .authorize(&credential, mind_place_state::ClientCapability::Read)
                .is_err(),
            "orphan hosted credential retained read access after helper exit"
        );
    }

    #[test]
    fn phase_3e_helper_cannot_inherit_admin_or_public_forwarding_configuration() {
        let command = super::build_client_command(
            std::path::PathBuf::from("tunnel-client"),
            std::path::Path::new("isolated.yaml"),
            &format!("tunnel_{}", "b".repeat(32)),
            "sk-proj_example_runtime_token",
            "mpc_v1_test_only",
        );
        let removed = [
            "OPENAI_ADMIN_KEY",
            "CONTROL_PLANE_EXTRA_HEADERS",
            "CONTROL_PLANE_BASE_URL",
            "TUNNEL_CLIENT_CONFIG",
            "TUNNEL_CLIENT_PROFILE",
            "TUNNEL_CLIENT_PROFILE_DIR",
            "MCP_COMMAND",
            "HARPOON_TARGETS",
            "HARPOON_ALLOW_PLAINTEXT_HTTP",
            "HARPOON_ADDITIONAL_TRANSPORTS",
            "HARPOON_CAPTURE_PAYLOADS",
            "MCP_OAUTH_TRUSTED_ORIGINS",
            "LOG_FILE",
            "TRACING_FILE",
            "CLOUDFLARED_TUNNEL_TOKEN",
        ];
        for name in removed {
            assert!(
                command
                    .get_envs()
                    .any(|(key, value)| key == name && value.is_none()),
                "unsafe inherited variable not explicitly removed: {name}"
            );
        }
        let argv = command
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        assert!(argv
            .iter()
            .all(|part| !part.starts_with("http://") && !part.starts_with("https://")));
        assert!(command.get_envs().any(|(key, value)| {
            key == "MCP_SERVER_URL" && value == Some(std::ffi::OsStr::new(super::MCP_ORIGIN))
        }));
        for name in [
            "ALLOW_REMOTE_UI",
            "OPEN_WEB_UI",
            "LOG_HTTP_RAW_UNSAFE",
            "TRACING_ENABLED",
        ] {
            assert!(command.get_envs().any(|(key, value)| {
                key == name && value == Some(std::ffi::OsStr::new("false"))
            }));
        }
    }
}
