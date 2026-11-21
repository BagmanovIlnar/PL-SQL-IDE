//! Verifies RAISE NOTICE capture for the Command window (Postgres).
//! The probe runs a DO block that only raises notices — it changes no data.
//!
//! Usage: cargo run --example pg_notice_probe

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    use plsqlide::core::connections_store;
    use plsqlide::core::postgres_spike::{
        notice_capture_layer, run_postgres_spike, PostgresSpikeRequest,
    };
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    tracing_subscriber::registry()
        .with(notice_capture_layer())
        .with(tracing_subscriber::fmt::layer().with_target(false))
        .init();
    let store = connections_store::load();
    let profile = store
        .connections
        .iter()
        .find(|profile| profile.db_kind == plsqlide::core::DbKind::Postgres)
        .expect("no postgres profile");
    let result = run_postgres_spike(PostgresSpikeRequest {
        connection_url: profile.postgres_url(),
        sql: Some(
            "DO $$ BEGIN RAISE NOTICE 'привёт из plpgsql'; RAISE NOTICE 'second line'; END $$;"
                .into(),
        ),
        timeout_ms: Some(10_000),
        cancel: false,
        max_rows: None,
    })
    .await?;
    println!("notices = {:?}", result.notices);
    println!("---\n{}", result.format_for_ui());
    assert!(
        result.notices.contains(&"привёт из plpgsql".to_string()),
        "notice not captured"
    );
    Ok(())
}
