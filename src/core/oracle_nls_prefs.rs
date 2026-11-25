//! Persisted Oracle → NLS Options preferences.
//!
//! Source of the Date / Time / Number formats shown in the IDE: user-defined
//! strings, Windows regional settings, or the Oracle session formats.

use std::path::PathBuf;
use std::sync::Mutex;

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

/// Where the displayed date format comes from.
///
/// # Examples
///
/// ```
/// use plsqlide::core::oracle_nls_prefs::NlsDateSource;
///
/// assert_eq!(NlsDateSource::default(), NlsDateSource::Windows);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum NlsDateSource {
    UserDefined,
    #[default]
    Windows,
    Oracle,
}

impl NlsDateSource {
    pub fn all() -> [Self; 3] {
        [Self::UserDefined, Self::Windows, Self::Oracle]
    }
}

/// Where the displayed time format comes from (no Oracle variant: the
/// session's NLS_TIME_FORMAT is rarely set, PL/SQL Developer omits it too).
///
/// # Examples
///
/// ```
/// use plsqlide::core::oracle_nls_prefs::NlsTimeSource;
///
/// assert_eq!(NlsTimeSource::default(), NlsTimeSource::Windows);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum NlsTimeSource {
    UserDefined,
    #[default]
    Windows,
}

impl NlsTimeSource {
    pub fn all() -> [Self; 2] {
        [Self::UserDefined, Self::Windows]
    }
}

/// Where the displayed number format comes from.
///
/// # Examples
///
/// ```
/// use plsqlide::core::oracle_nls_prefs::NlsNumberSource;
///
/// assert_eq!(NlsNumberSource::default(), NlsNumberSource::Windows);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum NlsNumberSource {
    UserDefined,
    #[default]
    Windows,
    Oracle,
}

impl NlsNumberSource {
    pub fn all() -> [Self; 3] {
        [Self::UserDefined, Self::Windows, Self::Oracle]
    }
}

/// Decimal / digit-group separator symbol.
///
/// # Examples
///
/// ```
/// use plsqlide::core::oracle_nls_prefs::NlsSymbol;
///
/// assert_eq!(NlsSymbol::Dot.to_string(), ".");
/// assert_eq!(NlsSymbol::Comma.to_string(), ",");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum NlsSymbol {
    #[default]
    Dot,
    Comma,
}

impl NlsSymbol {
    pub fn all() -> [Self; 2] {
        [Self::Dot, Self::Comma]
    }
}

impl std::fmt::Display for NlsSymbol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Dot => ".",
            Self::Comma => ",",
        })
    }
}

/// Date format default: the Oracle instance client's NLS_DATE_FORMAT when it
/// publishes one, otherwise the PL/SQL Developer default `M/d/yyyy`.
// ponytail: client NLS read from the process env; a live session could query
// v$nls_parameters after connect — switch then if env proves unreliable.
fn default_date_format() -> String {
    std::env::var("NLS_DATE_FORMAT")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "MM/DD/YYYY".to_string())
}

/// Oracle → NLS Options preferences.
///
/// # Examples
///
/// ```
/// use plsqlide::core::oracle_nls_prefs::{
///     NlsDateSource, NlsNumberSource, NlsSymbol, NlsTimeSource, OracleNlsPrefs,
/// };
///
/// let prefs = OracleNlsPrefs::default();
/// assert_eq!(prefs.date_format, "MM/DD/YYYY");
/// assert_eq!(prefs.time_format, "HH:MI:SS AM");
/// assert_eq!(prefs.date_source, NlsDateSource::Windows);
/// assert_eq!(prefs.time_source, NlsTimeSource::Windows);
/// assert_eq!(prefs.decimal_symbol, NlsSymbol::Dot);
/// assert_eq!(prefs.group_symbol, NlsSymbol::Comma);
/// assert_eq!(prefs.number_source, NlsNumberSource::Windows);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OracleNlsPrefs {
    /// Date format string — an Oracle NLS mask, e.g. `DD/MM/YYYY`
    /// (User defined value; the other sources resolve their own masks).
    #[serde(default = "default_date_format")]
    pub date_format: String,
    /// Source of the displayed date format.
    #[serde(default)]
    pub date_source: NlsDateSource,
    /// Time format string — an Oracle NLS mask, e.g. `HH24:MI:SS`
    /// (User defined value; Windows resolves its own).
    #[serde(default = "default_time_format")]
    pub time_format: String,
    /// Source of the displayed time format.
    #[serde(default)]
    pub time_source: NlsTimeSource,
    /// Decimal separator (User defined only).
    #[serde(default)]
    pub decimal_symbol: NlsSymbol,
    /// Digit group separator (User defined only).
    #[serde(default = "default_group_symbol")]
    pub group_symbol: NlsSymbol,
    /// Source of the displayed number format.
    #[serde(default)]
    pub number_source: NlsNumberSource,
}

fn default_time_format() -> String {
    "HH:MI:SS AM".to_string()
}

fn default_group_symbol() -> NlsSymbol {
    NlsSymbol::Comma
}

/// Session NLS parameters captured from the last Oracle connect — the
/// Oracle Format source of the NLS Options.
// ponytail: single global — the IDE talks to one instance at a time;
// per-connection state if that ever changes.
#[derive(Debug, Clone, Default)]
pub struct SessionNls {
    /// NLS_DATE_FORMAT of the session ("" when unset).
    pub date_format: String,
    /// NLS_TIME_FORMAT of the session ("" when unset).
    pub time_format: String,
    /// NLS_NUMERIC_CHARACTERS, e.g. ".," — [0] decimal, [1] group.
    pub numeric_characters: String,
}

static SESSION_NLS: Mutex<Option<SessionNls>> = Mutex::new(None);

/// Store the session NLS captured right after an Oracle connect.
pub fn set_session_nls(nls: SessionNls) {
    *SESSION_NLS.lock().unwrap() = Some(nls);
}

/// Session NLS of the last connect, if any.
pub fn session_nls() -> Option<SessionNls> {
    SESSION_NLS.lock().unwrap().clone()
}

/// Formats the process locale once from the environment (LANG / LC_*), so
/// the langinfo calls below report the user's regional settings and not C.
fn init_locale() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| unsafe {
        libc::setlocale(libc::LC_ALL, c"".as_ptr());
    });
}

/// OS regional ("Windows") short date format — the locale's strftime pattern.
/// Not the user-typed string: PL/SQL Developer reads the same from the
/// Windows control panel.
pub fn windows_date_format() -> String {
    init_locale();
    #[cfg(unix)]
    unsafe {
        std::ffi::CStr::from_ptr(libc::nl_langinfo(libc::D_FMT))
            .to_string_lossy()
            .into_owned()
    }
    #[cfg(not(unix))]
    {
        "%x".to_string()
    }
}

/// OS regional ("Windows") time format — the locale's strftime pattern.
pub fn windows_time_format() -> String {
    init_locale();
    #[cfg(unix)]
    unsafe {
        std::ffi::CStr::from_ptr(libc::nl_langinfo(libc::T_FMT))
            .to_string_lossy()
            .into_owned()
    }
    #[cfg(not(unix))]
    {
        "%X".to_string()
    }
}

/// OS regional number separators: (decimal, digit group).
fn windows_number_symbols() -> (String, String) {
    init_locale();
    #[cfg(unix)]
    {
        unsafe {
            let lc = libc::localeconv();
            if !lc.is_null() {
                let decimal = std::ffi::CStr::from_ptr((*lc).decimal_point)
                    .to_string_lossy()
                    .into_owned();
                let group = std::ffi::CStr::from_ptr((*lc).thousands_sep)
                    .to_string_lossy()
                    .into_owned();
                if !decimal.is_empty() {
                    return (decimal, group);
                }
            }
        }
    }
    (".".to_string(), ",".to_string())
}

/// Session value, else the client's NLS env, else `fallback`.
fn oracle_session_or_env(session: String, env_var: &str, fallback: &str) -> String {
    if !session.is_empty() {
        return session;
    }
    std::env::var(env_var)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

/// Converts an OS strftime pattern (`%d/%m/%Y`) into the Oracle NLS format
/// mask Oracle actually understands (`DD/MM/YYYY`). Non-`%` text passes
/// through untouched — Oracle masks and literals survive as-is.
///
/// # Examples
///
/// ```
/// use plsqlide::core::oracle_nls_prefs::strftime_to_oracle_mask;
///
/// assert_eq!(strftime_to_oracle_mask("%d/%m/%Y"), "DD/MM/YYYY");
/// assert_eq!(strftime_to_oracle_mask("%H:%M:%S"), "HH24:MI:SS");
/// assert_eq!(strftime_to_oracle_mask("DD.MM.YYYY"), "DD.MM.YYYY");
/// ```
pub fn strftime_to_oracle_mask(pattern: &str) -> String {
    let mask = |token: char| {
        match token {
            'Y' => "YYYY",
            'y' => "YY",
            'm' => "MM",
            'd' => "DD",
            'H' => "HH24",
            'I' => "HH12",
            'M' => "MI",
            'S' => "SS",
            'p' => "AM",
            'a' => "DY",
            'A' => "DAY",
            'b' | 'h' => "MON",
            'B' => "MONTH",
            'j' => "DDD",
            _ => "",
        }
        .to_string()
    };
    let mut out = String::new();
    let mut chars = pattern.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('%') => out.push('%'),
            Some(token) => out.push_str(&mask(token)),
            None => out.push('%'),
        }
    }
    out
}

impl Default for OracleNlsPrefs {
    fn default() -> Self {
        Self {
            date_format: default_date_format(),
            date_source: NlsDateSource::Windows,
            time_format: default_time_format(),
            time_source: NlsTimeSource::Windows,
            decimal_symbol: NlsSymbol::Dot,
            group_symbol: NlsSymbol::Comma,
            number_source: NlsNumberSource::Windows,
        }
    }
}

impl OracleNlsPrefs {
    /// Date format actually in effect for the selected source: the typed
    /// string (User defined), the OS regional format (Windows), or the
    /// session / client NLS (Oracle).
    pub fn resolved_date_format(&self) -> String {
        match self.date_source {
            NlsDateSource::UserDefined => self.date_format.clone(),
            NlsDateSource::Windows => windows_date_format(),
            NlsDateSource::Oracle => oracle_session_or_env(
                session_nls().map(|nls| nls.date_format).unwrap_or_default(),
                "NLS_DATE_FORMAT",
                "MM/DD/YYYY",
            ),
        }
    }

    /// Time format actually in effect for the selected source.
    pub fn resolved_time_format(&self) -> String {
        match self.time_source {
            NlsTimeSource::UserDefined => self.time_format.clone(),
            NlsTimeSource::Windows => windows_time_format(),
        }
    }

    /// Decimal separator actually in effect for the selected source.
    pub fn resolved_decimal_symbol(&self) -> String {
        match self.number_source {
            NlsNumberSource::UserDefined => self.decimal_symbol.to_string(),
            NlsNumberSource::Windows => windows_number_symbols().0,
            NlsNumberSource::Oracle => oracle_session_or_env(
                session_nls()
                    .map(|nls| nls.numeric_characters.chars().next().map(|c| c.to_string()))
                    .flatten()
                    .unwrap_or_default(),
                "NLS_NUMERIC_CHARACTERS",
                ".",
            ),
        }
    }

    /// Digit group separator actually in effect for the selected source.
    pub fn resolved_group_symbol(&self) -> String {
        match self.number_source {
            NlsNumberSource::UserDefined => self.group_symbol.to_string(),
            NlsNumberSource::Windows => windows_number_symbols().1,
            NlsNumberSource::Oracle => oracle_session_or_env(
                session_nls()
                    .and_then(|nls| nls.numeric_characters.chars().nth(1).map(|c| c.to_string()))
                    .unwrap_or_default(),
                "NLS_NUMERIC_CHARACTERS",
                ",",
            ),
        }
    }
}

pub(crate) fn config_dir() -> PathBuf {
    ProjectDirs::from("com", "plsqlide", "PL-SQL-IDE")
        .map(|dirs| dirs.config_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".").join("config"))
}

/// Path to `{config}/oracle_nls.json`.
pub fn prefs_path() -> PathBuf {
    config_dir().join("oracle_nls.json")
}

/// Load NLS options from disk (or defaults).
pub fn load() -> OracleNlsPrefs {
    std::fs::read_to_string(prefs_path())
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// Persist NLS options to disk.
pub fn save(prefs: &OracleNlsPrefs) {
    let path = prefs_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(prefs) {
        let _ = std::fs::write(path, json);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_prefields() {
        let prefs = OracleNlsPrefs {
            date_format: "DD.MM.YYYY".into(),
            date_source: NlsDateSource::UserDefined,
            decimal_symbol: NlsSymbol::Comma,
            group_symbol: NlsSymbol::Dot,
            number_source: NlsNumberSource::Oracle,
            ..OracleNlsPrefs::default()
        };
        let json = serde_json::to_string(&prefs).unwrap();
        let back: OracleNlsPrefs = serde_json::from_str(&json).unwrap();
        assert_eq!(back, prefs);
    }
}
