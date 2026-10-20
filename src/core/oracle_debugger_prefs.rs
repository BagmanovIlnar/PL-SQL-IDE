//! Persisted Oracle → Debugger preferences.
//!
//! Affects compile (`WITH DEBUG`) and debug-session behaviour
//! (DBMS_DEBUG / DBMS_DEBUG_JDWP defaults, step-over filters).

use std::path::PathBuf;

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

fn default_true() -> bool {
    true
}

fn default_jdwp_port() -> u16 {
    4001
}

fn default_network_adapter() -> String {
    "All".to_string()
}

/// Which Oracle debug API to use by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum DebugSystem {
    #[default]
    DbmsDebug,
    DbmsDebugJdwp,
}

impl DebugSystem {
    pub fn all() -> [Self; 2] {
        [Self::DbmsDebug, Self::DbmsDebugJdwp]
    }

    pub fn as_oracle(self) -> &'static str {
        match self {
            Self::DbmsDebug => "DBMS_DEBUG",
            Self::DbmsDebugJdwp => "DBMS_DEBUG_JDWP",
        }
    }
}

/// Oracle → Debugger preferences.
///
/// # Examples
///
/// ```
/// use plsqlide::core::oracle_debugger_prefs::{DebugSystem, OracleDebuggerPrefs};
///
/// let prefs = OracleDebuggerPrefs::default();
/// assert!(prefs.show_variable_values_popup);
/// assert_eq!(prefs.default_debug_system, DebugSystem::DbmsDebug);
/// assert_eq!(prefs.default_jdwp_port, 4001);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OracleDebuggerPrefs {
    /// Show variable values in a hover popup while debugging.
    #[serde(default = "default_true")]
    pub show_variable_values_popup: bool,
    /// Skip package specification initialization when stepping.
    #[serde(default)]
    pub step_over_package_spec_init: bool,
    /// Skip package body initialization when stepping.
    #[serde(default)]
    pub step_over_package_body_init: bool,
    /// Do not step into SYS-owned objects.
    #[serde(default = "default_true")]
    pub step_over_sys_objects: bool,
    #[serde(default)]
    pub default_debug_system: DebugSystem,
    #[serde(default = "default_jdwp_port")]
    pub default_jdwp_port: u16,
    /// Network adapter name for JDWP listen bind (`All` = any).
    #[serde(default = "default_network_adapter")]
    pub default_jdwp_network_adapter: String,
    /// When compiling, attach debug information by default.
    #[serde(default = "default_true")]
    pub add_debug_info_when_compiling: bool,
    /// One object name / pattern per line — never compile WITH DEBUG.
    #[serde(default)]
    pub never_debug_objects: String,
    /// One object name / pattern per line — always compile WITH DEBUG.
    #[serde(default)]
    pub always_debug_objects: String,
    /// One `user@db` connection key per line — never compile WITH DEBUG.
    #[serde(default)]
    pub never_debug_connections: String,
}

impl Default for OracleDebuggerPrefs {
    fn default() -> Self {
        Self {
            show_variable_values_popup: true,
            step_over_package_spec_init: false,
            step_over_package_body_init: false,
            step_over_sys_objects: true,
            default_debug_system: DebugSystem::DbmsDebug,
            default_jdwp_port: 4001,
            default_jdwp_network_adapter: default_network_adapter(),
            add_debug_info_when_compiling: true,
            never_debug_objects: String::new(),
            always_debug_objects: String::new(),
            never_debug_connections: String::new(),
        }
    }
}

impl OracleDebuggerPrefs {
    pub fn clamp_jdwp_port(port: u16) -> u16 {
        if port == 0 {
            4001
        } else {
            port
        }
    }

    fn lines_set(block: &str) -> Vec<String> {
        block
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect()
    }

    fn matches_pattern(pattern: &str, value: &str) -> bool {
        let p = pattern.trim();
        let v = value.trim();
        if p.is_empty() {
            return false;
        }
        if p.contains('*') {
            let lower_v = v.to_ascii_lowercase();
            let lower_p = p.to_ascii_lowercase();
            if let Some((pre, rest)) = lower_p.split_once('*') {
                return lower_v.starts_with(pre) && lower_v.ends_with(rest.trim_start_matches('*'));
            }
        }
        p.eq_ignore_ascii_case(v)
    }

    /// Whether compile of `object_name` on `connection_key` (`user@db`) should use debug info.
    ///
    /// Priority: connection NEVER → object ALWAYS → object NEVER → global default.
    pub fn should_add_debug_info(&self, connection_key: &str, object_name: Option<&str>) -> bool {
        for line in Self::lines_set(&self.never_debug_connections) {
            if Self::matches_pattern(&line, connection_key) {
                return false;
            }
        }
        if let Some(name) = object_name {
            for line in Self::lines_set(&self.always_debug_objects) {
                if Self::matches_pattern(&line, name) {
                    return true;
                }
            }
            for line in Self::lines_set(&self.never_debug_objects) {
                if Self::matches_pattern(&line, name) {
                    return false;
                }
            }
        }
        self.add_debug_info_when_compiling
    }

    /// SQL fragment for compile: empty or ` DEBUG` (as in `ALTER … COMPILE DEBUG`).
    pub fn compile_debug_clause(&self, connection_key: &str, object_name: Option<&str>) -> &'static str {
        if self.should_add_debug_info(connection_key, object_name) {
            " DEBUG"
        } else {
            ""
        }
    }
}

/// Local network adapter choices for the JDWP combo (`All` first).
///
/// Discovers IPv4 addresses from the OS when possible; always includes `All`.
pub fn list_jdwp_network_adapters() -> Vec<String> {
    let mut adapters = vec!["All".to_string()];
    push_unique(&mut adapters, "127.0.0.1");
    push_unique(&mut adapters, "0.0.0.0");

    if let Ok(ifaces) = local_ipv4_addresses() {
        for addr in ifaces {
            push_unique(&mut adapters, &addr);
        }
    }
    adapters
}

fn push_unique(list: &mut Vec<String>, value: &str) {
    if !list.iter().any(|a| a == value) {
        list.push(value.to_string());
    }
}

fn local_ipv4_addresses() -> std::io::Result<Vec<String>> {
    use std::net::UdpSocket;
    // Probe route: connect UDP without sending to learn the primary outbound IP.
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    let _ = socket.connect("8.8.8.8:80");
    let mut addrs = Vec::new();
    if let Ok(local) = socket.local_addr() {
        addrs.push(local.ip().to_string());
    }
    Ok(addrs)
}

fn config_dir() -> PathBuf {
    ProjectDirs::from("com", "plsqlide", "PL-SQL-IDE")
        .map(|dirs| dirs.config_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".").join("config"))
}

pub fn prefs_path() -> PathBuf {
    config_dir().join("oracle_debugger.json")
}

pub fn load() -> OracleDebuggerPrefs {
    let mut prefs: OracleDebuggerPrefs = std::fs::read_to_string(prefs_path())
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();
    prefs.default_jdwp_port = OracleDebuggerPrefs::clamp_jdwp_port(prefs.default_jdwp_port);
    if prefs.default_jdwp_network_adapter.trim().is_empty() {
        prefs.default_jdwp_network_adapter = default_network_adapter();
    }
    prefs
}

pub fn save(prefs: &OracleDebuggerPrefs) {
    let path = prefs_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut prefs = prefs.clone();
    prefs.default_jdwp_port = OracleDebuggerPrefs::clamp_jdwp_port(prefs.default_jdwp_port);
    if let Ok(json) = serde_json::to_string_pretty(&prefs) {
        let _ = std::fs::write(path, json);
    }
}
