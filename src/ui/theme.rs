//! Visual tokens and theme preference for the IDE shell.

use std::sync::{LazyLock, RwLock};

use iced::Color;

/// User-selected appearance preference.
///
/// # Examples
///
/// ```
/// use plsqlide::ui::theme::{ThemeMode, Palette};
///
/// let palette = Palette::resolve(ThemeMode::Light);
/// assert!(!palette.is_dark);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeMode {
    /// Follow the operating system light/dark setting.
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeMode {
    /// Label shown in Settings.
    pub fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }

    /// All selectable modes in Settings order.
    pub fn all() -> [Self; 3] {
        [Self::System, Self::Light, Self::Dark]
    }
}

/// Concrete colors used by the shell widgets.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub is_dark: bool,
    pub window_bg: Color,
    pub ribbon_bg: Color,
    pub ribbon_body_bg: Color,
    pub chrome_bg: Color,
    pub sidebar_bg: Color,
    pub sidebar_title_bg: Color,
    pub panel_bg: Color,
    pub border: Color,
    pub text: Color,
    pub muted: Color,
    pub accent: Color,
    pub hover: Color,
}

impl Palette {
    /// Light palette — editor matches the window chrome.
    pub fn light() -> Self {
        Self {
            is_dark: false,
            window_bg: Color::from_rgb(0.90, 0.91, 0.93),
            ribbon_bg: Color::from_rgb(0.94, 0.94, 0.96),
            ribbon_body_bg: Color::from_rgb(0.92, 0.93, 0.95),
            chrome_bg: Color::from_rgb(0.88, 0.89, 0.91),
            sidebar_bg: Color::from_rgb(0.93, 0.93, 0.95),
            sidebar_title_bg: Color::from_rgb(0.86, 0.87, 0.90),
            panel_bg: Color::from_rgb(0.96, 0.96, 0.97),
            border: Color::from_rgb(0.72, 0.74, 0.78),
            text: Color::from_rgb(0.18, 0.19, 0.22),
            muted: Color::from_rgb(0.42, 0.44, 0.48),
            accent: Color::from_rgb(0.18, 0.45, 0.78),
            hover: Color::from_rgb(0.82, 0.84, 0.88),
        }
    }

    /// Dark palette — editor matches the dark window chrome.
    pub fn dark() -> Self {
        Self {
            is_dark: true,
            window_bg: Color::from_rgb(0.16, 0.17, 0.19),
            ribbon_bg: Color::from_rgb(0.20, 0.21, 0.24),
            ribbon_body_bg: Color::from_rgb(0.18, 0.19, 0.22),
            chrome_bg: Color::from_rgb(0.18, 0.19, 0.22),
            sidebar_bg: Color::from_rgb(0.17, 0.18, 0.20),
            sidebar_title_bg: Color::from_rgb(0.22, 0.23, 0.26),
            panel_bg: Color::from_rgb(0.14, 0.15, 0.17),
            border: Color::from_rgb(0.32, 0.34, 0.38),
            text: Color::from_rgb(0.90, 0.91, 0.92),
            muted: Color::from_rgb(0.62, 0.65, 0.68),
            accent: Color::from_rgb(0.25, 0.52, 0.85),
            hover: Color::from_rgb(0.28, 0.30, 0.34),
        }
    }

    /// Resolves System/Light/Dark into concrete colors.
    pub fn resolve(mode: ThemeMode) -> Self {
        let dark = match mode {
            ThemeMode::Light => false,
            ThemeMode::Dark => true,
            ThemeMode::System => system_prefers_dark(),
        };
        if dark {
            Self::dark()
        } else {
            Self::light()
        }
    }
}

static ACTIVE: LazyLock<RwLock<Palette>> =
    LazyLock::new(|| RwLock::new(Palette::light()));

/// Applies the given preference so widget styles can read current colors.
pub fn apply(mode: ThemeMode) {
    if let Ok(mut guard) = ACTIVE.write() {
        *guard = Palette::resolve(mode);
    }
}

/// Current resolved palette (copied).
pub fn current() -> Palette {
    ACTIVE
        .read()
        .map(|guard| *guard)
        .unwrap_or_else(|_| Palette::light())
}

fn system_prefers_dark() -> bool {
    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = std::process::Command::new("defaults")
            .args(["read", "-g", "AppleInterfaceStyle"])
            .output()
        {
            return String::from_utf8_lossy(&output.stdout)
                .to_ascii_lowercase()
                .contains("dark");
        }
    }

    #[cfg(target_os = "windows")]
    {
        // AppsUseLightTheme = 0 means dark.
        use std::process::Command;
        if let Ok(output) = Command::new("reg")
            .args([
                "query",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
                "/v",
                "AppsUseLightTheme",
            ])
            .output()
        {
            let text = String::from_utf8_lossy(&output.stdout);
            if text.contains("0x0") {
                return true;
            }
            if text.contains("0x1") {
                return false;
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Ok(value) = std::env::var("GTK_THEME") {
            if value.to_ascii_lowercase().contains("dark") {
                return true;
            }
        }
        if let Ok(value) = std::env::var("COLORFGBG") {
            // Common convention: last number is background; high = light.
            if let Some(bg) = value.split(';').last() {
                if let Ok(n) = bg.parse::<u8>() {
                    return n < 8;
                }
            }
        }
    }

    false
}
