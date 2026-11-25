//! PostgreSQL input-format preferences (Preferences → PostgreSQL →
//! Formats). Postgres has no NLS — these are client-side input
//! conventions: the Date parameter input is filtered by the mask, and the
//! bind is emitted as an unambiguous ISO literal, so the server session is
//! never modified.

use std::path::PathBuf;

use crate::core::oracle_nls_prefs::{strftime_to_oracle_mask, windows_date_format};

/// Source of the PostgreSQL date input mask.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub enum PgDateSource {
    /// The OS locale date format, converted to an Oracle-style mask.
    #[default]
    Os,
    /// The user-typed Oracle-style mask (`DD.MM.YYYY`).
    UserDefined,
}

impl PgDateSource {
    /// i18n key for the source radio label.
    pub fn key(self) -> &'static str {
        match self {
            Self::Os => "prefs.pg.formats.source.os",
            Self::UserDefined => "prefs.pg.formats.source.user",
        }
    }
}

/// PostgreSQL input formats.
///
/// # Examples
///
/// ```
/// use plsqlide::core::postgres_format_prefs::{PgDateSource, PostgresFormatPrefs};
///
/// let prefs = PostgresFormatPrefs::default();
/// assert_eq!(prefs.date_source, PgDateSource::Os);
/// // An Oracle-style mask, never a raw strftime pattern.
/// assert!(!prefs.resolved_date_input().contains('%'));
/// ```
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PostgresFormatPrefs {
    /// User defined date input mask — an Oracle-style mask
    /// (e.g. `DD.MM.YYYY`); used only when the source is User defined.
    #[serde(default = "default_date_input")]
    pub date_input: String,
    /// Source of the date input mask.
    #[serde(default)]
    pub date_source: PgDateSource,
}

/// OS date format converted to an Oracle-style mask — `DD.MM.YYYY` on
/// day-first locales, `MM/DD/YYYY` on month-first ones.
fn default_date_input() -> String {
    let mask = strftime_to_oracle_mask(&windows_date_format());
    // Exotic OS pattern (unmapped % tokens) — fall back to a sane default.
    if mask.is_empty() || mask.contains('%') {
        "DD.MM.YYYY".to_string()
    } else {
        mask
    }
}

impl PostgresFormatPrefs {
    /// The date input mask the parameter editor actually follows.
    pub fn resolved_date_input(&self) -> String {
        match self.date_source {
            PgDateSource::Os => default_date_input(),
            PgDateSource::UserDefined => self.date_input.clone(),
        }
    }
}

impl Default for PostgresFormatPrefs {
    fn default() -> Self {
        Self {
            date_input: default_date_input(),
            date_source: PgDateSource::default(),
        }
    }
}

/// Config file path (`{config}/postgres_formats.json`).
pub fn prefs_path() -> PathBuf {
    crate::core::oracle_nls_prefs::config_dir().join("postgres_formats.json")
}

/// Load PostgreSQL format prefs from disk (or defaults).
pub fn load() -> PostgresFormatPrefs {
    std::fs::read_to_string(prefs_path())
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// Persist PostgreSQL format prefs to disk.
pub fn save(prefs: &PostgresFormatPrefs) {
    let path = prefs_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(prefs) {
        let _ = std::fs::write(path, json);
    }
}
