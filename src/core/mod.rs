//! Application core: shared DB types and Phase 0 connection spikes.

pub mod connection_profile;
pub mod db_kind;
pub mod error;
pub mod object_metadata;
pub mod oracle_connection_prefs;
pub mod postgres_spike;

#[cfg(feature = "oracle")]
pub mod oracle_spike;

pub use connection_profile::{ConnectionId, ConnectionProfile};
pub use db_kind::DbKind;
pub use error::{CoreError, CoreResult};
pub use oracle_connection_prefs::{
    OracleConnectionPrefs, OracleEnvVar, OracleSessionMode,
};
