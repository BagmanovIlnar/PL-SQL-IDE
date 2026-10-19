//! Persisted Oracle → Connection preferences and process-level client init.
//!
//! Values are stored in `{config}/oracle_connection.json` and applied once
//! before the first ODPI-C / `oracle` crate connection in this process.

use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use super::CoreResult;

/// How many DB sessions the IDE may open for Oracle work (session manager).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum OracleSessionMode {
    #[default]
    MultiSession,
    DualSession,
    SingleSession,
}

/// One Oracle process environment variable row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct OracleEnvVar {
    pub name: String,
    pub value: String,
}

/// Persisted Oracle → Connection preferences.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OracleConnectionPrefs {
    #[serde(default)]
    pub session_mode: OracleSessionMode,
    /// Empty = autodetect.
    #[serde(default)]
    pub oracle_home: String,
    /// Empty = autodetect. May be a library file or its directory.
    #[serde(default)]
    pub oci_library: String,
    #[serde(default)]
    pub env_vars: Vec<OracleEnvVar>,
}

impl Default for OracleConnectionPrefs {
    fn default() -> Self {
        Self {
            session_mode: OracleSessionMode::MultiSession,
            oracle_home: String::new(),
            oci_library: String::new(),
            env_vars: Vec::new(),
        }
    }
}

fn config_dir() -> PathBuf {
    ProjectDirs::from("com", "plsqlide", "PL-SQL-IDE")
        .map(|dirs| dirs.config_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".").join("config"))
}

/// Path to the preferences JSON file.
pub fn prefs_path() -> PathBuf {
    config_dir().join("oracle_connection.json")
}

/// Load Oracle connection prefs from disk (or defaults).
pub fn load() -> OracleConnectionPrefs {
    fs_read_prefs(&prefs_path()).unwrap_or_default()
}

fn fs_read_prefs(path: &Path) -> Option<OracleConnectionPrefs> {
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Persist Oracle connection prefs.
pub fn save(prefs: &OracleConnectionPrefs) {
    let path = prefs_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(prefs) {
        let _ = std::fs::write(path, json);
    }
}

/// Apply env vars from prefs to the current process (before ODPI init / connect).
pub fn apply_env_vars(prefs: &OracleConnectionPrefs) {
    if !prefs.oracle_home.is_empty() {
        // SAFETY: called once before Oracle client workers start using these vars.
        unsafe {
            std::env::set_var("ORACLE_HOME", &prefs.oracle_home);
        }
    }
    for var in &prefs.env_vars {
        let name = var.name.trim();
        if name.is_empty() {
            continue;
        }
        unsafe {
            std::env::set_var(name, &var.value);
        }
    }
}

/// Ensure ODPI-C is initialized using Preferences → Oracle → Connection.
///
/// Safe to call repeatedly; InitParams only take effect on the first init.
#[cfg(feature = "oracle")]
pub fn prepare_oracle_client() -> CoreResult<()> {
    use std::sync::OnceLock;

    use tracing::info;

    use super::CoreError;

    /// Directory that contains the OCI shared library (InitParams wants a dir).
    fn oci_library_dir(oci_library: &str) -> Option<PathBuf> {
        if oci_library.is_empty() {
            return None;
        }
        let path = PathBuf::from(oci_library);
        if path.is_dir() {
            return Some(path);
        }
        path.parent().map(|p| p.to_path_buf())
    }

    fn tns_admin_from_prefs(prefs: &OracleConnectionPrefs) -> Option<String> {
        prefs
            .env_vars
            .iter()
            .find(|v| v.name.eq_ignore_ascii_case("TNS_ADMIN"))
            .map(|v| v.value.clone())
            .filter(|v| !v.is_empty())
    }

    static PREPARED: OnceLock<Result<(), String>> = OnceLock::new();

    let result = PREPARED.get_or_init(|| {
        let prefs = load();
        apply_env_vars(&prefs);

        let mut params = oracle::InitParams::new();
        if let Some(dir) = oci_library_dir(&prefs.oci_library) {
            params
                .oracle_client_lib_dir(&dir)
                .map_err(|err| err.to_string())?;
            info!(lib_dir = %dir.display(), "oracle client lib dir from preferences");
        }
        if let Some(tns) = tns_admin_from_prefs(&prefs) {
            params
                .oracle_client_config_dir(&tns)
                .map_err(|err| err.to_string())?;
            info!(tns_admin = %tns, "oracle TNS_ADMIN from preferences");
        } else if !prefs.oracle_home.is_empty() {
            let network = PathBuf::from(&prefs.oracle_home)
                .join("network")
                .join("admin");
            if network.is_dir() {
                params
                    .oracle_client_config_dir(&network)
                    .map_err(|err| err.to_string())?;
            }
        }

        match params.init() {
            Ok(true) => {
                info!(
                    session_mode = ?prefs.session_mode,
                    "oracle client initialized from preferences"
                );
                Ok(())
            }
            Ok(false) => {
                info!("oracle client was already initialized; preference lib/config dirs ignored");
                Ok(())
            }
            Err(err) => Err(err.to_string()),
        }
    });

    result.clone().map_err(CoreError::Connection)
}

#[cfg(not(feature = "oracle"))]
pub fn prepare_oracle_client() -> CoreResult<()> {
    Ok(())
}
