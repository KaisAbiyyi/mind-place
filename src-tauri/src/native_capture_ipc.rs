//! Desktop-owned localhost IPC receiver for native browser capture.
//! Only Tauri keeps unlocked CryptoVault. Native messages authenticate an
//! ephemeral HMAC session capability and unique nonce; no DEK crosses IPC.
use mind_place_capture_host::ipc::{read_frame, write_json, SessionDescriptor};
use mind_place_index::vault_archive::VaultCaptureStore;
use mind_place_security::{SecurityGate, UnlockLease};
use std::{
    collections::HashSet,
    fs,
    io,
    net::TcpListener,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
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
    ) -> Result<Self, String> {
        security.validate_lease(lease)
            .map_err(|_| "MP_VAULT_IPC_LOCKED".to_owned())?;
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .map_err(|_| "MP_VAULT_IPC_BIND_FAILED".to_owned())?;
        let port = listener.local_addr()
            .map_err(|_| "MP_VAULT_IPC_BIND_FAILED".to_owned())?
            .port();
        listener.set_nonblocking(true)
            .map_err(|_| "MP_VAULT_IPC_BIND_FAILED".to_owned())?;
        let mut token = [0u8; 32];
        getrandom::fill(&mut token)
            .map_err(|_| "MP_VAULT_IPC_ENTROPY_FAILED".to_owned())?;
        let identity = SessionDescriptor::new(port, &token);
        let dir = archive_root.parent()
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
        serde_json::to_writer(pending.as_file_mut(), &identity)
            .map_err(|_| "MP_VAULT_IPC_DESCRIPTOR_FAILED".to_owned())?;
        pending.as_file_mut().sync_all()
            .map_err(|_| "MP_VAULT_IPC_DESCRIPTOR_FAILED".to_owned())?;
        pending.persist(dir.join("capture-ipc-v1.json"))
            .map_err(|_| "MP_VAULT_IPC_DESCRIPTOR_FAILED".to_owned())?;

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
                            security.validate_lease(lease).map_err(|_| ())?;
                            let receipt = writer.save(capture).map_err(|_| ())?;
                            security.validate_lease(lease).map_err(|_| ())?;
                            serde_json::to_value(receipt).map_err(|_| ())
                        })();
                        // Never expose verification, key or filesystem details
                        // through browser native messaging error responses.
                        let response = result.unwrap_or_else(|_| {
                            serde_json::json!({"ok":false,"error":"MP_VAULT_CAPTURE_DENIED"})
                        });
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
