//! Low-level JDWP packet codec (big-endian).

use std::io::{Read, Write};

use crate::core::{CoreError, CoreResult};

pub const HANDSHAKE: &[u8] = b"JDWP-Handshake";

pub const FLAG_REPLY: u8 = 0x80;

/// Command sets.
pub mod cmd_set {
    pub const VIRTUAL_MACHINE: u8 = 1;
    pub const REFERENCE_TYPE: u8 = 2;
    pub const THREAD_REFERENCE: u8 = 11;
    pub const STACK_FRAME: u8 = 16;
    pub const EVENT_REQUEST: u8 = 15;
    pub const EVENT: u8 = 64;
}

pub mod vm_cmd {
    pub const VERSION: u8 = 1;
    pub const ALL_CLASSES: u8 = 3;
    pub const ALL_THREADS: u8 = 4;
    pub const ID_SIZES: u8 = 7;
    pub const SUSPEND: u8 = 8;
    pub const RESUME: u8 = 9;
    pub const EXIT: u8 = 10;
}

pub mod event_req_cmd {
    pub const SET: u8 = 1;
    pub const CLEAR: u8 = 2;
    pub const CLEAR_ALL_BREAKPOINTS: u8 = 3;
}

pub mod event_cmd {
    pub const COMPOSITE: u8 = 100;
}

pub mod event_kind {
    pub const SINGLE_STEP: u8 = 1;
    pub const BREAKPOINT: u8 = 2;
    pub const EXCEPTION: u8 = 4;
    pub const THREAD_START: u8 = 6;
    pub const THREAD_DEATH: u8 = 7;
    pub const CLASS_PREPARE: u8 = 8;
    pub const VM_START: u8 = 90;
    pub const VM_DEATH: u8 = 99;
}

pub mod step_depth {
    pub const INTO: i32 = 0;
    pub const OVER: i32 = 1;
    pub const OUT: i32 = 2;
}

pub mod step_size {
    pub const MIN: i32 = 0;
    pub const LINE: i32 = 1;
}

pub mod suspend_policy {
    pub const NONE: u8 = 0;
    pub const EVENT_THREAD: u8 = 1;
    pub const ALL: u8 = 2;
}

pub mod mod_kind {
    pub const COUNT: u8 = 1;
    pub const THREAD_ONLY: u8 = 3;
    pub const CLASS_ONLY: u8 = 4;
    pub const CLASS_MATCH: u8 = 5;
    pub const LOCATION_ONLY: u8 = 7;
    pub const STEP: u8 = 10;
}

#[derive(Debug, Clone)]
pub struct IdSizes {
    pub field_id: i32,
    pub method_id: i32,
    pub object_id: i32,
    pub reference_type_id: i32,
    pub frame_id: i32,
}

impl Default for IdSizes {
    fn default() -> Self {
        Self {
            field_id: 8,
            method_id: 8,
            object_id: 8,
            reference_type_id: 8,
            frame_id: 8,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Packet {
    pub id: u32,
    pub flags: u8,
    pub cmd_set: u8,
    pub cmd: u8,
    pub error_code: u16,
    pub data: Vec<u8>,
}

impl Packet {
    pub fn command(id: u32, cmd_set: u8, cmd: u8, data: Vec<u8>) -> Self {
        Self {
            id,
            flags: 0,
            cmd_set,
            cmd,
            error_code: 0,
            data,
        }
    }

    pub fn is_reply(&self) -> bool {
        self.flags & FLAG_REPLY != 0
    }

    pub fn is_event(&self) -> bool {
        !self.is_reply() && self.cmd_set == cmd_set::EVENT && self.cmd == event_cmd::COMPOSITE
    }
}

pub struct PacketWriter {
    buf: Vec<u8>,
}

impl PacketWriter {
    pub fn new() -> Self {
        Self { buf: Vec::new() }
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.buf
    }

    pub fn write_byte(&mut self, v: u8) {
        self.buf.push(v);
    }

    pub fn write_boolean(&mut self, v: bool) {
        self.buf.push(u8::from(v));
    }

    pub fn write_int(&mut self, v: i32) {
        self.buf.extend_from_slice(&v.to_be_bytes());
    }

    pub fn write_long(&mut self, v: i64) {
        self.buf.extend_from_slice(&v.to_be_bytes());
    }

    pub fn write_string(&mut self, s: &str) {
        let bytes = s.as_bytes();
        self.write_int(bytes.len() as i32);
        self.buf.extend_from_slice(bytes);
    }

    pub fn write_id(&mut self, size: i32, value: u64) {
        match size {
            1 => self.write_byte(value as u8),
            2 => self.buf.extend_from_slice(&(value as u16).to_be_bytes()),
            4 => self.buf.extend_from_slice(&(value as u32).to_be_bytes()),
            _ => self.write_long(value as i64),
        }
    }

    /// Location: tag(1) + classID + methodID + index(8)
    pub fn write_location(&mut self, sizes: &IdSizes, tag: u8, class: u64, method: u64, index: u64) {
        self.write_byte(tag);
        self.write_id(sizes.reference_type_id, class);
        self.write_id(sizes.method_id, method);
        self.write_long(index as i64);
    }
}

pub struct PacketReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> PacketReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    pub fn read_byte(&mut self) -> CoreResult<u8> {
        if self.pos >= self.data.len() {
            return Err(CoreError::Other("JDWP: unexpected end of packet".into()));
        }
        let v = self.data[self.pos];
        self.pos += 1;
        Ok(v)
    }

    pub fn read_boolean(&mut self) -> CoreResult<bool> {
        Ok(self.read_byte()? != 0)
    }

    pub fn read_int(&mut self) -> CoreResult<i32> {
        let mut buf = [0u8; 4];
        self.read_exact(&mut buf)?;
        Ok(i32::from_be_bytes(buf))
    }

    pub fn read_long(&mut self) -> CoreResult<i64> {
        let mut buf = [0u8; 8];
        self.read_exact(&mut buf)?;
        Ok(i64::from_be_bytes(buf))
    }

    pub fn read_string(&mut self) -> CoreResult<String> {
        let len = self.read_int()?;
        if len < 0 {
            return Err(CoreError::Other("JDWP: negative string length".into()));
        }
        let len = len as usize;
        if self.pos + len > self.data.len() {
            return Err(CoreError::Other("JDWP: string overruns packet".into()));
        }
        let s = String::from_utf8_lossy(&self.data[self.pos..self.pos + len]).into_owned();
        self.pos += len;
        Ok(s)
    }

    pub fn read_id(&mut self, size: i32) -> CoreResult<u64> {
        match size {
            1 => Ok(u64::from(self.read_byte()?)),
            2 => {
                let mut buf = [0u8; 2];
                self.read_exact(&mut buf)?;
                Ok(u64::from(u16::from_be_bytes(buf)))
            }
            4 => {
                let mut buf = [0u8; 4];
                self.read_exact(&mut buf)?;
                Ok(u64::from(u32::from_be_bytes(buf)))
            }
            _ => Ok(self.read_long()? as u64),
        }
    }

    pub fn read_location(&mut self, sizes: &IdSizes) -> CoreResult<(u8, u64, u64, u64)> {
        let tag = self.read_byte()?;
        let class = self.read_id(sizes.reference_type_id)?;
        let method = self.read_id(sizes.method_id)?;
        let index = self.read_long()? as u64;
        Ok((tag, class, method, index))
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> CoreResult<()> {
        if self.pos + buf.len() > self.data.len() {
            return Err(CoreError::Other("JDWP: packet underrun".into()));
        }
        buf.copy_from_slice(&self.data[self.pos..self.pos + buf.len()]);
        self.pos += buf.len();
        Ok(())
    }
}

pub fn write_packet(stream: &mut impl Write, packet: &Packet) -> CoreResult<()> {
    let len = 11 + packet.data.len();
    let mut header = Vec::with_capacity(len);
    header.extend_from_slice(&(len as u32).to_be_bytes());
    header.extend_from_slice(&packet.id.to_be_bytes());
    header.push(packet.flags);
    if packet.is_reply() {
        header.extend_from_slice(&packet.error_code.to_be_bytes());
    } else {
        header.push(packet.cmd_set);
        header.push(packet.cmd);
    }
    header.extend_from_slice(&packet.data);
    stream
        .write_all(&header)
        .map_err(|err| CoreError::Other(format!("JDWP write: {err}")))?;
    stream
        .flush()
        .map_err(|err| CoreError::Other(format!("JDWP flush: {err}")))?;
    Ok(())
}

pub fn read_packet(stream: &mut impl Read) -> CoreResult<Packet> {
    let mut len_buf = [0u8; 4];
    stream
        .read_exact(&mut len_buf)
        .map_err(|err| CoreError::Other(format!("JDWP read length: {err}")))?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len < 11 {
        return Err(CoreError::Other(format!("JDWP: invalid packet length {len}")));
    }
    let mut rest = vec![0u8; len - 4];
    stream
        .read_exact(&mut rest)
        .map_err(|err| CoreError::Other(format!("JDWP read body: {err}")))?;
    let id = u32::from_be_bytes(rest[0..4].try_into().unwrap());
    let flags = rest[4];
    if flags & FLAG_REPLY != 0 {
        let error_code = u16::from_be_bytes(rest[5..7].try_into().unwrap());
        Ok(Packet {
            id,
            flags,
            cmd_set: 0,
            cmd: 0,
            error_code,
            data: rest[7..].to_vec(),
        })
    } else {
        Ok(Packet {
            id,
            flags,
            cmd_set: rest[5],
            cmd: rest[6],
            error_code: 0,
            data: rest[7..].to_vec(),
        })
    }
}

pub fn handshake(stream: &mut (impl Read + Write)) -> CoreResult<()> {
    stream
        .write_all(HANDSHAKE)
        .map_err(|err| CoreError::Other(format!("JDWP handshake write: {err}")))?;
    stream
        .flush()
        .map_err(|err| CoreError::Other(format!("JDWP handshake flush: {err}")))?;
    let mut buf = [0u8; HANDSHAKE.len()];
    stream
        .read_exact(&mut buf)
        .map_err(|err| CoreError::Other(format!("JDWP handshake read: {err}")))?;
    if &buf != HANDSHAKE {
        return Err(CoreError::Other(format!(
            "JDWP handshake mismatch: {:?}",
            String::from_utf8_lossy(&buf)
        )));
    }
    Ok(())
}
