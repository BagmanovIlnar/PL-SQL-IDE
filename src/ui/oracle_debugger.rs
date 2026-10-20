//! Preferences → Oracle → Debugger page.

use iced::alignment::{Horizontal, Vertical};
use iced::widget::text_editor::{Action, Content};
use iced::widget::{
    button, checkbox, column, container, pick_list, row, scrollable, text, text_editor, text_input,
    tooltip, Space,
};
use iced::{Element, Fill, Font, Padding};
use iced_fonts::lucide::circle_help;

use crate::core::oracle_debugger_prefs::{
    self as store, list_jdwp_network_adapters, DebugSystem, OracleDebuggerPrefs,
};
use crate::ui::i18n;
use crate::ui::theme as ui_theme;

const LABEL_SIZE: f32 = 11.0;
const FIELD_TEXT: f32 = 12.0;
const TITLE_SIZE: f32 = 13.0;
const FIELD_PAD: u16 = 3;

/// Draft Oracle → Debugger form (saved with Preferences OK).
///
/// # Examples
///
/// ```
/// use plsqlide::ui::oracle_debugger::OracleDebuggerForm;
///
/// let form = OracleDebuggerForm::load();
/// assert!(form.prefs.show_variable_values_popup);
/// assert_eq!(form.prefs.default_jdwp_port, 4001);
/// ```
#[derive(Debug)]
pub struct OracleDebuggerForm {
    pub prefs: OracleDebuggerPrefs,
    pub network_adapters: Vec<String>,
    pub never_objects: Content,
    pub always_objects: Content,
    pub never_connections: Content,
}

impl Clone for OracleDebuggerForm {
    fn clone(&self) -> Self {
        Self {
            prefs: self.prefs.clone(),
            network_adapters: self.network_adapters.clone(),
            never_objects: Content::with_text(&self.prefs.never_debug_objects),
            always_objects: Content::with_text(&self.prefs.always_debug_objects),
            never_connections: Content::with_text(&self.prefs.never_debug_connections),
        }
    }
}

impl OracleDebuggerForm {
    pub fn load() -> Self {
        let prefs = store::load();
        let mut network_adapters = list_jdwp_network_adapters();
        if !network_adapters
            .iter()
            .any(|a| a == &prefs.default_jdwp_network_adapter)
        {
            network_adapters.push(prefs.default_jdwp_network_adapter.clone());
        }
        Self {
            never_objects: Content::with_text(&prefs.never_debug_objects),
            always_objects: Content::with_text(&prefs.always_debug_objects),
            never_connections: Content::with_text(&prefs.never_debug_connections),
            prefs,
            network_adapters,
        }
    }

    pub fn save(&mut self) {
        self.sync_textareas_to_prefs();
        store::save(&self.prefs);
    }

    fn sync_textareas_to_prefs(&mut self) {
        self.prefs.never_debug_objects = self.never_objects.text();
        self.prefs.always_debug_objects = self.always_objects.text();
        self.prefs.never_debug_connections = self.never_connections.text();
    }

    pub fn update(&mut self, message: OracleDebuggerMessage) {
        match message {
            OracleDebuggerMessage::ShowPopup(value) => {
                self.prefs.show_variable_values_popup = value;
            }
            OracleDebuggerMessage::StepOverSpecInit(value) => {
                self.prefs.step_over_package_spec_init = value;
            }
            OracleDebuggerMessage::StepOverBodyInit(value) => {
                self.prefs.step_over_package_body_init = value;
            }
            OracleDebuggerMessage::StepOverSys(value) => {
                self.prefs.step_over_sys_objects = value;
            }
            OracleDebuggerMessage::DebugSystem(value) => {
                self.prefs.default_debug_system = value;
            }
            OracleDebuggerMessage::JdwpPortInc => {
                if self.prefs.default_jdwp_port < u16::MAX {
                    self.prefs.default_jdwp_port += 1;
                }
            }
            OracleDebuggerMessage::JdwpPortDec => {
                if self.prefs.default_jdwp_port > 1 {
                    self.prefs.default_jdwp_port -= 1;
                }
            }
            OracleDebuggerMessage::JdwpPortInput(raw) => {
                let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
                if digits.is_empty() {
                    return;
                }
                if let Ok(n) = digits.parse::<u16>() {
                    self.prefs.default_jdwp_port = OracleDebuggerPrefs::clamp_jdwp_port(n);
                }
            }
            OracleDebuggerMessage::NetworkAdapter(value) => {
                self.prefs.default_jdwp_network_adapter = value;
            }
            OracleDebuggerMessage::AddDebugInfo(value) => {
                self.prefs.add_debug_info_when_compiling = value;
            }
            OracleDebuggerMessage::NeverObjects(action) => {
                self.never_objects.perform(action);
            }
            OracleDebuggerMessage::AlwaysObjects(action) => {
                self.always_objects.perform(action);
            }
            OracleDebuggerMessage::NeverConnections(action) => {
                self.never_connections.perform(action);
            }
        }
    }
}

/// Messages for the Oracle Debugger preferences page.
#[derive(Debug, Clone)]
pub enum OracleDebuggerMessage {
    ShowPopup(bool),
    StepOverSpecInit(bool),
    StepOverBodyInit(bool),
    StepOverSys(bool),
    DebugSystem(DebugSystem),
    JdwpPortInc,
    JdwpPortDec,
    JdwpPortInput(String),
    NetworkAdapter(String),
    AddDebugInfo(bool),
    NeverObjects(Action),
    AlwaysObjects(Action),
    NeverConnections(Action),
}

impl DebugSystem {
    fn label(self) -> String {
        i18n::t(match self {
            Self::DbmsDebug => "prefs.oracle.debugger.system.dbms_debug",
            Self::DbmsDebugJdwp => "prefs.oracle.debugger.system.dbms_debug_jdwp",
        })
    }
}

impl std::fmt::Display for DebugSystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label())
    }
}

/// Right-hand Oracle → Debugger settings form.
pub fn oracle_debugger_page(form: &OracleDebuggerForm) -> Element<'_, OracleDebuggerMessage> {
    let body = column![
        text(i18n::t("prefs.page.debugger")).size(TITLE_SIZE),
        option_checkbox(
            i18n::t("prefs.oracle.debugger.show_popup"),
            i18n::t("prefs.oracle.debugger.show_popup_tip"),
            form.prefs.show_variable_values_popup,
            OracleDebuggerMessage::ShowPopup,
        ),
        option_checkbox(
            i18n::t("prefs.oracle.debugger.step_spec_init"),
            i18n::t("prefs.oracle.debugger.step_spec_init_tip"),
            form.prefs.step_over_package_spec_init,
            OracleDebuggerMessage::StepOverSpecInit,
        ),
        option_checkbox(
            i18n::t("prefs.oracle.debugger.step_body_init"),
            i18n::t("prefs.oracle.debugger.step_body_init_tip"),
            form.prefs.step_over_package_body_init,
            OracleDebuggerMessage::StepOverBodyInit,
        ),
        option_checkbox(
            i18n::t("prefs.oracle.debugger.step_sys"),
            i18n::t("prefs.oracle.debugger.step_sys_tip"),
            form.prefs.step_over_sys_objects,
            OracleDebuggerMessage::StepOverSys,
        ),
        option_combo(
            i18n::t("prefs.oracle.debugger.default_system"),
            i18n::t("prefs.oracle.debugger.default_system_tip"),
            DebugSystem::all(),
            form.prefs.default_debug_system,
            OracleDebuggerMessage::DebugSystem,
        ),
        jdwp_port_row(form.prefs.default_jdwp_port),
        network_adapter_row(form),
        option_checkbox(
            i18n::t("prefs.oracle.debugger.add_debug_info"),
            i18n::t("prefs.oracle.debugger.add_debug_info_tip"),
            form.prefs.add_debug_info_when_compiling,
            OracleDebuggerMessage::AddDebugInfo,
        ),
        textarea_block(
            i18n::t("prefs.oracle.debugger.never_objects"),
            i18n::t("prefs.oracle.debugger.never_objects_tip"),
            &form.never_objects,
            OracleDebuggerMessage::NeverObjects,
        ),
        textarea_block(
            i18n::t("prefs.oracle.debugger.always_objects"),
            i18n::t("prefs.oracle.debugger.always_objects_tip"),
            &form.always_objects,
            OracleDebuggerMessage::AlwaysObjects,
        ),
        textarea_block(
            i18n::t("prefs.oracle.debugger.never_connections"),
            i18n::t("prefs.oracle.debugger.never_connections_tip"),
            &form.never_connections,
            OracleDebuggerMessage::NeverConnections,
        ),
    ]
    .spacing(10)
    .width(Fill)
    .padding(Padding {
        top: 0.0,
        right: 0.0,
        bottom: 8.0,
        left: 0.0,
    });

    scrollable(body).height(Fill).into()
}

fn jdwp_port_row(port: u16) -> Element<'static, OracleDebuggerMessage> {
    column![
        row![
            text(i18n::t("prefs.oracle.debugger.jdwp_port"))
                .size(LABEL_SIZE)
                .color(ui_theme::current().muted),
            Space::new().width(6),
            help_icon(i18n::t("prefs.oracle.debugger.jdwp_port_tip")),
        ]
        .spacing(4)
        .align_y(Vertical::Center),
        row![
            text_input("", &port.to_string())
                .on_input(OracleDebuggerMessage::JdwpPortInput)
                .size(FIELD_TEXT)
                .padding(FIELD_PAD)
                .width(80),
            column![
                button(text("▲").size(8).align_x(Horizontal::Center))
                    .padding(Padding::from([1, 6]))
                    .on_press(OracleDebuggerMessage::JdwpPortInc),
                button(text("▼").size(8).align_x(Horizontal::Center))
                    .padding(Padding::from([1, 6]))
                    .on_press(OracleDebuggerMessage::JdwpPortDec),
            ]
            .spacing(1),
        ]
        .spacing(4)
        .align_y(Vertical::Center),
    ]
    .spacing(2)
    .into()
}

fn network_adapter_row(form: &OracleDebuggerForm) -> Element<'_, OracleDebuggerMessage> {
    column![
        row![
            text(i18n::t("prefs.oracle.debugger.jdwp_adapter"))
                .size(LABEL_SIZE)
                .color(ui_theme::current().muted),
            Space::new().width(6),
            help_icon(i18n::t("prefs.oracle.debugger.jdwp_adapter_tip")),
        ]
        .spacing(4)
        .align_y(Vertical::Center),
        pick_list(
            form.network_adapters.clone(),
            Some(form.prefs.default_jdwp_network_adapter.clone()),
            OracleDebuggerMessage::NetworkAdapter,
        )
        .padding(FIELD_PAD)
        .text_size(FIELD_TEXT)
        .width(260),
    ]
    .spacing(2)
    .into()
}

fn textarea_block<'a>(
    label: String,
    tip: String,
    content: &'a Content,
    on_action: impl Fn(Action) -> OracleDebuggerMessage + 'a,
) -> Element<'a, OracleDebuggerMessage> {
    column![
        row![
            text(label).size(LABEL_SIZE).color(ui_theme::current().muted),
            Space::new().width(6),
            help_icon(tip),
        ]
        .spacing(4)
        .align_y(Vertical::Center),
        container(
            text_editor(content)
                .height(72)
                .font(Font::MONOSPACE)
                .size(FIELD_TEXT)
                .padding(6)
                .on_action(on_action),
        )
        .width(Fill)
        .style(|_| container::Style {
            background: Some(iced::Background::Color(ui_theme::current().panel_bg)),
            border: iced::border::Border {
                color: ui_theme::current().border,
                width: 1.0,
                radius: 2.0.into(),
            },
            ..container::Style::default()
        }),
    ]
    .spacing(2)
    .into()
}

fn option_checkbox<'a>(
    label: String,
    tip: String,
    checked: bool,
    on_toggle: impl Fn(bool) -> OracleDebuggerMessage + 'a,
) -> Element<'a, OracleDebuggerMessage> {
    row![
        checkbox(checked)
            .label(label)
            .size(14)
            .text_size(FIELD_TEXT)
            .on_toggle(on_toggle),
        Space::new().width(6),
        help_icon(tip),
    ]
    .spacing(4)
    .align_y(Vertical::Center)
    .into()
}

fn option_combo<'a, T>(
    label: String,
    tip: String,
    options: impl IntoIterator<Item = T>,
    selected: T,
    on_select: impl Fn(T) -> OracleDebuggerMessage + 'a,
) -> Element<'a, OracleDebuggerMessage>
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

fn help_icon(tip: String) -> Element<'static, OracleDebuggerMessage> {
    tooltip(
        container(circle_help().size(14).color(ui_theme::current().muted))
            .padding(Padding::from([2, 2])),
        text(tip).size(LABEL_SIZE),
        tooltip::Position::FollowCursor,
    )
    .gap(6)
    .into()
}
