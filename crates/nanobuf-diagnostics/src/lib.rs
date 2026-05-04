//! Source spans and diagnostic rendering for the nanobuf compiler.
//!
//! Compiler stages report problems as [`Diagnostic`] values whose labels
//! point into a [`SourceMap`]. [`Diagnostic::render`] produces rustc-style
//! terminal output with optional ANSI color.

use std::fmt;
use std::fmt::Write;

/// Half-open byte range into one source file.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub const fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    /// Smallest span covering both inputs.
    pub fn join(self, other: Span) -> Span {
        Span::new(self.start.min(other.start), self.end.max(other.end))
    }
}

impl fmt::Debug for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}..{}", self.start, self.end)
    }
}

/// Handle to a file registered in a [`SourceMap`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct FileId(u32);

/// One source file plus a line-start index for span lookups.
pub struct SourceFile {
    name: String,
    text: String,
    line_starts: Vec<u32>,
}

impl SourceFile {
    fn new(name: String, text: String) -> Self {
        let mut line_starts = vec![0];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(i as u32 + 1);
            }
        }
        Self {
            name,
            text,
            line_starts,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// 1-based line and column of a byte offset. Columns count chars.
    pub fn line_col(&self, offset: u32) -> (u32, u32) {
        let offset = offset.min(self.text.len() as u32);
        let line = self.line_starts.partition_point(|&s| s <= offset);
        let start = self.line_starts[line - 1] as usize;
        let col = self.text[start..offset as usize].chars().count() + 1;
        (line as u32, col as u32)
    }

    /// Text of a 1-based line, without the trailing newline.
    pub fn line_text(&self, line: u32) -> &str {
        let start = self.line_starts[line as usize - 1] as usize;
        let end = self
            .line_starts
            .get(line as usize)
            .map_or(self.text.len(), |&s| s as usize);
        self.text[start..end].trim_end_matches(['\n', '\r'])
    }
}

/// Owns all source files referenced by diagnostics.
#[derive(Default)]
pub struct SourceMap {
    files: Vec<SourceFile>,
}

impl SourceMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, name: impl Into<String>, text: impl Into<String>) -> FileId {
        self.files.push(SourceFile::new(name.into(), text.into()));
        FileId(self.files.len() as u32 - 1)
    }

    pub fn file(&self, id: FileId) -> &SourceFile {
        &self.files[id.0 as usize]
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Severity {
    Error,
    Warning,
    Hint,
}

impl Severity {
    fn name(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Hint => "hint",
        }
    }

    fn color(self) -> &'static str {
        match self {
            Severity::Error => "\x1b[1;31m",
            Severity::Warning => "\x1b[1;33m",
            Severity::Hint => "\x1b[1;36m",
        }
    }
}

/// A span annotated with a message. The first label of a diagnostic is
/// primary and determines the reported location.
pub struct Label {
    pub file: FileId,
    pub span: Span,
    pub message: String,
}

pub struct Diagnostic {
    pub severity: Severity,
    pub code: &'static str,
    pub message: String,
    pub labels: Vec<Label>,
    pub notes: Vec<String>,
    pub help: Option<String>,
}

impl Diagnostic {
    pub fn new(severity: Severity, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            severity,
            code,
            message: message.into(),
            labels: Vec::new(),
            notes: Vec::new(),
            help: None,
        }
    }

    pub fn error(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(Severity::Error, code, message)
    }

    pub fn warning(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(Severity::Warning, code, message)
    }

    pub fn hint(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(Severity::Hint, code, message)
    }

    pub fn with_label(mut self, file: FileId, span: Span, message: impl Into<String>) -> Self {
        self.labels.push(Label {
            file,
            span,
            message: message.into(),
        });
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    /// Renders rustc-style terminal output, with ANSI color when `color`.
    pub fn render(&self, sources: &SourceMap, color: bool) -> String {
        Renderer { sources, color }.render(self)
    }
}

/// Ordered collection of diagnostics produced by a compiler run.
#[derive(Default)]
pub struct Diagnostics {
    items: Vec<Diagnostic>,
}

impl Diagnostics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.items.push(diagnostic);
    }

    pub fn has_errors(&self) -> bool {
        self.items.iter().any(|d| d.severity == Severity::Error)
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Diagnostic> {
        self.items.iter()
    }

    pub fn render(&self, sources: &SourceMap, color: bool) -> String {
        let mut out = String::new();
        for (i, d) in self.items.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            out.push_str(&d.render(sources, color));
        }
        out
    }
}

struct Renderer<'a> {
    sources: &'a SourceMap,
    color: bool,
}

const BOLD: &str = "\x1b[1m";
const BLUE: &str = "\x1b[1;34m";
const RESET: &str = "\x1b[0m";

impl Renderer<'_> {
    fn paint(&self, code: &str, text: &str) -> String {
        if self.color {
            format!("{code}{text}{RESET}")
        } else {
            text.to_string()
        }
    }

    fn render(&self, d: &Diagnostic) -> String {
        let mut out = String::new();
        let head = format!("{}[{}]", d.severity.name(), d.code);
        let _ = writeln!(
            out,
            "{}{} {}",
            self.paint(d.severity.color(), &head),
            self.paint(BOLD, ":"),
            self.paint(BOLD, &d.message)
        );

        let gutter = d
            .labels
            .iter()
            .map(|l| {
                let (line, _) = self.sources.file(l.file).line_col(l.span.start);
                line.to_string().len()
            })
            .max()
            .unwrap_or(1);
        let pad = " ".repeat(gutter);

        let mut prev_file = None;
        for label in &d.labels {
            let file = self.sources.file(label.file);
            let (line, col) = file.line_col(label.span.start);
            let arrow = if prev_file.is_none() { "-->" } else { "::" };
            let _ = writeln!(
                out,
                "{pad}{} {}:{line}:{col}",
                self.paint(BLUE, arrow),
                file.name()
            );
            if prev_file != Some(label.file) {
                let _ = writeln!(out, "{pad} {}", self.paint(BLUE, "|"));
            }
            prev_file = Some(label.file);

            let text = file.line_text(line);
            let _ = writeln!(
                out,
                "{} {} {text}",
                self.paint(BLUE, &format!("{line:>gutter$}")),
                self.paint(BLUE, "|")
            );

            let line_start = label.span.start - (col - 1).min(label.span.start);
            let underline_cols = file.text()[line_start as usize..label.span.start as usize]
                .chars()
                .count();
            let span_in_line = (label.span.end.min(line_start + text.len() as u32))
                .saturating_sub(label.span.start)
                .max(1) as usize;
            let carets = "^".repeat(span_in_line);
            let _ = writeln!(
                out,
                "{pad} {} {}{}",
                self.paint(BLUE, "|"),
                " ".repeat(underline_cols),
                self.paint(d.severity.color(), &format!("{carets} {}", label.message))
            );
        }

        for note in &d.notes {
            let _ = writeln!(out, "{pad} {} note: {note}", self.paint(BLUE, "="));
        }
        if let Some(help) = &d.help {
            let _ = writeln!(out, "{pad} {} help: {help}", self.paint(BLUE, "="));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map_with(text: &str) -> (SourceMap, FileId) {
        let mut sources = SourceMap::new();
        let id = sources.add("test.nb", text);
        (sources, id)
    }

    #[test]
    fn span_join_covers_both() {
        assert_eq!(Span::new(3, 7).join(Span::new(5, 12)), Span::new(3, 12));
        assert_eq!(Span::new(5, 12).join(Span::new(3, 7)), Span::new(3, 12));
    }

    #[test]
    fn span_debug_is_range() {
        assert_eq!(format!("{:?}", Span::new(3, 7)), "3..7");
    }

    #[test]
    fn line_col_basic() {
        let (sources, id) = map_with("ab\ncd\nef");
        let f = sources.file(id);
        assert_eq!(f.line_col(0), (1, 1));
        assert_eq!(f.line_col(1), (1, 2));
        assert_eq!(f.line_col(3), (2, 1));
        assert_eq!(f.line_col(4), (2, 2));
        assert_eq!(f.line_col(7), (3, 2));
    }

    #[test]
    fn line_col_at_newline_and_eof() {
        let (sources, id) = map_with("ab\ncd");
        let f = sources.file(id);
        assert_eq!(f.line_col(2), (1, 3));
        assert_eq!(f.line_col(5), (2, 3));
        assert_eq!(f.line_col(99), (2, 3));
    }

    #[test]
    fn line_col_counts_chars_not_bytes() {
        let (sources, id) = map_with("héllo");
        let f = sources.file(id);
        assert_eq!(f.line_col(3), (1, 3));
    }

    #[test]
    fn line_text_strips_newline() {
        let (sources, id) = map_with("ab\r\ncd\n");
        let f = sources.file(id);
        assert_eq!(f.line_text(1), "ab");
        assert_eq!(f.line_text(2), "cd");
    }

    #[test]
    fn empty_file() {
        let (sources, id) = map_with("");
        let f = sources.file(id);
        assert_eq!(f.line_col(0), (1, 1));
        assert_eq!(f.line_text(1), "");
    }

    #[test]
    fn render_plain() {
        let (sources, id) = map_with("message Packet {\n  pressure: f32 @3\n}\n");
        let d = Diagnostic::error("E0007", "duplicate field id @3")
            .with_label(id, Span::new(33, 35), "id @3 reused here")
            .with_note("field ids identify fields on the wire")
            .with_help("the next free id is @4");
        let rendered = d.render(&sources, false);
        let expected = "\
error[E0007]: duplicate field id @3
 --> test.nb:2:17
  |
2 |   pressure: f32 @3
  |                 ^^ id @3 reused here
  = note: field ids identify fields on the wire
  = help: the next free id is @4
";
        assert_eq!(rendered, expected);
    }

    #[test]
    fn render_secondary_label() {
        let (sources, id) = map_with("a: u32 @1\nb: u32 @1\n");
        let d = Diagnostic::error("E0007", "duplicate field id @1")
            .with_label(id, Span::new(17, 19), "reused here")
            .with_label(id, Span::new(7, 9), "first used here");
        let rendered = d.render(&sources, false);
        assert!(rendered.contains("--> test.nb:2:8"), "{rendered}");
        assert!(rendered.contains(":: test.nb:1:8"), "{rendered}");
        assert!(rendered.contains("^^ first used here"), "{rendered}");
    }

    #[test]
    fn render_clips_multiline_span_to_first_line() {
        let (sources, id) = map_with("abc\ndef\n");
        let d = Diagnostic::warning("W0001", "spans lines").with_label(id, Span::new(1, 6), "here");
        let rendered = d.render(&sources, false);
        assert!(rendered.contains("| abc"), "{rendered}");
        assert!(rendered.contains("^^ here"), "{rendered}");
        assert!(!rendered.contains("^^^"), "{rendered}");
    }

    #[test]
    fn render_zero_width_span_gets_one_caret() {
        let (sources, id) = map_with("abc\n");
        let d = Diagnostic::hint("H0001", "h").with_label(id, Span::new(2, 2), "x");
        assert!(d.render(&sources, false).contains("^ x"));
    }

    #[test]
    fn render_with_color_emits_ansi() {
        let (sources, id) = map_with("abc\n");
        let d = Diagnostic::error("E0001", "boom").with_label(id, Span::new(0, 3), "here");
        let rendered = d.render(&sources, true);
        assert!(rendered.contains("\x1b[1;31m"));
        assert!(rendered.contains("\x1b[0m"));
        assert_eq!(d.render(&sources, false).find('\x1b'), None);
    }

    #[test]
    fn render_without_labels() {
        let (sources, _) = map_with("");
        let d = Diagnostic::error("E0001", "schema statement missing");
        assert_eq!(
            d.render(&sources, false),
            "error[E0001]: schema statement missing\n"
        );
    }

    #[test]
    fn diagnostics_collection() {
        let (sources, id) = map_with("abc\n");
        let mut all = Diagnostics::new();
        assert!(all.is_empty());
        all.push(Diagnostic::warning("W0001", "w"));
        assert!(!all.has_errors());
        all.push(Diagnostic::error("E0001", "e").with_label(id, Span::new(0, 1), "x"));
        assert!(all.has_errors());
        assert_eq!(all.len(), 2);
        assert_eq!(all.iter().count(), 2);
        let rendered = all.render(&sources, false);
        assert!(rendered.contains("warning[W0001]"));
        assert!(rendered.contains("error[E0001]"));
    }

    #[test]
    fn gutter_width_follows_widest_line_number() {
        let text = format!("{}x: u32 @1\n", "\n".repeat(99));
        let (sources, id) = map_with(&text);
        let d = Diagnostic::error("E0001", "m").with_label(id, Span::new(99, 100), "here");
        let rendered = d.render(&sources, false);
        assert!(rendered.contains("100 | x: u32 @1"), "{rendered}");
    }
}
