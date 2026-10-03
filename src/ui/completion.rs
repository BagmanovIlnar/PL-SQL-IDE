//! Basic SQL / PL/SQL keyword completion for the editor.

/// Default keyword dictionary for Phase 0 autocomplete.
pub const SQL_KEYWORDS: &[&str] = &[
    "SELECT",
    "FROM",
    "WHERE",
    "AND",
    "OR",
    "NOT",
    "IN",
    "EXISTS",
    "BETWEEN",
    "LIKE",
    "IS",
    "NULL",
    "AS",
    "JOIN",
    "INNER",
    "LEFT",
    "RIGHT",
    "FULL",
    "OUTER",
    "ON",
    "GROUP",
    "BY",
    "ORDER",
    "HAVING",
    "UNION",
    "ALL",
    "DISTINCT",
    "INSERT",
    "INTO",
    "VALUES",
    "UPDATE",
    "SET",
    "DELETE",
    "CREATE",
    "OR",
    "REPLACE",
    "ALTER",
    "DROP",
    "TABLE",
    "VIEW",
    "INDEX",
    "SEQUENCE",
    "TRIGGER",
    "PROCEDURE",
    "FUNCTION",
    "PACKAGE",
    "BODY",
    "TYPE",
    "DECLARE",
    "BEGIN",
    "END",
    "IF",
    "THEN",
    "ELSE",
    "ELSIF",
    "LOOP",
    "WHILE",
    "FOR",
    "RETURN",
    "COMMIT",
    "ROLLBACK",
    "GRANT",
    "REVOKE",
    "EXPLAIN",
    "PLAN",
    "DUAL",
    "CASE",
    "WHEN",
    "WITH",
    "LIMIT",
    "FETCH",
    "FIRST",
    "ROWS",
    "ONLY",
    "COUNT",
    "SUM",
    "AVG",
    "MAX",
    "MIN",
    "NVL",
    "COALESCE",
    "TO_CHAR",
    "TO_DATE",
    "SYSDATE",
    "CURRENT_TIMESTAMP",
];

/// Extracts the word prefix at the given line/column.
///
/// # Examples
///
/// ```
/// use plsqlide::ui::completion::word_prefix_at;
///
/// assert_eq!(word_prefix_at("select * fr", 0, 11).as_deref(), Some("fr"));
/// assert_eq!(word_prefix_at("select ", 0, 7).as_deref(), None);
/// assert_eq!(word_prefix_at("selя", 0, 5).as_deref(), None);
/// assert_eq!(word_prefix_at("яse", 0, 4).as_deref(), Some("se"));
/// ```
pub fn word_prefix_at(line: &str, _line_index: usize, column: usize) -> Option<String> {
    // `column` from iced/cosmic-text is a byte offset; clamp to a char boundary.
    let mut end = column.min(line.len());
    while end > 0 && !line.is_char_boundary(end) {
        end -= 1;
    }
    let before = &line[..end];
    let start = before
        .rfind(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
        .map(|index| index + before[index..].chars().next().map_or(1, char::len_utf8))
        .unwrap_or(0);
    let prefix = &before[start..];
    if prefix.is_empty() {
        None
    } else {
        Some(prefix.to_string())
    }
}

/// Returns keyword suggestions matching the prefix (case-insensitive).
///
/// # Examples
///
/// ```
/// use plsqlide::ui::completion::suggest_keywords;
///
/// let items = suggest_keywords("se", 5);
/// assert!(items.iter().any(|item| item == "SELECT"));
/// ```
pub fn suggest_keywords(prefix: &str, limit: usize) -> Vec<String> {
    let needle = prefix.to_ascii_uppercase();
    let mut out = Vec::new();
    for keyword in SQL_KEYWORDS {
        if keyword.starts_with(&needle) && *keyword != needle.as_str() {
            if !out.iter().any(|item| item == *keyword) {
                out.push((*keyword).to_string());
            }
        }
        if out.len() >= limit {
            break;
        }
    }
    out
}
