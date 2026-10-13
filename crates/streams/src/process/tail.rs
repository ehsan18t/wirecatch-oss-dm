//! The end of what a program wrote to standard error: kept for its error message and the log,
//! bounded, and with every secret hidden.

#![forbid(unsafe_code)]

use std::collections::VecDeque;
use std::fmt;

use wirecatch_engine::units::KIB_USIZE;
use wirecatch_logging::redact;

use super::NO_REASON;

/// The most of standard error a run keeps, in bytes: the last lines that fit. A line longer than
/// this is cut to it.
pub const STDERR_TAIL_BYTES: usize = 8 * KIB_USIZE;

/// The most characters of a program's own words put in a message for a person: its error line can
/// be as long as a site makes it, and the whole tail is in the log.
pub const MAX_REASON_CHARS: usize = 300;

/// The last lines a program wrote to standard error, at most [`STDERR_TAIL_BYTES`] in all, each
/// with its secrets hidden ([`redact::text_for_log`]): ffmpeg and yt-dlp name the links they read,
/// and a signed link carries its token.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StderrTail {
    lines: VecDeque<String>,
    bytes: usize,
}

impl StderrTail {
    /// Keeps `line` as the newest, dropping the oldest lines that no longer fit.
    pub(crate) fn push(&mut self, line: &str) {
        let line = cut_bytes(&redact::text_for_log(line), STDERR_TAIL_BYTES);
        self.bytes += line.len();
        self.lines.push_back(line);
        while self.bytes > STDERR_TAIL_BYTES
            && let Some(oldest) = self.lines.pop_front()
        {
            self.bytes -= oldest.len();
        }
    }

    /// The lines kept, oldest first.
    pub fn lines(&self) -> impl ExactSizeIterator<Item = &str> {
        self.lines.iter().map(String::as_str)
    }

    /// How many bytes the lines kept hold.
    #[must_use]
    pub fn len_bytes(&self) -> usize {
        self.bytes
    }

    /// The most telling line: the last one naming an error, else the last line.
    #[must_use]
    pub fn error_line(&self) -> Option<&str> {
        self.lines
            .iter()
            .rev()
            .find(|l| l.contains("ERROR"))
            .or_else(|| self.lines.back())
            .map(|l| l.trim())
    }

    /// The last `count` lines that say something, joined with ` / ` and cut to
    /// [`MAX_REASON_CHARS`], for a person; [`NO_REASON`] when there are none.
    #[must_use]
    pub fn summary(&self, count: usize) -> String {
        let said: Vec<&str> = self
            .lines
            .iter()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .collect();
        let last = said[said.len().saturating_sub(count)..].join(" / ");
        if last.is_empty() {
            NO_REASON.into()
        } else {
            for_a_person(&last)
        }
    }
}

/// Every line kept, joined with ` / ` on one line, for the log.
impl fmt::Display for StderrTail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, line) in self.lines.iter().enumerate() {
            if i > 0 {
                f.write_str(" / ")?;
            }
            f.write_str(line)?;
        }
        Ok(())
    }
}

/// `text` cut to at most [`MAX_REASON_CHARS`] characters, ending in `…` when it was longer.
#[must_use]
pub fn for_a_person(text: &str) -> String {
    if text.chars().nth(MAX_REASON_CHARS).is_none() {
        return text.to_string();
    }
    let end = text
        .char_indices()
        .nth(MAX_REASON_CHARS - 1)
        .map_or(text.len(), |(end, _)| end);
    format!("{}…", text[..end].trim_end())
}

/// `text` cut to at most `limit` bytes at a character boundary.
fn cut_bytes(text: &str, limit: usize) -> String {
    let mut end = text.len().min(limit);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_newest_lines_that_fit_are_kept() {
        let mut tail = StderrTail::default();
        let line = "x".repeat(1000);
        for _ in 0..100 {
            tail.push(&line);
        }
        tail.push("ERROR: the last word");
        assert!(tail.len_bytes() <= STDERR_TAIL_BYTES);
        let last = "ERROR: the last word".len();
        assert_eq!(tail.lines().len(), (STDERR_TAIL_BYTES - last) / 1000 + 1);
        assert_eq!(tail.error_line(), Some("ERROR: the last word"));
    }

    #[test]
    fn a_line_longer_than_the_tail_is_cut_at_a_character() {
        let mut tail = StderrTail::default();
        tail.push(&"€".repeat(STDERR_TAIL_BYTES));
        assert!(tail.len_bytes() <= STDERR_TAIL_BYTES);
        assert_eq!(tail.lines().len(), 1);
    }

    #[test]
    fn secrets_are_hidden_as_lines_are_kept() {
        let mut tail = StderrTail::default();
        tail.push(
            "[https @ 0x1] Opening 'https://ada:pw@cdn.example.com/v.m3u8?token=T0K' for reading",
        );
        tail.push("ERROR: HTTP Error 403, sent Cookie: SID=C00KIE");
        let text = tail.to_string();
        for secret in [":pw@", "T0K", "C00KIE"] {
            assert!(!text.contains(secret), "{text}");
        }
        assert!(text.contains("cdn.example.com/v.m3u8"), "{text}");
    }

    #[test]
    fn a_reason_for_a_person_is_short() {
        let long = "word ".repeat(200);
        let reason = for_a_person(&long);
        assert!(reason.chars().count() <= MAX_REASON_CHARS, "{reason}");
        assert!(reason.ends_with('…'));
        assert_eq!(for_a_person("short"), "short");

        let mut tail = StderrTail::default();
        assert_eq!(tail.summary(3), NO_REASON);
        for n in 0..5 {
            tail.push(&format!("line {n}"));
        }
        tail.push("");
        assert_eq!(tail.summary(2), "line 3 / line 4");
    }
}
