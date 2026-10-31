//! Desktop-owned localhost IPC receiver for native browser capture.
//! Only Tauri keeps unlocked CryptoVault. Native messages authenticate an
//! ephemeral HMAC session capability and unique nonce; no DEK crosses IPC.
use mind_place_capture_host::ipc::{read_frame_with_timeout, write_json, SessionDescriptor};
use mind_place_index::vault_archive::VaultCaptureStore;
use mind_place_security::{SecurityGate, UnlockLease};
use std::{
    collections::HashSet,
    fs, io,
    net::{Shutdown, TcpListener, TcpStream},
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
    active_stream: Arc<Mutex<Option<TcpStream>>>,
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
        #[cfg(windows)]
        mind_place_capture_host::ipc::verify_descriptor_file(pending.as_file())
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
        let published_identity = SessionDescriptor::read(&archive_root)
            .map_err(|_| "MP_VAULT_IPC_OWNER_DACL_FAILED".to_owned())?;
        if published_identity != identity {
            return Err("MP_VAULT_IPC_PUBLISHED_IDENTITY_CHANGED".to_owned());
        }

        let stop = Arc::new(AtomicBool::new(false));
        let terminate = stop.clone();
        let active_stream = Arc::new(Mutex::new(None));
        let worker_active = active_stream.clone();
        let thread = thread::spawn(move || {
            let mut seen = HashSet::new();
            while !terminate.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        if let Ok(active) = stream.try_clone() {
                            *worker_active
                                .lock()
                                .unwrap_or_else(|error| error.into_inner()) = Some(active);
                        }
                        if terminate.load(Ordering::Acquire) {
                            let _ = stream.shutdown(Shutdown::Both);
                            *worker_active
                                .lock()
                                .unwrap_or_else(|error| error.into_inner()) = None;
                            continue;
                        }
                        let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
                        let result = (|| -> Result<serde_json::Value, ()> {
                            security.validate_lease(lease).map_err(|_| ())?;
                            let signed = read_frame_with_timeout(
                                &mut stream,
                                Duration::from_secs(5),
                            )
                            .map_err(|_| ())?;
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
                        *worker_active
                            .lock()
                            .unwrap_or_else(|error| error.into_inner()) = None;
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
            active_stream,
        })
    }
}

impl Drop for NativeCaptureIpc {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        {
            let active = self
                .active_stream
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if let Some(stream) = active.as_ref() {
                let _ = stream.shutdown(Shutdown::Both);
            }
        }
        if let Some(worker) = self.thread.take() {
            let _ = worker.join();
        }
        // A pinned native reader can briefly deny Windows DeleteFile. Retry
        // only our exact identity, with a bounded delay; never unlink a newer
        // session or force permissions on a changed/foreign descriptor.
        for _ in 0..50 {
            let Ok(current) = SessionDescriptor::read(&self.archive_root) else {
                break;
            };
            if current != self.identity {
                break;
            }
            let Some(parent) = self.archive_root.parent() else {
                break;
            };
            match fs::remove_file(parent.join("capture-ipc-v1.json")) {
                Ok(()) => break,
                Err(error) if matches!(error.raw_os_error(), Some(5 | 32 | 33)) => {
                    thread::sleep(Duration::from_millis(10))
                }
                Err(_) => break,
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

    #[test]
    fn slow_drip_header_uses_one_total_deadline() {
        use std::{io::Write, time::Instant};

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let sender = thread::spawn(move || {
            let mut client = TcpStream::connect(address).unwrap();
            for byte in [1u8, 0, 0, 0] {
                if client.write_all(&[byte]).is_err() {
                    break;
                }
                thread::sleep(Duration::from_millis(60));
            }
        });
        let (mut server, _) = listener.accept().unwrap();
        let started = Instant::now();
        assert!(read_frame_with_timeout(&mut server, Duration::from_millis(130)).is_err());
        assert!(started.elapsed() < Duration::from_millis(400));
        drop(server);
        let _ = sender.join();
    }

    #[test]
    fn lock_cancels_a_slow_partial_client_before_receiver_join() {
        use std::{io::Write, time::Instant};

        let home = tempfile::tempdir().unwrap();
        let gate = SecurityGate::open(home.path().join("security.json")).unwrap();
        gate.initialize(b"synthetic-slow-client-lock").unwrap();
        let lease = gate.require_unlocked().unwrap();
        let (vault, _, manifest) = CryptoVault::provision(&gate, lease).unwrap();
        let vault_root = home.path().join("vault");
        manifest.persist_new(&vault_root).unwrap();
        let writer = Arc::new(VaultCaptureStore::open(&vault_root, vault, &gate, lease).unwrap());
        let archive_root = home.path().join("archive-v1");
        let ipc = NativeCaptureIpc::start(
            archive_root.clone(),
            writer,
            gate.clone(),
            lease,
            Arc::new(Mutex::new(())),
        )
        .unwrap();
        let descriptor = SessionDescriptor::read(&archive_root).unwrap();
        let mut client = TcpStream::connect(("127.0.0.1", descriptor.port)).unwrap();
        client.write_all(&[1]).unwrap();

        let mut accepted = false;
        for _ in 0..100 {
            accepted = ipc
                .active_stream
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .is_some();
            if accepted {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        assert!(accepted, "receiver must have accepted the partial client");
        thread::sleep(Duration::from_millis(50));
        gate.lock();
        let started = Instant::now();
        drop(ipc);
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "Lock must cancel the active reader instead of waiting for a peer timeout"
        );
        drop(client);
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
        if matches!(
            mode.as_str(),
            "forged" | "replay" | "old-session" | "oversize"
        ) {
            use mind_place_capture_host::ipc::SignedCapture;
            use std::io::{Read, Write};
            let root = std::path::Path::new(&root);
            let descriptor = SessionDescriptor::read(root).unwrap();
            let send = |payload: &[u8], oversize: bool| {
                let mut stream =
                    std::net::TcpStream::connect(("127.0.0.1", descriptor.port)).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                stream
                    .write_all(
                        &(if oversize {
                            u32::MAX
                        } else {
                            payload.len() as u32
                        })
                        .to_le_bytes(),
                    )
                    .unwrap();
                if !oversize {
                    stream.write_all(payload).unwrap();
                }
                let mut size = [0u8; 4];
                stream.read_exact(&mut size).unwrap();
                let size = u32::from_le_bytes(size) as usize;
                assert!(size > 0 && size <= 1024 * 1024);
                let mut bytes = vec![0u8; size];
                stream.read_exact(&mut bytes).unwrap();
                serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["ok"] == true
            };
            let payload = if mode == "old-session" {
                fs::read(root.parent().unwrap().join("synthetic-old-frame.json")).unwrap()
            } else {
                let key = if mode == "forged" {
                    [0u8; 32]
                } else {
                    descriptor.key().unwrap()
                };
                serde_json::to_vec(
                    &SignedCapture::prepare(
                        &key,
                        &format!("chrome-extension://{}/", extension_id()),
                        synthetic_capture(),
                    )
                    .unwrap(),
                )
                .unwrap()
            };
            if mode == "replay" {
                assert!(send(&payload, false));
            }
            assert!(!send(&payload, mode == "oversize"));
            return;
        }
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
        let transcript = String::from_utf8_lossy(&output.stdout);
        // A mistyped --exact filter exits successfully after running *zero*
        // tests. Require the child probe itself to execute before accepting it.
        assert!(
            output.status.success()
                && transcript.contains("separate_process_capture_probe ... ok")
                && transcript.contains("1 passed; 0 failed"),
            "hostile IPC child probe missing or failed: {transcript}"
        );
    }

    #[test]
    fn owner_local_capture_roundtrip_requires_unlocked_epoch() {
        let home = tempfile::tempdir().unwrap();
        let archive_root = home.path().join("archive-v1");
        let gate = SecurityGate::open(home.path().join("security-v1.json")).unwrap();
        gate.initialize(b"synthetic-owner-session-only").unwrap();
        let lease = gate.require_unlocked().unwrap();
        let (vault, recovery, manifest) = CryptoVault::provision(&gate, lease).unwrap();
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
        run_process_probe(&archive_root, "forged");
        run_process_probe(&archive_root, "replay");
        run_process_probe(&archive_root, "oversize");
        let old = SessionDescriptor::read(&archive_root).unwrap();
        let signed = mind_place_capture_host::ipc::SignedCapture::prepare(
            &old.key().unwrap(),
            &origin,
            synthetic_capture(),
        )
        .unwrap();
        fs::write(
            home.path().join("synthetic-old-frame.json"),
            serde_json::to_vec(&signed).unwrap(),
        )
        .unwrap();
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
        gate.unlock(b"synthetic-owner-session-only").unwrap();
        let lease = gate.require_unlocked().unwrap();
        let vault = CryptoVault::restore_with_recovery(&gate, lease, &manifest, &recovery).unwrap();
        let writer = Arc::new(VaultCaptureStore::open(&vault_root, vault, &gate, lease).unwrap());
        let next = NativeCaptureIpc::start(
            archive_root.clone(),
            writer,
            gate.clone(),
            lease,
            Arc::new(Mutex::new(())),
        )
        .unwrap();
        assert_ne!(
            SessionDescriptor::read(&archive_root)
                .unwrap()
                .session_token,
            old.session_token
        );
        run_process_probe(&archive_root, "old-session");
        run_process_probe(&archive_root, "allowed");
        drop(next);
    }
}
