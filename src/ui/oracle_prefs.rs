//! Preferences → Oracle → Connection form UI.
//!
//! Draft values while Preferences is open; persistence lives in
//! [`crate::core::oracle_connection_prefs`] and is applied on Connect.

use iced::alignment::Vertical;
use iced::border::Border;
use iced::widget::{
    button, column, container, pick_list, row, scrollable, text, text_input, Space,
};
use iced::{Background, Color, Element, Fill, Length, Padding};

use crate::core::oracle_connection_prefs::{self as prefs_store};
pub use crate::core::{OracleConnectionPrefs, OracleEnvVar, OracleSessionMode};
use crate::ui::i18n;
use crate::ui::oracle_wizard::{
    discover_oci_libraries, infer_oracle_home, OracleClientWizard, OracleWizardMessage,
    WizardOutcome,
};
use crate::ui::theme as ui_theme;

impl OracleSessionMode {
    /// All modes in combo order.
    pub fn all() -> [Self; 3] {
        [Self::MultiSession, Self::DualSession, Self::SingleSession]
    }

    /// i18n key for the combo label.
    pub fn key(self) -> &'static str {
        match self {
            Self::MultiSession => "prefs.oracle.session.multi",
            Self::DualSession => "prefs.oracle.session.dual",
            Self::SingleSession => "prefs.oracle.session.single",
        }
    }
}

impl std::fmt::Display for OracleSessionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&i18n::t(self.key()))
    }
}

/// Path combo entry: empty string means autodetect.
///
/// # Examples
///
/// ```
/// use plsqlide::ui::i18n;
/// use plsqlide::ui::oracle_prefs::DetectablePath;
///
/// i18n::init();
/// let auto = DetectablePath::autodetect();
/// assert!(auto.path.is_empty());
/// assert!(!auto.to_string().is_empty());
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectablePath {
    pub path: String,
}

impl DetectablePath {
    /// Empty path = autodetect.
    pub fn autodetect() -> Self {
        Self {
            path: String::new(),
        }
    }

    pub fn new(path: impl Into<String>) -> Self {
        Self { path: path.into() }
    }
}

impl std::fmt::Display for DetectablePath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.path.is_empty() {
            f.write_str(&i18n::t("prefs.oracle.autodetect"))
        } else {
            f.write_str(&self.path)
        }
    }
}

/// Load Oracle connection prefs from disk (or defaults).
pub fn load() -> OracleConnectionPrefs {
    prefs_store::load()
}

/// Persist Oracle connection prefs.
pub fn save(prefs: &OracleConnectionPrefs) {
    prefs_store::save(prefs);
}

/// Draft editor for Add / Edit environment variable.
#[derive(Debug, Clone)]
pub struct EnvVarDraft {
    /// `None` = adding a new row; `Some(i)` = editing `env_vars[i]`.
    pub index: Option<usize>,
    pub name: String,
    pub value: String,
}

impl EnvVarDraft {
    pub fn add() -> Self {
        Self {
            index: None,
            name: String::new(),
            value: String::new(),
        }
    }

    pub fn edit(index: usize, var: &OracleEnvVar) -> Self {
        Self {
            index: Some(index),
            name: var.name.clone(),
            value: var.value.clone(),
        }
    }
}

/// Live form state for Preferences → Oracle → Connection.
#[derive(Debug, Clone)]
pub struct OracleConnectionForm {
    pub prefs: OracleConnectionPrefs,
    /// Options for the Oracle Home combo (plus autodetect).
    pub home_options: Vec<DetectablePath>,
    /// Options for the OCI library combo (plus autodetect).
    pub oci_options: Vec<DetectablePath>,
    pub selected_env: Option<usize>,
    pub env_editor: Option<EnvVarDraft>,
    /// Open Oracle Client Wizard (`None` when closed).
    pub wizard: Option<OracleClientWizard>,
    /// Short status note under the form.
    pub note: Option<String>,
}

impl OracleConnectionForm {
    /// Load from disk and seed combo options from auto-discovery + saved values.
    pub fn load() -> Self {
        let prefs = load();
        let mut home_options = vec![DetectablePath::autodetect()];
        let mut oci_options = vec![DetectablePath::autodetect()];

        // Scan common Instant Client / ORACLE_HOME locations.
        for oci in discover_oci_libraries() {
            push_unique_path(&mut oci_options, oci.display().to_string());
            if let Some(home) = infer_oracle_home(&oci) {
                push_unique_path(&mut home_options, home.display().to_string());
            }
        }
        if let Ok(home) = std::env::var("ORACLE_HOME") {
            if std::path::Path::new(&home).is_dir() {
                push_unique_path(&mut home_options, home);
            }
        }

        // Keep previously saved choices even if the path was not rediscovered.
        push_unique_path(&mut home_options, prefs.oracle_home.clone());
        push_unique_path(&mut oci_options, prefs.oci_library.clone());

        Self {
            prefs,
            home_options,
            oci_options,
            selected_env: None,
            env_editor: None,
            wizard: None,
            note: None,
        }
    }

    fn current_tns_admin(&self) -> String {
        self.prefs
            .env_vars
            .iter()
            .find(|v| v.name.eq_ignore_ascii_case("TNS_ADMIN"))
            .map(|v| v.value.clone())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        save(&self.prefs);
    }

    fn selected_home(&self) -> DetectablePath {
        DetectablePath::new(self.prefs.oracle_home.clone())
    }

    fn selected_oci(&self) -> DetectablePath {
        DetectablePath::new(self.prefs.oci_library.clone())
    }
}

/// Messages for the Oracle Connection page.
#[derive(Debug, Clone)]
pub enum OracleConnectionMessage {
    SessionModeChanged(OracleSessionMode),
    OracleHomeChanged(DetectablePath),
    OciLibraryChanged(DetectablePath),
    /// Open the Oracle Client Wizard modal.
    OpenClientWizard,
    /// Native file picker for OCI library path.
    LocateOciFile,
    /// Messages from the nested Oracle Client Wizard.
    Wizard(OracleWizardMessage),
    EnvSelect(usize),
    EnvAdd,
    EnvEdit,
    EnvDelete,
    EnvEditorName(String),
    EnvEditorValue(String),
    EnvEditorOk,
    EnvEditorCancel,
}

impl OracleConnectionForm {
    /// Apply a page message; never closes Preferences.
    pub fn update(&mut self, message: OracleConnectionMessage) {
        match message {
            OracleConnectionMessage::SessionModeChanged(mode) => {
                self.prefs.session_mode = mode;
            }
            OracleConnectionMessage::OracleHomeChanged(choice) => {
                self.prefs.oracle_home = choice.path;
            }
            OracleConnectionMessage::OciLibraryChanged(choice) => {
                self.prefs.oci_library = choice.path;
            }
            OracleConnectionMessage::OpenClientWizard => {
                self.note = None;
                self.wizard = Some(OracleClientWizard::new(
                    &self.prefs.oci_library,
                    &self.current_tns_admin(),
                ));
            }
            OracleConnectionMessage::LocateOciFile => {
                if let Some(path) = pick_oci_library_file() {
                    let path_str = path.display().to_string();
                    self.prefs.oci_library = path_str.clone();
                    if !self.oci_options.iter().any(|o| o.path == path_str) {
                        self.oci_options.push(DetectablePath::new(path_str));
                    }
                    self.note = None;
                }
            }
            OracleConnectionMessage::Wizard(message) => match self
                .wizard
                .as_mut()
                .map(|w| w.update(message))
                .unwrap_or(WizardOutcome::Close)
            {
                WizardOutcome::Continue => {}
                WizardOutcome::Close => {
                    self.wizard = None;
                }
                WizardOutcome::Finish => {
                    if let Some(wizard) = self.wizard.take() {
                        wizard.apply_to(self);
                    }
                }
            },
            OracleConnectionMessage::EnvSelect(index) => {
                self.selected_env = Some(index);
            }
            OracleConnectionMessage::EnvAdd => {
                self.env_editor = Some(EnvVarDraft::add());
            }
            OracleConnectionMessage::EnvEdit => {
                if let Some(index) = self.selected_env {
                    if let Some(var) = self.prefs.env_vars.get(index) {
                        self.env_editor = Some(EnvVarDraft::edit(index, var));
                    }
                }
            }
            OracleConnectionMessage::EnvDelete => {
                if let Some(index) = self.selected_env {
                    if index < self.prefs.env_vars.len() {
                        self.prefs.env_vars.remove(index);
                        self.selected_env = None;
                    }
                }
                self.env_editor = None;
            }
            OracleConnectionMessage::EnvEditorName(name) => {
                if let Some(draft) = &mut self.env_editor {
                    draft.name = name;
                }
            }
            OracleConnectionMessage::EnvEditorValue(value) => {
                if let Some(draft) = &mut self.env_editor {
                    draft.value = value;
                }
            }
            OracleConnectionMessage::EnvEditorCancel => {
                self.env_editor = None;
            }
            OracleConnectionMessage::EnvEditorOk => {
                if let Some(draft) = self.env_editor.take() {
                    let name = draft.name.trim().to_string();
                    if name.is_empty() {
                        self.env_editor = Some(draft);
                        return;
                    }
                    let row = OracleEnvVar {
                        name,
                        value: draft.value,
                    };
                    match draft.index {
                        Some(index) if index < self.prefs.env_vars.len() => {
                            self.prefs.env_vars[index] = row;
                            self.selected_env = Some(index);
                        }
                        _ => {
                            self.prefs.env_vars.push(row);
                            self.selected_env = Some(self.prefs.env_vars.len() - 1);
                        }
                    }
                }
            }
        }
    }
}

fn push_unique_path(options: &mut Vec<DetectablePath>, path: String) {
    if path.is_empty() {
        return;
    }
    if !options.iter().any(|item| item.path == path) {
        options.push(DetectablePath::new(path));
    }
}

fn pick_oci_library_file() -> Option<std::path::PathBuf> {
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

/// Compact control metrics for Preferences forms.
const LABEL_SIZE: f32 = 11.0;
const FIELD_TEXT: f32 = 12.0;
const TITLE_SIZE: f32 = 13.0;
const FIELD_PAD: f32 = 3.0;
const BTN_PAD: [u16; 2] = [3, 8];

/// Right-hand Oracle → Connection settings form.
pub fn oracle_connection_page(form: &OracleConnectionForm) -> Element<'_, OracleConnectionMessage> {
    let session = column![
        text(i18n::t("prefs.oracle.session_mode"))
            .size(LABEL_SIZE)
            .color(ui_theme::current().muted),
        pick_list(
            OracleSessionMode::all(),
            Some(form.prefs.session_mode),
            OracleConnectionMessage::SessionModeChanged,
        )
        .padding(FIELD_PAD)
        .text_size(FIELD_TEXT)
        .width(200),
    ]
    .spacing(2);

    let home = column![
        text(i18n::t("prefs.oracle.home"))
            .size(LABEL_SIZE)
            .color(ui_theme::current().muted),
        row![
            pick_list(
                form.home_options.clone(),
                Some(form.selected_home()),
                OracleConnectionMessage::OracleHomeChanged,
            )
            .padding(FIELD_PAD)
            .text_size(FIELD_TEXT)
            .width(Fill),
            button(text(i18n::t("prefs.oracle.client_wizard")).size(FIELD_TEXT))
                .padding(Padding::from(BTN_PAD))
                .style(|_, status| chrome_button_style(status))
                .on_press(OracleConnectionMessage::OpenClientWizard),
        ]
        .spacing(6)
        .align_y(Vertical::Center),
    ]
    .spacing(2);

    let oci = column![
        text(i18n::t("prefs.oracle.oci_library"))
            .size(LABEL_SIZE)
            .color(ui_theme::current().muted),
        row![
            pick_list(
                form.oci_options.clone(),
                Some(form.selected_oci()),
                OracleConnectionMessage::OciLibraryChanged,
            )
            .padding(FIELD_PAD)
            .text_size(FIELD_TEXT)
            .width(Fill),
            button(text(i18n::t("prefs.oracle.locate_file")).size(FIELD_TEXT))
                .padding(Padding::from(BTN_PAD))
                .style(|_, status| chrome_button_style(status))
                .on_press(OracleConnectionMessage::LocateOciFile),
        ]
        .spacing(6)
        .align_y(Vertical::Center),
    ]
    .spacing(2);

    let env_header = row![
        text(i18n::t("prefs.oracle.env_vars")).size(FIELD_TEXT),
        Space::new().width(Fill),
        env_tool_button(i18n::t("prefs.oracle.env_add"), OracleConnectionMessage::EnvAdd),
        env_tool_button(i18n::t("prefs.oracle.env_edit"), OracleConnectionMessage::EnvEdit),
        env_tool_button(
            i18n::t("prefs.oracle.env_delete"),
            OracleConnectionMessage::EnvDelete,
        ),
    ]
    .spacing(4)
    .align_y(Vertical::Center);

    let mut env_rows = column![].spacing(0);
    env_rows = env_rows.push(
        row![
            env_header_cell(i18n::t("prefs.oracle.env_name"), 120.0),
            env_header_cell(i18n::t("prefs.oracle.env_value"), Fill),
        ]
        .spacing(1),
    );
    for (index, var) in form.prefs.env_vars.iter().enumerate() {
        let selected = form.selected_env == Some(index);
        env_rows = env_rows.push(
            button(
                row![
                    container(text(var.name.clone()).size(FIELD_TEXT))
                        .width(120)
                        .padding(Padding::from([2, 4])),
                    container(text(var.value.clone()).size(FIELD_TEXT))
                        .width(Fill)
                        .padding(Padding::from([2, 4])),
                ]
                .spacing(1),
            )
            .padding(0)
            .width(Fill)
            .style(move |_, status| env_row_style(selected, status))
            .on_press(OracleConnectionMessage::EnvSelect(index)),
        );
    }

    let env_grid = container(scrollable(env_rows).height(Fill))
        .width(Fill)
        .height(110)
        .style(|_| container::Style {
            background: Some(Background::Color(ui_theme::current().panel_bg)),
            border: Border {
                color: ui_theme::current().border,
                width: 1.0,
                radius: 0.0.into(),
            },
            ..container::Style::default()
        });

    let mut body = column![
        text(i18n::t("prefs.page.connection")).size(TITLE_SIZE),
        session,
        home,
        oci,
        env_header,
        env_grid,
    ]
    .spacing(6)
    .width(Fill);

    if let Some(draft) = &form.env_editor {
        body = body.push(env_editor(draft));
    }

    if let Some(note) = &form.note {
        body = body.push(
            text(note.clone())
                .size(LABEL_SIZE)
                .color(ui_theme::current().muted),
        );
    }

    scrollable(body).height(Fill).into()
}

fn env_editor(draft: &EnvVarDraft) -> Element<'_, OracleConnectionMessage> {
    let title = if draft.index.is_some() {
        i18n::t("prefs.oracle.env_edit_title")
    } else {
        i18n::t("prefs.oracle.env_add_title")
    };
    container(
        column![
            text(title).size(FIELD_TEXT),
            text(i18n::t("prefs.oracle.env_name"))
                .size(LABEL_SIZE)
                .color(ui_theme::current().muted),
            text_input("", &draft.name)
                .on_input(OracleConnectionMessage::EnvEditorName)
                .size(FIELD_TEXT)
                .padding(FIELD_PAD),
            text(i18n::t("prefs.oracle.env_value"))
                .size(LABEL_SIZE)
                .color(ui_theme::current().muted),
            text_input("", &draft.value)
                .on_input(OracleConnectionMessage::EnvEditorValue)
                .size(FIELD_TEXT)
                .padding(FIELD_PAD),
            row![
                button(text(i18n::t("dialog.common.cancel")).size(FIELD_TEXT))
                    .padding(Padding::from(BTN_PAD))
                    .style(|_, status| chrome_button_style(status))
                    .on_press(OracleConnectionMessage::EnvEditorCancel),
                Space::new().width(Fill),
                button(text(i18n::t("dialog.common.ok")).size(FIELD_TEXT))
                    .padding(Padding::from(BTN_PAD))
                    .style(|_, status| primary_button_style(status))
                    .on_press(OracleConnectionMessage::EnvEditorOk),
            ]
            .spacing(6),
        ]
        .spacing(3)
        .padding(6),
    )
    .width(Fill)
    .style(|_| container::Style {
        background: Some(Background::Color(ui_theme::current().ribbon_bg)),
        border: Border {
            color: ui_theme::current().border,
            width: 1.0,
            radius: 2.0.into(),
        },
        ..container::Style::default()
    })
    .into()
}

fn env_tool_button(
    label: String,
    message: OracleConnectionMessage,
) -> Element<'static, OracleConnectionMessage> {
    button(text(label).size(LABEL_SIZE))
        .padding(Padding::from(BTN_PAD))
        .style(|_, status| chrome_button_style(status))
        .on_press(message)
        .into()
}

fn env_header_cell(
    label: String,
    width: impl Into<Length>,
) -> Element<'static, OracleConnectionMessage> {
    container(text(label).size(LABEL_SIZE).color(ui_theme::current().muted))
        .width(width)
        .padding(Padding::from([2, 4]))
        .style(|_| container::Style {
            background: Some(Background::Color(ui_theme::current().sidebar_title_bg)),
            ..container::Style::default()
        })
        .into()
}

fn env_row_style(
    selected: bool,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    let background = if selected {
        Some(Background::Color(ui_theme::current().hover))
    } else if matches!(
        status,
        iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed
    ) {
        Some(Background::Color(ui_theme::current().hover))
    } else {
        Some(Background::Color(ui_theme::current().panel_bg))
    };
    iced::widget::button::Style {
        background,
        text_color: ui_theme::current().text,
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
