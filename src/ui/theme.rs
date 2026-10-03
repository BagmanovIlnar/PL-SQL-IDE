//! Visual tokens for the IDE shell.
//!
//! Colors intentionally follow a classic desktop DB-IDE layout (menu, left browser,
//! editor, bottom output) without copying any commercial product branding.

use iced::Color;

/// Background of the whole window chrome.
pub const WINDOW_BG: Color = Color::from_rgb(0.18, 0.19, 0.21);

/// Menu bar and status bar fill.
pub const CHROME_BG: Color = Color::from_rgb(0.22, 0.24, 0.27);

/// Left object browser panel.
pub const SIDEBAR_BG: Color = Color::from_rgb(0.16, 0.17, 0.19);

/// Editor and output panels.
pub const PANEL_BG: Color = Color::from_rgb(0.12, 0.13, 0.15);

/// Borders between panes.
pub const BORDER: Color = Color::from_rgb(0.30, 0.32, 0.35);

/// Primary text.
pub const TEXT: Color = Color::from_rgb(0.90, 0.91, 0.92);

/// Secondary / muted text.
pub const MUTED: Color = Color::from_rgb(0.62, 0.65, 0.68);

/// Selected browser node / active tab accent.
pub const ACCENT: Color = Color::from_rgb(0.20, 0.45, 0.75);

/// Hover fill for browser rows and menu-like buttons.
pub const HOVER: Color = Color::from_rgb(0.26, 0.28, 0.32);
