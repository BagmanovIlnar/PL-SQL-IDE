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
