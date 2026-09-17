//! Utilities for loading and representing source files within the compiler.
//!
//! This crate provides [`SourceFile`] and [`SourceMap`] as the main APIs.

#![feature(seek_stream_len)]

/// The source extension of an alexandria file.
pub static SOURCE_EXTENSION: &str = "aa";
/// Files that may be in the root of a project.
pub static ROOT_FILES: &[&str] = &["main", "mod"];

use std::{
    collections::HashMap,
    fs::File,
    io::{self, Seek},
    ops::Deref,
    path::{Path, PathBuf},
    rc::Rc,
    str::Utf8Error,
    sync::Arc,
};

use index_vec::{IndexVec, define_index_type};
use internment::Intern;
use memchr::Memchr;
use memmap2::Mmap;

use span::Span;
use thiserror::Error;

/// Represents a source file.
///
/// This may be constructed using either [`Self::from_disk`] or [`Self::from_memory`].
#[derive(Debug)]
pub struct SourceFile {
    source: Option<Arc<Path>>,
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
#[derive(Error, Debug)]
pub enum SourceFileError {
    #[error("the supplied file is too large: {_0} bytes")]
    /// The file at the supplied path is too large.
    TooLarge(u64),
    #[error("I/O Error: {_0}")]
    /// I/O error.
    Io(#[from] io::Error),
    #[error("the supplied file does not contain valid UTF-8: {_0}")]
    /// The provided file does not have valid UTF-8 contents.
    Utf8(#[from] Utf8Error),
    /// No file matched the provided module name.
    #[error("a module was referenced which does not exist at the given lookup locations")]
    NoMatches,
}

/// The line and column of a span.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineCol {
    /// The line.
    pub line: u32,
    /// The column.
    pub column: u32,
}

impl SourceFile {
    /// Try to load a file from disk.
    pub fn from_disk(path: impl Into<PathBuf>) -> Result<Self, SourceFileError> {
        let source: PathBuf = path.into().canonicalize()?;

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
            source: Some(source.into()),
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
        self.contents
            .get(span.start() as usize..span.stop() as usize)
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

    /// The source of this span and the context around it (line before and the line after).
    pub fn context(&self, span: Span) -> Option<&str> {
        let newline_idx = match self.newlines.binary_search(&span.start()) {
            Ok(v) => v,
            Err(e) => e,
        };

        let start = if newline_idx == 0 {
            0
        } else {
            self.newlines
                .get(newline_idx.saturating_sub(1))
                .copied()
                .map(|nl| nl + 1)
                .unwrap_or(0)
        };

        let stop = match &self.newlines.binary_search(&span.stop()) {
            Ok(v) => self.newlines[*v],
            Err(e) => self.newlines.get(*e).copied().unwrap_or(span.stop()),
        };

        let (start, stop) = (start.min(stop), start.max(stop));
        self.region(Span::new(start, stop))
    }

    /// Get the line and column of a position.
    pub fn line_col(&self, pos: u32) -> Option<LineCol> {
        if pos as usize > self.contents.len() {
            return None;
        }

        let line = match self.newlines.binary_search(&pos) {
            Ok(v) => v,
            Err(e) => e,
        };

        let line_start = if line == 0 {
            0
        } else {
            &self.newlines[line - 1] + 1
        };

        Some(LineCol {
            line: line as u32 + 1,
            column: pos - line_start,
        })
    }
}

/// A module tree used for source file path resolution.
#[derive(Debug)]
pub enum ModuleTree {
    /// The root of the tree.
    Root {
        /// The path of the root.
        root: PathBuf,
    },
    /// A node.
    WithParent {
        /// The parent of this node.
        parent: Rc<ModuleTree>,
        /// The name of this node.
        name: Intern<str>,
        /// Whether this node was declared as `{name}/mod.aa` or `{name}.aa`.
        subdir: bool,
    },
}

impl ModuleTree {
    /// Create a new tree.
    pub fn new(path: PathBuf) -> Rc<Self> {
        Rc::new(Self::Root { root: path })
    }

    /// Create a child node to this one.
    pub fn child(self: &Rc<Self>, name: Intern<str>, subdir: bool) -> Rc<Self> {
        Rc::new(Self::WithParent {
            parent: Rc::clone(self),
            name,
            subdir,
        })
    }

    /// Obtain the directory where a new source file would be placed, should it be a child of this one.
    pub fn dir(&self) -> PathBuf {
        match self {
            Self::Root { root } => root.clone(),
            Self::WithParent {
                parent,
                name,
                subdir,
            } => {
                let last = if *subdir { name } else { "." };
                parent.dir().join(last)
            }
        }
    }
}

define_index_type! {
    /// A pointer to a source file.
    pub struct SourceIdx = u32;
}

/// A map of source files.
#[derive(Default, Debug)]
pub struct SourceMap {
    map: IndexVec<SourceIdx, SourceFile>,
    path_index: HashMap<Arc<Path>, SourceIdx>,
}

impl SourceMap {
    /// Create a new map.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a source file into the map. This also updates the path index.
    pub fn insert(&mut self, file: SourceFile) -> SourceIdx {
        let path = file.source.clone();
        let idx = self.map.push(file);
        if let Some(path) = path {
            self.path_index.insert(path, idx);
        }

        idx
    }

    /// Load a new source file with the given name as a child of the passed node.
    pub fn load_from_mod(
        &mut self,
        node: &ModuleTree,
        name: &str,
    ) -> Result<(SourceIdx, bool), SourceFileError> {
        let base_path = node.dir();
        let fname = format!("{name}.{SOURCE_EXTENSION}");
        let (path, subdir) = if let p = base_path.join(&fname)
            && p.exists()
        {
            (p, false)
        } else if let p = base_path.join(format!("{fname}/mod.{SOURCE_EXTENSION}"))
            && p.exists()
        {
            (p, true)
        } else {
            return Err(SourceFileError::NoMatches);
        };

        let file = SourceFile::from_disk(path)?;
        let idx = self.insert(file);

        Ok((idx, subdir))
    }

    /// Lookup a path for a source file.
    pub fn lookup(&self, path: &Path) -> Option<SourceIdx> {
        self.path_index.get(path).copied()
    }

    /// Fill the current source map with all relevant files in the given directory.
    #[deprecated]
    pub fn discover(&mut self, base: PathBuf) -> Result<(), SourceFileError> {
        let base = base.canonicalize()?;

        self.visit_dir(base)
    }

    fn visit_dir(&mut self, dir: PathBuf) -> Result<(), SourceFileError> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let ty = entry.file_type()?;
            if ty.is_dir() {
                self.visit_dir(entry.path())?;
            } else {
                let path = entry.path();
                if let Some(ext) = path.extension()
                    && ext == SOURCE_EXTENSION
                {
                    let file = SourceFile::from_disk(path)?;
                    self.insert(file);
                }
            }
        }

        Ok(())
    }
}

impl Deref for SourceMap {
    type Target = IndexVec<SourceIdx, SourceFile>;

    fn deref(&self) -> &Self::Target {
        &self.map
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_tree_get_dir() {
        assert_eq!(
            ModuleTree::WithParent {
                parent: Rc::new(ModuleTree::WithParent {
                    parent: Rc::new(ModuleTree::Root {
                        root: PathBuf::from("/Users/alexandria/code/project/")
                    }),
                    name: Intern::from("expr"),
                    subdir: true,
                }),
                name: Intern::from("stmt"),
                subdir: false
            }
            .dir(),
            PathBuf::from("/Users/alexandria/code/project/expr")
        );
    }
}
