//! Oracle Phase 0 spike enabled with `--features oracle`.
//!
//! Requires a local Oracle Client (Instant Client or full client) at runtime.

use std::time::Duration;

use oracle::{Connection, Connector};
use serde::Serialize;
use tracing::info;

use super::{CoreError, CoreResult};

/// Request used by the Oracle spike.
#[derive(Debug, Clone)]
pub struct OracleSpikeRequest {
    pub connect_descriptor: String,
    pub username: String,
    pub password: String,
    pub sql: Option<String>,
    pub timeout_ms: Option<u64>,
    pub cancel: bool,
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
///
/// # async fn demo() {
/// let result = run_oracle_spike(OracleSpikeRequest {
///     connect_descriptor: "localhost:1521/XEPDB1".into(),
///     username: "scott".into(),
///     password: "tiger".into(),
///     sql: Some("select 1 as value from dual".into()),
///     timeout_ms: Some(10_000),
///     cancel: false,
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

fn run_oracle_spike_blocking(
    request: OracleSpikeRequest,
    timeout_ms: u64,
) -> CoreResult<OracleSpikeResult> {
    super::oracle_connection_prefs::prepare_oracle_client()?;

    let started = std::time::Instant::now();
    let sql = request
        .sql
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "select 1 as value from dual".to_string());

    let mut connector = Connector::new(
        &request.username,
        &request.password,
        &request.connect_descriptor,
    );
    connector.stmt_cache_size(0);

    let connection = connector
        .connect()
        .map_err(|err| CoreError::Connection(err.to_string()))?;

    connection
        .set_call_timeout(Some(Duration::from_millis(timeout_ms)))
        .map_err(|err| CoreError::Other(err.to_string()))?;

    info!("oracle spike connected");

    if request.cancel {
        return cancel_long_query(&connection, started);
    }

    let rows = connection
        .query(&sql, &[])
        .map_err(|err| CoreError::Query(err.to_string()))?;

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
        if values.len() >= 100 {
            continue;
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

    Ok(OracleSpikeResult {
        connected: true,
        cancelled: false,
        elapsed_ms: started.elapsed().as_millis() as u64,
        columns,
        rows: values,
        message: "query completed".into(),
    })
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
