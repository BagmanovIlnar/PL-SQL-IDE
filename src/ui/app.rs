//! IDE shell: menu, Connections/Objects, worksheets, completion, connection modal.

use std::sync::Arc;

use iced::alignment::{Horizontal, Vertical};
use iced::border::Border;
use iced::keyboard::key::Named;
use iced::keyboard::Key;
use iced::widget::operation::focus as focus_widget;
use iced::widget::text_editor::{Action, Binding, Content, Edit};
use iced::widget::{
    button, column, container, mouse_area, opaque, pick_list, row, rule, scrollable, stack, text,
    text_editor, text_input, Space,
};
use iced::{Background, Color, Element, Fill, Font, Padding, Task, Theme};

/// Stable widget id so the editor can keep / regain keyboard focus while the
/// completion overlay is visible.
const EDITOR_ID: &str = "worksheet-editor";

/// Editor font size in logical pixels (shared by text and line gutter).
const EDITOR_TEXT_SIZE: f32 = 14.0;

/// Absolute line height for the editor and the line-number gutter.
const EDITOR_LINE_HEIGHT: f32 = 20.0;

/// Padding around editor text; gutter top padding matches this.
const EDITOR_PADDING: f32 = 5.0;

/// How many line numbers to render in the virtualized gutter window.
const GUTTER_VISIBLE_LINES: usize = 120;
use iced_aw::menu::{DrawPath, Item, Menu, MenuBar};
use iced_aw::style::menu_bar as menu_bar_style;

use crate::core::postgres_spike::{run_postgres_spike, PostgresSpikeRequest};
use crate::core::{ConnectionId, ConnectionProfile, DbKind};
use crate::ui::completion::{suggest_keywords, word_prefix_at};
use crate::ui::theme as ui_theme;
use crate::ui::window_kind::WindowKind;

/// Runs the iced IDE window.
pub fn run() -> iced::Result {
    iced::application(App::new, update, view)
        .title("PLSQL IDE")
        .theme(|_: &App| Theme::Dark)
        .centered()
        .window_size((1400.0, 900.0))
        .run()
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BrowserNode {
    Schema(String),
    Folder { schema: String, kind: ObjectKind },
    Object { schema: String, kind: ObjectKind, name: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ObjectKind {
    Tables,
    Views,
    Procedures,
    Functions,
    Packages,
}

impl ObjectKind {
    fn label(self) -> &'static str {
        match self {
            Self::Tables => "Tables",
            Self::Views => "Views",
            Self::Procedures => "Procedures",
            Self::Functions => "Functions",
            Self::Packages => "Packages",
        }
    }
}

#[derive(Debug)]
struct Worksheet {
    kind: WindowKind,
    content: Content,
    /// Vertical scroll offset of the editor, in logical pixels.
    scroll_y: f32,
}

#[derive(Debug, Clone)]
struct ConnectionDraft {
    id: Option<ConnectionId>,
    name: String,
    db_kind: DbKind,
    host: String,
    port: String,
    database: String,
    username: String,
    password: String,
}

impl ConnectionDraft {
    fn from_new() -> Self {
        let profile = ConnectionProfile::new("New connection", DbKind::Postgres);
        Self::from_profile(&profile, true)
    }

    fn from_profile(profile: &ConnectionProfile, is_new: bool) -> Self {
        Self {
            id: if is_new { None } else { Some(profile.id) },
            name: profile.name.clone(),
            db_kind: profile.db_kind,
            host: profile.host.clone(),
            port: profile.port.to_string(),
            database: profile.database.clone(),
            username: profile.username.clone(),
            password: profile.password.clone(),
        }
    }

    fn into_profile(self) -> Result<ConnectionProfile, String> {
        let port = self
            .port
            .parse::<u16>()
            .map_err(|_| "Port must be a number".to_string())?;
        if self.name.trim().is_empty() {
            return Err("Name is required".into());
        }
        if self.host.trim().is_empty() {
            return Err("Host is required".into());
        }

        let mut profile = ConnectionProfile {
            id: self.id.unwrap_or_else(uuid::Uuid::new_v4),
            name: self.name.trim().to_string(),
            db_kind: self.db_kind,
            host: self.host.trim().to_string(),
            port,
            database: self.database.trim().to_string(),
            username: self.username.trim().to_string(),
            password: self.password,
        };
        if profile.database.is_empty() {
            profile.database = match profile.db_kind {
                DbKind::Postgres => "postgres".into(),
                DbKind::Oracle => "XEPDB1".into(),
            };
        }
        Ok(profile)
    }
}

#[derive(Debug)]
struct App {
    worksheets: Vec<Worksheet>,
    active_tab: usize,
    connections: Vec<ConnectionProfile>,
    selected_connection: Option<ConnectionId>,
    dialog: Option<ConnectionDraft>,
    dialog_error: Option<String>,
    completion: Vec<String>,
    completion_prefix: String,
    selected_completion: usize,
    busy: bool,
    metrics: String,
    last_output: String,
    selected_node: Option<BrowserNode>,
    expanded: Vec<String>,
    filter: String,
}

#[derive(Debug, Clone)]
enum Message {
    EditorAction(Action),
    FilterChanged(String),
    ActivateBrowser {
        node: BrowserNode,
        expand_key: Option<String>,
        insert_name: Option<String>,
    },
    SelectTab(usize),
    CloseTab(usize),
    MenuNew(WindowKind),
    MenuOpenStub(&'static str),
    MenuSaveStub,
    MenuSaveAsStub,
    MenuExit,
    MenuLoadLarge,
    MenuExecute,
    SelectConnection(ConnectionId),
    AddConnection,
    EditConnection,
    DeleteConnection,
    DialogNameChanged(String),
    DialogDbKindChanged(DbKind),
    DialogHostChanged(String),
    DialogPortChanged(String),
    DialogDatabaseChanged(String),
    DialogUsernameChanged(String),
    DialogPasswordChanged(String),
    DialogSave,
    DialogCancel,
    ApplyCompletion(String),
    /// Apply the currently highlighted completion item (Enter).
    ApplySelectedCompletion,
    /// Move the completion highlight by `delta` (−1 / +1).
    CompletionNavigate(i32),
    /// Close the completion popup without inserting (Escape).
    DismissCompletion,
    SpikeFinished(Result<String, String>),
}

impl App {
    fn new() -> Self {
        let mut local = ConnectionProfile::new("Local PostgreSQL", DbKind::Postgres);
        local.username = "postgres".into();
        local.password = "postgres".into();

        let mut oracle = ConnectionProfile::new("Local Oracle", DbKind::Oracle);
        oracle.username = "scott".into();

        let selected = local.id;
        Self {
            worksheets: vec![Worksheet {
                kind: WindowKind::Sql,
                content: Content::with_text(WindowKind::Sql.template()),
                scroll_y: 0.0,
            }],
            active_tab: 0,
            connections: vec![local, oracle],
            selected_connection: Some(selected),
            dialog: None,
            dialog_error: None,
            completion: Vec::new(),
            completion_prefix: String::new(),
            selected_completion: 0,
            busy: false,
            metrics: "Ready".into(),
            last_output: String::new(),
            selected_node: None,
            expanded: vec![
                "schema:PUBLIC".into(),
                "folder:PUBLIC:Tables".into(),
            ],
            filter: String::new(),
        }
    }

    fn active_content_mut(&mut self) -> Option<&mut Content> {
        self.worksheets
            .get_mut(self.active_tab)
            .map(|sheet| &mut sheet.content)
    }

    fn active_content(&self) -> Option<&Content> {
        self.worksheets
            .get(self.active_tab)
            .map(|sheet| &sheet.content)
    }

    fn selected_profile(&self) -> Option<&ConnectionProfile> {
        let id = self.selected_connection?;
        self.connections.iter().find(|item| item.id == id)
    }

    fn open_window(&mut self, kind: WindowKind) {
        self.worksheets.push(Worksheet {
            kind,
            content: Content::with_text(kind.template()),
            scroll_y: 0.0,
        });
        self.active_tab = self.worksheets.len() - 1;
        self.completion.clear();
        self.metrics = format!("Opened {}", kind.tab_title());
    }

    fn active_scroll_y_mut(&mut self) -> Option<&mut f32> {
        self.worksheets
            .get_mut(self.active_tab)
            .map(|sheet| &mut sheet.scroll_y)
    }

    fn is_expanded(&self, key: &str) -> bool {
        self.expanded.iter().any(|item| item == key)
    }

    fn toggle_expanded(&mut self, key: String) {
        if let Some(index) = self.expanded.iter().position(|item| item == &key) {
            self.expanded.remove(index);
        } else {
            self.expanded.push(key);
        }
    }

    fn refresh_completion(&mut self) {
        self.completion.clear();
        self.completion_prefix.clear();
        self.selected_completion = 0;

        let Some(content) = self.active_content() else {
            return;
        };
        let cursor = content.cursor().position;
        let Some(line) = content.line(cursor.line) else {
            return;
        };
        let Some(prefix) = word_prefix_at(&line.text, cursor.line, cursor.column) else {
            return;
        };
        self.completion = suggest_keywords(&prefix, 8);
        self.completion_prefix = prefix;
        // Always highlight the first suggestion when the list appears / changes.
        self.selected_completion = 0;
    }

    fn apply_selected_completion(&mut self) -> bool {
        let Some(word) = self.completion.get(self.selected_completion).cloned() else {
            return false;
        };
        self.apply_completion_word(word);
        true
    }

    fn navigate_completion(&mut self, delta: i32) {
        let len = self.completion.len();
        if len == 0 {
            return;
        }
        let current = self.selected_completion as i32;
        let next = (current + delta).clamp(0, (len - 1) as i32);
        self.selected_completion = next as usize;
    }

    fn apply_completion_word(&mut self, word: String) {
        let prefix_len = self.completion_prefix.len();
        let Some(content) = self.active_content_mut() else {
            return;
        };
        for _ in 0..prefix_len {
            content.perform(Action::Edit(Edit::Backspace));
        }
        content.perform(Action::Edit(Edit::Paste(Arc::new(word))));
        self.completion.clear();
        self.completion_prefix.clear();
        self.selected_completion = 0;
    }
}

fn update(app: &mut App, message: Message) -> Task<Message> {
    match message {
        Message::EditorAction(action) => {
            if let Action::Scroll { lines } = &action {
                if let Some(scroll_y) = app.active_scroll_y_mut() {
                    *scroll_y = (*scroll_y + *lines as f32 * EDITOR_LINE_HEIGHT).max(0.0);
                }
            }

            let mut restored_empty = false;
            if let Some(content) = app.active_content_mut() {
                content.perform(action);
                // After Select-All + Delete (or clearing the last chars), cosmic-text
                // can leave scroll past the last line; highlighter then panics with
                // "Line layout should be cached" / slice OOB. Recreate empty content.
                if content.is_empty() {
                    *content = Content::new();
                    restored_empty = true;
                }
            }
            if restored_empty {
                if let Some(scroll_y) = app.active_scroll_y_mut() {
                    *scroll_y = 0.0;
                }
            }
            app.refresh_completion();
            // Keep keyboard focus when the completion overlay appears or content
            // was rebuilt after a full clear.
            if restored_empty || !app.completion.is_empty() {
                focus_widget(EDITOR_ID)
            } else {
                Task::none()
            }
        }
        Message::FilterChanged(value) => {
            app.filter = value;
            Task::none()
        }
        Message::ActivateBrowser {
            node,
            expand_key,
            insert_name,
        } => {
            app.selected_node = Some(node);
            if let Some(key) = expand_key {
                app.toggle_expanded(key);
            }
            if let Some(name) = insert_name {
                if let Some(content) = app.active_content_mut() {
                    let mut current = content.text();
                    if !current.ends_with('\n') && !current.is_empty() {
                        current.push(' ');
                    }
                    current.push_str(&name);
                    *content = Content::with_text(&current);
                    app.metrics = format!("Inserted `{name}`");
                    app.completion.clear();
                }
            }
            Task::none()
        }
        Message::SelectTab(index) => {
            if index < app.worksheets.len() {
                app.active_tab = index;
                app.completion.clear();
            }
            Task::none()
        }
        Message::CloseTab(index) => {
            if app.worksheets.len() <= 1 || index >= app.worksheets.len() {
                return Task::none();
            }
            app.worksheets.remove(index);
            if app.active_tab >= app.worksheets.len() {
                app.active_tab = app.worksheets.len() - 1;
            } else if index < app.active_tab {
                app.active_tab -= 1;
            }
            app.completion.clear();
            Task::none()
        }
        Message::MenuNew(kind) => {
            app.open_window(kind);
            Task::none()
        }
        Message::MenuOpenStub(label) => {
            app.metrics = format!("{label}: open dialog not implemented yet");
            Task::none()
        }
        Message::MenuSaveStub => {
            app.metrics = "Save: not implemented yet".into();
            Task::none()
        }
        Message::MenuSaveAsStub => {
            app.metrics = "Save As: not implemented yet".into();
            Task::none()
        }
        Message::MenuExit => iced::exit(),
        Message::MenuLoadLarge => {
            let started = std::time::Instant::now();
            let document = build_large_sql_document(100_000);
            let lines = document.lines().count();
            if let Some(sheet) = app.worksheets.get_mut(app.active_tab) {
                sheet.content = Content::with_text(&document);
                sheet.scroll_y = 0.0;
            }
            app.completion.clear();
            app.metrics = format!("Loaded {lines} lines in {} ms", started.elapsed().as_millis());
            Task::none()
        }
        Message::MenuExecute => run_spike(app),
        Message::SelectConnection(id) => {
            app.selected_connection = Some(id);
            if let Some(profile) = app.selected_profile() {
                app.metrics = format!("Selected {}", profile.name);
            }
            Task::none()
        }
        Message::AddConnection => {
            app.dialog = Some(ConnectionDraft::from_new());
            app.dialog_error = None;
            Task::none()
        }
        Message::EditConnection => {
            if let Some(profile) = app.selected_profile().cloned() {
                app.dialog = Some(ConnectionDraft::from_profile(&profile, false));
                app.dialog_error = None;
            } else {
                app.metrics = "Select a connection to edit".into();
            }
            Task::none()
        }
        Message::DeleteConnection => {
            let Some(id) = app.selected_connection else {
                app.metrics = "Select a connection to delete".into();
                return Task::none();
            };
            app.connections.retain(|item| item.id != id);
            app.selected_connection = app.connections.first().map(|item| item.id);
            app.metrics = "Connection deleted".into();
            Task::none()
        }
        Message::DialogNameChanged(value) => {
            if let Some(dialog) = &mut app.dialog {
                dialog.name = value;
            }
            Task::none()
        }
        Message::DialogDbKindChanged(kind) => {
            if let Some(dialog) = &mut app.dialog {
                dialog.db_kind = kind;
                dialog.port = match kind {
                    DbKind::Postgres => "5432".into(),
                    DbKind::Oracle => "1521".into(),
                };
            }
            Task::none()
        }
        Message::DialogHostChanged(value) => {
            if let Some(dialog) = &mut app.dialog {
                dialog.host = value;
            }
            Task::none()
        }
        Message::DialogPortChanged(value) => {
            if let Some(dialog) = &mut app.dialog {
                dialog.port = value;
            }
            Task::none()
        }
        Message::DialogDatabaseChanged(value) => {
            if let Some(dialog) = &mut app.dialog {
                dialog.database = value;
            }
            Task::none()
        }
        Message::DialogUsernameChanged(value) => {
            if let Some(dialog) = &mut app.dialog {
                dialog.username = value;
            }
            Task::none()
        }
        Message::DialogPasswordChanged(value) => {
            if let Some(dialog) = &mut app.dialog {
                dialog.password = value;
            }
            Task::none()
        }
        Message::DialogCancel => {
            app.dialog = None;
            app.dialog_error = None;
            Task::none()
        }
        Message::DialogSave => {
            let Some(draft) = app.dialog.clone() else {
                return Task::none();
            };
            match draft.into_profile() {
                Ok(profile) => {
                    let id = profile.id;
                    if let Some(index) = app.connections.iter().position(|item| item.id == id) {
                        app.connections[index] = profile;
                        app.metrics = "Connection updated".into();
                    } else {
                        app.connections.push(profile);
                        app.metrics = "Connection added".into();
                    }
                    app.selected_connection = Some(id);
                    app.dialog = None;
                    app.dialog_error = None;
                }
                Err(error) => {
                    app.dialog_error = Some(error);
                }
            }
            Task::none()
        }
        Message::ApplyCompletion(word) => {
            app.apply_completion_word(word);
            focus_widget(EDITOR_ID)
        }
        Message::ApplySelectedCompletion => {
            if app.apply_selected_completion() {
                focus_widget(EDITOR_ID)
            } else {
                Task::none()
            }
        }
        Message::CompletionNavigate(delta) => {
            app.navigate_completion(delta);
            Task::none()
        }
        Message::DismissCompletion => {
            app.completion.clear();
            app.completion_prefix.clear();
            app.selected_completion = 0;
            focus_widget(EDITOR_ID)
        }
        Message::SpikeFinished(result) => {
            app.busy = false;
            match result {
                Ok(output) => {
                    app.last_output = output;
                    app.metrics = "Execute completed".into();
                }
                Err(error) => {
                    app.last_output = error;
                    app.metrics = "Execute failed".into();
                }
            }
            Task::none()
        }
    }
}

fn run_spike(app: &mut App) -> Task<Message> {
    if app.busy {
        return Task::none();
    }
    let Some(profile) = app.selected_profile().cloned() else {
        app.metrics = "Select a connection first".into();
        return Task::none();
    };

    app.busy = true;
    app.metrics = format!("Executing on {}…", profile.name);
    app.last_output = "Running…".into();

    let sql = app.active_content().map(|content| content.text());

    match profile.db_kind {
        DbKind::Postgres => {
            let request = PostgresSpikeRequest {
                connection_url: profile.postgres_url(),
                sql,
                timeout_ms: Some(10_000),
                cancel: false,
            };
            Task::perform(
                async move {
                    run_postgres_spike(request)
                        .await
                        .map(|result| result.format_for_ui())
                        .map_err(|err| err.to_string())
                },
                Message::SpikeFinished,
            )
        }
        DbKind::Oracle => {
            #[cfg(feature = "oracle")]
            {
                use crate::core::oracle_spike::{run_oracle_spike, OracleSpikeRequest};
                let request = OracleSpikeRequest {
                    connect_descriptor: profile.oracle_connect_descriptor(),
                    username: profile.username,
                    password: profile.password,
                    sql,
                    timeout_ms: Some(10_000),
                    cancel: false,
                };
                Task::perform(
                    async move {
                        run_oracle_spike(request)
                            .await
                            .map(|result| result.format_for_ui())
                            .map_err(|err| err.to_string())
                    },
                    Message::SpikeFinished,
                )
            }
            #[cfg(not(feature = "oracle"))]
            {
                app.busy = false;
                app.last_output =
                    "Oracle feature is disabled. Rebuild with --features oracle".into();
                app.metrics = "Execute failed".into();
                Task::none()
            }
        }
    }
}

fn view(app: &App) -> Element<'_, Message> {
    let shell = column![
        menu_bar(),
        rule::horizontal(1).style(|_| rule_style()),
        row![
            left_browser(app),
            rule::vertical(1).style(|_| rule_style()),
            column![worksheet_tabs(app), editor_pane(app),]
                .width(Fill)
                .height(Fill),
        ]
        .height(Fill),
        rule::horizontal(1).style(|_| rule_style()),
        status_bar(app),
    ]
    .height(Fill);

    let base = container(shell)
        .width(Fill)
        .height(Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(ui_theme::WINDOW_BG)),
            text_color: Some(ui_theme::TEXT),
            ..container::Style::default()
        });

    if app.dialog.is_some() {
        stack![base, connection_modal(app)]
            .width(Fill)
            .height(Fill)
            .into()
    } else {
        base.into()
    }
}

fn menu_bar() -> Element<'static, Message> {
    let menu_root = |label: &'static str| {
        button(text(label).size(14))
            .padding(Padding::from([6, 14]))
            .style(move |_, status| menu_button_style(status))
    };

    let menu_item = |label: &'static str| {
        button(
            container(text(label).size(14))
                .width(Fill)
                .align_x(Horizontal::Left),
        )
        .width(Fill)
        .padding(Padding::from([7, 12]))
        .style(move |_, status| menu_item_button_style(status))
    };

    let program_menu = Menu::new(vec![
        Item::new(menu_item("Blank").on_press(Message::MenuNew(WindowKind::Blank))),
        Item::new(menu_item("Function").on_press(Message::MenuNew(WindowKind::Function))),
        Item::new(menu_item("Java Source").on_press(Message::MenuNew(WindowKind::JavaSource))),
        Item::new(menu_item("Package").on_press(Message::MenuNew(WindowKind::Package))),
        Item::new(menu_item("Package Body").on_press(Message::MenuNew(WindowKind::PackageBody))),
        Item::new(
            menu_item("Package Specification")
                .on_press(Message::MenuNew(WindowKind::PackageSpecification)),
        ),
        Item::new(menu_item("Procedure").on_press(Message::MenuNew(WindowKind::Procedure))),
        Item::new(menu_item("Trigger").on_press(Message::MenuNew(WindowKind::Trigger))),
        Item::new(menu_item("Type").on_press(Message::MenuNew(WindowKind::Type))),
        Item::new(menu_item("Type Body").on_press(Message::MenuNew(WindowKind::TypeBody))),
        Item::new(
            menu_item("Type Specification")
                .on_press(Message::MenuNew(WindowKind::TypeSpecification)),
        ),
    ])
    .max_width(240.0)
    .offset(2.0);

    let new_menu = Menu::new(vec![
        Item::with_menu(menu_item("Program Window"), program_menu),
        Item::new(menu_item("Test Window").on_press(Message::MenuNew(WindowKind::Test))),
        Item::new(menu_item("SQL Window").on_press(Message::MenuNew(WindowKind::Sql))),
        Item::new(menu_item("Command Window").on_press(Message::MenuNew(WindowKind::Command))),
        Item::new(
            menu_item("Explain Plan Window").on_press(Message::MenuNew(WindowKind::ExplainPlan)),
        ),
    ])
    .max_width(240.0)
    .offset(2.0);

    let open_menu = Menu::new(vec![
        Item::new(menu_item("Program File").on_press(Message::MenuOpenStub("Program File"))),
        Item::new(menu_item("Test Script").on_press(Message::MenuOpenStub("Test Script"))),
        Item::new(menu_item("SQL Script").on_press(Message::MenuOpenStub("SQL Script"))),
        Item::new(menu_item("Command File").on_press(Message::MenuOpenStub("Command File"))),
    ])
    .max_width(200.0)
    .offset(2.0);

    let file_menu = Menu::new(vec![
        Item::with_menu(menu_item("New"), new_menu),
        Item::with_menu(menu_item("Open"), open_menu),
        Item::new(menu_item("Save").on_press(Message::MenuSaveStub)),
        Item::new(menu_item("Save As").on_press(Message::MenuSaveAsStub)),
        Item::new(menu_item("Load 100k lines (gate)").on_press(Message::MenuLoadLarge)),
        Item::new(menu_item("Exit").on_press(Message::MenuExit)),
    ])
    .max_width(260.0)
    .offset(2.0);

    let session_menu = Menu::new(vec![Item::new(
        menu_item("Execute").on_press(Message::MenuExecute),
    )])
    .max_width(200.0)
    .offset(2.0);

    let bar = MenuBar::new(vec![
        Item::with_menu(menu_root("File"), file_menu),
        Item::with_menu(menu_root("Session"), session_menu),
        Item::new(menu_root("Tools")),
        Item::new(menu_root("Help")),
    ])
    .width(Fill)
    // Backdrop paints accent behind the open submenu path (items with children).
    .draw_path(DrawPath::Backdrop)
    .style(|_, _| menu_bar_style::Style {
        bar_background: Background::Color(ui_theme::CHROME_BG),
        bar_border: Border {
            radius: 0.0.into(),
            ..Border::default()
        },
        bar_shadow: iced::Shadow::default(),
        menu_background: Background::Color(ui_theme::CHROME_BG),
        menu_border: Border {
            color: ui_theme::BORDER,
            width: 1.0,
            radius: 0.0.into(),
        },
        menu_shadow: iced::Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.45),
            offset: iced::Vector::new(0.0, 4.0),
            blur_radius: 12.0,
        },
        path: Background::Color(ui_theme::ACCENT),
        path_border: Border {
            radius: 0.0.into(),
            ..Border::default()
        },
    });

    container(bar)
        .padding(Padding::from([2, 4]))
        .width(Fill)
        .style(|_| chrome_style())
        .into()
}

fn left_browser(app: &App) -> Element<'_, Message> {
    let mut list = column![].spacing(2);
    for profile in &app.connections {
        let selected = app.selected_connection == Some(profile.id);
        let id = profile.id;
        let label = profile.summary();
        list = list.push(
            button(
                container(text(label).size(12).font(Font::MONOSPACE))
                    .width(Fill)
                    .align_x(Horizontal::Left),
            )
            .width(Fill)
            .padding(Padding::from([8, 10]))
            .style(move |_, status| connection_row_style(selected, status))
            .on_press(Message::SelectConnection(id)),
        );
    }

    let connections = column![
        section_title("Connections"),
        rule::horizontal(1).style(|_| rule_style()),
        scrollable(list.padding(Padding::from([6, 6]))).height(Fill),
        rule::horizontal(1).style(|_| rule_style()),
        container(
            row![
                button(text("Add").size(12))
                    .padding(Padding::from([6, 10]))
                    .on_press(Message::AddConnection),
                button(text("Edit").size(12))
                    .padding(Padding::from([6, 10]))
                    .on_press(Message::EditConnection),
                button(text("Delete").size(12))
                    .padding(Padding::from([6, 10]))
                    .on_press(Message::DeleteConnection),
            ]
            .spacing(6)
        )
        .padding(8)
        .width(Fill),
    ]
    .height(Fill)
    .width(Fill);

    let objects = column![
        section_title("Objects"),
        rule::horizontal(1).style(|_| rule_style()),
        container(
            text_input("Filter objects…", &app.filter)
                .on_input(Message::FilterChanged)
                .padding(6)
                .size(13),
        )
        .padding(Padding {
            top: 6.0,
            right: 8.0,
            bottom: 4.0,
            left: 8.0,
        }),
        scrollable(
            column![
                browser_row(
                    app,
                    "▾  PUBLIC".into(),
                    BrowserNode::Schema("PUBLIC".into()),
                    Some("schema:PUBLIC".into()),
                ),
                object_folder(
                    app,
                    "PUBLIC",
                    ObjectKind::Tables,
                    &["EMPLOYEES", "DEPARTMENTS", "DUAL"]
                ),
                object_folder(app, "PUBLIC", ObjectKind::Views, &["USER_TABLES", "ALL_OBJECTS"]),
                object_folder(app, "PUBLIC", ObjectKind::Procedures, &["P_REFRESH", "P_CALC"]),
                object_folder(app, "PUBLIC", ObjectKind::Functions, &["F_GET_NAME", "NVL"]),
                object_folder(app, "PUBLIC", ObjectKind::Packages, &["DBMS_OUTPUT", "APP_UTILS"]),
            ]
            .spacing(1)
            .padding(Padding {
                top: 0.0,
                right: 4.0,
                bottom: 8.0,
                left: 4.0,
            }),
        )
        .height(Fill),
    ]
    .height(Fill)
    .width(Fill);

    container(
        column![
            container(connections).height(Fill),
            rule::horizontal(1).style(|_| rule_style()),
            container(objects).height(Fill),
        ]
        .height(Fill),
    )
    .width(300)
    .height(Fill)
    .style(|_| container::Style {
        background: Some(Background::Color(ui_theme::SIDEBAR_BG)),
        text_color: Some(ui_theme::TEXT),
        ..container::Style::default()
    })
    .into()
}

fn connection_modal(app: &App) -> Element<'_, Message> {
    let draft = app.dialog.as_ref().expect("modal open");
    let title = if draft.id.is_some() {
        "Edit connection"
    } else {
        "Add connection"
    };

    let db_kinds = [DbKind::Oracle, DbKind::Postgres];

    let form = column![
        text(title).size(18),
        labeled_input("Name", &draft.name, Message::DialogNameChanged),
        text("Database type").size(12).color(ui_theme::MUTED),
        pick_list(db_kinds, Some(draft.db_kind), Message::DialogDbKindChanged)
            .padding(8)
            .width(Fill),
        labeled_input("Host / address", &draft.host, Message::DialogHostChanged),
        labeled_input("Port", &draft.port, Message::DialogPortChanged),
        labeled_input(
            if draft.db_kind == DbKind::Oracle {
                "Service name / SID"
            } else {
                "Database"
            },
            &draft.database,
            Message::DialogDatabaseChanged,
        ),
        labeled_input("Username", &draft.username, Message::DialogUsernameChanged),
        {
            text_input("Password", &draft.password)
                .on_input(Message::DialogPasswordChanged)
                .secure(true)
                .padding(8)
        },
        if let Some(error) = &app.dialog_error {
            text(error.clone()).size(12).color(Color::from_rgb(1.0, 0.45, 0.45))
        } else {
            text("").size(12)
        },
        row![
            button(text("Cancel").size(13))
                .padding(Padding::from([8, 16]))
                .on_press(Message::DialogCancel),
            Space::new().width(Fill),
            button(text("Save").size(13))
                .padding(Padding::from([8, 16]))
                .style(|_, status| primary_button_style(status))
                .on_press(Message::DialogSave),
        ]
        .spacing(8),
    ]
    .spacing(8)
    .padding(20)
    .width(420);

    let card = container(form).style(|_| container::Style {
        background: Some(Background::Color(ui_theme::PANEL_BG)),
        border: Border {
            color: ui_theme::BORDER,
            width: 1.0,
            radius: 4.0.into(),
        },
        text_color: Some(ui_theme::TEXT),
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
    .on_press(Message::DialogCancel);

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

fn labeled_input<'a>(
    label: &'a str,
    value: &str,
    on_input: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    column![
        text(label).size(12).color(ui_theme::MUTED),
        text_input(label, value).on_input(on_input).padding(8),
    ]
    .spacing(4)
    .into()
}

fn section_title(title: &'static str) -> Element<'static, Message> {
    container(text(title).size(13))
        .width(Fill)
        .padding(Padding {
            top: 8.0,
            right: 10.0,
            bottom: 8.0,
            left: 10.0,
        })
        .style(|_| container::Style {
            background: Some(Background::Color(ui_theme::CHROME_BG)),
            text_color: Some(ui_theme::TEXT),
            ..container::Style::default()
        })
        .into()
}

fn object_folder<'a>(
    app: &'a App,
    schema: &str,
    kind: ObjectKind,
    names: &[&str],
) -> Element<'a, Message> {
    let key = format!("folder:{schema}:{}", kind.label());
    let expanded = app.is_expanded(&key);
    let marker = if expanded { "▾" } else { "▸" };
    let folder_node = BrowserNode::Folder {
        schema: schema.into(),
        kind,
    };

    let mut children = column![browser_row(
        app,
        format!("{marker}  {}", kind.label()),
        folder_node,
        Some(key.clone()),
    )];

    if expanded {
        for name in names {
            if !app.filter.is_empty()
                && !name.to_lowercase().contains(&app.filter.to_lowercase())
            {
                continue;
            }
            children = children.push(browser_row(
                app,
                format!("    {name}"),
                BrowserNode::Object {
                    schema: schema.into(),
                    kind,
                    name: (*name).into(),
                },
                None,
            ));
        }
    }

    children.into()
}

fn browser_row<'a>(
    app: &'a App,
    label: String,
    node: BrowserNode,
    expand_key: Option<String>,
) -> Element<'a, Message> {
    let selected = app.selected_node.as_ref() == Some(&node);
    let insert_name = match &node {
        BrowserNode::Object { name, .. } => Some(name.clone()),
        _ => None,
    };

    let row_body = container(text(label).size(13).font(Font::MONOSPACE))
        .width(Fill)
        .padding(Padding::from([4, 8]))
        .style(move |_| {
            if selected {
                container::Style {
                    background: Some(Background::Color(ui_theme::ACCENT)),
                    text_color: Some(Color::WHITE),
                    ..container::Style::default()
                }
            } else {
                container::Style {
                    background: Some(Background::Color(Color::TRANSPARENT)),
                    text_color: Some(ui_theme::TEXT),
                    ..container::Style::default()
                }
            }
        });

    mouse_area(row_body)
        .on_press(Message::ActivateBrowser {
            node,
            expand_key,
            insert_name,
        })
        .into()
}

fn worksheet_tabs(app: &App) -> Element<'_, Message> {
    let mut tabs = row![].spacing(4).align_y(Vertical::Center);

    for (index, sheet) in app.worksheets.iter().enumerate() {
        let active = index == app.active_tab;
        let title = sheet.kind.tab_title();
        let tab = button(text(title).size(12))
            .padding(Padding::from([7, 12]))
            .style(move |_, status| tab_button_style(active, status))
            .on_press(Message::SelectTab(index));

        let close = button(text("×").size(12))
            .padding(Padding::from([7, 8]))
            .style(move |_, status| tab_button_style(active, status))
            .on_press(Message::CloseTab(index));

        tabs = tabs.push(row![tab, close].spacing(0));
    }

    container(
        row![
            tabs,
            Space::new().width(Fill),
            text(&app.metrics).size(12).color(ui_theme::MUTED),
        ]
        .align_y(Vertical::Center)
        .padding(Padding::from([4, 8])),
    )
    .width(Fill)
    .style(|_| chrome_style())
    .into()
}

/// Builds the virtualized line-number gutter for the SQL editor.
///
/// Only a window of numbers around the current scroll position is created, so
/// large documents (e.g. the 100k-line gate) stay responsive.
///
/// # Examples
///
/// ```ignore
/// // Shown as the left column next to `text_editor` in `editor_pane`.
/// let gutter = line_number_gutter(42, 0.0);
/// ```
fn line_number_gutter(line_count: usize, scroll_y: f32) -> Element<'static, Message> {
    let line_count = line_count.max(1);
    let max_first = line_count.saturating_sub(1);
    let first = ((scroll_y / EDITOR_LINE_HEIGHT).floor() as usize).min(max_first);
    let last = (first + GUTTER_VISIBLE_LINES).min(line_count);
    let pixel_offset = scroll_y - first as f32 * EDITOR_LINE_HEIGHT;

    let digits = ((line_count as f32).log10().floor() as usize) + 1;
    let gutter_width = (digits as f32 * 9.0 + 20.0).clamp(36.0, 64.0);

    let mut numbers = column![].spacing(0);
    for number in (first + 1)..=last {
        numbers = numbers.push(
            container(
                text(format!("{number}"))
                    .size(EDITOR_TEXT_SIZE)
                    .font(Font::MONOSPACE)
                    .color(ui_theme::MUTED),
            )
            .width(Fill)
            .height(EDITOR_LINE_HEIGHT)
            .align_x(Horizontal::Right)
            .align_y(Vertical::Center)
            .padding(Padding {
                top: 0.0,
                right: 8.0,
                bottom: 0.0,
                left: 4.0,
            }),
        );
    }

    container(
        container(numbers).padding(Padding {
            top: (EDITOR_PADDING - pixel_offset).max(0.0),
            right: 0.0,
            bottom: 0.0,
            left: 0.0,
        }),
    )
    .width(gutter_width)
    .height(Fill)
    .clip(true)
    .style(|_| container::Style {
        background: Some(Background::Color(ui_theme::SIDEBAR_BG)),
        border: Border {
            color: ui_theme::BORDER,
            width: 1.0,
            radius: 0.0.into(),
        },
        text_color: Some(ui_theme::MUTED),
        ..container::Style::default()
    })
    .into()
}

fn editor_pane(app: &App) -> Element<'_, Message> {
    let sheet = app
        .worksheets
        .get(app.active_tab)
        .expect("at least one worksheet");
    let syntax = sheet.kind.syntax();
    let line_count = sheet.content.line_count().max(1);
    let scroll_y = sheet.scroll_y;

    let completion_open = !app.completion.is_empty();
    let editor = text_editor(&sheet.content)
        .id(EDITOR_ID)
        .height(Fill)
        .font(Font::MONOSPACE)
        .size(EDITOR_TEXT_SIZE)
        .line_height(iced::widget::text::LineHeight::Absolute(
            EDITOR_LINE_HEIGHT.into(),
        ))
        .padding(EDITOR_PADDING)
        .highlight(syntax, iced::highlighter::Theme::Base16Ocean)
        .style(|_, status| editor_style(status))
        .key_binding(move |key_press| {
            if completion_open {
                match key_press.key.as_ref() {
                    Key::Named(Named::Enter) => {
                        return Some(Binding::Custom(Message::ApplySelectedCompletion));
                    }
                    Key::Named(Named::ArrowUp) => {
                        return Some(Binding::Custom(Message::CompletionNavigate(-1)));
                    }
                    Key::Named(Named::ArrowDown) => {
                        return Some(Binding::Custom(Message::CompletionNavigate(1)));
                    }
                    Key::Named(Named::Escape) => {
                        return Some(Binding::Custom(Message::DismissCompletion));
                    }
                    _ => {}
                }
            }
            Binding::from_key_press(key_press)
        })
        .on_action(Message::EditorAction);

    let editor_body = row![
        line_number_gutter(line_count, scroll_y),
        container(editor)
            .width(Fill)
            .height(Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(ui_theme::PANEL_BG)),
                ..container::Style::default()
            }),
    ]
    .height(Fill)
    .width(Fill);

    let editor_layer = container(editor_body)
        .padding(1)
        .width(Fill)
        .height(Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(ui_theme::PANEL_BG)),
            border: Border {
                color: ui_theme::BORDER,
                width: 1.0,
                radius: 0.0.into(),
            },
            ..container::Style::default()
        });

    // Keep the stack structure always the same so the editor widget path
    // (and its focus state) is not reset when suggestions appear.
    let overlay: Element<'_, Message> = if app.completion.is_empty() {
        Space::new().width(0).height(0).into()
    } else {
        let mut items = column![].spacing(1);
        for (index, word) in app.completion.iter().enumerate() {
            let selected = index == app.selected_completion;
            let word_clone = word.clone();
            // mouse_area (not button) — buttons are focusable and steal typing.
            items = items.push(
                mouse_area(
                    container(text(word.as_str()).size(13).font(Font::MONOSPACE))
                        .width(220)
                        .padding(Padding::from([4, 8]))
                        .style(move |_| completion_item_container_style(selected)),
                )
                .on_press(Message::ApplyCompletion(word_clone)),
            );
        }

        let popup = container(items)
            .width(220)
            .padding(4)
            .style(|_| container::Style {
                background: Some(Background::Color(ui_theme::CHROME_BG)),
                border: Border {
                    color: ui_theme::ACCENT,
                    width: 1.0,
                    radius: 2.0.into(),
                },
                text_color: Some(ui_theme::TEXT),
                ..container::Style::default()
            });

        container(popup)
            .padding(Padding {
                top: 48.0,
                right: 0.0,
                bottom: 0.0,
                left: 24.0,
            })
            .into()
    };

    stack![editor_layer, overlay]
        .width(Fill)
        .height(Fill)
        .into()
}

fn status_bar(app: &App) -> Element<'_, Message> {
    let connection = app
        .selected_profile()
        .map(|profile| profile.summary())
        .unwrap_or_else(|| "no connection selected".into());
    let detail = if app.last_output.is_empty() {
        app.metrics.clone()
    } else {
        app.last_output.lines().next().unwrap_or("").to_string()
    };

    container(
        row![
            text(format!("Connection: {connection}")).size(12),
            Space::new().width(Fill),
            text(detail).size(12).color(ui_theme::MUTED),
        ]
        .spacing(10)
        .align_y(Vertical::Center)
        .padding(Padding::from([4, 10])),
    )
    .width(Fill)
    .style(|_| chrome_style())
    .into()
}

fn chrome_style() -> container::Style {
    container::Style {
        background: Some(Background::Color(ui_theme::CHROME_BG)),
        text_color: Some(ui_theme::TEXT),
        ..container::Style::default()
    }
}

fn rule_style() -> iced::widget::rule::Style {
    iced::widget::rule::Style {
        color: ui_theme::BORDER,
        radius: 0.0.into(),
        fill_mode: iced::widget::rule::FillMode::Full,
        snap: true,
    }
}

fn menu_button_style(status: iced::widget::button::Status) -> iced::widget::button::Style {
    match status {
        iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed => {
            iced::widget::button::Style {
                background: Some(Background::Color(ui_theme::ACCENT)),
                text_color: Color::WHITE,
                border: Border::default(),
                ..iced::widget::button::Style::default()
            }
        }
        _ => iced::widget::button::Style {
            background: Some(Background::Color(Color::TRANSPARENT)),
            text_color: ui_theme::TEXT,
            border: Border::default(),
            ..iced::widget::button::Style::default()
        },
    }
}

fn menu_item_button_style(status: iced::widget::button::Status) -> iced::widget::button::Style {
    // Keep Active transparent so DrawPath::Backdrop can paint the submenu path.
    match status {
        iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed => {
            iced::widget::button::Style {
                background: Some(Background::Color(ui_theme::ACCENT)),
                text_color: Color::WHITE,
                border: Border::default(),
                ..iced::widget::button::Style::default()
            }
        }
        _ => iced::widget::button::Style {
            background: Some(Background::Color(Color::TRANSPARENT)),
            text_color: ui_theme::TEXT,
            border: Border::default(),
            ..iced::widget::button::Style::default()
        },
    }
}

fn connection_row_style(
    selected: bool,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    let background = if selected {
        Some(Background::Color(ui_theme::ACCENT))
    } else if matches!(
        status,
        iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed
    ) {
        Some(Background::Color(ui_theme::HOVER))
    } else {
        Some(Background::Color(Color::TRANSPARENT))
    };

    iced::widget::button::Style {
        background,
        text_color: if selected {
            Color::WHITE
        } else {
            ui_theme::TEXT
        },
        border: Border::default(),
        ..iced::widget::button::Style::default()
    }
}

fn tab_button_style(
    active: bool,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    let background = if active {
        Some(Background::Color(ui_theme::ACCENT))
    } else if matches!(
        status,
        iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed
    ) {
        Some(Background::Color(ui_theme::HOVER))
    } else {
        Some(Background::Color(ui_theme::CHROME_BG))
    };

    iced::widget::button::Style {
        background,
        text_color: if active {
            Color::WHITE
        } else {
            ui_theme::TEXT
        },
        border: Border {
            color: ui_theme::BORDER,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..iced::widget::button::Style::default()
    }
}

fn primary_button_style(status: iced::widget::button::Status) -> iced::widget::button::Style {
    let background = match status {
        iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed => {
            Color::from_rgb(0.25, 0.52, 0.85)
        }
        _ => ui_theme::ACCENT,
    };
    iced::widget::button::Style {
        background: Some(Background::Color(background)),
        text_color: Color::WHITE,
        border: Border::default(),
        ..iced::widget::button::Style::default()
    }
}

fn completion_item_container_style(selected: bool) -> container::Style {
    container::Style {
        background: Some(Background::Color(if selected {
            ui_theme::ACCENT
        } else {
            Color::TRANSPARENT
        })),
        text_color: Some(if selected {
            Color::WHITE
        } else {
            ui_theme::TEXT
        }),
        ..container::Style::default()
    }
}

fn editor_style(status: iced::widget::text_editor::Status) -> iced::widget::text_editor::Style {
    let border_color = match status {
        iced::widget::text_editor::Status::Focused { .. } => ui_theme::ACCENT,
        _ => ui_theme::BORDER,
    };

    iced::widget::text_editor::Style {
        background: Background::Color(ui_theme::PANEL_BG),
        border: Border {
            color: border_color,
            width: 1.0,
            radius: 0.0.into(),
        },
        placeholder: ui_theme::MUTED,
        value: ui_theme::TEXT,
        selection: ui_theme::ACCENT,
    }
}

/// Builds a synthetic SQL script for the editor performance gate.
pub fn build_large_sql_document(line_count: usize) -> String {
    let mut lines = Vec::with_capacity(line_count + 2);
    lines.push("-- synthetic document for Phase 0 editor gate".to_string());
    lines.push("/* generated locally, not executed */".to_string());
    for index in 1..=line_count {
        lines.push(format!(
            "select {index} as id, 'row_{index}' as label from dual; -- line {index}"
        ));
    }
    lines.join("\n")
}
