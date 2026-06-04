use std::{
    fs::File,
    io::{self, Seek},
    ops::Deref,
    path::{Path, PathBuf},
    str::Utf8Error,
};

use derive_more::From;
use index_vec::{IndexVec, define_index_type};
use memchr::Memchr;
use memmap2::Mmap;

use crate::Span;

/// Represents a source file.
///
/// This may be constructed using either [`Self::from_disk`] or [`Self::from_memory`].
#[derive(Debug)]
pub struct SourceFile {
    source: Option<PathBuf>,
    newlines: Vec<u32>,
    contents: SourceFileContents,
}

#[derive(Debug)]
enum SourceFileContents {
    Mmap(Mmap),
    Memory(String),
}

impl Deref for SourceFileContents {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Memory(mem) => mem.as_str(),
            // SAFETY: this can only be constructed internally and when it is,
            //         we check that the string is valid.
            Self::Mmap(mmap) => unsafe { str::from_utf8_unchecked(mmap) },
        }
    }
}

/// Errors that may occur while trying to create a source file.
#[derive(From)]
pub enum SourceFileError {
    /// The file at the supplied path is too large.
    TooLarge(u64),
    /// I/O error.
    Io(io::Error),
    /// The provided file does not have valid UTF-8 contents.
    Utf8(Utf8Error),
}

/// The line and column of a span.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineCol {
    pub line: u32,
    pub column: u32,
}

impl SourceFile {
    /// Try to load a file from disk.
    pub fn from_disk(path: impl Into<PathBuf>) -> Result<Self, SourceFileError> {
        let source: PathBuf = path.into();

        let mut file = File::open(&source)?;
        let file_len = file.stream_len()?;

        if file_len > u32::MAX as u64 {
            return Err(SourceFileError::TooLarge(file_len));
        }

        let data = unsafe { Mmap::map(&file) }?;
        // TODO: collapse the memchr and this check into one loop, if possible.
        _ = str::from_utf8(&data)?;

        #[expect(clippy::char_lit_as_u8, reason = "newlines are ASCII")]
        let memchr = Memchr::new('\n' as u8, &data);
        let newlines = memchr.map(|v| v as u32).collect();

        Ok(Self {
            contents: SourceFileContents::Mmap(data),
            source: Some(source),
            newlines,
        })
    }

    /// Create a source "file" from in-memory contents.
    ///
    /// It is not recommended to use this if you have an existing file.
    pub fn from_memory(contents: String) -> Self {
        #[expect(clippy::char_lit_as_u8, reason = "newlines are ASCII")]
        let memchr = Memchr::new('\n' as u8, contents.as_bytes());
        let newlines = memchr.map(|v| v as u32).collect();

        Self {
            contents: SourceFileContents::Memory(contents),
            source: None,
            newlines,
        }
    }

    /// Get the region that the supplied span points at.
    ///
    /// For a wider selection of text, use [`Self::context`].
    pub fn region(&self, span: Span) -> Option<&str> {
        self.contents.get(span.start as usize..span.stop as usize)
    }

    /// Get the contents of this source file.
    pub fn contents(&self) -> &str {
        &self.contents
    }

    /// Get the source of this file. If this was an in-memory file, this returns [`None`].
    pub fn source(&self) -> Option<&Path> {
        self.source.as_deref()
    }

    /// Get the newline positions of this file.
    pub fn newlines(&self) -> &[u32] {
        &self.newlines
    }

    pub fn context(&self, span: Span) -> Option<&str> {
        let start = match self.newlines.binary_search(&span.start) {
            Ok(v) => self.newlines.get(v.saturating_sub(1)).copied().unwrap_or(0),
            Err(e) => self
                .newlines
                .get(e.saturating_sub(1))
                .copied()
                .map(|v| v + 1)
                .unwrap_or(0),
        };

        let stop = match self.newlines.binary_search(&span.stop) {
            Ok(v) => self.newlines.get(v + 1).copied().unwrap_or(span.stop),
            Err(e) => self.newlines.get(e).copied().unwrap_or(span.stop),
        };

        self.region(Span::new(start, stop))
    }

    /// Get the line and column of the given position.
    pub fn line_col(&self, pos: u32) -> Option<LineCol> {
        let line = match self.newlines.binary_search(&pos) {
            Ok(v) => v,
            Err(e) => e,
        } + 1;

        let column = pos
            - self
                .newlines
                .get(line - 1)
                .copied()
                .map(|nl| nl + 1)
                .unwrap_or(0);

        Some(LineCol {
            line: line as u32,
            column,
        })
    }
}

define_index_type! {
    pub struct SourceIdx = u32;
}

#[derive(Default, Debug)]
pub struct SourceMap(IndexVec<SourceIdx, SourceFile>);

impl SourceMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, file: SourceFile) -> SourceIdx {
        self.0.push(file)
    }
}

impl Deref for SourceMap {
    type Target = IndexVec<SourceIdx, SourceFile>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
