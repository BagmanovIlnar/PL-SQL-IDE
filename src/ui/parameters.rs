//! Bind / test parameters grid for SQL Window and Test Window.
//!
//! Provides the bottom parameters panel (Variable / Mode / Type / Value),
//! row selection, add/delete actions, and the Value ellipsis modal with
//! text / rtf / html / hex tabs.

use iced::alignment::{Horizontal, Vertical};
use iced::border::Border;
use iced::widget::{
    button, column, container, mouse_area, opaque, pick_list, row, scrollable, stack, text,
    text_editor, text_input, tooltip, Space,
};
use iced::widget::text_editor::{Action, Content};
use iced::{Background, Color, Element, Fill, Font, Length, Padding};
use iced_fonts::lucide::{plus, trash};

use crate::core::DbKind;
use crate::ui::theme as ui_theme;

/// Default height of the parameters strip under the editor.
pub const PANEL_HEIGHT: f32 = 168.0;

/// Minimum height of the parameters strip when resized by the user.
pub const PANEL_HEIGHT_MIN: f32 = 80.0;

/// Maximum height of the parameters strip when resized by the user.
pub const PANEL_HEIGHT_MAX: f32 = 480.0;

/// Input / output mode of a bind or test parameter.
///
/// # Examples
///
/// ```
/// use plsqlide::ui::parameters::ParamMode;
///
/// assert_eq!(ParamMode::In.label(), "IN");
/// assert_eq!(ParamMode::InOut.label(), "IN OUT");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ParamMode {
    #[default]
    In,
    Out,
    InOut,
}

impl ParamMode {
    /// Short label shown in the Mode column and pick list.
    pub fn label(self) -> &'static str {
        match self {
            Self::In => "IN",
            Self::Out => "OUT",
            Self::InOut => "IN OUT",
        }
    }

    /// All selectable modes in display order.
    pub fn all() -> [Self; 3] {
        [Self::In, Self::Out, Self::InOut]
    }
}

impl std::fmt::Display for ParamMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// Oracle SQL / PL/SQL types shown in the Type column for Oracle worksheets.
///
/// # Examples
///
/// ```
/// use plsqlide::ui::parameters::OracleType;
///
/// assert_eq!(OracleType::Varchar2.label(), "VARCHAR2");
/// assert!(OracleType::ALL.contains(&OracleType::Number));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OracleType {
    #[default]
    Varchar2,
    Nvarchar2,
    Char,
    Number,
    Integer,
    PlsInteger,
    BinaryFloat,
    BinaryDouble,
    Date,
    Timestamp,
    TimestampTz,
    TimestampLtz,
    Boolean,
    Clob,
    Nclob,
    Blob,
    Raw,
    Json,
    RefCursor,
    Rowid,
}

impl OracleType {
    /// Label shown in the Type pick list.
    pub fn label(self) -> &'static str {
        match self {
            Self::Varchar2 => "VARCHAR2",
            Self::Nvarchar2 => "NVARCHAR2",
            Self::Char => "CHAR",
            Self::Number => "NUMBER",
            Self::Integer => "INTEGER",
            Self::PlsInteger => "PLS_INTEGER",
            Self::BinaryFloat => "BINARY_FLOAT",
            Self::BinaryDouble => "BINARY_DOUBLE",
            Self::Date => "DATE",
            Self::Timestamp => "TIMESTAMP",
            Self::TimestampTz => "TIMESTAMP WITH TIME ZONE",
            Self::TimestampLtz => "TIMESTAMP WITH LOCAL TIME ZONE",
            Self::Boolean => "BOOLEAN",
            Self::Clob => "CLOB",
            Self::Nclob => "NCLOB",
            Self::Blob => "BLOB",
            Self::Raw => "RAW",
            Self::Json => "JSON",
            Self::RefCursor => "REF CURSOR",
            Self::Rowid => "ROWID",
        }
    }

    /// Oracle types offered for bind / test parameters.
    pub const ALL: [Self; 20] = [
        Self::Varchar2,
        Self::Nvarchar2,
        Self::Char,
        Self::Number,
        Self::Integer,
        Self::PlsInteger,
        Self::BinaryFloat,
        Self::BinaryDouble,
        Self::Date,
        Self::Timestamp,
        Self::TimestampTz,
        Self::TimestampLtz,
        Self::Boolean,
        Self::Clob,
        Self::Nclob,
        Self::Blob,
        Self::Raw,
        Self::Json,
        Self::RefCursor,
        Self::Rowid,
    ];
}

impl std::fmt::Display for OracleType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// PostgreSQL types shown in the Type column for Postgres worksheets.
///
/// # Examples
///
/// ```
/// use plsqlide::ui::parameters::PostgresType;
///
/// assert_eq!(PostgresType::Text.label(), "text");
/// assert!(PostgresType::ALL.contains(&PostgresType::Jsonb));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PostgresType {
    #[default]
    Text,
    Varchar,
    Char,
    Smallint,
    Integer,
    Bigint,
    Numeric,
    Real,
    DoublePrecision,
    Boolean,
    Date,
    Time,
    Timetz,
    Timestamp,
    Timestamptz,
    Interval,
    Bytea,
    Json,
    Jsonb,
    Uuid,
    Refcursor,
}

impl PostgresType {
    /// Label shown in the Type pick list (PostgreSQL spelling).
    pub fn label(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Varchar => "varchar",
            Self::Char => "char",
            Self::Smallint => "smallint",
            Self::Integer => "integer",
            Self::Bigint => "bigint",
            Self::Numeric => "numeric",
            Self::Real => "real",
            Self::DoublePrecision => "double precision",
            Self::Boolean => "boolean",
            Self::Date => "date",
            Self::Time => "time",
            Self::Timetz => "time with time zone",
            Self::Timestamp => "timestamp",
            Self::Timestamptz => "timestamptz",
            Self::Interval => "interval",
            Self::Bytea => "bytea",
            Self::Json => "json",
            Self::Jsonb => "jsonb",
            Self::Uuid => "uuid",
            Self::Refcursor => "refcursor",
        }
    }

    /// PostgreSQL types offered for bind / test parameters.
    pub const ALL: [Self; 21] = [
        Self::Text,
        Self::Varchar,
        Self::Char,
        Self::Smallint,
        Self::Integer,
        Self::Bigint,
        Self::Numeric,
        Self::Real,
        Self::DoublePrecision,
        Self::Boolean,
        Self::Date,
        Self::Time,
        Self::Timetz,
        Self::Timestamp,
        Self::Timestamptz,
        Self::Interval,
        Self::Bytea,
        Self::Json,
        Self::Jsonb,
        Self::Uuid,
        Self::Refcursor,
    ];
}

impl std::fmt::Display for PostgresType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// Dialect-tagged parameter type for the Type column.
///
/// # Examples
///
/// ```
/// use plsqlide::core::DbKind;
/// use plsqlide::ui::parameters::{OracleType, ParamDataType, PostgresType};
///
/// assert_eq!(ParamDataType::default_for(DbKind::Oracle).label(), "VARCHAR2");
/// assert_eq!(ParamDataType::default_for(DbKind::Postgres).label(), "text");
/// assert!(ParamDataType::for_db(DbKind::Postgres).contains(&ParamDataType::Postgres(PostgresType::Uuid)));
/// assert!(ParamDataType::for_db(DbKind::Oracle).contains(&ParamDataType::Oracle(OracleType::Clob)));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamDataType {
    Oracle(OracleType),
    Postgres(PostgresType),
}

impl ParamDataType {
    /// Label shown in the Type column.
    pub fn label(self) -> &'static str {
        match self {
            Self::Oracle(t) => t.label(),
            Self::Postgres(t) => t.label(),
        }
    }

    /// Database dialect this type belongs to.
    pub fn db_kind(self) -> DbKind {
        match self {
            Self::Oracle(_) => DbKind::Oracle,
            Self::Postgres(_) => DbKind::Postgres,
        }
    }

    /// Default type when adding a parameter for `kind`.
    pub fn default_for(kind: DbKind) -> Self {
        match kind {
            DbKind::Oracle => Self::Oracle(OracleType::Varchar2),
            DbKind::Postgres => Self::Postgres(PostgresType::Text),
        }
    }

    /// Types offered in the Type pick list for the worksheet database.
    pub fn for_db(kind: DbKind) -> Vec<Self> {
        match kind {
            DbKind::Oracle => OracleType::ALL
                .iter()
                .copied()
                .map(Self::Oracle)
                .collect(),
            DbKind::Postgres => PostgresType::ALL
                .iter()
                .copied()
                .map(Self::Postgres)
                .collect(),
        }
    }

    /// Keeps the value if it matches `kind`, otherwise returns the dialect default.
    pub fn coerce_to(self, kind: DbKind) -> Self {
        if self.db_kind() == kind {
            self
        } else {
            Self::default_for(kind)
        }
    }
}

impl Default for ParamDataType {
    fn default() -> Self {
        Self::Postgres(PostgresType::Text)
    }
}

impl std::fmt::Display for ParamDataType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// One bind / test parameter row.
///
/// # Examples
///
/// ```
/// use plsqlide::ui::parameters::{ParamDataType, ParamMode, Parameter, PostgresType};
///
/// let p = Parameter::new(
///     "p_id",
///     ParamMode::In,
///     ParamDataType::Postgres(PostgresType::Integer),
/// );
/// assert_eq!(p.name, "p_id");
/// assert_eq!(p.mode, ParamMode::In);
/// assert!(p.value.is_empty());
/// ```
#[derive(Debug, Clone)]
pub struct Parameter {
    pub name: String,
    pub mode: ParamMode,
    pub data_type: ParamDataType,
    pub value: String,
}

impl Parameter {
    /// Creates an empty-valued parameter with the given identity.
    ///
    /// # Examples
    ///
    /// ```
    /// use plsqlide::ui::parameters::{OracleType, ParamDataType, ParamMode, Parameter};
    ///
    /// let p = Parameter::new(
    ///     "v_name",
    ///     ParamMode::Out,
    ///     ParamDataType::Oracle(OracleType::Varchar2),
    /// );
    /// assert_eq!(p.mode, ParamMode::Out);
    /// assert_eq!(p.data_type.label(), "VARCHAR2");
    /// ```
    pub fn new(name: impl Into<String>, mode: ParamMode, data_type: ParamDataType) -> Self {
        Self {
            name: name.into(),
            mode,
            data_type,
            value: String::new(),
        }
    }
}

/// Mutable state of the parameters grid for one worksheet.
///
/// # Examples
///
/// ```
/// use plsqlide::core::DbKind;
/// use plsqlide::ui::parameters::ParametersPanel;
///
/// let mut panel = ParametersPanel::default();
/// panel.add(DbKind::Postgres);
/// assert_eq!(panel.rows.len(), 1);
/// panel.select(0);
/// assert_eq!(panel.selected, Some(0));
/// assert!(panel.delete_selected());
/// assert!(panel.rows.is_empty());
/// ```
#[derive(Debug, Clone, Default)]
pub struct ParametersPanel {
    pub rows: Vec<Parameter>,
    /// Index of the row highlighted for deletion (left-click selection).
    pub selected: Option<usize>,
}

impl ParametersPanel {
    /// Appends a new IN parameter with the default type for `db_kind`.
    ///
    /// # Examples
    ///
    /// ```
    /// use plsqlide::core::DbKind;
    /// use plsqlide::ui::parameters::ParametersPanel;
    ///
    /// let mut panel = ParametersPanel::default();
    /// panel.add(DbKind::Oracle);
    /// assert_eq!(panel.rows.len(), 1);
    /// assert_eq!(panel.selected, Some(0));
    /// assert_eq!(panel.rows[0].data_type.label(), "VARCHAR2");
    /// ```
    pub fn add(&mut self, db_kind: DbKind) {
        let index = self.rows.len() + 1;
        self.rows.push(Parameter::new(
            format!("p_{index}"),
            ParamMode::In,
            ParamDataType::default_for(db_kind),
        ));
        self.selected = Some(self.rows.len() - 1);
    }

    /// Highlights the row at `index` so Delete can remove it.
    ///
    /// # Examples
    ///
    /// ```
    /// use plsqlide::core::DbKind;
    /// use plsqlide::ui::parameters::ParametersPanel;
    ///
    /// let mut panel = ParametersPanel::default();
    /// panel.add(DbKind::Postgres);
    /// panel.select(0);
    /// assert_eq!(panel.selected, Some(0));
    /// panel.select(99);
    /// assert_eq!(panel.selected, Some(0));
    /// ```
    pub fn select(&mut self, index: usize) {
        if index < self.rows.len() {
            self.selected = Some(index);
        }
    }

    /// Removes the currently selected row.
    ///
    /// Returns `true` when a row was deleted.
    ///
    /// # Examples
    ///
    /// ```
    /// use plsqlide::core::DbKind;
    /// use plsqlide::ui::parameters::ParametersPanel;
    ///
    /// let mut panel = ParametersPanel::default();
    /// assert!(!panel.delete_selected());
    /// panel.add(DbKind::Postgres);
    /// assert!(panel.delete_selected());
    /// ```
    pub fn delete_selected(&mut self) -> bool {
        let Some(index) = self.selected else {
            return false;
        };
        if index >= self.rows.len() {
            self.selected = None;
            return false;
        }
        self.rows.remove(index);
        self.selected = if self.rows.is_empty() {
            None
        } else {
            Some(index.min(self.rows.len() - 1))
        };
        true
    }
}

/// Tab inside the Value ellipsis editor modal.
///
/// # Examples
///
/// ```
/// use plsqlide::ui::parameters::ValueEditorTab;
///
/// assert_eq!(ValueEditorTab::Text.label(), "text");
/// assert_eq!(ValueEditorTab::Hex.label(), "hex");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ValueEditorTab {
    #[default]
    Text,
    Rtf,
    Html,
    Hex,
}

impl ValueEditorTab {
    /// Lowercase tab caption (text / rtf / html / hex).
    pub fn label(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Rtf => "rtf",
            Self::Html => "html",
            Self::Hex => "hex",
        }
    }

    /// All tabs in left-to-right order.
    pub fn all() -> [Self; 4] {
        [Self::Text, Self::Rtf, Self::Html, Self::Hex]
    }
}

/// Draft state while the Value ellipsis modal is open.
///
/// # Examples
///
/// ```
/// use plsqlide::ui::parameters::{ValueEditorDraft, ValueEditorTab};
///
/// let mut draft = ValueEditorDraft::open(0, "hello");
/// assert_eq!(draft.param_index, 0);
/// assert_eq!(draft.tab, ValueEditorTab::Text);
/// draft.set_tab(ValueEditorTab::Hex);
/// assert_eq!(draft.tab, ValueEditorTab::Hex);
/// assert!(draft.hex_text.contains("68"));
/// assert_eq!(draft.committed_value(), "hello");
/// ```
#[derive(Debug)]
pub struct ValueEditorDraft {
    pub param_index: usize,
    pub tab: ValueEditorTab,
    /// Editable content for text / rtf / html tabs (same underlying string).
    pub content: Content,
    /// Editable hex representation when the hex tab is active.
    pub hex_text: String,
}

impl ValueEditorDraft {
    /// Opens the modal for `param_index` with the current parameter `value`.
    ///
    /// # Examples
    ///
    /// ```
    /// use plsqlide::ui::parameters::ValueEditorDraft;
    ///
    /// let draft = ValueEditorDraft::open(2, "abc");
    /// assert_eq!(draft.param_index, 2);
    /// assert_eq!(draft.hex_text, "61 62 63");
    /// ```
    pub fn open(param_index: usize, value: &str) -> Self {
        Self {
            param_index,
            tab: ValueEditorTab::Text,
            content: Content::with_text(value),
            hex_text: bytes_to_hex(value.as_bytes()),
        }
    }

    /// Switches the active tab; syncs hex ↔ text when entering/leaving hex.
    ///
    /// # Examples
    ///
    /// ```
    /// use plsqlide::ui::parameters::{ValueEditorDraft, ValueEditorTab};
    ///
    /// let mut draft = ValueEditorDraft::open(0, "A");
    /// draft.set_tab(ValueEditorTab::Hex);
    /// assert_eq!(draft.tab, ValueEditorTab::Hex);
    /// draft.set_tab(ValueEditorTab::Text);
    /// assert_eq!(draft.committed_value(), "A");
    /// ```
    pub fn set_tab(&mut self, tab: ValueEditorTab) {
        if self.tab == tab {
            return;
        }
        if self.tab == ValueEditorTab::Hex {
            if let Ok(raw) = hex_to_bytes(&self.hex_text) {
                let text = String::from_utf8_lossy(&raw).into_owned();
                self.content = Content::with_text(&text);
            }
        } else if tab == ValueEditorTab::Hex {
            self.hex_text = bytes_to_hex(self.content.text().as_bytes());
        }
        self.tab = tab;
    }

    /// Applies a text-editor action on the non-hex tabs.
    pub fn perform_editor(&mut self, action: Action) {
        self.content.perform(action);
        if self.content.is_empty() {
            self.content = Content::new();
        }
    }

    /// Replaces the hex draft text (hex tab only).
    pub fn set_hex_text(&mut self, value: String) {
        self.hex_text = value;
    }

    /// Value to write back into the parameter on OK.
    ///
    /// # Examples
    ///
    /// ```
    /// use plsqlide::ui::parameters::{ValueEditorDraft, ValueEditorTab};
    ///
    /// let mut draft = ValueEditorDraft::open(0, "hi");
    /// assert_eq!(draft.committed_value(), "hi");
    /// draft.set_tab(ValueEditorTab::Hex);
    /// draft.set_hex_text("6f 6b".into());
    /// assert_eq!(draft.committed_value(), "ok");
    /// ```
    pub fn committed_value(&self) -> String {
        if self.tab == ValueEditorTab::Hex {
            match hex_to_bytes(&self.hex_text) {
                Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
                Err(_) => self.content.text(),
            }
        } else {
            self.content.text()
        }
    }
}

/// Converts raw bytes to spaced lowercase hex (`"61 62 63"`).
///
/// # Examples
///
/// ```
/// use plsqlide::ui::parameters::bytes_to_hex;
///
/// assert_eq!(bytes_to_hex(b"AB"), "41 42");
/// assert_eq!(bytes_to_hex(b""), "");
/// ```
pub fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Parses spaced or continuous hex back into bytes.
///
/// # Examples
///
/// ```
/// use plsqlide::ui::parameters::hex_to_bytes;
///
/// assert_eq!(hex_to_bytes("41 42").unwrap(), b"AB");
/// assert_eq!(hex_to_bytes("4142").unwrap(), b"AB");
/// assert!(hex_to_bytes("zz").is_err());
/// ```
pub fn hex_to_bytes(input: &str) -> Result<Vec<u8>, String> {
    let cleaned: String = input.chars().filter(|c| !c.is_whitespace()).collect();
    if cleaned.is_empty() {
        return Ok(Vec::new());
    }
    if cleaned.len() % 2 != 0 {
        return Err("Hex length must be even".into());
    }
    (0..cleaned.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&cleaned[i..i + 2], 16)
                .map_err(|_| format!("Invalid hex at position {i}"))
        })
        .collect()
}

/// UI messages produced by the parameters panel and value editor.
#[derive(Debug, Clone)]
pub enum ParamMessage {
    Add,
    Delete,
    Select(usize),
    NameChanged(usize, String),
    ModeChanged(usize, ParamMode),
    TypeChanged(usize, ParamDataType),
    ValueChanged(usize, String),
    OpenValueEditor(usize),
    EditorTab(ValueEditorTab),
    EditorAction(Action),
    EditorHexChanged(String),
    EditorOk,
    EditorCancel,
}

/// Renders the parameters grid (header, toolbar, rows).
///
/// Type options come from `db_kind` (Oracle vs PostgreSQL).
/// `height` is the panel height in logical pixels (user-resizable via the
/// splitter above the panel).
///
/// # Examples
///
/// ```ignore
/// // Used from `editor_pane` when the active window is SQL or Test:
/// // parameters_view(&sheet.parameters, db_kind, height).map(Message::Param)
/// ```
pub fn parameters_view(
    panel: &ParametersPanel,
    db_kind: DbKind,
    height: f32,
) -> Element<'_, ParamMessage> {
    let toolbar = row![
        text("Parameters").size(12),
        Space::new().width(Fill),
        param_icon_button(plus().size(14), "Add", ParamMessage::Add),
        param_icon_button(trash().size(14), "Delete", ParamMessage::Delete),
    ]
    .spacing(6)
    .align_y(Vertical::Center)
    .padding(Padding::from([4, 8]));

    let header = row![
        header_cell("", 28.0),
        header_cell("Variable", 140.0),
        header_cell("Mode", 90.0),
        header_cell("Type", 180.0),
        header_cell("Value", Fill),
    ]
    .spacing(1)
    .padding(Padding::from([0, 4]));

    let mut rows = column![].spacing(1);
    for (index, param) in panel.rows.iter().enumerate() {
        let selected = panel.selected == Some(index);
        rows = rows.push(parameter_row(index, param, selected, db_kind));
    }

    let body = scrollable(rows.padding(Padding::from([0, 4]))).height(Fill);

    container(
        column![
            container(toolbar).style(|_| chrome_bar_style()),
            container(header).style(|_| header_bar_style()),
            body,
        ]
        .spacing(0)
        .height(Fill),
    )
    .width(Fill)
    .height(height)
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

fn parameter_row(
    index: usize,
    param: &Parameter,
    selected: bool,
    db_kind: DbKind,
) -> Element<'_, ParamMessage> {
    let marker = if selected { "✓" } else { " " };
    let type_options = ParamDataType::for_db(db_kind);
    let selected_type = param.data_type.coerce_to(db_kind);

    let name_input = text_input("Variable", &param.name)
        .on_input(move |value| ParamMessage::NameChanged(index, value))
        .padding(4)
        .size(12)
        .width(140);

    let mode_pick = pick_list(
        ParamMode::all(),
        Some(param.mode),
        move |mode| ParamMessage::ModeChanged(index, mode),
    )
    .padding(4)
    .text_size(12)
    .width(90);

    let type_pick = pick_list(
        type_options,
        Some(selected_type),
        move |data_type| ParamMessage::TypeChanged(index, data_type),
    )
    .padding(4)
    .text_size(12)
    .width(180);

    let value_input = text_input("Value", &param.value)
        .on_input(move |value| ParamMessage::ValueChanged(index, value))
        .padding(4)
        .size(12)
        .width(Fill);

    let ellipsis = button(text("...").size(11).font(Font::MONOSPACE))
        .padding(Padding::from([2, 8]))
        .style(|_, status| chrome_button_style(status))
        .on_press(ParamMessage::OpenValueEditor(index));

    let cells = row![
        container(text(marker).size(12))
            .width(28)
            .align_x(Horizontal::Center)
            .align_y(Vertical::Center),
        name_input,
        mode_pick,
        type_pick,
        value_input,
        ellipsis,
    ]
    .spacing(4)
    .align_y(Vertical::Center)
    .padding(Padding::from([2, 2]));

    mouse_area(
        container(cells)
            .width(Fill)
            .style(move |_| row_style(selected)),
    )
    .on_press(ParamMessage::Select(index))
    .into()
}

fn header_cell<'a>(label: &'a str, width: impl Into<Length>) -> Element<'a, ParamMessage> {
    container(text(label).size(11).color(ui_theme::current().muted))
        .width(width)
        .padding(Padding::from([4, 4]))
        .into()
}

/// Modal dialog for editing a parameter value with text/rtf/html/hex tabs.
///
/// # Examples
///
/// ```ignore
/// // Shown when `app.value_editor` is `Some`:
/// // value_editor_modal(&draft).map(Message::Param)
/// ```
pub fn value_editor_modal(draft: &ValueEditorDraft) -> Element<'_, ParamMessage> {
    let mut tabs = row![].spacing(0);
    for tab in ValueEditorTab::all() {
        let active = draft.tab == tab;
        tabs = tabs.push(
            button(text(tab.label()).size(12))
                .padding(Padding::from([6, 14]))
                .style(move |_, status| tab_style(active, status))
                .on_press(ParamMessage::EditorTab(tab)),
        );
    }

    let editor: Element<'_, ParamMessage> = match draft.tab {
        ValueEditorTab::Hex => text_input("hex bytes", &draft.hex_text)
            .on_input(ParamMessage::EditorHexChanged)
            .padding(8)
            .size(13)
            .font(Font::MONOSPACE)
            .width(Fill)
            .into(),
        ValueEditorTab::Text | ValueEditorTab::Rtf | ValueEditorTab::Html => {
            text_editor(&draft.content)
                .height(Fill)
                .font(Font::MONOSPACE)
                .size(13)
                .padding(8)
                .on_action(ParamMessage::EditorAction)
                .into()
        }
    };

    let form = column![
        text("Edit value").size(16),
        container(tabs).width(Fill).style(|_| header_bar_style()),
        container(editor)
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
            button(text("Cancel").size(13))
                .padding(Padding::from([8, 16]))
                .style(|_, status| chrome_button_style(status))
                .on_press(ParamMessage::EditorCancel),
            Space::new().width(Fill),
            button(text("OK").size(13))
                .padding(Padding::from([8, 16]))
                .style(|_, status| primary_button_style(status))
                .on_press(ParamMessage::EditorOk),
        ]
        .spacing(8),
    ]
    .spacing(10)
    .padding(16)
    .width(560);

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
    .on_press(ParamMessage::EditorCancel);

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

fn chrome_bar_style() -> container::Style {
    container::Style {
        background: Some(Background::Color(ui_theme::current().chrome_bg)),
        text_color: Some(ui_theme::current().text),
        ..container::Style::default()
    }
}

fn header_bar_style() -> container::Style {
    container::Style {
        background: Some(Background::Color(ui_theme::current().sidebar_title_bg)),
        text_color: Some(ui_theme::current().text),
        border: Border {
            color: ui_theme::current().border,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..container::Style::default()
    }
}

fn row_style(selected: bool) -> container::Style {
    container::Style {
        background: Some(Background::Color(if selected {
            ui_theme::current().hover
        } else {
            ui_theme::current().panel_bg
        })),
        border: Border {
            color: if selected {
                ui_theme::current().accent
            } else {
                Color::TRANSPARENT
            },
            width: if selected { 1.0 } else { 0.0 },
            radius: 0.0.into(),
        },
        ..container::Style::default()
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

/// Compact parameters toolbar button with a Lucide icon and a hover tooltip.
fn param_icon_button<'a>(
    icon: iced::widget::Text<'a>,
    tip: &'static str,
    message: ParamMessage,
) -> Element<'a, ParamMessage> {
    tooltip(
        button(icon)
            .padding(Padding::from([3, 8]))
            .style(|_, status| chrome_button_style(status))
            .on_press(message),
        text(tip).size(11),
        tooltip::Position::Bottom,
    )
    .gap(4)
    .into()
}

fn tab_style(active: bool, status: iced::widget::button::Status) -> iced::widget::button::Style {
    let background = if active {
        Some(Background::Color(ui_theme::current().panel_bg))
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
            ui_theme::current().accent
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
        border: Border {
            color: background,
            width: 1.0,
            radius: 3.0.into(),
        },
        ..iced::widget::button::Style::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_select_delete_roundtrip() {
        let mut panel = ParametersPanel::default();
        panel.add(DbKind::Postgres);
        panel.add(DbKind::Postgres);
        assert_eq!(panel.rows.len(), 2);
        assert_eq!(panel.rows[0].data_type.label(), "text");
        panel.select(0);
        assert!(panel.delete_selected());
        assert_eq!(panel.rows.len(), 1);
        assert_eq!(panel.selected, Some(0));
    }

    #[test]
    fn oracle_and_postgres_type_lists_differ() {
        let oracle = ParamDataType::for_db(DbKind::Oracle);
        let postgres = ParamDataType::for_db(DbKind::Postgres);
        assert!(oracle.iter().any(|t| t.label() == "VARCHAR2"));
        assert!(oracle.iter().any(|t| t.label() == "CLOB"));
        assert!(postgres.iter().any(|t| t.label() == "text"));
        assert!(postgres.iter().any(|t| t.label() == "jsonb"));
        assert!(postgres.iter().any(|t| t.label() == "uuid"));
        assert!(!postgres.iter().any(|t| t.label() == "VARCHAR2"));
        assert!(!oracle.iter().any(|t| t.label() == "jsonb"));
    }

    #[test]
    fn coerce_swaps_to_dialect_default() {
        let ora = ParamDataType::Oracle(OracleType::Clob);
        assert_eq!(ora.coerce_to(DbKind::Postgres).label(), "text");
        let pg = ParamDataType::Postgres(PostgresType::Uuid);
        assert_eq!(pg.coerce_to(DbKind::Oracle).label(), "VARCHAR2");
    }

    #[test]
    fn hex_roundtrip() {
        let hex = bytes_to_hex(b"PL/SQL");
        assert_eq!(hex_to_bytes(&hex).unwrap(), b"PL/SQL");
    }

    #[test]
    fn value_editor_commits_hex() {
        let mut draft = ValueEditorDraft::open(0, "");
        draft.set_tab(ValueEditorTab::Hex);
        draft.set_hex_text("4f 4b".into());
        assert_eq!(draft.committed_value(), "OK");
    }
}
