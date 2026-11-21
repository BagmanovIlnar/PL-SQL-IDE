//! Persistent command-window dialog history, keyed by connection id.
//! Survives app restarts — the command line keeps its input log.

use std::collections::HashMap;
use std::path::PathBuf;

use directories::ProjectDirs;

fn path() -> PathBuf {
    ProjectDirs::from("com", "plsqlide", "PL-SQL-IDE")
        .map(|dirs| dirs.config_dir().to_path_buf().join("command_history.json"))
        .unwrap_or_else(|| PathBuf::from("command_history.json"))
}

pub fn load() -> HashMap<String, String> {
    std::fs::read(path())
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn save(history: &HashMap<String, String>) {
    if let Ok(bytes) = serde_json::to_vec(history) {
        let _ = std::fs::write(path(), bytes);
    }
}
