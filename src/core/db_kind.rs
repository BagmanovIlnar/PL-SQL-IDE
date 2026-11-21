use serde::{Deserialize, Serialize};

/// Supported database dialects.
///
/// # Examples
///
/// ```
/// use plsqlide::core::DbKind;
///
/// assert_eq!(DbKind::Oracle.as_str(), "oracle");
/// assert_eq!(DbKind::Postgres.as_str(), "postgres");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DbKind {
    Oracle,
    Postgres,
}

impl DbKind {
    /// Returns a stable lowercase identifier used in UI and configs.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Oracle => "oracle",
            Self::Postgres => "postgres",
        }
    }
}

impl std::fmt::Display for DbKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Oracle => write!(f, "Oracle"),
            Self::Postgres => write!(f, "PostgreSQL"),
        }
    }
}

/// Oracle administrative connect role ("Connect as" in the connection dialog).
///
/// # Examples
///
/// ```
/// use plsqlide::core::ConnectAs;
///
/// assert_eq!(ConnectAs::default(), ConnectAs::Normal);
/// assert_eq!(ConnectAs::Sysdba.as_str(), "sysdba");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConnectAs {
    #[default]
    Normal,
    Sysdba,
    Sysoper,
}

impl ConnectAs {
    /// Stable lowercase identifier for configs.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Sysdba => "sysdba",
            Self::Sysoper => "sysoper",
        }
    }
}

impl std::fmt::Display for ConnectAs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Normal => write!(f, "{}", crate::ui::i18n::t("role.normal")),
            Self::Sysdba => write!(f, "SYSDBA"),
            Self::Sysoper => write!(f, "SYSOPER"),
        }
    }
}
