//! Rewriting of SQLx `?` parameter markers into the `@Pn` names used by
//! `sp_executesql`.
//!
//! SQLx builds statements with ODBC-style positional `?` markers, but
//! `mssql-tds` binds RPC parameters by name and derives the declared parameter
//! list from those names. Every marker therefore has to be rewritten to `@P1`,
//! `@P2`, ... in source order before execution.
//!
//! The scan is literal-aware: a `?` inside a string literal, a quoted
//! identifier or a comment is data, not a marker, and must be left alone.

/// Rewrites positional `?` markers into `@P1`-style names.
///
/// Returns the rewritten SQL and the number of markers replaced.
pub(crate) fn rewrite_param_markers(sql: &str) -> (String, usize) {
    let mut output = String::with_capacity(sql.len());
    let mut markers = 0usize;
    let mut index = 0usize;

    while index < sql.len() {
        let ch = next_char(sql, index);

        match ch {
            // `--` line comment: copied through to the end of the line.
            '-' if sql[index..].starts_with("--") => {
                let end = sql[index..]
                    .find('\n')
                    .map_or(sql.len(), |offset| index + offset);
                output.push_str(&sql[index..end]);
                index = end;
            }

            // `/* ... */` block comment.
            '/' if sql[index..].starts_with("/*") => {
                let end = sql[index + 2..]
                    .find("*/")
                    .map_or(sql.len(), |offset| index + 2 + offset + 2);
                output.push_str(&sql[index..end]);
                index = end;
            }

            // String literal: `''` escapes a quote.
            '\'' => copy_quoted(sql, &mut output, &mut index, '\''),

            // Quoted identifier: `""` escapes a quote.
            '"' => copy_quoted(sql, &mut output, &mut index, '"'),

            // Bracketed identifier: `]]` escapes a bracket.
            '[' => copy_quoted(sql, &mut output, &mut index, ']'),

            // A marker.
            '?' => {
                markers += 1;
                output.push_str("@P");
                output.push_str(&markers.to_string());
                index += 1;
            }

            other => {
                output.push(other);
                index += other.len_utf8();
            }
        }
    }

    (output, markers)
}

/// Returns the character starting at `index`, which must be a char boundary.
fn next_char(sql: &str, index: usize) -> char {
    sql[index..].chars().next().unwrap_or('\u{fffd}')
}

/// Copies a quoted region verbatim, including its delimiters.
///
/// `closing` ends the region; doubling it escapes it. Characters are copied
/// whole so that multi-byte text inside a literal survives intact.
fn copy_quoted(sql: &str, output: &mut String, index: &mut usize, closing: char) {
    let opening = next_char(sql, *index);
    output.push(opening);
    *index += opening.len_utf8();

    while *index < sql.len() {
        let ch = next_char(sql, *index);

        if ch == closing {
            let after = *index + ch.len_utf8();

            // A doubled delimiter is an escaped character, not the end.
            if sql[after..].starts_with(closing) {
                output.push(closing);
                output.push(closing);
                *index = after + closing.len_utf8();
                continue;
            }

            output.push(closing);
            *index = after;
            break;
        }

        output.push(ch);
        *index += ch.len_utf8();
    }
}

#[cfg(test)]
mod tests {
    use super::rewrite_param_markers;

    #[test]
    fn rewrites_markers_in_source_order() {
        let (sql, count) = rewrite_param_markers("SELECT ? , ?");

        assert_eq!(sql, "SELECT @P1 , @P2");
        assert_eq!(count, 2);
    }

    #[test]
    fn leaves_question_mark_inside_string_literal() {
        let (sql, count) = rewrite_param_markers("WHERE a = ? AND b = 'x?y'");

        assert_eq!(sql, "WHERE a = @P1 AND b = 'x?y'");
        assert_eq!(count, 1);
    }

    #[test]
    fn handles_escaped_quote_in_literal() {
        let (sql, count) = rewrite_param_markers("WHERE a = '''?''' AND b = ?");

        assert_eq!(sql, "WHERE a = '''?''' AND b = @P1");
        assert_eq!(count, 1);
    }

    #[test]
    fn skips_line_and_block_comments() {
        let (sql, count) = rewrite_param_markers("SELECT ? -- ?\n/* ? */ , ?");

        assert_eq!(sql, "SELECT @P1 -- ?\n/* ? */ , @P2");
        assert_eq!(count, 2);
    }

    #[test]
    fn skips_quoted_and_bracketed_identifiers() {
        let (sql, count) = rewrite_param_markers(r#"SELECT "a?b", [c?d], ? FROM t"#);

        assert_eq!(sql, r#"SELECT "a?b", [c?d], @P1 FROM t"#);
        assert_eq!(count, 1);
    }

    #[test]
    fn supports_question_equal_pattern() {
        let (sql, count) = rewrite_param_markers("?= EXEC dbo.p ?");

        assert_eq!(sql, "@P1= EXEC dbo.p @P2");
        assert_eq!(count, 2);
    }

    #[test]
    fn preserves_multibyte_text() {
        let (sql, count) = rewrite_param_markers("SELECT 'héllo wörld', ?");

        assert_eq!(sql, "SELECT 'héllo wörld', @P1");
        assert_eq!(count, 1);
    }

    #[test]
    fn passes_through_statements_without_markers() {
        let (sql, count) = rewrite_param_markers("SELECT 1");

        assert_eq!(sql, "SELECT 1");
        assert_eq!(count, 0);
    }
}
