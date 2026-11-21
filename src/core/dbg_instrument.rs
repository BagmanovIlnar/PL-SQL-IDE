//! Local (instrumented) debugging of anonymous PL/SQL blocks.
//!
//! The IDE rewrites the block so every executable statement reports its line
//! through `DBMS_PIPE` to the IDE and waits for a resume command. This gives
//! real breakpoints and stepping without any server debugger (no Probe,
//! no JDWP, no extra privileges, no DDL).
//!
//! Line numbers are preserved: the helper is appended to the DECLARE line and
//! the `plsqlide__dbg(k)` calls are appended to the END of the previous line
//! (k = the statement's own line number), so the editor line numbers match.

/// One-line debug stub injected into the block's DECLARE section.
/// `PLSQLIDE__MODE` is the enclosing block's local variable; it carries the
/// resume mode between calls: `STEP`, `STOP` or `RUN:<line>,<line>,…`.
fn stub(pipe: &str) -> String {
    format!(
        "plsqlide__mode VARCHAR2(300) := 'STEP'; \
         FUNCTION plsqlide__dbg(n NUMBER) RETURN NUMBER IS \
           st INTEGER; \
         BEGIN \
           IF SYS.DBMS_PIPE.RECEIVE_MESSAGE('{p}_C', 0) = 0 THEN \
             SYS.DBMS_PIPE.UNPACK_MESSAGE(plsqlide__mode); \
           END IF; \
           IF plsqlide__mode = 'STOP' THEN \
             RAISE_APPLICATION_ERROR(-20999, 'PLSQLIDE: debug stopped'); \
           END IF; \
           IF plsqlide__mode = 'STEP' \
              OR INSTR(',' || REPLACE(plsqlide__mode, 'RUN:', ',') || ',', ',' || n || ',') > 0 \
           THEN \
             SYS.DBMS_PIPE.PACK_MESSAGE('PAUSE ' || n); \
             st := SYS.DBMS_PIPE.SEND_MESSAGE('{p}_E'); \
             LOOP \
               EXIT WHEN SYS.DBMS_PIPE.RECEIVE_MESSAGE('{p}_C', 1) = 0; \
             END LOOP; \
             SYS.DBMS_PIPE.UNPACK_MESSAGE(plsqlide__mode); \
             IF plsqlide__mode = 'STOP' THEN \
               RAISE_APPLICATION_ERROR(-20999, 'PLSQLIDE: debug stopped'); \
             END IF; \
             RETURN 1; \
           END IF; \
           RETURN 0; \
         END;",
        p = pipe
    )
    .replace('\n', " ")
}

/// Top-level scalar declarations of the block: (NAME, value expression).
/// The stub can't see outer locals, so values are packed at call sites —
/// ponytail: naive ';'-statement scanner; nested procedures are skipped by
/// first word, exotic declarations (records, collections, cursors) are
/// simply not watched.
fn declared_vars(stripped: &[String]) -> Vec<(String, String)> {
    const TYPES: &[&str] = &[
        "NUMBER", "VARCHAR2", "VARCHAR", "CHAR", "NCHAR", "NVARCHAR2", "DATE", "BOOLEAN",
        "PLS_INTEGER", "BINARY_INTEGER", "BINARY_DOUBLE", "INTEGER", "INT", "SMALLINT",
        "FLOAT", "REAL", "TIMESTAMP",
    ];
    let section: String = stripped.join(" ");
    let mut vars = Vec::new();
    let mut skip = 0u32;
    for stmt in section.split(';') {
        let tokens: Vec<String> = stmt
            .split(|ch: char| !ch.is_alphanumeric() && ch != '_')
            .filter(|t| !t.is_empty())
            .map(|t| t.to_ascii_uppercase())
            .collect();
        let first = tokens.first().map(String::as_str).unwrap_or("");
        if first == "PROCEDURE" || first == "FUNCTION" {
            skip = 1;
            continue;
        }
        if skip > 0 {
            if first == "END" && !matches!(tokens.get(1).map(String::as_str), Some("IF") | Some("LOOP") | Some("CASE")) {
                skip = 0;
            }
            continue;
        }
        // Keywords without a ';' terminator glue onto the next declaration:
        // "BEGIN DECLARE v_x NUMBER" — drop leading BEGIN/DECLARE keywords.
        let tokens: &[String] = match (first, tokens.get(1).map(String::as_str)) {
            ("DECLARE", _) => &tokens[1..],
            ("BEGIN", Some("DECLARE")) => &tokens[2..],
            ("BEGIN", _) => &tokens[1..],
            _ => &tokens,
        };
        let (Some(name), Some(kind)) = (tokens.first(), tokens.get(1)) else {
            continue;
        };
        if name.starts_with("PLSQLIDE_") || !TYPES.contains(&kind.as_str()) {
            continue;
        }
        let expr = match kind.as_str() {
            "BOOLEAN" => format!(
                "CASE WHEN {name} IS NULL THEN 'NULL' WHEN {name} THEN 'TRUE' ELSE 'FALSE' END"
            ),
            "DATE" | "TIMESTAMP" => {
                format!("NVL(TO_CHAR({name},'DD.MM.YYYY HH24:MI:SS'),'NULL')")
            }
            _ => format!("NVL(SUBSTR(TO_CHAR({name}),1,120),'NULL')"),
        };
        vars.push((name.clone(), expr));
    }
    vars
}

/// Debug helper: declared scalar variables of the source block.
pub fn declared_vars_public(source: &str) -> Vec<(String, String)> {
    let raw: Vec<&str> = source.lines().collect();
    let mut stripped: Vec<String> = Vec::with_capacity(raw.len());
    let (mut in_string, mut in_block) = (false, false);
    for line in &raw {
        stripped.push(strip_line(line, &mut in_string, &mut in_block));
    }
    declared_vars(&stripped)
}

/// Pipe packs for the declared variables, sent as a separate message when
/// the pause actually happened (the stub returns 1).
fn var_packs(vars: &[(String, String)], pipe: &str) -> String {
    if vars.is_empty() {
        return String::new();
    }
    // ponytail: one item with CHR(10) separators — this 11.2 build stops
    // reporting items after the first UNPACK, so multi-item messages lose data.
    let mut big = String::new();
    for (name, expr) in vars {
        if !big.is_empty() {
            big.push_str(" || CHR(10) || ");
        }
        big.push_str(&format!("'V {name}=' || {expr}"));
    }
    // ponytail: send failures (huge lists) are swallowed — vars are a
    // convenience view, not a transactional feature.
    format!(
        " DECLARE st INTEGER; m VARCHAR2(32767); BEGIN m := {big}; \
         SYS.DBMS_PIPE.PACK_MESSAGE(m); \
         st := SYS.DBMS_PIPE.SEND_MESSAGE('{p}_E'); \
         EXCEPTION WHEN OTHERS THEN NULL; END;",
        p = pipe
    )
}

/// A line of PL/SQL reduced for keyword/terminator analysis: comments removed,
/// string contents collapsed to a single quote, trailing whitespace trimmed.
/// `in_string`/`in_block` carry the multiline states.
fn strip_line(line: &str, in_string: &mut bool, in_block: &mut bool) -> String {
    let mut out = String::new();
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if *in_block {
            if ch == '*' && chars.peek() == Some(&'/') {
                chars.next();
                *in_block = false;
            }
            continue;
        }
        if *in_string {
            match ch {
                '\'' => {
                    if chars.peek() == Some(&'\'') {
                        chars.next();
                    } else {
                        *in_string = false;
                    }
                }
                _ => {}
            }
            continue;
        }
        match ch {
            '-' if chars.peek() == Some(&'-') => break, // line comment
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                *in_block = true;
            }
            '\'' => {
                *in_string = true;
                out.push('\'');
                out.push('S');
            }
            _ => out.push(ch),
        }
    }
    out.trim_end().to_string()
}

fn last_word(stripped: &str) -> Option<String> {
    stripped
        .rsplit(|ch: char| !ch.is_alphanumeric() && ch != '_')
        .find(|word| !word.is_empty())
        .map(|word| word.to_ascii_uppercase())
}

/// Instrument an anonymous PL/SQL block. The result has the same number of
/// lines as the input; statements get `plsqlide__dbg(<line>);` calls.
///
/// ```
/// use plsqlide::core::dbg_instrument::instrument;
/// let src = "DECLARE\n  v_x NUMBER;\nBEGIN\n  NULL;\nEND;";
/// let out = instrument(src, "P");
/// assert!(out.contains("IF plsqlide__dbg(4) = 1 THEN"));
/// assert_eq!(out.lines().count(), src.lines().count());
/// ```
pub fn instrument(source: &str, pipe: &str) -> String {
    let raw: Vec<&str> = source.lines().collect();
    let mut stripped: Vec<String> = Vec::with_capacity(raw.len());
    let (mut in_string, mut in_block) = (false, false);
    for line in &raw {
        stripped.push(strip_line(line, &mut in_string, &mut in_block));
    }

    let mut declare_line: Option<usize> = None;
    let mut first_begin: Option<usize> = None;
    for (idx, code) in stripped.iter().enumerate() {
        let upper = code.trim().to_ascii_uppercase();
        let first_word = upper
            .split(|ch: char| !ch.is_alphanumeric() && ch != '_')
            .find(|word| !word.is_empty());
        if declare_line.is_none() && first_word == Some("DECLARE") {
            declare_line = Some(idx);
        }
        if first_begin.is_none() && first_word == Some("BEGIN") {
            first_begin = Some(idx);
        }
    }

    // The stub must be the LAST declaration, immediately before BEGIN: on
    // this Oracle build a nested procedure may not be followed by further
    // declarations, so the stub goes at the end of the previous code line.
    // ponytail: a trailing comment on that line would swallow the stub (rare).
    let stub_line: Option<usize> = match first_begin {
        None => declare_line,
        Some(0) => None, // BEGIN on the first line — stub prefixes that line
        Some(begin) => {
            let mut line = begin;
            let found = loop {
                line -= 1;
                if !stripped[line].trim().is_empty() {
                    break Some(line);
                }
                if line == 0 {
                    break None;
                }
            };
            match found {
                Some(line) if declare_line.is_some() => Some(line),
                _ => declare_line, // no declarations — stub after DECLARE
            }
        }
    };

    // Injection happens appended to the previous code line (never inside the
    // DECLARE section, i.e. only after some BEGIN has been seen).
    let mut injected: Vec<(usize, usize, Vec<(String, String)>)> =
        Vec::new(); // (prev_idx, stmt_line, visible vars)
    let mut executable_seen = false;
    // Any DECLARE section (outer block, wrapper block, nested procedure)
    // holds declarations — never inject there.
    let mut in_declare = false;
    // Visibility stack: vars collected in the DECLARE section are visible
    // from that section's BEGIN until the block's END.
    let mut scope_vars: Vec<Vec<(String, String)>> = Vec::new();
    let mut decl_buf: Vec<(String, String)> = Vec::new();
    for idx in 0..raw.len() {
        let upper = stripped[idx].trim().to_ascii_uppercase();
        let first_word = upper
            .split(|ch: char| !ch.is_alphanumeric() && ch != '_')
            .find(|word| !word.is_empty());
        match first_word {
            Some("DECLARE") => {
                in_declare = true;
                decl_buf = Vec::new();
            }
            Some("BEGIN") => {
                if in_declare {
                    scope_vars.push(std::mem::take(&mut decl_buf));
                }
                in_declare = false;
                executable_seen = true;
            }
            _ => {}
        }
        if in_declare {
            decl_buf.extend(declared_vars(
                stripped[idx..=idx.min(stripped.len() - 1)].to_vec().as_slice(),
            ));
        }
        if last_word(&stripped[idx]).as_deref() == Some("END") {
            scope_vars.pop();
        }
        if !executable_seen || in_declare || idx == 0 {
            continue;
        }
        // find previous code line
        let mut prev = idx;
        let prev_code = loop {
            prev -= 1;
            if !stripped[prev].trim().is_empty() {
                break stripped[prev].as_str();
            }
            if prev == 0 {
                break "";
            }
        };
        let last = last_word(prev_code).unwrap_or_default();
        let terminator =
            prev_code.ends_with(';') || matches!(last.as_str(), "BEGIN" | "THEN" | "ELSE" | "LOOP");
        if !terminator {
            continue;
        }
        // the statement line itself must carry code
        if stripped[idx].trim().is_empty() {
            continue;
        }
        // a block's own BEGIN is not a statement to stop on
        if stripped[idx]
            .trim()
            .to_ascii_uppercase()
            .starts_with("BEGIN")
        {
            continue;
        }
        // don't inject twice on the same physical line
        if injected.iter().any(|(p, _, _)| *p == prev) {
            continue;
        }
        let vars_here = scope_vars.last().cloned().unwrap_or_default();
        injected.push((prev, idx + 1, vars_here));
    }

    let stub_text = stub(pipe);
    let mut out: Vec<String> = Vec::with_capacity(raw.len());
    for (idx, line) in raw.iter().enumerate() {
        let line = line.trim_end_matches('\r');
        let calls: Vec<(usize, &Vec<(String, String)>)> = injected
            .iter()
            .filter(|(prev, _, _)| *prev == idx)
            .map(|(_, stmt, vars)| (*stmt, vars))
            .collect();
        let mut line = line.to_string();
        for (stmt, vars) in &calls {
            let packs = var_packs(vars, pipe);
            let body = if packs.is_empty() { " NULL;" } else { &packs };
            line.push_str(&format!(
                " IF plsqlide__dbg({stmt}) = 1 THEN{body} END IF;"
            ));
        }
        if Some(idx) == stub_line && first_begin.is_some() {
            line.push(' ');
            line.push_str(&stub_text);
        } else if first_begin == Some(idx) && stub_line.is_none() {
            line.insert_str(0, &format!("DECLARE {stub_text} "));
        }
        out.push(line);
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::instrument;

    #[test]
    fn preserves_line_count_and_injects_statements() {
        let src = "DECLARE\n  v_x NUMBER;\nBEGIN\n  NULL;\n  v_x := 1;\nEND;";
        let out = instrument(src, "P");
        assert_eq!(out.lines().count(), src.lines().count());
        assert!(out.contains("IF plsqlide__dbg(4) = 1 THEN"));
        assert!(out.contains("IF plsqlide__dbg(5) = 1 THEN"));
        assert!(out.contains("IF plsqlide__dbg(6) = 1 THEN"));
        // variable values are packed at call sites and sent as a message
        assert!(out.contains("m := 'V V_X='"));
        // stub is the last declaration, right before BEGIN
        assert!(out.lines().nth(1).unwrap().trim_end().ends_with("END;"));
    }

    #[test]
    fn block_without_declare_gets_helper_on_begin_line() {
        let src = "BEGIN\n  NULL;\nEND;";
        let out = instrument(src, "P");
        assert!(out
            .lines()
            .next()
            .unwrap()
            .starts_with("DECLARE plsqlide__mode"));
        assert!(out.contains("IF plsqlide__dbg(2) = 1 THEN"));
    }

    #[test]
    fn semicolons_inside_strings_do_not_break_terminators() {
        let src = "BEGIN\n  v := 'a; b';\n  NULL;\nEND;";
        let out = instrument(src, "P");
        assert!(out.contains("IF plsqlide__dbg(2) = 1 THEN"));
        assert!(out.contains("IF plsqlide__dbg(3) = 1 THEN"));
        assert!(out.contains("IF plsqlide__dbg(4) = 1 THEN"));
        // the string's inner semicolon must not create a phantom stop
        let line2 = out.lines().nth(1).unwrap();
        assert_eq!(line2.matches("plsqlide__dbg").count(), 1);
    }

    #[test]
    fn comments_are_ignored() {
        let src = "BEGIN\n  -- note; fake\n  NULL;\nEND;";
        let out = instrument(src, "P");
        assert!(out.contains("IF plsqlide__dbg(3) = 1 THEN"));
    }
}
