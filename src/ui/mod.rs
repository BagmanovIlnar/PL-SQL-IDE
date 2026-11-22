//! iced UI for the IDE shell.

pub mod app;
pub mod completion;
pub mod debug_toolbar;
pub mod i18n;
pub mod oracle_compiler;
pub mod oracle_debugger;
pub mod oracle_options;
pub mod oracle_prefs;
pub mod oracle_wizard;
pub mod parameters;
pub mod postgres_prefs;
pub mod preferences;
pub mod result_grid;
pub mod theme;
pub mod updater;
pub mod window_kind;

pub use app::run;
