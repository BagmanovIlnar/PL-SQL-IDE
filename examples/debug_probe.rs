//! Live check of the Probe (DBMS_DEBUG) flow exactly as the Test window runs it.
//!
//! Usage: cargo run --features oracle --example debug_probe

#[cfg(feature = "oracle")]
fn main() {
    tokio::runtime::Runtime::new().unwrap().block_on(run_main());
}

#[cfg(feature = "oracle")]
async fn run_main() {
    use plsqlide::core::connections_store;
    use plsqlide::core::debug_session::{Breakpoint, DebugCommand};
    use plsqlide::core::oracle_debug::{run_debug_command, DebugEngineRequest};
    use plsqlide::core::oracle_debugger_prefs::{load, DebugSystem};

    let store = connections_store::load();
    let mut profile = store
        .connections
        .iter()
        .find(|p| p.db_kind == plsqlide::core::DbKind::Oracle)
        .cloned()
        .expect("no oracle profile stored");
    // Ensure a fresh unique connection id for the debug pair.
    profile.id = uuid::Uuid::new_v4();
    println!("profile: {}", profile.name);

    let source = "DECLARE
      v_x NUMBER;
BEGIN
      :TEST := 'cdcd';
      v_x := 1;
END;";
    let mut params = plsqlide::ui::parameters::parse_binds(source)
        .into_iter()
        .map(|n| plsqlide::ui::parameters::Parameter::new(
            n, plsqlide::ui::parameters::ParamDataType::Oracle(
                plsqlide::ui::parameters::OracleType::Varchar2)))
        .collect::<Vec<_>>();
    params[0].value = "внутри".to_string();
    let (source, offset) = plsqlide::ui::parameters::bind_wrap(source, &params, true);
    println!("bind offset: {offset}");
    println!("--- source ---\n{source}\n");

    let step = |command| {
        let req = DebugEngineRequest {
            profile: profile.clone(),
            command,
            prefs: load(),
            breakpoints: vec![Breakpoint { line: 7 }],
            cursor_line: None,
            source: source.to_string(),
            connection_key: profile.name.clone(),
        };
        let started = std::time::Instant::now();
        println!(">> running {:?}", command);
        let res = run_debug_command(req);
        println!("[{:?} {:?}]", command, started.elapsed());
        match res {
            Ok(ev) => {
                println!(
                    "active={} line={:?} status={}",
                    ev.active, ev.current_line, ev.status
                );
                for l in &ev.log {
                    println!("  log: {l}");
                }
                for v in &ev.variables {
                    println!("  var: {v}");
                }
            }
            Err(err) => println!("ERROR: {err}"),
        }
        println!();
    };

    step(DebugCommand::Start(DebugSystem::DbmsDebug));
    step(DebugCommand::StepOver);
    step(DebugCommand::StepOver);
    step(DebugCommand::Run);
    step(DebugCommand::Run);
    step(DebugCommand::Stop);
}

#[cfg(feature = "oracle")]
use plsqlide::core::oracle_debugger_prefs::{self as _prefs};

#[cfg(not(feature = "oracle"))]
fn main() {
    eprintln!("build with --features oracle");
}
