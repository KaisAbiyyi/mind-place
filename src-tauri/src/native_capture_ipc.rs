//! Desktop-owned localhost IPC receiver for native browser capture.
//! Only Tauri keeps unlocked CryptoVault. Native messages authenticate an
//! ephemeral HMAC session capability and unique nonce; no DEK crosses IPC.
use mind_place_archive::MAX_CAPTURE_BYTES;
#[cfg(test)]
use mind_place_capture_host::ipc::read_frame_with_timeout;
use mind_place_capture_host::ipc::{SessionDescriptor, SignedCapture};
use mind_place_index::vault_archive::VaultCaptureStore;
use mind_place_security::{SecurityGate, UnlockLease};
#[cfg(unix)]
use std::os::fd::AsRawFd;
#[cfg(windows)]
use std::os::windows::io::AsRawSocket;
use std::{
    collections::HashSet,
    fs,
    io::{self, Read, Write},
    net::{Shutdown, TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub(super) struct NativeCaptureIpc {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    archive_root: PathBuf,
    identity: SessionDescriptor,
    active_stream: Arc<Mutex<Option<TcpStream>>>,
    // A loopback wake socket interrupts readiness waits without changing the
    // accepted client socket's blocking mode.
    wake_tx: TcpStream,
    // Test-only notifications from the real receiver, not fabricated accepts.
    #[cfg(test)]
    accepted_clients: Arc<(Mutex<u64>, std::sync::Condvar)>,
    #[cfg(test)]
    reader_started: Arc<(Mutex<bool>, std::sync::Condvar)>,
    #[cfg(test)]
    fail_next_clone: Arc<AtomicBool>,
    #[cfg(test)]
    clone_failure_seen: Arc<(Mutex<bool>, std::sync::Condvar)>,
    #[cfg(test)]
    force_shutdown_error: Arc<AtomicBool>,
    #[cfg(test)]
    worker_trace: Arc<Mutex<WorkerTrace>>,
}

#[cfg(test)]
#[derive(Clone, Default)]
struct WorkerTrace {
    read_started: Option<Instant>,
    read_finished: Option<Instant>,
    processing_finished: Option<Instant>,
    write_started: Option<Instant>,
    write_finished: Option<Instant>,
    active_cleared: Option<Instant>,
    exit_at: Option<Instant>,
}

#[derive(Clone, Copy)]
enum SocketInterest {
    Readable,
    Writable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SocketWait {
    Ready,
    Cancelled,
    TimedOut,
}

fn cancellation_pair() -> io::Result<(TcpStream, TcpStream)> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let sender = TcpStream::connect(listener.local_addr()?)?;
    let (receiver, _) = listener.accept()?;
    sender.set_nonblocking(true)?;
    receiver.set_nonblocking(false)?;
    Ok((sender, receiver))
}

fn poll_timeout_ms(deadline: Instant) -> i32 {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return 0;
    }
    let fractional_millisecond = remaining.subsec_nanos() % 1_000_000 != 0;
    let milliseconds = remaining.as_millis() + if fractional_millisecond { 1 } else { 0 };
    milliseconds.min(i32::MAX as u128) as i32
}

fn wait_for_socket_ready(
    stream: &TcpStream,
    wake: &TcpStream,
    interest: SocketInterest,
    deadline: Instant,
    stop: &AtomicBool,
) -> io::Result<SocketWait> {
    loop {
        if stop.load(Ordering::Acquire) {
            return Ok(SocketWait::Cancelled);
        }
        if Instant::now() >= deadline {
            return Ok(SocketWait::TimedOut);
        }
        let timeout_ms = poll_timeout_ms(deadline);
        #[cfg(unix)]
        {
            let target_events = match interest {
                SocketInterest::Readable => libc::POLLIN,
                SocketInterest::Writable => libc::POLLOUT,
            };
            let mut descriptors = [
                libc::pollfd {
                    fd: stream.as_raw_fd(),
                    events: target_events,
                    revents: 0,
                },
                libc::pollfd {
                    fd: wake.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                },
            ];
            let ready =
                unsafe { libc::poll(descriptors.as_mut_ptr(), descriptors.len() as _, timeout_ms) };
            if ready < 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(error);
            }
            if ready == 0 {
                continue;
            }
            if descriptors[1].revents != 0 || stop.load(Ordering::Acquire) {
                return Ok(SocketWait::Cancelled);
            }
            if descriptors[0].revents != 0 {
                return Ok(SocketWait::Ready);
            }
        }
        #[cfg(windows)]
        {
            use windows_sys::Win32::Networking::WinSock::{
                WSAGetLastError, WSAPoll, POLLRDNORM, POLLWRNORM, WSAPOLLFD,
            };
            let target_events = match interest {
                SocketInterest::Readable => POLLRDNORM,
                SocketInterest::Writable => POLLWRNORM,
            };
            let mut descriptors = [
                WSAPOLLFD {
                    fd: stream.as_raw_socket() as usize,
                    events: target_events,
                    revents: 0,
                },
                WSAPOLLFD {
                    fd: wake.as_raw_socket() as usize,
                    events: POLLRDNORM,
                    revents: 0,
                },
            ];
            let ready = unsafe {
                WSAPoll(
                    descriptors.as_mut_ptr(),
                    descriptors.len() as u32,
                    timeout_ms,
                )
            };
            if ready < 0 {
                let error = io::Error::from_raw_os_error(unsafe { WSAGetLastError() });
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(error);
            }
            if ready == 0 {
                continue;
            }
            if descriptors[1].revents != 0 || stop.load(Ordering::Acquire) {
                return Ok(SocketWait::Cancelled);
            }
            if descriptors[0].revents != 0 {
                return Ok(SocketWait::Ready);
            }
        }
    }
}

fn read_exact_cancellable(
    stream: &mut TcpStream,
    wake: &TcpStream,
    buffer: &mut [u8],
    deadline: Instant,
    stop: &AtomicBool,
) -> io::Result<()> {
    let mut offset = 0;
    while offset < buffer.len() {
        match wait_for_socket_ready(stream, wake, SocketInterest::Readable, deadline, stop)? {
            SocketWait::Ready => {}
            SocketWait::Cancelled => {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "native IPC receiver cancelled",
                ));
            }
            SocketWait::TimedOut => {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "native IPC frame deadline exceeded",
                ));
            }
        }
        if stop.load(Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "native IPC receiver cancelled",
            ));
        }
        match stream.read(&mut buffer[offset..]) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "native IPC client disconnected",
                ));
            }
            Ok(read) => offset += read,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
                ) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn read_signed_capture_cancellable(
    stream: &mut TcpStream,
    wake: &TcpStream,
    timeout: Duration,
    stop: &AtomicBool,
) -> Result<SignedCapture, ()> {
    let deadline = Instant::now().checked_add(timeout).ok_or(())?;
    let mut size = [0u8; 4];
    read_exact_cancellable(stream, wake, &mut size, deadline, stop).map_err(|_| ())?;
    let length = u32::from_le_bytes(size) as usize;
    if length == 0 || length > MAX_CAPTURE_BYTES + 8192 {
        return Err(());
    }
    let mut bytes = vec![0u8; length];
    read_exact_cancellable(stream, wake, &mut bytes, deadline, stop).map_err(|_| ())?;
    serde_json::from_slice(&bytes).map_err(|_| ())
}

fn write_all_cancellable(
    stream: &mut TcpStream,
    wake: &TcpStream,
    buffer: &[u8],
    deadline: Instant,
    stop: &AtomicBool,
) -> io::Result<()> {
    let mut offset = 0;
    while offset < buffer.len() {
        match wait_for_socket_ready(stream, wake, SocketInterest::Writable, deadline, stop)? {
            SocketWait::Ready => {}
            SocketWait::Cancelled => {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "native IPC response cancelled",
                ));
            }
            SocketWait::TimedOut => {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "native IPC response deadline exceeded",
                ));
            }
        }
        if stop.load(Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "native IPC response cancelled",
            ));
        }
        match stream.write(&buffer[offset..]) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "native IPC response socket closed",
                ));
            }
            Ok(written) => offset += written,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
                ) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn write_json_cancellable(
    stream: &mut TcpStream,
    wake: &TcpStream,
    value: &serde_json::Value,
    timeout: Duration,
    stop: &AtomicBool,
) -> Result<(), ()> {
    let bytes = serde_json::to_vec(value).map_err(|_| ())?;
    if bytes.len() > 1024 * 1024 {
        return Err(());
    }
    let deadline = Instant::now().checked_add(timeout).ok_or(())?;
    write_all_cancellable(
        stream,
        wake,
        &(bytes.len() as u32).to_le_bytes(),
        deadline,
        stop,
    )
    .map_err(|_| ())?;
    write_all_cancellable(stream, wake, &bytes, deadline, stop).map_err(|_| ())?;
    stream.flush().map_err(|_| ())?;
    Ok(())
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
        let (wake_tx, wake_rx) =
            cancellation_pair().map_err(|_| "MP_VAULT_IPC_CANCEL_FAILED".to_owned())?;
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
        #[cfg(test)]
        let accepted_clients = Arc::new((Mutex::new(0_u64), std::sync::Condvar::new()));
        #[cfg(test)]
        let worker_accepted = accepted_clients.clone();
        #[cfg(test)]
        let reader_started = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        #[cfg(test)]
        let worker_reader_started = reader_started.clone();
        #[cfg(test)]
        let fail_next_clone = Arc::new(AtomicBool::new(false));
        #[cfg(test)]
        let worker_fail_next_clone = fail_next_clone.clone();
        #[cfg(test)]
        let clone_failure_seen = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        #[cfg(test)]
        let worker_clone_failure_seen = clone_failure_seen.clone();
        #[cfg(test)]
        let force_shutdown_error = Arc::new(AtomicBool::new(false));
        #[cfg(test)]
        let worker_trace = Arc::new(Mutex::new(WorkerTrace::default()));
        #[cfg(test)]
        let worker_trace_worker = worker_trace.clone();
        let thread = thread::spawn(move || {
            let mut seen = HashSet::new();
            while !terminate.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        // Winsock accept inherits the listener's nonblocking mode.
                        // The frame reader requires blocking I/O to honor its
                        // bounded read deadline on both Windows and Unix.
                        if stream.set_nonblocking(false).is_err() {
                            let _ = stream.shutdown(Shutdown::Both);
                            continue;
                        }
                        // Never read from a socket that Drop cannot cancel.
                        // A failed clone must deny this client, not leave an
                        // untracked five-second read blocking receiver shutdown.
                        #[cfg(test)]
                        let clone_result = if worker_fail_next_clone.swap(false, Ordering::AcqRel) {
                            let (seen, ready) = &*worker_clone_failure_seen;
                            *seen.lock().unwrap_or_else(|error| error.into_inner()) = true;
                            ready.notify_all();
                            Err(io::Error::new(
                                io::ErrorKind::Other,
                                "injected native IPC cancellation clone failure",
                            ))
                        } else {
                            stream.try_clone()
                        };
                        #[cfg(not(test))]
                        let clone_result = stream.try_clone();
                        let active = match clone_result {
                            Ok(active) => active,
                            Err(_) => {
                                let _ = stream.shutdown(Shutdown::Both);
                                continue;
                            }
                        };
                        *worker_active
                            .lock()
                            .unwrap_or_else(|error| error.into_inner()) = Some(active);
                        #[cfg(test)]
                        {
                            let (count, ready) = &*worker_accepted;
                            *count.lock().unwrap_or_else(|error| error.into_inner()) += 1;
                            ready.notify_all();
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
                            #[cfg(test)]
                            {
                                worker_trace_worker
                                    .lock()
                                    .unwrap_or_else(|error| error.into_inner())
                                    .read_started = Some(Instant::now());
                                let (started, ready) = &*worker_reader_started;
                                *started.lock().unwrap_or_else(|error| error.into_inner()) = true;
                                ready.notify_all();
                            }
                            let frame = read_signed_capture_cancellable(
                                &mut stream,
                                &wake_rx,
                                Duration::from_secs(5),
                                &terminate,
                            );
                            #[cfg(test)]
                            {
                                worker_trace_worker
                                    .lock()
                                    .unwrap_or_else(|error| error.into_inner())
                                    .read_finished = Some(Instant::now());
                            }
                            let signed = frame.map_err(|_| ())?;
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
                        #[cfg(test)]
                        {
                            worker_trace_worker
                                .lock()
                                .unwrap_or_else(|error| error.into_inner())
                                .processing_finished = Some(Instant::now());
                        }
                        if terminate.load(Ordering::Acquire) {
                            *worker_active
                                .lock()
                                .unwrap_or_else(|error| error.into_inner()) = None;
                            #[cfg(test)]
                            {
                                worker_trace_worker
                                    .lock()
                                    .unwrap_or_else(|error| error.into_inner())
                                    .active_cleared = Some(Instant::now());
                            }
                            continue;
                        }
                        #[cfg(test)]
                        {
                            worker_trace_worker
                                .lock()
                                .unwrap_or_else(|error| error.into_inner())
                                .write_started = Some(Instant::now());
                        }
                        let response = result.unwrap_or_else(
                            |_| serde_json::json!({"ok":false,"error":"MP_VAULT_CAPTURE_DENIED"}),
                        );
                        let write_result = write_json_cancellable(
                            &mut stream,
                            &wake_rx,
                            &response,
                            Duration::from_secs(5),
                            &terminate,
                        );
                        #[cfg(test)]
                        {
                            worker_trace_worker
                                .lock()
                                .unwrap_or_else(|error| error.into_inner())
                                .write_finished = Some(Instant::now());
                        }
                        let _ = write_result;
                        *worker_active
                            .lock()
                            .unwrap_or_else(|error| error.into_inner()) = None;
                        #[cfg(test)]
                        {
                            worker_trace_worker
                                .lock()
                                .unwrap_or_else(|error| error.into_inner())
                                .active_cleared = Some(Instant::now());
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(25));
                    }
                    Err(_) => break,
                }
            }
            #[cfg(test)]
            {
                worker_trace_worker
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .exit_at = Some(Instant::now());
            }
        });
        Ok(Self {
            stop,
            thread: Some(thread),
            archive_root,
            identity,
            active_stream,
            wake_tx,
            #[cfg(test)]
            accepted_clients,
            #[cfg(test)]
            reader_started,
            #[cfg(test)]
            fail_next_clone,
            #[cfg(test)]
            clone_failure_seen,
            #[cfg(test)]
            force_shutdown_error,
            #[cfg(test)]
            worker_trace,
        })
    }
}

impl Drop for NativeCaptureIpc {
    fn drop(&mut self) {
        #[cfg(test)]
        let drop_started = Instant::now();
        #[cfg(test)]
        let stop_started = Instant::now();
        self.stop.store(true, Ordering::Release);
        #[cfg(test)]
        let stop_signal = stop_started.elapsed();

        #[cfg(test)]
        let wake_started = Instant::now();
        let wake_result = self.wake_tx.write(&[1]);
        #[cfg(test)]
        let wake_error = match wake_result.as_ref() {
            Ok(_) => None,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => None,
            Err(error) => Some(format!(
                "{:?}/{}",
                error.kind(),
                error.raw_os_error().unwrap_or_default()
            )),
        };
        #[cfg(not(test))]
        let _ = wake_result;
        #[cfg(test)]
        let wake_elapsed = wake_started.elapsed();

        #[cfg(test)]
        let active_lock_started = Instant::now();
        let active = self
            .active_stream
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        #[cfg(test)]
        let active_lock_wait = active_lock_started.elapsed();
        #[cfg(test)]
        let shutdown_started = Instant::now();
        #[cfg(test)]
        let mut shutdown_error = None;
        if let Some(stream) = active.as_ref() {
            #[cfg(test)]
            let result = if self.force_shutdown_error.swap(false, Ordering::AcqRel) {
                Err(io::Error::new(
                    io::ErrorKind::Other,
                    "injected native IPC socket shutdown failure",
                ))
            } else {
                stream.shutdown(Shutdown::Both)
            };
            #[cfg(not(test))]
            let result = stream.shutdown(Shutdown::Both);
            #[cfg(test)]
            if let Err(error) = result.as_ref() {
                shutdown_error = Some(format!(
                    "{:?}/{}",
                    error.kind(),
                    error.raw_os_error().unwrap_or_default()
                ));
            }
            #[cfg(not(test))]
            let _ = result;
        }
        #[cfg(test)]
        let shutdown_elapsed = shutdown_started.elapsed();
        drop(active);

        #[cfg(test)]
        let join_started = Instant::now();
        #[cfg(test)]
        let mut worker_panicked = false;
        if let Some(worker) = self.thread.take() {
            let result = worker.join();
            #[cfg(test)]
            {
                worker_panicked = result.is_err();
            }
            #[cfg(not(test))]
            let _ = result;
        }
        #[cfg(test)]
        let join_elapsed = join_started.elapsed();
        #[cfg(test)]
        let join_finished = Instant::now();
        #[cfg(test)]
        let trace = self
            .worker_trace
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();

        // A pinned native reader can briefly deny Windows DeleteFile. Retry
        // only our exact identity, with a bounded delay; never unlink a newer
        // session or force permissions on a changed/foreign descriptor.
        #[cfg(test)]
        let cleanup_started = Instant::now();
        #[cfg(test)]
        let mut cleanup_verify_elapsed = Duration::ZERO;
        #[cfg(test)]
        let mut cleanup_delete_elapsed = Duration::ZERO;
        #[cfg(test)]
        let mut cleanup_sleep_elapsed = Duration::ZERO;
        #[cfg(test)]
        let mut cleanup_attempts = 0_u32;
        #[cfg(test)]
        let mut cleanup_retries = 0_u32;
        #[cfg(test)]
        let mut cleanup_outcome = "not_attempted";
        for _ in 0..50 {
            #[cfg(test)]
            {
                cleanup_attempts += 1;
            }
            #[cfg(test)]
            let verify_started = Instant::now();
            let current_result = SessionDescriptor::read(&self.archive_root);
            #[cfg(test)]
            {
                cleanup_verify_elapsed += verify_started.elapsed();
            }
            let current = match current_result {
                Ok(current) => current,
                Err(_) => {
                    #[cfg(test)]
                    {
                        cleanup_outcome = "identity_read_failed_or_missing";
                    }
                    break;
                }
            };
            if current != self.identity {
                #[cfg(test)]
                {
                    cleanup_outcome = "identity_changed_foreign_or_newer";
                }
                break;
            }
            let Some(parent) = self.archive_root.parent() else {
                #[cfg(test)]
                {
                    cleanup_outcome = "parent_missing";
                }
                break;
            };
            #[cfg(test)]
            let delete_started = Instant::now();
            match fs::remove_file(parent.join("capture-ipc-v1.json")) {
                Ok(()) => {
                    #[cfg(test)]
                    {
                        cleanup_delete_elapsed += delete_started.elapsed();
                        cleanup_outcome = "deleted";
                    }
                    break;
                }
                Err(error) if matches!(error.raw_os_error(), Some(5 | 32 | 33)) => {
                    #[cfg(test)]
                    {
                        cleanup_delete_elapsed += delete_started.elapsed();
                        cleanup_retries += 1;
                        let sleep_started = Instant::now();
                        thread::sleep(Duration::from_millis(10));
                        cleanup_sleep_elapsed += sleep_started.elapsed();
                        cleanup_outcome = "retry_exhausted_or_pending";
                    }
                    #[cfg(not(test))]
                    thread::sleep(Duration::from_millis(10));
                }
                Err(_) => {
                    #[cfg(test)]
                    {
                        cleanup_delete_elapsed += delete_started.elapsed();
                        cleanup_outcome = "delete_failed_non_retryable";
                    }
                    break;
                }
            }
        }
        #[cfg(test)]
        {
            let worker_exit_from_drop = trace
                .exit_at
                .map(|exit| exit.saturating_duration_since(drop_started));
            let join_after_worker_exit = trace
                .exit_at
                .map(|exit| join_finished.saturating_duration_since(exit));
            let read_elapsed = trace
                .read_started
                .zip(trace.read_finished)
                .map(|(start, end)| end.saturating_duration_since(start));
            let post_read_processing = trace
                .read_finished
                .zip(trace.processing_finished)
                .map(|(start, end)| end.saturating_duration_since(start));
            let response_write = trace
                .write_started
                .zip(trace.write_finished)
                .map(|(start, end)| end.saturating_duration_since(start));
            let active_clear_after_write = trace
                .write_finished
                .zip(trace.active_cleared)
                .map(|(start, end)| end.saturating_duration_since(start));
            eprintln!(
                "MP9B_DROP_TIMING stop_signal_us={} wake_signal_us={} wake_error={:?} active_lock_wait_us={} socket_shutdown_us={} worker_read_us={} post_read_processing_us={} response_write_us={} active_clear_after_write_us={} worker_exit_from_drop_us={} join_wait_us={} join_after_worker_exit_us={} worker_panicked={} shutdown_error={:?} descriptor_identity_verify_us={} descriptor_delete_us={} descriptor_sleep_us={} descriptor_attempts={} descriptor_retries={} descriptor_outcome={} total_drop_us={}",
                stop_signal.as_micros(),
                wake_elapsed.as_micros(),
                wake_error,
                active_lock_wait.as_micros(),
                shutdown_elapsed.as_micros(),
                read_elapsed.map(|value| value.as_micros()).unwrap_or(0),
                post_read_processing.map(|value| value.as_micros()).unwrap_or(0),
                response_write.map(|value| value.as_micros()).unwrap_or(0),
                active_clear_after_write.map(|value| value.as_micros()).unwrap_or(0),
                worker_exit_from_drop.map(|value| value.as_micros()).unwrap_or(0),
                join_elapsed.as_micros(),
                join_after_worker_exit.map(|value| value.as_micros()).unwrap_or(0),
                worker_panicked,
                shutdown_error,
                cleanup_verify_elapsed.as_micros(),
                cleanup_delete_elapsed.as_micros(),
                cleanup_sleep_elapsed.as_micros(),
                cleanup_attempts,
                cleanup_retries,
                cleanup_outcome,
                drop_started.elapsed().as_micros(),
            );
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
    fn slow_drip_frame_uses_one_total_deadline() {
        use std::{io::Write, time::Instant};

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let sender = thread::spawn(move || {
            let mut client = TcpStream::connect(address).unwrap();
            for byte in [2u8, 0, 0, 0, b'{', b'}'] {
                if client.write_all(&[byte]).is_err() {
                    break;
                }
                thread::sleep(Duration::from_millis(120));
            }
        });
        let (mut server, _) = listener.accept().unwrap();
        let started = Instant::now();
        assert!(read_frame_with_timeout(&mut server, Duration::from_millis(200)).is_err());
        assert!(started.elapsed() < Duration::from_millis(400));
        drop(server);
        let _ = sender.join();
    }

    fn wait_for_accept_count(ipc: &NativeCaptureIpc, expected: u64) {
        let (count, ready) = &*ipc.accepted_clients;
        let seen = count.lock().unwrap_or_else(|error| error.into_inner());
        let (seen, result) = ready
            .wait_timeout_while(seen, Duration::from_secs(4), |count| *count < expected)
            .unwrap_or_else(|error| error.into_inner());
        assert!(
            !result.timed_out() && *seen >= expected,
            "receiver did not accept client {expected}"
        );
    }

    fn wait_for_reader_started(ipc: &NativeCaptureIpc) {
        let (started, ready) = &*ipc.reader_started;
        let started = started.lock().unwrap_or_else(|error| error.into_inner());
        let (started, result) = ready
            .wait_timeout_while(started, Duration::from_secs(4), |started| !*started)
            .unwrap_or_else(|error| error.into_inner());
        assert!(
            !result.timed_out() && *started,
            "receiver did not enter the real frame reader"
        );
        drop(started);
        thread::sleep(Duration::from_millis(50));
    }

    fn wait_for_active_stream_clear(ipc: &NativeCaptureIpc) {
        for _ in 0..100 {
            if ipc
                .active_stream
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .is_none()
            {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("receiver did not clear the disconnected active client");
    }

    #[test]
    fn lock_cancels_a_slow_partial_client_before_receiver_join() {
        for iteration in 0..30 {
            let home = tempfile::tempdir().unwrap();
            let gate = SecurityGate::open(home.path().join("security.json")).unwrap();
            gate.initialize(b"synthetic-slow-client-lock").unwrap();
            let lease = gate.require_unlocked().unwrap();
            let (vault, _, manifest) = CryptoVault::provision(&gate, lease).unwrap();
            let vault_root = home.path().join("vault");
            manifest.persist_new(&vault_root).unwrap();
            let writer =
                Arc::new(VaultCaptureStore::open(&vault_root, vault, &gate, lease).unwrap());
            let writer_after_lock = writer.clone();
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

            wait_for_accept_count(&ipc, 1);
            wait_for_reader_started(&ipc);
            assert!(
                ipc.active_stream
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .is_some(),
                "receiver cleared the partial client before vault lock"
            );
            gate.lock();
            let started = Instant::now();
            drop(ipc);
            let elapsed = started.elapsed();
            assert!(
                elapsed < Duration::from_secs(1),
                "Lock must cancel the active reader instead of waiting for a peer timeout (iteration {iteration}: {elapsed:?})"
            );
            assert!(
                !home.path().join("capture-ipc-v1.json").exists(),
                "owned session descriptor must be removed after cancellation"
            );
            assert!(
                writer_after_lock.save(synthetic_capture()).is_err(),
                "revoked epoch must reject every later archive commit"
            );
            drop(client);
        }
        eprintln!("MP9B_CANCEL_STABILITY iterations=30 passed=30");
    }

    #[test]
    fn cancellation_wake_survives_socket_shutdown_error() {
        let home = tempfile::tempdir().unwrap();
        let gate = SecurityGate::open(home.path().join("security.json")).unwrap();
        gate.initialize(b"synthetic-shutdown-error").unwrap();
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
        wait_for_accept_count(&ipc, 1);
        wait_for_reader_started(&ipc);
        ipc.force_shutdown_error.store(true, Ordering::Release);
        gate.lock();
        let started = Instant::now();
        drop(ipc);
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "cancellation wake must work even when socket shutdown fails"
        );
        drop(client);
    }

    #[test]
    fn injected_clone_failure_denies_client_without_untracked_reader() {
        let home = tempfile::tempdir().unwrap();
        let gate = SecurityGate::open(home.path().join("security.json")).unwrap();
        gate.initialize(b"synthetic-clone-failure").unwrap();
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
        ipc.fail_next_clone.store(true, Ordering::Release);
        let descriptor = SessionDescriptor::read(&archive_root).unwrap();
        let mut client = TcpStream::connect(("127.0.0.1", descriptor.port)).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        client.write_all(&[1]).unwrap();

        let (seen, ready) = &*ipc.clone_failure_seen;
        let seen = seen.lock().unwrap_or_else(|error| error.into_inner());
        let (seen, result) = ready
            .wait_timeout_while(seen, Duration::from_secs(4), |seen| !*seen)
            .unwrap_or_else(|error| error.into_inner());
        assert!(
            !result.timed_out() && *seen,
            "real accept path did not exercise injected clone failure"
        );
        drop(seen);
        assert!(
            ipc.active_stream
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .is_none(),
            "failed clone must never publish an active stream"
        );
        let mut byte = [0u8; 1];
        let disconnected = client.read(&mut byte);
        assert!(
            matches!(disconnected, Ok(0) | Err(_)),
            "client must be denied after clone failure"
        );
        let started = Instant::now();
        drop(ipc);
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(!home.path().join("capture-ipc-v1.json").exists());
    }

    #[test]
    fn disconnect_during_read_and_response_write_does_not_strand_receiver() {
        let home = tempfile::tempdir().unwrap();
        let gate = SecurityGate::open(home.path().join("security.json")).unwrap();
        gate.initialize(b"synthetic-client-disconnects").unwrap();
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

        let mut abrupt = TcpStream::connect(("127.0.0.1", descriptor.port)).unwrap();
        abrupt.write_all(&[1]).unwrap();
        wait_for_accept_count(&ipc, 1);
        wait_for_reader_started(&ipc);
        drop(abrupt);
        wait_for_active_stream_clear(&ipc);

        let mut response_disconnect = TcpStream::connect(("127.0.0.1", descriptor.port)).unwrap();
        response_disconnect
            .write_all(&[2, 0, 0, 0, b'{', b'}'])
            .unwrap();
        drop(response_disconnect);
        wait_for_accept_count(&ipc, 2);
        wait_for_active_stream_clear(&ipc);

        let concurrent_disconnect = TcpStream::connect(("127.0.0.1", descriptor.port)).unwrap();
        concurrent_disconnect.write_all(&[1]).unwrap();
        wait_for_accept_count(&ipc, 3);
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let disconnect_barrier = barrier.clone();
        let disconnect = thread::spawn(move || {
            disconnect_barrier.wait();
            drop(concurrent_disconnect);
        });
        barrier.wait();
        gate.lock();
        let started = Instant::now();
        drop(ipc);
        let elapsed = started.elapsed();
        disconnect.join().unwrap();
        assert!(
            elapsed < Duration::from_secs(1),
            "concurrent disconnect and receiver cleanup must remain bounded"
        );
    }

    #[test]
    fn descriptor_cleanup_preserves_replaced_session_identity() {
        let home = tempfile::tempdir().unwrap();
        let gate = SecurityGate::open(home.path().join("security.json")).unwrap();
        gate.initialize(b"synthetic-replaced-descriptor").unwrap();
        let lease = gate.require_unlocked().unwrap();
        let (vault, _, manifest) = CryptoVault::provision(&gate, lease).unwrap();
        let vault_root = home.path().join("vault");
        manifest.persist_new(&vault_root).unwrap();
        let writer = Arc::new(VaultCaptureStore::open(&vault_root, vault, &gate, lease).unwrap());
        let archive_root = home.path().join("archive-v1");
        let ipc = NativeCaptureIpc::start(
            archive_root.clone(),
            writer,
            gate,
            lease,
            Arc::new(Mutex::new(())),
        )
        .unwrap();
        let replacement = SessionDescriptor::new(65534, &[0xA5; 32]);
        fs::write(
            home.path().join("capture-ipc-v1.json"),
            serde_json::to_vec(&replacement).unwrap(),
        )
        .unwrap();
        drop(ipc);
        assert_eq!(
            SessionDescriptor::read(&archive_root).unwrap(),
            replacement,
            "Drop must never delete a newer or foreign descriptor"
        );
    }

    #[test]
    fn locked_epoch_rejects_completion_of_partial_frame_without_archive_commit() {
        let home = tempfile::tempdir().unwrap();
        let gate = SecurityGate::open(home.path().join("security.json")).unwrap();
        gate.initialize(b"synthetic-epoch-revocation").unwrap();
        let lease = gate.require_unlocked().unwrap();
        let (vault, _, manifest) = CryptoVault::provision(&gate, lease).unwrap();
        let vault_root = home.path().join("vault");
        manifest.persist_new(&vault_root).unwrap();
        let writer = Arc::new(VaultCaptureStore::open(&vault_root, vault, &gate, lease).unwrap());
        let writer_after_lock = writer.clone();
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
        let signed = SignedCapture::prepare(
            &descriptor.key().unwrap(),
            &format!("chrome-extension://{}/", extension_id()),
            synthetic_capture(),
        )
        .unwrap();
        let payload = serde_json::to_vec(&signed).unwrap();
        let mut framed = (payload.len() as u32).to_le_bytes().to_vec();
        framed.extend_from_slice(&payload);
        let mut client = TcpStream::connect(("127.0.0.1", descriptor.port)).unwrap();
        client.write_all(&framed[..1]).unwrap();
        wait_for_accept_count(&ipc, 1);
        wait_for_reader_started(&ipc);
        gate.lock();
        let _ = client.write_all(&framed[1..]);
        let started = Instant::now();
        drop(ipc);
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(
            writer_after_lock.save(synthetic_capture()).is_err(),
            "revoked epoch must reject any archive commit after lock"
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
