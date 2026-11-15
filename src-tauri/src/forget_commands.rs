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
//!
//! Lock serialization (independent review #6889676142, P1): the persistent
//! step (preview intent / tombstone commit) and the native token insert or
//! removal run under `document_authority_gate`, the same gate under which
//! Lock (`lock_document_authority`) and shutdown revoke the unlock lease. The
//! lease and `authority_ready` are re-checked inside the gate immediately
//! before the commit. Hence exactly one linearization exists:
//!   * Lock first  -> the in-gate check fails, nothing is written, error;
//!   * commit first -> Lock waits for the gate, the command reports success.
//! The outcome is decided inside the gate; there is deliberately no post-
//! commit lease check that could report a committed forget as failed.
//! Lock order: document_authority_gate -> privacy (exclusive) -> writer ->
//! SQLite. No path holds a privacy fence while acquiring this gate. The slow
//! password re-verification and connection setup happen before the gate so
//! Lock is never delayed by Argon2.
use crate::{begin_unlocked, finish_unlocked, owner_vault, security_error, AppState};
use mind_place_index::{vault_archive::VaultCaptureStore, IndexError};
use mind_place_security::{SecurityGate, UnlockLease};
use mind_place_state::{SourceForgetProgress, StateError};
use serde::Serialize;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use tauri::State;
use zeroize::Zeroizing;

/// Pending one-time confirmation tokens, keyed by source ID (native only).
pub(crate) type ForgetIntents = Arc<Mutex<HashMap<String, Zeroizing<String>>>>;

const VAULT_REQUIRED: &str = "MP_PRIVACY_REQUIRES_VAULT:forgetting requires the encrypted vault";
const NO_PREVIEW: &str = "MP_MEMORY_DENIED:no pending owner preview for this source";
const NOT_READY: &str = "MP_STATE_NOT_READY:authoritative state not ready";

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

/// The Lock authority a forget step must serialize its commit with.
pub(crate) struct LockAuthority<'a> {
    pub gate: &'a Mutex<()>,
    pub ready: &'a AtomicBool,
    pub security: &'a SecurityGate,
    pub lease: UnlockLease,
}

/// Deterministic test barriers; production passes `Hooks::default()`.
#[derive(Default)]
pub(crate) struct Hooks {
    /// After re-authentication and the encrypted connection, before the gate.
    pub after_connect: Option<Box<dyn Fn() + Send + Sync>>,
    /// Inside the gate, after the in-gate lease check, before the commit.
    pub before_commit: Option<Box<dyn Fn() + Send + Sync>>,
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

/// Run a persistent forget step serialized with Lock (see module docs).
fn commit_serialized_with_lock<T>(
    authority: &LockAuthority<'_>,
    hooks: &Hooks,
    commit: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let _serialized = authority
        .gate
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if !authority.ready.load(Ordering::Acquire) {
        return Err(NOT_READY.to_owned());
    }
    authority
        .security
        .validate_lease(authority.lease)
        .map_err(security_error)?;
    if let Some(hook) = &hooks.before_commit {
        hook();
    }
    commit()
}

pub(crate) fn owner_preview(
    vault: &VaultCaptureStore,
    authority: &LockAuthority<'_>,
    intents: &ForgetIntents,
    hooks: &Hooks,
    source_id: &str,
    reason: &str,
    secret: &[u8],
) -> Result<OwnerForgetPreview, String> {
    let proof = authority
        .security
        .reauthenticate_owner(authority.lease, secret)
        .map_err(security_error)?;
    let store = vault.open_state_store().map_err(index_error)?;
    if let Some(hook) = &hooks.after_connect {
        hook();
    }
    commit_serialized_with_lock(authority, hooks, || {
        let mut preview = store
            .preview_source_forget_as_owner(proof, source_id, reason)
            .map_err(state_error)?;
        let token = Zeroizing::new(std::mem::take(&mut preview.one_time_token));
        // Inside the gate: a later Lock's teardown always drops this token.
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
    })
}

pub(crate) fn owner_confirm(
    vault: &VaultCaptureStore,
    authority: &LockAuthority<'_>,
    intents: &ForgetIntents,
    hooks: &Hooks,
    source_id: &str,
    preview_sha256: &str,
    secret: &[u8],
) -> Result<SourceForgetProgress, String> {
    // Re-authenticate first: a wrong secret neither consumes nor reveals the
    // pending token, and shares the unlock failure backoff.
    let proof = authority
        .security
        .reauthenticate_owner(authority.lease, secret)
        .map_err(security_error)?;
    let store = vault.open_state_store().map_err(index_error)?;
    if let Some(hook) = &hooks.after_connect {
        hook();
    }
    commit_serialized_with_lock(authority, hooks, || {
        // Single attempt: the token is removed before it is presented.
        let token = intents_lock(intents)
            .remove(source_id)
            .ok_or_else(|| NO_PREVIEW.to_owned())?;
        store
            .confirm_source_forget_as_owner(proof, source_id, &token, preview_sha256)
            .map_err(state_error)
    })
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

/// Persistent owner step. The outcome is decided under the Lock gate, so no
/// trailing `finish_unlocked` is applied (it could misreport a commit).
async fn owner_mutation<T: Send + 'static>(
    state: &AppState,
    operation: impl FnOnce(Arc<VaultCaptureStore>, LockAuthority<'_>, ForgetIntents) -> Result<T, String>
        + Send
        + 'static,
) -> Result<T, String> {
    let vault = owner_vault(state).ok_or_else(|| VAULT_REQUIRED.to_owned())?;
    let lease = begin_unlocked(state)?;
    let security = state.security.clone();
    let gate = state.document_authority_gate.clone();
    let ready = state.authority_ready.clone();
    let intents = state.forget_intents.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let authority = LockAuthority {
            gate: &gate,
            ready: &ready,
            security: &security,
            lease,
        };
        operation(vault, authority, intents)
    })
    .await
    .map_err(|_| "MP_FORGET_WORKER:owner forget worker failed".to_owned())?
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
    owner_mutation(&state, move |vault, authority, intents| {
        owner_preview(
            &vault,
            &authority,
            &intents,
            &Hooks::default(),
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
    owner_mutation(&state, move |vault, authority, intents| {
        owner_confirm(
            &vault,
            &authority,
            &intents,
            &Hooks::default(),
            &source_id,
            &preview_sha256,
            secret.as_bytes(),
        )
    })
    .await
}

/// Read-only status; an ordinary unlocked read with a trailing lease check.
#[tauri::command]
pub(super) async fn get_source_forget_progress(
    state: State<'_, AppState>,
    source_id: String,
) -> Result<Option<SourceForgetProgress>, String> {
    let vault = owner_vault(&state).ok_or_else(|| VAULT_REQUIRED.to_owned())?;
    let lease = begin_unlocked(&state)?;
    let result = tauri::async_runtime::spawn_blocking(move || owner_progress(&vault, &source_id))
        .await
        .map_err(|_| "MP_FORGET_WORKER:owner forget worker failed".to_owned())??;
    finish_unlocked(&state, lease)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock_document_authority;
    use mind_place_security::vault::{CryptoVault, RecoverySecret, VaultManifest};
    use mind_place_state::{SourceDescriptor, SourceKey};
    use std::{
        sync::{mpsc, Barrier},
        thread,
        time::Duration,
    };

    const SECRET: &[u8] = b"synthetic-owner-forget-secret";

    struct Rig {
        home: tempfile::TempDir,
        security: SecurityGate,
        vault: Arc<VaultCaptureStore>,
        lease: UnlockLease,
        source: String,
        intents: ForgetIntents,
        recovery: RecoverySecret,
        gate: Arc<Mutex<()>>,
        ready: Arc<AtomicBool>,
    }

    impl Rig {
        fn authority(&self) -> LockAuthority<'_> {
            LockAuthority {
                gate: &self.gate,
                ready: &self.ready,
                security: &self.security,
                lease: self.lease,
            }
        }

        fn redacted(&self) -> bool {
            self.vault
                .open_state_store()
                .unwrap()
                .source_is_redacted(&self.source)
                .unwrap()
        }

        /// The exact production Lock transition (gate + ready + revoke),
        /// followed by the teardown's token drop.
        fn lock(&self) {
            lock_document_authority(&self.gate, &self.ready, &self.security);
            clear_intents(&self.intents);
        }
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
            gate: Arc::new(Mutex::new(())),
            ready: Arc::new(AtomicBool::new(true)),
        }
    }

    fn preview(r: &Rig) -> OwnerForgetPreview {
        owner_preview(
            &r.vault,
            &r.authority(),
            &r.intents,
            &Hooks::default(),
            &r.source,
            "Forget",
            SECRET,
        )
        .unwrap()
    }

    /// A hook that announces arrival and then blocks (bounded) until released.
    fn pause() -> (
        Box<dyn Fn() + Send + Sync>,
        mpsc::Receiver<()>,
        mpsc::Sender<()>,
    ) {
        let (arrived_tx, arrived_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let arrived_tx = Mutex::new(arrived_tx);
        let release_rx = Mutex::new(release_rx);
        let hook = Box::new(move || {
            arrived_tx.lock().unwrap().send(()).unwrap();
            // Bounded: a regression fails the test instead of hanging it.
            let _ = release_rx
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(20));
        });
        (hook, arrived_rx, release_tx)
    }

    #[test]
    fn owner_preview_keeps_token_native_and_confirmation_needs_a_fresh_reauth() {
        let r = rig();
        let hooks = Hooks::default();
        assert!(owner_preview(
            &r.vault,
            &r.authority(),
            &r.intents,
            &hooks,
            &r.source,
            "Forget",
            b"wrong-secret"
        )
        .unwrap_err()
        .starts_with("MP_SECURITY_INVALID_CREDENTIALS"));
        assert!(r.intents.lock().unwrap().is_empty());
        std::thread::sleep(Duration::from_millis(1100));

        let shown = preview(&r);
        let token = r.intents.lock().unwrap()[&r.source].to_string();
        assert_eq!(token.len(), 64);
        let payload = serde_json::to_string(&shown).unwrap();
        assert!(
            !payload.contains(&token),
            "token leaked to renderer payload"
        );

        // Wrong secret on confirm: refused, token NOT consumed, no tombstone.
        assert!(owner_confirm(
            &r.vault,
            &r.authority(),
            &r.intents,
            &hooks,
            &r.source,
            &shown.preview_sha256,
            b"wrong-secret",
        )
        .is_err());
        assert!(r.intents.lock().unwrap().contains_key(&r.source));
        assert!(!r.redacted());

        // Wrong fingerprint: the single attempt is spent, nothing committed.
        std::thread::sleep(Duration::from_millis(1100));
        assert!(owner_confirm(
            &r.vault,
            &r.authority(),
            &r.intents,
            &hooks,
            &r.source,
            &"0".repeat(64),
            SECRET
        )
        .is_err());
        assert!(!r.intents.lock().unwrap().contains_key(&r.source));
        assert!(!r.redacted());

        // Fresh preview, then a correct confirmation commits exactly once.
        let shown = preview(&r);
        let progress = owner_confirm(
            &r.vault,
            &r.authority(),
            &r.intents,
            &hooks,
            &r.source,
            &shown.preview_sha256,
            SECRET,
        )
        .unwrap();
        assert!(!progress.physical_erase_verified);
        assert!(r.redacted());
        assert_eq!(owner_progress(&r.vault, &r.source).unwrap(), Some(progress));
        assert_eq!(
            owner_confirm(
                &r.vault,
                &r.authority(),
                &r.intents,
                &hooks,
                &r.source,
                &shown.preview_sha256,
                SECRET,
            )
            .unwrap_err(),
            NO_PREVIEW
        );
    }

    /// Review P1 ordering 1 (confirm): Lock lands after re-authentication and
    /// the encrypted connection, before the commit. The in-gate check refuses;
    /// nothing is persisted and the error is truthful.
    #[test]
    fn lock_after_connect_before_commit_refuses_confirm_without_mutation() {
        let r = rig();
        let shown = preview(&r);
        let (hook, arrived, release) = pause();
        let hooks = Hooks {
            after_connect: Some(hook),
            before_commit: None,
        };
        thread::scope(|scope| {
            let worker = scope.spawn(|| {
                owner_confirm(
                    &r.vault,
                    &r.authority(),
                    &r.intents,
                    &hooks,
                    &r.source,
                    &shown.preview_sha256,
                    SECRET,
                )
            });
            arrived.recv_timeout(Duration::from_secs(30)).unwrap();
            lock_document_authority(&r.gate, &r.ready, &r.security);
            release.send(()).unwrap();
            let outcome = worker.join().unwrap();
            assert!(outcome.is_err(), "post-Lock confirm reported success");
        });
        // No hidden commit: unlock again and inspect durable state.
        r.security.unlock(SECRET).unwrap();
        let lease = r.security.require_unlocked().unwrap();
        let manifest = VaultManifest::read(&r.home.path().join("vault")).unwrap();
        let crypto =
            CryptoVault::restore_with_recovery(&r.security, lease, &manifest, &r.recovery).unwrap();
        let reopened =
            VaultCaptureStore::open(r.home.path().join("vault"), crypto, &r.security, lease)
                .unwrap();
        let state = reopened.open_state_store().unwrap();
        assert!(!state.source_is_redacted(&r.source).unwrap());
        assert_eq!(
            state.source_forget_progress_as_owner(&r.source).unwrap(),
            None
        );
    }

    /// Review P1 ordering 2 (confirm): the commit is inside the gate when Lock
    /// arrives. Lock waits for the gate; the command reports success and the
    /// tombstone exists: success <=> committed, never a failed result over a
    /// persisted forget.
    #[test]
    fn lock_during_commit_waits_and_the_reported_success_is_truthful() {
        let r = rig();
        let shown = preview(&r);
        let (hook, arrived, release) = pause();
        let hooks = Hooks {
            after_connect: None,
            before_commit: Some(hook),
        };
        let (locked_tx, locked_rx) = mpsc::channel();
        thread::scope(|scope| {
            let worker = scope.spawn(|| {
                owner_confirm(
                    &r.vault,
                    &r.authority(),
                    &r.intents,
                    &hooks,
                    &r.source,
                    &shown.preview_sha256,
                    SECRET,
                )
            });
            arrived.recv_timeout(Duration::from_secs(30)).unwrap();
            let (gate, ready, security) = (&r.gate, &r.ready, &r.security);
            let locker = scope.spawn(move || {
                lock_document_authority(gate, ready, security);
                locked_tx.send(()).unwrap();
            });
            assert!(
                locked_rx.recv_timeout(Duration::from_millis(300)).is_err(),
                "Lock revoked the lease while a forget commit was in its gate"
            );
            assert!(!r.security.status().locked);
            release.send(()).unwrap();
            let outcome = worker.join().unwrap();
            locked_rx.recv_timeout(Duration::from_secs(30)).unwrap();
            locker.join().unwrap();
            let progress = outcome.expect("commit linearized before Lock must report success");
            assert!(!progress.physical_erase_verified);
        });
        assert!(r.security.status().locked);
        assert!(r.security.validate_lease(r.lease).is_err());
        // Durable effect matches the reported success.
        r.security.unlock(SECRET).unwrap();
        let lease = r.security.require_unlocked().unwrap();
        let manifest = VaultManifest::read(&r.home.path().join("vault")).unwrap();
        let crypto =
            CryptoVault::restore_with_recovery(&r.security, lease, &manifest, &r.recovery).unwrap();
        let reopened =
            VaultCaptureStore::open(r.home.path().join("vault"), crypto, &r.security, lease)
                .unwrap();
        assert!(reopened
            .open_state_store()
            .unwrap()
            .source_is_redacted(&r.source)
            .unwrap());
    }

    /// Preview, both orderings: a post-Lock preview stores no token and no
    /// usable intent; an in-gate preview completes before Lock, and Lock's
    /// teardown then drops the token it inserted.
    #[test]
    fn preview_is_serialized_with_lock_in_both_orderings() {
        // Ordering 1: Lock after connect, before commit.
        let r = rig();
        let (hook, arrived, release) = pause();
        let hooks = Hooks {
            after_connect: Some(hook),
            before_commit: None,
        };
        thread::scope(|scope| {
            let worker = scope.spawn(|| {
                owner_preview(
                    &r.vault,
                    &r.authority(),
                    &r.intents,
                    &hooks,
                    &r.source,
                    "Forget",
                    SECRET,
                )
            });
            arrived.recv_timeout(Duration::from_secs(30)).unwrap();
            r.lock();
            release.send(()).unwrap();
            assert!(worker.join().unwrap().is_err());
        });
        assert!(r.intents.lock().unwrap().is_empty());

        // Ordering 2: Lock arrives while the preview is inside the gate.
        let r = rig();
        let (hook, arrived, release) = pause();
        let hooks = Hooks {
            after_connect: None,
            before_commit: Some(hook),
        };
        let barrier = Barrier::new(2);
        thread::scope(|scope| {
            let worker = scope.spawn(|| {
                owner_preview(
                    &r.vault,
                    &r.authority(),
                    &r.intents,
                    &hooks,
                    &r.source,
                    "Forget",
                    SECRET,
                )
            });
            arrived.recv_timeout(Duration::from_secs(30)).unwrap();
            let rig = &r;
            let barrier = &barrier;
            let locker = scope.spawn(move || {
                barrier.wait();
                rig.lock();
            });
            barrier.wait();
            thread::sleep(Duration::from_millis(200));
            assert!(!r.security.status().locked, "Lock passed the forget gate");
            release.send(()).unwrap();
            assert!(worker.join().unwrap().is_ok());
            locker.join().unwrap();
        });
        assert!(r.security.status().locked);
        assert!(
            r.intents.lock().unwrap().is_empty(),
            "a token inserted before Lock survived the Lock teardown"
        );
    }

    /// An interrupted worker (panic inside the gate, before the commit) leaves
    /// no tombstone, and Lock still completes through the poisoned gate.
    #[test]
    fn interrupted_worker_inside_the_gate_commits_nothing_and_lock_still_works() {
        let r = rig();
        let shown = preview(&r);
        let hooks = Hooks {
            after_connect: None,
            before_commit: Some(Box::new(|| panic!("synthetic interruption before commit"))),
        };
        let interrupted = thread::scope(|scope| {
            scope
                .spawn(|| {
                    owner_confirm(
                        &r.vault,
                        &r.authority(),
                        &r.intents,
                        &hooks,
                        &r.source,
                        &shown.preview_sha256,
                        SECRET,
                    )
                })
                .join()
        });
        assert!(interrupted.is_err());
        assert!(r.gate.is_poisoned());
        assert!(!r.redacted());
        r.lock();
        assert!(r.security.status().locked);
    }

    /// A worker that started under an earlier unlock epoch cannot commit after
    /// Lock + re-unlock, even with a valid fresh password.
    #[test]
    fn a_preview_from_an_earlier_unlock_epoch_cannot_be_confirmed() {
        let r = rig();
        let shown = preview(&r);
        r.lock();
        r.security.unlock(SECRET).unwrap();
        r.ready.store(true, Ordering::Release);
        let lease = r.security.require_unlocked().unwrap();
        assert_ne!(lease.epoch(), r.lease.epoch());
        // Stale lease (worker from the old epoch): refused before any commit.
        assert!(owner_confirm(
            &r.vault,
            &r.authority(),
            &r.intents,
            &Hooks::default(),
            &r.source,
            &shown.preview_sha256,
            SECRET,
        )
        .is_err());
        // New epoch: the old preview's token was dropped by Lock.
        let manifest = VaultManifest::read(&r.home.path().join("vault")).unwrap();
        let crypto =
            CryptoVault::restore_with_recovery(&r.security, lease, &manifest, &r.recovery).unwrap();
        let vault =
            VaultCaptureStore::open(r.home.path().join("vault"), crypto, &r.security, lease)
                .unwrap();
        let fresh = LockAuthority {
            gate: &r.gate,
            ready: &r.ready,
            security: &r.security,
            lease,
        };
        assert_eq!(
            owner_confirm(
                &vault,
                &fresh,
                &r.intents,
                &Hooks::default(),
                &r.source,
                &shown.preview_sha256,
                SECRET,
            )
            .unwrap_err(),
            NO_PREVIEW
        );
        assert!(!vault
            .open_state_store()
            .unwrap()
            .source_is_redacted(&r.source)
            .unwrap());
    }
}
