//! JDWP dial test: does Oracle actually CONNECT_TCP back to us?

#[cfg(feature = "oracle")]
fn main() {
    tokio::runtime::Runtime::new().unwrap().block_on(run());
}

#[cfg(feature = "oracle")]
async fn run() {
    use plsqlide::core::connections_store;
    use plsqlide::core::oracle_connection_prefs::prepare_oracle_client;
    use plsqlide::core::oracle_pool;
    use std::sync::{mpsc, Arc, Mutex};

    let store = connections_store::load();
    let profile = store
        .connections
        .iter()
        .find(|p| p.db_kind == plsqlide::core::DbKind::Oracle)
        .expect("no oracle profile stored");
    prepare_oracle_client().unwrap();

    let listener = std::net::TcpListener::bind("0.0.0.0:4001").unwrap();
    println!("bound 0.0.0.0:4001");

    let mut prof = profile.clone();
    prof.id = uuid::Uuid::new_v4();
    let conn = Arc::new(Mutex::new(oracle_pool::connect_profile(&prof).unwrap()));
    let conn2 = conn.clone();
    let sql = r#"
DECLARE
  c UTL_TCP.connection;
BEGIN
  c := UTL_TCP.OPEN_CONNECTION('192.168.1.83', 4001, timeout => 5);
  UTL_TCP.CLOSE_CONNECTION(c);
  DBMS_SESSION.SET_IDENTIFIER('UTL:ok');
EXCEPTION WHEN OTHERS THEN
  DBMS_SESSION.SET_IDENTIFIER('UTL:' || SQLERRM);
END;"#
        .to_string();
    let conn_thread = std::thread::spawn(move || {
        let conn = conn2.lock().unwrap();
        let res = conn.execute(&sql, &[]);
        let ident: String = conn
            .query_row_as(
                "SELECT SYS_CONTEXT('USERENV','CLIENT_IDENTIFIER') FROM DUAL",
                &[],
            )
            .unwrap_or_default();
        println!("UTL_TCP result: {ident}");
        res.map(|_| ()).map_err(|e| e.to_string())
    });

    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(listener.accept());
    });
    match rx.recv_timeout(std::time::Duration::from_secs(15)) {
        Ok(Ok((stream, peer))) => {
            println!("ACCEPTED from {peer}");
            drop(stream);
        }
        Ok(Err(err)) => println!("accept err: {err}"),
        Err(_) => println!("ACCEPT TIMEOUT (10s) — oracle did not dial"),
    }

    // DBMS_PIPE round-trip on a direct connection.
    {
        plsqlide::core::oracle_connection_prefs::prepare_oracle_client().unwrap();
        let mut p2 = profile.clone();
        p2.id = uuid::Uuid::new_v4();
        let conn = plsqlide::core::oracle_pool::connect_profile(&p2).unwrap();
        let res = conn.execute(
            "DECLARE \
               st INTEGER; msg VARCHAR2(100); \
             BEGIN \
               SYS.DBMS_PIPE.PURGE('PLSQLIDE_TEST'); \
               SYS.DBMS_PIPE.PACK_MESSAGE('hello'); \
               st := SYS.DBMS_PIPE.SEND_MESSAGE('PLSQLIDE_TEST', 2); \
               st := SYS.DBMS_PIPE.RECEIVE_MESSAGE('PLSQLIDE_TEST', 1); \
               SYS.DBMS_PIPE.UNPACK_MESSAGE(msg); \
               SYS.DBMS_SESSION.SET_IDENTIFIER('PIPE:' || st || ':' || msg); \
             END;",
            &[],
        );
        let ident: String = conn
            .query_row_as(
                "SELECT SYS_CONTEXT('USERENV','CLIENT_IDENTIFIER') FROM DUAL",
                &[],
            )
            .unwrap_or_default();
        println!("dbms_pipe round-trip: {:?} ident={ident}", res.err());
    }

    let ident = plsqlide::core::oracle_spike::run_oracle_spike(
        plsqlide::core::oracle_spike::OracleSpikeRequest {
            connect_descriptor: profile.oracle_connect_descriptor(),
            username: profile.username.clone(),
            password: profile.password.clone(),
            connect_as: plsqlide::core::ConnectAs::Normal,
            sql: Some("SELECT SYS_CONTEXT('USERENV','CLIENT_IDENTIFIER') FROM DUAL".into()),
            timeout_ms: Some(8_000),
            cancel: false,
            max_rows: None,
            dbms_output: None,
        },
    )
    .await;
    println!(
        "ident: {:?}",
        ident.map(|r| r
            .rows
            .iter()
            .map(|row| row.first().cloned().unwrap_or_default())
            .collect::<Vec<_>>())
    );

    // Ask the DB for the failure reason of a refused/unreachable dial.
    let privs = plsqlide::core::oracle_spike::run_oracle_spike(
        plsqlide::core::oracle_spike::OracleSpikeRequest {
            connect_descriptor: profile.oracle_connect_descriptor(),
            username: profile.username.clone(),
            password: profile.password.clone(),
            connect_as: plsqlide::core::ConnectAs::Normal,
            sql: Some(
                "SELECT PRIVILEGE FROM SESSION_PRIVS WHERE PRIVILEGE LIKE '%DEBUG%' ORDER BY 1"
                    .into(),
            ),
            timeout_ms: Some(8_000),
            cancel: false,
            max_rows: None,
            dbms_output: None,
        },
    )
    .await;
    println!(
        "debug privs: {:?}",
        privs.map(|r| r
            .rows
            .iter()
            .map(|row| row.first().cloned().unwrap_or_default())
            .collect::<Vec<_>>())
    );
    let ver = plsqlide::core::oracle_spike::run_oracle_spike(
        plsqlide::core::oracle_spike::OracleSpikeRequest {
            connect_descriptor: profile.oracle_connect_descriptor(),
            username: profile.username.clone(),
            password: profile.password.clone(),
            connect_as: plsqlide::core::ConnectAs::Normal,
            sql: Some("SELECT banner FROM v$version WHERE banner LIKE 'Oracle%'".into()),
            timeout_ms: Some(8_000),
            cancel: false,
            max_rows: None,
            dbms_output: None,
        },
    )
    .await;
    println!(
        "version: {:?}",
        ver.map(|r| r
            .rows
            .iter()
            .map(|row| row.first().cloned().unwrap_or_default())
            .collect::<Vec<_>>())
    );
    let probe = plsqlide::core::oracle_spike::run_oracle_spike(
        plsqlide::core::oracle_spike::OracleSpikeRequest {
            connect_descriptor: profile.oracle_connect_descriptor(),
            username: profile.username.clone(),
            password: profile.password.clone(),
            connect_as: plsqlide::core::ConnectAs::Normal,
            sql: Some("BEGIN DBMS_DEBUG_JDWP.CONNECT_TCP('192.168.1.83', 4001); END;".into()),
            timeout_ms: Some(8_000),
            cancel: false,
            max_rows: None,
            dbms_output: None,
        },
    )
    .await;
    println!(
        "spike CONNECT_TCP: {:?}",
        probe.as_ref().map(|_| "ok").map_err(|e| e.to_string())
    );
    let _ = conn_thread.join();
}

#[cfg(not(feature = "oracle"))]
fn main() {
    eprintln!("build with --features oracle");
}
