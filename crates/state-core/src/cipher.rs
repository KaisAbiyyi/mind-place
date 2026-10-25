//! SQLCipher key material; fail closed when the binary lacks a codec.
use mind_place_security::{SecurityGate, UnlockLease};
use rusqlite::{Connection, OptionalExtension};
use std::{fmt, sync::Arc};
use zeroize::{Zeroize, Zeroizing};

pub struct CipherKey {
    material: Zeroizing<[u8; 32]>,
    gate: SecurityGate,
    lease: UnlockLease,
}
impl CipherKey {
    pub fn new(raw: [u8; 32], gate: &SecurityGate, lease: UnlockLease) -> Arc<Self> {
        Arc::new(Self {
            material: Zeroizing::new(raw),
            gate: gate.clone(),
            lease,
        })
    }
    pub fn unlock(&self, db: &Connection) -> rusqlite::Result<()> {
        self.gate
            .validate_lease(self.lease)
            .map_err(|_| rusqlite::Error::InvalidQuery)?;
        let version: Option<String> = db
            .query_row("PRAGMA cipher_version", [], |r| r.get(0))
            .optional()?;
        let supported = version
            .as_deref()
            .and_then(|v| {
                let mut parts = v.split('.');
                Some((
                    parts.next()?.parse::<u32>().ok()?,
                    parts.next()?.parse::<u32>().ok()?,
                ))
            })
            .is_some_and(|(major, minor)| major > 4 || (major == 4 && minor >= 17));
        if !supported || rusqlite::version_number() < 3_053_002 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        // Do not create an unzeroized format! copy of the vault key.
        let mut pragma = String::with_capacity(84);
        pragma.push_str("PRAGMA key = \"x'");
        for byte in self.material.iter() {
            use fmt::Write as _;
            let _ = write!(&mut pragma, "{byte:02x}");
        }
        pragma.push_str("'\";");
        let result = db.execute_batch(&pragma);
        pragma.zeroize();
        result?;
        db.query_row("SELECT count(*) FROM sqlite_master", [], |r| {
            r.get::<_, i64>(0)
        })?;
        // SQLCipher 4.12+ reports 1 only when the keyed pager is active.
        // Refuse a provider that silently ignores PRAGMA key.
        // SQLCipher returns cipher_status as TEXT "1" on some providers.
        // Only an explicit active result is acceptable; NULL, 0 and unknown
        // representations must all fail closed.
        let active: Option<bool> = db
            .query_row("PRAGMA cipher_status", [], |r| match r.get_ref(0)? {
                rusqlite::types::ValueRef::Integer(1) => Ok(true),
                rusqlite::types::ValueRef::Text(bytes) => Ok(bytes == b"1"),
                _ => Ok(false),
            })
            .optional()?;
        if active != Some(true) {
            return Err(rusqlite::Error::InvalidQuery);
        }
        // SQLCipher's extension PRAGMA is not supported by rusqlite's
        // pragma_update whitelist, which reports SQLite API_MISUSE (21).
        // Execute the documented literal SQLCipher pragma, then independently
        // verify the setting. Missing/unrecognized support MUST fail closed.
        db.execute_batch("PRAGMA cipher_memory_security = ON;")?;
        let memory_security: Option<bool> = db
            .query_row("PRAGMA cipher_memory_security", [], |r| {
                match r.get_ref(0)? {
                    rusqlite::types::ValueRef::Integer(1) => Ok(true),
                    rusqlite::types::ValueRef::Text(bytes)
                        if bytes == b"1" || bytes.eq_ignore_ascii_case(b"on") =>
                    {
                        Ok(true)
                    }
                    _ => Ok(false),
                }
            })
            .optional()?;
        if memory_security != Some(true) {
            return Err(rusqlite::Error::InvalidQuery);
        }
        db.pragma_update(None, "temp_store", "MEMORY")?;
        self.gate
            .validate_lease(self.lease)
            .map_err(|_| rusqlite::Error::InvalidQuery)?;
        Ok(())
    }
}
impl fmt::Debug for CipherKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CipherKey([REDACTED])")
    }
}
