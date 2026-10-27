//! Desktop-owned localhost IPC receiver for native browser capture.
//! Only Tauri keeps unlocked CryptoVault. Native messages authenticate an
//! ephemeral HMAC session capability and unique nonce; no DEK crosses IPC.
use mind_place_capture_host::ipc::{read_frame, write_json, SessionDescriptor};
use mind_place_index::vault_archive::VaultCaptureStore;
use mind_place_security::{SecurityGate, UnlockLease};
use std::{
    collections::HashSet,
    fs, io,
    net::TcpListener,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

pub(super) struct NativeCaptureIpc {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    archive_root: PathBuf,
    identity: SessionDescriptor,
}

impl NativeCaptureIpc {
    pub(super) fn start(
        archive_root: PathBuf,
        writer: Arc<VaultCaptureStore>,
        security: SecurityGate,
        lease: UnlockLease,
        commit_gate: Arc<Mutex<()>>,
    ) -> Result<Self, String> {
        security
            .validate_lease(lease)
            .map_err(|_| "MP_VAULT_IPC_LOCKED".to_owned())?;
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .map_err(|_| "MP_VAULT_IPC_BIND_FAILED".to_owned())?;
        let port = listener
            .local_addr()
            .map_err(|_| "MP_VAULT_IPC_BIND_FAILED".to_owned())?
            .port();
        listener
            .set_nonblocking(true)
            .map_err(|_| "MP_VAULT_IPC_BIND_FAILED".to_owned())?;
        let mut token = [0u8; 32];
        getrandom::fill(&mut token).map_err(|_| "MP_VAULT_IPC_ENTROPY_FAILED".to_owned())?;
        let identity = SessionDescriptor::new(port, &token);
        let dir = archive_root
            .parent()
            .ok_or_else(|| "MP_VAULT_IPC_ROOT_UNAVAILABLE".to_owned())?;
        // The session descriptor contains an ephemeral auth capability, NOT
        // the vault DEK. It lives only in the user's application-data root.
        let mut pending = tempfile::NamedTempFile::new_in(dir)
            .map_err(|_| "MP_VAULT_IPC_DESCRIPTOR_FAILED".to_owned())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(pending.path(), fs::Permissions::from_mode(0o600))
                .map_err(|_| "MP_VAULT_IPC_DESCRIPTOR_FAILED".to_owned())?;
        }
        #[cfg(windows)]
        mind_place_capture_host::ipc::secure_descriptor(pending.path())
            .map_err(|_| "MP_VAULT_IPC_OWNER_DACL_FAILED".to_owned())?;
        serde_json::to_writer(pending.as_file_mut(), &identity)
            .map_err(|_| "MP_VAULT_IPC_DESCRIPTOR_FAILED".to_owned())?;
        pending
            .as_file_mut()
            .sync_all()
            .map_err(|_| "MP_VAULT_IPC_DESCRIPTOR_FAILED".to_owned())?;
        // Never replace an existing session capability. A second desktop
        // instance must fail closed rather than hijack capture routing from
        // another unlocked owner session. A stale post-crash descriptor also
        // requires explicit owner recovery rather than unsafe auto-takeover.
        let published = pending
            .persist_noclobber(dir.join("capture-ipc-v1.json"))
            .map_err(|_| "MP_VAULT_IPC_SESSION_ALREADY_PRESENT".to_owned())?;
        drop(published);
        #[cfg(windows)]
        mind_place_capture_host::ipc::verify_descriptor(&dir.join("capture-ipc-v1.json"))
            .map_err(|_| "MP_VAULT_IPC_OWNER_DACL_FAILED".to_owned())?;

        let stop = Arc::new(AtomicBool::new(false));
        let terminate = stop.clone();
        let thread = thread::spawn(move || {
            let mut seen = HashSet::new();
            while !terminate.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                        let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
                        let result = (|| -> Result<serde_json::Value, ()> {
                            security.validate_lease(lease).map_err(|_| ())?;
                            let signed = read_frame(&mut stream).map_err(|_| ())?;
                            let capture = signed.authenticate(&token, &mut seen).map_err(|_| ())?;
                            // Lock and owner-state grants acquire this same
                            // gate, so no IPC capture can commit *after* lock
                            // returns. Queued requests fail epoch validation.
                            let _owner_commit = commit_gate
                                .lock()
                                .unwrap_or_else(|error| error.into_inner());
                            security.validate_lease(lease).map_err(|_| ())?;
                            let receipt = writer.save(capture).map_err(|_| ())?;
                            security.validate_lease(lease).map_err(|_| ())?;
                            serde_json::to_value(receipt).map_err(|_| ())
                        })();
                        // Never expose verification, key or filesystem details
                        // through browser native messaging error responses.
                        let response = result.unwrap_or_else(
                            |_| serde_json::json!({"ok":false,"error":"MP_VAULT_CAPTURE_DENIED"}),
                        );
                        let _ = write_json(&mut stream, &response);
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(25));
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            stop,
            thread: Some(thread),
            archive_root,
            identity,
        })
    }
}

impl Drop for NativeCaptureIpc {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.thread.take() {
            let _ = worker.join();
        }
        // Do not unlink another instance's newer session descriptor.
        if let Ok(current) = SessionDescriptor::read(&self.archive_root) {
            if current.port == self.identity.port
                && current.session_token == self.identity.session_token
            {
                if let Some(parent) = self.archive_root.parent() {
                    let _ = fs::remove_file(parent.join("capture-ipc-v1.json"));
                }
            }
        }
    }
}

#[cfg(test)]
mod qualification {
    use super::*;
    use mind_place_archive::{extension_id, Capture};
    use mind_place_security::vault::CryptoVault;

    fn synthetic_capture() -> Capture {
        serde_json::from_value(serde_json::json!({
            "schema_version": 1,
            "provider": "chatgpt",
            "conversation_id": "123e4567-e89b-12d3-a456-426614174000",
            "source_url": "https://chatgpt.com/c/123e4567-e89b-12d3-a456-426614174000",
            "title": "Synthetic Native IPC",
            "captured_at": "2026-10-26T11:00:00Z",
            "capture_method": "manual-dom",
            "completeness": "partial",
            "warnings": ["synthetic"],
            "messages": [{
                "id": "ipc-1",
                "role": "user",
                "text": "synthetic-only",
                "format": "plain",
                "attachments": []
            }]
        }))
        .unwrap()
    }

    /// Real process boundary (not a thread/mock): the native-host equivalent
    /// is launched as a second test process on hosted Windows and Ubuntu.
    /// A same-owner process is intentionally authorized only while unlocked;
    /// a wrong extension origin and revoked epoch must both fail closed.
    #[test]
    fn separate_process_capture_probe() {
        let Some(root) = std::env::var_os("MP9B_SYNTHETIC_IPC_ROOT") else {
            return;
        };
        let mode = std::env::var("MP9B_SYNTHETIC_IPC_MODE").unwrap();
        let origin = if mode == "wrong-origin" {
            "chrome-extension://invalid/".to_owned()
        } else {
            format!("chrome-extension://{}/", extension_id())
        };
        let result = mind_place_capture_host::ipc::forward_capture(
            std::path::Path::new(&root),
            &origin,
            synthetic_capture(),
        );
        assert_eq!(result.is_ok(), mode == "allowed");
    }

    fn run_process_probe(root: &std::path::Path, mode: &str) {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("native_capture_ipc::qualification::separate_process_capture_probe")
            .arg("--nocapture")
            .env("MP9B_SYNTHETIC_IPC_ROOT", root)
            .env("MP9B_SYNTHETIC_IPC_MODE", mode)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "hostile IPC child probe failed: {}",
            String::from_utf8_lossy(&output.stdout)
        );
    }

    #[test]
    fn owner_local_capture_roundtrip_requires_unlocked_epoch() {
        let home = tempfile::tempdir().unwrap();
        let archive_root = home.path().join("archive-v1");
        let gate = SecurityGate::open(home.path().join("security-v1.json")).unwrap();
        gate.initialize(b"synthetic-owner-session-only").unwrap();
        let lease = gate.require_unlocked().unwrap();
        let (vault, _, manifest) = CryptoVault::provision(&gate, lease).unwrap();
        let vault_root = home.path().join("encrypted-vault-v1");
        manifest.persist_new(&vault_root).unwrap();
        let writer = Arc::new(VaultCaptureStore::open(&vault_root, vault, &gate, lease).unwrap());
        let ipc = NativeCaptureIpc::start(
            archive_root.clone(),
            writer.clone(),
            gate.clone(),
            lease,
            Arc::new(Mutex::new(())),
        )
        .unwrap();
        let origin = format!("chrome-extension://{}/", extension_id());
        let result = mind_place_capture_host::ipc::forward_capture(
            &archive_root,
            &origin,
            synthetic_capture(),
        )
        .unwrap();
        assert_eq!(result["ok"], true);
        let archive_id = result["id"].as_str().unwrap();
        assert_eq!(
            writer.read(archive_id, None).unwrap().capture.messages[0].text,
            "synthetic-only"
        );
        assert!(!archive_root.exists());
        run_process_probe(&archive_root, "allowed");
        run_process_probe(&archive_root, "wrong-origin");
        // An overlapping owner desktop must never overwrite the active token.
        // It cannot silently change which vault receives browser captures.
        let original = fs::read(home.path().join("capture-ipc-v1.json")).unwrap();
        assert!(NativeCaptureIpc::start(
            archive_root.clone(),
            writer.clone(),
            gate.clone(),
            lease,
            Arc::new(Mutex::new(())),
        )
        .is_err());
        assert_eq!(
            fs::read(home.path().join("capture-ipc-v1.json")).unwrap(),
            original
        );
        gate.lock();
        run_process_probe(&archive_root, "locked");
        assert!(mind_place_capture_host::ipc::forward_capture(
            &archive_root,
            &origin,
            synthetic_capture(),
        )
        .is_err());
        drop(ipc);
        assert!(!home.path().join("capture-ipc-v1.json").exists());
    }
}
