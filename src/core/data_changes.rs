//! Generates DML statements for editable result grids.
//!
//! Rows are identified by full-value match (no rowid/ctid): Postgres uses a
//! row-wide `IS NOT DISTINCT FROM` comparison, Oracle uses `DECODE`, which
//! treats NULLs as equal. This works on any SELECT over a single table;
//! duplicate rows are updated/deleted together.

use std::collections::{HashMap, HashSet};

use crate::core::db_kind::DbKind;

/// Cell text meaning SQL NULL (spikes render NULLs this way).
pub const NULL_CELL: &str = "NULL";

/// First table name after `FROM` in a SELECT, `None` when absent/subquery.
///
/// ponytail: whitespace tokenizer without quote awareness; queries with
/// "from" inside string literals can misdetect — error surfaces on post.
pub fn extract_table(sql: &str) -> Option<String> {
    let mut words = sql
        .split_whitespace()
        .map(|word| word.trim_end_matches([';', ',', ')']));
    while let Some(word) = words.next() {
        if word.eq_ignore_ascii_case("from") {
            if let Some(table) = words.next() {
                if table.starts_with('(') {
                    return None;
                }
                return Some(table.trim_end_matches(|c: char| c == ';').to_string());
            }
        }
    }
    None
}

/// String literal for a cell value: `NULL` cell → NULL keyword, else quoted.
fn lit(value: &str) -> String {
    if value == NULL_CELL {
        "NULL".to_string()
    } else {
        format!("'{}'", value.replace('\'', "''"))
    }
}

/// NULL-safe equality predicate for one original row.
fn match_predicate(kind: DbKind, columns: &[String], row: &[String]) -> String {
    if kind == DbKind::Oracle {
        let terms: Vec<String> = columns
            .iter()
            .zip(row)
            .map(|(column, value)| {
                // ponytail: empty string is NULL in Oracle, emit NULL keyword.
                let value = if value.is_empty() { NULL_CELL } else { value };
                format!("DECODE({}, {}, 0, 1) = 0", column, lit(value))
            })
            .collect();
        terms.join(" AND ")
    } else {
        let names = columns.join(", ");
        let values: Vec<String> = row.iter().map(|value| lit(value)).collect();
        format!("({names}) IS NOT DISTINCT FROM ({})", values.join(", "))
    }
}

/// SET clause for the changed cells of one row (oracle empty → NULL).
fn set_clause(
    kind: DbKind,
    columns: &[String],
    row_index: usize,
    edits: &HashMap<(usize, usize), String>,
    old_row: &[String],
) -> String {
    let mut parts = Vec::new();
    for (col, column) in columns.iter().enumerate() {
        if let Some(new_value) = edits.get(&(row_index, col)) {
            if Some(new_value) != old_row.get(col) {
                let value = if kind == DbKind::Oracle && new_value.is_empty() {
                    NULL_CELL
                } else {
                    new_value
                };
                parts.push(format!("{} = {}", column, lit(value)));
            }
        }
    }
    parts.join(", ")
}

/// DML statements applying grid edits, in insert/update/delete order.
pub fn build_statements(
    kind: DbKind,
    table: &str,
    columns: &[String],
    rows: &[Vec<String>],
    edits: &HashMap<(usize, usize), String>,
    inserted: &HashSet<usize>,
    deleted: &HashSet<usize>,
) -> Vec<String> {
    let mut statements = Vec::new();

    for &index in inserted {
        if deleted.contains(&index) {
            continue;
        }
        let values: Vec<String> = columns
            .iter()
            .enumerate()
            .map(|(col, _)| {
                let value = edits.get(&(index, col)).cloned().unwrap_or_default();
                let value = if kind == DbKind::Oracle && value.is_empty() {
                    NULL_CELL.to_string()
                } else {
                    value
                };
                lit(&value)
            })
            .collect();
        let names = columns.join(", ");
        statements.push(format!(
            "INSERT INTO {table} ({names}) VALUES ({})",
            values.join(", ")
        ));
    }

    for (index, row) in rows.iter().enumerate() {
        if inserted.contains(&index) || deleted.contains(&index) {
            continue;
        }
        let set = set_clause(kind, columns, index, edits, row);
        if !set.is_empty() {
            statements.push(format!(
                "UPDATE {table} SET {set} WHERE {}",
                match_predicate(kind, columns, row)
            ));
        }
    }

    for (index, row) in rows.iter().enumerate() {
        if deleted.contains(&index) && !inserted.contains(&index) {
            statements.push(format!(
                "DELETE FROM {table} WHERE {}",
                match_predicate(kind, columns, row)
            ));
        }
    }

    statements
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_table_finds_from() {
        assert_eq!(
            extract_table("select *\n  from hr.employees\n where id = 1"),
            Some("hr.employees".to_string())
        );
        assert_eq!(extract_table("select 1 from (select 2) t"), None);
        assert_eq!(extract_table("select 1"), None);
    }

    #[test]
    fn postgres_match_is_null_safe() {
        let predicate = match_predicate(
            DbKind::Postgres,
            &["ID".into(), "NAME".into()],
            &["1".into(), NULL_CELL.into()],
        );
        assert_eq!(
            predicate,
            "(ID, NAME) IS NOT DISTINCT FROM ('1', NULL)".to_string()
        );
    }

    #[test]
    fn oracle_edit_uses_decode_and_null() {
        let mut edits = HashMap::new();
        edits.insert((0usize, 1usize), "x".to_string());
        let statements = build_statements(
            DbKind::Oracle,
            "t",
            &["ID".into(), "NAME".into()],
            &[vec!["1".into(), String::new()]],
            &edits,
            &HashSet::new(),
            &HashSet::new(),
        );
        assert_eq!(
            statements,
            vec!["UPDATE t SET NAME = 'x' WHERE DECODE(ID, '1', 0, 1) = 0 AND DECODE(NAME, NULL, 0, 1) = 0".to_string()]
        );
    }

    #[test]
    fn insert_delete_and_skipped_noop() {
        let mut edits = HashMap::new();
        edits.insert((1usize, 0usize), "1".to_string()); // same as old: no UPDATE
        let mut inserted = HashSet::new();
        inserted.insert(2usize);
        let mut deleted = HashSet::new();
        deleted.insert(2usize); // inserted+deleted rows are skipped entirely
        deleted.insert(0usize);
        let statements = build_statements(
            DbKind::Postgres,
            "t",
            &["ID".into()],
            &[
                vec!["1".into()],
                vec!["1".into()], // same value: edit with old == new → no UPDATE
                vec![String::new()],
            ],
            &edits,
            &inserted,
            &deleted,
        );
        assert_eq!(
            statements,
            vec!["DELETE FROM t WHERE (ID) IS NOT DISTINCT FROM ('1')".to_string()]
        );
    }
}
