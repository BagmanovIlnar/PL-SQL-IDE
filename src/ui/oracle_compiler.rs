//! Preferences → Oracle → Compiler page.
//!
//! Draft values while Preferences is open; persistence lives in
//! [`crate::core::oracle_compiler_prefs`] and is applied via
//! `ALTER SESSION` when working with the database.

use iced::alignment::{Horizontal, Vertical};
use iced::border::Border;
use iced::widget::{
    button, column, container, mouse_area, opaque, pick_list, row, scrollable, stack, text,
    text_input, tooltip, Space,
};
use iced::{Background, Color, Element, Fill, Length, Padding};
use iced_fonts::lucide::circle_help;

use crate::core::oracle_compiler_prefs::{
    self as store, NameValuePair, OracleCompilerPrefs, PlscopeIdentifiers, PlscopeStatements,
    PlsqlCodeType, PlsqlWarningAction, PlsqlWarningEntry,
};
use crate::ui::i18n;
use crate::ui::theme as ui_theme;

const LABEL_SIZE: f32 = 11.0;
const FIELD_TEXT: f32 = 12.0;
const TITLE_SIZE: f32 = 13.0;
const FIELD_PAD: u16 = 3;
const BTN_PAD: [u16; 2] = [4, 8];
const GRID_HEIGHT: f32 = 90.0;

/// Which name/value grid is being edited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompilerGrid {
    CcFlags,
    Substitution,
}

/// Draft for Name / Value editors (CC flags, substitution vars).
#[derive(Debug, Clone)]
pub struct NameValueDraft {
    pub grid: CompilerGrid,
    pub index: Option<usize>,
    pub name: String,
    pub value: String,
}

/// Draft for the PL/SQL Warnings editor.
#[derive(Debug, Clone)]
pub struct WarningDraft {
    pub index: Option<usize>,
    pub action: PlsqlWarningAction,
    pub exception: String,
}

/// Draft Oracle → Compiler form (saved with Preferences OK).
///
/// # Examples
///
/// ```
/// use plsqlide::ui::oracle_compiler::OracleCompilerForm;
///
/// let form = OracleCompilerForm::load();
/// assert_eq!(form.prefs.optimize_level, 2);
/// ```
#[derive(Debug, Clone)]
pub struct OracleCompilerForm {
    pub prefs: OracleCompilerPrefs,
    pub selected_cc: Option<usize>,
    pub selected_warning: Option<usize>,
    pub selected_subst: Option<usize>,
    pub name_value_editor: Option<NameValueDraft>,
    pub warning_editor: Option<WarningDraft>,
}

impl OracleCompilerForm {
    pub fn load() -> Self {
        Self {
            prefs: store::load(),
            selected_cc: None,
            selected_warning: None,
            selected_subst: None,
            name_value_editor: None,
            warning_editor: None,
        }
    }

    pub fn save(&self) {
        store::save(&self.prefs);
    }

    pub fn update(&mut self, message: OracleCompilerMessage) {
        match message {
            OracleCompilerMessage::OptimizeInc => {
                if self.prefs.optimize_level < 3 {
                    self.prefs.optimize_level += 1;
                }
            }
            OracleCompilerMessage::OptimizeDec => {
                if self.prefs.optimize_level > 0 {
                    self.prefs.optimize_level -= 1;
                }
            }
            OracleCompilerMessage::OptimizeInput(raw) => {
                let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
                if digits.is_empty() {
                    return;
                }
                if let Ok(n) = digits.parse::<u8>() {
                    self.prefs.optimize_level = OracleCompilerPrefs::clamp_optimize_level(n);
                }
            }
            OracleCompilerMessage::CodeType(value) => {
                self.prefs.code_type = value;
            }
            OracleCompilerMessage::PlscopeIdentifiers(value) => {
                self.prefs.plscope_identifiers = value;
            }
            OracleCompilerMessage::PlscopeStatements(value) => {
                self.prefs.plscope_statements = value;
            }
            OracleCompilerMessage::CcSelect(index) => {
                self.selected_cc = Some(index);
            }
            OracleCompilerMessage::CcAdd => {
                self.warning_editor = None;
                self.name_value_editor = Some(NameValueDraft {
                    grid: CompilerGrid::CcFlags,
                    index: None,
                    name: String::new(),
                    value: String::new(),
                });
            }
            OracleCompilerMessage::CcEdit => {
                if let Some(index) = self.selected_cc {
                    if let Some(row) = self.prefs.conditional_compilation_flags.get(index) {
                        self.warning_editor = None;
                        self.name_value_editor = Some(NameValueDraft {
                            grid: CompilerGrid::CcFlags,
                            index: Some(index),
                            name: row.name.clone(),
                            value: row.value.clone(),
                        });
                    }
                }
            }
            OracleCompilerMessage::CcDelete => {
                if let Some(index) = self.selected_cc {
                    if index < self.prefs.conditional_compilation_flags.len() {
                        self.prefs.conditional_compilation_flags.remove(index);
                        self.selected_cc = None;
                    }
                }
            }
            OracleCompilerMessage::WarningSelect(index) => {
                self.selected_warning = Some(index);
            }
            OracleCompilerMessage::WarningAdd => {
                self.name_value_editor = None;
                self.warning_editor = Some(WarningDraft {
                    index: None,
                    action: PlsqlWarningAction::Disabled,
                    exception: String::new(),
                });
            }
            OracleCompilerMessage::WarningEdit => {
                if let Some(index) = self.selected_warning {
                    if let Some(row) = self.prefs.warnings.get(index) {
                        self.name_value_editor = None;
                        self.warning_editor = Some(WarningDraft {
                            index: Some(index),
                            action: row.action,
                            exception: row.exception.clone(),
                        });
                    }
                }
            }
            OracleCompilerMessage::WarningDelete => {
                if let Some(index) = self.selected_warning {
                    if index < self.prefs.warnings.len() {
                        self.prefs.warnings.remove(index);
                        self.selected_warning = None;
                    }
                }
            }
            OracleCompilerMessage::SubstSelect(index) => {
                self.selected_subst = Some(index);
            }
            OracleCompilerMessage::SubstAdd => {
                self.warning_editor = None;
                self.name_value_editor = Some(NameValueDraft {
                    grid: CompilerGrid::Substitution,
                    index: None,
                    name: String::new(),
                    value: String::new(),
                });
            }
            OracleCompilerMessage::SubstEdit => {
                if let Some(index) = self.selected_subst {
                    if let Some(row) = self.prefs.substitution_variables.get(index) {
                        self.warning_editor = None;
                        self.name_value_editor = Some(NameValueDraft {
                            grid: CompilerGrid::Substitution,
                            index: Some(index),
                            name: row.name.clone(),
                            value: row.value.clone(),
                        });
                    }
                }
            }
            OracleCompilerMessage::SubstDelete => {
                if let Some(index) = self.selected_subst {
                    if index < self.prefs.substitution_variables.len() {
                        self.prefs.substitution_variables.remove(index);
                        self.selected_subst = None;
                    }
                }
            }
            OracleCompilerMessage::NameValueName(name) => {
                if let Some(draft) = &mut self.name_value_editor {
                    draft.name = name;
                }
            }
            OracleCompilerMessage::NameValueValue(value) => {
                if let Some(draft) = &mut self.name_value_editor {
                    draft.value = value;
                }
            }
            OracleCompilerMessage::NameValueCancel => {
                self.name_value_editor = None;
            }
            OracleCompilerMessage::NameValueOk => {
                if let Some(draft) = self.name_value_editor.take() {
                    let name = draft.name.trim().to_string();
                    if name.is_empty() {
                        return;
                    }
                    let pair = NameValuePair {
                        name,
                        value: draft.value,
                    };
                    let list = match draft.grid {
                        CompilerGrid::CcFlags => &mut self.prefs.conditional_compilation_flags,
                        CompilerGrid::Substitution => &mut self.prefs.substitution_variables,
                    };
                    if let Some(index) = draft.index {
                        if let Some(slot) = list.get_mut(index) {
                            *slot = pair;
                        }
                    } else {
                        list.push(pair);
                    }
                }
            }
            OracleCompilerMessage::WarningAction(action) => {
                if let Some(draft) = &mut self.warning_editor {
                    draft.action = action;
                }
            }
            OracleCompilerMessage::WarningException(exception) => {
                if let Some(draft) = &mut self.warning_editor {
                    draft.exception = exception;
                }
            }
            OracleCompilerMessage::WarningEditorCancel => {
                self.warning_editor = None;
            }
            OracleCompilerMessage::WarningEditorOk => {
                if let Some(draft) = self.warning_editor.take() {
                    let exception = draft.exception.trim().to_string();
                    if exception.is_empty() {
                        return;
                    }
                    let entry = PlsqlWarningEntry {
                        action: draft.action,
                        exception,
                    };
                    if let Some(index) = draft.index {
                        if let Some(slot) = self.prefs.warnings.get_mut(index) {
                            *slot = entry;
                        }
                    } else {
                        self.prefs.warnings.push(entry);
                    }
                }
            }
        }
    }
}

/// Messages for the Oracle Compiler page.
#[derive(Debug, Clone)]
pub enum OracleCompilerMessage {
    OptimizeInc,
    OptimizeDec,
    OptimizeInput(String),
    CodeType(PlsqlCodeType),
    PlscopeIdentifiers(PlscopeIdentifiers),
    PlscopeStatements(PlscopeStatements),
    CcSelect(usize),
    CcAdd,
    CcEdit,
    CcDelete,
    WarningSelect(usize),
    WarningAdd,
    WarningEdit,
    WarningDelete,
    SubstSelect(usize),
    SubstAdd,
    SubstEdit,
    SubstDelete,
    NameValueName(String),
    NameValueValue(String),
    NameValueCancel,
    NameValueOk,
    WarningAction(PlsqlWarningAction),
    WarningException(String),
    WarningEditorCancel,
    WarningEditorOk,
}

impl PlsqlCodeType {
    fn label(self) -> String {
        i18n::t(match self {
            Self::Interpreted => "prefs.oracle.compiler.code_type.interpreted",
            Self::Native => "prefs.oracle.compiler.code_type.native",
        })
    }
}

impl std::fmt::Display for PlsqlCodeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label())
    }
}

impl PlscopeIdentifiers {
    fn label(self) -> String {
        i18n::t(match self {
            Self::None => "prefs.oracle.compiler.plscope.none",
            Self::All => "prefs.oracle.compiler.plscope.all",
            Self::Default => "prefs.oracle.compiler.plscope.default",
            Self::Public => "prefs.oracle.compiler.plscope.public",
            Self::Sql => "prefs.oracle.compiler.plscope.sql",
            Self::Plsql => "prefs.oracle.compiler.plscope.plsql",
        })
    }
}

impl std::fmt::Display for PlscopeIdentifiers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label())
    }
}

impl PlscopeStatements {
    fn label(self) -> String {
        i18n::t(match self {
            Self::None => "prefs.oracle.compiler.plscope.none",
            Self::All => "prefs.oracle.compiler.plscope.all",
            Self::Default => "prefs.oracle.compiler.plscope.default",
        })
    }
}

impl std::fmt::Display for PlscopeStatements {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label())
    }
}

impl PlsqlWarningAction {
    fn label(self) -> String {
        i18n::t(match self {
            Self::Disabled => "prefs.oracle.compiler.warning.disabled",
            Self::Warning => "prefs.oracle.compiler.warning.warning",
            Self::Error => "prefs.oracle.compiler.warning.error",
        })
    }
}

impl std::fmt::Display for PlsqlWarningAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label())
    }
}

/// Right-hand Oracle → Compiler settings form.
pub fn oracle_compiler_page(form: &OracleCompilerForm) -> Element<'_, OracleCompilerMessage> {
    let body = column![
        text(i18n::t("prefs.page.compiler")).size(TITLE_SIZE),
        optimize_level_row(form.prefs.optimize_level),
        labeled_combo(
            i18n::t("prefs.oracle.compiler.code_type"),
            i18n::t("prefs.oracle.compiler.code_type_tip"),
            PlsqlCodeType::all(),
            form.prefs.code_type,
            OracleCompilerMessage::CodeType,
        ),
        labeled_combo(
            i18n::t("prefs.oracle.compiler.plscope_identifiers"),
            i18n::t("prefs.oracle.compiler.plscope_identifiers_tip"),
            PlscopeIdentifiers::all(),
            form.prefs.plscope_identifiers,
            OracleCompilerMessage::PlscopeIdentifiers,
        ),
        labeled_combo(
            i18n::t("prefs.oracle.compiler.plscope_statements"),
            i18n::t("prefs.oracle.compiler.plscope_statements_tip"),
            PlscopeStatements::all(),
            form.prefs.plscope_statements,
            OracleCompilerMessage::PlscopeStatements,
        ),
        name_value_grid(
            i18n::t("prefs.oracle.compiler.cc_flags"),
            i18n::t("prefs.oracle.compiler.cc_flags_tip"),
            &form.prefs.conditional_compilation_flags,
            form.selected_cc,
            OracleCompilerMessage::CcSelect,
            OracleCompilerMessage::CcAdd,
            OracleCompilerMessage::CcEdit,
            OracleCompilerMessage::CcDelete,
        ),
        warning_grid(form),
        name_value_grid(
            i18n::t("prefs.oracle.compiler.subst_vars"),
            i18n::t("prefs.oracle.compiler.subst_vars_tip"),
            &form.prefs.substitution_variables,
            form.selected_subst,
            OracleCompilerMessage::SubstSelect,
            OracleCompilerMessage::SubstAdd,
            OracleCompilerMessage::SubstEdit,
            OracleCompilerMessage::SubstDelete,
        ),
    ]
    .spacing(8)
    .width(Fill)
    .padding(Padding {
        top: 0.0,
        right: 0.0,
        bottom: 8.0,
        left: 0.0,
    });

    scrollable(body).height(Fill).into()
}

/// Nested Add/Edit dialog over Preferences when a compiler grid editor is open.
///
/// # Examples
///
/// ```
/// use plsqlide::ui::oracle_compiler::{oracle_compiler_editor_modal, OracleCompilerForm};
///
/// let form = OracleCompilerForm::load();
/// assert!(oracle_compiler_editor_modal(&form).is_none());
/// ```
pub fn oracle_compiler_editor_modal(
    form: &OracleCompilerForm,
) -> Option<Element<'_, OracleCompilerMessage>> {
    if let Some(draft) = &form.name_value_editor {
        Some(compiler_dialog_modal(
            name_value_editor(draft),
            OracleCompilerMessage::NameValueCancel,
        ))
    } else if let Some(draft) = &form.warning_editor {
        Some(compiler_dialog_modal(
            warning_editor(draft),
            OracleCompilerMessage::WarningEditorCancel,
        ))
    } else {
        None
    }
}

fn compiler_dialog_modal(
    content: Element<'_, OracleCompilerMessage>,
    on_backdrop: OracleCompilerMessage,
) -> Element<'_, OracleCompilerMessage> {
    let form = column![content].spacing(10).padding(16).width(420);

    let card = container(form).style(|_| container::Style {
        background: Some(Background::Color(ui_theme::current().ribbon_bg)),
        border: Border {
            color: ui_theme::current().border,
            width: 1.0,
            radius: 4.0.into(),
        },
        text_color: Some(ui_theme::current().text),
        ..container::Style::default()
    });

    // Click outside closes only this editor, not Preferences.
    let backdrop = mouse_area(
        container(Space::new().width(Fill).height(Fill))
            .width(Fill)
            .height(Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.45))),
                ..container::Style::default()
            }),
    )
    .on_press(on_backdrop);

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

fn optimize_level_row(level: u8) -> Element<'static, OracleCompilerMessage> {
    column![
        row![
            text(i18n::t("prefs.oracle.compiler.optimize_level"))
                .size(LABEL_SIZE)
                .color(ui_theme::current().muted),
            Space::new().width(6),
            help_icon(i18n::t("prefs.oracle.compiler.optimize_level_tip")),
        ]
        .spacing(4)
        .align_y(Vertical::Center),
        row![
            text_input("", &level.to_string())
                .on_input(OracleCompilerMessage::OptimizeInput)
                .size(FIELD_TEXT)
                .padding(FIELD_PAD)
                .width(56),
            column![
                button(text("▲").size(8).align_x(Horizontal::Center))
                    .padding(Padding::from([1, 6]))
                    .style(|_, status| chrome_button_style(status))
                    .on_press(OracleCompilerMessage::OptimizeInc),
                button(text("▼").size(8).align_x(Horizontal::Center))
                    .padding(Padding::from([1, 6]))
                    .style(|_, status| chrome_button_style(status))
                    .on_press(OracleCompilerMessage::OptimizeDec),
            ]
            .spacing(1),
        ]
        .spacing(4)
        .align_y(Vertical::Center),
    ]
    .spacing(2)
    .into()
}

fn labeled_combo<'a, T>(
    label: String,
    tip: String,
    options: impl IntoIterator<Item = T>,
    selected: T,
    on_select: impl Fn(T) -> OracleCompilerMessage + 'a,
) -> Element<'a, OracleCompilerMessage>
where
    T: Clone + PartialEq + std::fmt::Display + 'static,
{
    let choices: Vec<T> = options.into_iter().collect();
    column![
        row![
            text(label).size(LABEL_SIZE).color(ui_theme::current().muted),
            Space::new().width(6),
            help_icon(tip),
        ]
        .spacing(4)
        .align_y(Vertical::Center),
        pick_list(choices, Some(selected), on_select)
            .padding(FIELD_PAD)
            .text_size(FIELD_TEXT)
            .width(260),
    ]
    .spacing(2)
    .into()
}

fn name_value_grid<'a>(
    title: String,
    tip: String,
    rows: &'a [NameValuePair],
    selected: Option<usize>,
    on_select: impl Fn(usize) -> OracleCompilerMessage + Copy + 'a,
    add: OracleCompilerMessage,
    edit: OracleCompilerMessage,
    delete: OracleCompilerMessage,
) -> Element<'a, OracleCompilerMessage> {
    let header = row![
        text(title).size(FIELD_TEXT),
        Space::new().width(6),
        help_icon(tip),
        Space::new().width(Fill),
        tool_button(i18n::t("prefs.oracle.env_add"), add),
        tool_button(i18n::t("prefs.oracle.env_edit"), edit),
        tool_button(i18n::t("prefs.oracle.env_delete"), delete),
    ]
    .spacing(4)
    .align_y(Vertical::Center);

    let mut table = column![].spacing(0);
    table = table.push(
        row![
            header_cell(i18n::t("prefs.oracle.env_name"), 120.0),
            header_cell(i18n::t("prefs.oracle.env_value"), Fill),
        ]
        .spacing(1),
    );
    for (index, row_data) in rows.iter().enumerate() {
        let is_selected = selected == Some(index);
        table = table.push(
            button(
                row![
                    container(text(row_data.name.clone()).size(FIELD_TEXT))
                        .width(120)
                        .padding(Padding::from([2, 4])),
                    container(text(row_data.value.clone()).size(FIELD_TEXT))
                        .width(Fill)
                        .padding(Padding::from([2, 4])),
                ]
                .spacing(1),
            )
            .padding(0)
            .width(Fill)
            .style(move |_, status| row_style(is_selected, status))
            .on_press(on_select(index)),
        );
    }

    column![header, grid_frame(table)].spacing(2).into()
}

fn warning_grid(form: &OracleCompilerForm) -> Element<'_, OracleCompilerMessage> {
    let header = row![
        text(i18n::t("prefs.oracle.compiler.warnings")).size(FIELD_TEXT),
        Space::new().width(6),
        help_icon(i18n::t("prefs.oracle.compiler.warnings_tip")),
        Space::new().width(Fill),
        tool_button(i18n::t("prefs.oracle.env_add"), OracleCompilerMessage::WarningAdd),
        tool_button(i18n::t("prefs.oracle.env_edit"), OracleCompilerMessage::WarningEdit),
        tool_button(
            i18n::t("prefs.oracle.env_delete"),
            OracleCompilerMessage::WarningDelete,
        ),
    ]
    .spacing(4)
    .align_y(Vertical::Center);

    let mut table = column![].spacing(0);
    table = table.push(
        row![
            header_cell(i18n::t("prefs.oracle.compiler.warning.action"), 100.0),
            header_cell(i18n::t("prefs.oracle.compiler.warning.exception"), Fill),
        ]
        .spacing(1),
    );
    for (index, row_data) in form.prefs.warnings.iter().enumerate() {
        let is_selected = form.selected_warning == Some(index);
        table = table.push(
            button(
                row![
                    container(text(row_data.action.label()).size(FIELD_TEXT))
                        .width(100)
                        .padding(Padding::from([2, 4])),
                    container(text(row_data.exception.clone()).size(FIELD_TEXT))
                        .width(Fill)
                        .padding(Padding::from([2, 4])),
                ]
                .spacing(1),
            )
            .padding(0)
            .width(Fill)
            .style(move |_, status| row_style(is_selected, status))
            .on_press(OracleCompilerMessage::WarningSelect(index)),
        );
    }

    column![header, grid_frame(table)].spacing(2).into()
}

fn name_value_editor(draft: &NameValueDraft) -> Element<'_, OracleCompilerMessage> {
    let title = match (draft.grid, draft.index.is_some()) {
        (CompilerGrid::CcFlags, false) => i18n::t("prefs.oracle.compiler.cc_add_title"),
        (CompilerGrid::CcFlags, true) => i18n::t("prefs.oracle.compiler.cc_edit_title"),
        (CompilerGrid::Substitution, false) => i18n::t("prefs.oracle.compiler.subst_add_title"),
        (CompilerGrid::Substitution, true) => i18n::t("prefs.oracle.compiler.subst_edit_title"),
    };
    column![
        text(title).size(16),
        text(i18n::t("prefs.oracle.env_name"))
            .size(LABEL_SIZE)
            .color(ui_theme::current().muted),
        text_input("", &draft.name)
            .on_input(OracleCompilerMessage::NameValueName)
            .size(FIELD_TEXT)
            .padding(8)
            .width(Fill),
        text(i18n::t("prefs.oracle.env_value"))
            .size(LABEL_SIZE)
            .color(ui_theme::current().muted),
        text_input("", &draft.value)
            .on_input(OracleCompilerMessage::NameValueValue)
            .size(FIELD_TEXT)
            .padding(8)
            .width(Fill),
        editor_buttons(
            OracleCompilerMessage::NameValueCancel,
            OracleCompilerMessage::NameValueOk,
        ),
    ]
    .spacing(6)
    .into()
}

fn warning_editor(draft: &WarningDraft) -> Element<'_, OracleCompilerMessage> {
    let title = if draft.index.is_some() {
        i18n::t("prefs.oracle.compiler.warning_edit_title")
    } else {
        i18n::t("prefs.oracle.compiler.warning_add_title")
    };
    column![
        text(title).size(16),
        text(i18n::t("prefs.oracle.compiler.warning.action"))
            .size(LABEL_SIZE)
            .color(ui_theme::current().muted),
        pick_list(
            PlsqlWarningAction::all().to_vec(),
            Some(draft.action),
            OracleCompilerMessage::WarningAction,
        )
        .padding(8)
        .text_size(FIELD_TEXT)
        .width(Fill),
        text(i18n::t("prefs.oracle.compiler.warning.exception"))
            .size(LABEL_SIZE)
            .color(ui_theme::current().muted),
        text_input("", &draft.exception)
            .on_input(OracleCompilerMessage::WarningException)
            .size(FIELD_TEXT)
            .padding(8)
            .width(Fill),
        editor_buttons(
            OracleCompilerMessage::WarningEditorCancel,
            OracleCompilerMessage::WarningEditorOk,
        ),
    ]
    .spacing(6)
    .into()
}

fn editor_buttons(
    cancel: OracleCompilerMessage,
    ok: OracleCompilerMessage,
) -> Element<'static, OracleCompilerMessage> {
    row![
        button(text(i18n::t("dialog.common.cancel")).size(13))
            .padding(Padding::from([8, 16]))
            .style(|_, status| chrome_button_style(status))
            .on_press(cancel),
        Space::new().width(Fill),
        button(text(i18n::t("dialog.common.ok")).size(13))
            .padding(Padding::from([8, 16]))
            .style(|_, status| primary_button_style(status))
            .on_press(ok),
    ]
    .spacing(8)
    .into()
}

fn grid_frame<'a>(
    table: iced::widget::Column<'a, OracleCompilerMessage>,
) -> Element<'a, OracleCompilerMessage> {
    container(scrollable(table).height(Fill))
        .width(Fill)
        .height(GRID_HEIGHT)
        .style(|_| container::Style {
            background: Some(Background::Color(ui_theme::current().panel_bg)),
            border: Border {
                color: ui_theme::current().border,
                width: 1.0,
                radius: 0.0.into(),
            },
            ..container::Style::default()
        })
        .into()
}

fn tool_button(
    label: String,
    message: OracleCompilerMessage,
) -> Element<'static, OracleCompilerMessage> {
    button(text(label).size(LABEL_SIZE))
        .padding(Padding::from(BTN_PAD))
        .style(|_, status| chrome_button_style(status))
        .on_press(message)
        .into()
}

fn header_cell(
    label: String,
    width: impl Into<Length>,
) -> Element<'static, OracleCompilerMessage> {
    container(text(label).size(LABEL_SIZE).color(ui_theme::current().muted))
        .width(width)
        .padding(Padding::from([2, 4]))
        .style(|_| container::Style {
            background: Some(Background::Color(ui_theme::current().sidebar_title_bg)),
            ..container::Style::default()
        })
        .into()
}

fn help_icon(tip: String) -> Element<'static, OracleCompilerMessage> {
    tooltip(
        container(circle_help().size(14).color(ui_theme::current().muted))
            .padding(Padding::from([2, 2])),
        text(tip).size(LABEL_SIZE),
        tooltip::Position::FollowCursor,
    )
    .gap(6)
    .into()
}

fn row_style(
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
