//! Compiler diagnostic implementation.
//!
//! This crate provides interfaces for declaring compiler diagnostics in three flavors:
//! + warnings
//! + errors
//! + notes.
//!
//! It also provides an API for converting the representation of a diagnostic into text.

use std::{
    fmt::Display,
    io::{self, Write},
    sync::{Arc, Mutex, MutexGuard},
};

use source::{SourceFile, SourceIdx, SourceMap};
use span::Span;

/// The diagnostic level.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DiagnosticLevel {
    /// A warning.
    Warn,
    /// An error.
    Error,
    /// A note.
    Note,
}

impl Display for DiagnosticLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::Error => "error",
                Self::Warn => "warning",
                Self::Note => "note",
            }
        )
    }
}

/// A diagnostic.
#[derive(Clone, Debug)]
pub struct Diagnostic {
    /// The span of the diagnostic.
    pub span: Option<Span>,
    /// The secondary span and the file it is located in, if any other context should be attached.
    pub secondary_span: Option<(SourceIdx, Span)>,
    /// The severity of this diagnostic.
    pub level: DiagnosticLevel,
    /// The message to display.
    pub message: String,
    /// A suggestion, if given.
    pub suggestion: Option<String>,
    /// The file in which the diagnostic was triggered.
    pub source_idx: SourceIdx,
}

impl Diagnostic {
    /// Create a warning diagnostic.
    pub fn warn(
        span: impl Into<Option<Span>>,
        message: impl Into<String>,
        suggestion: Option<String>,
        source_idx: SourceIdx,
    ) -> Self {
        Self::new(
            span.into(),
            DiagnosticLevel::Warn,
            message.into(),
            suggestion,
            source_idx,
        )
    }

    /// Create an error diagnostic.
    pub fn error(
        span: impl Into<Option<Span>>,
        message: impl Into<String>,
        suggestion: Option<String>,
        source_idx: SourceIdx,
    ) -> Self {
        Self::new(
            span.into(),
            DiagnosticLevel::Error,
            message.into(),
            suggestion,
            source_idx,
        )
    }

    /// Create a new diagnostic.
    pub const fn new(
        span: Option<Span>,
        level: DiagnosticLevel,
        message: String,
        suggestion: Option<String>,
        source_idx: SourceIdx,
    ) -> Self {
        Self {
            secondary_span: None,
            span,
            suggestion,
            message,
            level,
            source_idx,
        }
    }

    /// Attach a secondary span located in the same file to this diagnostic.
    pub fn with_secondary(self, span: Span) -> Self {
        let source = self.source_idx;
        self.with_secondary_in(source, span)
    }

    /// Attach a secondary span located in the given file to this diagnostic.
    pub fn with_secondary_in(self, source: SourceIdx, span: Span) -> Self {
        Self {
            secondary_span: Some((source, span)),
            ..self
        }
    }
}

/// A diagnostics pool.
///
/// This is a shared handle: clones refer to the same pool.
#[derive(Clone, Debug, Default)]
pub struct Diagnostics {
    diagnostics: Arc<Mutex<Vec<Diagnostic>>>,
}

impl Diagnostics {
    fn lock(&self) -> MutexGuard<'_, Vec<Diagnostic>> {
        self.diagnostics
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    /// Add a diagnostic to this pool.
    pub fn push(&self, diagnostic: Diagnostic) {
        self.lock().push(diagnostic)
    }

    /// Remove all diagnostics after a certain index.
    pub fn cull(&self, from: usize) {
        let mut diagnostics = self.lock();
        if from < diagnostics.len() {
            diagnostics.truncate(from);
        }
    }

    /// Retrieve the number of diagnostics in the pool.
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    /// Check whether this contains any diagnostics.
    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }

    /// Retrieve the number of error diagnostics in the pool.
    pub fn error_count(&self) -> usize {
        self.lock()
            .iter()
            .filter(|x| x.level == DiagnosticLevel::Error)
            .count()
    }

    /// Write all the diagnostics in this pool to a sink. This is commonly something like `stderr`.
    pub fn write(&self, map: &SourceMap, sink: &mut dyn Write) -> io::Result<()> {
        for diagnostic in &*self.lock() {
            write_diagnostic(diagnostic, map, sink)?;
        }

        Ok(())
    }

    /// A shorthand for `self.write(map, &mut io::stderr().lock())`.
    pub fn write_stderr(&self, map: &SourceMap) -> io::Result<()> {
        self.write(map, &mut io::stderr().lock())
    }

    /// A shorthand for `self.write(map, &mut io::stdout().lock())`.
    pub fn write_stdout(&self, map: &SourceMap) -> io::Result<()> {
        self.write(map, &mut io::stdout().lock())
    }
}

fn file_name(file: &SourceFile) -> impl Display + '_ {
    struct Name<'a>(&'a SourceFile);

    impl Display for Name<'_> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let cwd = std::env::current_dir().unwrap();

            match self.0.source() {
                Some(path) => write!(f, "{}", path.strip_prefix(cwd).unwrap_or(path).display()),
                None => write!(f, "<memory>"),
            }
        }
    }

    Name(file)
}

fn write_diagnostic(
    diagnostic: &Diagnostic,
    map: &SourceMap,
    sink: &mut dyn Write,
) -> io::Result<()> {
    writeln!(sink, "{}: {}", diagnostic.level, diagnostic.message)?;
    let file = &map[diagnostic.source_idx];

    match diagnostic.span {
        Some(span) => {
            let (line, column) = char_line_col(file, span.start());
            writeln!(sink, "  --> {}:{line}:{column}", file_name(file))?;
            write_snippet(file, span, sink)?;
        }
        None => writeln!(sink, "  --> {}", file_name(file))?,
    }

    if let Some((source, span)) = diagnostic.secondary_span {
        let file = &map[source];
        let (line, column) = char_line_col(file, span.start());
        writeln!(sink, "note: see also {}:{line}:{column}", file_name(file))?;
        write_snippet(file, span, sink)?;
    }

    if let Some(suggestion) = &diagnostic.suggestion {
        writeln!(sink, "suggestion: {suggestion}")?;
    }

    writeln!(sink)
}

/// The 1-based line and 1-based, character-counted column of a position.
fn char_line_col(file: &SourceFile, pos: u32) -> (u32, usize) {
    let Some(line_col) = file.line_col(pos) else {
        return (0, 0);
    };

    let line_start = (pos - line_col.column) as usize;
    let column = file
        .contents()
        .get(line_start..pos as usize)
        .map_or(line_col.column as usize, |x| x.chars().count());

    (line_col.line, column + 1)
}

fn write_snippet(file: &SourceFile, span: Span, sink: &mut dyn Write) -> io::Result<()> {
    let (Some(start), Some(context)) = (file.line_col(span.start()), file.context(span)) else {
        return Ok(());
    };

    let mut line_start = span.start() - start.column;

    for (idx, raw) in context.split('\n').enumerate() {
        let line_n = start.line + idx as u32;
        let text = raw.strip_suffix('\r').unwrap_or(raw);
        writeln!(sink, "{line_n:>5} | {text}")?;

        let line_end = line_start + text.len() as u32;
        let from = (span.start().clamp(line_start, line_end) - line_start) as usize;
        let to = (span.stop().clamp(line_start, line_end) - line_start) as usize;

        if idx == 0 || from != to {
            let pad = text.get(..from).map_or(from, |x| x.chars().count());
            let width = text
                .get(from..to)
                .map_or(to.saturating_sub(from), |x| x.chars().count())
                .max(1);

            writeln!(sink, "      | {}{}", " ".repeat(pad), "^".repeat(width))?;
        }

        line_start += raw.len() as u32 + 1;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(src: &str, span: Span) -> String {
        let map = SourceMap::new();
        let idx = map.insert(SourceFile::from_memory(src.to_owned()));
        let diagnostics = Diagnostics::default();
        diagnostics.push(Diagnostic::error(span, "msg", None, idx));
        let mut out = vec![];
        diagnostics.write(&map, &mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn renders_full_last_line() {
        let out = render("func id { a }", Span::new(5, 7));
        assert!(out.contains("<memory>:1:6"), "{out}");
        assert!(out.contains("    1 | func id { a }"), "{out}");
        assert!(out.contains("      |      ^^\n"), "{out}");
    }

    #[test]
    fn renders_zero_width_span() {
        let out = render("abc", Span::new(3, 3));
        assert!(out.contains("      |    ^\n"), "{out}");
    }

    #[test]
    fn counts_columns_in_chars() {
        let out = render("\"é\" x", Span::new(5, 6));
        assert!(out.contains(":1:5"), "{out}");
        assert!(out.contains("      |     ^\n"), "{out}");
    }
}
