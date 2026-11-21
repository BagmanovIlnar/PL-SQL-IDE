//! Preferences page: PostgreSQL → Connection.
//!
//! Mirrors the Oracle connection page's look (label + `?` tooltip + control)
//! but keeps only options sqlx actually honours from the connection URL:
//! sslmode and connect timeout. No client home / library / env grid — sqlx is
//! a pure-Rust driver and ignores those.

use iced::alignment::Vertical;
use iced::widget::{column, container, pick_list, row, text, text_input, tooltip, Space};
use iced::{Element, Length, Padding};
use iced_fonts::lucide::circle_help;

use crate::core::postgres_connection_prefs::{self as pg_prefs, PostgresConnectionPrefs, SslMode};
use crate::ui::i18n;
use crate::ui::theme;

// Compact control metrics, matching the Oracle prefs forms.
const LABEL_SIZE: f32 = 11.0;
const FIELD_TEXT: f32 = 12.0;
const TITLE_SIZE: f32 = 13.0;
const FIELD_PAD: f32 = 3.0;

/// Draft PostgreSQL → Connection form; saved on Preferences OK.
#[derive(Debug, Clone)]
pub struct PostgresConnectionForm {
    pub prefs: PostgresConnectionPrefs,
    timeout_draft: String,
}

impl PostgresConnectionForm {
    pub fn load() -> Self {
        let prefs = pg_prefs::load();
        let timeout_draft = prefs.connect_timeout_secs.to_string();
        Self {
            prefs,
            timeout_draft,
        }
    }

    /// Persist when the user clicks OK.
    pub fn save(&mut self) {
        if let Ok(secs) = self.timeout_draft.trim().parse::<u32>() {
            self.prefs.connect_timeout_secs = secs;
        }
        pg_prefs::save(&self.prefs);
    }

    /// Apply a control message.
    pub fn update(&mut self, message: PostgresConnectionMessage) {
        match message {
            PostgresConnectionMessage::SslModeChanged(mode) => self.prefs.ssl_mode = mode,
            PostgresConnectionMessage::TimeoutChanged(value) => self.timeout_draft = value,
        }
    }
}

/// PostgreSQL → Connection page controls.
#[derive(Debug, Clone)]
pub enum PostgresConnectionMessage {
    SslModeChanged(SslMode),
    TimeoutChanged(String),
}

impl std::fmt::Display for SslMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&i18n::t(self.key()))
    }
}

/// Right-hand PostgreSQL → Connection settings form.
pub fn postgres_connection_page(
    form: &PostgresConnectionForm,
) -> Element<'_, PostgresConnectionMessage> {
    let ssl = column![
        row![
            text(i18n::t("prefs.pg.ssl"))
                .size(LABEL_SIZE)
                .color(crate::ui::theme::current().muted),
            Space::new().width(6),
            help_icon(i18n::t("prefs.pg.ssl_tip")),
        ]
        .spacing(4)
        .align_y(Vertical::Center),
        pick_list(
            SslMode::all(),
            Some(form.prefs.ssl_mode),
            PostgresConnectionMessage::SslModeChanged,
        )
        .padding(FIELD_PAD)
        .text_size(FIELD_TEXT)
        .width(200),
    ]
    .spacing(2);

    let timeout = column![
        row![
            text(i18n::t("prefs.pg.timeout"))
                .size(LABEL_SIZE)
                .color(crate::ui::theme::current().muted),
            Space::new().width(6),
            help_icon(i18n::t("prefs.pg.timeout_tip")),
        ]
        .spacing(4)
        .align_y(Vertical::Center),
        text_input("", &form.timeout_draft)
            .on_input(PostgresConnectionMessage::TimeoutChanged)
            .size(FIELD_TEXT)
            .padding(FIELD_PAD)
            .width(200),
    ]
    .spacing(2);

    column![
        text(i18n::t("prefs.page.connection")).size(TITLE_SIZE),
        ssl,
        timeout,
    ]
    .spacing(6)
    .width(Length::Fill)
    .into()
}

fn help_icon(tip: String) -> Element<'static, PostgresConnectionMessage> {
    tooltip(
        container(
            circle_help()
                .size(14)
                .color(crate::ui::theme::current().muted),
        )
        .padding(Padding::from([2, 2])),
        text(tip).size(LABEL_SIZE),
        tooltip::Position::FollowCursor,
    )
    .style(|_| crate::ui::theme::tooltip_style())
    .gap(6)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_draft_parses_on_save() {
        let mut form = PostgresConnectionForm {
            prefs: PostgresConnectionPrefs::default(),
            timeout_draft: "7".into(),
        };
        form.save();
        assert_eq!(pg_prefs::load().connect_timeout_secs, 7);
    }
}
