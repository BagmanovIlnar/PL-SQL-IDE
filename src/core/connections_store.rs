//! Persist Connections list to `{config}/connections.json`.

use std::path::PathBuf;

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use super::connection_profile::{ConnectionId, ConnectionProfile};

/// On-disk Connections sidebar state.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConnectionsStore {
    #[serde(default)]
    pub connections: Vec<ConnectionProfile>,
    #[serde(default)]
    pub selected: Option<ConnectionId>,
}

fn config_dir() -> PathBuf {
    ProjectDirs::from("com", "plsqlide", "PL-SQL-IDE")
        .map(|dirs| dirs.config_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".").join("config"))
}

/// Path to `{config}/connections.json`.
pub fn store_path() -> PathBuf {
    config_dir().join("connections.json")
}

/// Load saved connections (empty list when missing / invalid).
pub fn load() -> ConnectionsStore {
    let path = store_path();
    match std::fs::read_to_string(&path) {
        Ok(raw) => match serde_json::from_str(&raw) {
            Ok(store) => store,
            Err(error) => {
                warn!(%error, path = %path.display(), "failed to parse connections.json");
                ConnectionsStore::default()
            }
        },
        Err(_) => ConnectionsStore::default(),
    }
}

/// Persist connections to disk.
pub fn save(store: &ConnectionsStore) {
    let path = store_path();
    if let Some(parent) = path.parent() {
        if let Err(error) = std::fs::create_dir_all(parent) {
            warn!(%error, "failed to create config dir for connections");
            return;
        }
    }
    match serde_json::to_string_pretty(store) {
        Ok(json) => {
            if let Err(error) = std::fs::write(&path, json) {
                warn!(%error, path = %path.display(), "failed to write connections.json");
            } else {
                info!(
                    count = store.connections.len(),
                    path = %path.display(),
                    "saved connections"
                );
            }
        }
        Err(error) => warn!(%error, "failed to serialize connections"),
    }
}
