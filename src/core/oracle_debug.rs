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
    pub const BREAK_NEXT_LINE: i32 = 2;
    pub const BREAK_ANY_CALL: i32 = 12;
    pub const BREAK_ANY_RETURN: i32 = 16;
    pub const BREAK_EXCEPTION: i32 = 512;
    pub const ABORT_EXECUTION: i32 = 8192;
}

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
        DebugCommand::StepInto => {
            continue_probe(&request, breakflags::BREAK_ANY_CALL, "Step Into")
        }
        DebugCommand::StepOver => {
            continue_probe(&request, breakflags::BREAK_NEXT_LINE, "Step Over")
        }
        DebugCommand::StepOut => {
            continue_probe(&request, breakflags::BREAK_ANY_RETURN, "Step Out")
        }
        DebugCommand::RunToCursor => run_to_cursor_probe(&request),
        DebugCommand::RunToNextException => {
            continue_probe(&request, breakflags::BREAK_EXCEPTION, "Run to next exception")
        }
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
    let oracle_host = JdwpSession::connect_host_for_oracle(&bind);

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
    log.extend(session.log.drain(..));

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
    log.extend(session.log.drain(..));

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
            log.extend(session.log.drain(..));
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
                log.extend(session.log.drain(..));
            }
        }
    }

    // Step / exception request before resume.
    let thread = LAST_THREAD
        .lock()
        .ok()
        .and_then(|g| g.get(&id).copied());
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
            log.extend(session.log.drain(..));
            session.resume_vm()?;
            log.extend(session.log.drain(..));
        } else {
            return Err(CoreError::Other("JDWP session missing — Start debugger first".into()));
        }
    }

    let maybe_source = SESSION_SOURCE
        .lock()
        .ok()
        .and_then(|mut g| g.remove(&id));

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
        Ok(())
    })?;
    log.push("controller ATTACH_SESSION ok".into());

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
    let mut log = vec![format!("{label} (breakflags={flags})")];

    let maybe_source = SESSION_SOURCE
        .lock()
        .ok()
        .and_then(|mut g| g.remove(&id));

    let pause = if let Some(source) = maybe_source {
        let (target_res, pause) = debug_access::with_pair_parallel(
            id,
            move |target| {
                target
                    .execute(&source, &[])
                    .map_err(|err| CoreError::Query(format!("target PL/SQL: {err}")))?;
                Ok(())
            },
            move |ctrl| probe_continue_on(ctrl, flags),
        )?;
        let _ = target_res;
        pause
    } else {
        debug_access::with_ctrl_mut(id, |ctrl| probe_continue_on(ctrl, flags))?
    };

    log.extend(pause.log);
    Ok(DebugEngineEvent {
        active: true,
        system: DebugSystem::DbmsDebug,
        status: pause.status.clone(),
        current_line: pause.line,
        message: format!("{label}: {}", pause.status),
        log,
        variables: read_probe_variables(id, &request.prefs)?,
    })
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
            let mut event =
                continue_probe(request, breakflags::BREAK_NEXT_LINE, "Run to cursor")?;
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
    Ok(PauseInfo {
        line,
        status,
        log: vec![format!(
            "CONTINUE flags={flags} retval={retval} line={line:?} raw={ident}"
        )],
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
  prog.namespace := DBMS_DEBUG.NAMESPACE_ANONYMOUS;
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
    let upper = trimmed.to_ascii_uppercase();
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
        return Err(CoreError::Other(
            "Start the debugger first (F9)".into(),
        ));
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
