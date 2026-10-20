//! Debug toolbar under the worksheet chrome (Start / Run / Step / …).
//!
//! Buttons show icons only; full labels appear in hover tooltips.
//! Start is a split-button: icon runs the selected system; ▾ opens an overlay
//! menu (MenuBar) like a combobox — without expanding the toolbar.

use iced::alignment::Vertical;
use iced::border::Border;
use iced::widget::{button, container, row, text, tooltip, Space};
use iced::{Background, Color, Element, Fill, Padding};
use iced_aw::menu::{DrawPath, Item, Menu, MenuBar};
use iced_aw::style::menu_bar as menu_bar_style;
use iced_fonts::lucide::{
    arrow_up, bug, corner_down_right, play, redo, skip_forward, terminal, triangle_alert,
};

use crate::core::oracle_debugger_prefs::DebugSystem;
use crate::ui::i18n;
use crate::ui::theme as ui_theme;

const ICON_SIZE: f32 = 14.0;
const BTN_HEIGHT: f32 = 26.0;
const BTN_PAD: [u16; 2] = [5, 7];
const ARROW_PAD: [u16; 2] = [5, 4];

/// Messages emitted by the debug toolbar.
#[derive(Debug, Clone)]
pub enum DebugToolbarMessage {
    /// Start with the currently selected system.
    Start,
    /// Choose a system from the split menu and start.
    StartWith(DebugSystem),
    Run,
    StepInto,
    StepOver,
    StepOut,
    RunToCursor,
    RunToNextException,
    ExecuteSqlInDebugSession,
}

/// Compact debug action bar (icons + Start split-button overlay menu).
pub fn debug_toolbar(selected_system: DebugSystem) -> Element<'static, DebugToolbarMessage> {
    let start_tip = format!(
        "{} — {}",
        i18n::t("debug.start"),
        system_label(selected_system)
    );

    let bar = row![
        start_split_button(start_tip, selected_system),
        tool_icon(play().size(ICON_SIZE), i18n::t("debug.run"), DebugToolbarMessage::Run),
        tool_icon(
            skip_forward().size(ICON_SIZE),
            i18n::t("debug.step_into"),
            DebugToolbarMessage::StepInto,
        ),
        tool_icon(
            redo().size(ICON_SIZE),
            i18n::t("debug.step_over"),
            DebugToolbarMessage::StepOver,
        ),
        tool_icon(
            arrow_up().size(ICON_SIZE),
            i18n::t("debug.step_out"),
            DebugToolbarMessage::StepOut,
        ),
        tool_icon(
            corner_down_right().size(ICON_SIZE),
            i18n::t("debug.run_to_cursor"),
            DebugToolbarMessage::RunToCursor,
        ),
        tool_icon(
            triangle_alert().size(ICON_SIZE),
            i18n::t("debug.run_to_exception"),
            DebugToolbarMessage::RunToNextException,
        ),
        tool_icon(
            terminal().size(ICON_SIZE),
            i18n::t("debug.execute_sql"),
            DebugToolbarMessage::ExecuteSqlInDebugSession,
        ),
        Space::new().width(Fill),
    ]
    .spacing(2)
    .align_y(Vertical::Center)
    .padding(Padding::from([4, 8]));

    container(bar)
        .width(Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(ui_theme::current().chrome_bg)),
            border: Border {
                color: ui_theme::current().border,
                width: 1.0,
                radius: 0.0.into(),
            },
            ..container::Style::default()
        })
        .into()
}

fn system_label(system: DebugSystem) -> String {
    match system {
        DebugSystem::DbmsDebug => i18n::t("debug.split.dbms_debug"),
        DebugSystem::DbmsDebugJdwp => i18n::t("debug.split.dbms_debug_jdwp"),
    }
}

/// Icon Start + ▾ overlay menu (does not grow the toolbar row).
fn start_split_button(
    start_tip: String,
    selected: DebugSystem,
) -> Element<'static, DebugToolbarMessage> {
    let start = tooltip(
        button(bug().size(ICON_SIZE))
            .padding(Padding::from(BTN_PAD))
            .height(BTN_HEIGHT)
            .style(|_, status| split_left_style(status))
            .on_press(DebugToolbarMessage::Start),
        text(start_tip).size(11),
        tooltip::Position::FollowCursor,
    );

    let menu = Menu::new(vec![
        Item::new(menu_row(
            system_label(DebugSystem::DbmsDebug),
            selected == DebugSystem::DbmsDebug,
            DebugToolbarMessage::StartWith(DebugSystem::DbmsDebug),
        )),
        Item::new(menu_row(
            system_label(DebugSystem::DbmsDebugJdwp),
            selected == DebugSystem::DbmsDebugJdwp,
            DebugToolbarMessage::StartWith(DebugSystem::DbmsDebugJdwp),
        )),
    ])
    .max_width(200.0)
    .offset(1.0)
    .close_on_item_click(true)
    .close_on_background_click(true);

    let arrow = button(text("▾").size(ICON_SIZE).center())
        .padding(Padding::from(ARROW_PAD))
        .height(BTN_HEIGHT)
        .style(|_, status| split_right_style(status));

    let arrow_menu = MenuBar::new(vec![Item::with_menu(arrow, menu)])
        .draw_path(DrawPath::Backdrop)
        .close_on_item_click(true)
        .close_on_background_click(true)
        .close_on_item_click_global(true)
        .close_on_background_click_global(true)
        .style(|_, _| split_menu_style());

    row![start, arrow_menu]
        .spacing(0)
        .height(BTN_HEIGHT)
        .align_y(Vertical::Center)
        .into()
}

fn menu_row(
    label: String,
    selected: bool,
    message: DebugToolbarMessage,
) -> iced::widget::Button<'static, DebugToolbarMessage> {
    let mark = if selected { "✓ " } else { "  " };
    button(text(format!("{mark}{label}")).size(12))
        .padding(Padding::from([6, 12]))
        .width(Fill)
        .style(move |_, status| menu_item_style(selected, status))
        .on_press(message)
}

fn tool_icon<'a>(
    icon: iced::widget::Text<'a>,
    tip: String,
    message: DebugToolbarMessage,
) -> Element<'a, DebugToolbarMessage> {
    tooltip(
        button(icon)
            .padding(Padding::from(BTN_PAD))
            .style(|_, status| tool_style(status))
            .on_press(message),
        text(tip).size(11),
        tooltip::Position::FollowCursor,
    )
    .into()
}

fn tool_style(status: iced::widget::button::Status) -> iced::widget::button::Style {
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

fn split_bg(status: iced::widget::button::Status) -> Option<Background> {
    match status {
        iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed => {
            Some(Background::Color(ui_theme::current().hover))
        }
        _ => Some(Background::Color(ui_theme::current().chrome_bg)),
    }
}

fn split_left_style(status: iced::widget::button::Status) -> iced::widget::button::Style {
    iced::widget::button::Style {
        background: split_bg(status),
        text_color: ui_theme::current().text,
        border: Border {
            color: ui_theme::current().border,
            width: 1.0,
            radius: iced::border::Radius {
                top_left: 2.0,
                bottom_left: 2.0,
                top_right: 0.0,
                bottom_right: 0.0,
            },
        },
        ..iced::widget::button::Style::default()
    }
}

fn split_right_style(status: iced::widget::button::Status) -> iced::widget::button::Style {
    iced::widget::button::Style {
        background: split_bg(status),
        text_color: ui_theme::current().text,
        border: Border {
            color: ui_theme::current().border,
            width: 1.0,
            radius: iced::border::Radius {
                top_left: 0.0,
                bottom_left: 0.0,
                top_right: 2.0,
                bottom_right: 2.0,
            },
        },
        ..iced::widget::button::Style::default()
    }
}

fn menu_item_style(
    selected: bool,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    let background = if selected
        || matches!(
            status,
            iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed
        ) {
        Some(Background::Color(ui_theme::current().hover))
    } else {
        Some(Background::Color(ui_theme::current().ribbon_bg))
    };
    iced::widget::button::Style {
        background,
        text_color: ui_theme::current().text,
        border: Border::default(),
        ..iced::widget::button::Style::default()
    }
}

fn split_menu_style() -> menu_bar_style::Style {
    menu_bar_style::Style {
        bar_background: Background::Color(Color::TRANSPARENT),
        bar_border: Border {
            radius: 0.0.into(),
            ..Border::default()
        },
        bar_shadow: iced::Shadow::default(),
        menu_background: Background::Color(ui_theme::current().ribbon_bg),
        menu_border: Border {
            color: ui_theme::current().border,
            width: 1.0,
            radius: 2.0.into(),
        },
        menu_shadow: iced::Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.25),
            offset: iced::Vector::new(0.0, 3.0),
            blur_radius: 10.0,
        },
        path: Background::Color(ui_theme::current().hover),
        path_border: Border {
            color: ui_theme::current().border,
            width: 1.0,
            radius: 2.0.into(),
        },
    }
}
