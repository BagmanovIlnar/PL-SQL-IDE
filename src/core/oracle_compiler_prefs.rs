//! Persisted Oracle → Compiler preferences.
//!
//! These map to session / compile settings (`PLSQL_OPTIMIZE_LEVEL`,
//! `PLSQL_CODE_TYPE`, `PLSCOPE_SETTINGS`, `PLSQL_CCFLAGS`, `PLSQL_WARNINGS`,
//! and SQL*Plus-style substitution variables) applied when compiling against
//! the database.

use std::path::PathBuf;

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

fn default_optimize_level() -> u8 {
    2
}

/// `PLSQL_CODE_TYPE` session value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PlsqlCodeType {
    #[default]
    Interpreted,
    Native,
}

impl PlsqlCodeType {
    pub fn all() -> [Self; 2] {
        [Self::Interpreted, Self::Native]
    }

    pub fn as_oracle(self) -> &'static str {
        match self {
            Self::Interpreted => "INTERPRETED",
            Self::Native => "NATIVE",
        }
    }
}

/// Identifier part of `PLSCOPE_SETTINGS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PlscopeIdentifiers {
    None,
    All,
    #[default]
    Default,
    Public,
    Sql,
    Plsql,
}

impl PlscopeIdentifiers {
    pub fn all() -> [Self; 6] {
        [
            Self::None,
            Self::All,
            Self::Default,
            Self::Public,
            Self::Sql,
            Self::Plsql,
        ]
    }

    pub fn as_oracle(self) -> &'static str {
        match self {
            Self::None => "NONE",
            Self::All => "ALL",
            Self::Default => "DEFAULT",
            Self::Public => "PUBLIC",
            Self::Sql => "SQL",
            Self::Plsql => "PLSQL",
        }
    }
}

/// Statements part of `PLSCOPE_SETTINGS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PlscopeStatements {
    None,
    All,
    #[default]
    Default,
}

impl PlscopeStatements {
    pub fn all() -> [Self; 3] {
        [Self::None, Self::All, Self::Default]
    }

    pub fn as_oracle(self) -> &'static str {
        match self {
            Self::None => "NONE",
            Self::All => "ALL",
            Self::Default => "DEFAULT",
        }
    }
}

/// Action for a `PLSQL_WARNINGS` entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PlsqlWarningAction {
    #[default]
    Disabled,
    Warning,
    Error,
}

impl PlsqlWarningAction {
    pub fn all() -> [Self; 3] {
        [Self::Disabled, Self::Warning, Self::Error]
    }

    pub fn as_oracle(self) -> &'static str {
        match self {
            Self::Disabled => "DISABLE",
            Self::Warning => "ENABLE",
            Self::Error => "ERROR",
        }
    }
}

/// Name / value pair (CC flags or substitution variables).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct NameValuePair {
    pub name: String,
    pub value: String,
}

/// One `PLSQL_WARNINGS` entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlsqlWarningEntry {
    pub action: PlsqlWarningAction,
    pub exception: String,
}

impl Default for PlsqlWarningEntry {
    fn default() -> Self {
        Self {
            action: PlsqlWarningAction::Disabled,
            exception: String::new(),
        }
    }
}

/// Oracle → Compiler preferences.
///
/// # Examples
///
/// ```
/// use plsqlide::core::oracle_compiler_prefs::{OracleCompilerPrefs, PlsqlCodeType};
///
/// let prefs = OracleCompilerPrefs::default();
/// assert_eq!(prefs.optimize_level, 2);
/// assert_eq!(prefs.code_type, PlsqlCodeType::Interpreted);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OracleCompilerPrefs {
    /// `PLSQL_OPTIMIZE_LEVEL` (0–3), default 2.
    #[serde(default = "default_optimize_level")]
    pub optimize_level: u8,
    #[serde(default)]
    pub code_type: PlsqlCodeType,
    #[serde(default)]
    pub plscope_identifiers: PlscopeIdentifiers,
    #[serde(default)]
    pub plscope_statements: PlscopeStatements,
    #[serde(default)]
    pub conditional_compilation_flags: Vec<NameValuePair>,
    #[serde(default)]
    pub warnings: Vec<PlsqlWarningEntry>,
    #[serde(default)]
    pub substitution_variables: Vec<NameValuePair>,
}

impl Default for OracleCompilerPrefs {
    fn default() -> Self {
        Self {
            optimize_level: 2,
            code_type: PlsqlCodeType::Interpreted,
            plscope_identifiers: PlscopeIdentifiers::Default,
            plscope_statements: PlscopeStatements::Default,
            conditional_compilation_flags: Vec::new(),
            warnings: Vec::new(),
            substitution_variables: Vec::new(),
        }
    }
}

impl OracleCompilerPrefs {
    /// Clamp optimize level into the Oracle-supported 0..=3 range.
    pub fn clamp_optimize_level(level: u8) -> u8 {
        level.min(3)
    }

    /// `ALTER SESSION` statements that apply these prefs before compile.
    ///
    /// # Examples
    ///
    /// ```
    /// use plsqlide::core::oracle_compiler_prefs::OracleCompilerPrefs;
    ///
    /// let sql = OracleCompilerPrefs::default().alter_session_sql();
    /// assert!(sql.iter().any(|s| s.contains("PLSQL_OPTIMIZE_LEVEL")));
    /// ```
    pub fn alter_session_sql(&self) -> Vec<String> {
        let level = Self::clamp_optimize_level(self.optimize_level);
        let mut stmts = vec![
            format!("ALTER SESSION SET PLSQL_OPTIMIZE_LEVEL = {level}"),
            format!(
                "ALTER SESSION SET PLSQL_CODE_TYPE = '{}'",
                self.code_type.as_oracle()
            ),
            format!(
                "ALTER SESSION SET PLSCOPE_SETTINGS = 'IDENTIFIERS:{} STATEMENTS:{}'",
                self.plscope_identifiers.as_oracle(),
                self.plscope_statements.as_oracle()
            ),
        ];

        if !self.conditional_compilation_flags.is_empty() {
            let flags = self
                .conditional_compilation_flags
                .iter()
                .filter(|f| !f.name.trim().is_empty())
                .map(|f| format!("{}:{}", f.name.trim(), f.value.trim()))
                .collect::<Vec<_>>()
                .join(", ");
            if !flags.is_empty() {
                stmts.push(format!("ALTER SESSION SET PLSQL_CCFLAGS = '{flags}'"));
            }
        }

        if !self.warnings.is_empty() {
            let parts = self
                .warnings
                .iter()
                .filter(|w| !w.exception.trim().is_empty())
                .map(|w| format!("{}:{}", w.action.as_oracle(), w.exception.trim()))
                .collect::<Vec<_>>()
                .join(", ");
            if !parts.is_empty() {
                stmts.push(format!("ALTER SESSION SET PLSQL_WARNINGS = '{parts}'"));
            }
        }

        stmts
    }
}

fn config_dir() -> PathBuf {
    ProjectDirs::from("com", "plsqlide", "PL-SQL-IDE")
        .map(|dirs| dirs.config_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".").join("config"))
}

/// Path to `{config}/oracle_compiler.json`.
pub fn prefs_path() -> PathBuf {
    config_dir().join("oracle_compiler.json")
}

/// Load compiler prefs from disk (or defaults).
pub fn load() -> OracleCompilerPrefs {
    let mut prefs: OracleCompilerPrefs = std::fs::read_to_string(prefs_path())
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();
    prefs.optimize_level = OracleCompilerPrefs::clamp_optimize_level(prefs.optimize_level);
    prefs
}

/// Persist compiler prefs to disk.
pub fn save(prefs: &OracleCompilerPrefs) {
    let path = prefs_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut prefs = prefs.clone();
    prefs.optimize_level = OracleCompilerPrefs::clamp_optimize_level(prefs.optimize_level);
    if let Ok(json) = serde_json::to_string_pretty(&prefs) {
        let _ = std::fs::write(path, json);
    }
}

/// Apply compiler prefs to an open Oracle session (`ALTER SESSION` statements).
///
/// Call after connect (and before compile) so Preferences → Oracle → Compiler
/// affect database work. Substitution variables are client-side DEFINE names
/// and are not sent here; they are available via [`load`] for SQL preprocessing.
#[cfg(feature = "oracle")]
pub fn apply_to_connection(connection: &oracle::Connection) -> super::CoreResult<()> {
    use tracing::info;

    use super::CoreError;

    let prefs = load();
    for sql in prefs.alter_session_sql() {
        connection
            .execute(&sql, &[])
            .map_err(|err| CoreError::Query(err.to_string()))?;
        info!(%sql, "applied oracle compiler preference");
    }
    Ok(())
}

