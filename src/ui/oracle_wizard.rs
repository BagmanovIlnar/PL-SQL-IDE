//! Oracle Client Wizard: multi-step modal to pick OCI library, TNS_ADMIN, and NLS.
//!
//! Steps: Welcome → OCI library → Network (TNS_ADMIN) → NLS → Finish.
//! On Finish the selected values are applied to
//! [`crate::ui::oracle_prefs::OracleConnectionForm`].

use std::fs;
use std::path::{Path, PathBuf};

use iced::alignment::Vertical;
use iced::border::Border;
use iced::widget::{
    button, column, container, mouse_area, opaque, pick_list, row, scrollable, stack, text, Space,
};
use iced::{Background, Color, Element, Fill, Padding};

use crate::ui::i18n;
use crate::ui::oracle_prefs::{DetectablePath, OracleConnectionForm, OracleEnvVar};
use crate::ui::theme as ui_theme;

const INSTANT_CLIENT_URL: &str =
    "https://www.oracle.com/database/technologies/instant-client/downloads.html";

const FIELD_TEXT: f32 = 12.0;
const LABEL_SIZE: f32 = 11.0;
const TITLE_SIZE: f32 = 14.0;
const BTN_PAD: [u16; 2] = [4, 10];

/// Wizard step index (0 = welcome … 3 = NLS).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WizardStep {
    Welcome,
    OciLibrary,
    Network,
    Nls,
}

impl WizardStep {
    fn next(self) -> Option<Self> {
        match self {
            Self::Welcome => Some(Self::OciLibrary),
            Self::OciLibrary => Some(Self::Network),
            Self::Network => Some(Self::Nls),
            Self::Nls => None,
        }
    }

    fn back(self) -> Option<Self> {
        match self {
            Self::Welcome => None,
            Self::OciLibrary => Some(Self::Welcome),
            Self::Network => Some(Self::OciLibrary),
            Self::Nls => Some(Self::Network),
        }
    }

    fn is_last(self) -> bool {
        matches!(self, Self::Nls)
    }
}

/// Live state of the Oracle Client Wizard modal.
///
/// # Examples
///
/// ```
/// use plsqlide::ui::oracle_wizard::{OracleClientWizard, WizardStep};
///
/// let wizard = OracleClientWizard::new("", "");
/// assert_eq!(wizard.step, WizardStep::Welcome);
/// assert!(wizard.can_go_next());
/// ```
#[derive(Debug, Clone)]
pub struct OracleClientWizard {
    pub step: WizardStep,
    pub oci_candidates: Vec<PathBuf>,
    pub selected_oci: Option<usize>,
    pub tns_candidates: Vec<PathBuf>,
    pub selected_tns: Option<usize>,
    pub tns_aliases: Vec<String>,
    pub nls_language: String,
    pub nls_charset: String,
    pub nls_date_format: String,
}

impl OracleClientWizard {
    /// Build wizard, seeding lists from disk scan and current prefs.
    pub fn new(current_oci: &str, current_tns_admin: &str) -> Self {
        let mut oci_candidates = discover_oci_libraries();
        if !current_oci.is_empty() {
            let path = PathBuf::from(current_oci);
            if path.exists() && !oci_candidates.iter().any(|p| p == &path) {
                oci_candidates.insert(0, path);
            }
        }
        let selected_oci = if oci_candidates.is_empty() {
            None
        } else {
            Some(0)
        };

        let mut tns_candidates = discover_tns_admin_dirs();
        if !current_tns_admin.is_empty() {
            let path = PathBuf::from(current_tns_admin);
            if path.is_dir() && !tns_candidates.iter().any(|p| p == &path) {
                tns_candidates.insert(0, path);
            }
        }
        let selected_tns = if tns_candidates.is_empty() {
            None
        } else {
            Some(0)
        };
        let tns_aliases = selected_tns
            .and_then(|i| tns_candidates.get(i))
            .map(|p| parse_tns_aliases(p))
            .unwrap_or_default();

        Self {
            step: WizardStep::Welcome,
            oci_candidates,
            selected_oci,
            tns_candidates,
            selected_tns,
            tns_aliases,
            nls_language: "AMERICAN_AMERICA".into(),
            nls_charset: "AL32UTF8".into(),
            nls_date_format: "DD-MM-YYYY".into(),
        }
    }

    pub fn can_go_back(&self) -> bool {
        self.step.back().is_some()
    }

    /// Next is allowed on welcome always; on OCI/Network when a selection exists; on NLS use Finish.
    pub fn can_go_next(&self) -> bool {
        match self.step {
            WizardStep::Welcome => true,
            WizardStep::OciLibrary => self.selected_oci.is_some(),
            // TNS_ADMIN is optional (EZCONNECT users may skip).
            WizardStep::Network => true,
            WizardStep::Nls => false,
        }
    }

    pub fn can_finish(&self) -> bool {
        self.step.is_last()
            && self.selected_oci.is_some()
            && !self.nls_language.trim().is_empty()
            && !self.nls_charset.trim().is_empty()
            && !self.nls_date_format.trim().is_empty()
    }

    /// Apply wizard choices onto the Preferences Oracle Connection form.
    pub fn apply_to(&self, form: &mut OracleConnectionForm) {
        if let Some(index) = self.selected_oci {
            if let Some(oci) = self.oci_candidates.get(index) {
                let oci_str = oci.display().to_string();
                form.prefs.oci_library = oci_str.clone();
                if !form.oci_options.iter().any(|o| o.path == oci_str) {
                    form.oci_options.push(DetectablePath::new(oci_str));
                }
                if let Some(home) = infer_oracle_home(oci) {
                    let home_str = home.display().to_string();
                    form.prefs.oracle_home = home_str.clone();
                    if !form.home_options.iter().any(|o| o.path == home_str) {
                        form.home_options.push(DetectablePath::new(home_str));
                    }
                }
            }
        }

        if let Some(index) = self.selected_tns {
            if let Some(tns) = self.tns_candidates.get(index) {
                upsert_env(
                    &mut form.prefs.env_vars,
                    "TNS_ADMIN",
                    &tns.display().to_string(),
                );
            }
        }

        let nls_lang = format!("{}.{}", self.nls_language, self.nls_charset);
        upsert_env(&mut form.prefs.env_vars, "NLS_LANG", &nls_lang);
        upsert_env(
            &mut form.prefs.env_vars,
            "NLS_DATE_FORMAT",
            &self.nls_date_format,
        );

        form.note = Some(i18n::t("prefs.oracle.wizard_applied"));
    }
}

fn upsert_env(vars: &mut Vec<OracleEnvVar>, name: &str, value: &str) {
    if let Some(row) = vars.iter_mut().find(|v| v.name.eq_ignore_ascii_case(name)) {
        row.value = value.to_string();
    } else {
        vars.push(OracleEnvVar {
            name: name.to_string(),
            value: value.to_string(),
        });
    }
}

/// Messages for the wizard modal.
#[derive(Debug, Clone)]
pub enum OracleWizardMessage {
    Cancel,
    Back,
    Next,
    Finish,
    OpenDownloadLink,
    SelectOci(usize),
    SelectTns(usize),
    BrowseOci,
    BrowseTns,
    NlsLanguage(String),
    NlsCharset(String),
    NlsDateFormat(String),
    /// Backdrop click — swallow only; do not close the wizard.
    Backdrop,
}

impl OracleClientWizard {
    /// Handle a wizard message. Returns `true` when the wizard should close
    /// (Cancel, or Finish after apply — caller applies on Finish).
    pub fn update(&mut self, message: OracleWizardMessage) -> WizardOutcome {
        match message {
            OracleWizardMessage::Backdrop => WizardOutcome::Continue,
            OracleWizardMessage::Cancel => WizardOutcome::Close,
            OracleWizardMessage::Back => {
                if let Some(prev) = self.step.back() {
                    self.step = prev;
                }
                WizardOutcome::Continue
            }
            OracleWizardMessage::Next => {
                if self.can_go_next() {
                    if let Some(next) = self.step.next() {
                        self.step = next;
                        if matches!(self.step, WizardStep::Network) {
                            self.refresh_tns_aliases();
                        }
                    }
                }
                WizardOutcome::Continue
            }
            OracleWizardMessage::Finish => {
                if self.can_finish() {
                    WizardOutcome::Finish
                } else {
                    WizardOutcome::Continue
                }
            }
            OracleWizardMessage::OpenDownloadLink => {
                let _ = open::that(INSTANT_CLIENT_URL);
                WizardOutcome::Continue
            }
            OracleWizardMessage::SelectOci(index) => {
                if index < self.oci_candidates.len() {
                    self.selected_oci = Some(index);
                }
                WizardOutcome::Continue
            }
            OracleWizardMessage::SelectTns(index) => {
                if index < self.tns_candidates.len() {
                    self.selected_tns = Some(index);
                    self.refresh_tns_aliases();
                }
                WizardOutcome::Continue
            }
            OracleWizardMessage::BrowseOci => {
                if let Some(path) = pick_oci_file() {
                    if let Some(pos) = self.oci_candidates.iter().position(|p| p == &path) {
                        self.selected_oci = Some(pos);
                    } else {
                        self.oci_candidates.push(path);
                        self.selected_oci = Some(self.oci_candidates.len() - 1);
                    }
                }
                WizardOutcome::Continue
            }
            OracleWizardMessage::BrowseTns => {
                if let Some(path) = pick_tns_dir() {
                    if let Some(pos) = self.tns_candidates.iter().position(|p| p == &path) {
                        self.selected_tns = Some(pos);
                    } else {
                        self.tns_candidates.push(path);
                        self.selected_tns = Some(self.tns_candidates.len() - 1);
                    }
                    self.refresh_tns_aliases();
                }
                WizardOutcome::Continue
            }
            OracleWizardMessage::NlsLanguage(value) => {
                self.nls_language = value;
                WizardOutcome::Continue
            }
            OracleWizardMessage::NlsCharset(value) => {
                self.nls_charset = value;
                WizardOutcome::Continue
            }
            OracleWizardMessage::NlsDateFormat(value) => {
                self.nls_date_format = value;
                WizardOutcome::Continue
            }
        }
    }

    fn refresh_tns_aliases(&mut self) {
        self.tns_aliases = self
            .selected_tns
            .and_then(|i| self.tns_candidates.get(i))
            .map(|p| parse_tns_aliases(p))
            .unwrap_or_default();
    }
}

/// Result of handling a wizard message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WizardOutcome {
    Continue,
    Close,
    Finish,
}

/// Full-screen dimmed overlay with the wizard card.
pub fn oracle_wizard_modal(wizard: &OracleClientWizard) -> Element<'_, OracleWizardMessage> {
    let body = match wizard.step {
        WizardStep::Welcome => welcome_step(),
        WizardStep::OciLibrary => oci_step(wizard),
        WizardStep::Network => network_step(wizard),
        WizardStep::Nls => nls_step(wizard),
    };

    let back_enabled = wizard.can_go_back();
    let next_or_finish: Element<'_, OracleWizardMessage> = if wizard.step.is_last() {
        let enabled = wizard.can_finish();
        let mut btn = button(text(i18n::t("wizard.oracle.finish")).size(FIELD_TEXT))
            .padding(Padding::from(BTN_PAD))
            .style(move |_, status| {
                if enabled {
                    primary_button_style(status)
                } else {
                    disabled_button_style()
                }
            });
        if enabled {
            btn = btn.on_press(OracleWizardMessage::Finish);
        }
        btn.into()
    } else {
        let enabled = wizard.can_go_next();
        let mut btn = button(text(i18n::t("wizard.oracle.next")).size(FIELD_TEXT))
            .padding(Padding::from(BTN_PAD))
            .style(move |_, status| {
                if enabled {
                    primary_button_style(status)
                } else {
                    disabled_button_style()
                }
            });
        if enabled {
            btn = btn.on_press(OracleWizardMessage::Next);
        }
        btn.into()
    };

    let mut back_btn = button(text(i18n::t("wizard.oracle.back")).size(FIELD_TEXT))
        .padding(Padding::from(BTN_PAD))
        .style(move |_, status| {
            if back_enabled {
                chrome_button_style(status)
            } else {
                disabled_button_style()
            }
        });
    if back_enabled {
        back_btn = back_btn.on_press(OracleWizardMessage::Back);
    }

    let footer = row![
        button(text(i18n::t("dialog.common.cancel")).size(FIELD_TEXT))
            .padding(Padding::from(BTN_PAD))
            .style(|_, status| chrome_button_style(status))
            .on_press(OracleWizardMessage::Cancel),
        Space::new().width(Fill),
        back_btn,
        next_or_finish,
    ]
    .spacing(6)
    .padding(Padding::from([8, 12]))
    .align_y(Vertical::Center);

    let card = container(
        column![
            container(text(i18n::t("wizard.oracle.title")).size(13))
                .width(Fill)
                .padding(Padding::from([10, 12]))
                .style(|_| container::Style {
                    background: Some(Background::Color(ui_theme::current().sidebar_title_bg)),
                    text_color: Some(ui_theme::current().text),
                    border: Border {
                        color: ui_theme::current().border,
                        width: 1.0,
                        radius: 0.0.into(),
                    },
                    ..container::Style::default()
                }),
            container(body)
                .width(Fill)
                .height(Fill)
                .padding(Padding::from([12, 14])),
            container(Space::new().height(1).width(Fill)).style(|_| container::Style {
                background: Some(Background::Color(ui_theme::current().border)),
                ..container::Style::default()
            }),
            footer,
        ]
        .width(560)
        .height(420),
    )
    .style(|_| container::Style {
        background: Some(Background::Color(ui_theme::current().ribbon_bg)),
        border: Border {
            color: ui_theme::current().border,
            width: 1.0,
            radius: 4.0.into(),
        },
        text_color: Some(ui_theme::current().text),
        ..container::Style::default()
    });

    // Capture outside clicks so they do not close the wizard or hit Preferences.
    let backdrop = mouse_area(
        container(Space::new().width(Fill).height(Fill))
            .width(Fill)
            .height(Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.45))),
                ..container::Style::default()
            }),
    )
    .on_press(OracleWizardMessage::Backdrop);

    opaque(
        stack![
            backdrop,
            container(card)
                .width(Fill)
                .height(Fill)
                .center_x(Fill)
                .center_y(Fill),
        ]
        .width(Fill)
        .height(Fill),
    )
}

fn welcome_step() -> Element<'static, OracleWizardMessage> {
    column![
        text(i18n::t("wizard.oracle.title")).size(TITLE_SIZE),
        text(i18n::t("wizard.oracle.welcome.intro"))
            .size(FIELD_TEXT),
        text(i18n::t("wizard.oracle.welcome.requires"))
            .size(FIELD_TEXT),
        text(i18n::t("wizard.oracle.welcome.download"))
            .size(FIELD_TEXT),
        button(text(INSTANT_CLIENT_URL).size(LABEL_SIZE))
            .padding(Padding::from([2, 0]))
            .style(|_, status| link_button_style(status))
            .on_press(OracleWizardMessage::OpenDownloadLink),
        text(i18n::t("wizard.oracle.welcome.after_install"))
            .size(FIELD_TEXT),
        Space::new().height(8),
        text(i18n::t("wizard.oracle.welcome.note"))
            .size(LABEL_SIZE)
            .color(ui_theme::current().muted),
    ]
    .spacing(8)
    .width(Fill)
    .into()
}

fn oci_step(wizard: &OracleClientWizard) -> Element<'_, OracleWizardMessage> {
    let mut list = column![].spacing(1);
    if wizard.oci_candidates.is_empty() {
        list = list.push(
            text(i18n::t("wizard.oracle.oci.empty"))
                .size(FIELD_TEXT)
                .color(ui_theme::current().muted),
        );
    } else {
        for (index, path) in wizard.oci_candidates.iter().enumerate() {
            let selected = wizard.selected_oci == Some(index);
            list = list.push(
                button(
                    container(text(path.display().to_string()).size(FIELD_TEXT))
                        .width(Fill)
                        .padding(Padding::from([4, 6])),
                )
                .padding(0)
                .width(Fill)
                .style(move |_, status| list_row_style(selected, status))
                .on_press(OracleWizardMessage::SelectOci(index)),
            );
        }
    }

    column![
        text(i18n::t("wizard.oracle.oci.header")).size(TITLE_SIZE),
        text(i18n::t("wizard.oracle.oci.description")).size(FIELD_TEXT),
        container(scrollable(list).height(Fill))
            .width(Fill)
            .height(200)
            .style(|_| list_box_style()),
        button(text(i18n::t("wizard.oracle.select_other")).size(FIELD_TEXT))
            .padding(Padding::from(BTN_PAD))
            .style(|_, status| chrome_button_style(status))
            .on_press(OracleWizardMessage::BrowseOci),
    ]
    .spacing(8)
    .width(Fill)
    .height(Fill)
    .into()
}

fn network_step(wizard: &OracleClientWizard) -> Element<'_, OracleWizardMessage> {
    let mut list = column![].spacing(1);
    if wizard.tns_candidates.is_empty() {
        list = list.push(
            text(i18n::t("wizard.oracle.tns.empty"))
                .size(FIELD_TEXT)
                .color(ui_theme::current().muted),
        );
    } else {
        for (index, path) in wizard.tns_candidates.iter().enumerate() {
            let selected = wizard.selected_tns == Some(index);
            list = list.push(
                button(
                    container(text(path.display().to_string()).size(FIELD_TEXT))
                        .width(Fill)
                        .padding(Padding::from([4, 6])),
                )
                .padding(0)
                .width(Fill)
                .style(move |_, status| list_row_style(selected, status))
                .on_press(OracleWizardMessage::SelectTns(index)),
            );
        }
    }

    let aliases_block = if let Some(index) = wizard.selected_tns {
        let path = wizard
            .tns_candidates
            .get(index)
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let tns_file = PathBuf::from(&path).join("tnsnames.ora");
        let header = i18n::t_args(
            "wizard.oracle.tns.databases",
            &[("path", &tns_file.display().to_string())],
        );
        let aliases = if wizard.tns_aliases.is_empty() {
            i18n::t("wizard.oracle.tns.no_aliases")
        } else {
            wizard.tns_aliases.join(", ")
        };
        column![
            text(header).size(LABEL_SIZE).color(ui_theme::current().muted),
            text(aliases).size(FIELD_TEXT),
        ]
        .spacing(2)
    } else {
        column![].spacing(0)
    };

    column![
        text(i18n::t("wizard.oracle.tns.header")).size(TITLE_SIZE),
        text(i18n::t("wizard.oracle.tns.description")).size(FIELD_TEXT),
        container(scrollable(list).height(Fill))
            .width(Fill)
            .height(140)
            .style(|_| list_box_style()),
        aliases_block,
        button(text(i18n::t("wizard.oracle.select_other")).size(FIELD_TEXT))
            .padding(Padding::from(BTN_PAD))
            .style(|_, status| chrome_button_style(status))
            .on_press(OracleWizardMessage::BrowseTns),
    ]
    .spacing(8)
    .width(Fill)
    .height(Fill)
    .into()
}

fn nls_step(wizard: &OracleClientWizard) -> Element<'_, OracleWizardMessage> {
    column![
        text(i18n::t("wizard.oracle.nls.header")).size(TITLE_SIZE),
        text(i18n::t("wizard.oracle.nls.description")).size(FIELD_TEXT),
        text(i18n::t("wizard.oracle.nls.language"))
            .size(LABEL_SIZE)
            .color(ui_theme::current().muted),
        pick_list(
            nls_languages(),
            Some(wizard.nls_language.clone()),
            OracleWizardMessage::NlsLanguage,
        )
        .padding(3)
        .text_size(FIELD_TEXT)
        .width(280),
        text(i18n::t("wizard.oracle.nls.charset"))
            .size(LABEL_SIZE)
            .color(ui_theme::current().muted),
        pick_list(
            nls_charsets(),
            Some(wizard.nls_charset.clone()),
            OracleWizardMessage::NlsCharset,
        )
        .padding(3)
        .text_size(FIELD_TEXT)
        .width(280),
        text(i18n::t("wizard.oracle.nls.date_format"))
            .size(LABEL_SIZE)
            .color(ui_theme::current().muted),
        pick_list(
            nls_date_formats(),
            Some(wizard.nls_date_format.clone()),
            OracleWizardMessage::NlsDateFormat,
        )
        .padding(3)
        .text_size(FIELD_TEXT)
        .width(280),
    ]
    .spacing(6)
    .width(Fill)
    .into()
}

fn nls_languages() -> Vec<String> {
    [
        "AMERICAN_AMERICA",
        "RUSSIAN_RUSSIA",
        "GERMAN_GERMANY",
        "FRENCH_FRANCE",
        "ENGLISH_UNITED KINGDOM",
        "BRAZILIAN PORTUGUESE_BRAZIL",
        "SPANISH_SPAIN",
        "JAPANESE_JAPAN",
        "KOREAN_KOREA",
        "SIMPLIFIED CHINESE_CHINA",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn nls_charsets() -> Vec<String> {
    [
        "AL32UTF8",
        "UTF8",
        "WE8MSWIN1252",
        "CL8MSWIN1251",
        "EE8MSWIN1250",
        "WE8ISO8859P1",
        "EE8ISO8859P2",
        "CL8ISO8859P5",
        "JA16SJIS",
        "KO16MSWIN949",
        "ZHS16GBK",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn nls_date_formats() -> Vec<String> {
    [
        "DD-MM-YYYY",
        "DD/MM/YYYY",
        "MM/DD/YYYY",
        "YYYY-MM-DD",
        "DD.MM.YYYY",
        "MON-DD-YYYY",
        "DD-MON-YYYY",
        "YYYY/MM/DD",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn pick_oci_file() -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new().set_title(i18n::t("wizard.oracle.oci.browse_title"));
    #[cfg(target_os = "windows")]
    {
        dialog = dialog.add_filter("OCI Library", &["dll"]);
    }
    #[cfg(target_os = "macos")]
    {
        dialog = dialog.add_filter("OCI Library", &["dylib", "a"]);
    }
    #[cfg(target_os = "linux")]
    {
        dialog = dialog.add_filter("OCI Library", &["so"]);
    }
    dialog.pick_file()
}

fn pick_tns_dir() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title(i18n::t("wizard.oracle.tns.browse_title"))
        .pick_folder()
}

fn oci_library_names() -> &'static [&'static str] {
    #[cfg(target_os = "windows")]
    {
        &["oci.dll"]
    }
    #[cfg(target_os = "macos")]
    {
        &["libclntsh.dylib", "libclntsh.dylib.19.1", "libclntsh.dylib.21.1"]
    }
    #[cfg(target_os = "linux")]
    {
        &[
            "libclntsh.so",
            "libclntsh.so.19.1",
            "libclntsh.so.21.1",
            "libclntsh.so.23.1",
        ]
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        &["libclntsh.so", "oci.dll"]
    }
}

/// Scan common Instant Client / ORACLE_HOME locations for the OCI library.
pub fn discover_oci_libraries() -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut roots = Vec::new();

    if let Ok(home) = std::env::var("ORACLE_HOME") {
        roots.push(PathBuf::from(home));
    }
    if let Ok(path) = std::env::var("PATH") {
        for part in std::env::split_paths(&path) {
            roots.push(part);
        }
    }
    #[cfg(target_os = "macos")]
    {
        roots.push(PathBuf::from("/opt/oracle"));
        roots.push(PathBuf::from("/usr/local/oracle"));
        if let Ok(home) = std::env::var("HOME") {
            roots.push(PathBuf::from(home).join("oracle"));
        }
    }
    #[cfg(target_os = "linux")]
    {
        roots.push(PathBuf::from("/usr/lib/oracle"));
        roots.push(PathBuf::from("/opt/oracle"));
        roots.push(PathBuf::from("/usr/local/oracle"));
    }
    #[cfg(target_os = "windows")]
    {
        roots.push(PathBuf::from(r"C:\oracle"));
        roots.push(PathBuf::from(r"C:\app"));
        if let Ok(program_files) = std::env::var("ProgramFiles") {
            roots.push(PathBuf::from(program_files).join("Oracle"));
        }
    }

    for root in roots {
        collect_oci_under(&root, &mut found, 0);
    }

    found.sort();
    found.dedup();
    found
}

fn collect_oci_under(dir: &Path, out: &mut Vec<PathBuf>, depth: u8) {
    if depth > 4 || !dir.is_dir() {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if oci_library_names()
                    .iter()
                    .any(|n| name.eq_ignore_ascii_case(n) || name.starts_with("libclntsh."))
                {
                    out.push(path);
                }
            }
        } else if path.is_dir() {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if name.contains("instantclient")
                || name.contains("oracle")
                || name == "lib"
                || name == "bin"
                || name.starts_with("client")
            {
                collect_oci_under(&path, out, depth + 1);
            }
        }
    }
}

/// Derive `ORACLE_HOME` from an OCI library path (Instant Client dir or parent of bin/lib).
pub fn infer_oracle_home(oci: &Path) -> Option<PathBuf> {
    let parent = oci.parent()?;
    // Instant Client: library sits in the home itself.
    if parent
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.to_ascii_lowercase().contains("instantclient"))
    {
        return Some(parent.to_path_buf());
    }
    // Classic: ORACLE_HOME/bin/oci.dll or ORACLE_HOME/lib/libclntsh.so
    parent.parent().map(|p| p.to_path_buf())
}

/// Discover likely TNS_ADMIN directories.
pub fn discover_tns_admin_dirs() -> Vec<PathBuf> {
    let mut found = Vec::new();
    if let Ok(tns) = std::env::var("TNS_ADMIN") {
        let path = PathBuf::from(tns);
        if path.is_dir() {
            found.push(path);
        }
    }
    if let Ok(home) = std::env::var("ORACLE_HOME") {
        let network = PathBuf::from(home).join("network").join("admin");
        if network.is_dir() {
            found.push(network);
        }
    }

    let mut roots = Vec::new();
    #[cfg(target_os = "macos")]
    {
        roots.push(PathBuf::from("/opt/oracle"));
        if let Ok(home) = std::env::var("HOME") {
            roots.push(PathBuf::from(home).join("oracle"));
        }
    }
    #[cfg(target_os = "linux")]
    {
        roots.push(PathBuf::from("/opt/oracle"));
        roots.push(PathBuf::from("/etc"));
        roots.push(PathBuf::from("/usr/lib/oracle"));
    }
    #[cfg(target_os = "windows")]
    {
        roots.push(PathBuf::from(r"C:\oracle"));
        roots.push(PathBuf::from(r"C:\app"));
    }

    for root in roots {
        collect_tns_under(&root, &mut found, 0);
    }

    found.sort();
    found.dedup();
    found
}

fn collect_tns_under(dir: &Path, out: &mut Vec<PathBuf>, depth: u8) {
    if depth > 5 || !dir.is_dir() {
        return;
    }
    let tnsnames = dir.join("tnsnames.ora");
    if tnsnames.is_file() {
        out.push(dir.to_path_buf());
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if name == "network"
            || name == "admin"
            || name.contains("oracle")
            || name.contains("instantclient")
        {
            collect_tns_under(&path, out, depth + 1);
        }
    }
}

/// Parse alias names from a `tnsnames.ora` directory or file path.
///
/// # Examples
///
/// ```
/// use std::io::Write;
/// use plsqlide::ui::oracle_wizard::parse_tns_aliases;
///
/// let dir = std::env::temp_dir().join("plsqlide_tns_test");
/// let _ = std::fs::create_dir_all(&dir);
/// let file = dir.join("tnsnames.ora");
/// let mut f = std::fs::File::create(&file).unwrap();
/// writeln!(f, "med2dev =\n  (DESCRIPTION = (ADDRESS = (PROTOCOL = TCP)))").unwrap();
/// writeln!(f, "mis2201=(DESCRIPTION=(ADDRESS=(PROTOCOL=TCP)))").unwrap();
/// let aliases = parse_tns_aliases(&dir);
/// assert!(aliases.iter().any(|a| a == "med2dev"));
/// assert!(aliases.iter().any(|a| a == "mis2201"));
/// let _ = std::fs::remove_dir_all(&dir);
/// ```
pub fn parse_tns_aliases(tns_admin: &Path) -> Vec<String> {
    let file = if tns_admin.is_file() {
        tns_admin.to_path_buf()
    } else {
        tns_admin.join("tnsnames.ora")
    };
    let Ok(raw) = fs::read_to_string(file) else {
        return Vec::new();
    };
    let mut aliases = Vec::new();
    let mut depth = 0i32;
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let open = trimmed.chars().filter(|c| *c == '(').count() as i32;
        let close = trimmed.chars().filter(|c| *c == ')').count() as i32;
        if depth == 0 {
            if let Some(name) = trimmed.split('=').next() {
                let name = name.trim();
                if !name.is_empty()
                    && name
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
                    && !name.eq_ignore_ascii_case("DESCRIPTION")
                    && !name.eq_ignore_ascii_case("ADDRESS")
                    && !name.eq_ignore_ascii_case("CONNECT_DATA")
                {
                    aliases.push(name.to_string());
                }
            }
        }
        depth = (depth + open - close).max(0);
    }
    aliases.sort();
    aliases.dedup();
    aliases
}

fn list_box_style() -> container::Style {
    container::Style {
        background: Some(Background::Color(ui_theme::current().panel_bg)),
        border: Border {
            color: ui_theme::current().border,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..container::Style::default()
    }
}

fn list_row_style(
    selected: bool,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    let background = if selected {
        Some(Background::Color(ui_theme::current().accent))
    } else if matches!(
        status,
        iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed
    ) {
        Some(Background::Color(ui_theme::current().hover))
    } else {
        Some(Background::Color(ui_theme::current().panel_bg))
    };
    let text_color = if selected {
        Color::WHITE
    } else {
        ui_theme::current().text
    };
    iced::widget::button::Style {
        background,
        text_color,
        border: Border::default(),
        ..iced::widget::button::Style::default()
    }
}

fn chrome_button_style(status: iced::widget::button::Status) -> iced::widget::button::Style {
    let background = match status {
        iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed => {
            Some(Background::Color(ui_theme::current().hover))
        }
        _ => Some(Background::Color(ui_theme::current().chrome_bg)),
    };
    iced::widget::button::Style {
        background,
        text_color: ui_theme::current().text,
        border: Border {
            color: ui_theme::current().border,
            width: 1.0,
            radius: 2.0.into(),
        },
        ..iced::widget::button::Style::default()
    }
}

fn primary_button_style(status: iced::widget::button::Status) -> iced::widget::button::Style {
    let background = match status {
        iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed => {
            Color::from_rgb(0.25, 0.52, 0.85)
        }
        _ => ui_theme::current().accent,
    };
    iced::widget::button::Style {
        background: Some(Background::Color(background)),
        text_color: Color::WHITE,
        border: Border {
            color: background,
            width: 1.0,
            radius: 2.0.into(),
        },
        ..iced::widget::button::Style::default()
    }
}

fn disabled_button_style() -> iced::widget::button::Style {
    iced::widget::button::Style {
        background: Some(Background::Color(ui_theme::current().chrome_bg)),
        text_color: ui_theme::current().muted,
        border: Border {
            color: ui_theme::current().border,
            width: 1.0,
            radius: 2.0.into(),
        },
        ..iced::widget::button::Style::default()
    }
}

fn link_button_style(status: iced::widget::button::Status) -> iced::widget::button::Style {
    let text_color = match status {
        iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed => {
            Color::from_rgb(0.15, 0.35, 0.85)
        }
        _ => ui_theme::current().accent,
    };
    iced::widget::button::Style {
        background: None,
        text_color,
        border: Border::default(),
        ..iced::widget::button::Style::default()
    }
}
