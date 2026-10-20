//! Persisted Oracle → Options preferences.
//!
//! Flags and DDL policies that affect database-object workflows (edit, save,
//! compile, commit, metadata views, charset checks, view DDL, scheduler, owner
//! prefixes).

use std::path::PathBuf;

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

fn default_true() -> bool {
    true
}

fn default_false() -> bool {
    false
}

/// When to add a column alias list in generated view DDL.
///
/// # Examples
///
/// ```
/// use plsqlide::core::oracle_options_prefs::ViewColumnAliasPolicy;
///
/// assert_eq!(ViewColumnAliasPolicy::default(), ViewColumnAliasPolicy::Auto);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ViewColumnAliasPolicy {
    #[default]
    Auto,
    Always,
    Never,
}

impl ViewColumnAliasPolicy {
    pub fn all() -> [Self; 3] {
        [Self::Auto, Self::Always, Self::Never]
    }
}

/// When to emit `FORCE` in generated view DDL.
///
/// # Examples
///
/// ```
/// use plsqlide::core::oracle_options_prefs::ViewForcePolicy;
///
/// assert_eq!(ViewForcePolicy::default(), ViewForcePolicy::OnlyForExports);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ViewForcePolicy {
    #[default]
    OnlyForExports,
    Always,
    Never,
}

impl ViewForcePolicy {
    pub fn all() -> [Self; 3] {
        [Self::OnlyForExports, Self::Always, Self::Never]
    }
}

/// Oracle → Options preferences.
///
/// # Examples
///
/// ```
/// use plsqlide::core::oracle_options_prefs::{
///     OracleOptionsPrefs, ViewColumnAliasPolicy, ViewForcePolicy,
/// };
///
/// let prefs = OracleOptionsPrefs::default();
/// assert!(prefs.allow_edit_database_source);
/// assert!(!prefs.use_dba_views_if_available);
/// assert!(prefs.unicode_enabled);
/// assert_eq!(prefs.view_column_alias_policy, ViewColumnAliasPolicy::Auto);
/// assert_eq!(prefs.view_force_policy, ViewForcePolicy::OnlyForExports);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OracleOptionsPrefs {
    /// When false, source loaded from the database is opened read-only.
    #[serde(default = "default_true")]
    pub allow_edit_database_source: bool,
    /// When true, closing a dirty DB-sourced editor asks to save.
    #[serde(default = "default_true")]
    pub ask_save_edited_database_source: bool,
    /// When true, compile is allowed even if the source buffer is read-only.
    #[serde(default = "default_true")]
    pub allow_compile_readonly_source: bool,
    /// When true, Commit / Rollback require an explicit confirmation dialog.
    #[serde(default = "default_true")]
    pub confirm_commit_rollback: bool,

    /// Prefer `DBA_*` dictionary views when the session can see them.
    #[serde(default = "default_false")]
    pub use_dba_views_if_available: bool,
    /// Prefer Unicode-aware client / session character handling.
    #[serde(default = "default_true")]
    pub unicode_enabled: bool,
    /// Warn when client and server character sets differ.
    #[serde(default = "default_false")]
    pub check_charset_mismatch: bool,
    /// Policy for column alias lists in view DDL.
    #[serde(default)]
    pub view_column_alias_policy: ViewColumnAliasPolicy,
    /// Policy for `FORCE` in view DDL.
    #[serde(default)]
    pub view_force_policy: ViewForcePolicy,
    /// Use timestamp formats when generating scheduler object DDL.
    #[serde(default = "default_false")]
    pub use_timestamp_format_for_scheduler: bool,
    /// Always qualify object names with the owner/schema prefix in generated SQL.
    #[serde(default = "default_false")]
    pub always_include_owner_prefix: bool,
}

impl Default for OracleOptionsPrefs {
    fn default() -> Self {
        Self {
            allow_edit_database_source: true,
            ask_save_edited_database_source: true,
            allow_compile_readonly_source: true,
            confirm_commit_rollback: true,
            use_dba_views_if_available: false,
            unicode_enabled: true,
            check_charset_mismatch: false,
            view_column_alias_policy: ViewColumnAliasPolicy::Auto,
            view_force_policy: ViewForcePolicy::OnlyForExports,
            use_timestamp_format_for_scheduler: false,
            always_include_owner_prefix: false,
        }
    }
}

fn config_dir() -> PathBuf {
    ProjectDirs::from("com", "plsqlide", "PL-SQL-IDE")
        .map(|dirs| dirs.config_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".").join("config"))
}

/// Path to `{config}/oracle_options.json`.
pub fn prefs_path() -> PathBuf {
    config_dir().join("oracle_options.json")
}

/// Load options from disk (or defaults).
pub fn load() -> OracleOptionsPrefs {
    std::fs::read_to_string(prefs_path())
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// Persist options to disk.
pub fn save(prefs: &OracleOptionsPrefs) {
    let path = prefs_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(prefs) {
        let _ = std::fs::write(path, json);
    }
}
