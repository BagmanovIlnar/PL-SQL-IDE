//! Bind / test parameters grid for SQL Window and Test Window.
//!
//! Provides the bottom parameters panel (Variable / Mode / Type / Value),
//! row selection, add/delete actions, and the Value ellipsis modal with
//! text / rtf / html / hex tabs.

use iced::alignment::{Horizontal, Vertical};
use iced::border::Border;
use iced::widget::text_editor::{Action, Content};
use iced::widget::{
    button, column, container, mouse_area, opaque, pick_list, row, scrollable, stack, text,
    text_editor, text_input, tooltip, Space,
};
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

/// Oracle parameter types of the Type column — the PL/SQL Developer list:
/// Integer, Float, String, Date, Long, Long Raw, Cursor, CLOB, BLOB, Bfile,
/// PL/SQL String, Char, Substitution, Temporary CLOB, Temporary BLOB.
///
/// # Examples
///
/// ```
/// use plsqlide::ui::parameters::OracleType;
///
/// assert_eq!(OracleType::String.label(), "String");
/// assert!(OracleType::ALL.contains(&OracleType::Substitution));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OracleType {
    #[default]
    String,
    Integer,
    Float,
    Date,
    Long,
    LongRaw,
    Cursor,
    Clob,
    Blob,
    Bfile,
    PlsqlString,
    Char,
    Substitution,
    TempClob,
    TempBlob,
}

impl OracleType {
    /// Label shown in the Type pick list (PL/SQL Developer spelling).
    pub fn label(self) -> &'static str {
        match self {
            Self::String => "String",
            Self::Integer => "Integer",
            Self::Float => "Float",
            Self::Date => "Date",
            Self::Long => "Long",
            Self::LongRaw => "Long Raw",
            Self::Cursor => "Cursor",
            Self::Clob => "CLOB",
            Self::Blob => "BLOB",
            Self::Bfile => "Bfile",
            Self::PlsqlString => "PL/SQL String",
            Self::Char => "Char",
            Self::Substitution => "Substitution",
            Self::TempClob => "Temporary CLOB",
            Self::TempBlob => "Temporary BLOB",
        }
    }

    /// PL/SQL declaration of the wrapper variable; `None` — the parameter
    /// is not declared (Substitution is replaced textually instead).
    /// Long / Long Raw have no PL/SQL-safe forms, so they bind through
    /// VARCHAR2(32760) / RAW(2000) — same semantics inside a block.
    fn decl(self) -> Option<&'static str> {
        match self {
            Self::String => Some("VARCHAR2(4000)"),
            Self::Integer | Self::Float => Some("NUMBER"),
            Self::Date => Some("DATE"),
            Self::Long => Some("VARCHAR2(32760)"),
            Self::LongRaw => Some("RAW(2000)"),
            Self::Cursor => Some("SYS_REFCURSOR"),
            Self::Clob | Self::TempClob => Some("CLOB"),
            Self::Blob | Self::TempBlob => Some("BLOB"),
            Self::Bfile => Some("BFILE"),
            Self::PlsqlString => Some("VARCHAR2(32767)"),
            Self::Char => Some("CHAR(2000)"),
            Self::Substitution => None,
        }
    }

    /// IN initializer expression (right side of `:=`); `None` — the type
    /// takes no literal initializer (Cursor).
    fn init(self, value: &str) -> Option<String> {
        let value = value.trim();
        let lit = || format!("'{}'", value.replace('\'', "''"));
        match self {
            Self::String
            | Self::PlsqlString
            | Self::Char
            | Self::Long
            | Self::Clob
            | Self::TempClob => Some(if value.is_empty() {
                "NULL".into()
            } else {
                lit()
            }),
            Self::Integer | Self::Float => {
                let decimal = nls_prefs().resolved_decimal_symbol();
                Some(numeric_init(value, &decimal))
            }
            Self::Date => Some(date_init(value, &date_input_mask())),
            // Binary panel values are hex strings; PL/SQL converts hex
            // literals to BLOB / RAW implicitly.
            Self::Blob | Self::TempBlob | Self::LongRaw => Some(if value.is_empty() {
                "NULL".into()
            } else {
                format!("'{}'", hex_compact(value.as_bytes()))
            }),
            // Bfile value is "directory, filename".
            Self::Bfile => Some(match value.split_once(',') {
                Some((dir, file)) if !dir.trim().is_empty() && !file.trim().is_empty() => format!(
                    "BFILENAME('{}', '{}')",
                    dir.trim().replace('\'', "''"),
                    file.trim().replace('\'', "''")
                ),
                _ => "NULL".into(),
            }),
            Self::Cursor | Self::Substitution => None,
        }
    }

    /// VAL expression of the capture select (`name` — uppercased parameter
    /// name); `None` — the value cannot be read back (cursors, substitution
    /// variables, BFILEs would need DBMS_LOB.FILEOPEN first).
    fn capture_expr(self, name: &str) -> Option<String> {
        match self {
            Self::String
            | Self::Integer
            | Self::Float
            | Self::Date
            | Self::Long
            | Self::Char
            | Self::PlsqlString
            | Self::Clob
            | Self::TempClob => Some(format!("TO_CHAR(PLSQLIDE_P_{name})")),
            Self::Blob | Self::TempBlob => Some(format!(
                "RAWTOHEX(DBMS_LOB.SUBSTR(PLSQLIDE_P_{name}, 1000, 1))"
            )),
            Self::LongRaw => Some(format!("RAWTOHEX(PLSQLIDE_P_{name})")),
            Self::Cursor | Self::Substitution | Self::Bfile => None,
        }
    }

    /// Oracle types offered for bind / test parameters.
    pub const ALL: [Self; 15] = [
        Self::Integer,
        Self::Float,
        Self::String,
        Self::Date,
        Self::Long,
        Self::LongRaw,
        Self::Cursor,
        Self::Clob,
        Self::Blob,
        Self::Bfile,
        Self::PlsqlString,
        Self::Char,
        Self::Substitution,
        Self::TempClob,
        Self::TempBlob,
    ];
}

impl std::fmt::Display for OracleType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// PostgreSQL parameter types of the Type column — the PL/SQL Developer
/// list mapped to the types that exist in Postgres: Integer, Float, String,
/// Date, Cursor, BLOB and Char. The Oracle-only entries (Long, Long Raw,
/// Bfile, PL/SQL String, Substitution, Temporary CLOB/BLOB) are dropped.
///
/// # Examples
///
/// ```
/// use plsqlide::ui::parameters::PostgresType;
///
/// assert_eq!(PostgresType::Text.label(), "text");
/// assert!(PostgresType::ALL.contains(&PostgresType::Bytea));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PostgresType {
    #[default]
    Text,
    Integer,
    DoublePrecision,
    Date,
    Char,
    Bytea,
    Refcursor,
}

impl PostgresType {
    /// Label shown in the Type pick list (PostgreSQL spelling).
    pub fn label(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Integer => "integer",
            Self::DoublePrecision => "double precision",
            Self::Date => "date",
            Self::Char => "char",
            Self::Bytea => "bytea",
            Self::Refcursor => "refcursor",
        }
    }

    /// PostgreSQL types offered for bind / test parameters.
    pub const ALL: [Self; 7] = [
        Self::Integer,
        Self::DoublePrecision,
        Self::Text,
        Self::Date,
        Self::Char,
        Self::Bytea,
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
/// assert_eq!(ParamDataType::default_for(DbKind::Oracle).label(), "String");
/// assert_eq!(ParamDataType::default_for(DbKind::Postgres).label(), "text");
/// assert!(ParamDataType::for_db(DbKind::Postgres).contains(&ParamDataType::Postgres(PostgresType::Bytea)));
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
            DbKind::Oracle => Self::Oracle(OracleType::String),
            DbKind::Postgres => Self::Postgres(PostgresType::Text),
        }
    }

    /// Types offered in the Type pick list for the worksheet database.
    pub fn for_db(kind: DbKind) -> Vec<Self> {
        match kind {
            DbKind::Oracle => OracleType::ALL.iter().copied().map(Self::Oracle).collect(),
            DbKind::Postgres => PostgresType::ALL
                .iter()
                .copied()
                .map(Self::Postgres)
                .collect(),
        }
    }

    /// Oracle type behind the value; `None` for Postgres rows.
    fn oracle_type(self) -> Option<OracleType> {
        match self {
            Self::Oracle(t) => Some(t),
            Self::Postgres(_) => None,
        }
    }

    /// Value coercion on a Type column change (and on every keystroke):
    /// strips what the new type cannot parse, so the panel never carries an
    /// impossible value. Cursor takes no value at all; strings, LOBs (the
    /// value bytes become the lob), Bfile and substitution keep any text.
    pub fn coerce_value(self, value: &str) -> String {
        match self {
            Self::Oracle(OracleType::Integer) | Self::Postgres(PostgresType::Integer) => {
                numeric_int_filter(value)
            }
            Self::Oracle(OracleType::Float) => {
                // One NLS decimal symbol (e.g. ',' for 1,5), not dot-runs.
                let decimal = nls_prefs().resolved_decimal_symbol();
                numeric_float_filter(value, &decimal)
            }
            Self::Postgres(PostgresType::DoublePrecision) => {
                // The IDE-wide NLS decimal symbol, like the Oracle Float
                // field — `1,5` stays `1,5`, never silently `15`.
                let decimal = nls_prefs().resolved_decimal_symbol();
                numeric_float_filter(value, &decimal)
            }
            Self::Oracle(OracleType::Date) => {
                // Only the longest prefix the NLS date mask accepts.
                date_mask_filter(value, &date_input_mask())
            }
            Self::Postgres(PostgresType::Date) => {
                // The NLS input mask, same machinery as Oracle.
                date_mask_filter(value, &pg_date_mask())
            }
            Self::Oracle(OracleType::Cursor) => String::new(),
            _ => value.to_string(),
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
/// use plsqlide::ui::parameters::{ParamDataType, Parameter, PostgresType};
///
/// let p = Parameter::new(
///     "p_id",
///     ParamDataType::Postgres(PostgresType::Integer),
/// );
/// assert_eq!(p.name, "p_id");
/// assert!(p.value.is_empty());
/// ```
#[derive(Debug, Clone)]
pub struct Parameter {
    pub name: String,
    pub data_type: ParamDataType,
    pub value: String,
}

impl Parameter {
    /// Creates an empty-valued parameter with the given identity.
    ///
    /// # Examples
    ///
    /// ```
    /// use plsqlide::ui::parameters::{OracleType, ParamDataType, Parameter};
    ///
    /// let p = Parameter::new(
    ///     "v_name",
    ///     ParamDataType::Oracle(OracleType::String),
    /// );
    /// assert_eq!(p.data_type.label(), "String");
    /// ```
    pub fn new(name: impl Into<String>, data_type: ParamDataType) -> Self {
        Self {
            name: name.into(),
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
    /// assert_eq!(panel.rows[0].data_type.label(), "String");
    /// ```
    pub fn add(&mut self, db_kind: DbKind) {
        let index = self.rows.len() + 1;
        self.rows.push(Parameter::new(
            format!("p_{index}"),
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

/// Compact uppercase hex (SQL hex literals: BLOB / RAW / bytea).
fn hex_compact(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
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
    if !cleaned.len().is_multiple_of(2) {
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
        text(crate::ui::i18n::t("params.title")).size(12),
        Space::new().width(Fill),
        param_icon_button(
            plus().size(14),
            crate::ui::i18n::t("params.add"),
            ParamMessage::Add
        ),
        param_icon_button(
            trash().size(14),
            crate::ui::i18n::t("params.delete"),
            ParamMessage::Delete,
        ),
    ]
    .spacing(6)
    .align_y(Vertical::Center)
    .padding(Padding::from([4, 8]));

    let header = row![
        header_cell(String::new(), 28.0),
        header_cell(crate::ui::i18n::t("params.variable"), 140.0),
        header_cell(crate::ui::i18n::t("params.type"), 180.0),
        header_cell(crate::ui::i18n::t("params.value"), Fill),
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

    let type_pick = pick_list(type_options, Some(selected_type), move |data_type| {
        ParamMessage::TypeChanged(index, data_type)
    })
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
        .style(|_, status| ui_theme::chrome_button(status))
        .on_press(ParamMessage::OpenValueEditor(index));

    let cells = row![
        container(text(marker).size(12))
            .width(28)
            .align_x(Horizontal::Center)
            .align_y(Vertical::Center),
        name_input,
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

fn header_cell(
    label: impl Into<String>,
    width: impl Into<Length>,
) -> Element<'static, ParamMessage> {
    container(text(label.into()).size(11).color(ui_theme::current().muted))
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
        text(crate::ui::i18n::t("params.edit_value")).size(16),
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
            button(text(crate::ui::i18n::t("params.editor.cancel")).size(13))
                .padding(Padding::from([8, 16]))
                .style(|_, status| ui_theme::chrome_button(status))
                .on_press(ParamMessage::EditorCancel),
            Space::new().width(Fill),
            button(text(crate::ui::i18n::t("params.editor.ok")).size(13))
                .padding(Padding::from([8, 16]))
                .style(|_, status| ui_theme::primary_button(status))
                .on_press(ParamMessage::EditorOk),
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
    .on_press(ParamMessage::EditorCancel);

    opaque(
        stack![
            backdrop,
            opaque(
                container(card)
                    .width(Fill)
                    .height(Fill)
                    .center_x(Fill)
                    .center_y(Fill)
            ),
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

/// Compact parameters toolbar button with a Lucide icon and a hover tooltip.
fn param_icon_button<'a>(
    icon: iced::widget::Text<'a>,
    tip: impl Into<String>,
    message: ParamMessage,
) -> Element<'a, ParamMessage> {
    tooltip(
        button(icon)
            .padding(Padding::from([3, 8]))
            .style(|_, status| ui_theme::chrome_button(status))
            .on_press(message),
        text(tip.into()).size(11),
        tooltip::Position::Bottom,
    )
    .style(|_| ui_theme::tooltip_style())
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

/// Calls `repl` for every named bind (`:NAME`) found in `source` outside
/// string literals and comments, replacing it with the returned text.
///
/// ponytail: q'…' and bracketed literals are not recognised — rare in blocks.
fn map_binds(source: &str, repl: &mut dyn FnMut(&str) -> String) -> String {
    let chars: Vec<char> = source.chars().collect();
    let (mut out, mut i, n) = (String::with_capacity(source.len()), 0usize, chars.len());
    while i < n {
        match chars[i] {
            '-' if i + 1 < n && chars[i + 1] == '-' => {
                while i < n && chars[i] != '\n' {
                    out.push(chars[i]);
                    i += 1;
                }
            }
            '/' if i + 1 < n && chars[i + 1] == '*' => {
                i += 2;
                while i + 1 < n && !(chars[i] == '*' && chars[i + 1] == '/') {
                    out.push(chars[i]);
                    i += 1;
                }
                out.push_str("*/");
                i = (i + 2).min(n);
            }
            '\'' => {
                out.push('\'');
                i += 1;
                while i < n {
                    out.push(chars[i]);
                    if chars[i] == '\'' {
                        // Doubled quote stays inside the literal.
                        if i + 1 < n && chars[i + 1] == '\'' {
                            out.push('\'');
                            i += 2;
                            continue;
                        }
                        i += 1;
                        break;
                    }
                    i += 1;
                }
            }
            ':' if i + 1 < n && (chars[i + 1].is_alphabetic() || chars[i + 1] == '_') => {
                i += 1;
                let start = i;
                while i < n
                    && (chars[i].is_alphanumeric()
                        || chars[i] == '_'
                        || chars[i] == '$'
                        || chars[i] == '#')
                {
                    i += 1;
                }
                let name: String = chars[start..i].iter().collect();
                out.push_str(&repl(&name));
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

/// Named bind placeholders in `source` (`:NAME`), uppercased, deduped,
/// in first-appearance order. Literals and comments are skipped.
pub fn parse_binds(source: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    map_binds(source, &mut |name| {
        let upper = name.to_ascii_uppercase();
        if !names.contains(&upper) {
            names.push(upper);
        }
        String::new()
    });
    names
}

/// Substitutes every `:NAME` bind with the `PLSQLIDE_P_NAME` variable.
fn bind_substitute(source: &str) -> String {
    map_binds(source, &mut |name| {
        format!("PLSQLIDE_P_{}", name.to_ascii_uppercase())
    })
}

/// NLS Options snapshot (date format / decimal symbol the parameter
/// values are entered in).
fn nls_prefs() -> crate::core::oracle_nls_prefs::OracleNlsPrefs {
    crate::core::oracle_nls_prefs::load()
}

/// Integer edit-time sanitizer: digits with a single leading `-`; the rest
/// — dot-runs (`6565........`), signs mid-number (`6-5`) — is dropped, so
/// the field can only ever hold `-?\d*`.
fn numeric_int_filter(value: &str) -> String {
    let value = value.split('.').next().unwrap_or("");
    let mut out = String::new();
    for (i, c) in value.chars().enumerate() {
        if c.is_ascii_digit() || (c == '-' && i == 0) {
            out.push(c);
        }
    }
    out
}

/// Float edit-time sanitizer: digits, one leading `-`, exactly one decimal
/// separator, one exponent `e`/`E` with its own optional sign. A second
/// separator (`6.......456`) never lands in the field.
fn numeric_float_filter(value: &str, decimal: &str) -> String {
    let decimal_char = decimal.chars().next().unwrap_or('.');
    let mut out = String::new();
    let mut seen_decimal = false;
    let mut seen_exponent = false;
    let mut seen_exp_sign = false;
    for (i, c) in value.chars().enumerate() {
        let keep = match c {
            '0'..='9' => true,
            '-' => {
                i == 0
                    || (seen_exponent
                        && !seen_exp_sign
                        && matches!(out.as_bytes().last(), Some(b'e') | Some(b'E')))
            }
            '+' => {
                seen_exponent
                    && !seen_exp_sign
                    && matches!(out.as_bytes().last(), Some(b'e') | Some(b'E'))
            }
            'e' | 'E' => !seen_exponent && i > 0,
            c if c == decimal_char && !seen_decimal && !seen_exponent => {
                seen_decimal = true;
                true
            }
            _ => false,
        };
        if keep {
            if c == 'e' || c == 'E' {
                seen_exponent = true;
            }
            if seen_exponent && (c == '-' || c == '+') && i > 0 {
                seen_exp_sign = true;
            }
            out.push(c);
        }
    }
    out
}

/// The date mask the Oracle Date input and the TO_DATE bind follow: the
/// NLS Options resolved format (converted to an Oracle mask), with
/// `HH24:MI:SS` appended when it carries no time part.
fn date_input_mask() -> String {
    with_time_default(crate::core::oracle_nls_prefs::strftime_to_oracle_mask(
        &nls_prefs().resolved_date_format(),
    ))
}

/// The PostgreSQL Date input mask (Preferences → PostgreSQL → NLS Options):
/// an Oracle-style mask with a time part appended when it has none.
fn pg_date_mask() -> String {
    with_time_default(crate::core::postgres_format_prefs::load().resolved_date_input())
}

/// `mask` with `HH24:MI:SS` appended when it carries no time part.
fn with_time_default(mask: String) -> String {
    let upper = mask.to_uppercase();
    if upper.contains("HH24") || upper.contains("HH12") {
        mask
    } else {
        format!("{mask} HH24:MI:SS")
    }
}

/// The mask with its time part cut — for values typed without a time.
fn date_only_part(mask: &str) -> String {
    let upper = mask.to_uppercase();
    let pos = ["HH24", "HH12", "HH"].iter().find_map(|t| upper.find(t));
    match pos {
        Some(p) => mask[..p].trim_end().to_string(),
        None => mask.to_string(),
    }
}

/// Which date field a numeric mask token stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DateField {
    Day,
    Month,
    Year2,
    Year4,
    DayOfYear,
    Hour,
    Minute,
    Second,
}

/// One token of a parsed date mask: a numeric field of at most `max`
/// digits, or a literal separator character.
enum DateMaskTok {
    Digits { max: u8, kind: DateField },
    Char(char),
}

/// Parses an Oracle date mask into numeric fields / literal separators.
/// `None` — masks with name tokens (DAY, MON, AM...) the editor doesn't
/// drive; those fall back to the plain character filter.
fn parse_date_mask(mask: &str) -> Option<Vec<DateMaskTok>> {
    let upper = mask.to_uppercase();
    let mut toks = Vec::new();
    let mut rest = upper.as_str();
    while !rest.is_empty() {
        let (tok, len) = if rest.starts_with("YYYY") {
            (
                DateMaskTok::Digits {
                    max: 4,
                    kind: DateField::Year4,
                },
                4,
            )
        } else if rest.starts_with("DDD") {
            (
                DateMaskTok::Digits {
                    max: 3,
                    kind: DateField::DayOfYear,
                },
                3,
            )
        } else if rest.starts_with("HH24") || rest.starts_with("HH12") {
            (
                DateMaskTok::Digits {
                    max: 2,
                    kind: DateField::Hour,
                },
                4,
            )
        } else if rest.starts_with("YY") {
            (
                DateMaskTok::Digits {
                    max: 2,
                    kind: DateField::Year2,
                },
                2,
            )
        } else if rest.starts_with("MM") {
            (
                DateMaskTok::Digits {
                    max: 2,
                    kind: DateField::Month,
                },
                2,
            )
        } else if rest.starts_with("DD") {
            (
                DateMaskTok::Digits {
                    max: 2,
                    kind: DateField::Day,
                },
                2,
            )
        } else if rest.starts_with("MI") {
            (
                DateMaskTok::Digits {
                    max: 2,
                    kind: DateField::Minute,
                },
                2,
            )
        } else if rest.starts_with("SS") {
            (
                DateMaskTok::Digits {
                    max: 2,
                    kind: DateField::Second,
                },
                2,
            )
        } else if rest.starts_with("HH") {
            (
                DateMaskTok::Digits {
                    max: 2,
                    kind: DateField::Hour,
                },
                2,
            )
        } else if ["DAY", "MONTH", "DY", "MON", "AM", "PM"]
            .iter()
            .any(|t| rest.starts_with(t))
        {
            return None;
        } else {
            let c = rest.chars().next().unwrap();
            (DateMaskTok::Char(c), c.len_utf8())
        };
        toks.push(tok);
        rest = &rest[len..];
    }
    Some(toks)
}

/// Oracle Date edit-time sanitizer: the longest prefix the mask accepts —
/// `5656767.576.55.75.65.` collapses to whatever fits DD.MM.YYYY.
/// `ponytail:` syntax-only; TO_DATE range-checks (month 57) at run time.
fn date_mask_filter(value: &str, mask: &str) -> String {
    let toks = match parse_date_mask(mask) {
        Some(toks) => toks,
        // Exotic mask: fall back to the plain character filter.
        None => {
            return value
                .chars()
                .filter(|c| c.is_ascii_digit() || matches!(c, '-' | ':' | ' ' | '.' | '/'))
                .collect()
        }
    };
    let mut out = String::new();
    let mut chars = value.chars().peekable();
    'mask: for tok in &toks {
        match tok {
            DateMaskTok::Digits { max, .. } => {
                for _ in 0..*max {
                    match chars.peek() {
                        Some(c) if c.is_ascii_digit() => {
                            out.push(*c);
                            chars.next();
                        }
                        _ => break,
                    }
                }
            }
            DateMaskTok::Char(c) => match chars.peek() {
                Some(x) if x == c => {
                    out.push(*c);
                    chars.next();
                }
                _ => break 'mask,
            },
        }
    }
    out
}

/// Postgres date edit-time sanitizer: ISO shape only —
/// `YYYY-MM-DD( HH:MM:SS)?`, longest valid prefix.
/// Postgres Date value converted from the NLS input mask to an
/// unambiguous ISO literal body — `25.12.2024` (DD.MM.YYYY) becomes
/// `2024-12-25`, with optional time. `None` — the value does not fully
/// match the mask (garbage or partial); the caller passes it through
/// quoted so the server reports it visibly.
/// `ponytail:` day-of-year (DDD) and name tokens are not converted.
fn pg_date_to_iso(value: &str, mask: &str) -> Option<String> {
    let toks = parse_date_mask(mask)?;
    let mut chars = value.trim().chars().peekable();
    let mut dd: Option<u32> = None;
    let mut mm: Option<u32> = None;
    let mut year: Option<(u32, usize)> = None; // (value, digit count)
    let mut hh: Option<u32> = None;
    let mut mi: Option<u32> = None;
    let mut ss: Option<u32> = None;
    for tok in &toks {
        match tok {
            DateMaskTok::Digits { max, kind } => {
                let mut digits = String::new();
                for _ in 0..*max {
                    match chars.peek() {
                        Some(c) if c.is_ascii_digit() => {
                            digits.push(*c);
                            chars.next();
                        }
                        _ => break,
                    }
                }
                if digits.is_empty() {
                    return None;
                }
                let value: u32 = digits.parse().ok()?;
                match kind {
                    DateField::Day => dd = Some(value),
                    DateField::Month => mm = Some(value),
                    DateField::Year2 => year = Some((value, digits.len())),
                    DateField::Year4 => year = Some((value, digits.len())),
                    DateField::Hour => hh = Some(value),
                    DateField::Minute => mi = Some(value),
                    DateField::Second => ss = Some(value),
                    DateField::DayOfYear => return None, // ponytail: no calendar math
                }
            }
            DateMaskTok::Char(c) => match chars.peek() {
                Some(x) if x == c => {
                    chars.next();
                }
                _ => break, // value ended before this separator (time absent)
            },
        }
    }
    if chars.peek().is_some() {
        return None; // leftover garbage
    }
    let (dd, mm) = (dd?, mm?);
    let (y, ydigits) = year?;
    // Two-digit years use the Postgres pivot: 69-99 → 19xx, 00-68 → 20xx.
    let year = if ydigits <= 2 {
        if y > 68 {
            1900 + y
        } else {
            2000 + y
        }
    } else {
        y
    };
    let date = format!("{year:04}-{mm:02}-{dd:02}");
    // Time must be complete or absent.
    match (hh, mi, ss) {
        (Some(h), Some(m), Some(sec)) => Some(format!("{date} {h:02}:{m:02}:{sec:02}")),
        (None, None, None) => Some(date),
        _ => None,
    }
}

/// TO_DATE literal for a Date parameter value — the value is typed in the
/// NLS Options date format (the same one grids render), so the mask comes
/// from there (already converted to an Oracle mask). A value with a time
/// part appends `HH24:MI:SS` unless the mask already carries time.
fn date_init(value: &str, mask: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        return "NULL".to_string();
    }
    // A value with a time part needs the mask's time tokens (or the
    // appended default); a plain date cuts them off.
    let mask = if value.len() > 10 {
        let upper = mask.to_uppercase();
        if upper.contains("HH24") || upper.contains("HH12") {
            mask.to_string()
        } else {
            format!("{mask} HH24:MI:SS")
        }
    } else {
        date_only_part(mask)
    };
    format!("TO_DATE('{}', '{}')", value.replace('\'', "''"), mask)
}

/// Numeric literal for Integer / Float parameter values: the NLS decimal
/// symbol (e.g. `1,5`) normalizes to the SQL `.`; a non-numeric value stays
/// a quoted literal so the run reports the typo visibly.
fn numeric_init(value: &str, decimal: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        return "NULL".to_string();
    }
    let normalized = if decimal == "." {
        value.to_string()
    } else {
        value.replace(decimal, ".")
    };
    if normalized.parse::<f64>().is_ok() {
        normalized
    } else {
        format!("'{}'", value.replace('\'', "''"))
    }
}

/// Wraps an anonymous block so its `:NAME` binds become PL/SQL variables:
/// each parameter is declared with its panel type and IN-initialized from
/// its value; Substitution parameters are replaced textually instead. With
/// `capture`, the wrapper opens the bound ref cursor `:PLSQLIDE_OUT`
/// returning every final value the type can express — the app reads them
/// back and writes them into the panel (IN/OUT “reference” semantics).
/// Returns the wrapped SQL and the line offset the block body moved down by
/// (the debugger maps pauses back to editor lines with it).
pub fn bind_wrap(source: &str, params: &[Parameter], capture: bool) -> (String, usize) {
    if params.is_empty() {
        return (source.to_string(), 0);
    }
    // Substitution rows are textually replaced (raw value, no quoting);
    // every other bind becomes a PL/SQL wrapper variable.
    let source = map_binds(source, &mut |name| match params
        .iter()
        .find(|p| p.name.eq_ignore_ascii_case(name))
    {
        Some(p) if p.data_type.oracle_type() == Some(OracleType::Substitution) => p.value.clone(),
        _ => format!("PLSQLIDE_P_{}", name.to_ascii_uppercase()),
    });
    let declared: Vec<&Parameter> = params
        .iter()
        .filter(|p| p.data_type.oracle_type() != Some(OracleType::Substitution))
        .collect();
    if declared.is_empty() {
        return (source.to_string(), 0);
    }

    let mut wrapped = String::from("DECLARE\n");
    for p in &declared {
        let Some(t) = p.data_type.oracle_type() else {
            continue;
        };
        let Some(decl) = t.decl() else {
            continue;
        };
        let init = t
            .init(&p.value)
            .map(|lit| format!(" := {lit}"))
            .unwrap_or_default();
        wrapped.push_str(&format!(
            "PLSQLIDE_P_{} {}{init};\n",
            p.name.to_ascii_uppercase(),
            decl
        ));
    }
    wrapped.push_str("BEGIN\n");
    wrapped.push_str(&bind_substitute(&source));
    if capture {
        // Cursor / Substitution / Bfile rows cannot be captured — keep the
        // ref cursor shape anyway: the runner reads :PLSQLIDE_OUT back
        // unconditionally.
        let arms: Vec<String> = declared
            .iter()
            .filter_map(|p| {
                let name = p.name.to_ascii_uppercase();
                p.data_type
                    .oracle_type()
                    .and_then(|t| t.capture_expr(&name))
                    // ponytail: single-row DUAL selects — no table, no DDL,
                    // works on gated DBs.
                    .map(|expr| format!("SELECT '{name}' AS NAME, {expr} AS VAL FROM DUAL"))
            })
            .collect();
        let select = if arms.is_empty() {
            "SELECT NULL AS NAME, NULL AS VAL FROM DUAL WHERE 1 = 0".to_string()
        } else {
            arms.join("\nUNION ALL\n")
        };
        wrapped.push_str("\nOPEN :PLSQLIDE_OUT FOR\n");
        wrapped.push_str(&select);
        wrapped.push_str(";\n");
    }
    wrapped.push_str("END;\n");
    (wrapped, declared.len() + 2)
}

/// Typed SQL literal for a Postgres parameter value (the wire protocol has
/// no named binds — the Test window substitutes textually).
fn pg_literal(data_type: PostgresType, value: &str) -> String {
    let value = value.trim();
    let lit = || format!("'{}'", value.replace('\'', "''"));
    let null_or = || {
        if value.is_empty() {
            "NULL".to_string()
        } else {
            lit()
        }
    };
    match data_type {
        PostgresType::Integer => {
            if value.is_empty() {
                "NULL::integer".into()
            } else if value.parse::<i64>().is_ok() {
                format!("{value}::integer")
            } else {
                // Run-time syntax error — the typo stays visible.
                format!("{}::integer", lit())
            }
        }
        PostgresType::DoublePrecision => {
            if value.is_empty() {
                "NULL::double precision".into()
            } else {
                // The NLS decimal symbol (1,5) normalizes to a plain '.'.
                let decimal = nls_prefs().resolved_decimal_symbol();
                let normalized = if decimal == "." {
                    value.to_string()
                } else {
                    value.replace(&decimal, ".")
                };
                if normalized.parse::<f64>().is_ok() {
                    format!("{normalized}::double precision")
                } else {
                    format!("{}::double precision", lit())
                }
            }
        }
        // Postgres parses date text itself; the value is converted from
        // the NLS input mask to an unambiguous ISO literal first.
        PostgresType::Date => {
            if value.is_empty() {
                "NULL::date".into()
            } else {
                match pg_date_to_iso(value, &pg_date_mask()) {
                    Some(iso) => format!("'{iso}'::date"),
                    // Not mask-shaped: pass through quoted so the
                    // server reports it visibly.
                    None => format!("{}::date", lit()),
                }
            }
        }
        PostgresType::Text => format!("{}::text", null_or()),
        PostgresType::Char => format!("{}::char", null_or()),
        PostgresType::Refcursor => format!("{}::refcursor", null_or()),
        // Binary panel values are hex strings.
        PostgresType::Bytea => {
            if value.is_empty() {
                "NULL::bytea".into()
            } else {
                format!("'\\x{}'::bytea", hex_compact(value.as_bytes()))
            }
        }
    }
}

/// Replaces `:NAME` binds of a Postgres script with typed literals from the
/// parameters panel (Test window). Binds without a panel row stay as-is.
pub fn bind_substitute_pg(source: &str, params: &[Parameter]) -> String {
    if params.is_empty() {
        return source.to_string();
    }
    map_binds(source, &mut |name| {
        match params.iter().find(|p| p.name.eq_ignore_ascii_case(name)) {
            Some(p) => match p.data_type {
                ParamDataType::Postgres(t) => pg_literal(t, &p.value),
                // ponytail: stale Oracle-dialect row after a profile switch —
                // raw value keeps the script runnable.
                _ => p.value.clone(),
            },
            None => format!(":{name}"),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bind_names_skipping_literals_and_comments() {
        let src = "DECLARE\n v NUMBER;\nBEGIN\n :Test := 'cd:x'; -- :NOComm\n/* :NOblock */\n x := :OUT_P;\n y := :Test;\nEND;";
        assert_eq!(parse_binds(src), vec!["TEST", "OUT_P"]);
    }

    #[test]
    fn wrap_declares_substitutes_and_offsets() {
        let params = vec![Parameter::new(
            "Test",
            ParamDataType::Oracle(OracleType::String),
        )];
        let (sql, offset) = bind_wrap("BEGIN\n :TEST := 'a''b';\nEND;", &params, true);
        assert_eq!(offset, 3);
        assert!(sql.starts_with(
            "DECLARE\nPLSQLIDE_P_TEST VARCHAR2(4000) := NULL;\nBEGIN\nBEGIN\n PLSQLIDE_P_TEST := 'a''b';\nEND;\n"
        ));
        assert!(sql.contains("OPEN :PLSQLIDE_OUT FOR\nSELECT 'TEST' AS NAME, TO_CHAR(PLSQLIDE_P_TEST) AS VAL FROM DUAL;"));
        assert_eq!(
            bind_wrap("BEGIN NULL; END;", &[], true).0,
            "BEGIN NULL; END;"
        );
    }

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
        assert!(oracle.iter().any(|t| t.label() == "String"));
        assert!(oracle.iter().any(|t| t.label() == "CLOB"));
        assert!(oracle.iter().any(|t| t.label() == "Substitution"));
        assert!(postgres.iter().any(|t| t.label() == "text"));
        assert!(postgres.iter().any(|t| t.label() == "bytea"));
        assert!(postgres.iter().any(|t| t.label() == "refcursor"));
        assert!(!postgres.iter().any(|t| t.label() == "CLOB"));
        assert!(!oracle.iter().any(|t| t.label() == "bytea"));
    }

    #[test]
    fn coerce_swaps_to_dialect_default() {
        let ora = ParamDataType::Oracle(OracleType::Clob);
        assert_eq!(ora.coerce_to(DbKind::Postgres).label(), "text");
        let pg = ParamDataType::Postgres(PostgresType::Bytea);
        assert_eq!(pg.coerce_to(DbKind::Oracle).label(), "String");
    }

    #[test]
    fn bind_wrap_declares_by_panel_type() {
        let mut params = vec![
            Parameter::new("n", ParamDataType::Oracle(OracleType::Integer)),
            Parameter::new("d", ParamDataType::Oracle(OracleType::Date)),
            Parameter::new("b", ParamDataType::Oracle(OracleType::Blob)),
            Parameter::new("c", ParamDataType::Oracle(OracleType::Cursor)),
            Parameter::new("f", ParamDataType::Oracle(OracleType::Bfile)),
        ];
        params[0].value = "42".into();
        params[1].value = "2024-02-03 10:11:12".into();
        params[2].value = "OK".into();
        params[3].value = "ignored".into();
        params[4].value = "DATA_DIR, logo.png".into();
        let (sql, offset) = bind_wrap("BEGIN NULL; END;", &params, true);
        assert_eq!(offset, params.len() + 2);
        assert!(sql.contains("PLSQLIDE_P_N NUMBER := 42;"));
        // The TO_DATE mask follows the NLS Options — machine-dependent,
        // so assert the value and the TO_DATE shape only.
        assert!(sql.contains("PLSQLIDE_P_D DATE := TO_DATE('2024-02-03 10:11:12', '"));
        assert!(sql.contains("PLSQLIDE_P_B BLOB := '4F4B';"));
        // Cursors take no initializer; Bfile splits directory / filename.
        assert!(sql.contains("PLSQLIDE_P_C SYS_REFCURSOR;"));
        assert!(sql.contains("PLSQLIDE_P_F BFILE := BFILENAME('DATA_DIR', 'logo.png');"));
        // Cursor / Bfile values are not captured; the rest come back typed.
        assert!(sql.contains("TO_CHAR(PLSQLIDE_P_N)"));
        assert!(sql.contains("RAWTOHEX(DBMS_LOB.SUBSTR(PLSQLIDE_P_B, 1000, 1))"));
        assert!(!sql.contains("SELECT 'C' AS NAME"));
        assert!(!sql.contains("SELECT 'F' AS NAME"));
    }

    #[test]
    fn substitution_params_replace_textually() {
        let mut params = vec![Parameter::new(
            "tbl",
            ParamDataType::Oracle(OracleType::Substitution),
        )];
        params[0].value = "emp".into();
        let (sql, offset) = bind_wrap("select * from :TBL where rownum = 1", &params, true);
        // No wrapper: nothing to declare, nothing to capture.
        assert_eq!(sql, "select * from emp where rownum = 1");
        assert_eq!(offset, 0);

        // Mixed: substitution inlined, the bind still wrapped.
        let mut params = vec![
            Parameter::new("tbl", ParamDataType::Oracle(OracleType::Substitution)),
            Parameter::new("id", ParamDataType::Oracle(OracleType::Integer)),
        ];
        params[0].value = "emp".into();
        params[1].value = "7".into();
        let (sql, _) = bind_wrap("select * from :TBL where id = :ID", &params, false);
        assert!(sql.contains("select * from emp where id = PLSQLIDE_P_ID"));
        assert!(sql.contains("PLSQLIDE_P_ID NUMBER := 7;"));
        assert!(!sql.contains("PLSQLIDE_P_TBL"));
    }

    #[test]
    fn pg_binds_substitute_typed_literals() {
        let mut params = vec![
            Parameter::new("id", ParamDataType::Postgres(PostgresType::Integer)),
            Parameter::new("name", ParamDataType::Postgres(PostgresType::Text)),
            Parameter::new("raw", ParamDataType::Postgres(PostgresType::Bytea)),
            Parameter::new("d", ParamDataType::Postgres(PostgresType::DoublePrecision)),
        ];
        params[0].value = "42".into();
        params[1].value = "o'brien".into();
        params[2].value = "OK".into();
        // '.'-typed values bind verbatim on any NLS decimal symbol.
        params[3].value = "1.5".into();
        let sql = bind_substitute_pg("select :id, :name, :raw, :d, :unknown from t", &params);
        assert_eq!(
            sql,
            "select 42::integer, 'o''brien'::text, '\\x4F4B'::bytea, 1.5::double precision, :unknown from t"
        );

        // Empty values bind NULL with the type kept.
        params[0].value.clear();
        params[1].value.clear();
        let sql = bind_substitute_pg("select :id, :name", &params);
        assert_eq!(sql, "select NULL::integer, NULL::text");
    }

    #[test]
    fn type_change_coerces_the_value() {
        use super::ParamDataType as T;

        // Integer keeps only the numeric part of the old value — the
        // decimal tail is cut, not kept.
        assert_eq!(T::Oracle(OracleType::Integer).coerce_value("12x7.5"), "127");
        // Date is covered by date_input_collapses_garbage (its mask is
        // machine-dependent — the NLS Windows format comes from the OS).
        // Float / double precision also keep the exponent (direct: the
        // NLS decimal symbol is machine-dependent).
        assert_eq!(numeric_float_filter("-1.5e3 kg", "."), "-1.5e3");
        // Cursor takes no value; strings / substitution keep any text.
        assert_eq!(T::Oracle(OracleType::Cursor).coerce_value("abc"), "");
        assert_eq!(T::Oracle(OracleType::String).coerce_value("abc"), "abc");
        assert_eq!(
            T::Oracle(OracleType::Substitution).coerce_value("a, b"),
            "a, b"
        );
    }

    #[test]
    fn date_input_collapses_garbage() {
        // Oracle Date: only the longest mask-valid prefix lands — the
        // dot-run collapses: DD eats 56, then the mask wants '.' but
        // sees '5' — everything after never lands (keystrokes get dropped).
        assert_eq!(
            date_mask_filter("5656767.576.55.75.65.", "DD.MM.YYYY HH24:MI:SS"),
            "56"
        );
        // Typed separators that don't fit the mask stop the run.
        assert_eq!(date_mask_filter("25-12-2024", "DD/MM/YYYY"), "25");
        assert_eq!(
            date_mask_filter("25/12/2024 10:11", "DD/MM/YYYY HH24:MI:SS"),
            "25/12/2024 10:11"
        );
        // Letters never fit a numeric mask.
        assert_eq!(date_mask_filter("hello", "DD/MM/YYYY"), "");
        // Name-token masks (AM/PM, MON...) fall back to the character filter.
        assert!(date_mask_filter("25/12/2024 AM", "DD/MM/YYYY AM")
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '-' | ':' | ' ' | '.' | '/')));

        // Postgres date uses the same mask machinery (the default mask
        // is OS-derived — machine-dependent, so test the masks directly).
        assert_eq!(
            date_mask_filter("5656767.576.55.75.65.", "YYYY-MM-DD HH24:MI:SS"),
            "5656"
        );
        assert_eq!(
            date_mask_filter("2024-12-25 10:11:12", "YYYY-MM-DD HH24:MI:SS"),
            "2024-12-25 10:11:12"
        );
    }

    #[test]
    fn pg_date_binds_as_iso_from_mask() {
        // The Russian default: DD.MM.YYYY typed, ISO bound.
        let mask = "DD.MM.YYYY HH24:MI:SS";
        assert_eq!(
            pg_date_to_iso("25.12.2024", mask),
            Some("2024-12-25".into())
        );
        assert_eq!(
            pg_date_to_iso("25.12.2024 10:11:12", mask),
            Some("2024-12-25 10:11:12".into())
        );
        // Single-digit fields, like psql accepts them.
        assert_eq!(pg_date_to_iso("5.1.2024", mask), Some("2024-01-05".into()));
        // US order in the mask is honoured.
        assert_eq!(
            pg_date_to_iso("12/25/2024", "MM/DD/YYYY"),
            Some("2024-12-25".into())
        );
        // Two-digit years use the Postgres pivot (69-99 → 19xx).
        assert_eq!(pg_date_to_iso("01.01.99", mask), Some("1999-01-01".into()));
        // Garbage / partial time never converts — passthrough, visible.
        assert_eq!(pg_date_to_iso("5656767.576.55.75.65.", mask), None);
        assert_eq!(pg_date_to_iso("25.12.2024 10", mask), None);
        // Day-of-year masks are not converted.
        assert_eq!(pg_date_to_iso("360 2024", "DDD YYYY"), None);
    }

    #[test]
    fn numeric_input_filters_garbage() {
        // Integer: dot-runs and mid-number signs never land in the field.
        assert_eq!(
            ParamDataType::Oracle(OracleType::Integer).coerce_value("6565........"),
            "6565"
        );
        assert_eq!(
            ParamDataType::Oracle(OracleType::Integer).coerce_value("6-5"),
            "65"
        );
        assert_eq!(
            ParamDataType::Postgres(PostgresType::Integer).coerce_value("-12x7.5"),
            "-127"
        );

        // Float: exactly one separator, one exponent — `6.......456.456..456.`
        // can only end up as the longest valid run. (Direct filter calls:
        // the PG Double field follows the NLS decimal symbol, which is
        // machine-dependent through the prefs file.)
        assert_eq!(
            numeric_float_filter("6.......456.456..456.456.", "."),
            "6.456456456456"
        );
        assert_eq!(numeric_float_filter("1e2e3", "."), "1e23");
        assert_eq!(numeric_float_filter("-1.5e-", "."), "-1.5e-");
        assert_eq!(numeric_float_filter("1,5", ","), "1,5");

        // The NLS decimal symbol (',' for 1,5) is the one separator kept.
        assert_eq!(numeric_float_filter("1,5", ","), "1,5");
        assert_eq!(numeric_float_filter("1.5", ","), "15");
    }

    #[test]
    fn date_and_number_binds_follow_nls() {
        // Date: the panel value is typed in the NLS date format.
        assert_eq!(
            date_init("25/12/2024", "DD/MM/YYYY"),
            "TO_DATE('25/12/2024', 'DD/MM/YYYY')"
        );
        // A value with a time part appends HH24:MI:SS when the mask has none.
        assert_eq!(
            date_init("25/12/2024 10:11:12", "DD/MM/YYYY"),
            "TO_DATE('25/12/2024 10:11:12', 'DD/MM/YYYY HH24:MI:SS')"
        );
        // A mask with its own time part is used as-is.
        assert_eq!(
            date_init("25/12/2024 10:11:12", "DD/MM/YYYY HH24:MI:SS"),
            "TO_DATE('25/12/2024 10:11:12', 'DD/MM/YYYY HH24:MI:SS')"
        );
        assert_eq!(date_init("", "DD/MM/YYYY"), "NULL");

        // Numbers: the NLS decimal symbol normalizes to '.'.
        assert_eq!(numeric_init("1,5", ","), "1.5");
        assert_eq!(numeric_init("1.5", "."), "1.5");
        assert_eq!(numeric_init("1,5", "."), "'1,5'");
        assert_eq!(numeric_init("42", ","), "42");
        assert_eq!(numeric_init("", ","), "NULL");

        // The Windows strftime pattern converts to the Oracle mask first.
        assert_eq!(
            crate::core::oracle_nls_prefs::strftime_to_oracle_mask("%d/%m/%Y"),
            "DD/MM/YYYY"
        );
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
