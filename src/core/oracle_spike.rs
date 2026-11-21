//! Oracle Phase 0 spike enabled with `--features oracle`.
//!
//! Requires a local Oracle Client (Instant Client or full client) at runtime.

use std::time::Duration;

use oracle::{Connection, Connector};
use serde::Serialize;
use tracing::info;

use super::{ConnectAs, CoreError, CoreResult};

/// Request used by the Oracle spike.
#[derive(Debug, Clone)]
pub struct OracleSpikeRequest {
    pub connect_descriptor: String,
    pub username: String,
    pub password: String,
    /// Administrative role (Normal / SYSDBA / SYSOPER).
    pub connect_as: ConnectAs,
    pub sql: Option<String>,
    pub timeout_ms: Option<u64>,
    pub cancel: bool,
    /// Server-side row cap for SELECT fetches; `None` fetches everything.
    pub max_rows: Option<usize>,
}

/// Result of a successful Oracle spike.
#[derive(Debug, Clone, Serialize)]
pub struct OracleSpikeResult {
    pub connected: bool,
    pub cancelled: bool,
    pub elapsed_ms: u64,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub message: String,
}

impl OracleSpikeResult {
    /// Formats the spike result for the UI output panel.
    pub fn format_for_ui(&self) -> String {
        let mut out = format!(
            "connected={}\ncancelled={}\nelapsed_ms={}\nmessage={}",
            self.connected, self.cancelled, self.elapsed_ms, self.message
        );
        if !self.columns.is_empty() {
            out.push_str("\n\n");
            out.push_str(&self.columns.join(" | "));
            for row in &self.rows {
                out.push('\n');
                out.push_str(&row.join(" | "));
            }
        }
        out
    }
}

/// Opens an Oracle connection on a blocking worker and runs a query or cancel demo.
///
/// # Examples
///
/// ```no_run
/// use plsqlide::core::oracle_spike::{run_oracle_spike, OracleSpikeRequest};
/// use plsqlide::core::ConnectAs;
///
/// # async fn demo() {
/// let result = run_oracle_spike(OracleSpikeRequest {
///     connect_descriptor: "localhost:1521/XEPDB1".into(),
///     username: "scott".into(),
///     password: "tiger".into(),
///     connect_as: ConnectAs::Normal,
///     sql: Some("select 1 as value from dual".into()),
///     timeout_ms: Some(10_000),
///     cancel: false,
///     max_rows: Some(10),
/// })
/// .await
/// .expect("spike");
/// assert!(result.connected);
/// # }
/// ```
pub async fn run_oracle_spike(request: OracleSpikeRequest) -> CoreResult<OracleSpikeResult> {
    let timeout_ms = request.timeout_ms.unwrap_or(10_000);
    tokio::task::spawn_blocking(move || run_oracle_spike_blocking(request, timeout_ms))
        .await
        .map_err(|err| CoreError::Other(err.to_string()))?
}

/// Maps a UI role onto the OCI privilege; Normal needs none.
/// (crate-visible so pooled sessions in `oracle_pool` reuse the same mapping.)
pub(crate) fn oracle_privilege_for(connect_as: ConnectAs) -> Option<oracle::Privilege> {
    match connect_as {
        ConnectAs::Normal => None,
        ConnectAs::Sysdba => Some(oracle::Privilege::Sysdba),
        ConnectAs::Sysoper => Some(oracle::Privilege::Sysoper),
    }
}

fn run_oracle_spike_blocking(
    request: OracleSpikeRequest,
    timeout_ms: u64,
) -> CoreResult<OracleSpikeResult> {
    super::oracle_connection_prefs::prepare_oracle_client()?;

    let started = std::time::Instant::now();
    let sql = request
        .sql
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "select 1 as value from dual".to_string());

    let mut connector = Connector::new(
        &request.username,
        &request.password,
        &request.connect_descriptor,
    );
    connector.stmt_cache_size(0);
    if let Some(privilege) = oracle_privilege_for(request.connect_as) {
        connector.privilege(privilege);
    }

    let connection = connector
        .connect()
        .map_err(|err| CoreError::Connection(err.to_string()))?;

    connection
        .set_call_timeout(Some(Duration::from_millis(timeout_ms)))
        .map_err(|err| CoreError::Other(err.to_string()))?;

    super::oracle_compiler_prefs::apply_to_connection(&connection)?;

    info!("oracle spike connected");

    if request.cancel {
        return cancel_long_query(&connection, started);
    }

    // SELECT/WITH go through the query path; everything else (PL/SQL
    // anonymous blocks, DML) must use execute — query() rejects non-selects.
    // ponytail: leading `--` comments only; /* */ blocks in tests are rare.
    let head = sql
        .trim_start()
        .lines()
        .skip_while(|l| l.trim_start().starts_with("--"))
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_uppercase();
    let is_query = head.starts_with("SELECT") || head.starts_with("WITH");
    let (columns, values, message) = if !is_query {
        // Bind wrapper ran (Test window) — make sure the OUT-capture table
        // exists in this session before the block inserts into it, then
        // read the returned values back as a two-column result.
        let capture = sql.starts_with("DECLARE\nPLSQLIDE_P");
        // ponytail: OUT capture needs PLSQLIDE_BIND_OUT; on med2dev DDL is gated
        // (DEV_HELLO + SET_MED_TASK), so probe and strip capture lines if absent.
        // Upgrade: drop stripping once the table is created for real.
        let mut out_ready = false;
        if capture {
            out_ready = fetch_query_rows(
                &connection,
                "select count(*) from user_tables where table_name = 'PLSQLIDE_BIND_OUT'",
                &request,
            )
            .ok()
            .and_then(|(_, rows, _)| {
                rows.first()
                    .and_then(|r| r.first().cloned())
                    .map(|n| n != "0")
            })
            .unwrap_or(false);
            let sql = if out_ready {
                sql
            } else {
                sql.lines()
                    .filter(|l| !l.contains("PLSQLIDE_BIND_OUT"))
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            connection
                .execute(&sql, &[])
                .map_err(|err| CoreError::Query(err.to_string()))?;
            info!("oracle statement executed");
            if out_ready {
                let (cols, vals, _) = fetch_query_rows(
                    &connection,
                    "select NAME, VAL from plsqlide_bind_out",
                    &request,
                )?;
                (cols, vals, "statement executed".to_string())
            } else {
                (
                    Vec::new(),
                    Vec::new(),
                    "statement executed (OUT capture skipped: PLSQLIDE_BIND_OUT table missing)"
                        .to_string(),
                )
            }
        } else {
            connection
                .execute(&sql, &[])
                .map_err(|err| CoreError::Query(err.to_string()))?;
            info!("oracle statement executed");
            (Vec::new(), Vec::new(), "statement executed".to_string())
        }
    } else {
        fetch_query_rows(&connection, &sql, &request)?
    };

    Ok(OracleSpikeResult {
        connected: true,
        cancelled: false,
        elapsed_ms: started.elapsed().as_millis() as u64,
        columns,
        rows: values,
        message,
    })
}

/// Run a SELECT and collect up to the row cap, dropping the paging helper
/// column added by the app's ROWNUM wrapper.
fn fetch_query_rows(
    connection: &Connection,
    sql: &str,
    request: &OracleSpikeRequest,
) -> CoreResult<(Vec<String>, Vec<Vec<String>>, String)> {
    let rows = connection
        .query(sql, &[])
        .map_err(|err| CoreError::Query(err.to_string()))?;

    info!("oracle query executing");

    let mut columns = Vec::new();
    let mut values = Vec::new();

    for (index, row) in rows.enumerate() {
        let row = row.map_err(|err| CoreError::Query(err.to_string()))?;
        if index == 0 {
            columns = row
                .column_info()
                .iter()
                .map(|info| info.name().to_string())
                .collect();
        }
        // Fetch cap reached — stop, do not drain the cursor.
        if request.max_rows.is_some_and(|cap| values.len() >= cap) {
            break;
        }
        let mut line = Vec::with_capacity(columns.len());
        for column_index in 0..columns.len() {
            let cell = row
                .get::<usize, Option<String>>(column_index)
                .unwrap_or(None)
                .unwrap_or_else(|| "NULL".to_string());
            line.push(cell);
        }
        values.push(line);
    }

    // Drop the paging helper column added by the app's ROWNUM wrapper.
    if columns
        .last()
        .map(|name| name.to_ascii_uppercase() == "_PLSQLIDE_RN")
        .unwrap_or(false)
    {
        columns.pop();
        for line in &mut values {
            line.pop();
        }
    }

    info!(rows = values.len(), "oracle query finished");
    Ok((columns, values, "query completed".to_string()))
}

fn cancel_long_query(
    connection: &Connection,
    started: std::time::Instant,
) -> CoreResult<OracleSpikeResult> {
    // Short call timeout acts as the cancel demo without a second connection handle.
    connection
        .set_call_timeout(Some(Duration::from_millis(500)))
        .map_err(|err| CoreError::Other(err.to_string()))?;

    let query_result =
        connection.query_as::<i32>("select 1 from dual connect by level <= 100000000", &[]);

    let cancelled = match query_result {
        Err(err) => {
            let text = err.to_string().to_lowercase();
            text.contains("cancel")
                || text.contains("break")
                || text.contains("timeout")
                || text.contains("ora-01013")
                || text.contains("dpi-1067")
        }
        Ok(_) => false,
    };

    Ok(OracleSpikeResult {
        connected: true,
        cancelled,
        elapsed_ms: started.elapsed().as_millis() as u64,
        columns: Vec::new(),
        rows: Vec::new(),
        message: if cancelled {
            "long query cancelled successfully".into()
        } else {
            "cancel path finished without clear cancellation signal".into()
        },
    })
}
