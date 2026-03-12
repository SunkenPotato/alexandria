use std::{
    fmt::Display,
    io::{self, Write},
};

use span::{
    Span,
    source::{SourceIdx, SourceMap},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DiagnosticLevel {
    Warn,
    Error,
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

#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub span: Span,
    pub level: DiagnosticLevel,
    pub message: String,
    pub suggestion: Option<String>,
}

impl Diagnostic {
    pub fn warn(span: Span, message: impl Into<String>, suggestion: Option<String>) -> Self {
        Self::new(span, DiagnosticLevel::Warn, message.into(), suggestion)
    }

    pub fn error(span: Span, message: impl Into<String>, suggestion: Option<String>) -> Self {
        Self::new(span, DiagnosticLevel::Error, message.into(), suggestion)
    }

    pub const fn new(
        span: Span,
        level: DiagnosticLevel,
        message: String,
        suggestion: Option<String>,
    ) -> Self {
        Self {
            span,
            suggestion,
            message,
            level,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Diagnostics {
    source_idx: SourceIdx,
    diagnostics: Vec<Diagnostic>,
}

impl Diagnostics {
    pub const fn new(source_idx: SourceIdx) -> Self {
        Self {
            source_idx,
            diagnostics: vec![],
        }
    }

    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic)
    }

    pub fn write(&self, map: &SourceMap, sink: &mut dyn Write) -> std::io::Result<()> {
        let file = &map[self.source_idx];

        for diagnostic in &self.diagnostics {
            let line_col = file
                .line_col(diagnostic.span.start())
                .expect("span should be valid");
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
                writeln!(sink, "{:>5} | {}", line_col.line + idx as u32, line)?;
            }

            if let Some(suggestion) = &diagnostic.suggestion {
                writeln!(sink, "suggestion: {suggestion}")?;
            }
        }

        Ok(())
    }

    pub fn write_stderr(&self, map: &SourceMap) -> std::io::Result<()> {
        let mut stderr = io::stderr().lock();

        self.write(map, &mut stderr)
    }
}
