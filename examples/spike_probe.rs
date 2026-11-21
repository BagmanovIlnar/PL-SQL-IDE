//! Reproduces the worksheet Execute path (wrapped SELECT) with stage timing.
//!
//! Usage: cargo run --features oracle --example spike_probe

#[cfg(feature = "oracle")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    use plsqlide::core::connection_profile::ConnectionProfile;
    use plsqlide::core::connections_store;
    use plsqlide::core::oracle_spike::{run_oracle_spike, OracleSpikeRequest};
    use plsqlide::core::ConnectAs;

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new("info"))
        .init();
    let store = connections_store::load();
    let profile = store
        .connections
        .iter()
        .find(|profile| profile.db_kind == plsqlide::core::DbKind::Oracle)
        .ok_or("no oracle profile stored")?;
    println!("profile: {}", profile.name);
    println!("connect_as: {profile:?}");

    let probes: Vec<(&str, String, Option<usize>)> = vec![
        (
            "first page (offset 0)",
            "SELECT * FROM (SELECT * FROM D_AGENTS) WHERE ROWNUM <= 10".to_string(),
            Some(10),
        ),
        (
            "app path: max_rows None (Execute button)",
            "SELECT * FROM (SELECT * FROM D_AGENTS) WHERE ROWNUM <= 10".to_string(),
            None,
        ),
        (
            "next page (offset > 0, quoted alias)",
            "SELECT * FROM (SELECT t.*, ROWNUM AS \"_PLSQLIDE_RN\" FROM (SELECT * FROM D_AGENTS) t WHERE ROWNUM <= 30) WHERE \"_PLSQLIDE_RN\" > 20".to_string(),
            Some(10),
        ),
        (
            "fetch all (offset > 0, no limit, small data)",
            "SELECT * FROM (SELECT t.*, ROWNUM AS \"_PLSQLIDE_RN\" FROM (SELECT * FROM D_AGENTS WHERE ROWNUM <= 50) t WHERE ROWNUM <= 1000000000) WHERE \"_PLSQLIDE_RN\" > 10".to_string(),
            None,
        ),
    ];
    // User's failing case: anonymous block with a bind, full Execute path
    // (auto-filled params + capture wrap), expecting the OUT-skip fallback.
    let src = "DECLARE\n      v_x NUMBER;\nBEGIN\n      :dcdcd := 'dcdcd';\nEND;";
    let mut params = plsqlide::ui::parameters::parse_binds(src)
        .into_iter()
        .map(|n| {
            plsqlide::ui::parameters::Parameter::new(
                n,
                plsqlide::ui::parameters::ParamDataType::Oracle(
                    plsqlide::ui::parameters::OracleType::Varchar2,
                ),
            )
        })
        .collect::<Vec<_>>();
    params[0].value = "vfvf".to_string();
    let (wrapped, _off) = plsqlide::ui::parameters::bind_wrap(src, &params, true);
    println!("\n=== bind block (capture wrapped) ===\n{wrapped}");
    let result = run_oracle_spike(OracleSpikeRequest {
        connect_descriptor: profile.oracle_connect_descriptor(),
        username: profile.username.clone(),
        password: profile.password.clone(),
        connect_as: ConnectAs::Normal,
        sql: Some(wrapped),
        timeout_ms: Some(10_000),
        cancel: false,
        max_rows: None,
    })
    .await?;
    println!(
        "ok={} rows={} msg={:?}",
        result.connected,
        result.rows.len(),
        result.message
    );
    for row in &result.rows {
        println!("out: {row:?}");
    }

    for (label, sql, max_rows) in probes {
        println!("\n=== {label} ===");
        println!("running: {sql} (max_rows: {max_rows:?})");
        let started = std::time::Instant::now();
        let result = run_oracle_spike(OracleSpikeRequest {
            connect_descriptor: profile.oracle_connect_descriptor(),
            username: profile.username.clone(),
            password: profile.password.clone(),
            connect_as: ConnectAs::Normal,
            sql: Some(sql),
            timeout_ms: Some(10_000),
            cancel: false,
            max_rows,
        })
        .await?;
        println!(
            "elapsed: {} ms, ok: {}, columns: {:?}, rows: {}, message: {}",
            started.elapsed().as_millis(),
            result.connected,
            result.columns,
            result.rows.len(),
            result.message
        );
        if let Some(first) = result.rows.first() {
            println!("first row: {first:?}");
        }
    }
    Ok(())
}

#[cfg(not(feature = "oracle"))]
fn main() {
    eprintln!("build with --features oracle");
}
