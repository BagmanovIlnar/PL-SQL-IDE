//! Object browser metadata: owners/schemas and object names per dialect.
//!
//! Opens a short-lived connection (same pattern as spikes), runs catalog queries,
//! then closes. No connection pool.

use sqlx::postgres::PgConnectOptions;
use sqlx::{ConnectOptions, Connection, PgConnection, Row};
use tracing::info;

use super::connection_profile::ConnectionProfile;
use super::db_kind::DbKind;
use super::{ConnectionId, CoreError, CoreResult};

/// Category of database object shown in the Objects tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObjectKind {
    Tables,
    Views,
    Procedures,
    Functions,
    Packages,
}

impl ObjectKind {
    /// Human-readable folder label in the Objects tree.
    ///
    /// # Examples
    ///
    /// ```
    /// use plsqlide::core::object_metadata::ObjectKind;
    ///
    /// assert_eq!(ObjectKind::Tables.label(), "Tables");
    /// assert_eq!(ObjectKind::Packages.label(), "Packages");
    /// ```
    pub fn label(self) -> &'static str {
        match self {
            Self::Tables => "Tables",
            Self::Views => "Views",
            Self::Procedures => "Procedures",
            Self::Functions => "Functions",
            Self::Packages => "Packages",
        }
    }

    /// Object kinds available for the given database dialect.
    ///
    /// Oracle includes Packages; PostgreSQL does not.
    ///
    /// # Examples
    ///
    /// ```
    /// use plsqlide::core::object_metadata::ObjectKind;
    /// use plsqlide::core::DbKind;
    ///
    /// assert!(ObjectKind::for_db(DbKind::Oracle).contains(&ObjectKind::Packages));
    /// assert!(!ObjectKind::for_db(DbKind::Postgres).contains(&ObjectKind::Packages));
    /// ```
    pub fn for_db(db_kind: DbKind) -> &'static [ObjectKind] {
        match db_kind {
            DbKind::Oracle => &[
                Self::Tables,
                Self::Views,
                Self::Procedures,
                Self::Functions,
                Self::Packages,
            ],
            DbKind::Postgres => &[
                Self::Tables,
                Self::Views,
                Self::Procedures,
                Self::Functions,
            ],
        }
    }
}

/// Root nodes of the Objects tree (Oracle users / PostgreSQL schemas).
#[derive(Debug, Clone)]
pub struct ObjectOwnerList {
    pub connection_id: ConnectionId,
    /// Login user used for the “current user only” filter and display.
    pub current_user: String,
    pub owners: Vec<String>,
}

/// Names inside one owner/schema folder of a given [`ObjectKind`].
#[derive(Debug, Clone)]
pub struct ObjectNameList {
    pub connection_id: ConnectionId,
    pub owner: String,
    pub kind: ObjectKind,
    pub names: Vec<String>,
}

/// Lists schema/owner roots for the Objects browser.
///
/// When `current_user_only` is true:
/// - PostgreSQL: schemas owned by `current_user`
/// - Oracle: a single root equal to the profile username (uppercased)
///
/// # Examples
///
/// ```no_run
/// use plsqlide::core::connection_profile::ConnectionProfile;
/// use plsqlide::core::object_metadata::list_owners;
/// use plsqlide::core::DbKind;
///
/// # async fn demo() {
/// let profile = ConnectionProfile::new("Local", DbKind::Postgres);
/// let roots = list_owners(&profile, true).await.expect("owners");
/// assert_eq!(roots.connection_id, profile.id);
/// # }
/// ```
pub async fn list_owners(
    profile: &ConnectionProfile,
    current_user_only: bool,
) -> CoreResult<ObjectOwnerList> {
    match profile.db_kind {
        DbKind::Postgres => list_owners_postgres(profile, current_user_only).await,
        DbKind::Oracle => list_owners_oracle(profile, current_user_only).await,
    }
}

/// Lists object names for `owner` / schema and `kind`.
///
/// # Examples
///
/// ```no_run
/// use plsqlide::core::connection_profile::ConnectionProfile;
/// use plsqlide::core::object_metadata::{list_objects, ObjectKind};
/// use plsqlide::core::DbKind;
///
/// # async fn demo() {
/// let profile = ConnectionProfile::new("Local", DbKind::Postgres);
/// let tables = list_objects(&profile, "public", ObjectKind::Tables)
///     .await
///     .expect("tables");
/// assert_eq!(tables.kind, ObjectKind::Tables);
/// # }
/// ```
pub async fn list_objects(
    profile: &ConnectionProfile,
    owner: &str,
    kind: ObjectKind,
) -> CoreResult<ObjectNameList> {
    match profile.db_kind {
        DbKind::Postgres => list_objects_postgres(profile, owner, kind).await,
        DbKind::Oracle => list_objects_oracle(profile, owner, kind).await,
    }
}

async fn list_owners_postgres(
    profile: &ConnectionProfile,
    current_user_only: bool,
) -> CoreResult<ObjectOwnerList> {
    let mut connection = connect_postgres(profile).await?;
    info!("postgres object metadata: listing schemas");

    let sql = if current_user_only {
        r#"
        SELECT n.nspname
        FROM pg_namespace n
        JOIN pg_roles r ON n.nspowner = r.oid
        WHERE r.rolname = current_user
        ORDER BY 1
        "#
    } else {
        r#"
        SELECT nspname
        FROM pg_namespace
        WHERE nspname NOT LIKE 'pg_%'
          AND nspname <> 'information_schema'
        ORDER BY 1
        "#
    };

    let rows = sqlx::query(sql)
        .fetch_all(&mut connection)
        .await
        .map_err(|err| CoreError::Query(sanitize_error(&err.to_string())))?;

    let current_user: String = sqlx::query_scalar("SELECT current_user")
        .fetch_one(&mut connection)
        .await
        .map_err(|err| CoreError::Query(sanitize_error(&err.to_string())))?;

    let owners = rows
        .iter()
        .filter_map(|row| row.try_get::<String, _>(0).ok())
        .collect();

    connection.close().await.ok();

    Ok(ObjectOwnerList {
        connection_id: profile.id,
        current_user,
        owners,
    })
}

async fn list_objects_postgres(
    profile: &ConnectionProfile,
    owner: &str,
    kind: ObjectKind,
) -> CoreResult<ObjectNameList> {
    let mut connection = connect_postgres(profile).await?;
    info!(%owner, ?kind, "postgres object metadata: listing objects");

    let names = match kind {
        ObjectKind::Tables => {
            fetch_strings(
                &mut connection,
                r#"
                SELECT table_name
                FROM information_schema.tables
                WHERE table_schema = $1
                  AND table_type = 'BASE TABLE'
                ORDER BY 1
                "#,
                owner,
            )
            .await?
        }
        ObjectKind::Views => {
            fetch_strings(
                &mut connection,
                r#"
                SELECT table_name
                FROM information_schema.views
                WHERE table_schema = $1
                ORDER BY 1
                "#,
                owner,
            )
            .await?
        }
        ObjectKind::Procedures => {
            fetch_strings(
                &mut connection,
                r#"
                SELECT routine_name
                FROM information_schema.routines
                WHERE routine_schema = $1
                  AND routine_type = 'PROCEDURE'
                ORDER BY 1
                "#,
                owner,
            )
            .await?
        }
        ObjectKind::Functions => {
            fetch_strings(
                &mut connection,
                r#"
                SELECT routine_name
                FROM information_schema.routines
                WHERE routine_schema = $1
                  AND routine_type = 'FUNCTION'
                ORDER BY 1
                "#,
                owner,
            )
            .await?
        }
        ObjectKind::Packages => {
            connection.close().await.ok();
            return Ok(ObjectNameList {
                connection_id: profile.id,
                owner: owner.into(),
                kind,
                names: Vec::new(),
            });
        }
    };

    connection.close().await.ok();

    Ok(ObjectNameList {
        connection_id: profile.id,
        owner: owner.into(),
        kind,
        names,
    })
}

async fn connect_postgres(profile: &ConnectionProfile) -> CoreResult<PgConnection> {
    let options = profile
        .postgres_url()
        .parse::<PgConnectOptions>()
        .map_err(|err| CoreError::Connection(sanitize_error(&err.to_string())))?
        .application_name("plsqlide-objects")
        .statement_cache_capacity(0);

    options
        .connect()
        .await
        .map_err(|err| CoreError::Connection(sanitize_error(&err.to_string())))
}

async fn fetch_strings(
    connection: &mut PgConnection,
    sql: &str,
    owner: &str,
) -> CoreResult<Vec<String>> {
    let rows = sqlx::query(sql)
        .bind(owner)
        .fetch_all(&mut *connection)
        .await
        .map_err(|err| CoreError::Query(sanitize_error(&err.to_string())))?;

    Ok(rows
        .iter()
        .filter_map(|row| row.try_get::<String, _>(0).ok())
        .collect())
}

async fn list_owners_oracle(
    profile: &ConnectionProfile,
    current_user_only: bool,
) -> CoreResult<ObjectOwnerList> {
    #[cfg(feature = "oracle")]
    {
        let profile = profile.clone();
        tokio::task::spawn_blocking(move || list_owners_oracle_blocking(&profile, current_user_only))
            .await
            .map_err(|err| CoreError::Other(err.to_string()))?
    }

    #[cfg(not(feature = "oracle"))]
    {
        let _ = (profile, current_user_only);
        Err(CoreError::UnsupportedDb("oracle (build with --features oracle)"))
    }
}

async fn list_objects_oracle(
    profile: &ConnectionProfile,
    owner: &str,
    kind: ObjectKind,
) -> CoreResult<ObjectNameList> {
    #[cfg(feature = "oracle")]
    {
        let profile = profile.clone();
        let owner = owner.to_string();
        tokio::task::spawn_blocking(move || list_objects_oracle_blocking(&profile, &owner, kind))
            .await
            .map_err(|err| CoreError::Other(err.to_string()))?
    }

    #[cfg(not(feature = "oracle"))]
    {
        let _ = (profile, owner, kind);
        Err(CoreError::UnsupportedDb("oracle (build with --features oracle)"))
    }
}

#[cfg(feature = "oracle")]
fn list_owners_oracle_blocking(
    profile: &ConnectionProfile,
    current_user_only: bool,
) -> CoreResult<ObjectOwnerList> {
    use oracle::Connector;

    super::oracle_connection_prefs::prepare_oracle_client()?;

    let username = oracle_owner_name(&profile.username);
    if current_user_only {
        return Ok(ObjectOwnerList {
            connection_id: profile.id,
            current_user: username.clone(),
            owners: vec![username],
        });
    }

    let mut connector = Connector::new(
        &profile.username,
        &profile.password,
        &profile.oracle_connect_descriptor(),
    );
    connector.stmt_cache_size(0);
    let connection = connector
        .connect()
        .map_err(|err| CoreError::Connection(err.to_string()))?;

    info!("oracle object metadata: listing owners");

    let rows = connection
        .query("SELECT DISTINCT owner FROM all_objects ORDER BY 1", &[])
        .map_err(|err| CoreError::Query(err.to_string()))?;

    let mut owners = Vec::new();
    for row in rows {
        let row = row.map_err(|err| CoreError::Query(err.to_string()))?;
        let owner: String = row
            .get(0)
            .map_err(|err| CoreError::Query(err.to_string()))?;
        owners.push(owner);
    }

    Ok(ObjectOwnerList {
        connection_id: profile.id,
        current_user: username,
        owners,
    })
}

#[cfg(feature = "oracle")]
fn list_objects_oracle_blocking(
    profile: &ConnectionProfile,
    owner: &str,
    kind: ObjectKind,
) -> CoreResult<ObjectNameList> {
    use oracle::Connector;

    super::oracle_connection_prefs::prepare_oracle_client()?;

    let mut connector = Connector::new(
        &profile.username,
        &profile.password,
        &profile.oracle_connect_descriptor(),
    );
    connector.stmt_cache_size(0);
    let connection = connector
        .connect()
        .map_err(|err| CoreError::Connection(err.to_string()))?;

    info!(%owner, ?kind, "oracle object metadata: listing objects");

    let (sql, bind_owner) = match kind {
        ObjectKind::Tables => (
            "SELECT table_name FROM all_tables WHERE owner = :1 ORDER BY 1",
            owner,
        ),
        ObjectKind::Views => (
            "SELECT view_name FROM all_views WHERE owner = :1 ORDER BY 1",
            owner,
        ),
        ObjectKind::Procedures => (
            "SELECT object_name FROM all_procedures WHERE owner = :1 AND object_type = 'PROCEDURE' ORDER BY 1",
            owner,
        ),
        ObjectKind::Functions => (
            "SELECT object_name FROM all_procedures WHERE owner = :1 AND object_type = 'FUNCTION' ORDER BY 1",
            owner,
        ),
        ObjectKind::Packages => (
            "SELECT object_name FROM all_objects WHERE owner = :1 AND object_type = 'PACKAGE' ORDER BY 1",
            owner,
        ),
    };

    let rows = connection
        .query(sql, &[&bind_owner])
        .map_err(|err| CoreError::Query(err.to_string()))?;

    let mut names = Vec::new();
    for row in rows {
        let row = row.map_err(|err| CoreError::Query(err.to_string()))?;
        let name: String = row
            .get(0)
            .map_err(|err| CoreError::Query(err.to_string()))?;
        names.push(name);
    }

    Ok(ObjectNameList {
        connection_id: profile.id,
        owner: owner.into(),
        kind,
        names,
    })
}

#[cfg(feature = "oracle")]
fn oracle_owner_name(username: &str) -> String {
    username.trim().to_uppercase()
}

fn sanitize_error(message: &str) -> String {
    redact_connection_urls(message).chars().take(400).collect()
}

fn redact_connection_urls(message: &str) -> String {
    let mut result = message.to_string();
    let mut search_from = 0;

    while let Some(rel_start) = result[search_from..].find("postgres://") {
        let start = search_from + rel_start;
        let after_scheme = start + "postgres://".len();
        if let Some(rel_at) = result[after_scheme..].find('@') {
            let at = after_scheme + rel_at;
            result.replace_range(after_scheme..at, "***");
            search_from = after_scheme + 3;
        } else {
            break;
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::{sanitize_error, ObjectKind};
    use crate::core::DbKind;

    #[test]
    fn for_db_hides_packages_on_postgres() {
        assert!(!ObjectKind::for_db(DbKind::Postgres).contains(&ObjectKind::Packages));
        assert!(ObjectKind::for_db(DbKind::Oracle).contains(&ObjectKind::Packages));
    }

    #[test]
    fn sanitize_hides_credentials_in_url() {
        let message = sanitize_error("error talking to postgres://alice:secret@localhost/db");
        assert!(message.contains("postgres://***@localhost/db"));
        assert!(!message.contains("alice:secret"));
    }
}
