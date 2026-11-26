//! Splitting a migration's SQL/CQL text into statements, shared by every backend runner.

use std::iter::Peekable;
use std::ops::Range;
use std::str::Chars;

use super::dollar_quote::{copy_to_closing_tag, take_dollar_tag};

/// Splits SQL/CQL into individual statements, respecting single/double-quoted
/// strings, dollar-quoted strings, line comments (`--`) and block comments (`/* */`).
/// Comments are stripped from the output and empty statements are dropped; each returned
/// statement keeps its trailing `;`.
pub fn split_statements(sql: &str) -> Vec<String> {
    statement_spans(sql)
        .into_iter()
        .map(|(_, statement)| statement)
        .collect()
}

/// Each statement [`split_statements`] reads in `sql`, beside its byte span: from its first
/// character after any leading whitespace or comment, through its `;`.
pub(crate) fn statement_spans(sql: &str) -> Vec<(Range<usize>, String)> {
    let mut statements = Vec::new();
    let mut current = String::new();
    let mut chars = sql.chars().peekable();
    let mut at = 0;
    let mut start = None;

    while let Some(c) = chars.next() {
        let here = at;
        match c {
            '-' if chars.peek() == Some(&'-') => at += c.len_utf8() + skip_line(&mut chars),
            '/' if chars.peek() == Some(&'*') => at += c.len_utf8() + skip_block(&mut chars),
            ';' => {
                at += c.len_utf8();
                let stmt = current.trim();
                if !stmt.is_empty() {
                    statements.push((start.unwrap_or(here)..at, format!("{stmt};")));
                }
                current.clear();
                start = None;
            }
            _ => {
                let pushed = current.len();
                read_into(c, &mut chars, &mut current);
                at += current.len() - pushed;
                if start.is_none() && !c.is_whitespace() {
                    start = Some(here);
                }
            }
        }
    }

    let stmt = current.trim();
    if let (false, Some(start)) = (stmt.is_empty(), start) {
        let end = start + sql[start..at].trim_end().len();
        statements.push((start..end, format!("{stmt};")));
    }

    statements
}

/// Skips a line comment, its first `-` read; answers the bytes skipped.
fn skip_line(chars: &mut Peekable<Chars<'_>>) -> usize {
    let mut skipped = 0;
    for c in chars.by_ref() {
        skipped += c.len_utf8();
        if c == '\n' {
            break;
        }
    }
    skipped
}

/// Skips a block comment, its `/` read; answers the bytes skipped.
fn skip_block(chars: &mut Peekable<Chars<'_>>) -> usize {
    let mut skipped = chars.next().map_or(0, char::len_utf8);
    let mut prev = '\0';
    for c in chars.by_ref() {
        skipped += c.len_utf8();
        if prev == '*' && c == '/' {
            break;
        }
        prev = c;
    }
    skipped
}

/// Copies `c` into `out`, and with it the whole of the quoted text it opens.
#[expect(clippy::excessive_nesting, reason = "readability baseline")]
fn read_into(c: char, chars: &mut Peekable<Chars<'_>>, out: &mut String) {
    match c {
        // Doubled quotes are escapes.
        '\'' | '"' => {
            out.push(c);
            while let Some(qc) = chars.next() {
                out.push(qc);
                if qc == c {
                    if chars.peek() == Some(&c) {
                        out.push(chars.next().expect("peeked"));
                    } else {
                        break;
                    }
                }
            }
        }
        // Dollar-quoted bodies hold `;` after each inner statement.
        '$' => match take_dollar_tag(chars) {
            Ok(tag) => {
                out.push_str(&tag);
                copy_to_closing_tag(chars, out, &tag);
            }
            // Not a quote (most often `$1`): tag-legal text, so it holds no separator.
            Err(text) => out.push_str(&text),
        },
        _ => out.push(c),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_on_semicolons_outside_quotes_and_comments() {
        let sql = "-- comment\nCREATE TABLE a (x TEXT); INSERT INTO a VALUES ('semi;colon');\n/* block; comment */\nDROP TABLE a";
        let stmts = split_statements(sql);
        assert_eq!(stmts.len(), 3);
        assert_eq!(stmts[0], "CREATE TABLE a (x TEXT);");
        assert_eq!(stmts[1], "INSERT INTO a VALUES ('semi;colon');");
        assert_eq!(stmts[2], "DROP TABLE a;");
    }

    #[test]
    fn each_statement_stands_in_its_own_span_and_a_comment_before_it_in_none() {
        let sql = "-- Ä; quoted\nA;\n/* B; é */ B 'x;ÿ';\n  C ";
        let spans = statement_spans(sql);
        let texts: Vec<_> = spans.iter().map(|(span, _)| &sql[span.clone()]).collect();
        assert_eq!(texts, ["A;", "B 'x;ÿ';", "C"]);
        assert_eq!(spans[2].1, "C;");
    }

    #[test]
    fn doubled_quotes_are_escapes_not_terminators() {
        let stmts = split_statements("INSERT INTO a VALUES ('it''s; fine');");
        assert_eq!(stmts.len(), 1);
        assert_eq!(stmts[0], "INSERT INTO a VALUES ('it''s; fine');");
    }

    #[test]
    fn empty_input_yields_no_statements() {
        assert!(split_statements("   \n-- only a comment\n").is_empty());
    }

    #[test]
    fn a_dollar_quoted_body_is_one_statement_and_not_its_semicolons() {
        let sql = "CREATE FUNCTION f() RETURNS void AS $$ \
                   BEGIN RAISE NOTICE 'x'; RAISE NOTICE 'y'; END $$ LANGUAGE plpgsql;";
        let stmts = split_statements(sql);
        assert_eq!(stmts.len(), 1, "shredded into {stmts:#?}");
        assert!(
            stmts[0].contains("RAISE NOTICE 'y'"),
            "the whole body must survive"
        );
    }

    #[test]
    fn a_named_tag_closes_only_on_itself() {
        let sql = "CREATE FUNCTION f() RETURNS void AS $body$ \
                   BEGIN PERFORM $$inner; text$$; END $body$ LANGUAGE plpgsql; SELECT 1;";
        let stmts = split_statements(sql);
        assert_eq!(stmts.len(), 2, "shredded into {stmts:#?}");
        assert!(
            stmts[0].contains("$$inner; text$$"),
            "a different tag must not close it"
        );
        assert_eq!(stmts[1], "SELECT 1;");
    }

    #[test]
    fn a_bind_placeholder_is_not_a_quote() {
        // `$1` opens nothing.
        let stmts = split_statements("INSERT INTO t (a, b) VALUES ($1, $2); SELECT 1;");
        assert_eq!(stmts.len(), 2, "shredded into {stmts:#?}");
        assert_eq!(stmts[0], "INSERT INTO t (a, b) VALUES ($1, $2);");
    }

    #[test]
    fn a_semicolon_inside_a_dollar_quoted_literal_is_not_a_boundary() {
        let stmts = split_statements("INSERT INTO t VALUES ($tag$a;b$tag$); SELECT 1;");
        assert_eq!(stmts.len(), 2, "shredded into {stmts:#?}");
        assert_eq!(stmts[0], "INSERT INTO t VALUES ($tag$a;b$tag$);");
    }

    #[test]
    fn an_unterminated_dollar_quote_is_the_engines_to_refuse_and_not_this_functions() {
        // Unterminated: the engine reports it, the splitter does not guess an end.
        let stmts = split_statements("SELECT $$ never closed; SELECT 1;");
        assert_eq!(stmts.len(), 1, "{stmts:#?}");
    }
}
