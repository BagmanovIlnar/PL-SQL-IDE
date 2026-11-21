//! Persistent Oracle connections keyed by [`ConnectionId`].
//!
//! Connect keeps a live session so debug / execute can reuse it instead of
//! opening a one-shot spike connection every time.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use super::connection_profile::ConnectionProfile;
use super::{ConnectionId, CoreError, CoreResult};

#[cfg(feature = "oracle")]
use oracle::Connection;

static POOL: LazyLock<Mutex<HashMap<ConnectionId, PoolEntry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

struct PoolEntry {
    #[cfg(feature = "oracle")]
    target: Option<Connection>,
    #[cfg(feature = "oracle")]
    debug_ctrl: Option<Connection>,
    /// Guards against a stale background thread reattaching a connection
    /// while a newer debug session already owns the slot.
    #[cfg(feature = "oracle")]
    generation: u64,
}

/// Open (or replace) the target session for `profile` after a successful Connect.
#[cfg(feature = "oracle")]
pub fn open_target(profile: &ConnectionProfile) -> CoreResult<()> {
    super::oracle_connection_prefs::prepare_oracle_client()?;
    let connection = connect_profile(profile)?;
    let mut guard = POOL
        .lock()
        .map_err(|_| CoreError::Other("oracle pool lock".into()))?;
    guard.insert(
        profile.id,
        PoolEntry {
            target: Some(connection),
            debug_ctrl: None,
            generation: 0,
        },
    );
    Ok(())
}

#[cfg(not(feature = "oracle"))]
pub fn open_target(_profile: &ConnectionProfile) -> CoreResult<()> {
    Err(CoreError::UnsupportedDb(
        "oracle (build with --features oracle)".into(),
    ))
}

/// Drop pooled sessions for `id` (Disconnect / profile delete).
pub fn close(id: ConnectionId) {
    if let Ok(mut guard) = POOL.lock() {
        guard.remove(&id);
    }
}

/// Whether a live target session exists for `id`.
pub fn has_target(id: ConnectionId) -> bool {
    POOL.lock().map(|g| g.contains_key(&id)).unwrap_or(false)
}

#[cfg(feature = "oracle")]
pub fn connect_profile(profile: &ConnectionProfile) -> CoreResult<Connection> {
    use oracle::Connector;

    let mut connector = Connector::new(
        &profile.username,
        &profile.password,
        profile.oracle_connect_descriptor(),
    );
    connector.stmt_cache_size(0);
    if let Some(privilege) = super::oracle_spike::oracle_privilege_for(profile.connect_as) {
        connector.privilege(privilege);
    }
    connector
        .connect()
        .map_err(|err| CoreError::Connection(err.to_string()))
}

/// Borrow-free helpers used by the debug engine on a blocking thread.
///
/// Connections are **removed** from the pool for the duration of `f` so a long
/// `DBMS_DEBUG.CONTINUE` on the controller cannot deadlock with target execute.
#[cfg(feature = "oracle")]
pub mod debug_access {
    use super::*;

    pub fn ensure_debug_pair(profile: &ConnectionProfile) -> CoreResult<()> {
        super::super::oracle_connection_prefs::prepare_oracle_client()?;
        let mut guard = POOL
            .lock()
            .map_err(|_| CoreError::Other("oracle pool lock".into()))?;
        if let std::collections::hash_map::Entry::Vacant(e) = guard.entry(profile.id) {
            let target = connect_profile(profile)?;
            e.insert(PoolEntry {
                target: Some(target),
                debug_ctrl: None,
                generation: 0,
            });
        }
        let entry = guard
            .get_mut(&profile.id)
            .ok_or_else(|| CoreError::Other("oracle session missing".into()))?;
        if entry.debug_ctrl.is_none() {
            entry.debug_ctrl = Some(connect_profile(profile)?);
        }
        Ok(())
    }

    pub fn with_target_mut<R>(
        id: ConnectionId,
        f: impl FnOnce(&mut Connection) -> CoreResult<R>,
    ) -> CoreResult<R> {
        let mut entry = take(id)?;
        if entry.target.is_none() {
            put(id, entry);
            return Err(CoreError::Other("debug target session is busy".into()));
        }
        let mut target = entry.target.take().unwrap();
        let result = f(&mut target);
        entry.target = Some(target);
        put(id, entry);
        result
    }

    /// Detach the target connection so a background thread can keep it while
    /// the controller keeps polling. Reattach with [`reattach_target`].
    pub fn detach_target(id: ConnectionId) -> CoreResult<(Connection, u64)> {
        let mut entry = take(id)?;
        if entry.target.is_none() {
            // put the entry BACK — dropping it turned any later call into
            // “oracle session missing — Connect first” for the whole window.
            put(id, entry);
            return Err(CoreError::Other("debug target session is busy".into()));
        }
        entry.generation += 1;
        let generation = entry.generation;
        let target = entry.target.take().unwrap();
        put(id, entry);
        Ok((target, generation))
    }

    /// Give a detached target connection back to the pool. A stale thread
    /// (previous generation) drops its connection instead of overwriting
    /// the slot of the live session.
    pub fn reattach_target(id: ConnectionId, connection: Connection, generation: u64) {
        if let Ok(mut guard) = POOL.lock() {
            if let Some(entry) = guard.get_mut(&id) {
                if entry.generation == generation {
                    entry.target = Some(connection);
                }
            }
        }
    }

    pub fn with_ctrl_mut<R>(
        id: ConnectionId,
        f: impl FnOnce(&mut Connection) -> CoreResult<R>,
    ) -> CoreResult<R> {
        let mut entry = take(id)?;
        let ctrl = entry
            .debug_ctrl
            .as_mut()
            .ok_or_else(|| CoreError::Other("debug controller session missing".into()))?;
        let result = f(ctrl);
        put(id, entry);
        result
    }

    /// Run `target_work` and `ctrl_work` on separate threads without holding the pool lock.
    pub fn with_pair_parallel<R1, R2>(
        id: ConnectionId,
        target_work: impl FnOnce(&mut Connection) -> CoreResult<R1> + Send + 'static,
        ctrl_work: impl FnOnce(&mut Connection) -> CoreResult<R2> + Send + 'static,
    ) -> CoreResult<(R1, R2)>
    where
        R1: Send + 'static,
        R2: Send + 'static,
    {
        let mut entry = take(id)?;
        if entry.target.is_none() {
            put(id, entry);
            return Err(CoreError::Other("debug target session is busy".into()));
        }
        if entry.debug_ctrl.is_none() {
            put(id, entry);
            return Err(CoreError::Other("debug controller session missing".into()));
        }
        let mut target = entry.target.take().unwrap();
        let mut ctrl = entry.debug_ctrl.take().unwrap();

        let t = std::thread::spawn(move || {
            let r = target_work(&mut target);
            (r, target)
        });
        let c = std::thread::spawn(move || {
            let r = ctrl_work(&mut ctrl);
            (r, ctrl)
        });

        let (target_result, target) = t
            .join()
            .map_err(|_| CoreError::Other("target debug thread panicked".into()))?;
        let (ctrl_result, ctrl) = c
            .join()
            .map_err(|_| CoreError::Other("controller debug thread panicked".into()))?;

        put(
            id,
            PoolEntry {
                target: Some(target),
                debug_ctrl: Some(ctrl),
                generation: 0,
            },
        );
        Ok((target_result?, ctrl_result?))
    }

    fn take(id: ConnectionId) -> CoreResult<PoolEntry> {
        let mut guard = POOL
            .lock()
            .map_err(|_| CoreError::Other("oracle pool lock".into()))?;
        guard
            .remove(&id)
            .ok_or_else(|| CoreError::Other("oracle session missing — Connect first".into()))
    }

    fn put(id: ConnectionId, entry: PoolEntry) {
        if let Ok(mut guard) = POOL.lock() {
            guard.insert(id, entry);
        }
    }
}
