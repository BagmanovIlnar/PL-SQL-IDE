//! PostgreSQL Phase 0 spike: connect, query, cancel and timeout.

use std::time::Duration;

use serde::Serialize;
use sqlx::postgres::PgConnectOptions;
use sqlx::{Column, ConnectOptions, Connection, Executor, PgConnection, Row, ValueRef};
use tokio::time::timeout;
use tracing::info;

use super::{CoreError, CoreResult};

/// Request used by the PostgreSQL spike.
#[derive(Debug, Clone)]
pub struct PostgresSpikeRequest {
    pub connection_url: String,
    pub sql: Option<String>,
    pub timeout_ms: Option<u64>,
    pub cancel: bool,
}

/// Result of a successful PostgreSQL spike.
#[derive(Debug, Clone, Serialize)]
pub struct PostgresSpikeResult {
    pub connected: bool,
    pub cancelled: bool,
    pub elapsed_ms: u64,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub message: String,
}

impl PostgresSpikeResult {
    /// Formats the spike result for the UI output panel.
    ///
    /// # Examples
    ///
    /// ```
    /// use plsqlide::core::postgres_spike::PostgresSpikeResult;
    ///
    /// let text = PostgresSpikeResult {
    ///     connected: true,
    ///     cancelled: false,
    ///     elapsed_ms: 12,
    ///     columns: vec!["v".into()],
    ///     rows: vec![vec!["1".into()]],
    ///     message: "ok".into(),
    /// }
    /// .format_for_ui();
    /// assert!(text.contains("connected=true"));
    /// ```
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

/// Opens a PostgreSQL connection, runs a query or cancel demo, then closes.
///
/// # Examples
///
/// ```no_run
/// use plsqlide::core::postgres_spike::{run_postgres_spike, PostgresSpikeRequest};
///
/// # async fn demo() {
/// let result = run_postgres_spike(PostgresSpikeRequest {
///     connection_url: "postgres://postgres:postgres@127.0.0.1:5432/postgres".into(),
///     sql: Some("select version()".into()),
///     timeout_ms: Some(5_000),
///     cancel: false,
/// })
/// .await
/// .expect("spike");
/// assert!(result.connected);
/// # }
/// ```
pub async fn run_postgres_spike(request: PostgresSpikeRequest) -> CoreResult<PostgresSpikeResult> {
    let timeout_ms = request.timeout_ms.unwrap_or(10_000);
    let started = std::time::Instant::now();
    let sql = request
        .sql
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "select 1::int as value".to_string());

    let options = request
        .connection_url
        .parse::<PgConnectOptions>()
        .map_err(|err| CoreError::Connection(sanitize_error(&err.to_string())))?
        .application_name("plsqlide-spike")
        .statement_cache_capacity(0);

    let mut connection = timeout(Duration::from_millis(timeout_ms), options.connect())
        .await
        .map_err(|_| CoreError::Timeout(timeout_ms))?
        .map_err(|err| CoreError::Connection(sanitize_error(&err.to_string())))?;

    info!("postgres spike connected");

    if request.cancel {
        let cancelled =
            cancel_long_query(&mut connection, &request.connection_url, timeout_ms).await?;
        connection.close().await.ok();
        return Ok(PostgresSpikeResult {
            connected: true,
            cancelled,
            elapsed_ms: started.elapsed().as_millis() as u64,
            columns: Vec::new(),
            rows: Vec::new(),
            message: if cancelled {
                "long query cancelled successfully".into()
            } else {
                "cancel path finished without cancellation signal".into()
            },
        });
    }

    let query_future = connection.fetch_all(sqlx::query(&sql));
    let rows = timeout(Duration::from_millis(timeout_ms), query_future)
        .await
        .map_err(|_| CoreError::Timeout(timeout_ms))?
        .map_err(|err| CoreError::Query(sanitize_error(&err.to_string())))?;

    let columns = if let Some(first) = rows.first() {
        first
            .columns()
            .iter()
            .map(|column| column.name().to_string())
            .collect()
    } else {
        Vec::new()
    };

    let mut values = Vec::with_capacity(rows.len().min(100));
    for row in rows.iter().take(100) {
        let mut line = Vec::with_capacity(columns.len());
        for index in 0..columns.len() {
            let cell = match row.try_get_raw(index) {
                Ok(raw) if raw.is_null() => "NULL".to_string(),
                Ok(_) => row
                    .try_get::<String, _>(index)
                    .ok()
                    .or_else(|| {
                        row.try_get::<i64, _>(index)
                            .ok()
                            .map(|number| number.to_string())
                    })
                    .unwrap_or_else(|| "<unprintable>".to_string()),
                Err(_) => "<unprintable>".to_string(),
            };
            line.push(cell);
        }
        values.push(line);
    }

    connection.close().await.ok();

    Ok(PostgresSpikeResult {
        connected: true,
        cancelled: false,
        elapsed_ms: started.elapsed().as_millis() as u64,
        columns,
        rows: values,
        message: "query completed".into(),
    })
}

async fn cancel_long_query(
    connection: &mut PgConnection,
    connection_url: &str,
    timeout_ms: u64,
) -> CoreResult<bool> {
    let pid: i32 = sqlx::query_scalar("select pg_backend_pid()")
        .fetch_one(&mut *connection)
        .await
        .map_err(|err| CoreError::Query(sanitize_error(&err.to_string())))?;

    let options = connection_url
        .parse::<PgConnectOptions>()
        .map_err(|err| CoreError::Connection(sanitize_error(&err.to_string())))?
        .application_name("plsqlide-spike-cancel")
        .statement_cache_capacity(0);

    let mut control = options
        .connect()
        .await
        .map_err(|err| CoreError::Connection(sanitize_error(&err.to_string())))?;

    let cancel_task = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        let accepted = sqlx::query_scalar::<_, bool>("select pg_cancel_backend($1)")
            .bind(pid)
            .fetch_one(&mut control)
            .await;
        let _ = control.close().await;
        accepted
    });

    let sleep_result = timeout(
        Duration::from_millis(timeout_ms),
        connection.execute(sqlx::query("select pg_sleep(30)")),
    )
    .await;

    let cancel_accepted = match cancel_task.await {
        Ok(Ok(accepted)) => accepted,
        Ok(Err(err)) => {
            return Err(CoreError::Query(sanitize_error(&err.to_string())));
        }
        Err(err) => {
            return Err(CoreError::Other(err.to_string()));
        }
    };

    match sleep_result {
        Ok(Ok(_)) => Ok(cancel_accepted),
        Ok(Err(err)) => {
            let text = err.to_string().to_lowercase();
            if text.contains("cancel") || text.contains("57014") || cancel_accepted {
                Ok(true)
            } else {
                Err(CoreError::Query(sanitize_error(&err.to_string())))
            }
        }
        Err(_) => Err(CoreError::Timeout(timeout_ms)),
    }
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
    use super::sanitize_error;

    #[test]
    fn sanitize_hides_credentials_in_url() {
        let message = sanitize_error("error talking to postgres://alice:secret@localhost/db");
        assert!(message.contains("postgres://***@localhost/db"));
        assert!(!message.contains("alice:secret"));
    }
}
