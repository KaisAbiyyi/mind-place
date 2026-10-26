pub mod ipc;

use mind_place_archive::{
    extension_id, ArchiveError, ArchiveStore, Capture, Result, MAX_CAPTURE_BYTES,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    path::Path,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    protocol: u32,
    operation: String,
    capture: Option<Capture>,
}

pub fn write_response(mut output: impl Write, response: &Value) -> Result<()> {
    let bytes = serde_json::to_vec(response)?;
    if bytes.len() > 1024 * 1024 {
        return Err(ArchiveError::Invalid(
            "Native response exceeds 1 MiB".into(),
        ));
    }
    output.write_all(&(bytes.len() as u32).to_le_bytes())?;
    output.write_all(&bytes)?;
    output.flush()?;
    Ok(())
}

fn read_request(mut input: impl Read) -> Result<Request> {
    let mut size = [0u8; 4];
    input.read_exact(&mut size)?;
    let size = u32::from_le_bytes(size) as usize;
    if size == 0 || size > MAX_CAPTURE_BYTES {
        return Err(ArchiveError::Invalid(
            "Native capture exceeds the 32 MiB limit or is empty".into(),
        ));
    }
    let mut bytes = vec![0; size];
    input.read_exact(&mut bytes)?;
    Ok(serde_json::from_slice(&bytes)?)
}

pub fn serve_one(input: impl Read, output: impl Write, root: &Path, origin: &str) -> Result<()> {
    let result = (|| -> Result<Value> {
        if origin != format!("chrome-extension://{}/", extension_id()) {
            return Err(ArchiveError::Invalid(
                "Native host accepts only the Mind Place Capture extension".into(),
            ));
        }
        let request = read_request(input)?;
        if request.protocol != 1 {
            return Err(ArchiveError::Invalid(
                "Unsupported native capture protocol".into(),
            ));
        }
        match request.operation.as_str() {
            "ping" if request.capture.is_none() => Ok(json!({"ok":true,"protocol":1})),
            "save" => {
                let capture = request
                    .capture
                    .ok_or_else(|| ArchiveError::Invalid("Capture is missing".into()))?;
                if !matches!(
                    capture.capture_method.as_str(),
                    "manual-dom" | "automatic-dom"
                ) {
                    return Err(ArchiveError::Invalid(
                        "Browser requests must be DOM captures".into(),
                    ));
                }
                // A marked encrypted installation MUST only use the
                // authenticated desktop writer. Never fall back to the legacy
                // raw archive after a failed/locked/unavailable IPC session.
                if root.parent().is_some_and(|parent| {
                    parent.join("encrypted-vault-v1/vault-v1.json").exists()
                }) {
                    return ipc::forward_capture(root, origin, capture);
                }
                if !root.is_dir() {
                    return Err(ArchiveError::Invalid(
                        "MP_VAULT_IPC_REQUIRED:owner session is not initialized".into(),
                    ));
                }
                let receipt = ArchiveStore::open(root)?.save(capture)?;
                Ok(serde_json::to_value(receipt)?)
            }
            _ => Err(ArchiveError::Invalid(
                "Native host supports only ping and DOM save".into(),
            )),
        }
    })();
    match result {
        Ok(value) => write_response(output, &value),
        Err(error) => write_response(output, &json!({"ok":false,"error":error.to_string()})),
    }
}
