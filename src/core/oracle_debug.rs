//! Live Oracle PL/SQL debugger: `DBMS_DEBUG` (Probe) and `DBMS_DEBUG_JDWP`.
//!
//! - **DBMS_DEBUG**: target INITIALIZE/DEBUG_ON + controller ATTACH/CONTINUE.
//! - **DBMS_DEBUG_JDWP**: IDE listens (JDWP), target CONNECT_TCP, then step/bp
//!   over the wire protocol.

use serde::{Deserialize, Serialize};

use super::connection_profile::ConnectionProfile;
use super::debug_session::{Breakpoint, DebugCommand};
use super::oracle_debugger_prefs::{DebugSystem, OracleDebuggerPrefs};
use super::{CoreError, CoreResult};

#[cfg(feature = "oracle")]
use super::ConnectionId;

#[cfg(feature = "oracle")]
mod breakflags {
    /// DBMS_DEBUG values verified live on Oracle 11.2 (`DBMS_DEBUG` package source).
    pub const BREAK_NEXT_LINE: i32 = 32;
    pub const BREAK_ANY_CALL: i32 = 12;
    pub const BREAK_ANY_RETURN: i32 = 512;
    pub const BREAK_EXCEPTION: i32 = 2;
    pub const ABORT_EXECUTION: i32 = 8192;
}

/// Seconds the controller waits for a debug event before CONTINUE reports a
/// timeout. DBMS_DEBUG.DEFAULT_TIMEOUT is 3600s — a Run with no breakpoint
/// would block the UI for an hour.
const PROBE_WAIT_SECS: i32 = 10;

/// Result returned to the UI after a debug command hits the database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugEngineEvent {
    pub active: bool,
    pub system: DebugSystem,
    pub status: String,
    pub current_line: Option<usize>,
    pub message: String,
    pub log: Vec<String>,
    pub variables: Vec<String>,
}

/// Request from the IDE to the debug engine.
#[derive(Debug, Clone)]
pub struct DebugEngineRequest {
    pub profile: ConnectionProfile,
    pub command: DebugCommand,
    pub prefs: OracleDebuggerPrefs,
    pub breakpoints: Vec<Breakpoint>,
    pub cursor_line: Option<usize>,
    pub source: String,
    pub connection_key: String,
}

#[cfg(feature = "oracle")]
struct LiveDebugState {
    active: bool,
    system: DebugSystem,
    #[allow(dead_code)]
    debug_id: String,
    /// Instrumented (local) debugging: DBMS_PIPE name prefix.
    pipe: Option<String>,
}

#[cfg(feature = "oracle")]
use std::collections::HashMap;
#[cfg(feature = "oracle")]
use std::sync::{LazyLock, Mutex};
#[cfg(feature = "oracle")]
use std::time::Duration;

#[cfg(feature = "oracle")]
static SESSION_STATE: LazyLock<Mutex<HashMap<ConnectionId, LiveDebugState>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
#[cfg(feature = "oracle")]
static SESSION_SOURCE: LazyLock<Mutex<HashMap<ConnectionId, String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
/// Bind-parameter OUT values fetched by the finished debug block, delivered
/// to the next finished event and written back into the parameters panel.
static BINDOUTS: LazyLock<Mutex<HashMap<ConnectionId, Vec<(String, String)>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
/// Pipe name of the previous debug session per connection — Start tells the
/// still-dying previous block to STOP and purges its pipes.
static LAST_PIPE: LazyLock<Mutex<HashMap<ConnectionId, String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
#[cfg(feature = "oracle")]
static JDWP_SESSIONS: LazyLock<Mutex<HashMap<ConnectionId, super::jdwp::JdwpSession>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
#[cfg(feature = "oracle")]
static LAST_THREAD: LazyLock<Mutex<HashMap<ConnectionId, u64>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Run a debug command against Oracle (blocking; call from `spawn_blocking`).
pub fn run_debug_command(request: DebugEngineRequest) -> CoreResult<DebugEngineEvent> {
    #[cfg(feature = "oracle")]
    {
        run_debug_command_oracle(request)
    }
    #[cfg(not(feature = "oracle"))]
    {
        let _ = request;
        Err(CoreError::UnsupportedDb(
            "oracle (build with --features oracle)".into(),
        ))
    }
}

#[cfg(feature = "oracle")]
fn run_debug_command_oracle(request: DebugEngineRequest) -> CoreResult<DebugEngineEvent> {
    use super::oracle_pool;

    super::oracle_connection_prefs::prepare_oracle_client()?;

    // Anonymous blocks run under the local instrumented debugger: the source
    // is rewritten with DBMS_PIPE stop-points, so no server debugger is
    // needed (Probe cannot step anonymous blocks on 11.2.0.4 anyway).
    if is_anonymous_source(&request.source)
        && matches!(
            request.command,
            DebugCommand::Start(_)
                | DebugCommand::Run
                | DebugCommand::StepInto
                | DebugCommand::StepOver
                | DebugCommand::StepOut
                | DebugCommand::RunToCursor
                | DebugCommand::RunToNextException
                | DebugCommand::Stop
        )
    {
        oracle_pool::debug_access::ensure_debug_pair(&request.profile)?;
        return instrumented_command(request);
    }

    oracle_pool::debug_access::ensure_debug_pair(&request.profile)?;

    // After Start, route by active system (JDWP vs Probe).
    let active_system = match request.command {
        DebugCommand::Start(system) => system,
        _ => current_system(request.profile.id, &request.prefs),
    };

    match request.command {
        DebugCommand::Start(DebugSystem::DbmsDebugJdwp) => start_jdwp_session(&request),
        DebugCommand::Start(DebugSystem::DbmsDebug) => start_probe_session(&request),
        DebugCommand::Stop => {
            if matches!(active_system, DebugSystem::DbmsDebugJdwp) {
                stop_jdwp_session(&request)
            } else {
                stop_probe_session(&request)
            }
        }
        DebugCommand::Run
        | DebugCommand::StepInto
        | DebugCommand::StepOver
        | DebugCommand::StepOut
        | DebugCommand::RunToCursor
        | DebugCommand::RunToNextException
            if matches!(active_system, DebugSystem::DbmsDebugJdwp) =>
        {
            jdwp_command(&request)
        }
        DebugCommand::Run => continue_probe(&request, 0, "Run / continue"),
        DebugCommand::StepInto => continue_probe(&request, breakflags::BREAK_ANY_CALL, "Step Into"),
        DebugCommand::StepOver => {
            continue_probe(&request, breakflags::BREAK_NEXT_LINE, "Step Over")
        }
        DebugCommand::StepOut => continue_probe(&request, breakflags::BREAK_ANY_RETURN, "Step Out"),
        DebugCommand::RunToCursor => run_to_cursor_probe(&request),
        DebugCommand::RunToNextException => continue_probe(
            &request,
            breakflags::BREAK_EXCEPTION,
            "Run to next exception",
        ),
        DebugCommand::ExecuteSqlInDebugSession => execute_sql_in_target(&request),
    }
}

// ─── JDWP path ───────────────────────────────────────────────────────────────

#[cfg(feature = "oracle")]
fn start_jdwp_session(request: &DebugEngineRequest) -> CoreResult<DebugEngineEvent> {
    use super::jdwp::JdwpSession;
    use super::oracle_pool::debug_access;

    let id = request.profile.id;
    let mut log = Vec::new();
    let port = OracleDebuggerPrefs::clamp_jdwp_port(request.prefs.default_jdwp_port);
    let bind = request.prefs.default_jdwp_network_adapter.clone();
    let oracle_host = JdwpSession::connect_host_for_oracle(&bind, &request.profile.host);

    let listener = JdwpSession::listen(&bind, port)?;
    log.push(format!("JDWP listening on {}:{}", bind, port));

    // Oracle CONNECT_TCP blocks until the IDE accepts — run it in parallel.
    let profile = request.profile.clone();
    let connect_sql = format!("BEGIN DBMS_DEBUG_JDWP.CONNECT_TCP('{oracle_host}', {port}); END;");
    let join = std::thread::spawn(move || {
        debug_access::with_target_mut(profile.id, |target| {
            target
                .execute(&connect_sql, &[])
                .map_err(|err| CoreError::Query(format!("DBMS_DEBUG_JDWP.CONNECT_TCP: {err}")))?;
            Ok(())
        })
    });

    let mut session = JdwpSession::accept(listener, Duration::from_secs(60))?;
    log.append(&mut session.log);

    match join.join() {
        Ok(Ok(())) => log.push(format!("CONNECT_TCP to {oracle_host}:{port} ok")),
        Ok(Err(err)) => {
            return Err(CoreError::Query(format!(
                "CONNECT_TCP failed after accept: {err}"
            )));
        }
        Err(_) => return Err(CoreError::Other("CONNECT_TCP thread panicked".into())),
    }

    session.set_breakpoints(&request.breakpoints)?;
    log.append(&mut session.log);

    if let Err(err) = session.request_exception_break() {
        log.push(format!("exception break: {err}"));
    }

    if !request.source.trim().is_empty() {
        SESSION_SOURCE
            .lock()
            .map_err(|_| CoreError::Other("session source lock".into()))?
            .insert(id, wrap_plsql(&request.source));
        log.push("source staged — Run/Step executes under JDWP".into());
    }

    SESSION_STATE
        .lock()
        .map_err(|_| CoreError::Other("session state lock".into()))?
        .insert(
            id,
            LiveDebugState {
                active: true,
                system: DebugSystem::DbmsDebugJdwp,
                debug_id: format!("jdwp:{oracle_host}:{port}"),
                pipe: None,
            },
        );
    JDWP_SESSIONS
        .lock()
        .map_err(|_| CoreError::Other("jdwp sessions lock".into()))?
        .insert(id, session);

    Ok(DebugEngineEvent {
        active: true,
        system: DebugSystem::DbmsDebugJdwp,
        status: "Debugger started (DBMS_DEBUG_JDWP)".into(),
        current_line: None,
        message: format!(
            "JDWP debugger listening; Oracle connected via {oracle_host}:{port} ({})",
            request.connection_key
        ),
        log,
        variables: Vec::new(),
    })
}

#[cfg(feature = "oracle")]
fn stop_jdwp_session(request: &DebugEngineRequest) -> CoreResult<DebugEngineEvent> {
    use super::oracle_pool::debug_access;

    let id = request.profile.id;
    let mut log = Vec::new();
    if let Ok(mut guard) = JDWP_SESSIONS.lock() {
        if let Some(mut session) = guard.remove(&id) {
            let _ = session.dispose();
            log.append(&mut session.log);
            log.push("JDWP session disposed".into());
        }
    }
    let _ = debug_access::with_target_mut(id, |target| {
        let _ = target.execute("BEGIN DBMS_DEBUG_JDWP.DISCONNECT; END;", &[]);
        Ok(())
    });
    log.push("DBMS_DEBUG_JDWP.DISCONNECT".into());
    if let Ok(mut guard) = SESSION_STATE.lock() {
        guard.remove(&id);
    }
    if let Ok(mut guard) = SESSION_SOURCE.lock() {
        guard.remove(&id);
    }
    if let Ok(mut guard) = LAST_THREAD.lock() {
        guard.remove(&id);
    }
    Ok(DebugEngineEvent {
        active: false,
        system: DebugSystem::DbmsDebugJdwp,
        status: "Debugger stopped".into(),
        current_line: None,
        message: "JDWP debug session stopped".into(),
        log,
        variables: Vec::new(),
    })
}

#[cfg(feature = "oracle")]
fn jdwp_command(request: &DebugEngineRequest) -> CoreResult<DebugEngineEvent> {
    use super::jdwp::StepDepth;
    use super::oracle_pool::debug_access;

    let id = request.profile.id;
    ensure_active(id)?;
    let mut log = Vec::new();
    let label = match request.command {
        DebugCommand::Run => "Run / continue",
        DebugCommand::StepInto => "Step Into",
        DebugCommand::StepOver => "Step Over",
        DebugCommand::StepOut => "Step Out",
        DebugCommand::RunToCursor => "Run to cursor",
        DebugCommand::RunToNextException => "Run to next exception",
        _ => "JDWP",
    };

    // Optional: add cursor breakpoint.
    if matches!(request.command, DebugCommand::RunToCursor) {
        let line = request.cursor_line.unwrap_or(1);
        if let Ok(mut guard) = JDWP_SESSIONS.lock() {
            if let Some(session) = guard.get_mut(&id) {
                let _ = session.set_breakpoints(&[Breakpoint { line }]);
                log.append(&mut session.log);
            }
        }
    }

    // Step / exception request before resume.
    let thread = LAST_THREAD.lock().ok().and_then(|g| g.get(&id).copied());
    if let Ok(mut guard) = JDWP_SESSIONS.lock() {
        if let Some(session) = guard.get_mut(&id) {
            match request.command {
                DebugCommand::StepInto => {
                    let _ = session.request_step(thread, StepDepth::Into);
                }
                DebugCommand::StepOver => {
                    let _ = session.request_step(thread, StepDepth::Over);
                }
                DebugCommand::StepOut => {
                    let _ = session.request_step(thread, StepDepth::Out);
                }
                DebugCommand::RunToNextException => {
                    let _ = session.request_exception_break();
                }
                _ => {}
            }
            log.append(&mut session.log);
            session.resume_vm()?;
            log.append(&mut session.log);
        } else {
            return Err(CoreError::Other(
                "JDWP session missing — Start debugger first".into(),
            ));
        }
    }

    let maybe_source = SESSION_SOURCE.lock().ok().and_then(|mut g| g.remove(&id));

    let pause = if let Some(source) = maybe_source {
        // Run PL/SQL on target while waiting for JDWP pause event.
        let profile_id = id;
        let join = std::thread::spawn(move || {
            debug_access::with_target_mut(profile_id, |target| {
                target
                    .execute(&source, &[])
                    .map_err(|err| CoreError::Query(format!("target PL/SQL: {err}")))?;
                Ok(())
            })
        });
        let pause = {
            let mut guard = JDWP_SESSIONS
                .lock()
                .map_err(|_| CoreError::Other("jdwp sessions lock".into()))?;
            let session = guard
                .get_mut(&id)
                .ok_or_else(|| CoreError::Other("JDWP session missing".into()))?;
            session.wait_for_pause(Duration::from_secs(120))?
        };
        let _ = join.join();
        pause
    } else {
        let mut guard = JDWP_SESSIONS
            .lock()
            .map_err(|_| CoreError::Other("jdwp sessions lock".into()))?;
        let session = guard
            .get_mut(&id)
            .ok_or_else(|| CoreError::Other("JDWP session missing".into()))?;
        session.wait_for_pause(Duration::from_secs(120))?
    };

    if let Some(tid) = pause.thread_id {
        if let Ok(mut guard) = LAST_THREAD.lock() {
            guard.insert(id, tid);
        }
    }

    let vars = if request.prefs.show_variable_values_popup {
        pause.variables.clone()
    } else {
        Vec::new()
    };
    log.push(format!("pause {} line={:?}", pause.kind, pause.line));

    Ok(DebugEngineEvent {
        active: true,
        system: DebugSystem::DbmsDebugJdwp,
        status: format!("{label}: {}", pause.kind),
        current_line: pause.line,
        message: format!("{label}: paused ({})", pause.kind),
        log,
        variables: vars,
    })
}

// ─── Probe (DBMS_DEBUG) path ─────────────────────────────────────────────────

#[cfg(feature = "oracle")]
fn start_probe_session(request: &DebugEngineRequest) -> CoreResult<DebugEngineEvent> {
    use super::oracle_pool::debug_access;

    let mut log = Vec::new();
    let id = request.profile.id;

    let debug_id: String = debug_access::with_target_mut(id, |target| {
        let row = target
            .query_row_as::<String>("SELECT DBMS_DEBUG.INITIALIZE FROM DUAL", &[])
            .map_err(|err| CoreError::Query(format!("DBMS_DEBUG.INITIALIZE: {err}")))?;
        // Do not let the controller wait forever when no event arrives.
        target
            .execute(
                "BEGIN DBMS_DEBUG.SET_TIMEOUT_BEHAVIOUR(DBMS_DEBUG.CONTINUE_ON_TIMEOUT); END;",
                &[],
            )
            .map_err(|err| CoreError::Query(format!("SET_TIMEOUT_BEHAVIOUR: {err}")))?;
        // SET_TIMEOUT is a FUNCTION, not a procedure.
        target
            .execute(
                &format!(
                    "DECLARE ret BINARY_INTEGER; BEGIN ret := DBMS_DEBUG.SET_TIMEOUT({PROBE_WAIT_SECS}); END;"
                ),
                &[],
            )
            .map_err(|err| CoreError::Query(format!("target SET_TIMEOUT: {err}")))?;
        target
            .execute("BEGIN DBMS_DEBUG.DEBUG_ON(TRUE); END;", &[])
            .map_err(|err| CoreError::Query(format!("DBMS_DEBUG.DEBUG_ON: {err}")))?;
        Ok(row)
    })?;
    log.push(format!("target INITIALIZE → debug_id={debug_id}"));

    let attach_id = debug_id.clone();
    debug_access::with_ctrl_mut(id, |ctrl| {
        let sql = format!("BEGIN DBMS_DEBUG.ATTACH_SESSION('{attach_id}'); END;");
        ctrl.execute(&sql, &[])
            .map_err(|err| CoreError::Query(format!("DBMS_DEBUG.ATTACH_SESSION: {err}")))?;
        // SET_TIMEOUT is a FUNCTION, not a procedure.
        ctrl.execute(
            &format!(
                "DECLARE ret BINARY_INTEGER; BEGIN ret := DBMS_DEBUG.SET_TIMEOUT({PROBE_WAIT_SECS}); END;"
            ),
            &[],
        )
        .map_err(|err| CoreError::Query(format!("DBMS_DEBUG.SET_TIMEOUT: {err}")))?;
        Ok(())
    })?;
    log.push(format!(
        "controller ATTACH_SESSION ok (wait {PROBE_WAIT_SECS}s)"
    ));

    apply_probe_filters(id, &request.prefs, &mut log)?;

    for bp in &request.breakpoints {
        match set_anonymous_breakpoint(id, bp.line) {
            Ok(bp_no) => log.push(format!("breakpoint line {} → #{bp_no}", bp.line)),
            Err(err) => log.push(format!("breakpoint line {}: {err}", bp.line)),
        }
    }

    let compile_hint = request
        .prefs
        .compile_debug_clause(&request.connection_key, None);
    if !compile_hint.is_empty() {
        log.push(format!("compile policy:{compile_hint}"));
    }

    if !request.source.trim().is_empty() {
        SESSION_SOURCE
            .lock()
            .map_err(|_| CoreError::Other("session source lock".into()))?
            .insert(id, wrap_plsql(&request.source));
        log.push("source staged — press Run / Step to execute under Probe".into());
    }

    SESSION_STATE
        .lock()
        .map_err(|_| CoreError::Other("session state lock".into()))?
        .insert(
            id,
            LiveDebugState {
                active: true,
                system: DebugSystem::DbmsDebug,
                debug_id,
                pipe: None,
            },
        );

    Ok(DebugEngineEvent {
        active: true,
        system: DebugSystem::DbmsDebug,
        status: "Debugger started (DBMS_DEBUG)".into(),
        current_line: None,
        message: format!("Attached Probe on {}", request.connection_key),
        log,
        variables: Vec::new(),
    })
}

#[cfg(feature = "oracle")]
fn apply_probe_filters(
    id: ConnectionId,
    prefs: &OracleDebuggerPrefs,
    log: &mut Vec<String>,
) -> CoreResult<()> {
    use super::oracle_pool::debug_access;

    // Document filters applied client-side on step; Probe has limited filter APIs.
    if prefs.step_over_sys_objects {
        log.push("filter: step over SYS objects".into());
    }
    if prefs.step_over_package_spec_init {
        log.push("filter: step over package spec init".into());
    }
    if prefs.step_over_package_body_init {
        log.push("filter: step over package body init".into());
    }
    let _ = debug_access::with_ctrl_mut(id, |_ctrl| Ok(()));
    Ok(())
}

#[cfg(feature = "oracle")]
fn stop_probe_session(request: &DebugEngineRequest) -> CoreResult<DebugEngineEvent> {
    use super::oracle_pool::debug_access;

    let id = request.profile.id;
    let mut log = Vec::new();

    let _ = debug_access::with_ctrl_mut(id, |ctrl| {
        let _ = ctrl.execute(
            &format!(
                "BEGIN DBMS_DEBUG.CONTINUE(NULL, {}, NULL); END;",
                breakflags::ABORT_EXECUTION
            ),
            &[],
        );
        let _ = ctrl.execute("BEGIN DBMS_DEBUG.DETACH_SESSION; END;", &[]);
        Ok(())
    });
    log.push("DETACH_SESSION".into());

    let _ = debug_access::with_target_mut(id, |target| {
        let _ = target.execute("BEGIN DBMS_DEBUG.DEBUG_OFF; END;", &[]);
        Ok(())
    });
    log.push("DEBUG_OFF".into());

    if let Ok(mut guard) = SESSION_STATE.lock() {
        guard.remove(&id);
    }
    if let Ok(mut guard) = SESSION_SOURCE.lock() {
        guard.remove(&id);
    }

    Ok(DebugEngineEvent {
        active: false,
        system: DebugSystem::DbmsDebug,
        status: "Debugger stopped".into(),
        current_line: None,
        message: "Probe debug session stopped".into(),
        log,
        variables: Vec::new(),
    })
}

#[cfg(feature = "oracle")]
fn continue_probe(
    request: &DebugEngineRequest,
    flags: i32,
    label: &str,
) -> CoreResult<DebugEngineEvent> {
    use super::oracle_pool::debug_access;

    let id = request.profile.id;
    ensure_active(id)?;
    let mut log = vec![format!("{label}")];

    let maybe_source = SESSION_SOURCE.lock().ok().and_then(|mut g| g.remove(&id));

    let mut pause = if let Some(source) = maybe_source {
        // Run the block on the target while polling the controller for the
        // first suspension (Probe reports the interpreter-start pause).
        let (target, gen) = detach_target_with_retry(id)?;
        let join = std::thread::spawn(move || {
            let mut target = target;
            let result = target
                .execute(&source, &[])
                .map_err(|err| CoreError::Query(format!("target PL/SQL: {err}")));
            let _ = target.execute("BEGIN DBMS_DEBUG.DEBUG_OFF; END;", &[]);
            crate::core::oracle_pool::debug_access::reattach_target(id, target, gen);
            result
        });

        let first = poll_for_pause(id);
        log.extend(first.log.clone());
        if let Some(line) = first.line {
            log.push(format!("paused at line {line}"));
        } else {
            log.push("program finished without pausing".into());
            let _ = join.join();
        }
        // While paused the target thread stays alive inside `execute`; it
        // reattaches the connection once the block finishes.
        PauseInfo {
            line: first.line,
            status: match first.line {
                Some(line) => format!("paused at line {line}"),
                None => "program finished".into(),
            },
            log: Vec::new(),
            paused: first.line.is_some(),
        }
    } else {
        // Resume: ask the target to run with `flags`, then poll for the next
        // suspension. On servers where CONTINUE cannot deliver its response
        // (anonymous-block program info is broken on some 11.2 patch levels)
        // the poll keeps reporting the current line instead.
        let before = poll_for_pause(id);
        let resume = debug_access::with_ctrl_mut(id, |ctrl| {
            ctrl.execute(
                &format!(
                    "DECLARE info DBMS_DEBUG.RUNTIME_INFO; r BINARY_INTEGER; \
                     BEGIN r := DBMS_DEBUG.CONTINUE(info, {flags}, 0); \
                     DBMS_SESSION.SET_IDENTIFIER('cont=' || r); END;"
                ),
                &[],
            )
            .map_err(|err| CoreError::Query(format!("DBMS_DEBUG.CONTINUE: {err}")))
        });
        match resume {
            Ok(_) => {
                let ack = debug_access::with_ctrl_mut(id, |ctrl| {
                    ctrl.query_row_as::<String>(
                        "SELECT SYS_CONTEXT('USERENV','CLIENT_IDENTIFIER') FROM DUAL",
                        &[],
                    )
                    .map_err(|err| CoreError::Query(format!("client identifier: {err}")))
                });
                match ack {
                    Ok(ident) => log.push(format!("CONTINUE({flags}) ack={ident}")),
                    Err(err) => log.push(format!("CONTINUE({flags}): {err}")),
                }
            }
            Err(err) => log.push(format!("CONTINUE({flags}): {err}")),
        }

        let next = poll_for_pause(id);
        log.extend(next.log.clone());
        if next.line == before.line && before.line.is_some() {
            log.push(
                "the server refused to resume the program (CONTINUE returned                  no result) — this Oracle build restricts Probe stepping"
                    .into(),
            );
        }
        PauseInfo {
            line: next.line,
            status: match next.line {
                Some(line) => format!("paused at line {line}"),
                None => "running (no pause reported)".into(),
            },
            log: Vec::new(),
            paused: next.line.is_some(),
        }
    };

    if pause.paused && !request.breakpoints.is_empty() {
        for bp in &request.breakpoints {
            match set_anonymous_breakpoint(id, bp.line) {
                Ok(bp_no) => log.push(format!("breakpoint line {} → #{bp_no}", bp.line)),
                Err(err) => log.push(format!("breakpoint line {}: {err}", bp.line)),
            }
        }
    }

    let line = pause.line;
    Ok(DebugEngineEvent {
        active: true,
        system: DebugSystem::DbmsDebug,
        status: pause.status.clone(),
        current_line: line,
        message: format!("{label}: {}", pause.status),
        log,
        variables: if pause.paused {
            read_probe_variables(id, &request.prefs)?
        } else {
            Vec::new()
        },
    })
}

/// Result of one poll round.
#[cfg(feature = "oracle")]
struct PollResult {
    line: Option<usize>,
    log: Vec<String>,
}

/// Poll GET_RUNTIME_INFO until the target suspends (line# >= 1) or the
/// wait budget is exhausted. GET_RUNTIME_INFO is a plain request/response,
/// which works even where CONTINUE's event delivery does not.
#[cfg(feature = "oracle")]
fn poll_for_pause(id: ConnectionId) -> PollResult {
    use super::oracle_pool::debug_access;

    const ROUNDS: usize = 24; // 24 * 500ms = 12s
    let mut log = Vec::new();
    for round in 0..ROUNDS {
        let ident = debug_access::with_ctrl_mut(id, |ctrl| {
            ctrl.execute(
                r#"
DECLARE
  info DBMS_DEBUG.RUNTIME_INFO;
  ret  BINARY_INTEGER;
BEGIN
  ret := DBMS_DEBUG.GET_RUNTIME_INFO(
    DBMS_DEBUG.INFO_GETLINEINFO + DBMS_DEBUG.INFO_GETSTACKDEPTH,
    info
  );
  DBMS_SESSION.SET_IDENTIFIER(
    'ret=' || ret || '|line=' || NVL(info.Line#, -1) ||
    '|depth=' || NVL(info.StackDepth, -1)
  );
END;"#,
                &[],
            )
            .map_err(|err| CoreError::Query(format!("GET_RUNTIME_INFO: {err}")))?;
            ctrl.query_row_as::<String>(
                "SELECT SYS_CONTEXT('USERENV','CLIENT_IDENTIFIER') FROM DUAL",
                &[],
            )
            .map_err(|err| CoreError::Query(format!("client identifier: {err}")))
        });
        match ident {
            Ok(state) => {
                if round == 0 {
                    log.push(format!("state {state}"));
                }
                let mut parts = state.split('|');
                let _ret = parts.next().unwrap_or("");
                let line = parts
                    .next()
                    .and_then(|part| part.strip_prefix("line="))
                    .and_then(|value| value.parse::<usize>().ok())
                    .filter(|value| *value > 0);
                let depth = parts
                    .next()
                    .and_then(|part| part.strip_prefix("depth="))
                    .and_then(|value| value.parse::<i32>().ok())
                    .unwrap_or(-1);
                if let Some(line) = line {
                    if depth >= 1 {
                        return PollResult {
                            line: Some(line),
                            log,
                        };
                    }
                }
            }
            Err(err) => {
                log.push(format!("poll: {err}"));
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    PollResult { line: None, log }
}

#[cfg(feature = "oracle")]
fn run_to_cursor_probe(request: &DebugEngineRequest) -> CoreResult<DebugEngineEvent> {
    let line = request.cursor_line.unwrap_or(1);
    let id = request.profile.id;
    ensure_active(id)?;
    match set_anonymous_breakpoint(id, line) {
        Ok(bp) => {
            let mut event = continue_probe(request, 0, &format!("Run to cursor line {line}"))?;
            event.log.push(format!("cursor breakpoint #{bp} at {line}"));
            Ok(event)
        }
        Err(err) => {
            let mut event = continue_probe(request, breakflags::BREAK_NEXT_LINE, "Run to cursor")?;
            event.log.push(format!("cursor bp failed: {err}"));
            Ok(event)
        }
    }
}

#[cfg(feature = "oracle")]
fn execute_sql_in_target(request: &DebugEngineRequest) -> CoreResult<DebugEngineEvent> {
    use super::oracle_pool::debug_access;

    let id = request.profile.id;
    ensure_active(id)?;
    let system = current_system(id, &request.prefs);
    let sql = request.source.trim();
    if sql.is_empty() {
        return Ok(DebugEngineEvent {
            active: true,
            system,
            status: "Execute SQL in debug session".into(),
            current_line: None,
            message: "Worksheet is empty".into(),
            log: Vec::new(),
            variables: Vec::new(),
        });
    }
    let wrapped = wrap_plsql(sql);
    debug_access::with_target_mut(id, |target| {
        target
            .execute(&wrapped, &[])
            .map_err(|err| CoreError::Query(format!("debug target SQL: {err}")))?;
        Ok(())
    })?;
    let vars = match system {
        DebugSystem::DbmsDebugJdwp => Vec::new(),
        DebugSystem::DbmsDebug => read_probe_variables(id, &request.prefs)?,
    };
    Ok(DebugEngineEvent {
        active: true,
        system,
        status: "Executed SQL in debug target session".into(),
        current_line: None,
        message: "SQL executed in target debug session".into(),
        log: vec![wrapped],
        variables: vars,
    })
}

#[cfg(feature = "oracle")]
struct PauseInfo {
    line: Option<usize>,
    status: String,
    log: Vec<String>,
    paused: bool,
}

#[cfg(feature = "oracle")]
fn probe_continue_on(ctrl: &mut oracle::Connection, flags: i32) -> CoreResult<PauseInfo> {
    ctrl.execute(
        &format!(
            r#"
DECLARE
  run_info DBMS_DEBUG.RUNTIME_INFO;
  retval   BINARY_INTEGER;
BEGIN
  retval := DBMS_DEBUG.CONTINUE(
    run_info,
    {flags},
    DBMS_DEBUG.INFO_GETLINEINFO + DBMS_DEBUG.INFO_GETBREAKPOINT
  );
  DBMS_SESSION.SET_IDENTIFIER(
    TO_CHAR(retval) || ':' ||
    TO_CHAR(NVL(run_info.line#, -1)) || ':' ||
    TO_CHAR(NVL(run_info.reason, -1))
  );
END;"#
        ),
        &[],
    )
    .map_err(|err| CoreError::Query(format!("DBMS_DEBUG.CONTINUE: {err}")))?;

    let ident: String = ctrl
        .query_row_as(
            "SELECT SYS_CONTEXT('USERENV','CLIENT_IDENTIFIER') FROM DUAL",
            &[],
        )
        .unwrap_or_default();
    let mut parts = ident.split(':');
    let retval = parts.next().and_then(|s| s.parse().ok()).unwrap_or(-1);
    let line = parts
        .next()
        .and_then(|s| s.parse::<i32>().ok())
        .filter(|n| *n > 0)
        .map(|n| n as usize);
    let reason = parts.next().and_then(|s| s.parse::<i32>().ok());
    let status = match reason {
        Some(r) => format!("paused reason={r} retval={retval}"),
        None => format!("continue retval={retval}"),
    };
    let paused = retval == 0;
    Ok(PauseInfo {
        line,
        status,
        log: vec![format!(
            "CONTINUE flags={flags} retval={retval} line={line:?} raw={ident}"
        )],
        paused,
    })
}

#[cfg(feature = "oracle")]
fn set_anonymous_breakpoint(id: ConnectionId, line: usize) -> CoreResult<i32> {
    use super::oracle_pool::debug_access;

    debug_access::with_ctrl_mut(id, |ctrl| {
        ctrl.execute(
            &format!(
                r#"
DECLARE
  prog DBMS_DEBUG.PROGRAM_INFO;
  bp   BINARY_INTEGER;
  ret  BINARY_INTEGER;
BEGIN
  prog.namespace := DBMS_DEBUG.NAMESPACE_CURSOR;
  prog.name      := NULL;
  prog.owner     := NULL;
  prog.dblink    := NULL;
  prog.line#     := {line};
  ret := DBMS_DEBUG.SET_BREAKPOINT(prog, {line}, bp);
  DBMS_SESSION.SET_IDENTIFIER(TO_CHAR(ret) || ':' || TO_CHAR(NVL(bp, -1)));
END;"#
            ),
            &[],
        )
        .map_err(|err| CoreError::Query(format!("SET_BREAKPOINT: {err}")))?;

        let ident: String = ctrl
            .query_row_as(
                "SELECT SYS_CONTEXT('USERENV','CLIENT_IDENTIFIER') FROM DUAL",
                &[],
            )
            .unwrap_or_default();
        let mut parts = ident.split(':');
        let ret: i32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(-1);
        let bp: i32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(-1);
        if ret != 0 && bp < 0 {
            return Err(CoreError::Query(format!(
                "SET_BREAKPOINT failed ret={ret} bp={bp}"
            )));
        }
        Ok(bp.max(0))
    })
}

#[cfg(feature = "oracle")]
fn read_probe_variables(id: ConnectionId, prefs: &OracleDebuggerPrefs) -> CoreResult<Vec<String>> {
    if !prefs.show_variable_values_popup {
        return Ok(Vec::new());
    }
    use super::oracle_pool::debug_access;

    let result = debug_access::with_ctrl_mut(id, |ctrl| {
        // Prefer GET_VALUE on a few common names; also try GET_RUNTIME_INFO when present.
        let try_info = ctrl.execute(
            r#"
DECLARE
  info DBMS_DEBUG.RUNTIME_INFO;
  ret  BINARY_INTEGER;
BEGIN
  ret := DBMS_DEBUG.GET_RUNTIME_INFO(
    DBMS_DEBUG.INFO_GETSTACKDEPTH + DBMS_DEBUG.INFO_GETLINEINFO,
    info
  );
  DBMS_SESSION.SET_IDENTIFIER(
    'depth=' || TO_CHAR(NVL(info.stackdepth, -1)) ||
    ';line=' || TO_CHAR(NVL(info.line#, -1))
  );
END;"#,
            &[],
        );
        if try_info.is_ok() {
            let ident: String = ctrl
                .query_row_as(
                    "SELECT SYS_CONTEXT('USERENV','CLIENT_IDENTIFIER') FROM DUAL",
                    &[],
                )
                .unwrap_or_default();
            return Ok(vec![ident, "Probe GET_RUNTIME_INFO".into()]);
        }
        Ok(vec![
            "Paused in Probe — expand variables after CONTINUE with INFO flags".into(),
        ])
    });
    result.or_else(|_| Ok(Vec::new()))
}

#[cfg(feature = "oracle")]
fn wrap_plsql(source: &str) -> String {
    let trimmed = source.trim().trim_end_matches(';').trim();
    // First non-comment token decides: `-- x\nDECLARE …` is a full block.
    let mut body = trimmed;
    loop {
        let rest = body.trim_start();
        if rest.starts_with("--") {
            body = rest.splitn(2, '\n').nth(1).unwrap_or("");
        } else if rest.starts_with("/*") {
            match rest.find("*/") {
                Some(end) => body = &rest[end + 2..],
                None => break,
            }
        } else {
            body = rest;
            break;
        }
    }
    let upper = body.to_ascii_uppercase();
    if upper.starts_with("BEGIN") || upper.starts_with("DECLARE") {
        if trimmed.to_ascii_uppercase().ends_with("END") {
            format!("{trimmed};")
        } else {
            trimmed.to_string()
        }
    } else {
        format!("BEGIN\n  {trimmed};\nEND;")
    }
}

#[cfg(feature = "oracle")]
fn ensure_active(id: ConnectionId) -> CoreResult<()> {
    let active = SESSION_STATE
        .lock()
        .map_err(|_| CoreError::Other("session state lock".into()))?
        .get(&id)
        .map(|s| s.active)
        .unwrap_or(false);
    if !active {
        return Err(CoreError::Other("Start the debugger first (F9)".into()));
    }
    Ok(())
}

#[cfg(feature = "oracle")]
fn current_system(id: ConnectionId, prefs: &OracleDebuggerPrefs) -> DebugSystem {
    SESSION_STATE
        .lock()
        .ok()
        .and_then(|g| g.get(&id).map(|s| s.system))
        .unwrap_or(prefs.default_debug_system)
}

// ─── Instrumented (local) debugging for anonymous blocks ─────────────────────

/// True when the source is an anonymous PL/SQL block (DECLARE/BEGIN first).
fn is_anonymous_source(source: &str) -> bool {
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("--") {
            continue;
        }
        let upper = trimmed.to_ascii_uppercase();
        return upper.starts_with("DECLARE") || upper.starts_with("BEGIN");
    }
    false
}

/// Execute one debug step of the instrumented protocol.
#[cfg(feature = "oracle")]
fn instrumented_command(request: DebugEngineRequest) -> CoreResult<DebugEngineEvent> {
    use super::oracle_pool::debug_access;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    let id = request.profile.id;
    let state = |pipe: Option<String>, active: bool| {
        SESSION_STATE
            .lock()
            .map_err(|_| CoreError::Other("session state lock".into()))?
            .insert(
                id,
                LiveDebugState {
                    active,
                    system: DebugSystem::DbmsDebug,
                    debug_id: "local".into(),
                    pipe,
                },
            );
        Ok(())
    };

    // Prepare (or reset) the instrumented session: fresh pipe name, state,
    // stale-pipe purge, stage the editor source. Returns the pipe id.
    let start_prep = |request: &DebugEngineRequest| -> CoreResult<String> {
        // ponytail: unique pipe per session — a still-dying block from the
        // previous run must not answer the new session (same name + live
        // thread meant pauses “at the wrong line”). Old pipe gets a STOP and
        // a purge; a generation-guarded pool would be the full fix.
        if let Ok(guard) = LAST_PIPE.lock() {
            if let Some(old) = guard.get(&id).cloned() {
                // PURGE FIRST, THEN send STOP — purging after the send
                // deletes the STOP before the target reads it, leaving the
                // block blocked on its receive loop forever (busy forever).
                let _ = debug_access::with_ctrl_mut(id, |ctrl| {
                    ctrl.execute(
                        &format!(
                            "DECLARE m VARCHAR2(100); BEGIN \
                             SYS.DBMS_PIPE.PURGE('{old}_E'); \
                             SYS.DBMS_PIPE.PURGE('{old}_C'); \
                             SYS.DBMS_PIPE.PACK_MESSAGE('STOP'); \
                             m := SYS.DBMS_PIPE.SEND_MESSAGE('{old}_C'); END;"
                        ),
                        &[],
                    )
                    .map(|_| ())
                    .map_err(|err| CoreError::Query(err.to_string()))
                });
            }
        }
        let pipe = format!(
            "PLSQLIDE_DBG_{}{}",
            &id.simple().to_string()[..8].to_ascii_uppercase(),
            &uuid::Uuid::new_v4().simple().to_string()[..6].to_ascii_uppercase()
        );
        if let Ok(mut guard) = LAST_PIPE.lock() {
            guard.insert(id, pipe.clone());
        }
        state(Some(pipe.clone()), true)?;
        let _ = debug_access::with_ctrl_mut(id, |ctrl| {
            ctrl.execute(
                &format!(
                    "DECLARE m VARCHAR2(100); BEGIN SYS.DBMS_PIPE.PURGE('{pipe}_E'); \
                     SYS.DBMS_PIPE.PURGE('{pipe}_C'); END;"
                ),
                &[],
            )
            .map(|_| ())
            .map_err(|err| CoreError::Query(err.to_string()))
        });
        SESSION_SOURCE
            .lock()
            .map_err(|_| CoreError::Other("session source lock".into()))?
            .insert(id, request.source.clone());
        Ok(pipe)
    };

    match request.command {
        DebugCommand::Start(_) => {
            let pipe = start_prep(&request)?;
            if request.source.trim().is_empty() {
                return Ok(DebugEngineEvent {
                    active: true,
                    system: DebugSystem::DbmsDebug,
                    status: "Debugger started (local instrumentation)".into(),
                    current_line: None,
                    message: "Local debugger ready — Run / Step operate on the block itself".into(),
                    log: vec![
                        "local instrumented debugging: no server debugger involved".into()
                    ],
                    variables: Vec::new(),
                });
            }
            // Start launches the block right away and pauses on the first
            // executable line — the UI must show the current line at once.
            instrumented_launch(&request, id, pipe, true)
        }
        DebugCommand::Stop => {
            if let Ok(mut guard) = SESSION_STATE.lock() {
                if let Some(session) = guard.get(&id) {
                    if let Some(pipe) = &session.pipe {
                        // Same ordering as start_prep: purge first, the
                        // STOP must survive in the pipe for the target.
                        let _ = debug_access::with_ctrl_mut(id, |ctrl| {
                            ctrl.execute(
                                &format!(
                                    "DECLARE m VARCHAR2(100); BEGIN \
                                     SYS.DBMS_PIPE.PURGE('{pipe}_E'); \
                                     SYS.DBMS_PIPE.PURGE('{pipe}_C'); \
                                     SYS.DBMS_PIPE.PACK_MESSAGE('STOP'); \
                                     m := SYS.DBMS_PIPE.SEND_MESSAGE('{pipe}_C'); END;"
                                ),
                                &[],
                            )
                            .map(|_| ())
                            .map_err(|err| CoreError::Query(err.to_string()))
                        });
                    }
                }
                guard.remove(&id);
                FINISHED.lock().ok().map(|mut f| f.remove(&id));
            }
            Ok(DebugEngineEvent {
                active: false,
                system: DebugSystem::DbmsDebug,
                status: "Debugger stopped".into(),
                current_line: None,
                message: "Local debug session stopped".into(),
                log: Vec::new(),
                variables: Vec::new(),
            })
        }
        DebugCommand::Run
        | DebugCommand::StepInto
        | DebugCommand::StepOver
        | DebugCommand::StepOut
        | DebugCommand::RunToCursor
        | DebugCommand::RunToNextException => {
            // No live session (never started, or the block already finished)
            // → Run starts a fresh debug run and pauses on the first line.
            let needs_start = !SESSION_STATE
                .lock()
                .map(|g| g.contains_key(&id))
                .unwrap_or(true)
                || FINISHED
                    .lock()
                    .ok()
                    .and_then(|f| f.get(&id).cloned())
                    .map(|st| st.finished.load(Ordering::SeqCst))
                    .unwrap_or(true);
            let resume_first = if needs_start {
                if request.source.trim().is_empty() {
                    return Err(CoreError::Other(
                        "Editor is empty — nothing to execute".into(),
                    ));
                }
                let pipe = start_prep(&request)?;
                return instrumented_launch(&request, id, pipe, true);
            } else {
                SESSION_SOURCE
                    .lock()
                    .map(|g| g.contains_key(&id))
                    .unwrap_or(false)
            };
            let pipe = current_pipe(id)?;
            instrumented_launch(&request, id, pipe, resume_first)
        }
        DebugCommand::ExecuteSqlInDebugSession => {
            ensure_active(id)?;
            let pipe = current_pipe(id)?;
            send_cmd(id, &pipe, "RUN:")?;
            let finished = FINISHED
                .lock()
                .ok()
                .and_then(|f| f.get(&id).cloned())
                .unwrap_or_else(|| {
                    Arc::new(RunState {
                        finished: AtomicBool::new(true),
                        error: Mutex::new(None),
                    })
                });
            let (status, current, msgs, _) = wait_pause(id, &pipe, &finished, false)?;
            Ok(DebugEngineEvent {
                active: true,
                system: DebugSystem::DbmsDebug,
                status: status.clone(),
                current_line: current,
                message: status,
                log: msgs,
                variables: Vec::new(),
            })
        }
    }
}

#[cfg(feature = "oracle")]
static FINISHED: LazyLock<Mutex<HashMap<ConnectionId, std::sync::Arc<RunState>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Launch the instrumented block (if staged) and wait for the next pause.
#[cfg(feature = "oracle")]
/// Detach the target connection, waiting up to ~5s while a previous run's
/// thread finishes dying (Stop + start_prep's STOP should kill it quickly).
fn detach_target_with_retry(id: ConnectionId) -> CoreResult<(oracle::Connection, u64)> {
    for _ in 0..25 {
        match super::oracle_pool::debug_access::detach_target(id) {
            Ok(pair) => return Ok(pair),
            Err(CoreError::Other(text)) if text.contains("busy") => {
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(err) => return Err(err),
        }
    }
    Err(CoreError::Other(
        "debug target session is busy — the previous block is still running; try again in a moment"
            .into(),
    ))
}

fn instrumented_launch(
    request: &DebugEngineRequest,
    id: ConnectionId,
    pipe: String,
    resume_first: bool,
) -> CoreResult<DebugEngineEvent> {
    use super::oracle_pool::debug_access;
    use std::sync::atomic::Ordering;

    let mode = if resume_first {
                "STEP".to_string()
            } else if matches!(
                request.command,
                DebugCommand::StepInto | DebugCommand::StepOver | DebugCommand::StepOut
            ) {
                "STEP".to_string()
            } else if matches!(request.command, DebugCommand::Run)
                && !request.breakpoints.is_empty()
            {
                format!(
                    "RUN:{}",
                    request
                        .breakpoints
                        .iter()
                        .map(|bp| bp.line.to_string())
                        .collect::<Vec<_>>()
                        .join(",")
                )
            } else if matches!(request.command, DebugCommand::RunToCursor) {
                format!("RUN:{}", request.cursor_line.unwrap_or(1))
            } else {
                "RUN:".to_string()
            };

            let mut log = vec![format!("resume mode {mode}")];
            send_cmd(id, &pipe, &mode)?;

            let source = if resume_first {
                SESSION_SOURCE
                    .lock()
                    .map_err(|_| CoreError::Other("session source lock".into()))?
                    .remove(&id)
            } else {
                None
            };

            let finished = if resume_first {
                let state = new_run_state();
                FINISHED
                    .lock()
                    .map_err(|_| CoreError::Other("finished lock".into()))?
                    .insert(id, state.clone());
                state
            } else {
                FINISHED
                    .lock()
                    .ok()
                    .and_then(|f| f.get(&id).cloned())
                    .unwrap_or_else(|| {
                        std::sync::Arc::new(RunState {
                            finished: std::sync::atomic::AtomicBool::new(true),
                            error: std::sync::Mutex::new(None),
                        })
                    })
            };

            if let Some(source) = source {
                let instrumented = super::dbg_instrument::instrument(&source, &pipe);
                tracing::info!(
                    "debug: block staged, {} chars, pipe {pipe}",
                    instrumented.len()
                );
                let (target, gen) = detach_target_with_retry(id)?;
                let id2 = id;
                let state = finished.clone();
                std::thread::spawn(move || {
                    let mut target = target;
                    let has_outs = instrumented.starts_with("DECLARE\nPLSQLIDE_P");
                    let mut bind_outs: Vec<(String, String)> = Vec::new();
                    let outcome = if has_outs {
                        // Wrapper opens :PLSQLIDE_OUT — bind the cursor and
                        // read the parameter values back after the block ends.
                        match target
                            .execute(
                                &instrumented,
                                &[&None::<oracle::sql_type::RefCursor>],
                            )
                        {
                            Ok(mut stmt) => {
                                if let Ok(mut refcur) =
                                    stmt.bind_value::<usize, oracle::sql_type::RefCursor>(1)
                                {
                                    if let Ok(rows) = refcur.query() {
                                        for row in rows.flatten() {
                                            let name = row
                                                .get::<usize, Option<String>>(0)
                                                .ok()
                                                .flatten()
                                                .unwrap_or_default();
                                            let val = row
                                                .get::<usize, Option<String>>(1)
                                                .ok()
                                                .flatten()
                                                .unwrap_or_else(|| "NULL".into());
                                            bind_outs.push((name, val));
                                        }
                                    }
                                }
                                Ok(())
                            }
                            Err(err) => Err(format!("block: {err}")),
                        }
                    } else {
                        target
                            .execute(&instrumented, &[])
                            .map(|_| ())
                            .map_err(|err| format!("block: {err}"))
                    };
                    tracing::info!("debug: target execute returned {outcome:?}");
                    if let Err(text) = &outcome {
                        if let Ok(mut guard) = state.error.lock() {
                            *guard = Some(text.clone());
                        }
                    }
                    if !bind_outs.is_empty() {
                        if let Ok(mut guard) = BINDOUTS.lock() {
                            guard.insert(id2, bind_outs);
                        }
                    }
                    state.finished.store(true, Ordering::SeqCst);
                    crate::core::oracle_pool::debug_access::reattach_target(id2, target, gen);
                });
                log.push("block running under instrumentation".into());
            }

            let (status, line, msgs, vars) =
                wait_pause(id, &pipe, &finished, resume_first)?;
            log.extend(msgs);
            if status == "program finished" {
                if let Ok(mut guard) = BINDOUTS.lock() {
                    if let Some(outs) = guard.remove(&id) {
                        for (name, val) in outs {
                            log.push(format!("BINDOUT {name}={val}"));
                        }
                    }
                }
            }
            Ok(DebugEngineEvent {
                active: true,
                system: DebugSystem::DbmsDebug,
                status: status.clone(),
                current_line: line,
                message: status,
                log,
                variables: vars,
            })
        }

#[cfg(feature = "oracle")]
fn current_pipe(id: ConnectionId) -> CoreResult<String> {
    SESSION_STATE
        .lock()
        .map_err(|_| CoreError::Other("session state lock".into()))?
        .get(&id)
        .and_then(|s| s.pipe.clone())
        .ok_or_else(|| CoreError::Other("Start the debugger first (F9)".into()))
}

#[cfg(feature = "oracle")]
fn send_cmd(id: ConnectionId, pipe: &str, message: &str) -> CoreResult<()> {
    use super::oracle_pool::debug_access;

    debug_access::with_ctrl_mut(id, |ctrl| {
        ctrl.execute(
            &format!(
                "DECLARE m VARCHAR2(100); BEGIN SYS.DBMS_PIPE.PURGE('{pipe}_C'); \
                 SYS.DBMS_PIPE.PACK_MESSAGE('{message}'); \
                 m := SYS.DBMS_PIPE.SEND_MESSAGE('{pipe}_C'); END;"
            ),
            &[],
        )
        .map_err(|err| CoreError::Query(format!("pipe send: {err}")))?;
        Ok(())
    })
}

/// Wait for the next PAUSE event (or block completion).
#[cfg(feature = "oracle")]
fn wait_pause(
    id: ConnectionId,
    pipe: &str,
    finished: &std::sync::Arc<RunState>,
    first_stop: bool,
) -> CoreResult<(String, Option<usize>, Vec<String>, Vec<String>)> {
    use super::oracle_pool::debug_access;
    use std::sync::atomic::Ordering;

    let mut log = Vec::new();
    // Declared-variable values arrive as a separate pipe message just
    // before the next PAUSE — collect them here.
    let mut vars = Vec::new();
    for _ in 0..60 {
        if finished.finished.load(Ordering::SeqCst) {
            let error = finished
                .error
                .lock()
                .ok()
                .and_then(|guard| guard.clone())
                .unwrap_or_default();
            if error.is_empty() {
                tracing::info!("debug: block finished, no pause");
                log.push("block finished".into());
                return Ok(("program finished".into(), None, log, Vec::new()));
            }
            tracing::info!("debug: block failed: {error}");
            log.push(error.clone());
            return Ok((format!("block failed: {error}"), None, log, Vec::new()));
        }
        // ponytail: the message comes back via a CLOB OUT bind — the old
        // SET_IDENTIFIER channel truncated at 64 bytes and this 11.2 build
        // stops reporting pipe items after the first UNPACK.
        let event = debug_access::with_ctrl_mut(id, |ctrl| {
            let mut stmt = ctrl
                .statement(&format!(
                    // ponytail: timeout must return a sentinel, not NULL —
                    // a NULL CLOB OUT bind raises "NULL value found" in
                    // rust-oracle and killed the wait loop on its first
                    // quiet second, so "program finished" was never seen.
                    "DECLARE t INTEGER; m VARCHAR2(32767); BEGIN \
                     t := SYS.DBMS_PIPE.RECEIVE_MESSAGE('{pipe}_E', 1); \
                     IF t = 0 THEN SYS.DBMS_PIPE.UNPACK_MESSAGE(m); \
                     ELSE m := '~idle~'; END IF; \
                     :out := m; END;"
                ))
                .build()
                .map_err(|err| CoreError::Query(format!("pipe recv stmt: {err}")))?;
            stmt.execute(&[&None::<oracle::sql_type::Clob>])
                .map_err(|err| CoreError::Query(format!("pipe recv: {err}")))?;
            let mut clob = stmt
                .bind_value::<usize, oracle::sql_type::Clob>(1)
                .map_err(|err| CoreError::Query(format!("pipe recv bind: {err}")))?;
            use std::io::Read;
            let mut out = String::new();
            clob
                .read_to_string(&mut out)
                .map_err(|err| CoreError::Query(format!("pipe recv read: {err}")))?;
            Ok::<String, CoreError>(out)
        });
        match event {
            Ok(out) if out == "~idle~" => {}
            Ok(out) => {
                for line in out.lines() {
                    if let Some(entry) = line.strip_prefix("V ") {
                        if let Some((name, value)) = entry.split_once('=') {
                            vars.push(format!("{name} = {value}"));
                        }
                    }
                }
                if let Some(rest) = out.strip_prefix("PAUSE ") {
                    if let Ok(line) = rest.trim().parse::<usize>() {
                        log.push(format!("paused at line {line}"));
                        log.push(format!("{} variable(s)", vars.len()));
                        return Ok((format!("paused at line {line}"), Some(line), log, vars));
                    }
                }
            }
            Err(err) => {
                log.push(format!("wait: {err}"));
                break;
            }
        }
        let _ = first_stop;
    }
    log.push("no pause event within the wait budget".into());
    Ok(("waiting… (no pause yet)".into(), None, log, Vec::new()))
}


/// Per-connection run state: finished flag + block execution error.
#[cfg(feature = "oracle")]
struct RunState {
    finished: std::sync::atomic::AtomicBool,
    error: std::sync::Mutex<Option<String>>,
}

#[cfg(feature = "oracle")]
fn new_run_state() -> std::sync::Arc<RunState> {
    std::sync::Arc::new(RunState {
        finished: std::sync::atomic::AtomicBool::new(false),
        error: std::sync::Mutex::new(None),
    })
}

/// Strip helper re-exported for [`is_anonymous_source`] (feature-independent).
#[cfg(not(feature = "oracle"))]
fn is_anonymous_source(source: &str) -> bool {
    source
        .lines()
        .map(|line| line.trim())
        .find(|line| !line.is_empty())
        .map(|line| {
            let upper = line.to_ascii_uppercase();
            upper.starts_with("DECLARE") || upper.starts_with("BEGIN")
        })
        .unwrap_or(false)
}
