//! Worksheet window kinds and code templates from the product plan.

/// Window type created via File → New.
///
/// # Examples
///
/// ```
/// use plsqlide::ui::window_kind::WindowKind;
///
/// assert_eq!(WindowKind::Sql.tab_title(), "SQL Window");
/// assert!(WindowKind::Procedure.template().contains("PROCEDURE"));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowKind {
    Blank,
    Function,
    JavaSource,
    Package,
    PackageBody,
    PackageSpecification,
    Procedure,
    Trigger,
    Type,
    TypeBody,
    TypeSpecification,
    Test,
    Sql,
    Command,
    ExplainPlan,
}

impl WindowKind {
    /// Short title shown on the worksheet tab.
    pub fn tab_title(self) -> &'static str {
        match self {
            Self::Blank => "Program Window",
            Self::Function => "Function",
            Self::JavaSource => "Java Source",
            Self::Package => "Package",
            Self::PackageBody => "Package Body",
            Self::PackageSpecification => "Package Spec",
            Self::Procedure => "Procedure",
            Self::Trigger => "Trigger",
            Self::Type => "Type",
            Self::TypeBody => "Type Body",
            Self::TypeSpecification => "Type Spec",
            Self::Test => "Test Window",
            Self::Sql => "SQL Window",
            Self::Command => "Command Window",
            Self::ExplainPlan => "Explain Plan",
        }
    }

    /// Syntect / iced highlighter token for this window kind.
    ///
    /// # Examples
    ///
    /// ```
    /// use plsqlide::ui::window_kind::WindowKind;
    ///
    /// assert_eq!(WindowKind::Sql.syntax(), "sql");
    /// assert_eq!(WindowKind::JavaSource.syntax(), "java");
    /// ```
    pub fn syntax(self) -> &'static str {
        match self {
            Self::JavaSource => "java",
            _ => "sql",
        }
    }

    /// Whether this window shows the bind / test parameters grid.
    ///
    /// # Examples
    ///
    /// ```
    /// use plsqlide::ui::window_kind::WindowKind;
    ///
    /// assert!(WindowKind::Sql.shows_parameters());
    /// assert!(WindowKind::Test.shows_parameters());
    /// assert!(!WindowKind::Command.shows_parameters());
    /// ```
    pub fn shows_parameters(self) -> bool {
        matches!(self, Self::Test | Self::Sql)
    }

    /// Initial template text for a new window.
    pub fn template(self) -> &'static str {
        match self {
            Self::Blank => "",
            Self::Function => {
                "CREATE OR REPLACE FUNCTION function_name (\n\
                 \t  p_arg IN NUMBER\n\
                 ) RETURN NUMBER\n\
                 AS\n\
                 BEGIN\n\
                 \t  RETURN p_arg;\n\
                 END;\n\
                 /\n"
            }
            Self::JavaSource => {
                "CREATE OR REPLACE AND COMPILE JAVA SOURCE NAMED \"Hello\" AS\n\
                 public class Hello {\n\
                 \t  public static String world() {\n\
                 \t    return \"Hello\";\n\
                 \t  }\n\
                 }\n\
                 /\n"
            }
            Self::Package | Self::PackageSpecification => {
                "CREATE OR REPLACE PACKAGE package_name AS\n\
                 \t  PROCEDURE demo;\n\
                 END package_name;\n\
                 /\n"
            }
            Self::PackageBody => {
                "CREATE OR REPLACE PACKAGE BODY package_name AS\n\
                 \t  PROCEDURE demo IS\n\
                 \t  BEGIN\n\
                 \t    NULL;\n\
                 \t  END;\n\
                 END package_name;\n\
                 /\n"
            }
            Self::Procedure => {
                "CREATE OR REPLACE PROCEDURE procedure_name (\n\
                 \t  p_arg IN NUMBER\n\
                 ) AS\n\
                 BEGIN\n\
                 \t  NULL;\n\
                 END;\n\
                 /\n"
            }
            Self::Trigger => {
                "CREATE OR REPLACE TRIGGER trigger_name\n\
                 BEFORE INSERT ON table_name\n\
                 FOR EACH ROW\n\
                 BEGIN\n\
                 \t  NULL;\n\
                 END;\n\
                 /\n"
            }
            Self::Type | Self::TypeSpecification => {
                "CREATE OR REPLACE TYPE type_name AS OBJECT (\n\
                 \t  id NUMBER,\n\
                 \t  name VARCHAR2(100)\n\
                 );\n\
                 /\n"
            }
            Self::TypeBody => {
                "CREATE OR REPLACE TYPE BODY type_name AS\n\
                 \t  -- member implementations\n\
                 END;\n\
                 /\n"
            }
            Self::Test => {
                "DECLARE\n\
                 \t  v_x NUMBER;\n\
                 BEGIN\n\
                 \t  NULL;\n\
                 END;\n\
                 /\n"
            }
            Self::Sql => "SELECT * FROM dual;\n",
            Self::Command => "SET SERVEROUTPUT ON\n",
            Self::ExplainPlan => "EXPLAIN PLAN FOR\nSELECT 1 FROM dual;\n",
        }
    }
}
