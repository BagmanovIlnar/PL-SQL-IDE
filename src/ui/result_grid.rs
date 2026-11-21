//! Editable result grid for the SQL window bottom "Result" tab.
//!
//! Shows a paged slice (10 rows) of the fetched buffer with an icon toolbar:
//! edit data / insert / delete / post changes / fetch next / fetch last page.

use std::collections::{HashMap, HashSet};

use iced::alignment::Vertical;
use iced::widget::text_editor::{Action, Content};
use std::sync::LazyLock;

use iced::widget::operation::{scroll_to, AbsoluteOffset};
use iced::widget::scrollable::Viewport;
use iced::widget::Id;
use iced::widget::{
    button, column, container, mouse_area, opaque, row, rule, scrollable, space::Space, stack,
    text, text_editor, text_input, tooltip,
};
use iced::{mouse, Background, Border, Element, Fill, Font, Padding, Shrink, Task};
use iced_fonts::lucide::{check, chevron_down, chevrons_down, pencil, plus, trash};

use crate::ui::i18n;
use crate::ui::theme as ui_theme;

/// Rows shown per page.
pub const PAGE: usize = 10;
const COLUMN_WIDTH: f32 = 160.0;
const ROW_HEIGHT: f32 = 26.0;

/// Messages produced by the result grid.
#[derive(Debug, Clone)]
pub enum ResultMessage {
    ToggleEdit,
    Select(usize),
    Cell(usize, usize, String),
    Insert,
    Delete,
    Post,
    FetchNext,
    FetchLast,
    /// Open the "…" value modal for a cell.
    OpenCellEditor(usize, usize),
    CellEditorAction(Action),
    CellEditorOk,
    CellEditorCancel,
    /// Start dragging the column resize handle.
    ResizeStart(usize),
    /// Pointer moved while a column resize drag is active (pointer x).
    ResizeMove(f32),
    /// Finish the drag.
    ResizeEnd,
    /// Horizontal scroll of the unified grid; moves the sticky header replica.
    ContentScrolled(Viewport),
}

/// Id of the sticky header replica's scrollable (synced one-way).
static REPLICA_SCROLL: LazyLock<Id> = LazyLock::new(Id::unique);
static GRID_SCROLL: LazyLock<Id> = LazyLock::new(Id::unique);

/// Mirrors the grid's horizontal offset onto the sticky header replica.
/// One-way: the replica has no on_scroll, so no feedback loop is possible.
/// Content widths and viewports are equal, so absolute x maps exactly.
pub fn handle_content_scrolled(viewport: Viewport) -> Task<ResultMessage> {
    scroll_to(
        REPLICA_SCROLL.clone(),
        AbsoluteOffset {
            x: Some(viewport.absolute_offset().x),
            y: None,
        },
    )
}

/// Header row builder shared by the in-grid header and the sticky replica —
/// identical widgets guarantee identical column geometry.
fn build_header(result: &QueryResult) -> iced::widget::Row<'static, ResultMessage> {
    row(
        std::iter::once(header_cell(None, "#".to_owned(), 28.0)).chain(
            result.columns.iter().enumerate().flat_map(|(col, name)| {
                // Fixed-height 1px separator (a Fill-height rule would blow up
                // to the whole stack in the unclamped replica layer).
                [
                    header_rule(),
                    header_cell(Some(col), name.clone(), result.col_width(col)),
                ]
            }),
        ),
    )
}

/// 1px header column separator, exactly one row tall.
fn header_rule() -> Element<'static, ResultMessage> {
    container(Space::new().width(1.0).height(ROW_HEIGHT))
        .style(|_| container::Style {
            background: Some(Background::Color(ui_theme::current().border)),
            ..container::Style::default()
        })
        .into()
}

/// Scrollable style with invisible scrollbars: the replica's bar would
/// duplicate the grid's own.
fn hidden_scrollbars(theme: &iced::Theme, status: scrollable::Status) -> scrollable::Style {
    let mut style = scrollable::default(theme, status);
    for rail in [&mut style.vertical_rail, &mut style.horizontal_rail] {
        rail.background = None;
        rail.scroller.background = Background::Color(iced::Color::TRANSPARENT);
        rail.border = Border::default();
    }
    style
}

/// Active column resize drag.
#[derive(Debug, Clone, Copy)]
pub struct ColumnDrag {
    pub col: usize,
    pub start_w: f32,
    /// Pointer x captured at the first move after press.
    pub anchor_x: Option<f32>,
}

/// Draft state while the cell value modal is open.
#[derive(Debug)]
pub struct CellEditorDraft {
    pub row: usize,
    pub col: usize,
    pub content: Content,
}

impl CellEditorDraft {
    /// Opens the modal for a cell with its current text `value`.
    pub fn open(row: usize, col: usize, value: &str) -> Self {
        Self {
            row,
            col,
            content: Content::with_text(value),
        }
    }

    pub fn perform_editor(&mut self, action: Action) {
        self.content.perform(action);
    }
}

/// Structured query result with pending edit state, stored per worksheet.
#[derive(Debug, Clone, Default)]
pub struct QueryResult {
    pub source_sql: String,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub edit_mode: bool,
    pub selected: Option<usize>,
    /// Cell edits keyed by (row index, column index).
    pub edits: HashMap<(usize, usize), String>,
    pub inserted: HashSet<usize>,
    pub deleted: HashSet<usize>,
    /// Per-column pixel widths (only present after a resize).
    pub col_widths: Vec<f32>,
}

impl QueryResult {
    /// Pixel width of a column (default when unset).
    pub fn col_width(&self, col: usize) -> f32 {
        self.col_widths.get(col).copied().unwrap_or(COLUMN_WIDTH)
    }

    /// Aligns `col_widths` with the column count so a resize can write into it.
    pub fn ensure_col_widths(&mut self) {
        let n = self.columns.len();
        if self.col_widths.len() != n {
            self.col_widths.resize(n, COLUMN_WIDTH);
        }
    }
    /// Current text of a cell: edit overlay wins over the fetched value.
    pub fn cell(&self, row_index: usize, col: usize) -> &str {
        self.edits
            .get(&(row_index, col))
            .map(String::as_str)
            .unwrap_or_else(|| {
                self.rows[row_index]
                    .get(col)
                    .map(String::as_str)
                    .unwrap_or("")
            })
    }

    /// True when any change would be posted.
    pub fn has_changes(&self) -> bool {
        !self.edits.is_empty() || !self.inserted.is_empty() || !self.deleted.is_empty()
    }
}

/// Renders the result tab: toolbar + paged grid, or a hint when empty.
pub fn view(result: &QueryResult) -> Element<'_, ResultMessage> {
    if result.columns.is_empty() {
        return container(
            text(i18n::t("result.empty"))
                .size(12)
                .color(ui_theme::current().muted),
        )
        .width(Fill)
        .height(Fill)
        .padding(Padding::from([8, 10]))
        .into();
    }

    let toolbar = row![
        grid_icon(
            pencil().size(14),
            i18n::t("result.edit_data"),
            result.edit_mode,
            ResultMessage::ToggleEdit
        ),
        grid_icon(
            plus().size(14),
            i18n::t("result.insert_record"),
            result.edit_mode,
            ResultMessage::Insert
        ),
        grid_icon(
            trash().size(14),
            i18n::t("result.delete_record"),
            result.edit_mode && result.selected.is_some(),
            ResultMessage::Delete,
        ),
        grid_icon(
            check().size(14),
            i18n::t("result.post_changes"),
            result.edit_mode && result.has_changes(),
            ResultMessage::Post,
        ),
        toolbar_divider(),
        grid_icon(
            chevron_down().size(14),
            i18n::t("result.fetch_next"),
            !result.source_sql.is_empty(),
            ResultMessage::FetchNext,
        ),
        grid_icon(
            chevrons_down().size(14),
            i18n::t("result.fetch_all"),
            !result.source_sql.is_empty(),
            ResultMessage::FetchLast,
        ),
        Space::new().width(Fill),
        text(i18n::t_args(
            "result.loaded",
            &[("n", result.rows.len().to_string().as_str())],
        ))
        .size(11)
        .color(ui_theme::current().muted),
    ]
    .spacing(4)
    .align_y(Vertical::Center)
    .padding(Padding::from([3, 8]));

    let header = build_header(result);
    let header_replica = build_header(result);

    let last = result.rows.len();
    let mut body = column![];
    // No leading rule: the header's own bottom border is the seam.
    for index in 0..last {
        body = body.push(data_row(result, index));
        body = body.push(horizontal_rule());
    }

    container(
        column![
            container(toolbar).style(|_| chrome_bar_style()),
            // Unified grid (borders match by construction) with a sticky
            // header replica pinned on top. The replica is synced one-way
            // from the grid's horizontal offset (ContentScrolled) and never
            // emits on_scroll itself, so no feedback loop is possible.
            stack![
                scrollable(
                    column![
                        // No width(Fill) here: a Fill sibling inside a
                        // compressed scrollable clamps every Shrink sibling
                        // (including the rows) to zero width.
                        container(header).style(|_| header_bar_style()),
                        body,
                    ]
                    .spacing(0)
                    .padding(Padding {
                        top: 0.0,
                        right: 4.0,
                        bottom: 2.0,
                        left: 4.0,
                    })
                )
                .id(GRID_SCROLL.clone())
                .height(Fill)
                .direction(scrollable::Direction::Both {
                    vertical: scrollable::Scrollbar::default(),
                    horizontal: scrollable::Scrollbar::default(),
                })
                .on_scroll(|viewport| ResultMessage::ContentScrolled(viewport)),
                scrollable(
                    // Same padding as the grid content so the replica sits
                    // exactly on top of the in-grid header.
                    container(header_replica)
                        .style(|_| header_bar_style())
                        .padding(Padding {
                            top: 0.0,
                            right: 4.0,
                            bottom: 0.0,
                            left: 4.0,
                        })
                )
                .id(REPLICA_SCROLL.clone())
                .width(Fill)
                .direction(scrollable::Direction::Horizontal(
                    scrollable::Scrollbar::default()
                ))
                .style(|theme, status| hidden_scrollbars(theme, status)),
            ]
            .width(Fill)
            .height(Fill),
        ]
        .spacing(0)
        .height(Fill),
    )
    .width(Fill)
    .height(Fill)
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

fn data_row(result: &QueryResult, index: usize) -> Element<'_, ResultMessage> {
    let selected = result.selected == Some(index);
    let marked = result.deleted.contains(&index) || result.inserted.contains(&index);
    let background = if selected {
        Some(Background::Color(ui_theme::current().hover))
    } else if marked {
        Some(Background::Color(ui_theme::current().sidebar_title_bg))
    } else {
        None
    };
    let text_color = if result.deleted.contains(&index) {
        ui_theme::error()
    } else if result.inserted.contains(&index) {
        ui_theme::success()
    } else {
        ui_theme::current().text
    };

    let mut cells = row![row_number_cell(index, marked)];
    for (col, _name) in result.columns.iter().enumerate() {
        let width = result.col_width(col);
        cells = cells.push(vertical_rule());
        let value = result.cell(index, col).to_string();
        // Buttonedit: input field + "..." button in every cell. Typing edits
        // the grid buffer only with Edit data on; "..." opens the modal.
        let input = text_input("", &value)
            .size(12)
            .width(width - 26.0)
            // ponytail: text_input has no .height(); its height is
            // line-height + padding, so padding is tuned to fill ROW_HEIGHT
            // exactly (26 = 14.3 + 5.85 * 2) for a seamless cell look.
            .padding(Padding {
                top: 5.85,
                bottom: 5.85,
                left: 5.0,
                right: 5.0,
            })
            .style(move |_, _| cell_input_style(text_color));
        let input = if result.edit_mode {
            input.on_input(move |text| ResultMessage::Cell(index, col, text))
        } else {
            input
        };
        let cell: Element<'_, ResultMessage> = row![input, cell_more_button(index, col)].into();
        cells = cells.push(cell);
    }

    mouse_area(
        container(cells)
            .width(Shrink)
            .height(ROW_HEIGHT)
            .style(move |_| container::Style {
                background,
                ..container::Style::default()
            }),
    )
    .on_press(ResultMessage::Select(index))
    .into()
}

/// Small "…" button opening the cell value modal (like in Parameters).
fn cell_more_button(index: usize, col: usize) -> Element<'static, ResultMessage> {
    tooltip(
        button(text("...").size(11).font(Font::MONOSPACE))
            // Fixed so the body cell matches the header cell exactly and
            // fills the full row height (same as the input).
            .width(26.0)
            .height(ROW_HEIGHT)
            .padding(Padding::from([7, 5]))
            .style(|_, status| ui_theme::chrome_button(status))
            .on_press(ResultMessage::OpenCellEditor(index, col)),
        text(i18n::t("result.edit_value")).size(11),
        tooltip::Position::FollowCursor,
    )
    .style(|_| ui_theme::tooltip_style())
    .into()
}

/// Modal value editor for a grid cell — same look as the Parameters one.
pub fn cell_editor_modal(draft: &CellEditorDraft) -> Element<'_, ResultMessage> {
    let form = column![
        text(i18n::t("result.edit_value")).size(16),
        container(
            text_editor(&draft.content)
                .height(Fill)
                .font(Font::MONOSPACE)
                .size(13)
                .padding(8)
                .on_action(ResultMessage::CellEditorAction),
        )
        .width(Fill)
        .height(280)
        .style(|_| container::Style {
            background: Some(Background::Color(ui_theme::current().panel_bg)),
            border: Border {
                color: ui_theme::current().border,
                width: 1.0,
                radius: 0.0.into(),
            },
            ..container::Style::default()
        }),
        row![
            button(text(i18n::t("params.editor.cancel")).size(13))
                .padding(Padding::from([8, 16]))
                .style(|_, status| ui_theme::chrome_button(status))
                .on_press(ResultMessage::CellEditorCancel),
            Space::new().width(Fill),
            button(text(i18n::t("params.editor.ok")).size(13))
                .padding(Padding::from([8, 16]))
                .style(|_, status| ui_theme::primary_button(status))
                .on_press(ResultMessage::CellEditorOk),
        ]
        .spacing(8),
    ]
    .spacing(10)
    .padding(16)
    .width(560);

    let card = container(form).style(|_| ui_theme::modal_card());
    let backdrop = mouse_area(
        container(Space::new().width(Fill).height(Fill))
            .width(Fill)
            .height(Fill)
            .style(|_| ui_theme::modal_backdrop()),
    )
    .on_press(ResultMessage::CellEditorCancel);

    opaque(
        stack![
            backdrop,
            opaque(container(card)
                .width(Fill)
                .height(Fill)
                .center_x(Fill)
                .center_y(Fill)),
        ]
        .width(Fill)
        .height(Fill),
    )
}

fn row_number_cell(index: usize, marked: bool) -> Element<'static, ResultMessage> {
    container(text(index + 1).size(11).color(if marked {
        ui_theme::current().text
    } else {
        ui_theme::current().muted
    }))
    .width(28.0)
    .height(Fill)
    .padding(Padding::from([5, 4]))
    .style(|_| container::Style {
        background: Some(Background::Color(ui_theme::current().sidebar_title_bg)),
        ..container::Style::default()
    })
    .into()
}

fn header_cell(index: Option<usize>, label: String, width: f32) -> Element<'static, ResultMessage> {
    let mut content = row![container(text(label).size(11))
        .width(width - 6.0)
        .height(Fill)
        .padding(Padding::from([4, 6]))
        .clip(true),];
    if let Some(col) = index {
        content = content.push(
            mouse_area(Space::new().width(6).height(Fill))
                .on_press(ResultMessage::ResizeStart(col))
                .interaction(mouse::Interaction::ResizingHorizontally),
        );
    }
    container(content)
        .width(width)
        .height(ROW_HEIGHT)
        .style(|_| container::Style {
            // Framed header cell: the border is drawn inside the width, so
            // column alignment with body cells is preserved.
            border: Border {
                color: ui_theme::current().border,
                width: 1.0,
                radius: 0.0.into(),
            },
            ..container::Style::default()
        })
        .into()
}

/// 1px horizontal separator between grid rows.
fn horizontal_rule() -> Element<'static, ResultMessage> {
    rule::horizontal(1).style(|_| rule_style()).into()
}

/// 1px vertical separator between grid cells.
fn vertical_rule() -> Element<'static, ResultMessage> {
    rule::vertical(1).style(|_| rule_style()).into()
}

/// Full-screen pointer catcher while a column resize drag is active.
pub fn drag_overlay() -> Element<'static, ResultMessage> {
    opaque(
        mouse_area(Space::new().width(Fill).height(Fill))
            .on_move(|point| ResultMessage::ResizeMove(point.x))
            .on_release(ResultMessage::ResizeEnd)
            .on_press(ResultMessage::ResizeEnd)
            .interaction(mouse::Interaction::ResizingHorizontally),
    )
}

fn toolbar_divider() -> Element<'static, ResultMessage> {
    container(rule::vertical(1).style(|_| rule_style()))
        .height(20)
        .into()
}

/// Icon toolbar button with tooltip; disabled (no press) when `enabled` is false.
fn grid_icon(
    icon: iced::widget::Text<'static>,
    tip: String,
    enabled: bool,
    message: ResultMessage,
) -> Element<'static, ResultMessage> {
    let mut btn = button(icon)
        .padding(Padding::from([4, 6]))
        .style(move |_, status| {
            if enabled {
                ui_theme::chrome_button(status)
            } else {
                ui_theme::disabled_button()
            }
        });
    if enabled {
        btn = btn.on_press(message);
    }
    tooltip(btn, text(tip).size(11), tooltip::Position::FollowCursor)
        .style(|_| ui_theme::tooltip_style())
        .into()
}

fn chrome_bar_style() -> container::Style {
    container::Style {
        background: Some(Background::Color(ui_theme::current().chrome_bg)),
        text_color: Some(ui_theme::current().text),
        border: Border {
            color: ui_theme::current().border,
            width: 0.0,
            radius: 0.0.into(),
        },
        ..container::Style::default()
    }
}

fn header_bar_style() -> container::Style {
    container::Style {
        background: Some(Background::Color(ui_theme::current().sidebar_title_bg)),
        text_color: Some(ui_theme::current().text),
        border: Border {
            color: ui_theme::current().border,
            width: 0.0,
            radius: 0.0.into(),
        },
        ..container::Style::default()
    }
}

fn rule_style() -> iced::widget::rule::Style {
    iced::widget::rule::Style {
        color: ui_theme::current().border,
        radius: 0.0.into(),
        fill_mode: iced::widget::rule::FillMode::Full,
        snap: true,
    }
}

fn cell_input_style(value_color: iced::Color) -> text_input::Style {
    text_input::Style {
        background: Background::Color(ui_theme::current().panel_bg),
        border: Border {
            color: ui_theme::current().border,
            width: 1.0,
            radius: 0.0.into(),
        },
        icon: ui_theme::current().muted,
        placeholder: ui_theme::current().muted,
        value: value_color,
        selection: iced::Color::from_rgba(0.3, 0.5, 0.8, 0.35),
    }
}

#[cfg(test)]
mod layout_probe {
    use super::*;
    use iced::Font;
    use iced_core::layout::{Limits, Node};
    use iced_core::widget::Tree;
    use iced_core::Size;

    #[test]
    fn probe() {
        let mut result = QueryResult::default();
        result.columns = (0..6).map(|i| format!("COL_{i}")).collect();
        result.rows = (0..50)
            .map(|r| (0..6).map(|c| format!("cell {r}/{c}")).collect())
            .collect();
        result.source_sql = "select 1".into();

        let mut element: Element<'_, ResultMessage> = view(&result);
        let renderer = iced_renderer::Renderer::Secondary(iced_tiny_skia::Renderer::new(
            Font::DEFAULT,
            12.0.into(),
        ));
        let mut tree = Tree::new(element.as_widget());
        let limits = Limits::new(Size::ZERO, Size::new(1200.0, 138.0));
        let node = element
            .as_widget_mut()
            .layout(&mut tree, &renderer, &limits);
        dump(&node, 0);
    }

    fn dump(node: &Node, depth: usize) {
        let pad = "  ".repeat(depth);
        println!(
            "{pad}size={}x{} children={}",
            node.size().width,
            node.size().height,
            node.children().len()
        );
        for child in node.children() {
            dump(child, depth + 1);
        }
    }
}
