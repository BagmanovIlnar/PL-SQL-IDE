//! Preferences → Oracle → Options page.

use iced::alignment::Vertical;
use iced::widget::{checkbox, column, container, pick_list, row, scrollable, text, tooltip, Space};
use iced::{Element, Fill, Padding};
use iced_fonts::lucide::circle_help;

use crate::core::oracle_options_prefs::{
    self as store, OracleOptionsPrefs, ViewColumnAliasPolicy, ViewForcePolicy,
};
use crate::ui::i18n;
use crate::ui::theme as ui_theme;

const LABEL_SIZE: f32 = 11.0;
const FIELD_TEXT: f32 = 12.0;
const TITLE_SIZE: f32 = 13.0;

/// Draft Oracle → Options form (saved with Preferences OK).
///
/// # Examples
///
/// ```
/// use plsqlide::ui::oracle_options::OracleOptionsForm;
///
/// let form = OracleOptionsForm::load();
/// assert!(form.prefs.allow_edit_database_source);
/// assert!(!form.prefs.use_dba_views_if_available);
/// ```
#[derive(Debug, Clone)]
pub struct OracleOptionsForm {
    pub prefs: OracleOptionsPrefs,
}

impl OracleOptionsForm {
    pub fn load() -> Self {
        Self {
            prefs: store::load(),
        }
    }

    pub fn save(&self) {
        store::save(&self.prefs);
    }

    pub fn update(&mut self, message: OracleOptionsMessage) {
        match message {
            OracleOptionsMessage::AllowEdit(value) => {
                self.prefs.allow_edit_database_source = value;
            }
            OracleOptionsMessage::AskSave(value) => {
                self.prefs.ask_save_edited_database_source = value;
            }
            OracleOptionsMessage::AllowCompileReadonly(value) => {
                self.prefs.allow_compile_readonly_source = value;
            }
            OracleOptionsMessage::ConfirmCommitRollback(value) => {
                self.prefs.confirm_commit_rollback = value;
            }
            OracleOptionsMessage::UseDbaViews(value) => {
                self.prefs.use_dba_views_if_available = value;
            }
            OracleOptionsMessage::UnicodeEnabled(value) => {
                self.prefs.unicode_enabled = value;
            }
            OracleOptionsMessage::CheckCharsetMismatch(value) => {
                self.prefs.check_charset_mismatch = value;
            }
            OracleOptionsMessage::ViewColumnAlias(policy) => {
                self.prefs.view_column_alias_policy = policy;
            }
            OracleOptionsMessage::ViewForce(policy) => {
                self.prefs.view_force_policy = policy;
            }
            OracleOptionsMessage::SchedulerTimestamp(value) => {
                self.prefs.use_timestamp_format_for_scheduler = value;
            }
            OracleOptionsMessage::AlwaysOwnerPrefix(value) => {
                self.prefs.always_include_owner_prefix = value;
            }
        }
    }
}

/// Messages for the Oracle Options page.
#[derive(Debug, Clone)]
pub enum OracleOptionsMessage {
    AllowEdit(bool),
    AskSave(bool),
    AllowCompileReadonly(bool),
    ConfirmCommitRollback(bool),
    UseDbaViews(bool),
    UnicodeEnabled(bool),
    CheckCharsetMismatch(bool),
    ViewColumnAlias(ViewColumnAliasPolicy),
    ViewForce(ViewForcePolicy),
    SchedulerTimestamp(bool),
    AlwaysOwnerPrefix(bool),
}

impl ViewColumnAliasPolicy {
    fn label(self) -> String {
        i18n::t(match self {
            Self::Auto => "prefs.oracle.options.view_alias.auto",
            Self::Always => "prefs.oracle.options.view_alias.always",
            Self::Never => "prefs.oracle.options.view_alias.never",
        })
    }
}

impl std::fmt::Display for ViewColumnAliasPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label())
    }
}

impl ViewForcePolicy {
    fn label(self) -> String {
        i18n::t(match self {
            Self::OnlyForExports => "prefs.oracle.options.view_force.exports",
            Self::Always => "prefs.oracle.options.view_force.always",
            Self::Never => "prefs.oracle.options.view_force.never",
        })
    }
}

impl std::fmt::Display for ViewForcePolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label())
    }
}

/// Right-hand Oracle → Options settings form.
pub fn oracle_options_page(form: &OracleOptionsForm) -> Element<'_, OracleOptionsMessage> {
    let body = column![
        text(i18n::t("prefs.page.options")).size(TITLE_SIZE),
        option_checkbox(
            i18n::t("prefs.oracle.options.allow_edit"),
            i18n::t("prefs.oracle.options.allow_edit_tip"),
            form.prefs.allow_edit_database_source,
            OracleOptionsMessage::AllowEdit,
        ),
        option_checkbox(
            i18n::t("prefs.oracle.options.ask_save"),
            i18n::t("prefs.oracle.options.ask_save_tip"),
            form.prefs.ask_save_edited_database_source,
            OracleOptionsMessage::AskSave,
        ),
        option_checkbox(
            i18n::t("prefs.oracle.options.allow_compile_readonly"),
            i18n::t("prefs.oracle.options.allow_compile_readonly_tip"),
            form.prefs.allow_compile_readonly_source,
            OracleOptionsMessage::AllowCompileReadonly,
        ),
        option_checkbox(
            i18n::t("prefs.oracle.options.confirm_commit_rollback"),
            i18n::t("prefs.oracle.options.confirm_commit_rollback_tip"),
            form.prefs.confirm_commit_rollback,
            OracleOptionsMessage::ConfirmCommitRollback,
        ),
        option_checkbox(
            i18n::t("prefs.oracle.options.use_dba_views"),
            i18n::t("prefs.oracle.options.use_dba_views_tip"),
            form.prefs.use_dba_views_if_available,
            OracleOptionsMessage::UseDbaViews,
        ),
        option_checkbox(
            i18n::t("prefs.oracle.options.unicode_enabled"),
            i18n::t("prefs.oracle.options.unicode_enabled_tip"),
            form.prefs.unicode_enabled,
            OracleOptionsMessage::UnicodeEnabled,
        ),
        option_checkbox(
            i18n::t("prefs.oracle.options.check_charset"),
            i18n::t("prefs.oracle.options.check_charset_tip"),
            form.prefs.check_charset_mismatch,
            OracleOptionsMessage::CheckCharsetMismatch,
        ),
        option_combo(
            i18n::t("prefs.oracle.options.view_alias"),
            i18n::t("prefs.oracle.options.view_alias_tip"),
            ViewColumnAliasPolicy::all(),
            form.prefs.view_column_alias_policy,
            OracleOptionsMessage::ViewColumnAlias,
        ),
        option_combo(
            i18n::t("prefs.oracle.options.view_force"),
            i18n::t("prefs.oracle.options.view_force_tip"),
            ViewForcePolicy::all(),
            form.prefs.view_force_policy,
            OracleOptionsMessage::ViewForce,
        ),
        option_checkbox(
            i18n::t("prefs.oracle.options.scheduler_timestamp"),
            i18n::t("prefs.oracle.options.scheduler_timestamp_tip"),
            form.prefs.use_timestamp_format_for_scheduler,
            OracleOptionsMessage::SchedulerTimestamp,
        ),
        option_checkbox(
            i18n::t("prefs.oracle.options.owner_prefix"),
            i18n::t("prefs.oracle.options.owner_prefix_tip"),
            form.prefs.always_include_owner_prefix,
            OracleOptionsMessage::AlwaysOwnerPrefix,
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

fn option_checkbox<'a>(
    label: String,
    tip: String,
    checked: bool,
    on_toggle: impl Fn(bool) -> OracleOptionsMessage + 'a,
) -> Element<'a, OracleOptionsMessage> {
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
    on_select: impl Fn(T) -> OracleOptionsMessage + 'a,
) -> Element<'a, OracleOptionsMessage>
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
            .padding(3)
            .text_size(FIELD_TEXT)
            .width(260),
    ]
    .spacing(2)
    .into()
}

fn help_icon(tip: String) -> Element<'static, OracleOptionsMessage> {
    tooltip(
        container(circle_help().size(14).color(ui_theme::current().muted))
            .padding(Padding::from([2, 2])),
        text(tip).size(LABEL_SIZE),
        tooltip::Position::FollowCursor,
    )
    .gap(6)
    .into()
}
