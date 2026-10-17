//! Pane text reads and the wait-output matcher.
//!
//! Rows keep vt100's soft-wrap flag. `pane read` prints one line per row as displayed, while
//! wait-output matches logical lines, where a soft-wrapped row continues on the next row.
use crate::api::{ApiError, Pattern, ReadSource, lock};
use crate::server::{Shared, State};
use regex::Regex;
use unicode_width::UnicodeWidthStr;

/// ConPTY emits CRLF at full rows, so on Windows a full-width row is assumed to continue.
/// (An exact-width line followed by a new line is then joined too: a match superset.)
const HARD_WRAP_AT_WIDTH: bool = cfg!(windows);

/// Rows returned by `Recent` when no count is given.
const RECENT_ROWS: usize = 80;

/// One terminal row. `wrapped` means the text continues on the next row (soft wrap).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Row {
    text: String,
    wrapped: bool,
}

/// Read a pane's text. Visible = the screen as currently displayed (same view as `capture-pane`).
/// Recent = last N rows (default 80) across scrollback and screen, regardless of the view offset.
pub fn read_text(
    st: &mut State,
    pid: usize,
    source: ReadSource,
    lines: Option<usize>,
) -> Result<String, ApiError> {
    let rows = source_rows(st, pid, source, lines)?;
    Ok(finish_text(&rows))
}

/// Rows of a pane for `source`, already windowed: trailing blank rows dropped, last N kept.
fn source_rows(
    st: &mut State,
    pid: usize,
    source: ReadSource,
    lines: Option<usize>,
) -> Result<Vec<Row>, ApiError> {
    let p = st
        .panes
        .get_mut(&pid)
        .ok_or_else(|| ApiError::pane_not_found(pid))?;
    let screen = p.parser.screen_mut();
    Ok(match source {
        ReadSource::Visible => window(view_rows(screen, 0, usize::MAX), lines),
        ReadSource::Recent => {
            let want = lines.unwrap_or(RECENT_ROWS);
            window(recent_rows(screen, want), Some(want))
        }
    })
}

/// Drop trailing blank rows, then keep only the last `keep` rows.
fn window(mut rows: Vec<Row>, keep: Option<usize>) -> Vec<Row> {
    while rows.last().is_some_and(|r| r.text.trim_end().is_empty()) {
        rows.pop();
    }
    if let Some(n) = keep {
        let skip = rows.len().saturating_sub(n);
        rows.drain(..skip);
    }
    rows
}

/// Rows as lines joined by `\n`, each trimmed on the right, with a trailing newline when non-empty.
fn finish_text(rows: &[Row]) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let lines: Vec<&str> = rows.iter().map(|r| r.text.trim_end()).collect();
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// Logical lines: a soft-wrapped row is joined with the row after it. Text is not trimmed.
fn logical_lines(rows: &[Row]) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for row in rows {
        cur.push_str(&row.text);
        if !row.wrapped {
            lines.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

/// Rows `skip..` of the current view, at most `take` of them, with their soft-wrap flags.
fn view_rows(screen: &vt100::Screen, skip: usize, take: usize) -> Vec<Row> {
    let width = screen.size().1;
    screen
        .rows(0, width)
        .enumerate()
        .skip(skip)
        .take(take)
        .map(|(i, text)| {
            // ConPTY (Windows) emits an explicit CRLF at a full row instead of a soft wrap, so
            // vt100 never flags it; treat a row filled to the pane width as continuing too.
            let full =
                HARD_WRAP_AT_WIDTH && UnicodeWidthStr::width(text.as_str()) >= usize::from(width);
            let soft = u16::try_from(i).is_ok_and(|r| screen.row_wrapped(r));
            Row {
                wrapped: soft || full,
                text,
            }
        })
        .collect()
}

/// The last `want` rows across scrollback and screen, oldest first, untrimmed.
/// Scrollback is restored to its previous offset, so an active copy-mode view is undisturbed.
/// Fetches `h` extra rows so that trailing blank screen rows do not eat into `want`.
fn recent_rows(screen: &mut vt100::Screen, want: usize) -> Vec<Row> {
    let (rows, _) = screen.size();
    let h = usize::from(rows);
    if h == 0 {
        return Vec::new();
    }
    let saved = screen.scrollback();
    screen.set_scrollback(usize::MAX);
    let history = screen.scrollback();
    let total = history + h;
    let mut p = total.saturating_sub(want.saturating_add(h));
    let mut out = Vec::new();
    while p < history {
        // at offset `history - p` the first rows on screen are absolute lines p, p+1, ...
        let take = (history - p).min(h);
        screen.set_scrollback(history - p);
        out.extend(view_rows(screen, 0, take));
        p += take;
    }
    screen.set_scrollback(0);
    out.extend(view_rows(screen, p - history, usize::MAX));
    screen.set_scrollback(saved);
    out
}

/// What wait-output looks for, compiled once per request.
pub(super) enum Matcher {
    Literal(String),
    Re(Regex),
}

impl Matcher {
    pub(super) fn new(pattern: &Pattern) -> Result<Self, ApiError> {
        match pattern {
            Pattern::Match(s) => Ok(Matcher::Literal(s.clone())),
            Pattern::Regex(s) => Regex::new(s)
                .map(Matcher::Re)
                .map_err(|e| ApiError::invalid(format!("invalid regex: {e}"))),
        }
    }

    /// Tested against the untrimmed logical line, so a literal with trailing spaces can match.
    pub(super) fn is_match(&self, line: &str) -> bool {
        match self {
            Matcher::Literal(s) => line.contains(s.as_str()),
            Matcher::Re(re) => re.is_match(line),
        }
    }
}

/// Trimmed text of the first logical line matching `matcher`, read under a short lock.
pub(super) fn find_line(
    shared: &Shared,
    pid: usize,
    matcher: &Matcher,
    source: ReadSource,
    lines: Option<usize>,
) -> Result<Option<String>, ApiError> {
    let mut st = lock(shared);
    let rows = source_rows(&mut st, pid, source, lines)?;
    Ok(logical_lines(&rows)
        .into_iter()
        .find(|line| matcher.is_match(line))
        .map(|line| line.trim_end().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_rows(parser: &vt100::Parser) -> Vec<Row> {
        view_rows(parser.screen(), 0, usize::MAX)
    }

    fn wrapped_parser() -> vt100::Parser {
        let mut parser = vt100::Parser::new(5, 10, 100);
        parser.process(b"abcdefghijklmnopqrstuvwxy\r\n");
        parser
    }

    #[test]
    fn finish_text_trims_right_and_joins_rows() {
        let row = |t: &str| Row {
            text: t.into(),
            wrapped: false,
        };
        let rows = vec![row("a   "), row("b"), row(""), row("c"), row("  "), row("")];
        assert_eq!(finish_text(&window(rows.clone(), None)), "a\nb\n\nc\n");
        assert_eq!(finish_text(&window(rows, Some(1))), "c\n");
        assert_eq!(finish_text(&window(vec![row(""), row(" ")], None)), "");
        assert_eq!(finish_text(&window(vec![row("x")], Some(0))), "");
    }

    #[test]
    fn visible_read_uses_screen_rows() {
        let mut parser = vt100::Parser::new(3, 10, 100);
        parser.process(b"x\r\ny");
        let rows = text_rows(&parser);
        assert_eq!(finish_text(&window(rows.clone(), None)), "x\ny\n");
        assert_eq!(finish_text(&window(rows, Some(1))), "y\n");
    }

    #[test]
    fn recent_read_spans_scrollback_and_restores_offset() {
        let mut parser = vt100::Parser::new(3, 10, 100);
        parser.process(b"a\r\nb\r\nc\r\nd\r\ne\r\n");
        let screen = parser.screen_mut();
        screen.set_scrollback(1);
        let all = finish_text(&window(recent_rows(screen, 80), Some(80)));
        assert_eq!(all, "a\nb\nc\nd\ne\n");
        assert_eq!(screen.scrollback(), 1);
        let last2 = finish_text(&window(recent_rows(screen, 2), Some(2)));
        assert_eq!(last2, "d\ne\n");
        assert_eq!(screen.scrollback(), 1);
    }

    #[test]
    fn recent_read_on_empty_pane_is_empty() {
        let mut parser = vt100::Parser::new(4, 10, 100);
        let screen = parser.screen_mut();
        let all = window(recent_rows(screen, 80), Some(80));
        assert_eq!(finish_text(&all), "");
        let three = window(recent_rows(screen, usize::MAX), Some(3));
        assert_eq!(finish_text(&three), "");
    }

    #[test]
    fn pane_read_keeps_one_line_per_row() {
        let parser = wrapped_parser();
        let rows = window(text_rows(&parser), None);
        assert_eq!(finish_text(&rows), "abcdefghij\nklmnopqrst\nuvwxy\n");
    }

    #[test]
    fn wait_match_spans_soft_wrapped_rows() {
        let parser = wrapped_parser();
        let rows = text_rows(&parser);
        let lines = logical_lines(&window(rows.clone(), None));
        assert_eq!(lines, vec!["abcdefghijklmnopqrstuvwxy".to_string()]);
        let re = Matcher::new(&Pattern::Regex("ghij.*uvw".into())).unwrap();
        assert!(re.is_match(&lines[0]));
        assert!(rows.iter().all(|r| !re.is_match(&r.text)));
        let lit = Matcher::new(&Pattern::Match("jklm".into())).unwrap();
        assert!(lit.is_match(&lines[0]));
    }

    #[test]
    fn exact_width_line_joins_only_on_conpty() {
        let mut parser = vt100::Parser::new(3, 10, 100);
        parser.process(b"abcdefghij\r\nk\r\n");
        let lines = logical_lines(&window(text_rows(&parser), None));
        if HARD_WRAP_AT_WIDTH {
            assert_eq!(lines, vec!["abcdefghijk".to_string()]);
        } else {
            assert_eq!(lines, vec!["abcdefghij".to_string(), "k".to_string()]);
        }
    }

    #[test]
    fn recent_rows_join_wrapped_lines_across_scrollback() {
        let mut parser = vt100::Parser::new(3, 10, 100);
        parser.process(b"abcdefghijklmnopqrstuvwxy\r\nz\r\n");
        let rows = window(recent_rows(parser.screen_mut(), 80), Some(80));
        assert_eq!(
            logical_lines(&rows),
            vec!["abcdefghijklmnopqrstuvwxy".to_string(), "z".to_string()]
        );
    }

    #[test]
    fn match_sees_trailing_spaces_of_the_logical_line() {
        let mut parser = vt100::Parser::new(3, 10, 100);
        parser.process(b"foo   \r\n");
        let lines = logical_lines(&window(text_rows(&parser), None));
        assert_eq!(lines[0], "foo   ");
        let lit = Matcher::new(&Pattern::Match("foo ".into())).unwrap();
        assert!(lit.is_match(&lines[0]));
        let re = Matcher::new(&Pattern::Regex("o +$".into())).unwrap();
        assert!(re.is_match(&lines[0]));
    }

    #[test]
    fn invalid_regex_is_invalid_argument() {
        let err = Matcher::new(&Pattern::Regex("(".into())).err().unwrap();
        assert_eq!(err.code, "invalid_argument");
    }
}
