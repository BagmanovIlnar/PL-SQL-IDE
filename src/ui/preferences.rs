//! Preferences modal: left tree navigation, right page content.
//!
//! Appearance hosts the Language combo backed by JSON locales from
//! [`crate::ui::i18n`]. Other Oracle pages remain stubs for now.

use iced::alignment::{Horizontal, Vertical};
use iced::border::Border;
use iced::widget::{
    button, column, container, mouse_area, opaque, pick_list, row, rule, scrollable, stack, text,
    Space,
};
use iced::{Background, Color, Element, Fill, Padding};

use crate::ui::i18n::{self, LocaleInfo};
use crate::ui::theme as ui_theme;

/// Top-level Preferences category shown in the left tree.
///
/// # Examples
///
/// ```
/// use plsqlide::ui::i18n;
/// use plsqlide::ui::preferences::PreferencesSection;
///
/// i18n::init();
/// assert_eq!(PreferencesSection::Oracle.key(), "prefs.section.oracle");
/// assert!(!PreferencesSection::Oracle.pages().is_empty());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PreferencesSection {
    /// Shell chrome, themes, and localization.
    UserInterface,
    /// Oracle-specific IDE options.
    Oracle,
}

impl PreferencesSection {
    /// i18n key for the section title.
    pub fn key(self) -> &'static str {
        match self {
            Self::UserInterface => "prefs.section.user_interface",
            Self::Oracle => "prefs.section.oracle",
        }
    }

    /// Translated display name.
    pub fn label(self) -> String {
        i18n::t(self.key())
    }

    /// Leaf pages under this section, in menu order.
    pub fn pages(self) -> &'static [PreferencesPage] {
        match self {
            Self::UserInterface => &[PreferencesPage::UiAppearance],
            Self::Oracle => &[
                PreferencesPage::OracleConnection,
                PreferencesPage::OracleOptions,
                PreferencesPage::OracleCompiler,
                PreferencesPage::OracleDebugger,
                PreferencesPage::OracleOutput,
                PreferencesPage::OracleTrace,
                PreferencesPage::OracleProfiler,
                PreferencesPage::OracleHint,
            ],
        }
    }

    /// All sections currently registered in Preferences.
    pub fn all() -> &'static [Self] {
        &[Self::UserInterface, Self::Oracle]
    }
}

/// Leaf Preferences page (right-hand content area).
///
/// # Examples
///
/// ```
/// use plsqlide::ui::i18n;
/// use plsqlide::ui::preferences::{PreferencesPage, PreferencesSection};
///
/// i18n::init();
/// assert_eq!(
///     PreferencesPage::OracleConnection.section(),
///     PreferencesSection::Oracle
/// );
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PreferencesPage {
    UiAppearance,
    OracleConnection,
    OracleOptions,
    OracleCompiler,
    OracleDebugger,
    OracleOutput,
    OracleTrace,
    OracleProfiler,
    OracleHint,
}

impl PreferencesPage {
    /// i18n key for the page title.
    pub fn key(self) -> &'static str {
        match self {
            Self::UiAppearance => "prefs.page.appearance",
            Self::OracleConnection => "prefs.page.connection",
            Self::OracleOptions => "prefs.page.options",
            Self::OracleCompiler => "prefs.page.compiler",
            Self::OracleDebugger => "prefs.page.debugger",
            Self::OracleOutput => "prefs.page.output",
            Self::OracleTrace => "prefs.page.trace",
            Self::OracleProfiler => "prefs.page.profiler",
            Self::OracleHint => "prefs.page.hint",
        }
    }

    /// Translated short label.
    pub fn label(self) -> String {
        i18n::t(self.key())
    }

    /// Parent section for this page.
    pub fn section(self) -> PreferencesSection {
        match self {
            Self::UiAppearance => PreferencesSection::UserInterface,
            Self::OracleConnection
            | Self::OracleOptions
            | Self::OracleCompiler
            | Self::OracleDebugger
            | Self::OracleOutput
            | Self::OracleTrace
            | Self::OracleProfiler
            | Self::OracleHint => PreferencesSection::Oracle,
        }
    }
}

/// Open Preferences dialog state (tree selection + draft locale).
///
/// # Examples
///
/// ```
/// use plsqlide::ui::i18n;
/// use plsqlide::ui::preferences::{PreferencesPage, PreferencesState};
///
/// i18n::init();
/// let state = PreferencesState::new();
/// assert_eq!(state.selected, PreferencesPage::UiAppearance);
/// assert!(!state.available.is_empty());
/// ```
#[derive(Debug, Clone)]
pub struct PreferencesState {
    /// Currently shown leaf page.
    pub selected: PreferencesPage,
    /// Draft locale in the Appearance combo.
    pub language: LocaleInfo,
    /// Locales discovered on disk when the dialog opened.
    pub available: Vec<LocaleInfo>,
    /// Sections whose children are visible in the tree.
    expanded: Vec<PreferencesSection>,
}

impl PreferencesState {
    /// Opens Preferences on Appearance; reloads locale list from disk.
    pub fn new() -> Self {
        i18n::reload_available();
        let available = i18n::available();
        let code = i18n::current_code();
        let language = available
            .iter()
            .find(|l| l.code == code)
            .cloned()
            .unwrap_or(LocaleInfo {
                code: "en".into(),
                name: "English".into(),
            });
        Self {
            selected: PreferencesPage::UiAppearance,
            language,
            available,
            expanded: vec![
                PreferencesSection::UserInterface,
                PreferencesSection::Oracle,
            ],
        }
    }

    /// Whether `section` is expanded in the left tree.
    pub fn is_expanded(&self, section: PreferencesSection) -> bool {
        self.expanded.iter().any(|item| *item == section)
    }

    /// Expand or collapse a section row.
    pub fn toggle_section(&mut self, section: PreferencesSection) {
        if let Some(index) = self.expanded.iter().position(|item| *item == section) {
            self.expanded.remove(index);
        } else {
            self.expanded.push(section);
        }
    }

    /// Select a leaf page for the right-hand panel.
    pub fn select_page(&mut self, page: PreferencesPage) {
        self.selected = page;
        if !self.is_expanded(page.section()) {
            self.expanded.push(page.section());
        }
    }
}

impl Default for PreferencesState {
    fn default() -> Self {
        Self::new()
    }
}

/// Messages produced by the Preferences modal.
#[derive(Debug, Clone)]
pub enum PreferencesMessage {
    /// User clicked a leaf page in the tree.
    SelectPage(PreferencesPage),
    /// User toggled a section expand/collapse marker.
    ToggleSection(PreferencesSection),
    /// Appearance → Language combo changed.
    LanguageChanged(LocaleInfo),
    /// Close without applying (Cancel / backdrop).
    Close,
    /// Apply draft language and close.
    Ok,
}

impl PreferencesState {
    /// Apply a Preferences UI message.
    ///
    /// Returns `true` when the dialog should close. On `Ok`, activates and
    /// persists the draft locale via [`i18n::set_locale`].
    pub fn update(&mut self, message: PreferencesMessage) -> bool {
        match message {
            PreferencesMessage::SelectPage(page) => {
                self.select_page(page);
                false
            }
            PreferencesMessage::ToggleSection(section) => {
                self.toggle_section(section);
                false
            }
            PreferencesMessage::LanguageChanged(language) => {
                self.language = language;
                false
            }
            PreferencesMessage::Close => true,
            PreferencesMessage::Ok => {
                let _ = i18n::set_locale(&self.language.code);
                true
            }
        }
    }
}

/// Right-hand content for the currently selected Preferences page.
fn preferences_page(state: &PreferencesState) -> Element<'_, PreferencesMessage> {
    match state.selected {
        PreferencesPage::UiAppearance => appearance_page(state),
        page => column![
            text(page.label()).size(16),
            text(i18n::t_args(
                "prefs.page_stub",
                &[
                    ("section", &page.section().label()),
                    ("page", &page.label()),
                ],
            ))
            .size(13)
            .color(ui_theme::current().muted),
        ]
        .spacing(10)
        .width(Fill)
        .height(Fill)
        .into(),
    }
}

/// Appearance page: Language combo filled from discovered JSON locales.
fn appearance_page(state: &PreferencesState) -> Element<'_, PreferencesMessage> {
    let locales_path = i18n::locales_dir().display().to_string();
    column![
        text(i18n::t("prefs.page.appearance")).size(16),
        text(i18n::t("prefs.language"))
            .size(12)
            .color(ui_theme::current().muted),
        pick_list(
            state.available.clone(),
            Some(state.language.clone()),
            PreferencesMessage::LanguageChanged,
        )
        .padding(8)
        .width(220),
        text(i18n::t_args(
            "prefs.locales_hint",
            &[("path", &locales_path)],
        ))
        .size(11)
        .color(ui_theme::current().muted),
    ]
    .spacing(8)
    .width(Fill)
    .height(Fill)
    .into()
}

/// Full-screen Preferences overlay: dimmed backdrop + two-pane card.
///
/// # Examples
///
/// ```ignore
/// // Shown when `app.preferences` is `Some`:
/// // preferences_modal(state).map(Message::Preferences)
/// ```
pub fn preferences_modal(state: &PreferencesState) -> Element<'_, PreferencesMessage> {
    let tree = scrollable(preferences_tree(state)).height(Fill);

    let page_body = container(preferences_page(state))
        .padding(16)
        .width(Fill)
        .height(Fill);

    let content = row![
        container(tree)
            .width(220)
            .height(Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(ui_theme::current().sidebar_bg)),
                border: Border {
                    color: ui_theme::current().border,
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..container::Style::default()
            }),
        rule::vertical(1).style(|_| rule_style()),
        container(page_body)
            .width(Fill)
            .height(Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(ui_theme::current().panel_bg)),
                ..container::Style::default()
            }),
    ]
    .height(Fill)
    .width(Fill);

    let footer = row![
        button(text(i18n::t("dialog.common.cancel")).size(13))
            .padding(Padding::from([8, 16]))
            .style(|_, status| chrome_button_style(status))
            .on_press(PreferencesMessage::Close),
        Space::new().width(Fill),
        button(text(i18n::t("dialog.common.ok")).size(13))
            .padding(Padding::from([8, 16]))
            .style(|_, status| primary_button_style(status))
            .on_press(PreferencesMessage::Ok),
    ]
    .spacing(8)
    .padding(Padding::from([10, 16]));

    let form = column![
        container(text(i18n::t("prefs.title")).size(16))
            .width(Fill)
            .padding(Padding::from([12, 16]))
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
        content,
        rule::horizontal(1).style(|_| rule_style()),
        footer,
    ]
    .width(720)
    .height(480);

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

    let backdrop = mouse_area(
        container(Space::new().width(Fill).height(Fill))
            .width(Fill)
            .height(Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.55))),
                ..container::Style::default()
            }),
    )
    .on_press(PreferencesMessage::Close);

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

fn preferences_tree(state: &PreferencesState) -> Element<'_, PreferencesMessage> {
    let mut tree = column![].spacing(2).padding(Padding::from([8, 6]));

    for section in PreferencesSection::all() {
        let expanded = state.is_expanded(*section);
        let marker = if expanded { "▾" } else { "▸" };
        tree = tree.push(
            button(
                row![
                    text(marker).size(12).color(ui_theme::current().muted),
                    text(section.label()).size(13),
                ]
                .spacing(6)
                .align_y(Vertical::Center),
            )
            .width(Fill)
            .padding(Padding::from([6, 8]))
            .style(move |_, status| tree_row_style(false, status))
            .on_press(PreferencesMessage::ToggleSection(*section)),
        );

        if expanded {
            for page in section.pages() {
                let selected = state.selected == *page;
                tree = tree.push(
                    button(
                        container(text(page.label()).size(12))
                            .width(Fill)
                            .padding(Padding {
                                top: 0.0,
                                right: 0.0,
                                bottom: 0.0,
                                left: 18.0,
                            })
                            .align_x(Horizontal::Left),
                    )
                    .width(Fill)
                    .padding(Padding::from([5, 8]))
                    .style(move |_, status| tree_row_style(selected, status))
                    .on_press(PreferencesMessage::SelectPage(*page)),
                );
            }
        }
    }

    tree.into()
}

fn rule_style() -> rule::Style {
    rule::Style {
        color: ui_theme::current().border,
        radius: 0.0.into(),
        fill_mode: rule::FillMode::Full,
        snap: true,
    }
}

fn tree_row_style(
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
        None
    };

    iced::widget::button::Style {
        background,
        text_color: if selected {
            ui_theme::current().accent
        } else {
            ui_theme::current().text
        },
        border: Border {
            color: if selected {
                ui_theme::current().accent
            } else {
                Color::TRANSPARENT
            },
            width: if selected { 1.0 } else { 0.0 },
            radius: 2.0.into(),
        },
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
