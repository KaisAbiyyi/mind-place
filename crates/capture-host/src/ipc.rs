//! Authenticated owner-local capture IPC. The native host never loads a DEK.
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use hmac::{Hmac, Mac};
use mind_place_archive::{extension_id, ArchiveError, Capture, Result, MAX_CAPTURE_BYTES};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::{
    collections::HashSet,
    fs,
    io::{Read, Write},
    net::{Ipv4Addr, SocketAddrV4, TcpStream},
    path::Path,
    time::{Duration, Instant},
};

#[cfg(windows)]
#[path = "ipc_acl.rs"]
mod acl;

#[cfg(windows)]
pub fn secure_descriptor(path: &Path) -> Result<()> {
    acl::protect(path).map_err(|_| invalid("IPC owner DACL cannot be enforced"))
}

#[cfg(windows)]
pub fn verify_descriptor(path: &Path) -> Result<()> {
    acl::verify(path).map_err(|_| invalid("IPC owner DACL not verifiable"))
}

const VERSION: u32 = 1;
const MAX_FRAME: usize = MAX_CAPTURE_BYTES + 8192;
const MAX_NONCES: usize = 200_000;

fn open_descriptor(path: &Path) -> Result<fs::File> {
    for component in path.ancestors() {
        let metadata = fs::symlink_metadata(component)?;
        if metadata.file_type().is_symlink() {
            return Err(invalid("IPC descriptor ancestor is a link"));
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err(invalid("IPC descriptor ancestor is a reparse point"));
            }
        }
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Pin this exact object against both write/truncate and rename/delete.
        options.share_mode(1).custom_flags(0x0020_0000); // SHARE_READ / OPEN_REPARSE_POINT
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > 512 {
        return Err(invalid("IPC session descriptor is unsafe"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(invalid("IPC descriptor handle is a reparse point"));
        }
    }
    Ok(file)
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SessionDescriptor {
    pub version: u32,
    pub port: u16,
    pub session_token: String,
}
impl std::fmt::Debug for SessionDescriptor {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SessionDescriptor")
            .field("version", &self.version)
            .field("port", &self.port)
            .field("session_token", &"[redacted]")
            .finish()
    }
}
impl SessionDescriptor {
    pub fn new(port: u16, key: &[u8; 32]) -> Self {
        Self {
            version: VERSION,
            port,
            session_token: URL_SAFE_NO_PAD.encode(key),
        }
    }
    pub fn key(&self) -> Result<[u8; 32]> {
        if self.version != VERSION || self.port == 0 {
            return Err(invalid("IPC session version or port is invalid"));
        }
        let decoded = URL_SAFE_NO_PAD
            .decode(&self.session_token)
            .map_err(|_| invalid("IPC session capability is invalid"))?;
        decoded
            .try_into()
            .map_err(|_| invalid("IPC session capability length is invalid"))
    }
    pub fn read(root: &Path) -> Result<Self> {
        let base = root
            .parent()
            .ok_or_else(|| invalid("IPC app root is unavailable"))?;
        let path = base.join("capture-ipc-v1.json");
        let file = open_descriptor(&path)?;
        #[cfg(windows)]
        verify_descriptor_file(&file)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if file.metadata()?.permissions().mode() & 0o077 != 0 {
                return Err(invalid("IPC session permissions are not private"));
            }
        }
        let mut bytes = Vec::new();
        file.take(513).read_to_end(&mut bytes)?;
        if bytes.len() > 512 {
            return Err(invalid("IPC descriptor grew beyond its limit"));
        }
        let descriptor: Self = serde_json::from_slice(&bytes)?;
        descriptor.key()?;
        Ok(descriptor)
    }
}

#[cfg(windows)]
pub fn verify_descriptor_file(file: &fs::File) -> Result<()> {
    acl::verify_file(file)
        .map_err(|_| invalid("IPC descriptor owner/DACL not verifiable on handle"))
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedCapture {
    pub version: u32,
    pub nonce: String,
    pub tag: String,
    pub origin: String,
    pub capture: Capture,
}

fn invalid(message: &str) -> ArchiveError {
    ArchiveError::Invalid(message.into())
}

fn sign(key: &[u8; 32], nonce: &[u8], origin: &str, capture: &Capture) -> Result<Vec<u8>> {
    let payload = serde_json::to_vec(&(VERSION, nonce, origin, capture))?;
    let mut mac = Hmac::<Sha256>::new_from_slice(key)
        .map_err(|_| invalid("IPC authentication unavailable"))?;
    mac.update(&payload);
    Ok(mac.finalize().into_bytes().to_vec())
}

impl SignedCapture {
    pub fn prepare(key: &[u8; 32], origin: &str, capture: Capture) -> Result<Self> {
        let mut nonce = [0u8; 16];
        getrandom::fill(&mut nonce).map_err(|_| invalid("IPC random source unavailable"))?;
        let tag = sign(key, &nonce, origin, &capture)?;
        Ok(Self {
            version: VERSION,
            nonce: URL_SAFE_NO_PAD.encode(nonce),
            tag: URL_SAFE_NO_PAD.encode(tag),
            origin: origin.to_owned(),
            capture,
        })
    }

    pub fn authenticate(self, key: &[u8; 32], seen: &mut HashSet<String>) -> Result<Capture> {
        if self.version != VERSION
            || self.origin != format!("chrome-extension://{}/", extension_id())
            || seen.len() >= MAX_NONCES
        {
            return Err(invalid("IPC origin, epoch or replay budget denied"));
        }
        let nonce = URL_SAFE_NO_PAD
            .decode(&self.nonce)
            .map_err(|_| invalid("IPC nonce encoding invalid"))?;
        let proof = URL_SAFE_NO_PAD
            .decode(&self.tag)
            .map_err(|_| invalid("IPC authentication encoding invalid"))?;
        if nonce.len() != 16 || proof.len() != 32 {
            return Err(invalid("IPC authentication size invalid"));
        }
        let expected = sign(key, &nonce, &self.origin, &self.capture)?;
        let mut verifier =
            Hmac::<Sha256>::new_from_slice(key).map_err(|_| invalid("IPC verifier unavailable"))?;
        verifier.update(&serde_json::to_vec(&(
            VERSION,
            nonce.as_slice(),
            self.origin.as_str(),
            &self.capture,
        ))?);
        verifier
            .verify_slice(&proof)
            .map_err(|_| invalid("IPC authentication rejected"))?;
        if expected != proof || !seen.insert(self.nonce) {
            return Err(invalid("IPC capture replay rejected"));
        }
        Ok(self.capture)
    }
}

pub fn read_frame(mut input: impl Read) -> Result<SignedCapture> {
    let mut size = [0u8; 4];
    input.read_exact(&mut size)?;
    let length = u32::from_le_bytes(size) as usize;
    if length == 0 || length > MAX_FRAME {
        return Err(invalid("IPC payload outside allowed limits"));
    }
    let mut bytes = vec![0u8; length];
    input.read_exact(&mut bytes)?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn read_exact_before_deadline(
    input: &mut TcpStream,
    mut buffer: &mut [u8],
    deadline: Instant,
) -> Result<()> {
    while !buffer.is_empty() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "IPC frame deadline exceeded",
            )
            .into());
        }
        input.set_read_timeout(Some(remaining))?;
        match input.read(buffer) {
            Ok(0) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "IPC frame ended early",
                )
                .into())
            }
            Ok(count) => {
                let (_, rest) = buffer.split_at_mut(count);
                buffer = rest;
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

/// Read one bounded frame against a single monotonic deadline. Updating the
/// socket timeout before every read prevents a peer from extending the frame
/// indefinitely by delivering one byte just before each per-read timeout.
pub fn read_frame_with_timeout(
    input: &mut TcpStream,
    timeout: Duration,
) -> Result<SignedCapture> {
    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or_else(|| invalid("IPC frame deadline is invalid"))?;
    let mut size = [0u8; 4];
    read_exact_before_deadline(input, &mut size, deadline)?;
    let length = u32::from_le_bytes(size) as usize;
    if length == 0 || length > MAX_FRAME {
        return Err(invalid("IPC payload outside allowed limits"));
    }
    let mut bytes = vec![0u8; length];
    read_exact_before_deadline(input, &mut bytes, deadline)?;
    Ok(serde_json::from_slice(&bytes)?)
}

pub fn write_json(mut output: impl Write, value: &serde_json::Value) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    if bytes.len() > 1024 * 1024 {
        return Err(invalid("IPC response too large"));
    }
    output.write_all(&(bytes.len() as u32).to_le_bytes())?;
    output.write_all(&bytes)?;
    output.flush()?;
    Ok(())
}

pub fn forward_capture(root: &Path, origin: &str, capture: Capture) -> Result<serde_json::Value> {
    let descriptor = SessionDescriptor::read(root)?;
    let key = descriptor.key()?;
    let frame = SignedCapture::prepare(&key, origin, capture)?;
    let payload = serde_json::to_vec(&frame)?;
    if payload.len() > MAX_FRAME {
        return Err(invalid("IPC payload exceeds capacity"));
    }
    let endpoint = SocketAddrV4::new(Ipv4Addr::LOCALHOST, descriptor.port);
    let mut socket = TcpStream::connect_timeout(&endpoint.into(), Duration::from_secs(2))?;
    socket.set_read_timeout(Some(Duration::from_secs(5)))?;
    socket.set_write_timeout(Some(Duration::from_secs(5)))?;
    socket.write_all(&(payload.len() as u32).to_le_bytes())?;
    socket.write_all(&payload)?;
    let mut header = [0u8; 4];
    socket.read_exact(&mut header)?;
    let size = u32::from_le_bytes(header) as usize;
    if size == 0 || size > 1024 * 1024 {
        return Err(invalid("IPC response length invalid"));
    }
    let mut response = vec![0u8; size];
    socket.read_exact(&mut response)?;
    let value: serde_json::Value = serde_json::from_slice(&response)?;
    if value.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        return Err(invalid("Encrypted capture writer denied the request"));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Capture {
        serde_json::from_value(serde_json::json!({
            "schema_version":1,
            "provider":"chatgpt",
            "conversation_id":"123e4567-e89b-12d3-a456-426614174000",
            "source_url":"https://chatgpt.com/c/123e4567-e89b-12d3-a456-426614174000",
            "title":"Synthetic",
            "captured_at":"2026-10-26T03:00:00Z",
            "capture_method":"manual-dom",
            "completeness":"partial",
            "messages":[{"id":"a","role":"user","text":"synthetic",
                "format":"plain","attachments":[]}],
            "warnings":["synthetic"]
        }))
        .unwrap()
    }

    #[cfg(windows)]
    #[test]
    fn descriptor_handle_blocks_replacement_and_truncation_until_closed() {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join("capture-ipc-v1.json");
        fs::write(
            &path,
            serde_json::to_vec(&SessionDescriptor::new(12345, &[7u8; 32])).unwrap(),
        )
        .unwrap();
        secure_descriptor(&path).unwrap();
        let held = open_descriptor(&path).unwrap();
        let moved = home.path().join("moved.json");
        assert!(fs::rename(&path, &moved).is_err());
        assert!(fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&path)
            .is_err());
        drop(held);
        fs::rename(&path, &moved).unwrap();
        fs::rename(&moved, &path).unwrap();
        SessionDescriptor::read(&home.path().join("archive-v1")).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn anonymous_descriptor_read_probe() {
        let Some(root) = std::env::var_os("MP9B_SYNTHETIC_DESCRIPTOR_ROOT") else {
            return;
        };
        use windows_sys::Win32::{
            Security::{ImpersonateAnonymousToken, RevertToSelf},
            System::Threading::GetCurrentThread,
        };
        let root = std::path::PathBuf::from(root);
        SessionDescriptor::read(&root).unwrap();
        struct Revert;
        impl Drop for Revert {
            fn drop(&mut self) {
                unsafe {
                    RevertToSelf();
                }
            }
        }
        assert_ne!(unsafe { ImpersonateAnonymousToken(GetCurrentThread()) }, 0);
        let revert = Revert;
        let denied =
            fs::File::open(root.parent().unwrap().join("capture-ipc-v1.json")).unwrap_err();
        assert_eq!(denied.raw_os_error(), Some(5));
        assert!(SessionDescriptor::read(&root).is_err());
        drop(revert);
        SessionDescriptor::read(&root).unwrap();
        println!("SYNTHETIC_ANONYMOUS_TOKEN_DENIED");
    }

    #[cfg(windows)]
    #[test]
    fn separate_process_anonymous_token_cannot_read_owner_capability() {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join("capture-ipc-v1.json");
        fs::write(
            &path,
            serde_json::to_vec(&SessionDescriptor::new(12345, &[9u8; 32])).unwrap(),
        )
        .unwrap();
        secure_descriptor(&path).unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "ipc::tests::anonymous_descriptor_read_probe",
                "--nocapture",
            ])
            .env(
                "MP9B_SYNTHETIC_DESCRIPTOR_ROOT",
                home.path().join("archive-v1"),
            )
            .output()
            .unwrap();
        let transcript = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success()
                && transcript.contains("anonymous_descriptor_read_probe ... ok")
                && transcript.contains("1 passed; 0 failed")
                && transcript.contains("SYNTHETIC_ANONYMOUS_TOKEN_DENIED"),
            "Anonymous-token child failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        // This is an actual anonymous thread token in another process, not a
        // different installed Windows account. That external gate stays open.
    }

    #[test]
    fn rejects_forgery_replay_wrong_origin_and_session_rotation() {
        let key = [7u8; 32];
        let origin = format!("chrome-extension://{}/", extension_id());
        let mut seen = HashSet::new();
        let first = SignedCapture::prepare(&key, &origin, fixture()).unwrap();
        let copy = serde_json::from_slice(&serde_json::to_vec(&first).unwrap()).unwrap();
        assert!(first.authenticate(&key, &mut seen).is_ok());
        assert!(SignedCapture::authenticate(copy, &key, &mut seen).is_err());
        let forged = SignedCapture::prepare(&[8u8; 32], &origin, fixture()).unwrap();
        assert!(forged.authenticate(&key, &mut seen).is_err());
        let bad_origin =
            SignedCapture::prepare(&key, "chrome-extension://evil/", fixture()).unwrap();
        assert!(bad_origin.authenticate(&key, &mut seen).is_err());
    }
}
