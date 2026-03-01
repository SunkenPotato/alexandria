use std::{
    collections::HashMap,
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

#[derive(From)]
pub enum SourceFileError {
    TooLarge(u64),
    Io(io::Error),
    Utf8(Utf8Error),
}

impl SourceFile {
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

    pub fn region(&self, span: Span) -> Option<&str> {
        self.contents.get(span.start as usize..span.stop as usize)
    }

    pub fn contents(&self) -> &str {
        &self.contents
    }

    pub fn source(&self) -> Option<&Path> {
        self.source.as_deref()
    }

    pub fn newlines(&self) -> &[u32] {
        &self.newlines
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
