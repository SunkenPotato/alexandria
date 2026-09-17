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
};

use source::{SourceIdx, SourceMap};
use span::Span;

/// The diagnostic level.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DiagnosticLevel {
    /// A warning.
    Warn,
    /// An error.
    Error,
    /// A different kind of diagnostic. This is commonly a note.
    Other,
}

impl Display for DiagnosticLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::Error => "error",
                Self::Warn => "warn",
                Self::Other => "suggestion",
            }
        )
    }
}

/// A diagnostic.
#[derive(Clone, Debug)]
pub struct Diagnostic {
    /// The span of the diagnostic.
    pub span: Span,
    /// The secondary span, if any other context should be attached.
    pub secondary_span: Option<Span>,
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
        span: Span,
        message: impl Into<String>,
        suggestion: Option<String>,
        source_idx: SourceIdx,
    ) -> Self {
        Self::new(
            span,
            DiagnosticLevel::Warn,
            message.into(),
            suggestion,
            source_idx,
        )
    }

    /// Create an error diagnostic.
    pub fn error(
        span: Span,
        message: impl Into<String>,
        suggestion: Option<String>,
        source_idx: SourceIdx,
    ) -> Self {
        Self::new(
            span,
            DiagnosticLevel::Error,
            message.into(),
            suggestion,
            source_idx,
        )
    }

    /// Create a new diagnostic.
    pub const fn new(
        span: Span,
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

    /// Attach a secondary span to this diagnostic.
    pub fn with_secondary(self, span: Span) -> Self {
        Self {
            secondary_span: Some(span),
            ..self
        }
    }
}

/// A diagnostics pool. This is simply a monotonic wrapper around a [`Vec`].
#[derive(Clone, Debug, Default)]
pub struct Diagnostics {
    diagnostics: Vec<Diagnostic>,
}

impl Diagnostics {
    /// Merge this diagnostic pool with another one.
    pub fn merge(&mut self, mut other: Self) {
        self.diagnostics.append(&mut other.diagnostics);
    }

    /// Add a diagnostic to this pool.
    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic)
    }

    /// Remove all diagnostics after a certain index.
    pub fn cull(&mut self, from: usize) {
        self.diagnostics.drain(from..);
    }

    /// Retrieve the number of diagnostics in the file.
    pub const fn len(&self) -> usize {
        self.diagnostics.len()
    }

    /// Check whether this contains any diagnostics.
    pub const fn is_empty(&self) -> bool {
        self.diagnostics.is_empty()
    }

    /// Write all the diagnostics in this pool to a sink. This is commonly something like `stdout`.
    pub fn write(&self, map: &SourceMap, sink: &mut dyn Write) -> std::io::Result<()> {
        for diagnostic in &self.diagnostics {
            let file = &map[diagnostic.source_idx];
            let line_col = file
                .line_col(diagnostic.span.start())
                .expect("span should be valid");
            let line_col_stop = file
                .line_col(diagnostic.span.stop())
                .expect("span should be valid");
            // note: context != range of span!
            let context = file.context(diagnostic.span).expect("span should be valid");
            let source: &dyn Display = match file.source() {
                Some(v) => &v.display() as &dyn Display,
                None => &"tmp",
            };

            writeln!(sink, "{}: {}", diagnostic.level, diagnostic.message)?;
            writeln!(
                sink,
                " -> {}:{}:{}:",
                source, line_col.line, line_col.column
            )?;

            for (idx, line) in context.lines().enumerate() {
                let line_n = line_col.line + idx as u32;
                writeln!(sink, "{:>5} | {}", line_n, line)?;
                let underline_start = if line_n == line_col.line {
                    line_col.column
                } else {
                    0
                };

                let underline_stop = if line_n == line_col_stop.line {
                    line_col_stop.column
                } else {
                    line.len() as u32
                };

                write!(sink, "----- | ")?;
                for _ in 0..underline_start {
                    write!(sink, " ")?;
                }

                for _ in 0..(underline_stop - underline_start) {
                    write!(sink, "^")?;
                }

                writeln!(sink, "\n")?;
            }

            if let Some(sec_span) = diagnostic.secondary_span {
                let context = file
                    .context(sec_span)
                    .expect("secondary span should be valid");
                let line_col = file
                    .line_col(sec_span.start())
                    .expect("secondary span should be valid");

                writeln!(sink, "additional context:")?;
                for (idx, line) in context.lines().enumerate() {
                    writeln!(sink, "{:>5} | {}", line_col.line + idx as u32, line)?;
                }
            }

            if let Some(suggestion) = &diagnostic.suggestion {
                writeln!(sink, "suggestion: {suggestion}")?;
            }
        }

        Ok(())
    }

    /// A shorthand for `self.write(map, &mut io::stderr().lock())`.
    pub fn write_stderr(&self, map: &SourceMap) -> std::io::Result<()> {
        let mut stderr = io::stderr().lock();

        self.write(map, &mut stderr)
    }

    /// A shorthand for `self.write(map, &mut io::stdout().lock())`.
    pub fn write_stdout(&self, map: &SourceMap) -> std::io::Result<()> {
        let mut stdout = io::stdout().lock();

        self.write(map, &mut stdout)
    }
}
