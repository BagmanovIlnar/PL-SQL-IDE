//! Persisted PostgreSQL → Connection preferences (`{config}/postgres_connection.json`).
//!
//! sqlx connects straight from the connection URL, so unlike Oracle there is
//! no client home / library / env grid — just URL-level connect options.

use std::path::PathBuf;

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

/// libpq-style sslmode for new connections.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SslMode {
    Disable,
    #[default]
    Prefer,
    Require,
}

impl SslMode {
    pub fn all() -> &'static [SslMode] {
        &[SslMode::Disable, SslMode::Prefer, SslMode::Require]
    }

    /// i18n key for the pick-list label.
    pub fn key(self) -> &'static str {
        match self {
            SslMode::Disable => "prefs.pg.ssl.disable",
            SslMode::Prefer => "prefs.pg.ssl.prefer",
            SslMode::Require => "prefs.pg.ssl.require",
        }
    }

    /// Value expected in `postgres://…?sslmode=`.
    pub fn url_param(self) -> &'static str {
        match self {
            SslMode::Disable => "disable",
            SslMode::Prefer => "prefer",
            SslMode::Require => "require",
        }
    }
}

/// Persisted PostgreSQL connection preferences.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostgresConnectionPrefs {
    #[serde(default)]
    pub ssl_mode: SslMode,
    /// Seconds; 0 = no limit.
    #[serde(default = "default_timeout")]
    pub connect_timeout_secs: u32,
}

fn default_timeout() -> u32 {
    10
}

impl Default for PostgresConnectionPrefs {
    fn default() -> Self {
        Self {
            ssl_mode: SslMode::Prefer,
            connect_timeout_secs: default_timeout(),
        }
    }
}

fn prefs_path() -> PathBuf {
    ProjectDirs::from("com", "plsqlide", "PL-SQL-IDE")
        .map(|dirs| dirs.config_dir().join("postgres_connection.json"))
        .unwrap_or_else(|| {
            PathBuf::from(".")
                .join("config")
                .join("postgres_connection.json")
        })
}

/// Load PostgreSQL connection prefs (defaults when missing / invalid).
pub fn load() -> PostgresConnectionPrefs {
    std::fs::read_to_string(prefs_path())
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// Persist PostgreSQL connection prefs.
pub fn save(prefs: &PostgresConnectionPrefs) {
    let path = prefs_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(prefs) {
        let _ = std::fs::write(path, json);
    }
}

/// URL query suffix appended to every PostgreSQL connection string.
pub fn url_query(prefs: &PostgresConnectionPrefs) -> String {
    format!(
        "sslmode={}&connect_timeout={}",
        prefs.ssl_mode.url_param(),
        prefs.connect_timeout_secs
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_query_covers_both_options() {
        let prefs = PostgresConnectionPrefs {
            ssl_mode: SslMode::Require,
            connect_timeout_secs: 5,
        };
        assert_eq!(url_query(&prefs), "sslmode=require&connect_timeout=5");
    }

    #[test]
    fn json_roundtrip_keeps_defaults() {
        let raw = serde_json::to_string(&PostgresConnectionPrefs::default()).unwrap();
        let back: PostgresConnectionPrefs = serde_json::from_str(&raw).unwrap();
        assert_eq!(back, PostgresConnectionPrefs::default());
    }
}
