//! MP-9C Slice 1 trusted owner forget commands (desktop only; never MCP).
//!
//! The owner re-enters the unlock secret for BOTH the preview and the
//! confirmation; each re-authentication is consumed by exactly one state call
//! and bound to the live unlock epoch. The one-time confirmation token never
//! leaves native memory: the renderer receives only the impact summary and
//! the preview fingerprint, and confirmation looks the token up here. A
//! token is single-attempt, and every pending token is dropped on Lock.
//! Encrypted vault only. Confirmation commits a LOGICAL tombstone with a
//! pending receipt; no archive, object or backup bytes are removed (MP-9C2).
use crate::{begin_unlocked, finish_unlocked, owner_vault, security_error, AppState};
use mind_place_index::{vault_archive::VaultCaptureStore, IndexError};
use mind_place_security::{SecurityGate, UnlockLease};
use mind_place_state::{SourceForgetProgress, StateError};
use serde::Serialize;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tauri::State;
use zeroize::Zeroizing;

/// Pending one-time confirmation tokens, keyed by source ID (native only).
pub(crate) type ForgetIntents = Arc<Mutex<HashMap<String, Zeroizing<String>>>>;

const VAULT_REQUIRED: &str = "MP_PRIVACY_REQUIRES_VAULT:forgetting requires the encrypted vault";
const NO_PREVIEW: &str = "MP_MEMORY_DENIED:no pending owner preview for this source";

/// What the renderer may see. Deliberately has no token field.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(crate) struct OwnerForgetPreview {
    pub source_id: String,
    pub preview_sha256: String,
    pub affected_claims: usize,
    pub shared_claims: usize,
    pub expires_at_ms: i64,
    pub physical_inventory_complete: bool,
    pub retained_backup_review_required: bool,
}

fn state_error(error: StateError) -> String {
    format!("{}:{}", error.code(), error)
}

fn index_error(error: IndexError) -> String {
    format!("{}:{}", error.code(), error)
}

fn intents_lock(
    intents: &ForgetIntents,
) -> std::sync::MutexGuard<'_, HashMap<String, Zeroizing<String>>> {
    intents
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub(crate) fn clear_intents(intents: &ForgetIntents) {
    intents_lock(intents).clear();
}

pub(crate) fn owner_preview(
    vault: &VaultCaptureStore,
    security: &SecurityGate,
    lease: UnlockLease,
    intents: &ForgetIntents,
    source_id: &str,
    reason: &str,
    secret: &[u8],
) -> Result<OwnerForgetPreview, String> {
    let proof = security
        .reauthenticate_owner(lease, secret)
        .map_err(security_error)?;
    let authority = vault.open_state_store().map_err(index_error)?;
    let mut preview = authority
        .preview_source_forget_as_owner(proof, source_id, reason)
        .map_err(state_error)?;
    let token = Zeroizing::new(std::mem::take(&mut preview.one_time_token));
    // A newer preview supersedes (and invalidates) any older one.
    intents_lock(intents).insert(source_id.to_owned(), token);
    Ok(OwnerForgetPreview {
        source_id: preview.source_id,
        preview_sha256: preview.preview_sha256,
        affected_claims: preview.affected_claims,
        shared_claims: preview.shared_claims,
        expires_at_ms: preview.expires_at_ms,
        physical_inventory_complete: preview.physical_inventory_complete,
        retained_backup_review_required: preview.retained_backup_review_required,
    })
}

pub(crate) fn owner_confirm(
    vault: &VaultCaptureStore,
    security: &SecurityGate,
    lease: UnlockLease,
    intents: &ForgetIntents,
    source_id: &str,
    preview_sha256: &str,
    secret: &[u8],
) -> Result<SourceForgetProgress, String> {
    // Re-authenticate first: a wrong secret neither consumes nor reveals the
    // pending token, and shares the unlock failure backoff.
    let proof = security
        .reauthenticate_owner(lease, secret)
        .map_err(security_error)?;
    // Single attempt: the token is removed before it is presented.
    let token = intents_lock(intents)
        .remove(source_id)
        .ok_or_else(|| NO_PREVIEW.to_owned())?;
    let authority = vault.open_state_store().map_err(index_error)?;
    authority
        .confirm_source_forget_as_owner(proof, source_id, &token, preview_sha256)
        .map_err(state_error)
}

pub(crate) fn owner_progress(
    vault: &VaultCaptureStore,
    source_id: &str,
) -> Result<Option<SourceForgetProgress>, String> {
    vault
        .open_state_store()
        .map_err(index_error)?
        .source_forget_progress_as_owner(source_id)
        .map_err(state_error)
}

async fn owner_task<T: Send + 'static>(
    state: &AppState,
    operation: impl FnOnce(
            Arc<VaultCaptureStore>,
            SecurityGate,
            UnlockLease,
            ForgetIntents,
        ) -> Result<T, String>
        + Send
        + 'static,
) -> Result<T, String> {
    let vault = owner_vault(state).ok_or_else(|| VAULT_REQUIRED.to_owned())?;
    let lease = begin_unlocked(state)?;
    let security = state.security.clone();
    let intents = state.forget_intents.clone();
    let result =
        tauri::async_runtime::spawn_blocking(move || operation(vault, security, lease, intents))
            .await
            .map_err(|_| "MP_FORGET_WORKER:owner forget worker failed".to_owned())??;
    finish_unlocked(state, lease)?;
    Ok(result)
}

/// Owner-only: show what forgetting this source affects. Requires the unlock secret.
#[tauri::command]
pub(super) async fn preview_source_forget(
    state: State<'_, AppState>,
    source_id: String,
    reason: String,
    secret: String,
) -> Result<OwnerForgetPreview, String> {
    let secret = Zeroizing::new(secret);
    owner_task(&state, move |vault, security, lease, intents| {
        owner_preview(
            &vault,
            &security,
            lease,
            &intents,
            &source_id,
            &reason,
            secret.as_bytes(),
        )
    })
    .await
}

/// Owner-only, irreversible logical forget of the previewed source. Requires
/// the unlock secret again and the fingerprint the owner was shown.
#[tauri::command]
pub(super) async fn confirm_source_forget(
    state: State<'_, AppState>,
    source_id: String,
    preview_sha256: String,
    secret: String,
) -> Result<SourceForgetProgress, String> {
    let secret = Zeroizing::new(secret);
    owner_task(&state, move |vault, security, lease, intents| {
        owner_confirm(
            &vault,
            &security,
            lease,
            &intents,
            &source_id,
            &preview_sha256,
            secret.as_bytes(),
        )
    })
    .await
}

#[tauri::command]
pub(super) async fn get_source_forget_progress(
    state: State<'_, AppState>,
    source_id: String,
) -> Result<Option<SourceForgetProgress>, String> {
    owner_task(&state, move |vault, _, _, _| {
        owner_progress(&vault, &source_id)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use mind_place_security::vault::CryptoVault;
    use mind_place_state::{SourceDescriptor, SourceKey};

    const SECRET: &[u8] = b"synthetic-owner-forget-secret";

    struct Rig {
        home: tempfile::TempDir,
        security: SecurityGate,
        vault: Arc<VaultCaptureStore>,
        lease: UnlockLease,
        source: String,
        intents: ForgetIntents,
        recovery: mind_place_security::vault::RecoverySecret,
    }

    fn rig() -> Rig {
        let home = tempfile::tempdir().unwrap();
        let security = SecurityGate::open(home.path().join("security.json")).unwrap();
        security.initialize(SECRET).unwrap();
        let lease = security.require_unlocked().unwrap();
        let (crypto, recovery, manifest) = CryptoVault::provision(&security, lease).unwrap();
        let root = home.path().join("vault");
        manifest.persist_new(&root).unwrap();
        let vault = Arc::new(VaultCaptureStore::open(&root, crypto, &security, lease).unwrap());
        let source = vault
            .open_state_store()
            .unwrap()
            .register_source(&SourceDescriptor {
                provider: "chatgpt",
                source_namespace: "synthetic",
                source_kind: "conversation",
                key: SourceKey::External("owner-forget-source"),
            })
            .unwrap();
        Rig {
            home,
            security,
            vault,
            lease,
            source,
            intents: ForgetIntents::default(),
            recovery,
        }
    }

    fn redacted(rig: &Rig) -> bool {
        rig.vault
            .open_state_store()
            .unwrap()
            .source_is_redacted(&rig.source)
            .unwrap()
    }

    #[test]
    fn owner_preview_keeps_token_native_and_confirmation_needs_a_fresh_reauth() {
        let r = rig();
        assert!(owner_preview(
            &r.vault,
            &r.security,
            r.lease,
            &r.intents,
            &r.source,
            "Forget",
            b"wrong-secret"
        )
        .unwrap_err()
        .starts_with("MP_SECURITY_INVALID_CREDENTIALS"));
        assert!(r.intents.lock().unwrap().is_empty());

        let preview = owner_preview(
            &r.vault,
            &r.security,
            r.lease,
            &r.intents,
            &r.source,
            "Forget",
            SECRET,
        )
        .unwrap();
        let token = r.intents.lock().unwrap()[&r.source].to_string();
        assert_eq!(token.len(), 64);
        let shown = serde_json::to_string(&preview).unwrap();
        assert!(!shown.contains(&token), "token leaked to renderer payload");

        // Wrong secret on confirm: refused, token NOT consumed, no tombstone.
        assert!(owner_confirm(
            &r.vault,
            &r.security,
            r.lease,
            &r.intents,
            &r.source,
            &preview.preview_sha256,
            b"wrong-secret",
        )
        .is_err());
        assert!(r.intents.lock().unwrap().contains_key(&r.source));
        assert!(!redacted(&r));

        // Wrong fingerprint: the single attempt is spent, nothing committed.
        let wrong = "0".repeat(64);
        // Backoff from the failed secret above may apply; wait it out.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        assert!(owner_confirm(
            &r.vault,
            &r.security,
            r.lease,
            &r.intents,
            &r.source,
            &wrong,
            SECRET
        )
        .is_err());
        assert!(!r.intents.lock().unwrap().contains_key(&r.source));
        assert!(!redacted(&r));

        // Fresh preview, then a correct confirmation commits exactly once.
        let preview = owner_preview(
            &r.vault,
            &r.security,
            r.lease,
            &r.intents,
            &r.source,
            "Forget",
            SECRET,
        )
        .unwrap();
        let progress = owner_confirm(
            &r.vault,
            &r.security,
            r.lease,
            &r.intents,
            &r.source,
            &preview.preview_sha256,
            SECRET,
        )
        .unwrap();
        assert!(!progress.physical_erase_verified);
        assert!(redacted(&r));
        assert_eq!(owner_progress(&r.vault, &r.source).unwrap(), Some(progress));
        // Replay: no pending token remains.
        assert_eq!(
            owner_confirm(
                &r.vault,
                &r.security,
                r.lease,
                &r.intents,
                &r.source,
                &preview.preview_sha256,
                SECRET,
            )
            .unwrap_err(),
            NO_PREVIEW
        );
    }

    #[test]
    fn a_preview_from_an_earlier_unlock_epoch_cannot_be_confirmed() {
        let r = rig();
        let preview = owner_preview(
            &r.vault,
            &r.security,
            r.lease,
            &r.intents,
            &r.source,
            "Forget",
            SECRET,
        )
        .unwrap();
        // Lock (as teardown does) then unlock into a new epoch.
        r.security.lock();
        clear_intents(&r.intents);
        r.security.unlock(SECRET).unwrap();
        let lease = r.security.require_unlocked().unwrap();
        assert_ne!(lease.epoch(), r.lease.epoch());
        let manifest =
            mind_place_security::vault::VaultManifest::read(&r.home.path().join("vault")).unwrap();
        let crypto =
            CryptoVault::restore_with_recovery(&r.security, lease, &manifest, &r.recovery).unwrap();
        let vault =
            VaultCaptureStore::open(r.home.path().join("vault"), crypto, &r.security, lease)
                .unwrap();
        assert_eq!(
            owner_confirm(
                &vault,
                &r.security,
                lease,
                &r.intents,
                &r.source,
                &preview.preview_sha256,
                SECRET,
            )
            .unwrap_err(),
            NO_PREVIEW
        );
        // Even a token smuggled across the epoch is refused by state-core.
        let stale_lease_proof = r.security.reauthenticate_owner(r.lease, SECRET);
        assert!(stale_lease_proof.is_err());
        assert!(!vault
            .open_state_store()
            .unwrap()
            .source_is_redacted(&r.source)
            .unwrap());
    }
}
