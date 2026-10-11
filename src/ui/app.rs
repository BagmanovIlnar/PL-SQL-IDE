//! IDE shell: menu, Connections/Objects, worksheets, completion, connection modal.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use iced::alignment::{Horizontal, Vertical};
use iced::border::Border;
use iced::keyboard::key::Named;
use iced::keyboard::Key;
use iced::mouse;
use iced::widget::operation::focus as focus_widget;
use iced::widget::text_editor::{Action, Binding, Content, Edit};
use iced::widget::{
    button, checkbox, column, container, mouse_area, opaque, pick_list, row, rule, scrollable,
    stack, text, text_editor, text_input, tooltip, Space,
};
use iced::{Background, Color, Element, Fill, Font, Padding, Point, Task, Theme};
use iced_fonts::lucide::{pencil, plug, plus, trash};
use iced_fonts::LUCIDE_FONT_BYTES;

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

/// Hit-area thickness of panel resize splitters (logical pixels).
const SPLITTER_HIT: f32 = 6.0;

/// Default width of the Connections / Objects sidebar.
const SIDEBAR_WIDTH_DEFAULT: f32 = 320.0;

/// Minimum width of the Connections / Objects sidebar.
const SIDEBAR_WIDTH_MIN: f32 = 180.0;

/// Maximum width of the Connections / Objects sidebar.
const SIDEBAR_WIDTH_MAX: f32 = 640.0;
use iced_aw::menu::{DrawPath, Item, Menu, MenuBar};
use iced_aw::style::menu_bar as menu_bar_style;

use crate::core::object_metadata::{list_objects, list_owners, ObjectKind, ObjectOwnerList};
use crate::core::postgres_spike::{run_postgres_spike, PostgresSpikeRequest};
use crate::core::{ConnectionId, ConnectionProfile, DbKind};
use crate::ui::completion::{suggest_keywords, word_prefix_at};
use crate::ui::parameters::{
    parameters_view, value_editor_modal, ParamMessage, ParametersPanel, ValueEditorDraft,
    PANEL_HEIGHT, PANEL_HEIGHT_MAX, PANEL_HEIGHT_MIN,
};
use crate::ui::theme::{self as ui_theme, ThemeMode};
use crate::ui::window_kind::WindowKind;

/// Runs the iced IDE window.
pub fn run() -> iced::Result {
    iced::application(App::new, update, view)
        .title("PLSQL IDE")
        .theme(|app: &App| {
            if ui_theme::Palette::resolve(app.theme_mode).is_dark {
                Theme::Dark
            } else {
                Theme::Light
            }
        })
        .font(LUCIDE_FONT_BYTES)
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

/// Which panel border is being dragged to resize.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResizeTarget {
    /// Horizontal border between editor and parameters (changes parameters height).
    Parameters,
    /// Vertical border between sidebar and work area (changes sidebar width).
    Sidebar,
}

/// Active drag state while the user resizes a panel border.
#[derive(Debug, Clone, Copy)]
struct ResizeDrag {
    target: ResizeTarget,
    /// Last cursor position in window coordinates; `None` until the first move.
    last: Option<Point>,
}

#[derive(Debug)]
struct Worksheet {
    kind: WindowKind,
    content: Content,
    /// Vertical scroll offset of the editor, in logical pixels.
    scroll_y: f32,
    /// Bind / test parameters (SQL Window and Test Window).
    parameters: ParametersPanel,
    /// Connection selected in Connections when this worksheet was created.
    connection_id: Option<ConnectionId>,
}

/// Live connectivity state for a saved profile in the Connections list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum ConnectionStatus {
    /// Not tested yet in this session.
    #[default]
    Unknown,
    /// Last Connect attempt succeeded.
    Connected,
    /// Last Connect attempt failed.
    Failed,
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

/// Which File-ribbon dropdown is under the cursor (MenuBar does not hover its root).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RibbonHoverTarget {
    New,
    Open,
}

/// Top ribbon tab (лента).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RibbonTab {
    File,
    Session,
    Settings,
    Help,
}

impl RibbonTab {
    fn label(self) -> &'static str {
        match self {
            Self::File => "File",
            Self::Session => "Session",
            Self::Settings => "Settings",
            Self::Help => "Help",
        }
    }

    fn all() -> [Self; 4] {
        [Self::File, Self::Session, Self::Settings, Self::Help]
    }
}

#[derive(Debug)]
struct App {
    worksheets: Vec<Worksheet>,
    active_tab: usize,
    ribbon_tab: RibbonTab,
    ribbon_hover: Option<RibbonHoverTarget>,
    theme_mode: ThemeMode,
    connections: Vec<ConnectionProfile>,
    selected_connection: Option<ConnectionId>,
    /// Per-profile result of the last Connect attempt in this session.
    connection_status: HashMap<ConnectionId, ConnectionStatus>,
    /// Error text for the Connect failure modal (shown when set).
    connect_error: Option<String>,
    dialog: Option<ConnectionDraft>,
    dialog_error: Option<String>,
    /// Open Value ellipsis editor for a parameter row.
    value_editor: Option<ValueEditorDraft>,
    completion: Vec<String>,
    completion_prefix: String,
    selected_completion: usize,
    busy: bool,
    metrics: String,
    last_output: String,
    selected_node: Option<BrowserNode>,
    expanded: Vec<String>,
    filter: String,
    /// When true, Objects shows only the current login user / owned schemas.
    current_user_only: bool,
    /// Owner / schema roots for the Objects tree of `objects_connection`.
    object_owners: Vec<String>,
    /// Cached object names keyed by owner (or schema) and kind.
    object_cache: HashMap<(String, ObjectKind), Vec<String>>,
    /// Folder loads currently in flight.
    object_folder_loading: HashSet<(String, ObjectKind)>,
    /// True while root owners/schemas are loading.
    objects_loading: bool,
    /// Last Objects browser error (shown as muted text).
    objects_error: Option<String>,
    /// Connection id whose objects are currently shown.
    objects_connection: Option<ConnectionId>,
    /// Width of the Connections / Objects sidebar (user-resizable).
    sidebar_width: f32,
    /// Height of the parameters panel under the editor (user-resizable).
    parameters_height: f32,
    /// Active panel-border drag, if any.
    resize_drag: Option<ResizeDrag>,
}

#[derive(Debug, Clone)]
enum Message {
    EditorAction(Action),
    FilterChanged(String),
    /// Toggle “Only current user” in the Objects panel.
    ToggleCurrentUserOnly(bool),
    ActivateBrowser {
        node: BrowserNode,
        expand_key: Option<String>,
        insert_name: Option<String>,
    },
    /// Async result of loading Objects tree roots (users / schemas).
    ObjectsRootsLoaded(Result<ObjectOwnerList, String>),
    /// Async result of loading one Objects folder (tables, views, …).
    ObjectsFolderLoaded {
        owner: String,
        kind: ObjectKind,
        result: Result<Vec<String>, String>,
        connection_id: ConnectionId,
    },
    SelectTab(usize),
    CloseTab(usize),
    SelectRibbonTab(RibbonTab),
    RibbonHover(Option<RibbonHoverTarget>),
    SetThemeMode(ThemeMode),
    MenuNew(WindowKind),
    MenuOpenStub(&'static str),
    MenuSaveStub,
    MenuSaveAsStub,
    MenuExit,
    MenuExecute,
    SelectConnection(ConnectionId),
    AddConnection,
    EditConnection,
    DeleteConnection,
    /// Attempt to open a live session for the selected Connections profile.
    ConnectSelected,
    /// Async result of a Connect attempt for `ConnectionId`.
    ConnectFinished(ConnectionId, Result<(), String>),
    /// Close the Connect error modal.
    DismissConnectError,
    DialogNameChanged(String),
    DialogDbKindChanged(DbKind),
    DialogHostChanged(String),
    DialogPortChanged(String),
    DialogDatabaseChanged(String),
    DialogUsernameChanged(String),
    DialogPasswordChanged(String),
    DialogSave,
    DialogCancel,
    /// Parameters grid / Value editor messages (SQL & Test windows).
    Param(ParamMessage),
    ApplyCompletion(String),
    /// Apply the currently highlighted completion item (Enter).
    ApplySelectedCompletion,
    /// Move the completion highlight by `delta` (−1 / +1).
    CompletionNavigate(i32),
    /// Close the completion popup without inserting (Escape).
    DismissCompletion,
    SpikeFinished(Result<String, String>),
    /// Begin dragging a panel resize border.
    ResizeStart(ResizeTarget),
    /// Cursor moved while a panel resize drag is active (window coordinates).
    ResizeMove(Point),
    /// End a panel resize drag (left mouse button released).
    ResizeEnd,
}

impl App {
    fn new() -> Self {
        let mut local = ConnectionProfile::new("Local PostgreSQL", DbKind::Postgres);
        local.username = "postgres".into();
        local.password = "postgres".into();

        let mut oracle = ConnectionProfile::new("Local Oracle", DbKind::Oracle);
        oracle.username = "scott".into();

        let selected = local.id;
        ui_theme::apply(ThemeMode::System);
        Self {
            worksheets: vec![Worksheet {
                kind: WindowKind::Sql,
                content: Content::with_text(WindowKind::Sql.template()),
                scroll_y: 0.0,
                parameters: ParametersPanel::default(),
                connection_id: Some(selected),
            }],
            active_tab: 0,
            ribbon_tab: RibbonTab::File,
            ribbon_hover: None,
            theme_mode: ThemeMode::System,
            connections: vec![local, oracle],
            selected_connection: Some(selected),
            connection_status: HashMap::new(),
            connect_error: None,
            dialog: None,
            dialog_error: None,
            value_editor: None,
            completion: Vec::new(),
            completion_prefix: String::new(),
            selected_completion: 0,
            busy: false,
            metrics: "Ready".into(),
            last_output: String::new(),
            selected_node: None,
            expanded: Vec::new(),
            filter: String::new(),
            current_user_only: true,
            object_owners: Vec::new(),
            object_cache: HashMap::new(),
            object_folder_loading: HashSet::new(),
            objects_loading: false,
            objects_error: None,
            objects_connection: None,
            sidebar_width: SIDEBAR_WIDTH_DEFAULT,
            parameters_height: PANEL_HEIGHT,
            resize_drag: None,
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

    /// Profile bound to the active worksheet (File → New uses Connections selection).
    fn active_worksheet_profile(&self) -> Option<&ConnectionProfile> {
        let sheet = self.worksheets.get(self.active_tab)?;
        let id = sheet.connection_id?;
        self.connections.iter().find(|item| item.id == id)
    }

    /// Label for the connection bound to a worksheet.
    fn worksheet_connection_label(&self, sheet: &Worksheet) -> String {
        match sheet.connection_id {
            Some(id) => self
                .connections
                .iter()
                .find(|item| item.id == id)
                .map(|profile| profile.summary())
                .unwrap_or_else(|| "connection removed".into()),
            None => "no connection".into(),
        }
    }

    fn open_window(&mut self, kind: WindowKind) {
        let connection_id = self.selected_connection;
        let connection_label = connection_id
            .and_then(|id| self.connections.iter().find(|item| item.id == id))
            .map(|profile| profile.name.clone())
            .unwrap_or_else(|| "no connection".into());

        self.worksheets.push(Worksheet {
            kind,
            content: Content::with_text(kind.template()),
            scroll_y: 0.0,
            parameters: ParametersPanel::default(),
            connection_id,
        });
        self.active_tab = self.worksheets.len() - 1;
        self.completion.clear();
        self.value_editor = None;
        self.metrics = format!("Opened {} · {connection_label}", kind.tab_title());
    }

    fn active_parameters_mut(&mut self) -> Option<&mut ParametersPanel> {
        self.worksheets
            .get_mut(self.active_tab)
            .map(|sheet| &mut sheet.parameters)
    }

    fn handle_param_message(&mut self, message: ParamMessage) {
        match message {
            ParamMessage::Add => {
                let db_kind = self
                    .active_worksheet_profile()
                    .map(|profile| profile.db_kind)
                    .unwrap_or(DbKind::Postgres);
                if let Some(panel) = self.active_parameters_mut() {
                    panel.add(db_kind);
                    self.metrics = "Parameter added".into();
                }
            }
            ParamMessage::Delete => {
                if let Some(panel) = self.active_parameters_mut() {
                    if panel.delete_selected() {
                        self.metrics = "Parameter deleted".into();
                    } else {
                        self.metrics = "Select a parameter to delete".into();
                    }
                }
            }
            ParamMessage::Select(index) => {
                if let Some(panel) = self.active_parameters_mut() {
                    panel.select(index);
                }
            }
            ParamMessage::NameChanged(index, value) => {
                if let Some(panel) = self.active_parameters_mut() {
                    if let Some(row) = panel.rows.get_mut(index) {
                        row.name = value;
                    }
                }
            }
            ParamMessage::ModeChanged(index, mode) => {
                if let Some(panel) = self.active_parameters_mut() {
                    if let Some(row) = panel.rows.get_mut(index) {
                        row.mode = mode;
                    }
                    panel.select(index);
                }
            }
            ParamMessage::TypeChanged(index, data_type) => {
                if let Some(panel) = self.active_parameters_mut() {
                    if let Some(row) = panel.rows.get_mut(index) {
                        row.data_type = data_type;
                    }
                    panel.select(index);
                }
            }
            ParamMessage::ValueChanged(index, value) => {
                if let Some(panel) = self.active_parameters_mut() {
                    if let Some(row) = panel.rows.get_mut(index) {
                        row.value = value;
                    }
                }
            }
            ParamMessage::OpenValueEditor(index) => {
                let value = self
                    .worksheets
                    .get(self.active_tab)
                    .and_then(|sheet| sheet.parameters.rows.get(index))
                    .map(|row| row.value.clone())
                    .unwrap_or_default();
                if let Some(panel) = self.active_parameters_mut() {
                    panel.select(index);
                }
                self.value_editor = Some(ValueEditorDraft::open(index, &value));
            }
            ParamMessage::EditorTab(tab) => {
                if let Some(editor) = &mut self.value_editor {
                    editor.set_tab(tab);
                }
            }
            ParamMessage::EditorAction(action) => {
                if let Some(editor) = &mut self.value_editor {
                    editor.perform_editor(action);
                }
            }
            ParamMessage::EditorHexChanged(value) => {
                if let Some(editor) = &mut self.value_editor {
                    editor.set_hex_text(value);
                }
            }
            ParamMessage::EditorOk => {
                if let Some(editor) = self.value_editor.take() {
                    let index = editor.param_index;
                    let value = editor.committed_value();
                    if let Some(panel) = self.active_parameters_mut() {
                        if let Some(row) = panel.rows.get_mut(index) {
                            row.value = value;
                        }
                    }
                    self.metrics = "Parameter value updated".into();
                }
            }
            ParamMessage::EditorCancel => {
                self.value_editor = None;
            }
        }
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

    /// Whether the currently selected Connections profile is Connected.
    fn selected_is_connected(&self) -> bool {
        let Some(id) = self.selected_connection else {
            return false;
        };
        self.connection_status.get(&id) == Some(&ConnectionStatus::Connected)
    }

    /// Clears Objects tree state (used when not connected or switching away).
    fn clear_object_browser(&mut self) {
        self.object_owners.clear();
        self.object_cache.clear();
        self.object_folder_loading.clear();
        self.objects_loading = false;
        self.objects_error = None;
        self.objects_connection = None;
        self.selected_node = None;
        self.expanded.clear();
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
        Message::ToggleCurrentUserOnly(value) => {
            app.current_user_only = value;
            if app.selected_is_connected() {
                start_load_object_roots(app)
            } else {
                app.clear_object_browser();
                Task::none()
            }
        }
        Message::ActivateBrowser {
            node,
            expand_key,
            insert_name,
        } => {
            let folder_to_load = match (&expand_key, &node) {
                (Some(key), BrowserNode::Folder { schema, kind }) if !app.is_expanded(key) => {
                    Some((schema.clone(), *kind))
                }
                _ => None,
            };

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

            if let Some((schema, kind)) = folder_to_load {
                start_load_object_folder(app, schema, kind)
            } else {
                Task::none()
            }
        }
        Message::ObjectsRootsLoaded(result) => {
            app.objects_loading = false;
            match result {
                Ok(list) => {
                    if app.selected_connection != Some(list.connection_id) {
                        return Task::none();
                    }
                    app.objects_connection = Some(list.connection_id);
                    app.object_owners = list.owners;
                    app.objects_error = None;
                    app.metrics = format!(
                        "Loaded {} object root(s) for {}",
                        app.object_owners.len(),
                        list.current_user
                    );
                }
                Err(error) => {
                    app.object_owners.clear();
                    app.objects_error = Some(error.clone());
                    app.metrics = "Failed to load objects".into();
                }
            }
            Task::none()
        }
        Message::ObjectsFolderLoaded {
            owner,
            kind,
            result,
            connection_id,
        } => {
            app.object_folder_loading.remove(&(owner.clone(), kind));
            if app.objects_connection != Some(connection_id) {
                return Task::none();
            }
            match result {
                Ok(names) => {
                    app.object_cache.insert((owner, kind), names);
                    app.objects_error = None;
                }
                Err(error) => {
                    app.objects_error = Some(error);
                    app.metrics = "Failed to load object folder".into();
                }
            }
            Task::none()
        }
        Message::SelectTab(index) => {
            if index < app.worksheets.len() {
                app.active_tab = index;
                app.completion.clear();
                app.value_editor = None;
            }
            Task::none()
        }
        Message::SelectRibbonTab(tab) => {
            app.ribbon_tab = tab;
            app.ribbon_hover = None;
            Task::none()
        }
        Message::RibbonHover(target) => {
            app.ribbon_hover = target;
            Task::none()
        }
        Message::SetThemeMode(mode) => {
            app.theme_mode = mode;
            ui_theme::apply(mode);
            app.metrics = format!("Theme: {}", mode.label());
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
            app.value_editor = None;
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
        Message::MenuExecute => run_spike(app),
        Message::SelectConnection(id) => {
            app.selected_connection = Some(id);
            if let Some(profile) = app.selected_profile() {
                app.metrics = format!("Selected {}", profile.name);
            }
            if app.selected_is_connected() {
                start_load_object_roots(app)
            } else {
                app.clear_object_browser();
                Task::none()
            }
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
            app.connection_status.remove(&id);
            for sheet in &mut app.worksheets {
                if sheet.connection_id == Some(id) {
                    sheet.connection_id = None;
                }
            }
            if app.objects_connection == Some(id) {
                app.clear_object_browser();
            }
            app.selected_connection = app.connections.first().map(|item| item.id);
            app.metrics = "Connection deleted".into();
            if app.selected_is_connected() {
                start_load_object_roots(app)
            } else {
                app.clear_object_browser();
                Task::none()
            }
        }
        Message::ConnectSelected => run_connect(app),
        Message::ConnectFinished(id, result) => {
            app.busy = false;
            match result {
                Ok(()) => {
                    app.connection_status.insert(id, ConnectionStatus::Connected);
                    app.connect_error = None;
                    let name = app
                        .connections
                        .iter()
                        .find(|item| item.id == id)
                        .map(|item| item.name.clone())
                        .unwrap_or_else(|| "connection".into());
                    app.metrics = format!("Connected to {name}");
                    if app.selected_connection == Some(id) {
                        start_load_object_roots(app)
                    } else {
                        Task::none()
                    }
                }
                Err(error) => {
                    app.connection_status.insert(id, ConnectionStatus::Failed);
                    app.connect_error = Some(error);
                    app.metrics = "Connect failed".into();
                    if app.objects_connection == Some(id) {
                        app.clear_object_browser();
                    }
                    Task::none()
                }
            }
        }
        Message::DismissConnectError => {
            app.connect_error = None;
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
        Message::Param(message) => {
            app.handle_param_message(message);
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
                        app.connection_status.remove(&id);
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
        Message::ResizeStart(target) => {
            app.resize_drag = Some(ResizeDrag {
                target,
                last: None,
            });
            Task::none()
        }
        Message::ResizeMove(point) => {
            if let Some(drag) = &mut app.resize_drag {
                if let Some(last) = drag.last {
                    match drag.target {
                        ResizeTarget::Parameters => {
                            // Dragging the shared border down shrinks parameters
                            // (editor grows); dragging up grows parameters.
                            let next = app.parameters_height - (point.y - last.y);
                            app.parameters_height =
                                next.clamp(PANEL_HEIGHT_MIN, PANEL_HEIGHT_MAX);
                        }
                        ResizeTarget::Sidebar => {
                            let next = app.sidebar_width + (point.x - last.x);
                            app.sidebar_width =
                                next.clamp(SIDEBAR_WIDTH_MIN, SIDEBAR_WIDTH_MAX);
                        }
                    }
                }
                drag.last = Some(point);
            }
            Task::none()
        }
        Message::ResizeEnd => {
            app.resize_drag = None;
            Task::none()
        }
    }
}

fn run_connect(app: &mut App) -> Task<Message> {
    if app.busy {
        return Task::none();
    }
    let Some(profile) = app.selected_profile().cloned() else {
        app.metrics = "Select a connection to connect".into();
        return Task::none();
    };

    let id = profile.id;
    app.busy = true;
    app.connect_error = None;
    app.metrics = format!("Connecting to {}…", profile.name);

    match profile.db_kind {
        DbKind::Postgres => {
            let request = PostgresSpikeRequest {
                connection_url: profile.postgres_url(),
                sql: Some("select 1::int as value".into()),
                timeout_ms: Some(10_000),
                cancel: false,
            };
            Task::perform(
                async move {
                    run_postgres_spike(request)
                        .await
                        .map(|_| ())
                        .map_err(|err| err.to_string())
                },
                move |result| Message::ConnectFinished(id, result),
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
                    sql: Some("select 1 as value from dual".into()),
                    timeout_ms: Some(10_000),
                    cancel: false,
                };
                Task::perform(
                    async move {
                        run_oracle_spike(request)
                            .await
                            .map(|_| ())
                            .map_err(|err| err.to_string())
                    },
                    move |result| Message::ConnectFinished(id, result),
                )
            }
            #[cfg(not(feature = "oracle"))]
            {
                app.busy = false;
                app.connection_status.insert(id, ConnectionStatus::Failed);
                app.connect_error =
                    Some("Oracle feature is disabled. Rebuild with --features oracle".into());
                app.metrics = "Connect failed".into();
                Task::none()
            }
        }
    }
}

/// Starts async load of Objects roots for the selected connected profile.
fn start_load_object_roots(app: &mut App) -> Task<Message> {
    let Some(profile) = app.selected_profile().cloned() else {
        app.clear_object_browser();
        return Task::none();
    };
    if app.connection_status.get(&profile.id) != Some(&ConnectionStatus::Connected) {
        app.clear_object_browser();
        return Task::none();
    }

    let current_user_only = app.current_user_only;
    app.objects_loading = true;
    app.objects_error = None;
    app.object_owners.clear();
    app.object_cache.clear();
    app.object_folder_loading.clear();
    app.expanded.clear();
    app.selected_node = None;
    app.objects_connection = Some(profile.id);
    app.metrics = format!("Loading objects for {}…", profile.name);

    Task::perform(
        async move {
            list_owners(&profile, current_user_only)
                .await
                .map_err(|err| err.to_string())
        },
        Message::ObjectsRootsLoaded,
    )
}

/// Starts async load of one Objects folder unless already cached or in flight.
fn start_load_object_folder(app: &mut App, owner: String, kind: ObjectKind) -> Task<Message> {
    let key = (owner.clone(), kind);
    if app.object_cache.contains_key(&key) || app.object_folder_loading.contains(&key) {
        return Task::none();
    }
    let Some(profile) = app.selected_profile().cloned() else {
        return Task::none();
    };
    if app.objects_connection != Some(profile.id) {
        return Task::none();
    }

    let connection_id = profile.id;
    app.object_folder_loading.insert(key);
    Task::perform(
        async move {
            let owner_for_msg = owner.clone();
            match list_objects(&profile, &owner, kind).await {
                Ok(list) => Message::ObjectsFolderLoaded {
                    owner: list.owner,
                    kind: list.kind,
                    result: Ok(list.names),
                    connection_id: list.connection_id,
                },
                Err(err) => Message::ObjectsFolderLoaded {
                    owner: owner_for_msg,
                    kind,
                    result: Err(err.to_string()),
                    connection_id,
                },
            }
        },
        |message| message,
    )
}

fn run_spike(app: &mut App) -> Task<Message> {
    if app.busy {
        return Task::none();
    }
    let Some(profile) = app.active_worksheet_profile().cloned() else {
        app.metrics = "Active worksheet has no connection".into();
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
    // Keep style helpers in sync with the selected preference every frame
    // (System may follow OS changes while the app is open).
    ui_theme::apply(app.theme_mode);

    let shell = column![
        ribbon(app),
        rule::horizontal(1).style(|_| rule_style()),
        row![
            side_browser(app),
            vertical_resize_handle(),
            column![
                worksheet_tabs(app),
                worksheet_connection_bar(app),
                editor_pane(app),
            ]
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
            background: Some(Background::Color(ui_theme::current().window_bg)),
            text_color: Some(ui_theme::current().text),
            ..container::Style::default()
        });

    if app.resize_drag.is_some() {
        stack![base, resize_drag_overlay(app)]
            .width(Fill)
            .height(Fill)
            .into()
    } else if let Some(draft) = &app.value_editor {
        stack![base, value_editor_modal(draft).map(Message::Param)]
            .width(Fill)
            .height(Fill)
            .into()
    } else if app.dialog.is_some() {
        stack![base, connection_modal(app)]
            .width(Fill)
            .height(Fill)
            .into()
    } else if app.connect_error.is_some() {
        stack![base, connect_error_modal(app)]
            .width(Fill)
            .height(Fill)
            .into()
    } else {
        base.into()
    }
}

fn ribbon_menu_item(label: &'static str) -> iced::widget::Button<'static, Message> {
    button(
        container(text(label).size(13))
            .width(Fill)
            .align_x(Horizontal::Left),
    )
    .width(Fill)
    .padding(Padding::from([7, 12]))
    .style(move |_, status| menu_item_button_style(status))
}

fn ribbon_command(label: &'static str) -> iced::widget::Button<'static, Message> {
    button(text(label).size(12))
        .padding(Padding::from([4, 10]))
        .style(move |_, status| ribbon_command_style(status, false))
}

/// File ribbon New/Open: own hover tracking, because iced_aw MenuBar does not
/// put the root button into `Hovered` while the menu is closed.
fn ribbon_dropdown(
    label: &'static str,
    target: RibbonHoverTarget,
    hovered: bool,
    menu: Menu<'static, Message, Theme, iced::Renderer>,
) -> Element<'static, Message> {
    let trigger = button(text(label).size(12))
        .padding(Padding::from([4, 10]))
        .style(move |_, status| ribbon_command_style(status, hovered));

    let bar = MenuBar::new(vec![Item::with_menu(trigger, menu)])
        // Backdrop paints one highlight behind the open path (e.g. Program Window).
        // Root New/Open closed-hover is handled by `hovered` on the button style only.
        .draw_path(DrawPath::Backdrop)
        .close_on_item_click(true)
        .close_on_background_click(true)
        .close_on_item_click_global(true)
        .close_on_background_click_global(true)
        .style(|_, _| ribbon_dropdown_style());

    mouse_area(
        container(bar)
            .height(28)
            .align_y(Vertical::Center),
    )
    .on_enter(Message::RibbonHover(Some(target)))
    .on_exit(Message::RibbonHover(None))
    .into()
}

fn ribbon_divider() -> Element<'static, Message> {
    container(rule::vertical(1).style(|_| rule_style()))
        .height(22)
        .into()
}

fn ribbon_group<'a>(
    label: &'static str,
    content: Element<'a, Message>,
) -> Element<'a, Message> {
    container(
        column![
            container(content)
                .height(28)
                .align_y(Vertical::Center),
            text(label).size(10).color(ui_theme::current().muted),
        ]
        .spacing(1)
        .align_x(Horizontal::Center),
    )
    .padding(Padding::from([2, 8]))
    .into()
}

fn ribbon_dropdown_style() -> menu_bar_style::Style {
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
            radius: 0.0.into(),
        },
        menu_shadow: iced::Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.25),
            offset: iced::Vector::new(0.0, 3.0),
            blur_radius: 10.0,
        },
        // Same look as ribbon_command hover — used for New (while open) and Program Window.
        path: Background::Color(ui_theme::current().hover),
        path_border: Border {
            color: ui_theme::current().border,
            width: 1.0,
            radius: 3.0.into(),
        },
    }
}

fn ribbon(app: &App) -> Element<'_, Message> {
    let mut tabs = row![].spacing(0);
    for tab in RibbonTab::all() {
        let active = app.ribbon_tab == tab;
        tabs = tabs.push(
            button(text(tab.label()).size(12))
                .padding(Padding::from([4, 12]))
                .style(move |_, status| ribbon_tab_style(active, status))
                .on_press(Message::SelectRibbonTab(tab)),
        );
    }

    let tab_strip = container(tabs)
        .width(Fill)
        .padding(Padding::from([0, 4]))
        .style(|_| container::Style {
            background: Some(Background::Color(ui_theme::current().ribbon_bg)),
            text_color: Some(ui_theme::current().text),
            ..container::Style::default()
        });

    let program_menu = Menu::new(vec![
        Item::new(ribbon_menu_item("Blank").on_press(Message::MenuNew(WindowKind::Blank))),
        Item::new(ribbon_menu_item("Function").on_press(Message::MenuNew(WindowKind::Function))),
        Item::new(
            ribbon_menu_item("Java Source").on_press(Message::MenuNew(WindowKind::JavaSource)),
        ),
        Item::new(ribbon_menu_item("Package").on_press(Message::MenuNew(WindowKind::Package))),
        Item::new(
            ribbon_menu_item("Package Body").on_press(Message::MenuNew(WindowKind::PackageBody)),
        ),
        Item::new(
            ribbon_menu_item("Package Specification")
                .on_press(Message::MenuNew(WindowKind::PackageSpecification)),
        ),
        Item::new(ribbon_menu_item("Procedure").on_press(Message::MenuNew(WindowKind::Procedure))),
        Item::new(ribbon_menu_item("Trigger").on_press(Message::MenuNew(WindowKind::Trigger))),
        Item::new(ribbon_menu_item("Type").on_press(Message::MenuNew(WindowKind::Type))),
        Item::new(ribbon_menu_item("Type Body").on_press(Message::MenuNew(WindowKind::TypeBody))),
        Item::new(
            ribbon_menu_item("Type Specification")
                .on_press(Message::MenuNew(WindowKind::TypeSpecification)),
        ),
    ])
    .max_width(240.0)
    .offset(1.0)
    .close_on_item_click(true)
    .close_on_background_click(true);

    let new_menu = Menu::new(vec![
        Item::with_menu(ribbon_menu_item("Program Window"), program_menu),
        Item::new(ribbon_menu_item("Test Window").on_press(Message::MenuNew(WindowKind::Test))),
        Item::new(ribbon_menu_item("SQL Window").on_press(Message::MenuNew(WindowKind::Sql))),
        Item::new(ribbon_menu_item("Command Window").on_press(Message::MenuNew(WindowKind::Command))),
        Item::new(
            ribbon_menu_item("Explain Plan Window")
                .on_press(Message::MenuNew(WindowKind::ExplainPlan)),
        ),
    ])
    .max_width(240.0)
    .offset(1.0)
    .close_on_item_click(true)
    .close_on_background_click(true);

    let open_menu = Menu::new(vec![
        Item::new(ribbon_menu_item("Program File").on_press(Message::MenuOpenStub("Program File"))),
        Item::new(ribbon_menu_item("Test Script").on_press(Message::MenuOpenStub("Test Script"))),
        Item::new(ribbon_menu_item("SQL Script").on_press(Message::MenuOpenStub("SQL Script"))),
        Item::new(ribbon_menu_item("Command File").on_press(Message::MenuOpenStub("Command File"))),
    ])
    .max_width(200.0)
    .offset(1.0)
    .close_on_item_click(true)
    .close_on_background_click(true);

    // Classic ribbon: tabs on top, commands in the body below — just without empty bulk.
    let body: Element<'_, Message> = match app.ribbon_tab {
        RibbonTab::File => {
            let new_hovered = app.ribbon_hover == Some(RibbonHoverTarget::New);
            let open_hovered = app.ribbon_hover == Some(RibbonHoverTarget::Open);

            row![
                ribbon_group(
                    "Open",
                    row![
                        ribbon_dropdown("New", RibbonHoverTarget::New, new_hovered, new_menu),
                        ribbon_dropdown("Open", RibbonHoverTarget::Open, open_hovered, open_menu),
                    ]
                    .spacing(4)
                    .into(),
                ),
                ribbon_divider(),
                ribbon_group(
                    "Save",
                    row![
                        ribbon_command("Save").on_press(Message::MenuSaveStub),
                        ribbon_command("Save As").on_press(Message::MenuSaveAsStub),
                    ]
                    .spacing(4)
                    .into(),
                ),
                ribbon_divider(),
                ribbon_group(
                    "Application",
                    ribbon_command("Exit").on_press(Message::MenuExit).into(),
                ),
                Space::new().width(Fill),
            ]
            .spacing(2)
            .align_y(Vertical::Center)
            .into()
        }
        RibbonTab::Session => row![
            ribbon_group(
                "Execute",
                ribbon_command("Execute").on_press(Message::MenuExecute).into(),
            ),
            Space::new().width(Fill),
        ]
        .align_y(Vertical::Center)
        .into(),
        RibbonTab::Settings => {
            let mut theme_row = row![].spacing(6).align_y(Vertical::Center);
            for mode in ThemeMode::all() {
                let selected = app.theme_mode == mode;
                theme_row = theme_row.push(
                    button(text(mode.label()).size(12))
                        .padding(Padding::from([4, 10]))
                        .style(move |_, status| theme_choice_style(selected, status))
                        .on_press(Message::SetThemeMode(mode)),
                );
            }
            row![
                ribbon_group("Theme", theme_row.into()),
                Space::new().width(Fill),
            ]
            .align_y(Vertical::Center)
            .into()
        }
        RibbonTab::Help => container(
            text("About and documentation will appear here")
                .size(12)
                .color(ui_theme::current().muted),
        )
        .padding(Padding::from([6, 10]))
        .into(),
    };

    let body_band = container(body)
        .width(Fill)
        .padding(Padding::from([2, 6]))
        .style(|_| container::Style {
            background: Some(Background::Color(ui_theme::current().ribbon_body_bg)),
            text_color: Some(ui_theme::current().text),
            ..container::Style::default()
        });

    column![tab_strip, body_band].into()
}

fn side_browser(app: &App) -> Element<'_, Message> {
    let mut list = column![].spacing(2);
    for profile in &app.connections {
        let selected = app.selected_connection == Some(profile.id);
        let id = profile.id;
        let status = app
            .connection_status
            .get(&id)
            .copied()
            .unwrap_or_default();
        let label = profile.summary();
        let mut label_text = text(label).size(12).font(Font::MONOSPACE);
        label_text = match status {
            ConnectionStatus::Connected => label_text.color(Color::from_rgb(0.12, 0.72, 0.32)),
            ConnectionStatus::Failed => label_text.color(Color::from_rgb(0.90, 0.28, 0.28)),
            ConnectionStatus::Unknown => label_text,
        };
        list = list.push(
            button(
                container(label_text)
                    .width(Fill)
                    .align_x(Horizontal::Left),
            )
            .width(Fill)
            .padding(Padding::from([8, 10]))
            .style(move |_, btn_status| connection_row_style(selected, status, btn_status))
            .on_press(Message::SelectConnection(id)),
        );
    }

    let connections = column![
        section_title("Connections"),
        container(
            row![
                dock_icon_button(plus().size(14), "Add", Message::AddConnection),
                dock_icon_button(pencil().size(14), "Edit", Message::EditConnection),
                dock_icon_button(trash().size(14), "Delete", Message::DeleteConnection),
                dock_icon_button(plug().size(14), "Connect", Message::ConnectSelected),
            ]
            .spacing(4)
        )
        .padding(Padding::from([4, 8]))
        .width(Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(ui_theme::current().ribbon_bg)),
            ..container::Style::default()
        }),
        rule::horizontal(1).style(|_| rule_style()),
        scrollable(list.padding(Padding::from([6, 6]))).height(Fill),
    ]
    .height(Fill)
    .width(Fill);

    let objects_body: Element<'_, Message> = if !app.selected_is_connected() {
        container(
            text("Connect to load objects")
                .size(12)
                .color(ui_theme::current().muted),
        )
        .padding(Padding::from([8, 10]))
        .width(Fill)
        .into()
    } else if app.objects_loading {
        container(
            text("Loading objects…")
                .size(12)
                .color(ui_theme::current().muted),
        )
        .padding(Padding::from([8, 10]))
        .width(Fill)
        .into()
    } else {
        let db_kind = app
            .selected_profile()
            .map(|profile| profile.db_kind)
            .unwrap_or(DbKind::Postgres);

        let mut tree = column![].spacing(1).padding(Padding {
            top: 0.0,
            right: 4.0,
            bottom: 8.0,
            left: 4.0,
        });

        if let Some(error) = &app.objects_error {
            tree = tree.push(
                text(error.clone())
                    .size(11)
                    .color(Color::from_rgb(0.90, 0.40, 0.40)),
            );
        }

        if app.object_owners.is_empty() && app.objects_error.is_none() {
            tree = tree.push(
                text("No schemas / users found")
                    .size(12)
                    .color(ui_theme::current().muted),
            );
        }

        for owner in &app.object_owners {
            let schema_key = format!("schema:{owner}");
            let owner_expanded = app.is_expanded(&schema_key);
            let marker = if owner_expanded { "▾" } else { "▸" };
            tree = tree.push(browser_row(
                app,
                format!("{marker}  {owner}"),
                BrowserNode::Schema(owner.clone()),
                Some(schema_key),
            ));

            if owner_expanded {
                for kind in ObjectKind::for_db(db_kind) {
                    let names = app
                        .object_cache
                        .get(&(owner.clone(), *kind))
                        .map(|items| items.as_slice());
                    let loading = app.object_folder_loading.contains(&(owner.clone(), *kind));
                    tree = tree.push(object_folder(app, owner, *kind, names, loading));
                }
            }
        }

        scrollable(tree).height(Fill).into()
    };

    let objects = column![
        section_title("Objects"),
        container(
            checkbox(app.current_user_only)
                .label("Only current user")
                .size(14)
                .text_size(12)
                .on_toggle(Message::ToggleCurrentUserOnly),
        )
        .padding(Padding {
            top: 6.0,
            right: 8.0,
            bottom: 0.0,
            left: 8.0,
        }),
        container(
            text_input("Enter filter text…", &app.filter)
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
        objects_body,
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
    .width(app.sidebar_width)
    .height(Fill)
    .style(|_| container::Style {
        background: Some(Background::Color(ui_theme::current().sidebar_bg)),
        text_color: Some(ui_theme::current().text),
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
        text("Database type").size(12).color(ui_theme::current().muted),
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

/// Modal shown when Connect fails; displays the driver / network error text.
fn connect_error_modal(app: &App) -> Element<'_, Message> {
    let error = app
        .connect_error
        .as_deref()
        .unwrap_or("Unknown connection error");

    let form = column![
        text("Connection failed").size(18).color(Color::from_rgb(0.90, 0.28, 0.28)),
        text(error.to_string())
            .size(13)
            .color(ui_theme::current().text),
        row![
            Space::new().width(Fill),
            button(text("OK").size(13))
                .padding(Padding::from([8, 16]))
                .style(|_, status| primary_button_style(status))
                .on_press(Message::DismissConnectError),
        ],
    ]
    .spacing(12)
    .padding(20)
    .width(460);

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
    .on_press(Message::DismissConnectError);

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
        text(label).size(12).color(ui_theme::current().muted),
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
            background: Some(Background::Color(ui_theme::current().sidebar_title_bg)),
            text_color: Some(ui_theme::current().text),
            ..container::Style::default()
        })
        .into()
}

fn object_folder<'a>(
    app: &'a App,
    schema: &str,
    kind: ObjectKind,
    names: Option<&'a [String]>,
    loading: bool,
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
        format!("  {marker}  {}", kind.label()),
        folder_node,
        Some(key.clone()),
    )];

    if expanded {
        if loading && names.is_none() {
            children = children.push(
                container(
                    text("    …")
                        .size(12)
                        .color(ui_theme::current().muted)
                        .font(Font::MONOSPACE),
                )
                .padding(Padding::from([4, 8])),
            );
        } else if let Some(names) = names {
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
                        name: name.clone(),
                    },
                    None,
                ));
            }
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
                    background: Some(Background::Color(ui_theme::current().accent)),
                    text_color: Some(Color::WHITE),
                    ..container::Style::default()
                }
            } else {
                container::Style {
                    background: Some(Background::Color(Color::TRANSPARENT)),
                    text_color: Some(ui_theme::current().text),
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
            text(&app.metrics).size(12).color(ui_theme::current().muted),
        ]
        .align_y(Vertical::Center)
        .padding(Padding::from([4, 8])),
    )
    .width(Fill)
    .style(|_| chrome_style())
    .into()
}

/// Shows which connection the active worksheet was created for.
fn worksheet_connection_bar(app: &App) -> Element<'_, Message> {
    let label = app
        .worksheets
        .get(app.active_tab)
        .map(|sheet| app.worksheet_connection_label(sheet))
        .unwrap_or_else(|| "no connection".into());

    container(
        row![
            text("Database:").size(11).color(ui_theme::current().muted),
            text(label).size(11),
        ]
        .spacing(6)
        .align_y(Vertical::Center)
        .padding(Padding::from([3, 10])),
    )
    .width(Fill)
    .style(|_| container::Style {
        background: Some(Background::Color(ui_theme::current().sidebar_title_bg)),
        text_color: Some(ui_theme::current().text),
        border: Border {
            color: ui_theme::current().border,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..container::Style::default()
    })
    .into()
}

/// Builds the virtualized line-number gutter for the SQL editor.
///
/// Only a window of numbers around the current scroll position is created, so
/// large documents stay responsive.
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
                    .color(ui_theme::current().muted),
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
        background: Some(Background::Color(ui_theme::current().sidebar_bg)),
        border: Border {
            color: ui_theme::current().border,
            width: 1.0,
            radius: 0.0.into(),
        },
        text_color: Some(ui_theme::current().muted),
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
    let highlighter_theme = if ui_theme::current().is_dark {
        iced::highlighter::Theme::Base16Ocean
    } else {
        iced::highlighter::Theme::InspiredGitHub
    };
    let editor = text_editor(&sheet.content)
        .id(EDITOR_ID)
        .height(Fill)
        .font(Font::MONOSPACE)
        .size(EDITOR_TEXT_SIZE)
        .line_height(iced::widget::text::LineHeight::Absolute(
            EDITOR_LINE_HEIGHT.into(),
        ))
        .padding(EDITOR_PADDING)
        .highlight(syntax, highlighter_theme)
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
                background: Some(Background::Color(ui_theme::current().panel_bg)),
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
            background: Some(Background::Color(ui_theme::current().panel_bg)),
            border: Border {
                color: ui_theme::current().border,
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
                background: Some(Background::Color(ui_theme::current().panel_bg)),
                border: Border {
                    color: ui_theme::current().accent,
                    width: 1.0,
                    radius: 2.0.into(),
                },
                text_color: Some(ui_theme::current().text),
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

    let editor_stack = stack![editor_layer, overlay]
        .width(Fill)
        .height(Fill);

    if sheet.kind.shows_parameters() {
        let db_kind = sheet
            .connection_id
            .and_then(|id| {
                app.connections
                    .iter()
                    .find(|profile| profile.id == id)
                    .map(|profile| profile.db_kind)
            })
            .unwrap_or(DbKind::Postgres);
        column![
            editor_stack,
            horizontal_resize_handle(),
            parameters_view(&sheet.parameters, db_kind, app.parameters_height).map(Message::Param),
        ]
        .spacing(0)
        .width(Fill)
        .height(Fill)
        .into()
    } else {
        editor_stack.into()
    }
}

fn status_bar(app: &App) -> Element<'_, Message> {
    let connection = app
        .active_worksheet_profile()
        .map(|profile| profile.summary())
        .or_else(|| {
            app.worksheets
                .get(app.active_tab)
                .map(|sheet| app.worksheet_connection_label(sheet))
        })
        .unwrap_or_else(|| "no connection".into());
    let detail = if app.last_output.is_empty() {
        app.metrics.clone()
    } else {
        app.last_output.lines().next().unwrap_or("").to_string()
    };

    container(
        row![
            text(format!("Worksheet DB: {connection}")).size(12),
            Space::new().width(Fill),
            text(detail).size(12).color(ui_theme::current().muted),
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
        background: Some(Background::Color(ui_theme::current().chrome_bg)),
        text_color: Some(ui_theme::current().text),
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

/// Full-window capture layer while a panel border is being dragged.
///
/// Keeps receiving move / release events after the cursor leaves the thin
/// splitter hit area, and shows the matching resize cursor.
fn resize_drag_overlay(app: &App) -> Element<'_, Message> {
    let interaction = match app.resize_drag.map(|drag| drag.target) {
        Some(ResizeTarget::Parameters) => mouse::Interaction::ResizingVertically,
        Some(ResizeTarget::Sidebar) | None => mouse::Interaction::ResizingHorizontally,
    };

    mouse_area(
        container(Space::new().width(Fill).height(Fill))
            .width(Fill)
            .height(Fill),
    )
    .interaction(interaction)
    .on_move(Message::ResizeMove)
    .on_release(Message::ResizeEnd)
    .into()
}

/// Vertical hit strip between the sidebar and the work area.
///
/// Hover shows a horizontal-resize cursor; press + drag changes `sidebar_width`.
fn vertical_resize_handle() -> Element<'static, Message> {
    let line = container(Space::new().width(1).height(Fill))
        .width(1)
        .height(Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(ui_theme::current().border)),
            ..container::Style::default()
        });

    mouse_area(
        container(line)
            .width(SPLITTER_HIT)
            .height(Fill)
            .align_x(Horizontal::Center),
    )
    .interaction(mouse::Interaction::ResizingHorizontally)
    .on_press(Message::ResizeStart(ResizeTarget::Sidebar))
    .into()
}

/// Horizontal hit strip between the editor and the parameters panel.
///
/// Hover shows a vertical-resize cursor; press + drag changes `parameters_height`.
fn horizontal_resize_handle() -> Element<'static, Message> {
    let line = container(Space::new().width(Fill).height(1))
        .width(Fill)
        .height(1)
        .style(|_| container::Style {
            background: Some(Background::Color(ui_theme::current().border)),
            ..container::Style::default()
        });

    mouse_area(
        container(line)
            .width(Fill)
            .height(SPLITTER_HIT)
            .align_y(Vertical::Center),
    )
    .interaction(mouse::Interaction::ResizingVertically)
    .on_press(Message::ResizeStart(ResizeTarget::Parameters))
    .into()
}

fn ribbon_tab_style(
    active: bool,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    let background = if active {
        Some(Background::Color(ui_theme::current().ribbon_body_bg))
    } else if matches!(
        status,
        iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed
    ) {
        Some(Background::Color(ui_theme::current().hover))
    } else {
        Some(Background::Color(Color::TRANSPARENT))
    };

    iced::widget::button::Style {
        background,
        text_color: if active {
            ui_theme::current().accent
        } else {
            ui_theme::current().text
        },
        border: Border {
            color: if active {
                ui_theme::current().accent
            } else {
                Color::TRANSPARENT
            },
            width: if active { 2.0 } else { 0.0 },
            radius: 0.0.into(),
        },
        ..iced::widget::button::Style::default()
    }
}

fn ribbon_command_style(
    status: iced::widget::button::Status,
    force_hover: bool,
) -> iced::widget::button::Style {
    let highlighted = force_hover
        || matches!(
            status,
            iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed
        );

    if highlighted {
        iced::widget::button::Style {
            background: Some(Background::Color(ui_theme::current().hover)),
            text_color: ui_theme::current().text,
            border: Border {
                color: ui_theme::current().border,
                width: 1.0,
                radius: 3.0.into(),
            },
            ..iced::widget::button::Style::default()
        }
    } else {
        iced::widget::button::Style {
            background: Some(Background::Color(Color::TRANSPARENT)),
            text_color: ui_theme::current().text,
            border: Border {
                color: Color::TRANSPARENT,
                width: 1.0,
                radius: 3.0.into(),
            },
            ..iced::widget::button::Style::default()
        }
    }
}

fn dock_tool_style(status: iced::widget::button::Status) -> iced::widget::button::Style {
    ribbon_command_style(status, false)
}

/// Compact toolbar button with a Lucide icon and a hover tooltip.
fn dock_icon_button<'a>(
    icon: iced::widget::Text<'a>,
    tip: &'static str,
    message: Message,
) -> Element<'a, Message> {
    tooltip(
        button(icon)
            .padding(Padding::from([4, 8]))
            .style(|_, status| dock_tool_style(status))
            .on_press(message),
        text(tip).size(11),
        tooltip::Position::Bottom,
    )
    .gap(4)
    .into()
}

fn theme_choice_style(
    selected: bool,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    let background = if selected {
        Some(Background::Color(ui_theme::current().accent))
    } else if matches!(
        status,
        iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed
    ) {
        Some(Background::Color(ui_theme::current().hover))
    } else {
        Some(Background::Color(ui_theme::current().ribbon_bg))
    };

    iced::widget::button::Style {
        background,
        text_color: if selected {
            Color::WHITE
        } else {
            ui_theme::current().text
        },
        border: Border {
            color: ui_theme::current().border,
            width: 1.0,
            radius: 3.0.into(),
        },
        ..iced::widget::button::Style::default()
    }
}

fn menu_item_button_style(status: iced::widget::button::Status) -> iced::widget::button::Style {
    // Active stays transparent so Backdrop can paint parents like "Program Window".
    // Leaf items still get Hovered from the real cursor.
    match status {
        iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed => {
            iced::widget::button::Style {
                background: Some(Background::Color(ui_theme::current().hover)),
                text_color: ui_theme::current().text,
                border: Border {
                    color: ui_theme::current().border,
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..iced::widget::button::Style::default()
            }
        }
        _ => iced::widget::button::Style {
            background: Some(Background::Color(Color::TRANSPARENT)),
            text_color: ui_theme::current().text,
            border: Border::default(),
            ..iced::widget::button::Style::default()
        },
    }
}

fn connection_row_style(
    selected: bool,
    connection_status: ConnectionStatus,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    let background = if selected {
        Some(Background::Color(ui_theme::current().accent))
    } else if matches!(
        status,
        iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed
    ) {
        Some(Background::Color(ui_theme::current().hover))
    } else {
        Some(Background::Color(Color::TRANSPARENT))
    };

    // Status colors come from the label widget; keep button text neutral so
    // Connected/Failed greens and reds are not overridden by selection white.
    let text_color = match connection_status {
        ConnectionStatus::Connected => Color::from_rgb(0.12, 0.72, 0.32),
        ConnectionStatus::Failed => Color::from_rgb(0.90, 0.28, 0.28),
        ConnectionStatus::Unknown if selected => Color::WHITE,
        ConnectionStatus::Unknown => ui_theme::current().text,
    };

    iced::widget::button::Style {
        background,
        text_color,
        border: Border::default(),
        ..iced::widget::button::Style::default()
    }
}

fn tab_button_style(
    active: bool,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    let background = if active {
        Some(Background::Color(ui_theme::current().accent))
    } else if matches!(
        status,
        iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed
    ) {
        Some(Background::Color(ui_theme::current().hover))
    } else {
        Some(Background::Color(ui_theme::current().chrome_bg))
    };

    iced::widget::button::Style {
        background,
        text_color: if active {
            Color::WHITE
        } else {
            ui_theme::current().text
        },
        border: Border {
            color: ui_theme::current().border,
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
        _ => ui_theme::current().accent,
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
            ui_theme::current().accent
        } else {
            Color::TRANSPARENT
        })),
        text_color: Some(if selected {
            Color::WHITE
        } else {
            ui_theme::current().text
        }),
        ..container::Style::default()
    }
}

fn editor_style(status: iced::widget::text_editor::Status) -> iced::widget::text_editor::Style {
    let border_color = match status {
        iced::widget::text_editor::Status::Focused { .. } => ui_theme::current().accent,
        _ => ui_theme::current().border,
    };

    iced::widget::text_editor::Style {
        background: Background::Color(ui_theme::current().panel_bg),
        border: Border {
            color: border_color,
            width: 1.0,
            radius: 0.0.into(),
        },
        placeholder: ui_theme::current().muted,
        value: ui_theme::current().text,
        selection: ui_theme::current().accent,
    }
}
