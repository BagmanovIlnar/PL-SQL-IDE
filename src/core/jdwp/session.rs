//! JDWP debugger session: listen, handshake, breakpoints, step, resume, vars.

use std::io::Read;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use super::protocol::{
    self, cmd_set, event_kind, event_req_cmd, mod_kind, step_depth, step_size, suspend_policy,
    vm_cmd, IdSizes, Packet, PacketReader, PacketWriter,
};
use crate::core::debug_session::Breakpoint;
use crate::core::{CoreError, CoreResult};

/// Step depth for EventRequest.Set STEP modifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepDepth {
    Into,
    Over,
    Out,
}

impl StepDepth {
    fn as_jdwp(self) -> i32 {
        match self {
            Self::Into => step_depth::INTO,
            Self::Over => step_depth::OVER,
            Self::Out => step_depth::OUT,
        }
    }
}

/// Pause information from a JDWP composite event.
#[derive(Debug, Clone, Default)]
pub struct JdwpPause {
    pub line: Option<usize>,
    pub thread_id: Option<u64>,
    pub class_id: Option<u64>,
    pub method_id: Option<u64>,
    pub code_index: Option<u64>,
    pub kind: String,
    pub variables: Vec<String>,
}

/// Active JDWP debugger connection (IDE side).
pub struct JdwpSession {
    stream: TcpStream,
    next_id: AtomicU32,
    sizes: IdSizes,
    pub bind_addr: SocketAddr,
    pub log: Vec<String>,
}

impl JdwpSession {
    /// Bind a TCP listener for Oracle `CONNECT_TCP` to dial into.
    pub fn listen(bind_host: &str, port: u16) -> CoreResult<TcpListener> {
        let host = if bind_host.eq_ignore_ascii_case("All") || bind_host == "0.0.0.0" {
            "0.0.0.0"
        } else {
            bind_host
        };
        let addr = format!("{host}:{port}");
        let listener = TcpListener::bind(&addr)
            .map_err(|err| CoreError::Other(format!("JDWP listen {addr}: {err}")))?;
        listener
            .set_nonblocking(false)
            .map_err(|err| CoreError::Other(format!("JDWP listen mode: {err}")))?;
        Ok(listener)
    }

    /// Host string Oracle should use in `CONNECT_TCP` (reachable address).
    pub fn connect_host_for_oracle(adapter: &str) -> String {
        if adapter.eq_ignore_ascii_case("All")
            || adapter == "0.0.0.0"
            || adapter.trim().is_empty()
        {
            "127.0.0.1".into()
        } else {
            adapter.trim().to_string()
        }
    }

    /// Accept one Oracle JDWP connection (with timeout) and complete handshake + IDSizes.
    pub fn accept(listener: TcpListener, timeout: Duration) -> CoreResult<Self> {
        let bind_addr = listener
            .local_addr()
            .unwrap_or_else(|_| SocketAddr::from(([0, 0, 0, 0], 0)));
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(listener.accept());
        });
        let (mut stream, peer) = match rx.recv_timeout(timeout) {
            Ok(Ok(pair)) => pair,
            Ok(Err(err)) => {
                return Err(CoreError::Other(format!("JDWP accept: {err}")));
            }
            Err(_) => {
                return Err(CoreError::Other(format!(
                    "JDWP accept timed out after {timeout:?}"
                )));
            }
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .ok();
        stream
            .set_write_timeout(Some(Duration::from_secs(30)))
            .ok();
        protocol::handshake(&mut stream)?;
        let mut session = Self {
            stream,
            next_id: AtomicU32::new(1),
            sizes: IdSizes::default(),
            bind_addr,
            log: vec![format!("JDWP accepted from {peer}")],
        };
        session.sizes = session.query_id_sizes()?;
        session.log.push(format!(
            "IDSizes field={} method={} object={} ref={} frame={}",
            session.sizes.field_id,
            session.sizes.method_id,
            session.sizes.object_id,
            session.sizes.reference_type_id,
            session.sizes.frame_id
        ));
        // Drain optional VM_START event.
        if let Ok(Some(pause)) = session.try_read_event(Duration::from_millis(500)) {
            session.log.push(format!("initial event: {}", pause.kind));
        }
        Ok(session)
    }

    fn next_packet_id(&self) -> u32 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    fn send_command(&mut self, cmd_set: u8, cmd: u8, data: Vec<u8>) -> CoreResult<Packet> {
        let id = self.next_packet_id();
        let packet = Packet::command(id, cmd_set, cmd, data);
        protocol::write_packet(&mut self.stream, &packet)?;
        loop {
            let reply = protocol::read_packet(&mut self.stream)?;
            if reply.is_event() {
                // Queue-less: parse later via wait — for now ignore async events mid-command
                // by continuing until matching reply id.
                self.log.push("JDWP event interleaved with command".into());
                continue;
            }
            if reply.id != id {
                continue;
            }
            if reply.error_code != 0 {
                return Err(CoreError::Other(format!(
                    "JDWP command {cmd_set}/{cmd} error {}",
                    reply.error_code
                )));
            }
            return Ok(reply);
        }
    }

    fn query_id_sizes(&mut self) -> CoreResult<IdSizes> {
        let reply = self.send_command(cmd_set::VIRTUAL_MACHINE, vm_cmd::ID_SIZES, Vec::new())?;
        let mut r = PacketReader::new(&reply.data);
        Ok(IdSizes {
            field_id: r.read_int()?,
            method_id: r.read_int()?,
            object_id: r.read_int()?,
            reference_type_id: r.read_int()?,
            frame_id: r.read_int()?,
        })
    }

    pub fn resume_vm(&mut self) -> CoreResult<()> {
        let _ = self.send_command(cmd_set::VIRTUAL_MACHINE, vm_cmd::RESUME, Vec::new())?;
        self.log.push("VirtualMachine.Resume".into());
        Ok(())
    }

    pub fn suspend_vm(&mut self) -> CoreResult<()> {
        let _ = self.send_command(cmd_set::VIRTUAL_MACHINE, vm_cmd::SUSPEND, Vec::new())?;
        self.log.push("VirtualMachine.Suspend".into());
        Ok(())
    }

    /// Clear all breakpoints then set line breakpoints (best-effort LocationOnly).
    pub fn set_breakpoints(&mut self, breakpoints: &[Breakpoint]) -> CoreResult<()> {
        let _ = self.send_command(
            cmd_set::EVENT_REQUEST,
            event_req_cmd::CLEAR_ALL_BREAKPOINTS,
            Vec::new(),
        );
        // Discover classes — Oracle may expose PL/SQL units here.
        let classes = self.all_classes().unwrap_or_default();
        self.log.push(format!("AllClasses count={}", classes.len()));

        for bp in breakpoints {
            // Prefer class-match on anonymous / worksheet units; also try first class.
            match self.set_line_breakpoint(bp.line, &classes) {
                Ok(req_id) => self
                    .log
                    .push(format!("breakpoint line {} → requestId={req_id}", bp.line)),
                Err(err) => self
                    .log
                    .push(format!("breakpoint line {}: {err}", bp.line)),
            }
        }
        Ok(())
    }

    fn all_classes(&mut self) -> CoreResult<Vec<(u8, u64, String)>> {
        let reply = self.send_command(cmd_set::VIRTUAL_MACHINE, vm_cmd::ALL_CLASSES, Vec::new())?;
        let mut r = PacketReader::new(&reply.data);
        let count = r.read_int()?;
        let mut out = Vec::new();
        for _ in 0..count.max(0) {
            let tag = r.read_byte()?;
            let type_id = r.read_id(self.sizes.reference_type_id)?;
            let signature = r.read_string()?;
            let _status = r.read_int()?;
            out.push((tag, type_id, signature));
        }
        Ok(out)
    }

    fn set_line_breakpoint(
        &mut self,
        line: usize,
        classes: &[(u8, u64, String)],
    ) -> CoreResult<i32> {
        // Try LocationOnly on a matching class; fall back to ClassMatch + Count.
        let class = classes
            .iter()
            .find(|(_, _, sig)| {
                let s = sig.to_ascii_lowercase();
                s.contains("anonymous") || s.contains("plsql") || s.contains("$oracle")
            })
            .or_else(|| classes.first());

        let mut w = PacketWriter::new();
        w.write_byte(event_kind::BREAKPOINT);
        w.write_byte(suspend_policy::ALL);
        if let Some(&(tag, type_id, _)) = class {
            w.write_int(1); // modifiers
            w.write_byte(mod_kind::LOCATION_ONLY);
            // code index often equals line for Oracle PL/SQL JDWP
            w.write_location(&self.sizes, tag, type_id, 0, line as u64);
        } else {
            w.write_int(1);
            w.write_byte(mod_kind::CLASS_MATCH);
            w.write_string("*");
        }
        let reply =
            self.send_command(cmd_set::EVENT_REQUEST, event_req_cmd::SET, w.into_bytes())?;
        let mut r = PacketReader::new(&reply.data);
        r.read_int()
    }

    /// Install a one-shot step request on `thread` (or all threads if None).
    pub fn request_step(&mut self, thread_id: Option<u64>, depth: StepDepth) -> CoreResult<i32> {
        let mut w = PacketWriter::new();
        w.write_byte(event_kind::SINGLE_STEP);
        w.write_byte(suspend_policy::ALL);
        w.write_int(1);
        w.write_byte(mod_kind::STEP);
        let tid = thread_id.unwrap_or(0);
        w.write_id(self.sizes.object_id, tid);
        w.write_int(step_size::LINE);
        w.write_int(depth.as_jdwp());
        let reply =
            self.send_command(cmd_set::EVENT_REQUEST, event_req_cmd::SET, w.into_bytes())?;
        let mut r = PacketReader::new(&reply.data);
        let req = r.read_int()?;
        self.log.push(format!(
            "step request id={req} depth={depth:?} thread={tid}"
        ));
        Ok(req)
    }

    /// Request break on uncaught exceptions.
    pub fn request_exception_break(&mut self) -> CoreResult<i32> {
        let mut w = PacketWriter::new();
        w.write_byte(event_kind::EXCEPTION);
        w.write_byte(suspend_policy::ALL);
        w.write_int(1);
        // ExceptionOnly modifier = 8: refTypeID + caught + uncaught
        w.write_byte(8);
        w.write_id(self.sizes.reference_type_id, 0);
        w.write_boolean(false);
        w.write_boolean(true);
        let reply =
            self.send_command(cmd_set::EVENT_REQUEST, event_req_cmd::SET, w.into_bytes())?;
        let mut r = PacketReader::new(&reply.data);
        r.read_int()
    }

    pub fn wait_for_pause(&mut self, timeout: Duration) -> CoreResult<JdwpPause> {
        self.stream
            .set_read_timeout(Some(timeout))
            .map_err(|err| CoreError::Other(format!("JDWP wait timeout set: {err}")))?;
        loop {
            let packet = protocol::read_packet(&mut self.stream)?;
            if packet.is_reply() {
                continue;
            }
            if packet.is_event() {
                let pause = self.parse_composite_event(&packet.data)?;
                let mut pause = pause;
                if let (Some(thread), Some(_class)) = (pause.thread_id, pause.class_id) {
                    pause.variables = self
                        .read_frame_variables(thread)
                        .unwrap_or_default();
                }
                return Ok(pause);
            }
        }
    }

    fn try_read_event(&mut self, timeout: Duration) -> CoreResult<Option<JdwpPause>> {
        self.stream.set_read_timeout(Some(timeout)).ok();
        match protocol::read_packet(&mut self.stream) {
            Ok(packet) if packet.is_event() => Ok(Some(self.parse_composite_event(&packet.data)?)),
            Ok(_) => Ok(None),
            Err(_) => Ok(None),
        }
    }

    fn parse_composite_event(&self, data: &[u8]) -> CoreResult<JdwpPause> {
        let mut r = PacketReader::new(data);
        let _suspend = r.read_byte()?;
        let count = r.read_int()?;
        let mut pause = JdwpPause {
            kind: "event".into(),
            ..JdwpPause::default()
        };
        for _ in 0..count.max(0) {
            let kind = r.read_byte()?;
            let _request_id = r.read_int()?;
            let thread = r.read_id(self.sizes.object_id)?;
            pause.thread_id = Some(thread);
            pause.kind = match kind {
                event_kind::SINGLE_STEP => "step".into(),
                event_kind::BREAKPOINT => "breakpoint".into(),
                event_kind::EXCEPTION => "exception".into(),
                event_kind::VM_START => "vm_start".into(),
                event_kind::VM_DEATH => "vm_death".into(),
                other => format!("kind={other}"),
            };
            match kind {
                event_kind::SINGLE_STEP | event_kind::BREAKPOINT => {
                    let (_tag, class, method, index) = r.read_location(&self.sizes)?;
                    pause.class_id = Some(class);
                    pause.method_id = Some(method);
                    pause.code_index = Some(index);
                    pause.line = Some(index as usize);
                }
                event_kind::EXCEPTION => {
                    let _exc = r.read_id(self.sizes.object_id)?;
                    let (_tag, class, method, index) = r.read_location(&self.sizes)?;
                    let _catch = r.read_location(&self.sizes)?;
                    pause.class_id = Some(class);
                    pause.method_id = Some(method);
                    pause.code_index = Some(index);
                    pause.line = Some(index as usize);
                }
                event_kind::VM_START | event_kind::THREAD_START | event_kind::THREAD_DEATH => {}
                event_kind::CLASS_PREPARE => {
                    let _ref_type_tag = r.read_byte()?;
                    let _type_id = r.read_id(self.sizes.reference_type_id)?;
                    let _sig = r.read_string()?;
                    let _status = r.read_int()?;
                }
                _ => {
                    // Best-effort: stop parsing further unknown events.
                    break;
                }
            }
        }
        Ok(pause)
    }

    fn read_frame_variables(&mut self, thread_id: u64) -> CoreResult<Vec<String>> {
        // ThreadReference.Frames
        let mut w = PacketWriter::new();
        w.write_id(self.sizes.object_id, thread_id);
        w.write_int(0);
        w.write_int(1);
        let reply = self.send_command(cmd_set::THREAD_REFERENCE, 6, w.into_bytes())?;
        let mut r = PacketReader::new(&reply.data);
        let frame_count = r.read_int()?;
        if frame_count <= 0 {
            return Ok(vec!["(no stack frames)".into()]);
        }
        let frame_id = r.read_id(self.sizes.frame_id)?;
        let (_tag, class, method, index) = r.read_location(&self.sizes)?;
        let mut vars = vec![format!(
            "frame class={class:#x} method={method:#x} index={index}"
        )];

        // ReferenceType.Methods / VariableTable is heavy; expose location for UI popup.
        vars.push(format!("thread={thread_id:#x} frame={frame_id:#x}"));
        let _ = frame_id;
        Ok(vars)
    }

    pub fn dispose(&mut self) -> CoreResult<()> {
        // VirtualMachine.Dispose (6) — detach without killing the debuggee process.
        let _ = self.send_command(cmd_set::VIRTUAL_MACHINE, 6, Vec::new());
        Ok(())
    }
}

impl Drop for JdwpSession {
    fn drop(&mut self) {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }
}

/// Helper used by tests / diagnostics.
#[allow(dead_code)]
pub fn drain_stream(stream: &mut TcpStream) {
    let mut buf = [0u8; 256];
    let _ = stream.read(&mut buf);
}
