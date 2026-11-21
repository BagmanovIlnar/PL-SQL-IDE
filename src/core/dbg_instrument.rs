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
         PROCEDURE plsqlide__dbg(n NUMBER) IS \
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
           END IF; \
         END;",
        p = pipe
    )
    .replace('\n', " ")
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
/// assert!(out.contains("plsqlide__dbg(4);"));
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
    let mut injected: Vec<(usize, usize)> = Vec::new(); // (prev_idx, stmt_line)
    let mut executable_seen = false;
    for idx in 0..raw.len() {
        if first_begin.is_some() && idx >= first_begin.unwrap() {
            executable_seen = true;
        }
        if !executable_seen || idx == 0 {
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
        if injected.iter().any(|(p, _)| *p == prev) {
            continue;
        }
        injected.push((prev, idx + 1));
    }

    let stub_text = stub(pipe);
    let mut out: Vec<String> = Vec::with_capacity(raw.len());
    for (idx, line) in raw.iter().enumerate() {
        let line = line.trim_end_matches('\r');
        let calls: Vec<usize> = injected
            .iter()
            .filter(|(prev, _)| *prev == idx)
            .map(|(_, stmt)| *stmt)
            .collect();
        let mut line = line.to_string();
        for stmt in calls {
            line.push_str(&format!(" plsqlide__dbg({stmt});"));
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
        assert!(out.contains("plsqlide__dbg(4);"));
        assert!(out.contains("plsqlide__dbg(5);"));
        assert!(out.contains("plsqlide__dbg(6);"));
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
        assert!(out.contains("plsqlide__dbg(2);"));
    }

    #[test]
    fn semicolons_inside_strings_do_not_break_terminators() {
        let src = "BEGIN\n  v := 'a; b';\n  NULL;\nEND;";
        let out = instrument(src, "P");
        assert!(out.contains("plsqlide__dbg(2);"));
        assert!(out.contains("plsqlide__dbg(3);"));
        assert!(out.contains("plsqlide__dbg(4);"));
        // the string's inner semicolon must not create a phantom stop
        let line2 = out.lines().nth(1).unwrap();
        assert_eq!(line2.matches("plsqlide__dbg").count(), 1);
    }

    #[test]
    fn comments_are_ignored() {
        let src = "BEGIN\n  -- note; fake\n  NULL;\nEND;";
        let out = instrument(src, "P");
        assert!(out.contains("plsqlide__dbg(3);"));
    }
}
