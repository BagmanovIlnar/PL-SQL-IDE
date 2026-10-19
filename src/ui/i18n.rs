//! Runtime UI localization loaded from JSON locale files.
//!
//! Built-in English and Russian catalogs ship with the binary and are copied
//! into the user config `locales/` folder on first launch (only if missing).
//! Extra languages can be added by dropping a new `*.json` file there — no rebuild.
//!
//! # Locale JSON shape
//!
//! ```json
//! {
//!   "code": "en",
//!   "name": "English",
//!   "strings": {
//!     "ribbon.tab.file": "File"
//!   }
//! }
//! ```
//!
//! # Examples
//!
//! ```
//! use plsqlide::ui::i18n::{self, LocaleInfo};
//!
//! i18n::init();
//! assert_eq!(i18n::t("ribbon.tab.file"), "File");
//! assert!(i18n::available().iter().any(|l| l.code == "en"));
//!
//! let _ = LocaleInfo {
//!     code: "en".into(),
//!     name: "English".into(),
//! };
//! ```

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, RwLock};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

const EMBEDDED_EN: &str = include_str!("../../locales/en.json");
const EMBEDDED_RU: &str = include_str!("../../locales/ru.json");
const DEFAULT_LOCALE: &str = "en";

static I18N: LazyLock<RwLock<I18n>> = LazyLock::new(|| RwLock::new(I18n::uninitialized()));

/// One language offered in Preferences → Appearance.
///
/// # Examples
///
/// ```
/// use plsqlide::ui::i18n::LocaleInfo;
///
/// let locale = LocaleInfo {
///     code: "ru".into(),
///     name: "Russian".into(),
/// };
/// assert_eq!(locale.to_string(), "Russian");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocaleInfo {
    /// Stable locale id (`en`, `ru`, `de`, …) — filename stem / JSON `code`.
    pub code: String,
    /// Human-readable name shown in the Language combo.
    pub name: String,
}

impl std::fmt::Display for LocaleInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)
    }
}

#[derive(Debug, Clone, Deserialize)]
struct LocaleFile {
    code: String,
    name: String,
    #[serde(default)]
    strings: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct AppSettings {
    #[serde(default = "default_locale_code")]
    locale: String,
}

fn default_locale_code() -> String {
    DEFAULT_LOCALE.to_string()
}

/// In-memory localization state: available locales, active catalog, English fallback.
struct I18n {
    locales_dir: PathBuf,
    settings_path: PathBuf,
    available: Vec<LocaleInfo>,
    current_code: String,
    current: HashMap<String, String>,
    fallback: HashMap<String, String>,
}

impl I18n {
    fn uninitialized() -> Self {
        Self {
            locales_dir: PathBuf::new(),
            settings_path: PathBuf::new(),
            available: Vec::new(),
            current_code: DEFAULT_LOCALE.to_string(),
            current: HashMap::new(),
            fallback: HashMap::new(),
        }
    }

    fn bootstrap(config_dir: &Path) -> Self {
        let locales_dir = config_dir.join("locales");
        let settings_path = config_dir.join("settings.json");
        let _ = fs::create_dir_all(&locales_dir);
        let english = parse_locale_file(EMBEDDED_EN);
        sync_shipped_locale(&locales_dir, "en.json", EMBEDDED_EN, None);
        sync_shipped_locale(
            &locales_dir,
            "ru.json",
            EMBEDDED_RU,
            english.as_ref(),
        );

        let fallback = parse_locale_file(EMBEDDED_EN)
            .map(|file| file.strings)
            .unwrap_or_default();

        let settings = load_settings(&settings_path);
        let available = scan_locales(&locales_dir);
        let current_code = if available.iter().any(|l| l.code == settings.locale) {
            settings.locale
        } else {
            DEFAULT_LOCALE.to_string()
        };

        let current = load_catalog(&locales_dir, &current_code).unwrap_or_else(|| fallback.clone());

        Self {
            locales_dir,
            settings_path,
            available,
            current_code,
            current,
            fallback,
        }
    }

    fn t(&self, key: &str) -> String {
        self.current
            .get(key)
            .or_else(|| self.fallback.get(key))
            .cloned()
            .unwrap_or_else(|| key.to_string())
    }

    fn set_locale(&mut self, code: &str) -> bool {
        if !self.available.iter().any(|l| l.code == code) {
            return false;
        }
        let Some(catalog) = load_catalog(&self.locales_dir, code) else {
            return false;
        };
        self.current_code = code.to_string();
        self.current = catalog;
        true
    }

    fn reload_available(&mut self) {
        self.available = scan_locales(&self.locales_dir);
        if !self.available.iter().any(|l| l.code == self.current_code) {
            self.current_code = DEFAULT_LOCALE.to_string();
            self.current =
                load_catalog(&self.locales_dir, DEFAULT_LOCALE).unwrap_or_else(|| self.fallback.clone());
        }
    }

    fn save_settings(&self) {
        let settings = AppSettings {
            locale: self.current_code.clone(),
        };
        if let Ok(json) = serde_json::to_string_pretty(&settings) {
            let _ = fs::write(&self.settings_path, json);
        }
    }
}

/// Write shipped locale if missing; otherwise merge keys from the embedded
/// catalog. Existing custom translations are kept. Values that still match the
/// English catalog (untranslated placeholders) are refreshed from `embedded`.
fn sync_shipped_locale(
    dir: &Path,
    file_name: &str,
    contents: &str,
    english: Option<&LocaleFile>,
) {
    let path = dir.join(file_name);
    let Some(embedded) = parse_locale_file(contents) else {
        return;
    };
    if !path.exists() {
        let _ = fs::write(&path, contents);
        return;
    }
    let Ok(raw) = fs::read_to_string(&path) else {
        return;
    };
    let Some(mut on_disk) = parse_locale_file(&raw) else {
        let _ = fs::write(&path, contents);
        return;
    };
    let mut changed = false;
    if on_disk.code.is_empty() {
        on_disk.code = embedded.code.clone();
        changed = true;
    }
    if on_disk.name.is_empty() {
        on_disk.name = embedded.name.clone();
        changed = true;
    }
    for (key, value) in &embedded.strings {
        match on_disk.strings.get(key) {
            None => {
                on_disk.strings.insert(key.clone(), value.clone());
                changed = true;
            }
            Some(existing)
                if existing != value
                    && english
                        .and_then(|en| en.strings.get(key))
                        .is_some_and(|en_val| en_val == existing) =>
            {
                // Still the English string — apply the shipped translation.
                on_disk.strings.insert(key.clone(), value.clone());
                changed = true;
            }
            _ => {}
        }
    }
    if changed {
        let _ = write_locale_file(&path, &on_disk);
    }
}

fn write_locale_file(path: &Path, file: &LocaleFile) -> std::io::Result<()> {
    #[derive(Serialize)]
    struct Out<'a> {
        code: &'a str,
        name: &'a str,
        strings: &'a HashMap<String, String>,
    }
    let out = Out {
        code: &file.code,
        name: &file.name,
        strings: &file.strings,
    };
    let json = serde_json::to_string_pretty(&out).map_err(std::io::Error::other)?;
    fs::write(path, json)
}

fn parse_locale_file(raw: &str) -> Option<LocaleFile> {
    serde_json::from_str(raw).ok()
}

fn load_catalog(dir: &Path, code: &str) -> Option<HashMap<String, String>> {
    let path = dir.join(format!("{code}.json"));
    let raw = fs::read_to_string(path).ok()?;
    parse_locale_file(&raw).map(|file| file.strings)
}

fn scan_locales(dir: &Path) -> Vec<LocaleInfo> {
    let mut locales = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return locales;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(raw) = fs::read_to_string(&path) else {
            continue;
        };
        let Some(file) = parse_locale_file(&raw) else {
            continue;
        };
        if file.code.is_empty() || file.name.is_empty() {
            continue;
        }
        locales.push(LocaleInfo {
            code: file.code,
            name: file.name,
        });
    }
    locales.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    // Prefer English first when names collide in order — move `en` to front.
    if let Some(index) = locales.iter().position(|l| l.code == DEFAULT_LOCALE) {
        let en = locales.remove(index);
        locales.insert(0, en);
    }
    locales
}

fn load_settings(path: &Path) -> AppSettings {
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// Application config directory (`settings.json`, `locales/`, Oracle prefs, …).
pub fn config_dir() -> PathBuf {
    ProjectDirs::from("com", "plsqlide", "PL-SQL-IDE")
        .map(|dirs| dirs.config_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".").join("config"))
}

/// Initialize localization (safe to call more than once).
///
/// Creates the config `locales/` directory, seeds `en.json` / `ru.json` when
/// missing, loads `settings.json`, and activates the saved locale (default `en`).
pub fn init() {
    let mut guard = I18N.write().expect("i18n lock");
    if !guard.locales_dir.as_os_str().is_empty() {
        return;
    }
    *guard = I18n::bootstrap(&config_dir());
}

/// Translate `key`. Missing keys fall back to English, then to the key itself.
pub fn t(key: &str) -> String {
    I18N.read().expect("i18n lock").t(key)
}

/// Translate `key` and replace `{name}` placeholders from `args`.
///
/// # Examples
///
/// ```
/// use plsqlide::ui::i18n;
///
/// i18n::init();
/// let msg = i18n::t_args("status.connected", &[("name", "db1")]);
/// assert!(msg.contains("db1"));
/// ```
pub fn t_args(key: &str, args: &[(&str, &str)]) -> String {
    let mut text = t(key);
    for (name, value) in args {
        text = text.replace(&format!("{{{name}}}"), value);
    }
    text
}

/// Currently active locale code (`en`, `ru`, …).
pub fn current_code() -> String {
    I18N.read().expect("i18n lock").current_code.clone()
}

/// Locales discovered in the user `locales/` folder.
pub fn available() -> Vec<LocaleInfo> {
    I18N.read().expect("i18n lock").available.clone()
}

/// Absolute path to the user locales directory (for Appearance hint).
pub fn locales_dir() -> PathBuf {
    I18N.read().expect("i18n lock").locales_dir.clone()
}

/// Re-scan the locales directory (call when opening Preferences).
pub fn reload_available() {
    I18N.write().expect("i18n lock").reload_available();
}

/// Switch active catalog. Returns `false` if the locale file is missing/invalid.
pub fn set_locale(code: &str) -> bool {
    let ok = I18N.write().expect("i18n lock").set_locale(code);
    if ok {
        I18N.read().expect("i18n lock").save_settings();
    }
    ok
}

/// Persist the current locale code to `settings.json`.
pub fn save_settings() {
    I18N.read().expect("i18n lock").save_settings();
}
