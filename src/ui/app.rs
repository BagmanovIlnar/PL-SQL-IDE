//! IDE shell: menu, Connections/Objects, worksheets, completion, connection modal.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use iced::alignment::{Horizontal, Vertical};
use iced::border::Border;
use iced::keyboard::key::Named;
use iced::keyboard::{self, Key, Modifiers};
use iced::mouse;
use iced::widget::operation::focus as focus_widget;
use iced::widget::text_editor::{Action, Binding, Content, Edit, Motion};
use iced::widget::{
    button, checkbox, column, container, mouse_area, opaque, pick_list, row, rule, scrollable,
    stack, text, text_editor, text_input, tooltip, Space,
};
use iced::{
    Background, Color, Element, Event, Fill, Font, Padding, Point, Subscription, Task, Theme,
};
use iced_fonts::lucide::{
    chevron_down, chevron_right, database, folder_plus, pencil, play, plug, plus, square, trash,
};
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

use crate::core::connections_store::{self, ConnectionsStore};
use crate::core::data_changes;
use crate::core::debug_session::{Breakpoint, DebugCommand, DebugSession};
use crate::core::object_metadata::{list_objects, list_owners, ObjectKind, ObjectOwnerList};
use crate::core::oracle_debug::{self, DebugEngineEvent, DebugEngineRequest};
use crate::core::oracle_debugger_prefs::{self, DebugSystem};
use crate::core::oracle_pool;
use crate::core::postgres_spike::{run_postgres_spike, PostgresSpikeRequest};
use crate::core::{ConnectAs, ConnectionId, ConnectionProfile, DbKind};
use crate::ui::completion::{suggest_keywords, word_prefix_at};
use crate::ui::debug_toolbar::{debug_toolbar, DebugToolbarMessage};
use crate::ui::i18n;
use crate::ui::parameters::{
    parameters_view, value_editor_modal, ParamMessage, ParametersPanel, ValueEditorDraft,
    PANEL_HEIGHT, PANEL_HEIGHT_MAX, PANEL_HEIGHT_MIN,
};
use crate::ui::preferences::{preferences_modal, PreferencesMessage, PreferencesState};
use crate::ui::result_grid::{self, QueryResult, ResultMessage};
use crate::ui::theme::{self as ui_theme, ThemeMode};
use crate::ui::window_kind::WindowKind;

/// Runs the iced IDE window.
pub fn run() -> iced::Result {
    i18n::init();
    iced::application(App::new, update, view)
        .title("PLSQL IDE")
        .theme(|app: &App| {
            if ui_theme::Palette::resolve(app.theme_mode).is_dark {
                Theme::Dark
            } else {
                Theme::Light
            }
        })
        .subscription(subscription)
        .font(LUCIDE_FONT_BYTES)
        .centered()
        .window_size((1400.0, 900.0))
        .run()
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BrowserNode {
    Schema(String),
    Folder {
        schema: String,
        kind: ObjectKind,
    },
    Object {
        schema: String,
        kind: ObjectKind,
        name: String,
    },
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BottomTab {
    Parameters,
    Result,
}

/// Structured outcome of a spike run: text for output plus grid data.
#[derive(Debug, Clone)]
struct SpikePayload {
    sql: String,
    output: String,
    columns: Vec<String>,
    rows: Vec<Vec<String>>,
}

#[derive(Debug)]
struct Worksheet {
    kind: WindowKind,
    content: Content,
    /// Vertical scroll offset of the editor, in logical pixels.
    scroll_y: f32,
    /// Bind / test parameters (SQL Window and Test Window).
    parameters: ParametersPanel,
    /// Result grid of the last executed query (SQL Window).
    result: QueryResult,
    /// Connection selected in Connections when this worksheet was created.
    connection_id: Option<ConnectionId>,
    /// Source breakpoints (1-based line numbers).
    breakpoints: HashSet<usize>,
    /// Disk path after Save / Save As (None = never saved).
    file_path: Option<PathBuf>,
    /// True when buffer differs from last save (or was edited and never saved).
    dirty: bool,
}

impl Worksheet {
    fn tab_label(&self) -> String {
        let name = self
            .file_path
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.kind.tab_title());
        if self.dirty {
            format!("{name} *")
        } else {
            name
        }
    }

    /// Closing needs a Save / Don't save / Cancel prompt.
    fn needs_save_prompt(&self) -> bool {
        self.dirty || self.file_path.is_none()
    }
}

/// Result of attempting to persist a worksheet to disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SaveOutcome {
    Saved,
    Cancelled,
    Failed,
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
    group: String,
    connect_as: ConnectAs,
    db_kind: DbKind,
    host: String,
    port: String,
    database: String,
    username: String,
    password: String,
}

/// Sidebar group entry in the connection dialog pick list;
/// empty name renders localized "No group".
#[derive(Debug, Clone, PartialEq, Eq)]
struct GroupOption(String);

impl std::fmt::Display for GroupOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.0.is_empty() {
            write!(f, "{}", i18n::t("dialog.connection.group_none"))
        } else {
            write!(f, "{}", self.0)
        }
    }
}

impl ConnectionDraft {
    fn from_new(group: &str) -> Self {
        let profile =
            ConnectionProfile::new(i18n::t("dialog.connection.default_name"), DbKind::Postgres);
        let mut draft = Self::from_profile(&profile, true);
        draft.group = group.to_string();
        draft
    }

    fn from_profile(profile: &ConnectionProfile, is_new: bool) -> Self {
        Self {
            id: if is_new { None } else { Some(profile.id) },
            name: profile.name.clone(),
            group: profile.group.clone(),
            connect_as: profile.connect_as,
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
            .map_err(|_| i18n::t("dialog.connection.port_invalid"))?;
        if self.name.trim().is_empty() {
            return Err(i18n::t("dialog.connection.name_required"));
        }
        if self.host.trim().is_empty() {
            return Err(i18n::t("dialog.connection.host_required"));
        }

        let mut profile = ConnectionProfile {
            id: self.id.unwrap_or_else(uuid::Uuid::new_v4),
            name: self.name.trim().to_string(),
            group: self.group.trim().to_string(),
            connect_as: self.connect_as,
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
    fn label(self) -> String {
        i18n::t(match self {
            Self::File => "ribbon.tab.file",
            Self::Session => "ribbon.tab.session",
            Self::Settings => "ribbon.tab.settings",
            Self::Help => "ribbon.tab.help",
        })
    }

    fn all() -> [Self; 4] {
        [Self::File, Self::Session, Self::Settings, Self::Help]
    }
}

/// Ribbon File → New → Program Window menu label for a worksheet kind.
fn window_kind_menu_label(kind: WindowKind) -> String {
    i18n::t(match kind {
        WindowKind::Blank => "menu.blank",
        WindowKind::Function => "menu.function",
        WindowKind::JavaSource => "menu.java_source",
        WindowKind::Package => "menu.package",
        WindowKind::PackageBody => "menu.package_body",
        WindowKind::PackageSpecification => "menu.package_specification",
        WindowKind::Procedure => "menu.procedure",
        WindowKind::Trigger => "menu.trigger",
        WindowKind::Type => "menu.type",
        WindowKind::TypeBody => "menu.type_body",
        WindowKind::TypeSpecification => "menu.type_specification",
        WindowKind::Test => "menu.test_window",
        WindowKind::Sql => "menu.sql_window",
        WindowKind::Command => "menu.command_window",
        WindowKind::ExplainPlan => "menu.explain_plan_window",
    })
}

/// Translated folder title in the Objects tree (expand keys still use [`ObjectKind::label`]).
fn object_kind_display(kind: ObjectKind) -> String {
    i18n::t(match kind {
        ObjectKind::Tables => "objects.kind.tables",
        ObjectKind::Views => "objects.kind.views",
        ObjectKind::Procedures => "objects.kind.procedures",
        ObjectKind::Functions => "objects.kind.functions",
        ObjectKind::Packages => "objects.kind.packages",
    })
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
    /// SQL execution error shown in a modal (with caret jump on close).
    error_modal: Option<String>,
    dialog: Option<ConnectionDraft>,
    dialog_error: Option<String>,
    /// Sidebar group names in display order (persisted in connections.json).
    groups: Vec<String>,
    /// Some(draft) = the "new group" dialog is open with that name draft.
    group_draft: Option<GroupDraft>,
    /// Groups whose rows are hidden; absent = expanded (default).
    collapsed_groups: HashSet<String>,
    /// Highlighted group header in the sidebar (Delete acts on it).
    selected_group: Option<String>,
    /// Open Preferences dialog (`None` when closed).
    preferences: Option<PreferencesState>,
    /// Open Value ellipsis editor for a parameter row.
    value_editor: Option<ValueEditorDraft>,
    /// Open "…" value editor for a result-grid cell.
    cell_editor: Option<result_grid::CellEditorDraft>,
    /// Active result-grid column resize drag.
    column_drag: Option<result_grid::ColumnDrag>,
    /// Mode of the in-flight spike run (execute / fetch next / fetch all).
    fetch_mode: FetchMode,
    completion: Vec<String>,
    completion_prefix: String,
    selected_completion: usize,
    busy: bool,
    /// Monotonic id of the running spike; Stop bumps it so late results are dropped.
    spike_run: u64,
    /// Active tab of the bottom panel (Parameters / Results).
    bottom_tab: BottomTab,
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
    /// Oracle debug session controller (toolbar / hotkeys).
    debug_session: DebugSession,
    /// Debug system selected in the toolbar combobox.
    debug_system: DebugSystem,
    /// Tab index waiting for Save / Don't save / Cancel before close.
    close_confirm: Option<usize>,
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
    DismissErrorModal,
    SelectTab(usize),
    CloseTab(usize),
    /// Save the pending tab, then close it.
    CloseTabSave,
    /// Discard changes and close the pending tab.
    CloseTabDiscard,
    /// Keep the pending tab open.
    CloseTabCancel,
    SelectRibbonTab(RibbonTab),
    RibbonHover(Option<RibbonHoverTarget>),
    SetThemeMode(ThemeMode),
    MenuNew(WindowKind),
    MenuOpenStub(String),
    MenuSaveStub,
    MenuSaveAsStub,
    MenuExit,
    MenuExecute,
    /// Cancel waiting for the running SQL/Command execution.
    MenuStop,
    SelectConnection(ConnectionId),
    SelectGroup(String),
    DeleteGroup,
    AddConnection,
    EditConnection,
    DeleteConnection,
    /// Open the "new sidebar group" dialog.
    AddGroup,
    EditGroup,
    GroupDraftChanged(String),
    GroupDialogSave,
    GroupDialogCancel,
    /// Attempt to open a live session for the selected Connections profile.
    ConnectSelected,
    /// Async result of a Connect attempt for `ConnectionId`.
    ConnectFinished(ConnectionId, Result<(), String>),
    /// Persistent Oracle session opened in background after Connect.
    PoolOpened(ConnectionId, Result<(), String>),
    /// Close the Connect error modal.
    DismissConnectError,
    DialogNameChanged(String),
    DialogGroupChanged(String),
    DialogConnectAsChanged(ConnectAs),
    DialogDbKindChanged(DbKind),
    DialogHostChanged(String),
    DialogPortChanged(String),
    DialogDatabaseChanged(String),
    DialogUsernameChanged(String),
    DialogPasswordChanged(String),
    DialogSave,
    DialogCancel,
    /// Open the Preferences modal from Settings.
    OpenPreferences,
    /// Preferences tree / buttons.
    Preferences(PreferencesMessage),
    /// Parameters grid / Value editor messages (SQL & Test windows).
    Param(ParamMessage),
    ApplyCompletion(String),
    /// Apply the currently highlighted completion item (Enter).
    ApplySelectedCompletion,
    /// Move the completion highlight by `delta` (−1 / +1).
    CompletionNavigate(i32),
    /// Close the completion popup without inserting (Escape).
    DismissCompletion,
    SpikeFinished(u64, Result<SpikePayload, String>),
    /// Generated UPDATE/INSERT/DELETE batch from the result grid finished.
    PostFinished(u64, Result<String, String>),
    /// Switch the bottom panel between Parameters and Results.
    BottomTab(BottomTab),
    /// Result-grid interactions (edit mode, cell edits, fetch, post).
    Grid(ResultMessage),
    /// Begin dragging a panel resize border.
    ResizeStart(ResizeTarget),
    /// Cursor moved while a panel resize drag is active (window coordinates).
    ResizeMove(Point),
    /// End a panel resize drag (left mouse button released).
    ResizeEnd,
    /// Toggle a breakpoint on the given 1-based line in the active worksheet.
    ToggleBreakpoint(usize),
    /// Debug toolbar actions.
    Debug(DebugToolbarMessage),
    /// Async result from the live Oracle debug engine.
    DebugEngineFinished(Result<DebugEngineEvent, String>),
}

impl App {
    fn new() -> Self {
        let stored = connections_store::load();
        let selected_connection = stored
            .selected
            .filter(|id| stored.connections.iter().any(|item| item.id == *id))
            .or_else(|| stored.connections.first().map(|item| item.id));

        ui_theme::apply(ThemeMode::System);
        Self {
            worksheets: Vec::new(),
            active_tab: 0,
            ribbon_tab: RibbonTab::File,
            ribbon_hover: None,
            theme_mode: ThemeMode::System,
            connections: stored.connections,
            groups: stored.groups,
            group_draft: None,
            collapsed_groups: HashSet::new(),
            selected_group: None,
            selected_connection,
            connection_status: HashMap::new(),
            connect_error: None,
            error_modal: None,
            dialog: None,
            dialog_error: None,
            preferences: None,
            value_editor: None,
            cell_editor: None,
            column_drag: None,
            fetch_mode: FetchMode::Execute,
            completion: Vec::new(),
            completion_prefix: String::new(),
            selected_completion: 0,
            busy: false,
            spike_run: 0,
            bottom_tab: BottomTab::Parameters,
            metrics: i18n::t("status.ready"),
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
            debug_session: DebugSession::default(),
            debug_system: oracle_debugger_prefs::load().default_debug_system,
            close_confirm: None,
        }
    }

    /// Write Connections list + selection to `{config}/connections.json`.
    fn persist_connections(&self) {
        connections_store::save(&ConnectionsStore {
            connections: self.connections.clone(),
            selected: self.selected_connection,
            groups: self.groups.clone(),
        });
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
            None => i18n::t("worksheet.no_connection"),
        }
    }

    fn open_window(&mut self, kind: WindowKind) {
        let connection_id = self.selected_connection;
        let connection_label = connection_id
            .and_then(|id| self.connections.iter().find(|item| item.id == id))
            .map(|profile| profile.name.clone())
            .unwrap_or_else(|| i18n::t("worksheet.no_connection"));

        self.worksheets.push(Worksheet {
            kind,
            content: Content::with_text(kind.template()),
            scroll_y: 0.0,
            parameters: ParametersPanel::default(),
            result: QueryResult::default(),
            connection_id,
            breakpoints: HashSet::new(),
            file_path: None,
            dirty: false,
        });
        self.active_tab = self.worksheets.len() - 1;
        self.completion.clear();
        self.value_editor = None;
        self.close_confirm = None;
        let kind_title = kind.tab_title();
        self.metrics = i18n::t_args(
            "status.opened",
            &[
                ("kind", kind_title.as_str()),
                ("connection", connection_label.as_str()),
            ],
        );
    }

    /// Removes a worksheet tab and fixes `active_tab` (may leave the workspace empty).
    fn remove_worksheet(&mut self, index: usize) -> Task<Message> {
        if index >= self.worksheets.len() {
            return Task::none();
        }
        let owner = self.debug_session.tab;
        self.worksheets.remove(index);
        // Closing the owning window ends its debug session (the block runs on
        // a pooled session; leaving it would keep the debugger active
        // invisibly). Other tabs closing must not touch the session.
        let stop_task = if self.debug_session.active && owner == Some(index) {
            self.apply_debug_command(DebugCommand::Stop)
        } else {
            Task::none()
        };
        match self.debug_session.tab {
            Some(t) if t == index => self.debug_session.tab = None,
            Some(t) if t > index => self.debug_session.tab = Some(t - 1),
            _ => {}
        }
        if self.worksheets.is_empty() {
            self.active_tab = 0;
        } else if self.active_tab >= self.worksheets.len() {
            self.active_tab = self.worksheets.len() - 1;
        } else if index < self.active_tab {
            self.active_tab -= 1;
        }
        self.completion.clear();
        self.value_editor = None;
        self.close_confirm = None;
        stop_task
    }

    /// Persist worksheet `index` to disk. Uses Save As when `force_save_as` or no path yet.
    fn save_worksheet_at(&mut self, index: usize, force_save_as: bool) -> SaveOutcome {
        let Some(sheet) = self.worksheets.get(index) else {
            return SaveOutcome::Cancelled;
        };
        let text = sheet.content.text();
        let suggested = sheet
            .file_path
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| default_file_name(sheet.kind));

        let path = if force_save_as || sheet.file_path.is_none() {
            let ext = file_extension(sheet.kind);
            let picked = rfd::FileDialog::new()
                .set_title(i18n::t("dialog.save.title"))
                .set_file_name(&suggested)
                .add_filter(i18n::t("dialog.save.filter"), &[ext])
                .add_filter(i18n::t("dialog.save.filter_all"), &["*"])
                .save_file();
            match picked {
                Some(path) => path,
                None => return SaveOutcome::Cancelled,
            }
        } else {
            sheet.file_path.clone().expect("path checked")
        };

        match std::fs::write(&path, text.as_bytes()) {
            Ok(()) => {
                if let Some(sheet) = self.worksheets.get_mut(index) {
                    sheet.file_path = Some(path.clone());
                    sheet.dirty = false;
                }
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string());
                self.metrics = i18n::t_args("status.saved", &[("name", name.as_str())]);
                SaveOutcome::Saved
            }
            Err(error) => {
                self.metrics = i18n::t_args(
                    "status.save_failed",
                    &[("error", error.to_string().as_str())],
                );
                SaveOutcome::Failed
            }
        }
    }

    fn save_active_worksheet(&mut self, force_save_as: bool) -> SaveOutcome {
        if self.worksheets.is_empty() {
            self.metrics = i18n::t("status.no_worksheet");
            return SaveOutcome::Cancelled;
        }
        self.save_worksheet_at(self.active_tab, force_save_as)
    }

    fn connection_key(&self) -> String {
        self.active_worksheet_profile()
            .map(|profile| format!("{}@{}", profile.username, profile.database))
            .unwrap_or_else(|| "user@db".into())
    }

    fn sync_session_breakpoints_from_sheet(&mut self) {
        let bps = self
            .worksheets
            .get(self.active_tab)
            .map(|sheet| {
                let mut list: Vec<_> = sheet
                    .breakpoints
                    .iter()
                    .copied()
                    .map(|line| Breakpoint { line })
                    .collect();
                list.sort_by_key(|b| b.line);
                list
            })
            .unwrap_or_default();
        self.debug_session.breakpoints = bps;
    }

    fn apply_debug_command(&mut self, command: DebugCommand) -> Task<Message> {
        tracing::info!("debug: toolbar command {command:?}");
        if matches!(command, DebugCommand::Start(_)) {
            self.debug_session.tab = Some(self.active_tab);
        } else if self.debug_session.active
            && !matches!(command, DebugCommand::Stop)
            && self.debug_session.tab != Some(self.active_tab)
        {
            self.metrics = i18n::t("debug.other_tab");
            self.last_output = i18n::t("debug.other_tab");
            return Task::none();
        }
        self.sync_session_breakpoints_from_sheet();
        if let Some(sheet) = self.worksheets.get(self.active_tab) {
            let line = ((sheet.scroll_y / EDITOR_LINE_HEIGHT).floor() as usize) + 1;
            self.debug_session.cursor_line = Some(line.max(1));
        }

        let Some(profile) = self.active_worksheet_profile().cloned() else {
            self.metrics = i18n::t("status.no_worksheet_connection");
            self.last_output = i18n::t("status.no_worksheet_connection");
            return Task::none();
        };
        if profile.db_kind != DbKind::Oracle {
            self.last_output = i18n::t("debug.need_oracle");
            self.metrics = i18n::t("debug.need_oracle");
            return Task::none();
        }
        if self.busy {
            return Task::none();
        }

        let prefs = oracle_debugger_prefs::load();
        let key = self.connection_key();
        let mut source = self.active_content().map(|c| c.text()).unwrap_or_default();
        // Test window: bind `:NAME` parameters before handing the source to
        // the engine; remember the line offset so pauses map back to editor
        // lines (the wrapper injects declarations above the block).
        self.debug_session.bind_offset = 0;
        let test_window = self
            .worksheets
            .get(self.active_tab)
            .map(|sheet| sheet.kind == WindowKind::Test)
            .unwrap_or(false);
        if test_window && profile.db_kind == DbKind::Oracle {
            let text = source.clone();
            let params = auto_fill_bind_params(self, &text);
            if !params.is_empty() {
                let (wrapped, offset) = crate::ui::parameters::bind_wrap(&text, &params, false);
                source = wrapped;
                self.debug_session.bind_offset = offset;
            }
        }
        let request = DebugEngineRequest {
            profile,
            command,
            prefs,
            breakpoints: self.debug_session.breakpoints.clone(),
            cursor_line: self.debug_session.cursor_line,
            source,
            connection_key: key,
        };

        self.busy = true;
        self.metrics = i18n::t("debug.working");
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || oracle_debug::run_debug_command(request))
                    .await
                    .map_err(|err| err.to_string())
                    .and_then(|r| r.map_err(|err| err.to_string()))
            },
            Message::DebugEngineFinished,
        )
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
                    self.metrics = i18n::t("status.parameter_added");
                }
            }
            ParamMessage::Delete => {
                if let Some(panel) = self.active_parameters_mut() {
                    if panel.delete_selected() {
                        self.metrics = i18n::t("status.parameter_deleted");
                    } else {
                        self.metrics = i18n::t("status.parameter_select_delete");
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
                    self.metrics = i18n::t("status.parameter_value_updated");
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
        // DB object names (tables/views/procs/...) first, then SQL keywords.
        let needle = prefix.to_ascii_uppercase();
        let mut items: Vec<String> = Vec::new();
        for names in self.object_cache.values() {
            for name in names {
                if name.to_ascii_uppercase().starts_with(&needle)
                    && needle.len() < name.len()
                    && !items.iter().any(|item| item == name)
                {
                    items.push(name.clone());
                }
            }
        }
        for keyword in suggest_keywords(&prefix, 8) {
            if !items.iter().any(|item| item == &keyword) {
                items.push(keyword);
            }
        }
        items.truncate(8);
        if items.is_empty() {
            return;
        }
        self.completion = items;
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
        if let Some(sheet) = self.worksheets.get_mut(self.active_tab) {
            sheet.dirty = true;
        }
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

            let marks_dirty = matches!(action, Action::Edit(_));
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
            if marks_dirty || restored_empty {
                if let Some(sheet) = app.worksheets.get_mut(app.active_tab) {
                    sheet.dirty = true;
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
                if let Some(sheet) = app.worksheets.get_mut(app.active_tab) {
                    let mut current = sheet.content.text();
                    if !current.ends_with('\n') && !current.is_empty() {
                        current.push(' ');
                    }
                    current.push_str(&name);
                    sheet.content = Content::with_text(&current);
                    sheet.dirty = true;
                    app.metrics = i18n::t_args("status.inserted", &[("name", name.as_str())]);
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
                    app.metrics = i18n::t_args(
                        "status.objects_loaded",
                        &[
                            ("count", &app.object_owners.len().to_string()),
                            ("name", list.current_user.as_str()),
                        ],
                    );
                    // Warm the completion dictionary with the current
                    // user's objects (all kinds) in the background.
                    let mut tasks = Vec::new();
                    if let Some(owner) = app.object_owners.first().cloned() {
                        if let Some(profile) = app.selected_profile() {
                            for kind in ObjectKind::for_db(profile.db_kind) {
                                tasks.push(start_load_object_folder(app, owner.clone(), *kind));
                            }
                        }
                    }
                    return Task::batch(tasks);
                }
                Err(error) => {
                    app.object_owners.clear();
                    app.objects_error = Some(error.clone());
                    app.metrics = i18n::t("status.objects_load_failed");
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
                    app.metrics = i18n::t("status.objects_folder_failed");
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
            let theme_name = mode.label();
            app.metrics = i18n::t_args("theme.changed", &[("name", theme_name.as_str())]);
            Task::none()
        }
        Message::CloseTab(index) => {
            if index >= app.worksheets.len() {
                return Task::none();
            }
            if app.worksheets[index].needs_save_prompt() {
                app.active_tab = index;
                app.close_confirm = Some(index);
                return Task::none();
            }
            app.remove_worksheet(index)
        }
        Message::CloseTabSave => {
            let Some(index) = app.close_confirm else {
                return Task::none();
            };
            match app.save_worksheet_at(index, false) {
                SaveOutcome::Saved => return app.remove_worksheet(index),
                SaveOutcome::Cancelled | SaveOutcome::Failed => {}
            }
            Task::none()
        }
        Message::CloseTabDiscard => {
            if let Some(index) = app.close_confirm.take() {
                return app.remove_worksheet(index);
            }
            Task::none()
        }
        Message::CloseTabCancel => {
            app.close_confirm = None;
            Task::none()
        }
        Message::MenuNew(kind) => {
            app.open_window(kind);
            Task::none()
        }
        Message::MenuOpenStub(label) => {
            app.metrics = i18n::t_args("status.open_stub", &[("label", label.as_str())]);
            Task::none()
        }
        Message::MenuSaveStub => {
            let _ = app.save_active_worksheet(false);
            Task::none()
        }
        Message::MenuSaveAsStub => {
            let _ = app.save_active_worksheet(true);
            Task::none()
        }
        Message::MenuExit => iced::exit(),
        Message::MenuExecute => {
            tracing::info!("execute requested");
            run_spike(app, FetchMode::Execute)
        }
        Message::MenuStop => {
            let owns_debug = app.debug_session.active
                && app.debug_session.tab == Some(app.active_tab);
            if owns_debug {
                return app.apply_debug_command(DebugCommand::Stop);
            }
            if !app.busy {
                return Task::none();
            }
            // ponytail: cancels the UI wait only; server-side kill needs a
            // cancellation token threaded through the spike runs.
            app.busy = false;
            app.spike_run += 1;
            app.last_output = i18n::t("status.cancelled");
            app.metrics = i18n::t("status.cancelled");
            Task::none()
        }
        Message::SelectConnection(id) => {
            app.selected_connection = Some(id);
            app.selected_group = None;
            app.persist_connections();
            if let Some(profile) = app.selected_profile() {
                app.metrics = i18n::t_args(
                    "status.selected_connection",
                    &[("name", profile.name.as_str())],
                );
            }
            if app.selected_is_connected() {
                start_load_object_roots(app)
            } else {
                app.clear_object_browser();
                Task::none()
            }
        }
        Message::AddConnection => {
            let group = app.selected_group.clone().unwrap_or_default();
            app.dialog = Some(ConnectionDraft::from_new(&group));
            app.dialog_error = None;
            Task::none()
        }
        Message::AddGroup => {
            app.group_draft = Some(GroupDraft {
                original: None,
                name: String::new(),
            });
            Task::none()
        }
        Message::EditGroup => {
            if let Some(name) = app.selected_group.clone() {
                app.group_draft = Some(GroupDraft {
                    original: Some(name.clone()),
                    name,
                });
            }
            Task::none()
        }
        Message::GroupDraftChanged(value) => {
            if let Some(draft) = &mut app.group_draft {
                draft.name = value;
            }
            Task::none()
        }
        Message::GroupDialogSave => {
            if let Some(draft) = app.group_draft.take() {
                let name = draft.name.trim().to_string();
                match draft.original {
                    None => {
                        if !name.is_empty() && !app.groups.contains(&name) {
                            app.groups.push(name);
                            app.persist_connections();
                        }
                    }
                    Some(old) => {
                        if !name.is_empty() && (name == old || !app.groups.contains(&name)) {
                            if let Some(slot) = app.groups.iter_mut().find(|g| **g == old) {
                                *slot = name.clone();
                            }
                            for profile in &mut app.connections {
                                if profile.group == old {
                                    profile.group = name.clone();
                                }
                            }
                            if app.collapsed_groups.remove(&old) {
                                app.collapsed_groups.insert(name.clone());
                            }
                            app.selected_group = Some(name);
                            app.persist_connections();
                        }
                    }
                }
            }
            Task::none()
        }
        Message::GroupDialogCancel => {
            app.group_draft = None;
            Task::none()
        }
        Message::SelectGroup(name) => {
            if app.selected_group.as_deref() == Some(name.as_str()) {
                app.selected_group = None;
            } else {
                app.selected_group = Some(name.clone());
                app.selected_connection = None;
            }
            if !app.collapsed_groups.remove(&name) {
                app.collapsed_groups.insert(name);
            }
            Task::none()
        }
        Message::DeleteGroup => {
            if let Some(name) = app.selected_group.take() {
                app.groups.retain(|group| *group != name);
                for profile in &mut app.connections {
                    if profile.group == name {
                        profile.group.clear();
                    }
                }
                app.collapsed_groups.remove(&name);
                app.persist_connections();
                app.metrics = i18n::t_args("status.group_deleted", &[("name", name.as_str())]);
            }
            Task::none()
        }
        Message::EditConnection => {
            if let Some(profile) = app.selected_profile().cloned() {
                app.dialog = Some(ConnectionDraft::from_profile(&profile, false));
                app.dialog_error = None;
            } else {
                app.metrics = i18n::t("status.select_to_edit");
            }
            Task::none()
        }
        Message::DeleteConnection => {
            let Some(id) = app.selected_connection else {
                app.metrics = i18n::t("status.select_to_delete");
                return Task::none();
            };
            oracle_pool::close(id);
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
            app.persist_connections();
            app.metrics = i18n::t("status.connection_deleted");
            if app.selected_is_connected() {
                start_load_object_roots(app)
            } else {
                app.clear_object_browser();
                Task::none()
            }
        }
        Message::ConnectSelected => run_connect(app),
        Message::PoolOpened(id, result) => {
            if let Err(err) = result {
                app.last_output = format!("Connected, but persistent Oracle session failed: {err}");
            }
            let _ = id;
            Task::none()
        }
        Message::ConnectFinished(id, result) => {
            app.busy = false;
            match result {
                Ok(()) => {
                    app.connection_status
                        .insert(id, ConnectionStatus::Connected);
                    app.connect_error = None;
                    let mut tasks: Vec<Task<Message>> = Vec::new();
                    if let Some(profile) =
                        app.connections.iter().find(|item| item.id == id).cloned()
                    {
                        if profile.db_kind == DbKind::Oracle {
                            // Blocking OCI connect: keep it off the UI thread.
                            tasks.push(Task::perform(
                                async move {
                                    let result = tokio::task::spawn_blocking(move || {
                                        oracle_pool::open_target(&profile)
                                    })
                                    .await
                                    .map_err(|err| err.to_string())
                                    .and_then(|r| r.map_err(|err| err.to_string()));
                                    Message::PoolOpened(id, result)
                                },
                                |message| message,
                            ));
                        }
                    }
                    let name = app
                        .connections
                        .iter()
                        .find(|item| item.id == id)
                        .map(|item| item.name.clone())
                        .unwrap_or_else(|| "connection".into());
                    app.metrics = i18n::t_args("status.connected", &[("name", name.as_str())]);
                    if app.selected_connection == Some(id) {
                        tasks.push(start_load_object_roots(app));
                    }
                    Task::batch(tasks)
                }
                Err(error) => {
                    app.connection_status.insert(id, ConnectionStatus::Failed);
                    app.connect_error = Some(error);
                    app.metrics = i18n::t("status.connect_failed");
                    if app.objects_connection == Some(id) {
                        app.clear_object_browser();
                    }
                    Task::none()
                }
            }
        }
        Message::DismissErrorModal => {
            let error = app.error_modal.take().unwrap_or_default();
            // ponytail: self-check runs on the text as it is NOW; if the user
            // edited after the failed run, the hint may point one edit off.
            let sql_text = app
                .active_content()
                .map(|content| content.text())
                .unwrap_or_default();
            let position = parse_error_position(&error).or_else(|| self_check_position(&sql_text));
            if let Some((line, col)) = position {
                if let Some(content) = app.active_content_mut() {
                    content.perform(Action::Select(Motion::DocumentStart));
                    for _ in 0..line.saturating_sub(1) {
                        content.perform(Action::Move(Motion::Down));
                    }
                    content.perform(Action::Move(Motion::Home));
                    for _ in 0..col.saturating_sub(1) {
                        content.perform(Action::Move(Motion::Right));
                    }
                }
            }
            Task::none()
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
        Message::DialogGroupChanged(value) => {
            if let Some(dialog) = &mut app.dialog {
                dialog.group = value;
            }
            Task::none()
        }
        Message::DialogConnectAsChanged(value) => {
            if let Some(dialog) = &mut app.dialog {
                dialog.connect_as = value;
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
        Message::OpenPreferences => {
            app.preferences = Some(PreferencesState::new());
            Task::none()
        }
        Message::Preferences(message) => {
            let should_close = app
                .preferences
                .as_mut()
                .map(|state| state.update(message))
                .unwrap_or(true);
            if should_close {
                app.preferences = None;
            }
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
                        app.metrics = i18n::t("status.connection_updated");
                    } else {
                        app.connections.push(profile);
                        app.metrics = i18n::t("status.connection_added");
                    }
                    app.selected_connection = Some(id);
                    app.dialog = None;
                    app.dialog_error = None;
                    app.persist_connections();
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
        Message::SpikeFinished(run_id, outcome) => {
            tracing::info!(run_id, current = app.spike_run, "spike finished");
            if run_id != app.spike_run {
                return Task::none(); // stale result of a cancelled run
            }
            app.busy = false;
            match outcome {
                Ok(payload) => {
                    tracing::info!(
                        cols = payload.columns.len(),
                        rows = payload.rows.len(),
                        "grid updated"
                    );
                    app.last_output = payload.output;
                    app.metrics = i18n::t("status.execute_completed");
                    let mode = app.fetch_mode;
                    if let Some(sheet) = app.worksheets.get_mut(app.active_tab) {
                        match mode {
                            FetchMode::Execute => {
                                if !payload.columns.is_empty() {
                                    sheet.result = QueryResult {
                                        source_sql: payload.sql,
                                        columns: payload.columns,
                                        rows: payload.rows,
                                        ..QueryResult::default()
                                    };
                                    if sheet.kind == WindowKind::Sql {
                                        app.bottom_tab = BottomTab::Result;
                                    }
                                }
                            }
                            // Paging continues into the same buffer.
                            FetchMode::Next | FetchMode::All => {
                                sheet.result.rows.extend(payload.rows);
                            }
                        }
                    }
                }
                Err(error) => {
                    app.last_output = error.clone();
                    app.error_modal = Some(error);
                    app.metrics = i18n::t("status.execute_failed");
                }
            }
            Task::none()
        }
        Message::PostFinished(run_id, outcome) => {
            if run_id != app.spike_run {
                return Task::none(); // stale result of a cancelled run
            }
            app.busy = false;
            match outcome {
                Ok(output) => {
                    app.last_output = output;
                    app.metrics = i18n::t("status.post_ok");
                    if let Some(sheet) = app.worksheets.get_mut(app.active_tab) {
                        sheet.result.edits.clear();
                        sheet.result.inserted.clear();
                        sheet.result.deleted.clear();
                        sheet.result.edit_mode = false;
                    }
                }
                Err(error) => {
                    app.last_output = error.clone();
                    app.error_modal = Some(error);
                    app.metrics = i18n::t("status.execute_failed");
                }
            }
            Task::none()
        }
        Message::BottomTab(tab) => {
            app.bottom_tab = tab;
            Task::none()
        }
        Message::Grid(message) => handle_grid_message(app, message),
        Message::ResizeStart(target) => {
            app.resize_drag = Some(ResizeDrag { target, last: None });
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
                            app.parameters_height = next.clamp(PANEL_HEIGHT_MIN, PANEL_HEIGHT_MAX);
                        }
                        ResizeTarget::Sidebar => {
                            let next = app.sidebar_width + (point.x - last.x);
                            app.sidebar_width = next.clamp(SIDEBAR_WIDTH_MIN, SIDEBAR_WIDTH_MAX);
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
        Message::ToggleBreakpoint(line) => {
            if let Some(sheet) = app.worksheets.get_mut(app.active_tab) {
                if !sheet.breakpoints.remove(&line) {
                    sheet.breakpoints.insert(line);
                }
            }
            Task::none()
        }
        Message::Debug(message) => match message {
            DebugToolbarMessage::Start => {
                app.apply_debug_command(DebugCommand::Start(app.debug_system))
            }
            DebugToolbarMessage::StartWith(system) => {
                app.debug_system = system;
                app.apply_debug_command(DebugCommand::Start(system))
            }
            DebugToolbarMessage::Run => {
                let owns_debug = app.debug_session.active
                    && app.debug_session.tab == Some(app.active_tab);
                if owns_debug {
                    app.apply_debug_command(DebugCommand::Run)
                } else {
                    // No debug session here — run the block fully, no debugger.
                    run_spike(app, FetchMode::Execute)
                }
            }
            DebugToolbarMessage::StepInto => app.apply_debug_command(DebugCommand::StepInto),
            DebugToolbarMessage::StepOver => app.apply_debug_command(DebugCommand::StepOver),
            DebugToolbarMessage::StepOut => app.apply_debug_command(DebugCommand::StepOut),
            DebugToolbarMessage::RunToCursor => app.apply_debug_command(DebugCommand::RunToCursor),
            DebugToolbarMessage::RunToNextException => {
                app.apply_debug_command(DebugCommand::RunToNextException)
            }
            DebugToolbarMessage::Stop => {
                let owns_debug = app.debug_session.active
                    && app.debug_session.tab == Some(app.active_tab);
                if owns_debug {
                    app.apply_debug_command(DebugCommand::Stop)
                } else if app.busy {
                    // Plain run in flight — cancel it (same as toolbar Stop).
                    // ponytail: cancels the UI wait only; server-side kill
                    // needs a cancellation token threaded through the spike.
                    app.busy = false;
                    app.spike_run += 1;
                    app.last_output = i18n::t("status.cancelled");
                    app.metrics = i18n::t("status.cancelled");
                    Task::none()
                } else {
                    Task::none()
                }
            }
            DebugToolbarMessage::ExecuteSqlInDebugSession => {
                app.apply_debug_command(DebugCommand::ExecuteSqlInDebugSession)
            }
        },
        Message::DebugEngineFinished(result) => {
            app.busy = false;
            match result {
                Ok(event) => {
                    tracing::info!(
                        "debug: engine event active={} line={:?} status={}",
                        event.active,
                        event.current_line,
                        event.status
                    );
                    // The block ended (finished or failed) — the session is
                    // over, so toolbar buttons grey out until the next Start.
                    let over = event.status.starts_with("program finished")
                        || event.status.starts_with("block failed");
                    app.debug_session.active = event.active && !over;
                    app.debug_session.system = event.system;
                    app.debug_session.status = event.status.clone();
                    // Map wrapper-injected lines back to the editor text.
                    app.debug_session.current_line = event
                        .current_line
                        .map(|l| l.saturating_sub(app.debug_session.bind_offset).max(1));
                    app.metrics = event.status.clone();
                    if event.status.starts_with("block failed") {
                        app.error_modal = Some(event.message.clone());
                    }
                    let mut out = event.message;
                    if !event.log.is_empty() {
                        out.push('\n');
                        out.push_str(&event.log.join("\n"));
                    }
                    if !event.variables.is_empty() {
                        out.push_str("\n--- variables ---\n");
                        out.push_str(&event.variables.join("\n"));
                    }
                    app.last_output = out;
                }
                Err(err) => {
                    app.metrics = i18n::t("debug.failed");
                    app.last_output = err.clone();
                    app.error_modal = Some(err);
                }
            }
            Task::none()
        }
    }
}

fn subscription(_app: &App) -> Subscription<Message> {
    iced::event::listen_with(|event, _status, _window| {
        let Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) = event else {
            return None;
        };
        debug_hotkey(key, modifiers)
    })
}

fn debug_hotkey(key: Key, modifiers: Modifiers) -> Option<Message> {
    match key.as_ref() {
        Key::Named(Named::F9) if modifiers.control() => {
            Some(Message::Debug(DebugToolbarMessage::Run))
        }
        Key::Named(Named::F9) => Some(Message::Debug(DebugToolbarMessage::Start)),
        Key::Character("n") | Key::Character("N") if modifiers.control() && !modifiers.shift() => {
            Some(Message::Debug(DebugToolbarMessage::StepInto))
        }
        Key::Character("o") | Key::Character("O") if modifiers.control() && !modifiers.shift() => {
            Some(Message::Debug(DebugToolbarMessage::StepOver))
        }
        Key::Character("t") | Key::Character("T") if modifiers.control() && !modifiers.shift() => {
            Some(Message::Debug(DebugToolbarMessage::StepOut))
        }
        _ => None,
    }
}

fn run_connect(app: &mut App) -> Task<Message> {
    if app.busy {
        return Task::none();
    }
    let Some(profile) = app.selected_profile().cloned() else {
        app.metrics = i18n::t("status.select_to_connect");
        return Task::none();
    };

    let id = profile.id;
    app.busy = true;
    app.connect_error = None;
    app.metrics = i18n::t_args("status.connecting", &[("name", profile.name.as_str())]);

    match profile.db_kind {
        DbKind::Postgres => {
            let request = PostgresSpikeRequest {
                connection_url: profile.postgres_url(),
                sql: Some("select 1::int as value".into()),
                timeout_ms: Some(10_000),
                cancel: false,
                max_rows: Some(10),
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
                    connect_as: profile.connect_as,
                    sql: Some("select 1 as value from dual".into()),
                    timeout_ms: Some(10_000),
                    cancel: false,
                    max_rows: Some(10),
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
                app.connect_error = Some(i18n::t("status.oracle_disabled"));
                app.metrics = i18n::t("status.connect_failed");
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
    app.metrics = i18n::t_args("status.loading_objects", &[("name", profile.name.as_str())]);

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

/// Skips leading whitespace and single/multi-line comments before the first keyword.
fn strip_leading_comments(text: &str) -> &str {
    let mut rest = text.trim_start();
    loop {
        if let Some(after) = rest.strip_prefix("--") {
            rest = match after.split_once('\n') {
                Some((_, tail)) => tail.trim_start(),
                None => "",
            };
        } else if rest.strip_prefix("/*").is_some() {
            rest = match rest[2..].find("*/") {
                Some(end) => rest[2 + end + 2..].trim_start(),
                None => "",
            };
        } else {
            return rest;
        }
    }
}

/// What a spike run does with the result grid buffer.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
enum FetchMode {
    /// Fresh execution — replaces the buffer with the first page.
    #[default]
    Execute,
    /// Append the next page to the grid.
    Next,
    /// Append all remaining rows.
    All,
}

/// Helper column added by the Oracle ROWNUM paging wrapper; stripped after fetch.
const PAGE_HELPER_COLUMN: &str = "_PLSQLIDE_RN";

/// Wraps a top-level SELECT for server-side paging: skip `offset` rows,
/// fetch `limit` rows (or everything when `None`). DML and multi-statement
/// text pass through untouched. Oracle uses the ROWNUM trick (pre-12c safe);
/// Postgres uses OFFSET/FETCH.
/// Test window: discover `:NAME` binds in the block and add any new ones to
/// the bottom parameters panel (default type, empty value — values survive
/// re-parses). Returns the panel rows for bind wrapping.
fn auto_fill_bind_params(app: &mut App, text: &str) -> Vec<crate::ui::parameters::Parameter> {
    let names = crate::ui::parameters::parse_binds(text);
    let Some(kind) = app
        .active_worksheet_profile()
        .map(|p| crate::ui::parameters::ParamDataType::default_for(p.db_kind))
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let Some(panel) = app.active_parameters_mut() {
        for name in names {
            if !panel
                .rows
                .iter()
                .any(|p| p.name.eq_ignore_ascii_case(&name))
            {
                panel
                    .rows
                    .push(crate::ui::parameters::Parameter::new(name.clone(), kind));
            }
        }
        out = panel.rows.clone();
    }
    out
}

fn page_select(sql: &str, kind: DbKind, offset: usize, limit: Option<usize>) -> String {
    let trimmed = strip_leading_comments(sql).trim_end_matches(';').trim();
    let upper = trimmed.to_ascii_uppercase();
    if !upper.starts_with("SELECT") || trimmed.contains(';') || upper.contains("FOR UPDATE") {
        return sql.to_string();
    }
    match kind {
        DbKind::Postgres => {
            let mut wrapped = format!("SELECT * FROM ({trimmed}) OFFSET {offset} ROWS");
            if let Some(cap) = limit {
                wrapped.push_str(&format!(" FETCH FIRST {cap} ROWS ONLY"));
            }
            wrapped
        }
        DbKind::Oracle => match (offset, limit) {
            // First page without a helper column; `ROWNUM` bounds the fetch.
            (0, None) => trimmed.to_string(),
            (0, Some(cap)) => format!("SELECT * FROM ({trimmed}) WHERE ROWNUM <= {cap}"),
            (offset, limit) => {
                let end = match limit {
                    Some(cap) => offset + cap,
                    None => 1_000_000_000,
                };
                format!(
                    "SELECT * FROM (SELECT t.*, ROWNUM AS \"{PAGE_HELPER_COLUMN}\" FROM ({trimmed}) t \
                     WHERE ROWNUM <= {end}) WHERE \"{PAGE_HELPER_COLUMN}\" > {offset}"
                )
            }
        },
    }
}

fn run_spike(app: &mut App, mode: FetchMode) -> Task<Message> {
    if app.busy {
        return Task::none();
    }
    let Some(profile) = app.active_worksheet_profile().cloned() else {
        app.metrics = i18n::t("status.no_worksheet_connection");
        return Task::none();
    };

    app.busy = true;
    app.spike_run += 1;
    let run_id = app.spike_run;
    app.metrics = i18n::t_args("status.executing", &[("name", profile.name.as_str())]);
    app.last_output = i18n::t("status.running");

    let sql = app.active_content().map(|content| content.text());
    let sql_text = sql.clone().unwrap_or_default();
    // Test window: discover `:NAME` binds, auto-fill the parameters panel,
    // and wrap the block so panel values bind IN and OUT values come back.
    let sql = if profile.db_kind == DbKind::Oracle
        && app
            .worksheets
            .get(app.active_tab)
            .map(|sheet| sheet.kind == WindowKind::Test)
            .unwrap_or(false)
    {
        sql.map(|text| {
            let params = auto_fill_bind_params(app, &text);
            if params.is_empty() {
                text
            } else {
                crate::ui::parameters::bind_wrap(&text, &params, true).0
            }
        })
    } else {
        sql
    };
    // Server-side paging: first run loads the first page, fetch-next/all
    // continue from the rows already in the grid buffer.
    let offset = app
        .worksheets
        .get(app.active_tab)
        .map(|sheet| sheet.result.rows.len())
        .unwrap_or(0);
    let sql = sql.map(|text| match mode {
        FetchMode::Execute => page_select(&text, profile.db_kind, 0, Some(result_grid::PAGE)),
        FetchMode::Next => page_select(&text, profile.db_kind, offset, Some(result_grid::PAGE)),
        FetchMode::All => page_select(&text, profile.db_kind, offset, None),
    });
    tracing::info!(sql = sql.as_deref().unwrap_or(""), "paging sql prepared");
    app.fetch_mode = mode;

    match profile.db_kind {
        DbKind::Postgres => {
            let request = PostgresSpikeRequest {
                connection_url: profile.postgres_url(),
                sql,
                timeout_ms: Some(10_000),
                cancel: false,
                max_rows: None,
            };
            Task::perform(
                async move {
                    run_postgres_spike(request)
                        .await
                        .map(|result| {
                            let output = result.format_for_ui();
                            SpikePayload {
                                sql: sql_text,
                                output,
                                columns: result.columns,
                                rows: result.rows,
                            }
                        })
                        .map_err(|err| err.to_string())
                },
                move |result| Message::SpikeFinished(run_id, result),
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
                    connect_as: profile.connect_as,
                    sql,
                    timeout_ms: Some(10_000),
                    cancel: false,
                    max_rows: None,
                };
                Task::perform(
                    async move {
                        run_oracle_spike(request)
                            .await
                            .map(|result| {
                                let output = result.format_for_ui();
                                SpikePayload {
                                    sql: sql_text,
                                    output,
                                    columns: result.columns,
                                    rows: result.rows,
                                }
                            })
                            .map_err(|err| err.to_string())
                    },
                    move |result| Message::SpikeFinished(run_id, result),
                )
            }
            #[cfg(not(feature = "oracle"))]
            {
                app.busy = false;
                app.last_output = i18n::t("status.oracle_disabled");
                app.metrics = i18n::t("status.execute_failed");
                Task::none()
            }
        }
    }
}

/// Result-grid interactions: pure state changes, Post runs the DML batch.
fn handle_grid_message(app: &mut App, message: ResultMessage) -> Task<Message> {
    // Cell value modal and pure grid state first; Post needs &mut App freely.
    if let ResultMessage::OpenCellEditor(row, col) = message {
        if let Some(sheet) = app.worksheets.get(app.active_tab) {
            let value = sheet.result.cell(row, col).to_string();
            app.cell_editor = Some(result_grid::CellEditorDraft::open(row, col, &value));
        }
        return Task::none();
    }
    if let ResultMessage::CellEditorAction(action) = message {
        if let Some(draft) = &mut app.cell_editor {
            draft.perform_editor(action);
        }
        return Task::none();
    }
    if let ResultMessage::CellEditorOk = message {
        if let Some(draft) = app.cell_editor.take() {
            if let Some(sheet) = app.worksheets.get_mut(app.active_tab) {
                sheet
                    .result
                    .edits
                    .insert((draft.row, draft.col), draft.content.text());
            }
        }
        return Task::none();
    }
    if let ResultMessage::CellEditorCancel = message {
        app.cell_editor = None;
        return Task::none();
    }
    if let ResultMessage::ResizeStart(col) = message {
        if let Some(sheet) = app.worksheets.get(app.active_tab) {
            app.column_drag = Some(result_grid::ColumnDrag {
                col,
                start_w: sheet.result.col_width(col),
                anchor_x: None,
            });
        }
        return Task::none();
    }
    if let ResultMessage::ResizeMove(x) = message {
        let drag = match &mut app.column_drag {
            Some(drag) => drag,
            None => return Task::none(),
        };
        match drag.anchor_x {
            None => drag.anchor_x = Some(x),
            Some(ax) => {
                let width = (drag.start_w + (x - ax)).max(60.0);
                if let Some(sheet) = app.worksheets.get_mut(app.active_tab) {
                    sheet.result.ensure_col_widths();
                    if let Some(slot) = sheet.result.col_widths.get_mut(drag.col) {
                        *slot = width;
                    }
                }
            }
        }
        return Task::none();
    }
    if let ResultMessage::ResizeEnd = message {
        app.column_drag = None;
        return Task::none();
    }
    if let ResultMessage::ContentScrolled(viewport) = message {
        return result_grid::handle_content_scrolled(viewport).map(Message::Grid);
    }
    if let ResultMessage::Post = message {
        return run_result_post(app);
    }
    if let ResultMessage::FetchNext = message {
        return run_spike(app, FetchMode::Next);
    }
    if let ResultMessage::FetchLast = message {
        return run_spike(app, FetchMode::All);
    }
    let Some(sheet) = app.worksheets.get_mut(app.active_tab) else {
        return Task::none();
    };
    let result = &mut sheet.result;
    match message {
        ResultMessage::ToggleEdit => result.edit_mode = !result.edit_mode,
        ResultMessage::Select(index) => result.selected = Some(index),
        ResultMessage::Cell(index, col, value) => {
            result.edits.insert((index, col), value);
        }
        ResultMessage::Insert => {
            let index = result.rows.len();
            result.rows.push(vec![String::new(); result.columns.len()]);
            result.inserted.insert(index);
            result.selected = Some(index);
            result.edit_mode = true;
        }
        ResultMessage::Delete => {
            if let Some(index) = result.selected {
                result.deleted.insert(index);
            }
        }
        ResultMessage::Post | ResultMessage::FetchNext | ResultMessage::FetchLast => {
            unreachable!("handled above")
        }
        ResultMessage::OpenCellEditor(..)
        | ResultMessage::CellEditorAction(..)
        | ResultMessage::CellEditorOk
        | ResultMessage::CellEditorCancel
        | ResultMessage::ResizeStart(..)
        | ResultMessage::ResizeMove(..)
        | ResultMessage::ResizeEnd
        | ResultMessage::ContentScrolled(..) => unreachable!("handled above"),
    }
    Task::none()
}

/// Posts the result-grid changes: generates DML and runs it on the same connection.
fn run_result_post(app: &mut App) -> Task<Message> {
    if app.busy {
        return Task::none();
    }
    let Some(profile) = app.active_worksheet_profile().cloned() else {
        app.metrics = i18n::t("status.no_worksheet_connection");
        return Task::none();
    };
    let Some(sheet) = app.worksheets.get(app.active_tab) else {
        return Task::none();
    };
    let result = &sheet.result;
    if !result.has_changes() {
        app.last_output = i18n::t("result.no_changes");
        return Task::none();
    }
    let Some(table) = data_changes::extract_table(&result.source_sql) else {
        app.last_output = i18n::t("result.no_table");
        app.metrics = i18n::t("status.execute_failed");
        return Task::none();
    };
    let statements = data_changes::build_statements(
        profile.db_kind,
        &table,
        &result.columns,
        &result.rows,
        &result.edits,
        &result.inserted,
        &result.deleted,
    );
    if statements.is_empty() {
        app.last_output = i18n::t("result.no_changes");
        return Task::none();
    }

    app.busy = true;
    app.spike_run += 1;
    let run_id = app.spike_run;
    app.metrics = i18n::t("status.running");

    // Oracle batches DML in one anonymous block; Postgres takes multi-statement
    // strings via simple protocol (see postgres_spike).
    let sql = match profile.db_kind {
        DbKind::Postgres => statements.join(";\n"),
        DbKind::Oracle => format!("BEGIN\n{};\nEND;", statements.join(";\n")),
    };

    match profile.db_kind {
        DbKind::Postgres => {
            let request = PostgresSpikeRequest {
                connection_url: profile.postgres_url(),
                sql: Some(sql),
                timeout_ms: Some(10_000),
                cancel: false,
                max_rows: None,
            };
            Task::perform(
                async move {
                    run_postgres_spike(request)
                        .await
                        .map(|result| result.format_for_ui())
                        .map_err(|err| err.to_string())
                },
                move |result| Message::PostFinished(run_id, result),
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
                    connect_as: profile.connect_as,
                    sql: Some(sql),
                    timeout_ms: Some(10_000),
                    cancel: false,
                    max_rows: None,
                };
                Task::perform(
                    async move {
                        run_oracle_spike(request)
                            .await
                            .map(|result| result.format_for_ui())
                            .map_err(|err| err.to_string())
                    },
                    move |result| Message::PostFinished(run_id, result),
                )
            }
            #[cfg(not(feature = "oracle"))]
            {
                app.busy = false;
                app.last_output = i18n::t("status.oracle_disabled");
                app.metrics = i18n::t("status.execute_failed");
                Task::none()
            }
        }
    }
}

fn view(app: &App) -> Element<'_, Message> {
    // Keep style helpers in sync with the selected preference every frame
    // (System may follow OS changes while the app is open).
    ui_theme::apply(app.theme_mode);

    let work_area: Element<'_, Message> = if app.worksheets.is_empty() {
        column![worksheet_tabs(app), empty_workspace()]
            .width(Fill)
            .height(Fill)
            .into()
    } else {
        column![
            worksheet_tabs(app),
            worksheet_connection_bar(app),
            worksheet_toolbar(app),
            editor_pane(app),
        ]
        .width(Fill)
        .height(Fill)
        .into()
    };

    let shell = column![
        ribbon(app),
        rule::horizontal(1).style(|_| rule_style()),
        row![side_browser(app), vertical_resize_handle(), work_area,].height(Fill),
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
    } else if let Some(state) = &app.preferences {
        stack![base, preferences_modal(state).map(Message::Preferences)]
            .width(Fill)
            .height(Fill)
            .into()
    } else if let Some(draft) = &app.value_editor {
        stack![base, value_editor_modal(draft).map(Message::Param)]
            .width(Fill)
            .height(Fill)
            .into()
    } else if let Some(draft) = &app.cell_editor {
        stack![
            base,
            result_grid::cell_editor_modal(draft).map(Message::Grid)
        ]
        .width(Fill)
        .height(Fill)
        .into()
    } else if app.column_drag.is_some() {
        stack![base, result_grid::drag_overlay().map(Message::Grid)]
            .width(Fill)
            .height(Fill)
            .into()
    } else if app.dialog.is_some() {
        stack![base, connection_modal(app)]
            .width(Fill)
            .height(Fill)
            .into()
    } else if app.group_draft.is_some() {
        stack![base, group_name_modal(app)]
            .width(Fill)
            .height(Fill)
            .into()
    } else if app.error_modal.is_some() {
        stack![base, error_modal(app)]
            .width(Fill)
            .height(Fill)
            .into()
    } else if app.connect_error.is_some() {
        stack![base, connect_error_modal(app)]
            .width(Fill)
            .height(Fill)
            .into()
    } else if app.close_confirm.is_some() {
        stack![base, close_tab_confirm_modal(app)]
            .width(Fill)
            .height(Fill)
            .into()
    } else {
        base.into()
    }
}

fn ribbon_menu_item(label: impl Into<String>) -> iced::widget::Button<'static, Message> {
    let label = label.into();
    button(
        container(text(label).size(13))
            .width(Fill)
            .align_x(Horizontal::Left),
    )
    .width(Fill)
    .padding(Padding::from([7, 12]))
    .style(move |_, status| menu_item_button_style(status))
}

fn ribbon_command(label: impl Into<String>) -> iced::widget::Button<'static, Message> {
    let label = label.into();
    button(text(label).size(12))
        .padding(Padding::from([4, 10]))
        .style(move |_, status| ribbon_command_style(status, false))
}

/// File ribbon New/Open: own hover tracking, because iced_aw MenuBar does not
/// put the root button into `Hovered` while the menu is closed.
fn ribbon_dropdown(
    label: impl Into<String>,
    target: RibbonHoverTarget,
    hovered: bool,
    menu: Menu<'static, Message, Theme, iced::Renderer>,
) -> Element<'static, Message> {
    let label = label.into();
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

    mouse_area(container(bar).height(28).align_y(Vertical::Center))
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
    label: impl Into<String>,
    content: Element<'a, Message>,
) -> Element<'a, Message> {
    let label = label.into();
    container(
        column![
            container(content).height(28).align_y(Vertical::Center),
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
        Item::new(
            ribbon_menu_item(window_kind_menu_label(WindowKind::Blank))
                .on_press(Message::MenuNew(WindowKind::Blank)),
        ),
        Item::new(
            ribbon_menu_item(window_kind_menu_label(WindowKind::Function))
                .on_press(Message::MenuNew(WindowKind::Function)),
        ),
        Item::new(
            ribbon_menu_item(window_kind_menu_label(WindowKind::JavaSource))
                .on_press(Message::MenuNew(WindowKind::JavaSource)),
        ),
        Item::new(
            ribbon_menu_item(window_kind_menu_label(WindowKind::Package))
                .on_press(Message::MenuNew(WindowKind::Package)),
        ),
        Item::new(
            ribbon_menu_item(window_kind_menu_label(WindowKind::PackageBody))
                .on_press(Message::MenuNew(WindowKind::PackageBody)),
        ),
        Item::new(
            ribbon_menu_item(window_kind_menu_label(WindowKind::PackageSpecification))
                .on_press(Message::MenuNew(WindowKind::PackageSpecification)),
        ),
        Item::new(
            ribbon_menu_item(window_kind_menu_label(WindowKind::Procedure))
                .on_press(Message::MenuNew(WindowKind::Procedure)),
        ),
        Item::new(
            ribbon_menu_item(window_kind_menu_label(WindowKind::Trigger))
                .on_press(Message::MenuNew(WindowKind::Trigger)),
        ),
        Item::new(
            ribbon_menu_item(window_kind_menu_label(WindowKind::Type))
                .on_press(Message::MenuNew(WindowKind::Type)),
        ),
        Item::new(
            ribbon_menu_item(window_kind_menu_label(WindowKind::TypeBody))
                .on_press(Message::MenuNew(WindowKind::TypeBody)),
        ),
        Item::new(
            ribbon_menu_item(window_kind_menu_label(WindowKind::TypeSpecification))
                .on_press(Message::MenuNew(WindowKind::TypeSpecification)),
        ),
    ])
    .max_width(240.0)
    .offset(1.0)
    .close_on_item_click(true)
    .close_on_background_click(true);

    let new_menu = Menu::new(vec![
        Item::with_menu(
            ribbon_menu_item(i18n::t("menu.program_window")),
            program_menu,
        ),
        Item::new(
            ribbon_menu_item(window_kind_menu_label(WindowKind::Test))
                .on_press(Message::MenuNew(WindowKind::Test)),
        ),
        Item::new(
            ribbon_menu_item(window_kind_menu_label(WindowKind::Sql))
                .on_press(Message::MenuNew(WindowKind::Sql)),
        ),
        Item::new(
            ribbon_menu_item(window_kind_menu_label(WindowKind::Command))
                .on_press(Message::MenuNew(WindowKind::Command)),
        ),
        Item::new(
            ribbon_menu_item(window_kind_menu_label(WindowKind::ExplainPlan))
                .on_press(Message::MenuNew(WindowKind::ExplainPlan)),
        ),
    ])
    .max_width(240.0)
    .offset(1.0)
    .close_on_item_click(true)
    .close_on_background_click(true);

    let open_menu = Menu::new(vec![
        Item::new(
            ribbon_menu_item(i18n::t("menu.program_file"))
                .on_press(Message::MenuOpenStub(i18n::t("menu.program_file"))),
        ),
        Item::new(
            ribbon_menu_item(i18n::t("menu.test_script"))
                .on_press(Message::MenuOpenStub(i18n::t("menu.test_script"))),
        ),
        Item::new(
            ribbon_menu_item(i18n::t("menu.sql_script"))
                .on_press(Message::MenuOpenStub(i18n::t("menu.sql_script"))),
        ),
        Item::new(
            ribbon_menu_item(i18n::t("menu.command_file"))
                .on_press(Message::MenuOpenStub(i18n::t("menu.command_file"))),
        ),
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
                    i18n::t("ribbon.group.open"),
                    row![
                        ribbon_dropdown(
                            i18n::t("ribbon.new"),
                            RibbonHoverTarget::New,
                            new_hovered,
                            new_menu,
                        ),
                        ribbon_dropdown(
                            i18n::t("ribbon.open"),
                            RibbonHoverTarget::Open,
                            open_hovered,
                            open_menu,
                        ),
                    ]
                    .spacing(4)
                    .into(),
                ),
                ribbon_divider(),
                ribbon_group(
                    i18n::t("ribbon.group.save"),
                    row![
                        ribbon_command(i18n::t("ribbon.save")).on_press(Message::MenuSaveStub),
                        ribbon_command(i18n::t("ribbon.save_as")).on_press(Message::MenuSaveAsStub),
                    ]
                    .spacing(4)
                    .into(),
                ),
                ribbon_divider(),
                ribbon_group(
                    i18n::t("ribbon.group.application"),
                    ribbon_command(i18n::t("ribbon.exit"))
                        .on_press(Message::MenuExit)
                        .into(),
                ),
                Space::new().width(Fill),
            ]
            .spacing(2)
            .align_y(Vertical::Center)
            .into()
        }
        RibbonTab::Session => row![
            ribbon_group(
                i18n::t("ribbon.group.execute"),
                ribbon_command(i18n::t("ribbon.execute"))
                    .on_press(Message::MenuExecute)
                    .into(),
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
                ribbon_group(
                    i18n::t("ribbon.group.application"),
                    ribbon_command(i18n::t("ribbon.preferences"))
                        .on_press(Message::OpenPreferences)
                        .into(),
                ),
                ribbon_divider(),
                ribbon_group(i18n::t("ribbon.group.theme"), theme_row.into()),
                Space::new().width(Fill),
            ]
            .align_y(Vertical::Center)
            .into()
        }
        RibbonTab::Help => container(
            text(i18n::t("help.placeholder"))
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

fn connection_row(app: &App, profile: &ConnectionProfile) -> Element<'static, Message> {
    let selected = app.selected_connection == Some(profile.id);
    let id = profile.id;
    let status = app.connection_status.get(&id).copied().unwrap_or_default();
    let label = profile.summary();
    let mut label_text = text(label).size(12).font(Font::MONOSPACE);
    label_text = match status {
        ConnectionStatus::Connected => label_text.color(ui_theme::success()),
        ConnectionStatus::Failed => label_text.color(ui_theme::error()),
        ConnectionStatus::Unknown => label_text,
    };
    button(container(label_text).width(Fill).align_x(Horizontal::Left))
        .width(Fill)
        .padding(Padding::from([8, 10]))
        .style(move |_, btn_status| connection_row_style(selected, status, btn_status))
        .on_press(Message::SelectConnection(id))
        .into()
}

/// Collapsible sidebar group header: chevron + name + member count.
fn group_header(app: &App, name: &str) -> Element<'static, Message> {
    let collapsed = app.collapsed_groups.contains(name);
    let members = app.connections.iter().filter(|p| p.group == name).count();
    let icon = if collapsed {
        chevron_right().size(12)
    } else {
        chevron_down().size(12)
    };
    let name = name.to_string();
    let selected = app.selected_group.as_deref() == Some(name.as_str());
    let count = text(format!("({members})"))
        .size(11)
        .color(ui_theme::current().muted);
    button(
        row![
            icon.color(ui_theme::current().muted),
            text(name.clone()).size(11),
            count,
        ]
        .spacing(4)
        .align_y(Vertical::Center),
    )
    .width(Fill)
    .padding(Padding::from([6, 6]))
    .style(move |_, status| connection_row_style(selected, ConnectionStatus::Unknown, status))
    .on_press(Message::SelectGroup(name))
    .into()
}

fn side_browser(app: &App) -> Element<'_, Message> {
    let mut list = column![].spacing(2);
    // Groups in persisted order, then ungrouped connections at the end.
    for group in &app.groups {
        list = list.push(group_header(app, group));
        if !app.collapsed_groups.contains(group) {
            for profile in app.connections.iter().filter(|p| &p.group == group) {
                list = list.push(connection_row(app, profile));
            }
        }
    }
    for profile in app.connections.iter().filter(|p| p.group.is_empty()) {
        list = list.push(connection_row(app, profile));
    }

    let connections = column![
        section_title(i18n::t("connections.title")),
        container(
            row![
                dock_icon_button(
                    folder_plus().size(14),
                    i18n::t("connections.add_group"),
                    Message::AddGroup
                ),
                dock_icon_button(
                    plus().size(14),
                    i18n::t("connections.add"),
                    Message::AddConnection
                ),
                dock_icon_button(
                    pencil().size(14),
                    i18n::t("connections.edit"),
                    if app.selected_group.is_some() {
                        Message::EditGroup
                    } else {
                        Message::EditConnection
                    }
                ),
                dock_icon_button(
                    trash().size(14),
                    i18n::t("connections.delete"),
                    if app.selected_group.is_some() {
                        Message::DeleteGroup
                    } else {
                        Message::DeleteConnection
                    }
                ),
                dock_icon_button(
                    plug().size(14),
                    i18n::t("connections.connect"),
                    Message::ConnectSelected
                ),
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
            text(i18n::t("objects.connect_to_load"))
                .size(12)
                .color(ui_theme::current().muted),
        )
        .padding(Padding::from([8, 10]))
        .width(Fill)
        .into()
    } else if app.objects_loading {
        container(
            text(i18n::t("objects.loading"))
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
            tree = tree.push(text(error.clone()).size(11).color(ui_theme::error()));
        }

        if app.object_owners.is_empty() && app.objects_error.is_none() {
            tree = tree.push(
                text(i18n::t("objects.empty"))
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
        section_title(i18n::t("objects.title")),
        container(
            checkbox(app.current_user_only)
                .label(i18n::t("objects.only_current_user"))
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
            column![
                text(i18n::t("objects.filter_placeholder"))
                    .size(11)
                    .color(ui_theme::current().muted),
                text_input("", &app.filter)
                    .on_input(Message::FilterChanged)
                    .padding(6)
                    .size(13),
            ]
            .spacing(2),
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
        i18n::t("dialog.connection.edit")
    } else {
        i18n::t("dialog.connection.add")
    };
    let db_kinds = [DbKind::Oracle, DbKind::Postgres];
    let database_field_label = if draft.db_kind == DbKind::Oracle {
        i18n::t("dialog.connection.service")
    } else {
        i18n::t("dialog.connection.database")
    };

    // Groups combo: existing groups + a "no group" entry; the picked name
    // lands in the draft as plain String (free-form groups stay possible
    // only via the "new group" dialog).
    let mut group_options = vec![GroupOption(String::new())];
    group_options.extend(app.groups.iter().map(|g| GroupOption(g.clone())));
    let group_field: Element<'_, Message> = column![
        text(i18n::t("dialog.connection.group"))
            .size(12)
            .color(ui_theme::current().muted),
        pick_list(
            group_options,
            Some(GroupOption(draft.group.clone())),
            |option| Message::DialogGroupChanged(option.0),
        )
        .padding(8)
        .width(Fill),
    ]
    .spacing(2)
    .into();

    let connect_as_field: Element<'_, Message> = if draft.db_kind == DbKind::Oracle {
        column![
            text(i18n::t("dialog.connection.connect_as"))
                .size(12)
                .color(ui_theme::current().muted),
            pick_list(
                [ConnectAs::Normal, ConnectAs::Sysdba, ConnectAs::Sysoper],
                Some(draft.connect_as),
                Message::DialogConnectAsChanged,
            )
            .padding(8)
            .width(Fill),
        ]
        .spacing(2)
        .into()
    } else {
        Space::new().height(0).into()
    };

    let form = column![
        text(title).size(18),
        labeled_input(
            i18n::t("dialog.connection.name"),
            &draft.name,
            Message::DialogNameChanged
        ),
        group_field,
        text(i18n::t("dialog.connection.db_type"))
            .size(12)
            .color(ui_theme::current().muted),
        pick_list(db_kinds, Some(draft.db_kind), Message::DialogDbKindChanged)
            .padding(8)
            .width(Fill),
        labeled_input(
            i18n::t("dialog.connection.host"),
            &draft.host,
            Message::DialogHostChanged
        ),
        labeled_input(
            i18n::t("dialog.connection.port"),
            &draft.port,
            Message::DialogPortChanged
        ),
        labeled_input(
            database_field_label,
            &draft.database,
            Message::DialogDatabaseChanged,
        ),
        labeled_input(
            i18n::t("dialog.connection.username"),
            &draft.username,
            Message::DialogUsernameChanged,
        ),
        labeled_input_secure(
            i18n::t("dialog.connection.password"),
            &draft.password,
            Message::DialogPasswordChanged,
            true,
        ),
        connect_as_field,
        if let Some(error) = &app.dialog_error {
            text(error.clone()).size(12).color(ui_theme::error())
        } else {
            text("").size(12)
        },
        row![
            button(text(i18n::t("dialog.connection.cancel")).size(13))
                .padding(Padding::from([8, 16]))
                .on_press(Message::DialogCancel),
            Space::new().width(Fill),
            button(text(i18n::t("dialog.connection.save")).size(13))
                .padding(Padding::from([8, 16]))
                .style(|_, status| ui_theme::primary_button(status))
                .on_press(Message::DialogSave),
        ]
        .spacing(8),
    ]
    .spacing(8)
    .padding(20)
    .width(420);

    let card = container(form).style(|_| ui_theme::modal_card());

    // Dim background only — do not close on outside click (Cancel / Save only).
    let backdrop = container(Space::new().width(Fill).height(Fill))
        .width(Fill)
        .height(Fill)
        .style(|_| ui_theme::modal_backdrop());

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

/// Modal shown when Connect fails; displays the driver / network error text.
/// "New / rename group" dialog state; `original` is set while renaming.
#[derive(Debug, Clone)]
struct GroupDraft {
    original: Option<String>,
    name: String,
}

/// Modal asking for a new (or renamed) sidebar group name.
fn group_name_modal(app: &App) -> Element<'_, Message> {
    let draft = app.group_draft.clone().unwrap_or(GroupDraft {
        original: None,
        name: String::new(),
    });
    let title_key = if draft.original.is_some() {
        "dialog.group.rename"
    } else {
        "dialog.group.add"
    };
    let form = column![
        text(i18n::t(title_key)).size(18),
        labeled_input(
            i18n::t("dialog.group.name"),
            &draft.name,
            Message::GroupDraftChanged
        ),
        row![
            button(text(i18n::t("dialog.group.cancel")).size(13))
                .padding(Padding::from([8, 16]))
                .on_press(Message::GroupDialogCancel),
            Space::new().width(Fill),
            button(text(i18n::t("dialog.group.save")).size(13))
                .padding(Padding::from([8, 16]))
                .style(|_, status| ui_theme::primary_button(status))
                .on_press(Message::GroupDialogSave),
        ]
        .spacing(8),
    ]
    .spacing(8)
    .padding(20)
    .width(320);

    let card = container(form).style(|_| ui_theme::modal_card());

    opaque(
        stack![
            container(Space::new().width(Fill).height(Fill))
                .width(Fill)
                .height(Fill)
                .style(|_| ui_theme::modal_backdrop()),
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

fn connect_error_modal(app: &App) -> Element<'_, Message> {
    let error = app
        .connect_error
        .clone()
        .unwrap_or_else(|| i18n::t("dialog.connect_failed.unknown"));

    let form = column![
        text(i18n::t("dialog.connect_failed.title"))
            .size(18)
            .color(ui_theme::error()),
        text(error).size(13).color(ui_theme::current().text),
        row![
            Space::new().width(Fill),
            button(text(i18n::t("dialog.connect_failed.ok")).size(13))
                .padding(Padding::from([8, 16]))
                .style(|_, status| ui_theme::primary_button(status))
                .on_press(Message::DismissConnectError),
        ],
    ]
    .spacing(12)
    .padding(20)
    .width(460);

    let card = container(form).style(|_| ui_theme::modal_card());

    let backdrop = mouse_area(
        container(Space::new().width(Fill).height(Fill))
            .width(Fill)
            .height(Fill)
            .style(|_| ui_theme::modal_backdrop()),
    )
    .on_press(Message::DismissConnectError);

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

/// SQL execution error dialog. Closing it moves the caret to the parsed
/// error position (line/column from ORA-06550 / ORA-06512 / PG "LINE N:").
fn error_modal(app: &App) -> Element<'_, Message> {
    let error = app.error_modal.clone().unwrap_or_default();

    let form = column![
        text(i18n::t("dialog.sql_error.title"))
            .size(18)
            .color(ui_theme::error()),
        scrollable(text(error).size(13).font(Font::MONOSPACE))
            .height(260)
            .width(Fill),
        row![
            Space::new().width(Fill),
            button(text(i18n::t("dialog.connect_failed.ok")).size(13))
                .padding(Padding::from([8, 16]))
                .style(|_, status| ui_theme::primary_button(status))
                .on_press(Message::DismissErrorModal),
        ],
    ]
    .spacing(12)
    .padding(20)
    .width(560);

    let card = container(form).style(|_| ui_theme::modal_card());

    let backdrop = mouse_area(
        container(Space::new().width(Fill).height(Fill))
            .width(Fill)
            .height(Fill)
            .style(|_| ui_theme::modal_backdrop()),
    )
    .on_press(Message::DismissErrorModal);

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

/// Parses `line N[, column M]` from Oracle/Postgres error text.
///
/// # Examples
///
/// ```
/// # use plsqlide::ui::app::parse_error_position;
/// assert_eq!(parse_error_position("ORA-06550: line 5, column 3:"), Some((5, 3)));
/// assert_eq!(parse_error_position("4/1    PL/SQL: Statement ignored"), Some((4, 1)));
/// assert_eq!(parse_error_position("no position here"), None);
/// ```
pub fn parse_error_position(text: &str) -> Option<(usize, usize)> {
    for line in text.lines() {
        if line.contains("ORA-06550") {
            if let Some(l) = number_after(line, "line ") {
                return Some((l, number_after(line, "column ").unwrap_or(1)));
            }
        }
    }
    for line in text.lines() {
        if line.trim_start().to_ascii_uppercase().starts_with("LINE ") {
            if let Some(l) = number_after(&line.to_ascii_uppercase(), "LINE ") {
                return Some((l, 1));
            }
        }
    }
    for line in text.lines() {
        if line.contains("ORA-06512") {
            if let Some(l) = number_after(line, "line ") {
                return Some((l, 1));
            }
        }
    }
    // PL/SQL compilation table rows: "LINE/COL ERROR" → "4/1  PL/SQL: ..."
    for line in text.lines() {
        for token in line.split_whitespace() {
            if let Some((l, c)) = token.split_once('/') {
                let digits = |s: &str| !s.is_empty() && s.chars().all(|ch| ch.is_ascii_digit());
                if digits(l) && digits(c) {
                    return Some((l.parse().ok()?, c.parse().ok()?));
                }
            }
        }
    }
    None
}

/// Local syntax sanity check: finds the first unbalanced quote, comment or
/// parenthesis and returns its 1-based (line, column). Used when the DB
/// error text carries no position of its own.
///
/// # Examples
///
/// ```
/// # use plsqlide::ui::app::self_check_position;
/// assert_eq!(self_check_position("select * from t\nwhere x = 'o"), Some((2, 11)));
/// assert_eq!(self_check_position("select (1 from t"), Some((1, 8)));
/// assert_eq!(self_check_position("select * from t -- ok"), None);
/// assert_eq!(self_check_position("SELECT * D_AGENTS"), Some((1, 10)));
/// assert_eq!(self_check_position("select a,, b from t"), Some((1, 10)));
/// assert_eq!(self_check_position("select a, from t"), Some((1, 9)));
/// assert_eq!(self_check_position("select * from t where 1=1 where 2=2"), Some((1, 27)));
/// assert_eq!(self_check_position("select price * total from t"), None);
/// ```
pub fn self_check_position(sql: &str) -> Option<(usize, usize)> {
    let mut line = 1usize;
    let mut col = 1usize;
    let mut in_string = false;
    let mut string_start = (1usize, 1usize);
    let mut in_block_comment = false;
    let mut comment_start = (1usize, 1usize);
    let mut in_line_comment = false;
    let mut open_parens: Vec<(usize, usize)> = Vec::new();
    let mut tokens: Vec<(String, (usize, usize))> = Vec::new();
    let mut token_text = String::new();
    let mut token_start = (1usize, 1usize);
    let mut chars = sql.chars().peekable();

    while let Some(ch) = chars.next() {
        if in_line_comment {
            if ch == '\n' {
                in_line_comment = false;
                line += 1;
                col = 1;
                continue;
            }
        } else if in_block_comment {
            if ch == '*' && chars.peek() == Some(&'/') {
                chars.next();
                in_block_comment = false;
                col += 2;
                continue;
            }
        } else if in_string {
            if ch == '\'' {
                if chars.peek() == Some(&'\'') {
                    chars.next(); // escaped ''
                    col += 2;
                    continue;
                }
                in_string = false;
            }
        } else {
            match ch {
                '-' if chars.peek() == Some(&'-') => {
                    chars.next();
                    in_line_comment = true;
                }
                '/' if chars.peek() == Some(&'*') => {
                    chars.next();
                    in_block_comment = true;
                    comment_start = (line, col);
                }
                '\'' => {
                    in_string = true;
                    string_start = (line, col);
                }
                '(' => open_parens.push((line, col)),
                ')' => {
                    if open_parens.pop().is_none() {
                        return Some((line, col)); // extra ')'
                    }
                }
                _ => {}
            }
            // Tokenize words and statement-shaping symbols for the SELECT lint.
            if ch.is_alphanumeric() || ch == '_' || ch == '$' || ch == '#' {
                if token_text.is_empty() {
                    token_start = (line, col);
                }
                token_text.push(ch);
            } else {
                if !token_text.is_empty() {
                    tokens.push((token_text.to_ascii_uppercase(), token_start));
                    token_text.clear();
                }
                if matches!(ch, '(' | ')' | '*' | ',' | ';') {
                    tokens.push((ch.to_string(), (line, col)));
                }
            }
        }
        match ch {
            '\n' => {
                line += 1;
                col = 1;
            }
            _ => col += 1,
        }
    }

    if in_string {
        return Some(string_start); // unterminated string literal
    }
    if in_block_comment {
        return Some(comment_start); // unterminated /* comment
    }
    if !token_text.is_empty() {
        tokens.push((token_text.to_ascii_uppercase(), token_start));
    }
    if let Some((line, col)) = open_parens.last().copied() {
        return Some((line, col)); // unclosed '('
    }
    // Paren depth of every token (for clause-level lints).
    let mut depths = Vec::with_capacity(tokens.len());
    let mut depth = 0usize;
    for (word, _) in &tokens {
        match word.as_str() {
            "(" => {
                depths.push(depth);
                depth += 1;
            }
            ")" => {
                depth = depth.saturating_sub(1);
                depths.push(depth);
            }
            _ => depths.push(depth),
        }
    }

    // Double comma: "a,, b" — flag the extra comma.
    for window in tokens.windows(2) {
        if window[0].0 == "," && window[1].0 == "," {
            return Some(window[1].1);
        }
    }
    // Trailing comma: "a, FROM" / "a,)" / "a," at end — flag the comma.
    for (i, (word, pos)) in tokens.iter().enumerate() {
        if word != "," {
            continue;
        }
        let next = tokens.get(i + 1);
        let ends_list = match next {
            None => true,
            Some((n, _)) => matches!(n.as_str(), ")" | ";" | "FROM" | "WHERE"),
        };
        if ends_list {
            return Some(*pos);
        }
    }
    // "SELECT * <ident>" / "SELECT *" with no FROM: a bare select-all star
    // (preceded by SELECT/,/() followed by anything but FROM. A
    // multiplication star never enters this branch.
    if tokens.first().map(|(word, _)| word.as_str()) == Some("SELECT") {
        for (i, window) in tokens.windows(3).enumerate() {
            if window[1].0 == "*" && matches!(window[0].0.as_str(), "SELECT" | "," | "(") {
                let next = &window[2];
                let next_word = next.0.chars().next().is_some_and(|ch| ch.is_alphabetic());
                if next_word && next.0 != "FROM" {
                    return Some(next.1);
                }
                if !next_word
                    && matches!(
                        next.0.as_str(),
                        ";" | "WHERE" | "GROUP" | "ORDER" | "HAVING"
                    )
                {
                    return Some(window[1].1);
                }
            }
            let _ = i;
        }
        // Bare "SELECT *" at end of text.
        let last = tokens.last().map(|(w, _)| w.as_str());
        if matches!(last, Some("*") | Some(";") | None) {
            if let Some((_, pos)) = tokens.iter().rev().find(|(w, _)| w == "*") {
                return Some(*pos);
            }
        }
    }
    // Repeated WHERE at the same depth (subquery WHEREs live deeper);
    // set-operators (UNION/MINUS/INTERSECT) start a new clause.
    let mut last_where: Option<(usize, (usize, usize))> = None; // (depth, position)
    for ((word, position), depth) in tokens.iter().zip(depths.iter()) {
        match word.as_str() {
            "UNION" | "MINUS" | "INTERSECT" => last_where = None,
            "WHERE" => {
                if last_where.is_some_and(|(d, _)| d == *depth) {
                    return Some(*position);
                }
                last_where = Some((*depth, *position));
            }
            _ => {}
        }
    }
    None
}

fn number_after(s: &str, marker: &str) -> Option<usize> {
    let rest = s.split_once(marker)?.1;
    let digits: String = rest
        .chars()
        .skip_while(|c| c.is_whitespace())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

fn labeled_input(
    label: impl Into<String>,
    value: &str,
    on_input: impl Fn(String) -> Message + 'static,
) -> Element<'static, Message> {
    labeled_input_secure(label, value, on_input, false)
}

fn labeled_input_secure(
    label: impl Into<String>,
    value: &str,
    on_input: impl Fn(String) -> Message + 'static,
    secure: bool,
) -> Element<'static, Message> {
    let label = label.into();
    let mut input = text_input("", value).on_input(on_input).padding(8);
    if secure {
        input = input.secure(true);
    }
    column![text(label).size(12).color(ui_theme::current().muted), input,]
        .spacing(4)
        .into()
}

fn section_title(title: impl Into<String>) -> Element<'static, Message> {
    let title = title.into();
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
        format!("  {marker}  {}", object_kind_display(kind)),
        folder_node,
        Some(key.clone()),
    )];

    if expanded {
        if loading && names.is_none() {
            children = children.push(
                container(
                    text(format!("    {}", i18n::t("objects.loading_folder")))
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
        let title = sheet.tab_label();
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

/// Full work-area placeholder when no worksheets are open.
fn empty_workspace() -> Element<'static, Message> {
    let icon = database().size(128.0).color(ui_theme::current().accent);

    container(icon)
        .width(Fill)
        .height(Fill)
        .center_x(Fill)
        .center_y(Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(ui_theme::current().panel_bg)),
            text_color: Some(ui_theme::current().accent),
            ..container::Style::default()
        })
        .into()
}

/// Ask Save / Don't save / Cancel before closing a dirty or never-saved tab.
fn close_tab_confirm_modal(app: &App) -> Element<'_, Message> {
    let index = app.close_confirm.expect("confirm open");
    let label = app
        .worksheets
        .get(index)
        .map(|sheet| sheet.tab_label().replace(" *", ""))
        .unwrap_or_else(|| i18n::t("window.sql"));

    let form = column![
        text(i18n::t("dialog.close_tab.title")).size(18),
        text(i18n::t_args(
            "dialog.close_tab.message",
            &[("name", label.as_str())],
        ))
        .size(13)
        .color(ui_theme::current().text),
        row![
            button(text(i18n::t("dialog.close_tab.save")).size(13))
                .padding(Padding::from([8, 16]))
                .style(|_, status| ui_theme::primary_button(status))
                .on_press(Message::CloseTabSave),
            button(text(i18n::t("dialog.close_tab.discard")).size(13))
                .padding(Padding::from([8, 16]))
                .on_press(Message::CloseTabDiscard),
            Space::new().width(Fill),
            button(text(i18n::t("dialog.close_tab.cancel")).size(13))
                .padding(Padding::from([8, 16]))
                .on_press(Message::CloseTabCancel),
        ]
        .spacing(8),
    ]
    .spacing(12)
    .padding(20)
    .width(440);

    let card = container(form).style(|_| ui_theme::modal_card());

    let backdrop = mouse_area(
        container(Space::new().width(Fill).height(Fill))
            .width(Fill)
            .height(Fill)
            .style(|_| ui_theme::modal_backdrop()),
    )
    .on_press(Message::CloseTabCancel);

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

/// Shows which connection the active worksheet was created for.
/// Toolbar strip under the connection bar, tailored to the window kind:
/// Test → debug block + Execute/Stop; SQL/Command → Execute/Stop;
/// code windows (procedure, package, …) → debug block as before.
fn worksheet_toolbar(app: &App) -> Element<'static, Message> {
    let kind = app.worksheets.get(app.active_tab).map(|sheet| sheet.kind);
    let debug_here =
        app.debug_session.active && app.debug_session.tab == Some(app.active_tab);
    let debug = debug_toolbar(app.debug_system, debug_here, app.busy).map(Message::Debug);
    let execute = toolbar_icon_button(
        play().size(14),
        i18n::t("ribbon.execute"),
        !app.busy && app.active_worksheet_profile().is_some(),
        Message::MenuExecute,
    );
    let stop = toolbar_icon_button(
        square().size(14),
        i18n::t("toolbar.stop"),
        app.busy
            || (app.debug_session.active
                && app.debug_session.tab == Some(app.active_tab)),
        Message::MenuStop,
    );
    let bar = match kind {
        Some(WindowKind::Sql | WindowKind::Command) => row![execute, stop],
        // Test and code windows: debug controls only — no script buttons.
        _ => row![debug],
    };
    container(bar.spacing(6).align_y(Vertical::Center))
        .padding(Padding::from([4, 10]))
        .style(|_| container::Style {
            background: Some(Background::Color(ui_theme::current().ribbon_body_bg)),
            text_color: Some(ui_theme::current().text),
            ..container::Style::default()
        })
        .into()
}

/// Popup origin so it sits just below the caret line (flips above it when
/// the caret is near the bottom of the editor). Approximates wrapped lines
/// as one visual row — `ponytail:` switch to caret geometry if iced exposes it.
fn completion_popup_origin(
    caret_line: f32,
    caret_col: f32,
    scroll_y: f32,
    items: usize,
) -> (f32, f32) {
    let est_h = (items.min(10) as f32) * 26.0 + 8.0;
    let line_top = (caret_line * EDITOR_LINE_HEIGHT - scroll_y).max(0.0);
    let top = if line_top + 24.0 + est_h > 500.0 {
        (line_top - est_h - 2.0).max(0.0)
    } else {
        line_top + 24.0
    };
    let left = (24.0 + caret_col * 7.8).min(700.0);
    (top, left)
}

#[cfg(test)]
mod completion_popup_origin_tests {
    use super::completion_popup_origin;

    #[test]
    fn below_caret_when_room() {
        let (top, _) = completion_popup_origin(2.0, 10.0, 0.0, 5);
        assert!((top - (2.0 * 20.0 + 24.0)).abs() < 0.01);
    }

    #[test]
    fn flips_above_near_bottom() {
        let (top, _) = completion_popup_origin(24.0, 0.0, 0.0, 10);
        assert!(top < 24.0 * 20.0);
        assert!(top >= 0.0);
    }

    #[test]
    fn follows_scroll() {
        let (top_a, _) = completion_popup_origin(5.0, 0.0, 0.0, 5);
        let (top_b, _) = completion_popup_origin(5.0, 0.0, 100.0, 5);
        assert!((top_a - top_b - 100.0).abs() < 0.01);
    }
}

fn toolbar_divider() -> Element<'static, Message> {
    container(rule::vertical(1).style(|_| rule_style()))
        .height(22)
        .into()
}

/// Small icon toolbar button with tooltip; disabled when `enabled` is false.
fn toolbar_icon_button(
    icon: iced::widget::Text<'static>,
    tip: String,
    enabled: bool,
    message: Message,
) -> Element<'static, Message> {
    use iced::widget::tooltip;
    let mut btn = button(icon)
        .padding(Padding::from([5, 7]))
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

fn worksheet_connection_bar(app: &App) -> Element<'_, Message> {
    let label = app
        .worksheets
        .get(app.active_tab)
        .map(|sheet| app.worksheet_connection_label(sheet))
        .unwrap_or_else(|| i18n::t("worksheet.no_connection"));

    container(
        row![
            text(i18n::t("worksheet.database"))
                .size(11)
                .color(ui_theme::current().muted),
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

/// Builds the virtualized breakpoint + line-number gutter for the SQL editor.
///
/// Only a window of numbers around the current scroll position is created, so
/// large documents stay responsive. Click the left margin to toggle a breakpoint.
fn gutter_width(line_count: usize) -> f32 {
    let digits = ((line_count.max(1) as f32).log10().floor() as usize) + 1;
    (digits as f32 * 9.0 + 36.0).clamp(52.0, 80.0)
}

fn line_number_gutter(
    line_count: usize,
    scroll_y: f32,
    breakpoints: &HashSet<usize>,
    current_line: Option<usize>,
) -> Element<'static, Message> {
    let line_count = line_count.max(1);
    let max_first = line_count.saturating_sub(1);
    let first = ((scroll_y / EDITOR_LINE_HEIGHT).floor() as usize).min(max_first);
    let last = (first + GUTTER_VISIBLE_LINES).min(line_count);
    let pixel_offset = scroll_y - first as f32 * EDITOR_LINE_HEIGHT;

    let gutter_width = gutter_width(line_count);

    let mut numbers = column![].spacing(0);
    for number in (first + 1)..=last {
        let has_bp = breakpoints.contains(&number);
        let is_current = current_line == Some(number);
        let marker = if is_current {
            "▶"
        } else if has_bp {
            "●"
        } else {
            " "
        };
        let marker_color = if is_current {
            ui_theme::warning()
        } else if has_bp {
            ui_theme::error()
        } else {
            ui_theme::current().muted
        };
        numbers = numbers.push(
            button(
                row![
                    text(marker)
                        .size(EDITOR_TEXT_SIZE)
                        .color(marker_color)
                        .width(16),
                    text(format!("{number}"))
                        .size(EDITOR_TEXT_SIZE)
                        .font(Font::MONOSPACE)
                        .color(ui_theme::current().muted),
                ]
                .spacing(2)
                .align_y(Vertical::Center),
            )
            .padding(Padding {
                top: 0.0,
                right: 6.0,
                bottom: 0.0,
                left: 4.0,
            })
            .width(Fill)
            .height(EDITOR_LINE_HEIGHT)
            .style(move |_, status| gutter_line_style(is_current, status))
            .on_press(Message::ToggleBreakpoint(number)),
        );
    }

    container(container(numbers).padding(Padding {
        top: (EDITOR_PADDING - pixel_offset).max(0.0),
        right: 0.0,
        bottom: 0.0,
        left: 0.0,
    }))
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

fn gutter_line_style(
    is_current: bool,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    let background = if is_current {
        Some(Background::Color(ui_theme::warning()))
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
        text_color: if is_current {
            Color::BLACK
        } else {
            ui_theme::current().muted
        },
        border: Border::default(),
        ..iced::widget::button::Style::default()
    }
}

fn editor_pane(app: &App) -> Element<'_, Message> {
    let Some(sheet) = app.worksheets.get(app.active_tab) else {
        return empty_workspace();
    };
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

    let breakpoints = &sheet.breakpoints;
    let debug_here =
        app.debug_session.active && app.debug_session.tab == Some(app.active_tab);
    let current_line = if debug_here {
        app.debug_session.current_line
    } else {
        None
    };
    let editor_body = row![
        line_number_gutter(line_count, scroll_y, breakpoints, current_line),
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

        let caret = sheet.content.cursor().position;
        let (top, left) = completion_popup_origin(
            caret.line as f32,
            caret.column as f32,
            sheet.scroll_y,
            app.completion.len(),
        );
        container(popup)
            .padding(Padding {
                top,
                right: 0.0,
                bottom: 0.0,
                left,
            })
            .into()
    };

    // Current debug line: full-width band under the editor text so the paused
    // line is unmistakable. Approximates wrapped lines as one visual row —
    // `ponytail:` same ceiling as the gutter; auto-scroll when iced allows it.
    let debug_here =
        app.debug_session.active && app.debug_session.tab == Some(app.active_tab);
    let current_line_layer: Element<'_, Message> =
        match (debug_here, app.debug_session.current_line) {
        (true, Some(line)) => {
            let top =
                (EDITOR_PADDING + (line as f32 - 1.0) * EDITOR_LINE_HEIGHT - sheet.scroll_y)
                    .max(0.0);
            container(Space::new().width(Fill).height(EDITOR_LINE_HEIGHT))
                .padding(Padding {
                    top,
                    right: 0.0,
                    bottom: 0.0,
                    left: gutter_width(line_count),
                })
                .width(Fill)
                .height(Fill)
                .style(|_| container::Style {
                    background: Some(Background::Color(Color {
                        a: 0.35,
                        ..ui_theme::warning()
                    })),
                    ..container::Style::default()
                })
                .into()
        }
        _ => Space::new().width(0).height(0).into(),
    };

    let editor_stack = stack![current_line_layer, editor_layer, overlay]
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
        let show_result_tab = sheet.kind == WindowKind::Sql;
        let tab = if show_result_tab {
            app.bottom_tab
        } else {
            BottomTab::Parameters
        };
        let panel: Element<'_, Message> = match tab {
            BottomTab::Result => container(result_grid::view(&sheet.result).map(Message::Grid))
                .width(Fill)
                .height(app.parameters_height)
                .into(),
            BottomTab::Parameters => {
                parameters_view(&sheet.parameters, db_kind, app.parameters_height)
                    .map(Message::Param)
                    .into()
            }
        };
        column![
            editor_stack,
            horizontal_resize_handle(),
            panel_tab_strip(show_result_tab, tab),
            panel,
        ]
        .spacing(0)
        .width(Fill)
        .height(Fill)
        .into()
    } else {
        editor_stack.into()
    }
}

/// Tab strip of the bottom panel (Parameters / Results).
fn panel_tab_strip(show_result: bool, active: BottomTab) -> Element<'static, Message> {
    let mut strip = row![]
        .spacing(4)
        .padding(Padding::from([2, 8]))
        .align_y(Vertical::Center);
    strip = strip.push(panel_tab(
        i18n::t("panel.parameters"),
        BottomTab::Parameters,
        active,
    ));
    if show_result {
        strip = strip.push(panel_tab(
            i18n::t("panel.result"),
            BottomTab::Result,
            active,
        ));
    }
    strip.into()
}

fn panel_tab(label: String, tab: BottomTab, active: BottomTab) -> Element<'static, Message> {
    let selected = tab == active;
    button(text(label).size(11))
        .padding(Padding::from([2, 10]))
        .style(move |_, status| {
            let mut style = ui_theme::chrome_button(status);
            if selected {
                style.background = Some(Background::Color(ui_theme::current().accent));
                style.text_color = Color::WHITE;
            }
            style
        })
        .on_press(Message::BottomTab(tab))
        .into()
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
        .unwrap_or_else(|| i18n::t("worksheet.no_connection"));
    let detail = if app.last_output.is_empty() {
        app.metrics.clone()
    } else {
        app.last_output.lines().next().unwrap_or("").to_string()
    };

    container(
        row![
            text(i18n::t_args(
                "worksheet.db_label",
                &[("connection", connection.as_str())],
            ))
            .size(12),
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
    tip: impl Into<String>,
    message: Message,
) -> Element<'a, Message> {
    let tip = tip.into();
    tooltip(
        button(icon)
            .padding(Padding::from([4, 8]))
            .style(|_, status| dock_tool_style(status))
            .on_press(message),
        text(tip).size(11),
        tooltip::Position::Bottom,
    )
    .style(|_| ui_theme::tooltip_style())
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
        ConnectionStatus::Connected => ui_theme::success(),
        ConnectionStatus::Failed => ui_theme::error(),
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

fn file_extension(kind: WindowKind) -> &'static str {
    match kind {
        WindowKind::JavaSource => "java",
        _ => "sql",
    }
}

fn default_file_name(kind: WindowKind) -> String {
    format!("untitled.{}", file_extension(kind))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_select_wraps_plain_selects_only() {
        assert_eq!(
            page_select("select * from d_agents", DbKind::Postgres, 0, Some(10)),
            "SELECT * FROM (select * from d_agents) OFFSET 0 ROWS FETCH FIRST 10 ROWS ONLY"
        );
        assert_eq!(
            page_select("SELECT *\nFROM hr.d_agents\n", DbKind::Oracle, 20, Some(10)),
            "SELECT * FROM (SELECT t.*, ROWNUM AS \"_PLSQLIDE_RN\" FROM (SELECT *\nFROM hr.d_agents) t WHERE ROWNUM <= 30) WHERE \"_PLSQLIDE_RN\" > 20"
        );
        assert_eq!(
            page_select("select * from t", DbKind::Oracle, 10, None),
            "SELECT * FROM (SELECT t.*, ROWNUM AS \"_PLSQLIDE_RN\" FROM (select * from t) t WHERE ROWNUM <= 1000000000) WHERE \"_PLSQLIDE_RN\" > 10"
        );
        assert_eq!(
            page_select("select * from t", DbKind::Oracle, 0, None),
            "select * from t"
        );
        // DML, multi-statement and FOR UPDATE pass through untouched.
        assert_eq!(
            page_select("update t set a = 1", DbKind::Oracle, 0, Some(10)),
            "update t set a = 1"
        );
        assert_eq!(
            page_select("select 1; select 2", DbKind::Postgres, 0, Some(10)),
            "select 1; select 2"
        );
        assert_eq!(
            page_select("select * from t for update", DbKind::Oracle, 0, Some(10)),
            "select * from t for update"
        );
        // Leading comments do not hide the SELECT (they are stripped).
        assert_eq!(
            page_select(
                "-- report query\nSELECT * FROM d_agents",
                DbKind::Oracle,
                0,
                Some(10)
            ),
            "SELECT * FROM (SELECT * FROM d_agents) WHERE ROWNUM <= 10"
        );
        assert_eq!(
            page_select(
                "/* start */ select 1 from dual",
                DbKind::Postgres,
                0,
                Some(10)
            ),
            "SELECT * FROM (select 1 from dual) OFFSET 0 ROWS FETCH FIRST 10 ROWS ONLY"
        );
    }
}
