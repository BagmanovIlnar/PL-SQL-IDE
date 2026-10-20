//! In-IDE debug session state mirrored from the live Oracle Probe engine.
//!
//! Database work lives in [`crate::core::oracle_debug`]; this module holds UI
//! breakpoints / current line and high-level [`DebugCommand`] values.

use serde::{Deserialize, Serialize};

use super::oracle_debugger_prefs::{self, DebugSystem, OracleDebuggerPrefs};

/// One source breakpoint (1-based line in the active worksheet).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Breakpoint {
    pub line: usize,
}

/// High-level debug actions from the toolbar / hotkeys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugCommand {
    /// Start debugger (F9) with the chosen system.
    Start(DebugSystem),
    /// Continue / run (Ctrl+F9).
    Run,
    /// Step into (Ctrl+N).
    StepInto,
    /// Step over (Ctrl+O).
    StepOver,
    /// Step out (Ctrl+T).
    StepOut,
    /// Run to the current cursor line.
    RunToCursor,
    /// Continue until the next exception.
    RunToNextException,
    /// Execute arbitrary SQL inside the debug target session.
    ExecuteSqlInDebugSession,
    /// Stop the active debug session.
    Stop,
}

/// Result of handling a debug command (UI status + optional SQL to run later).
#[derive(Debug, Clone)]
pub struct DebugCommandResult {
    pub message: String,
    /// Session / compile SQL suggested by this command (applied when a live session exists).
    pub sql: Vec<String>,
}

/// Active debug session state for the UI.
///
/// # Examples
///
/// ```
/// use plsqlide::core::debug_session::DebugSession;
/// use plsqlide::core::oracle_debugger_prefs::DebugSystem;
///
/// let mut session = DebugSession::default();
/// assert!(!session.active);
/// session.start(DebugSystem::DbmsDebug);
/// assert!(session.active);
/// ```
#[derive(Debug, Clone, Default)]
pub struct DebugSession {
    pub active: bool,
    pub system: DebugSystem,
    pub status: String,
    pub current_line: Option<usize>,
    pub cursor_line: Option<usize>,
    pub breakpoints: Vec<Breakpoint>,
}

impl DebugSession {
    pub fn start(&mut self, system: DebugSystem) {
        self.active = true;
        self.system = system;
        self.status = format!("Debugger started ({})", system.as_oracle());
        self.current_line = None;
    }

    pub fn stop(&mut self) {
        self.active = false;
        self.status = "Debugger stopped".into();
        self.current_line = None;
    }

    pub fn toggle_breakpoint(&mut self, line: usize) {
        if line == 0 {
            return;
        }
        if let Some(index) = self.breakpoints.iter().position(|b| b.line == line) {
            self.breakpoints.remove(index);
        } else {
            self.breakpoints.push(Breakpoint { line });
            self.breakpoints.sort_by_key(|b| b.line);
        }
    }

    pub fn has_breakpoint(&self, line: usize) -> bool {
        self.breakpoints.iter().any(|b| b.line == line)
    }

    /// Handle a toolbar/hotkey command using current debugger preferences.
    pub fn handle(
        &mut self,
        command: DebugCommand,
        prefs: &OracleDebuggerPrefs,
        connection_key: &str,
    ) -> DebugCommandResult {
        match command {
            DebugCommand::Start(system) => {
                self.start(system);
                let mut sql = Vec::new();
                match system {
                    DebugSystem::DbmsDebug => {
                        sql.push(
                            "-- DBMS_DEBUG: target session will use Probe API (attach on next compile/run)"
                                .into(),
                        );
                    }
                    DebugSystem::DbmsDebugJdwp => {
                        let host = if prefs.default_jdwp_network_adapter.eq_ignore_ascii_case("All")
                        {
                            "0.0.0.0"
                        } else {
                            prefs.default_jdwp_network_adapter.as_str()
                        };
                        let port = OracleDebuggerPrefs::clamp_jdwp_port(prefs.default_jdwp_port);
                        sql.push(format!(
                            "BEGIN DBMS_DEBUG_JDWP.CONNECT_TCP('{host}', {port}); END;"
                        ));
                    }
                }
                if prefs.step_over_sys_objects {
                    sql.push("-- step filter: skip SYS objects".into());
                }
                if prefs.step_over_package_spec_init {
                    sql.push("-- step filter: skip package spec init".into());
                }
                if prefs.step_over_package_body_init {
                    sql.push("-- step filter: skip package body init".into());
                }
                DebugCommandResult {
                    message: format!(
                        "Start debugger ({}) on {connection_key}; breakpoints={}",
                        system.as_oracle(),
                        self.breakpoints.len()
                    ),
                    sql,
                }
            }
            DebugCommand::Stop => {
                self.stop();
                DebugCommandResult {
                    message: "Debug session stopped".into(),
                    sql: Vec::new(),
                }
            }
            DebugCommand::Run => {
                if !self.active {
                    return self.handle(DebugCommand::Start(prefs.default_debug_system), prefs, connection_key);
                }
                self.status = "Run / continue".into();
                DebugCommandResult {
                    message: "Run (continue)".into(),
                    sql: vec!["-- DBMS_DEBUG.CONTINUE / JDWP resume".into()],
                }
            }
            DebugCommand::StepInto => self.step("Step Into", prefs),
            DebugCommand::StepOver => self.step("Step Over", prefs),
            DebugCommand::StepOut => self.step("Step Out", prefs),
            DebugCommand::RunToCursor => {
                if !self.active {
                    return DebugCommandResult {
                        message: "Start the debugger first (F9)".into(),
                        sql: Vec::new(),
                    };
                }
                let line = self.cursor_line.unwrap_or(1);
                self.status = format!("Run to cursor line {line}");
                DebugCommandResult {
                    message: self.status.clone(),
                    sql: vec![format!("-- run to line {line}")],
                }
            }
            DebugCommand::RunToNextException => {
                if !self.active {
                    return DebugCommandResult {
                        message: "Start the debugger first (F9)".into(),
                        sql: Vec::new(),
                    };
                }
                self.status = "Run to next exception".into();
                DebugCommandResult {
                    message: self.status.clone(),
                    sql: vec!["-- break on next exception".into()],
                }
            }
            DebugCommand::ExecuteSqlInDebugSession => {
                if !self.active {
                    return DebugCommandResult {
                        message: "Start the debugger first (F9)".into(),
                        sql: Vec::new(),
                    };
                }
                self.status = "Execute SQL in debug session…".into();
                DebugCommandResult {
                    message: self.status.clone(),
                    sql: Vec::new(),
                }
            }
        }
    }

    fn step(&mut self, label: &str, prefs: &OracleDebuggerPrefs) -> DebugCommandResult {
        if !self.active {
            return DebugCommandResult {
                message: "Start the debugger first (F9)".into(),
                sql: Vec::new(),
            };
        }
        self.status = label.to_string();
        let mut notes = vec![format!("-- {label}")];
        if prefs.step_over_sys_objects {
            notes.push("-- respecting: step over SYS".into());
        }
        DebugCommandResult {
            message: label.to_string(),
            sql: notes,
        }
    }
}

/// Load debugger prefs and decide compile DEBUG clause for a connection/object.
pub fn compile_debug_clause(connection_key: &str, object_name: Option<&str>) -> &'static str {
    oracle_debugger_prefs::load().compile_debug_clause(connection_key, object_name)
}
