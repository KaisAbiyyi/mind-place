//! MP-4F: owner-opted-in, bounded document refresh schedule.
//! A root is polled only after the owner explicitly indexes it. No boot scans,
//! implicit client grants, persistent background work, or MCP exposure.
use crate::AppState;
use mind_place_index::IndexError;
use mind_place_security::UnlockLease;
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

const MAX_ACTIVE_ROOTS: usize = 16;
const POLL_PERIOD: Duration = Duration::from_secs(300);

struct Worker {
    active: Arc<AtomicBool>,
    thread: thread::Thread,
}

#[derive(Default)]
pub(crate) struct DocumentRefreshRegistry {
    workers: Mutex<BTreeMap<String, Worker>>,
}

impl DocumentRefreshRegistry {
    pub(crate) fn activate(
        self: &Arc<Self>,
        app: &AppState,
        root_id: &str,
        lease: UnlockLease,
    ) -> bool {
        let mut workers = self.workers.lock().unwrap_or_else(|e| e.into_inner());
        if workers.contains_key(root_id) {
            return true;
        }
        if workers.len() >= MAX_ACTIVE_ROOTS {
            return false;
        }
        let token = Arc::new(AtomicBool::new(true));
        let active = token.clone();
        let id = root_id.to_owned();
        let state_path = app.state_path.clone();
        let index_path = app.index_path.with_file_name("document-index.sqlite3");
        let vault = crate::owner_vault(app);
        let security = app.security.clone();
        let ready = app.authority_ready.clone();
        let registry = self.clone();

        let spawned = thread::Builder::new()
            .name("mp-document-refresh".into())
            .spawn(move || {
                loop {
                    thread::park_timeout(POLL_PERIOD);
                    if !active.load(Ordering::Acquire)
                        || !ready.load(Ordering::Acquire)
                        || security.validate_lease(lease).is_err()
                    {
                        break;
                    }
                    // Failure leaves the previous published generation intact.
                    // Search independently verifies every candidate against native
                    // bytes; a failed/incomplete refresh cannot expose stale data.
                    let attempt = (|| -> Result<(), IndexError> {
                        let authority = crate::open_owner_state(&vault, &state_path)?;
                        let index = crate::open_owner_document_index(&vault, &index_path)?;
                        index.refresh_root(&authority, &id, || {
                            if !active.load(Ordering::Acquire)
                                || !ready.load(Ordering::Acquire)
                                || security.validate_lease(lease).is_err()
                            {
                                Err(IndexError::Revoked)
                            } else {
                                Ok(())
                            }
                        })?;
                        Ok(())
                    })();
                    if attempt.is_err() {
                        if !active.load(Ordering::Acquire)
                            || !ready.load(Ordering::Acquire)
                            || security.validate_lease(lease).is_err()
                            || crate::open_owner_state(&vault, &state_path)
                                .and_then(|s| s.document_root(&id).map_err(IndexError::from))
                                .ok()
                                .flatten()
                                .is_none_or(|r| !r.is_approved())
                        {
                            break;
                        }
                    }
                }
                // An older worker must not erase a newly registered successor.
                registry.deactivate_matching(&id, &active);
            });
        match spawned {
            Ok(handle) => {
                workers.insert(
                    root_id.to_owned(),
                    Worker {
                        active: token,
                        thread: handle.thread().clone(),
                    },
                );
                true
            }
            Err(_) => false,
        }
    }

    fn deactivate_matching(&self, root_id: &str, token: &Arc<AtomicBool>) {
        let mut workers = self.workers.lock().unwrap_or_else(|e| e.into_inner());
        if workers
            .get(root_id)
            .is_some_and(|w| Arc::ptr_eq(&w.active, token))
        {
            workers.remove(root_id);
        }
    }

    pub(crate) fn deactivate(&self, root_id: &str) {
        if let Some(worker) = self
            .workers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(root_id)
        {
            worker.active.store(false, Ordering::Release);
            worker.thread.unpark();
        }
    }

    pub(crate) fn deactivate_all(&self) {
        let mut workers = self.workers.lock().unwrap_or_else(|e| e.into_inner());
        for worker in workers.values() {
            worker.active.store(false, Ordering::Release);
            worker.thread.unpark();
        }
        workers.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty_registry_is_safe_to_cancel_repeatedly() {
        let registry = DocumentRefreshRegistry::default();
        registry.deactivate("root:v1:unknown");
        registry.deactivate_all();
        registry.deactivate_all();
        assert!(registry.workers.lock().unwrap().is_empty());
    }

    #[test]
    fn old_worker_cannot_remove_successor_after_replacement() {
        let registry = DocumentRefreshRegistry::default();
        let root = "root:v1:fixture";
        let old = Arc::new(AtomicBool::new(true));
        let new = Arc::new(AtomicBool::new(true));
        let current = thread::current();
        registry.workers.lock().unwrap().insert(
            root.into(),
            Worker {
                active: new.clone(),
                thread: current,
            },
        );
        registry.deactivate_matching(root, &old);
        assert!(registry.workers.lock().unwrap().contains_key(root));
        registry.deactivate(root);
        assert!(!new.load(Ordering::Acquire));
    }
}
