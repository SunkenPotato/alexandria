//! Utilities for loading and representing source files within the compiler.
//!
//! This crate provides [`SourceFile`] and [`SourceMap`] as the main APIs.

/// The source extension of an alexandria file.
pub static SOURCE_EXTENSION: &str = "aa";
/// Files that may be in the root of a project.
pub static ROOT_FILES: &[&str] = &["main", "mod"];

use std::{
    fs::File,
    io,
    ops::Deref,
    path::{Path, PathBuf},
    rc::Rc,
    str::Utf8Error,
    sync::Arc,
};

use dashmap::DashMap;
use index_boxcar_vec::IndexBoxcarVec;
use index_vec::define_index_type;
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

        let file = File::open(&source)?;
        let file_len = file.metadata()?.len();

        if file_len > u32::MAX as u64 {
            return Err(SourceFileError::TooLarge(file_len));
        }

        // SAFETY: the mapping is read-only. Modifying the file on disk while the compiler runs is
        //         undefined behaviour; this is accepted, as with any mmap-based compiler.
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

    /// The full lines covered by this span.
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
            Err(e) => self
                .newlines
                .get(*e)
                .copied()
                .unwrap_or(self.contents.len() as u32),
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
#[derive(Default, Debug, Clone)]
pub struct SourceMap {
    raw: Arc<SourceMapRef>,
}

#[derive(Default, Debug)]
struct SourceMapRef {
    map: index_boxcar_vec::IndexBoxcarVec<SourceIdx, SourceFile>,
    path_index: DashMap<Arc<Path>, SourceIdx>,
}

impl SourceMap {
    /// Create a new map.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a source file into the map. This also updates the path index.
    pub fn insert(&self, file: SourceFile) -> SourceIdx {
        let path = file.source.clone();
        let idx = self.raw.map.push(file);
        if let Some(path) = path {
            self.raw.path_index.insert(path, idx);
        }

        idx
    }

    /// Load a new source file with the given name as a child of the passed node.
    ///
    /// The file is looked up as `{name}.aa` first and `{name}/mod.aa` second. If the file was
    /// already loaded, it is not loaded again; see [`LoadedModule::fresh`].
    pub fn load_from_mod(
        &self,
        node: &ModuleTree,
        name: &str,
    ) -> Result<LoadedModule, SourceFileError> {
        let base_path = node.dir();
        let (path, subdir) = if let p = base_path.join(format!("{name}.{SOURCE_EXTENSION}"))
            && p.exists()
        {
            (p, false)
        } else if let p = base_path.join(name).join(format!("mod.{SOURCE_EXTENSION}"))
            && p.exists()
        {
            (p, true)
        } else {
            return Err(SourceFileError::NoMatches);
        };

        if let Some(idx) = self.lookup(&path.canonicalize()?) {
            return Ok(LoadedModule {
                idx,
                subdir,
                fresh: false,
            });
        }

        let file = SourceFile::from_disk(path)?;
        let idx = self.insert(file);

        Ok(LoadedModule {
            idx,
            subdir,
            fresh: true,
        })
    }

    /// Lookup a path for a source file. The path must be canonical.
    pub fn lookup(&self, path: &Path) -> Option<SourceIdx> {
        self.raw.path_index.get(path).as_deref().copied()
    }
}

/// The result of [`SourceMap::load_from_mod`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoadedModule {
    /// The index of the loaded file.
    pub idx: SourceIdx,
    /// Whether the module was found as `{name}/mod.aa` (`true`) or `{name}.aa` (`false`).
    pub subdir: bool,
    /// Whether the file was newly loaded (`true`) or had already been loaded before (`false`).
    pub fresh: bool,
}

impl Deref for SourceMap {
    type Target = IndexBoxcarVec<SourceIdx, SourceFile>;

    fn deref(&self) -> &Self::Target {
        &self.raw.map
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(s: &str) -> SourceFile {
        SourceFile::from_memory(s.to_owned())
    }

    #[test]
    fn context_single_line_without_trailing_newline() {
        let f = file("func id { a }");
        assert_eq!(f.context(Span::new(5, 7)), Some("func id { a }"));
    }

    #[test]
    fn context_last_line_with_trailing_newline() {
        let f = file("abc\ndef\n");
        assert_eq!(f.context(Span::new(4, 5)), Some("def"));
    }

    #[test]
    fn context_first_line() {
        let f = file("abc\ndef");
        assert_eq!(f.context(Span::new(0, 1)), Some("abc"));
    }

    #[test]
    fn context_multi_line() {
        let f = file("abc\ndef\nghi");
        assert_eq!(f.context(Span::new(1, 5)), Some("abc\ndef"));
    }

    #[test]
    fn line_col_positions() {
        let f = file("abc\ndef");
        assert_eq!(f.line_col(0), Some(LineCol { line: 1, column: 0 }));
        assert_eq!(f.line_col(3), Some(LineCol { line: 1, column: 3 }));
        assert_eq!(f.line_col(4), Some(LineCol { line: 2, column: 0 }));
        assert_eq!(f.line_col(7), Some(LineCol { line: 2, column: 3 }));
        assert_eq!(f.line_col(8), None);
    }

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
