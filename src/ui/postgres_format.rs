//! Preferences → PostgreSQL → Formats page.
//!
//! Postgres has no NLS — this page is a client-side input convention.
//! Mirrors the Oracle NLS page's radio look: OS format (read-only resolved
//! mask) or a user-typed mask. Date values bind as unambiguous ISO
//! literals, so nothing here touches the server session.

use iced::alignment::Vertical;
use iced::widget::{column, radio, row, text, text_input};
use iced::{Element, Fill, Length};

use crate::core::postgres_format_prefs::{self as store, PgDateSource, PostgresFormatPrefs};
use crate::ui::i18n;
use crate::ui::oracle_nls::readonly_value;
use crate::ui::theme as ui_theme;

const LABEL_SIZE: f32 = 11.0;
const FIELD_TEXT: f32 = 12.0;
const TITLE_SIZE: f32 = 13.0;

/// Draft PostgreSQL → Formats form (saved with Preferences OK).
///
/// # Examples
///
/// ```
/// use plsqlide::ui::postgres_format::PostgresFormatForm;
///
/// let form = PostgresFormatForm::load();
/// assert!(!form.prefs.resolved_date_input().is_empty());
/// ```
#[derive(Debug, Clone)]
pub struct PostgresFormatForm {
    pub prefs: PostgresFormatPrefs,
}

impl PostgresFormatForm {
    pub fn load() -> Self {
        Self {
            prefs: store::load(),
        }
    }

    pub fn save(&self) {
        store::save(&self.prefs);
    }

    pub fn update(&mut self, message: PostgresFormatMessage) {
        match message {
            PostgresFormatMessage::DateSource(source) => self.prefs.date_source = source,
            PostgresFormatMessage::DateInputChanged(mask) => self.prefs.date_input = mask,
        }
    }
}

/// PostgreSQL → Formats page controls.
#[derive(Debug, Clone)]
pub enum PostgresFormatMessage {
    DateSource(PgDateSource),
    DateInputChanged(String),
}

/// Right-hand PostgreSQL → Formats settings form.
pub fn postgres_format_page(form: &PostgresFormatForm) -> Element<'_, PostgresFormatMessage> {
    let user_defined = form.prefs.date_source == PgDateSource::UserDefined;
    let field: Element<'_, PostgresFormatMessage> = if user_defined {
        text_input("DD.MM.YYYY", &form.prefs.date_input)
            .on_input(PostgresFormatMessage::DateInputChanged)
            .size(FIELD_TEXT)
            .width(180)
            .into()
    } else {
        // The resolved OS mask, exactly what the parameter editor follows.
        readonly_value(form.prefs.resolved_date_input(), 180.0)
    };
    let radios: Vec<Element<'_, PostgresFormatMessage>> =
        [PgDateSource::Os, PgDateSource::UserDefined]
            .into_iter()
            .map(|source| {
                radio(
                    i18n::t(source.key()),
                    source,
                    Some(form.prefs.date_source),
                    PostgresFormatMessage::DateSource,
                )
                .size(13)
                .text_size(FIELD_TEXT)
                .into()
            })
            .collect();

    column![
        text(i18n::t("prefs.page.formats")).size(TITLE_SIZE),
        text(i18n::t("prefs.pg.formats.date"))
            .size(LABEL_SIZE)
            .color(ui_theme::current().muted),
        row![
            column(radios).spacing(6),
            row![field].align_y(Vertical::Center)
        ]
        .spacing(30)
        .align_y(Vertical::Center)
        .height(Length::Shrink),
        text(i18n::t("prefs.pg.formats.date_hint"))
            .size(11)
            .color(ui_theme::current().muted),
    ]
    .spacing(6)
    .width(Fill)
    .height(Fill)
    .into()
}
