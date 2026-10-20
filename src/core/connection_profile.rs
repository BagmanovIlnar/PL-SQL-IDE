//! Saved database connection profiles (non-secret metadata + password in memory for now).

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::core::DbKind;

/// Identifier of a saved connection profile.
pub type ConnectionId = Uuid;

/// Connection profile shown in the Connections list.
///
/// # Examples
///
/// ```
/// use plsqlide::core::connection_profile::ConnectionProfile;
/// use plsqlide::core::DbKind;
///
/// let profile = ConnectionProfile::new("Local PG", DbKind::Postgres);
/// assert_eq!(profile.port, 5432);
/// assert!(profile.summary().contains("PostgreSQL"));
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionProfile {
    pub id: ConnectionId,
    pub name: String,
    pub db_kind: DbKind,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    /// Temporary in-memory secret until OS keyring is wired in Phase 1.
    pub password: String,
}

impl ConnectionProfile {
    /// Creates an empty profile with only the dialect default port filled in.
    ///
    /// Host / database / credentials stay blank so the user enters real values
    /// (no seeded localhost demos).
    pub fn new(name: impl Into<String>, db_kind: DbKind) -> Self {
        let port = match db_kind {
            DbKind::Postgres => 5432,
            DbKind::Oracle => 1521,
        };

        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            db_kind,
            host: String::new(),
            port,
            database: String::new(),
            username: String::new(),
            password: String::new(),
        }
    }

    /// Applies default port when the database kind changes.
    pub fn set_db_kind(&mut self, db_kind: DbKind) {
        self.db_kind = db_kind;
        self.port = match db_kind {
            DbKind::Postgres => 5432,
            DbKind::Oracle => 1521,
        };
    }

    /// One-line summary for the Connections list.
    pub fn summary(&self) -> String {
        let kind = match self.db_kind {
            DbKind::Oracle => "Oracle",
            DbKind::Postgres => "PostgreSQL",
        };
        format!(
            "{} · {} · {}:{} / {}",
            self.name, kind, self.host, self.port, self.database
        )
    }

    /// Builds a PostgreSQL URL for the Phase 0 spike.
    pub fn postgres_url(&self) -> String {
        format!(
            "postgres://{}:{}@{}:{}/{}",
            self.username, self.password, self.host, self.port, self.database
        )
    }

    /// Oracle easy-connect descriptor for the Phase 0 spike.
    pub fn oracle_connect_descriptor(&self) -> String {
        format!("{}:{}/{}", self.host, self.port, self.database)
    }
}
