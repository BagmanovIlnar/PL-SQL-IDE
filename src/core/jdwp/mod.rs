//! JDWP (Java Debug Wire Protocol) debugger side for Oracle `DBMS_DEBUG_JDWP`.
//!
//! The IDE listens on TCP; the Oracle session connects via
//! `DBMS_DEBUG_JDWP.CONNECT_TCP`. After the handshake the IDE drives
//! suspend/resume, breakpoints, stepping, and stack/variable reads.

#![cfg(feature = "oracle")]

mod protocol;
mod session;

pub use session::{JdwpSession, StepDepth};
