//! Preferences → Oracle → NLS Options page.

use iced::alignment::Vertical;
use iced::widget::{column, container, pick_list, radio, row, scrollable, text, text_input, Space};
use iced::{Element, Fill, Length};

use crate::core::oracle_nls_prefs::{
    self as store, NlsDateSource, NlsNumberSource, NlsSymbol, NlsTimeSource, OracleNlsPrefs,
};
use crate::ui::i18n;
use crate::ui::theme as ui_theme;

const LABEL_SIZE: f32 = 11.0;
const FIELD_TEXT: f32 = 12.0;
const TITLE_SIZE: f32 = 13.0;

/// Draft Oracle → NLS Options form (saved with Preferences OK).
///
/// # Examples
///
/// ```
/// use plsqlide::core::oracle_nls_prefs::NlsSymbol;
/// use plsqlide::ui::oracle_nls::OracleNlsForm;
///
/// let form = OracleNlsForm::load();
/// assert_eq!(form.prefs.decimal_symbol, NlsSymbol::Dot);
/// assert_eq!(form.prefs.group_symbol, NlsSymbol::Comma);
/// ```
#[derive(Debug, Clone)]
pub struct OracleNlsForm {
    pub prefs: OracleNlsPrefs,
}

impl OracleNlsForm {
    pub fn load() -> Self {
        Self {
            prefs: store::load(),
        }
    }

    pub fn save(&self) {
        store::save(&self.prefs);
    }

    pub fn update(&mut self, message: OracleNlsMessage) {
        match message {
            OracleNlsMessage::DateFormatChanged(value) => self.prefs.date_format = value,
            OracleNlsMessage::DateSource(source) => self.prefs.date_source = source,
            OracleNlsMessage::TimeFormatChanged(value) => self.prefs.time_format = value,
            OracleNlsMessage::TimeSource(source) => self.prefs.time_source = source,
            OracleNlsMessage::DecimalSymbol(symbol) => self.prefs.decimal_symbol = symbol,
            OracleNlsMessage::GroupSymbol(symbol) => self.prefs.group_symbol = symbol,
            OracleNlsMessage::NumberSource(source) => self.prefs.number_source = source,
        }
    }
}

/// Messages for the Oracle NLS Options page.
#[derive(Debug, Clone)]
pub enum OracleNlsMessage {
    DateFormatChanged(String),
    DateSource(NlsDateSource),
    TimeFormatChanged(String),
    TimeSource(NlsTimeSource),
    DecimalSymbol(NlsSymbol),
    GroupSymbol(NlsSymbol),
    NumberSource(NlsNumberSource),
}

impl NlsDateSource {
    fn label(self) -> String {
        i18n::t(match self {
            Self::UserDefined => "prefs.oracle.nls.user_defined",
            Self::Windows => "prefs.oracle.nls.windows_format",
            Self::Oracle => "prefs.oracle.nls.oracle_format",
        })
    }
}

impl NlsTimeSource {
    fn label(self) -> String {
        i18n::t(match self {
            Self::UserDefined => "prefs.oracle.nls.user_defined",
            Self::Windows => "prefs.oracle.nls.windows_format",
        })
    }
}

impl NlsNumberSource {
    fn label(self) -> String {
        i18n::t(match self {
            Self::UserDefined => "prefs.oracle.nls.user_defined",
            Self::Windows => "prefs.oracle.nls.windows_format",
            Self::Oracle => "prefs.oracle.nls.oracle_format",
        })
    }
}

impl std::fmt::Display for NlsTimeSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label())
    }
}

impl std::fmt::Display for NlsNumberSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label())
    }
}

/// Right-hand Oracle → NLS Options settings form: Date, Time and Numbers
/// format sources with their user-defined values.
pub fn oracle_nls_page(form: &OracleNlsForm) -> Element<'_, OracleNlsMessage> {
    let body = column![
        text(i18n::t("prefs.page.nls")).size(TITLE_SIZE),
        // Date
        text(i18n::t("prefs.oracle.nls.date"))
            .size(LABEL_SIZE)
            .color(ui_theme::current().muted),
        format_field(
            &form.prefs.date_format,
            form.prefs.resolved_date_format(),
            form.prefs.date_source == NlsDateSource::UserDefined,
            OracleNlsMessage::DateFormatChanged,
        ),
        source_radios(NlsDateSource::all(), form.prefs.date_source, |value| {
            OracleNlsMessage::DateSource(value)
        }),
        // Time
        text(i18n::t("prefs.oracle.nls.time"))
            .size(LABEL_SIZE)
            .color(ui_theme::current().muted),
        format_field(
            &form.prefs.time_format,
            form.prefs.resolved_time_format(),
            form.prefs.time_source == NlsTimeSource::UserDefined,
            OracleNlsMessage::TimeFormatChanged,
        ),
        source_radios(NlsTimeSource::all(), form.prefs.time_source, |value| {
            OracleNlsMessage::TimeSource(value)
        }),
        // Numbers
        text(i18n::t("prefs.oracle.nls.numbers"))
            .size(LABEL_SIZE)
            .color(ui_theme::current().muted),
        symbol_row(
            i18n::t("prefs.oracle.nls.decimal_symbol"),
            form.prefs.decimal_symbol,
            form.prefs.resolved_decimal_symbol(),
            form.prefs.number_source == NlsNumberSource::UserDefined,
            OracleNlsMessage::DecimalSymbol,
        ),
        symbol_row(
            i18n::t("prefs.oracle.nls.group_symbol"),
            form.prefs.group_symbol,
            form.prefs.resolved_group_symbol(),
            form.prefs.number_source == NlsNumberSource::UserDefined,
            OracleNlsMessage::GroupSymbol,
        ),
        source_radios(NlsNumberSource::all(), form.prefs.number_source, |value| {
            OracleNlsMessage::NumberSource(value)
        }),
        resolved_hint(format!(
            "1{}234{}5",
            form.prefs.resolved_group_symbol(),
            form.prefs.resolved_decimal_symbol()
        )),
    ]
    .spacing(10)
    .width(Fill);

    scrollable(body).height(Fill).into()
}

impl std::fmt::Display for NlsDateSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label())
    }
}

/// Muted read-only line under a source group: what the selection resolves
/// to right now (auto-determined for Windows / Oracle, typed for User
/// defined).
fn resolved_hint(value: String) -> Element<'static, OracleNlsMessage> {
    text(format!("→ {value}"))
        .size(LABEL_SIZE)
        .color(ui_theme::current().muted)
        .into()
}

/// Radio group of format sources.
fn source_radios<'a, T>(
    options: impl IntoIterator<Item = T>,
    selected: T,
    on_select: fn(T) -> OracleNlsMessage,
) -> Element<'a, OracleNlsMessage>
where
    T: Copy + Eq + std::fmt::Display,
{
    let mut radios = row![].spacing(18);
    for value in options {
        radios = radios.push(
            radio(value.to_string(), value, Some(selected), on_select)
                .size(13)
                .text_size(FIELD_TEXT),
        );
    }
    radios.into()
}

/// Decimal / group symbol line: a pick list while the number source is
/// User defined, a bordered read-only value otherwise (PL/SQL Developer
/// grays the combos out the same way).
fn symbol_row<'a>(
    label: String,
    symbol: NlsSymbol,
    resolved: String,
    enabled: bool,
    on_select: fn(NlsSymbol) -> OracleNlsMessage,
) -> Element<'a, OracleNlsMessage> {
    let field: Element<'a, OracleNlsMessage> = if enabled {
        pick_list(NlsSymbol::all().to_vec(), Some(symbol), on_select)
            .padding(3)
            .text_size(FIELD_TEXT)
            .width(90)
            .into()
    } else {
        readonly_value(resolved, 90.0)
    };
    row![
        text(label)
            .size(LABEL_SIZE)
            .width(Length::Fixed(150.0))
            .align_y(Vertical::Center),
        Space::new().width(10),
        field,
    ]
    .spacing(4)
    .align_y(Vertical::Center)
    .into()
}

/// Date / time format field: an editable input while the source is User
/// defined, otherwise a bordered read-only value auto-determined by the
/// selected source (Windows / Oracle).
fn format_field<'a>(
    stored: &str,
    resolved: String,
    enabled: bool,
    on_input: fn(String) -> OracleNlsMessage,
) -> Element<'a, OracleNlsMessage> {
    if enabled {
        // The field takes an Oracle NLS format mask (DD/MM/YYYY, HH24:MI:SS).
        text_input("DD/MM/YYYY", stored)
            .on_input(on_input)
            .size(FIELD_TEXT)
            .width(180)
            .into()
    } else {
        // Show the Oracle mask that actually goes to the session (ALTER
        // SESSION), not the raw strftime pattern behind Windows format.
        let mask = crate::core::oracle_nls_prefs::strftime_to_oracle_mask(&resolved);
        readonly_value(mask, 180.0)
    }
}

/// Bordered read-only stand-in for a disabled input (grayed value).
pub fn readonly_value<M: 'static>(value: String, width: f32) -> Element<'static, M> {
    container(text(value).size(FIELD_TEXT))
        .width(width)
        .padding(6.0)
        .style(|_| container::Style {
            border: iced::Border {
                color: ui_theme::current().muted,
                width: 1.0,
                radius: 3.0.into(),
            },
            ..container::Style::default()
        })
        .center_y(20)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_symbols_only_editable_when_user_defined() {
        let mut form = OracleNlsForm {
            prefs: OracleNlsPrefs::default(),
        };
        // Defaults: Windows source — combos disabled.
        assert_eq!(form.prefs.number_source, NlsNumberSource::Windows);
        form.update(OracleNlsMessage::NumberSource(NlsNumberSource::UserDefined));
        form.update(OracleNlsMessage::DecimalSymbol(NlsSymbol::Comma));
        form.update(OracleNlsMessage::GroupSymbol(NlsSymbol::Dot));
        assert_eq!(form.prefs.decimal_symbol, NlsSymbol::Comma);
        assert_eq!(form.prefs.group_symbol, NlsSymbol::Dot);
    }

    #[test]
    fn sources_resolve_automatically() {
        use crate::core::oracle_nls_prefs::{session_nls, set_session_nls, SessionNls};

        // User defined: the typed string wins.
        let mut prefs = OracleNlsPrefs {
            date_format: "DD.MM.YYYY".into(),
            date_source: NlsDateSource::UserDefined,
            ..OracleNlsPrefs::default()
        };
        assert_eq!(prefs.resolved_date_format(), "DD.MM.YYYY");

        // Windows: the OS regional format (auto-determined, never the
        // typed string).
        prefs.date_source = NlsDateSource::Windows;
        let os_format = prefs.resolved_date_format();
        assert!(!os_format.is_empty());
        assert_ne!(os_format, "DD.MM.YYYY");

        // Oracle: the captured session NLS while connected.
        set_session_nls(SessionNls {
            date_format: "RR/MM/DD".into(),
            numeric_characters: ".,".into(),
            time_format: String::new(),
        });
        prefs.date_source = NlsDateSource::Oracle;
        prefs.number_source = NlsNumberSource::Oracle;
        assert_eq!(prefs.resolved_date_format(), "RR/MM/DD");
        assert_eq!(prefs.resolved_decimal_symbol(), ".");
        assert_eq!(prefs.resolved_group_symbol(), ",");
        // User-defined time keeps the typed string (there is no Oracle
        // variant for time — Windows or typed only).
        prefs.time_source = NlsTimeSource::UserDefined;
        assert_eq!(prefs.resolved_time_format(), "HH:MI:SS AM");
        // Restore the empty "not connected" state for other tests.
        set_session_nls(SessionNls::default());
        assert!(session_nls().unwrap().date_format.is_empty());
    }
}
