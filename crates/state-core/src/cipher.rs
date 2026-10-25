//! SQLCipher key material; fail closed when the binary lacks a codec.
use rusqlite::{Connection, OptionalExtension};
use mind_place_security::{SecurityGate, UnlockLease};
use std::{fmt, sync::Arc};
use zeroize::{Zeroize, Zeroizing};

pub struct CipherKey {
    material: Zeroizing<[u8; 32]>,
    gate: SecurityGate,
    lease: UnlockLease,
}
impl CipherKey {
    pub fn new(raw: [u8; 32], gate: &SecurityGate, lease: UnlockLease) -> Arc<Self> {
        Arc::new(Self { material: Zeroizing::new(raw), gate: gate.clone(), lease })
    }
    pub fn unlock(&self, db: &Connection) -> rusqlite::Result<()> {
        self.gate.validate_lease(self.lease).map_err(|_| rusqlite::Error::InvalidQuery)?;
        let version: Option<String> = db
            .query_row("PRAGMA cipher_version", [], |r| r.get(0))
            .optional()?;
        if version.as_deref().unwrap_or("").is_empty() {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let mut hex = String::with_capacity(64);
        for byte in self.material.iter() {
            use fmt::Write as _;
            let _ = write!(&mut hex, "{byte:02x}");
        }
        let result = db.execute_batch(&format!("PRAGMA key = \"x'{hex}'\";"));
        hex.zeroize();
        result?;
        db.query_row("SELECT count(*) FROM sqlite_master", [], |r| {
            r.get::<_, i64>(0)
        })?;
        db.pragma_update(None, "temp_store", "MEMORY")?;
        db.pragma_update(None, "cipher_memory_security", "ON")?;
        self.gate.validate_lease(self.lease).map_err(|_| rusqlite::Error::InvalidQuery)?;
        Ok(())
    }
}
impl fmt::Debug for CipherKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CipherKey([REDACTED])")
    }
}
