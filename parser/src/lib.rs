//! The parser for alexandria. See [`Parser`] for the entrypoint.
#![feature(str_from_raw_parts)]

pub mod expr;
pub mod item;
pub mod stmt;

use std::rc::Rc;

use derive_more::From;
use diagnostic::{Diagnostic, Diagnostics};
use lexer::{Intern, LexError, Lexer, Token, TokenKind};
use node::NodeId;
use smallvec::SmallVec;
use source::{ModuleTree, SourceIdx, SourceMap};
use span::{Span, Spanned};

use crate::{
    ast_table::AstTable,
    crate_table::{CrateParseClaim, CrateTable},
    item::{InlineModule, Item},
};

/// A specialized `Result<T, ParseError>`
pub type ParseResult<T> = std::result::Result<T, ParseError>;

macro_rules! keywords {
    (
        $(
            $ident:ident = $value:expr
        ),*
    ) => {
        $(
            #[doc = concat!("The `", $value, "` keyword.")]
            pub static $ident: ::std::sync::LazyLock<Intern<str>> =
            ::std::sync::LazyLock::new(|| Intern::from($value));
        )*

        /// A set of all keywords.
        pub static KEYWORDS: &[&::std::sync::LazyLock<Intern<str>>] = &[$(&$ident),*];
    };
}

keywords! {
    DECL = "decl",
    IF = "if",
    ELSE = "else",
    LOOP = "loop",
    CONTINUE = "continue",
    BREAK = "break",
    RETURN = "return",
    PRODUCT = "product",
    SUM = "sum",
    FUNC = "func",
    PUBLIC = "pub",
    IMPORT = "import",
    STATIC = "static",
    CONST = "const",
    MODULE = "module",
    INCLUDE = "include",
    SUPER = "super",
    CRATE = "crate"
}

/// Implementation of the [`AstTable`].
pub mod ast_table {
    use std::sync::Arc;

    use dashmap::DashMap;
    use node::NodeId;
    use source::SourceIdx;

    use crate::item::InlineModule;

    /// A collection of AST, grouped by file.
    #[derive(Clone, Debug, Default)]
    pub struct AstTable {
        raw: Arc<AstTableRef>,
    }

    #[derive(Debug, Default)]
    struct AstTableRef {
        /// The source-AST relation.
        source_map: DashMap<SourceIdx, InlineModule>,
        /// The node-source relation.
        node_map: DashMap<NodeId, SourceIdx>,
    }

    impl AstTable {
        /// Retrieve the AST of a given file.
        ///
        /// # Panics
        /// This immediately calls unwrap, so if the table does not contain `k`, this will panic.
        pub fn by_src(
            &self,
            k: SourceIdx,
        ) -> dashmap::mapref::one::Ref<'_, SourceIdx, InlineModule> {
            self.raw.source_map.get(&k).unwrap()
        }

        /// Retrieve the AST of a given node.
        ///
        /// # Panics
        /// This immediately calls unwrap, so if the table does not contain `k`, this will panic.
        pub fn by_node_id(
            &self,
            k: NodeId,
        ) -> dashmap::mapref::one::Ref<'_, SourceIdx, InlineModule> {
            self.by_src(self.source_idx(k))
        }

        /// Retrieve the source ID of a node.
        ///
        /// # Panics
        /// This immediately calls unwrap, so if the table does not contain `k`, this will panic.
        pub fn source_idx(&self, k: NodeId) -> SourceIdx {
            *self.raw.node_map.get(&k).unwrap()
        }

        /// Add an AST to the table.
        pub fn insert(&self, source: SourceIdx, node: NodeId, data: InlineModule) {
            self.raw.source_map.insert(source, data);
            self.raw.node_map.insert(node, source);
        }

        /// Check whether the table is empty.
        pub fn is_empty(&self) -> bool {
            self.raw.source_map.is_empty()
        }
    }
}

/// Implementation of the [`CrateTable`].
pub mod crate_table {
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex, OnceLock},
    };

    use index_vec::IndexVec;
    use lexer::Intern;
    use source::SourceIdx;

    use crate::ParseError;

    index_vec::define_index_type! {
        /// A unique identifier for a crate.
        pub struct CrateId = u32;
    }

    type Lock = Arc<OnceLock<Result<(), ParseError>>>;

    // Private to ensure proper state transitions.
    #[derive(Clone, Debug)]
    enum CrateStatusRaw {
        /// The job has not yet been claimed by a thread. It is free to claim and may be claimed with [`CrateStatus::claim`].
        Unclaimed { source: SourceIdx },
        /// The job has either been completed or is being completed by a thread.
        Processed { value: Lock, source: SourceIdx },
    }

    /// The status of the parsing of an entire crate.
    ///
    /// To obtain the [`SourceIdx`] of the crate, use [`CrateStatus::get`].
    ///
    /// To claim the job, use [`CrateStatus::claim`].
    #[derive(Clone, Debug)]
    pub struct CrateStatus {
        raw: CrateStatusRaw,
    }

    /// A claim issued to a thread willing to parse a crate.
    #[derive(Debug)]
    pub struct CrateParseClaim {
        source_idx: SourceIdx,
        lock: Arc<OnceLock<Result<(), ParseError>>>,
    }

    impl CrateParseClaim {
        const fn new(source_idx: SourceIdx, lock: Lock) -> Self {
            Self { source_idx, lock }
        }

        /// Obtain the source ID of the entrypoint to parse.
        pub fn source_idx(&self) -> SourceIdx {
            self.source_idx
        }

        /// Submit the parse result and mark the crate as parsed.
        pub fn finish(self, result: Result<(), ParseError>) {
            self.lock.set(result).unwrap();
        }
    }

    impl CrateStatus {
        /// Returns `true` if the crate status is [`Unclaimed`].
        ///
        /// [`Unclaimed`]: CrateStatus::Unclaimed
        #[must_use]
        pub fn is_unclaimed(&self) -> bool {
            matches!(self.raw, CrateStatusRaw::Unclaimed { .. })
        }

        /// Claim the job, if available. The caller is expected to place the [`SourceIdx`] back into the provided
        /// [`OnceLock`] once—and only once completed. The caller is also expected to place the parsed AST into the [`AstTable`].
        pub fn claim(&mut self) -> Option<CrateParseClaim> {
            match self.raw {
                CrateStatusRaw::Unclaimed { source } => {
                    let lock = Arc::new(OnceLock::new());
                    self.raw = CrateStatusRaw::Processed {
                        value: Arc::clone(&lock),
                        source,
                    };
                    Some(CrateParseClaim::new(source, lock))
                }
                CrateStatusRaw::Processed { .. } => None,
            }
        }

        /// Retrieve the [`SourceIdx`], if the crate has successfully been parsed.
        pub fn get(&self) -> Option<&Result<(), ParseError>> {
            match &self.raw {
                CrateStatusRaw::Unclaimed { .. } => None,
                CrateStatusRaw::Processed { value, .. } => value.get(),
            }
        }

        /// Create a new [`CrateStatus`] that has not been processed.
        pub const fn new(source: SourceIdx) -> Self {
            Self {
                raw: CrateStatusRaw::Unclaimed { source },
            }
        }

        /// Create a new [`CrateStatus`] that has been processed.
        pub fn processed(source: SourceIdx) -> Self {
            Self {
                raw: CrateStatusRaw::Processed {
                    value: Arc::new(OnceLock::from(Ok(()))),
                    source,
                },
            }
        }

        /// Retrieve the [`SourceIdx`] of this [`CrateStatus`].
        pub fn source_idx(&self) -> SourceIdx {
            match self.raw {
                CrateStatusRaw::Unclaimed { source } | CrateStatusRaw::Processed { source, .. } => {
                    source
                }
            }
        }
    }

    /// A registry for crates.
    #[derive(Clone, Debug, Default)]
    pub struct CrateTable {
        raw: Arc<Mutex<CrateTableRaw>>,
    }

    #[derive(Debug, Default)]
    struct CrateTableRaw {
        crate_table: IndexVec<CrateId, CrateStatus>,
        name_table: HashMap<Intern<str>, CrateId>,
    }

    impl CrateTable {
        /// Retrieve the status and information stored about this crate.
        pub fn status(&self, id: CrateId) -> CrateStatus {
            self.raw.lock().unwrap().crate_table[id].clone()
        }

        /// Attempt to retrieve the crate by it's name.
        pub fn status_by_name(&self, name: Intern<str>) -> Option<CrateStatus> {
            let raw = self.raw.lock().unwrap();
            raw.name_table
                .get(&name)
                .map(|&id| raw.crate_table[id].clone())
        }

        /// Attempt to retrieve the crate ID by it's name.
        pub fn id_by_name(&self, name: Intern<str>) -> Option<CrateId> {
            self.raw.lock().unwrap().name_table.get(&name).copied()
        }

        /// Directly insert a parsed crate.
        pub fn insert_processed(&self, name: Intern<str>, id: SourceIdx) -> Option<CrateId> {
            let mut lock = self.raw.lock().unwrap();

            if lock.name_table.contains_key(&name) {
                None
            } else {
                let id = lock.crate_table.push(CrateStatus::processed(id));
                lock.name_table.insert(name, id);
                Some(id)
            }
        }

        /// Insert a crate parse job.
        pub fn insert(&self, name: Intern<str>, id: SourceIdx) -> bool {
            let mut lock = self.raw.lock().unwrap();

            if lock.name_table.contains_key(&name) {
                false
            } else {
                let id = lock.crate_table.push(CrateStatus::new(id));
                lock.name_table.insert(name, id);
                true
            }
        }
    }
}

/// A parser error. Most errors are expressed via diagnostics instead of this
#[derive(From, Clone, Debug)]
pub enum ParseError {
    /// Expected one of the given tokens at the specified location.
    TokenMismatch(SmallVec<[TokenKind; 6]>, Span),
    /// The end of the input was reached. The variant stores the expected tokens and the location.
    Eof(SmallVec<[TokenKind; 3]>, Span),
    /// Expected the keyword at that location.
    ExpectedKw(Intern<str>, Span),
    #[doc(hidden)]
    InternalParseError,
    /// An error was produced by the lexer/tokenizer.
    LexError(LexError),
    /// The entire input was not consumed fully.
    ///
    /// The span points to the last token in the source.
    InputNotConsumed(Span),
}

impl ParseError {
    /// Convert this error into a diagnostic and add it to the given pool.
    #[track_caller]
    pub fn display(&self, source: SourceIdx, mut diag: Diagnostics) {
        match self {
            ParseError::ExpectedKw(kw, span) => {
                diag.push(Diagnostic::error(
                    *span,
                    format!("expected keyword '{kw}'"),
                    None,
                    source,
                ));
            }
            ParseError::TokenMismatch(tokens, span) => {
                let msg = if tokens.len() > 1 {
                    format!("expected one of {tokens:?}")
                } else {
                    format!("expected {:?}", tokens[0])
                };

                diag.push(Diagnostic::error(*span, msg, None, source));
            }
            ParseError::Eof(tokens, span) => {
                let msg = if tokens.len() > 1 {
                    format!("expected one of {tokens:?}, got EOF")
                } else if tokens.is_empty() {
                    "unexpected EOF".to_owned()
                } else {
                    format!("expected {:?}", tokens[0])
                };

                diag.push(Diagnostic::error(*span, msg, None, source));
            }
            ParseError::InputNotConsumed(span) => diag.push(Diagnostic::error(
                *span,
                "failed to consume input fully",
                None,
                source,
            )),
            // lexer already pushes diagnostics
            ParseError::LexError(_) => (),
            ParseError::InternalParseError => unimplemented!(),
        }
    }
}

/// The parser. See [`Parser::parse`].
pub struct Parser {
    entrypoint: SourceIdx,
    sources: SourceMap,
    diagnostics: Diagnostics,
    ast_table: AstTable,
    crate_table: CrateTable,
    claim: CrateParseClaim,
}

impl Parser {
    /// Create a new parser to parse the given source file as the entrypoint for a crate.
    pub fn new(
        sources: SourceMap,
        entrypoint: SourceIdx,
        mut diagnostics: Diagnostics,
        ast_table: AstTable,
        crate_table: CrateTable,
        name: Intern<str>,
    ) -> Option<Self> {
        let Some(id) = crate_table.insert_processed(name, entrypoint) else {
            diagnostics.push(Diagnostic::error(
                None::<Span>,
                "a crate with the name `{}` already exists",
                None,
                entrypoint,
            ));

            return None;
        };

        Some(Self {
            sources,
            entrypoint,
            diagnostics,
            ast_table,
            claim: crate_table.status(id).claim().unwrap(),
            crate_table,
        })
    }

    /// Parse the specified input. This mutates the AST table instead of returning the result.
    pub fn parse(mut self) {
        let res = self.parse_1();
        self.claim.finish(res);
    }

    fn parse_1(&mut self) -> ParseResult<()> {
        let lexed = Lexer::new(&self.sources, self.entrypoint, self.diagnostics.clone()).lex()?;
        let mut guard = ParseGuard {
            diagnostics: self.diagnostics.clone(),
            index: &mut 0,
            committed: 0,
            diag_len: 0,
            stream: lexed.tokens(),
            source_idx: self.entrypoint,
            ast_table: self.ast_table.clone(),
            crate_table: self.crate_table.clone(),
            module_tree: ModuleTree::new(
                self.sources[self.entrypoint].source().unwrap().to_owned(),
            ),
            sources: self.sources.clone(),
        };

        guard.commit_diag();

        let parse_res = guard.with(|mut guard| {
            let mut items = vec![];

            loop {
                match guard.with(Item::parse) {
                    Ok(r) => items.push(r),
                    Err(e) => return Err(e),
                }

                guard.commit_diag();

                if let Err(ParseError::Eof(..)) = guard.peek() {
                    guard.rollback_diag();

                    break;
                }
            }

            Ok(InlineModule { items })
        });
        let consumed_tokens = *guard.index;

        if consumed_tokens != lexed.tokens().len() {
            return Err(parse_res.err().unwrap_or(ParseError::InputNotConsumed(
                guard
                    .peek()
                    .map(|x| x.span)
                    .ok()
                    .or_else(|| guard.stream.last().map(|x| x.span))
                    .unwrap_or(Span::new(0, 0)),
            )));
        }

        drop(guard);

        self.ast_table
            .insert(self.entrypoint, NodeId::new(), parse_res?);

        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn guard<'s, 'gs, 'this>(
        &'this mut self,
        stream: &'s [Spanned<Token>],
    ) -> ParseGuard<'gs, 'static>
    where
        'this: 'gs,
        's: 'gs,
    {
        use std::path::PathBuf;

        ParseGuard::<'gs, 'static> {
            diag_len: self.diagnostics.len(),
            diagnostics: self.diagnostics.clone(),
            // ONLY because this is #[cfg(test)]
            index: Box::leak(Box::new(0)),
            committed: 0,
            source_idx: self.entrypoint,
            sources: self.sources.clone(),
            ast_table: self.ast_table.clone(),
            crate_table: self.crate_table.clone(),
            module_tree: ModuleTree::new(PathBuf::new()),
            stream,
        }
    }
}

/// A parser guard for parsing a specific element.
#[derive(Debug)]
pub struct ParseGuard<'s, 'i> {
    diagnostics: Diagnostics,
    index: &'i mut usize,
    committed: usize,
    diag_len: usize,
    stream: &'s [Spanned<Token>],
    source_idx: SourceIdx,
    sources: SourceMap,
    ast_table: AstTable,
    crate_table: CrateTable,
    module_tree: Rc<ModuleTree>,
}

impl<'s, 'i> ParseGuard<'s, 'i> {
    fn subguard<'d2, 's2, 'i2, 'this>(
        &'this mut self,
        index: &'i2 mut usize,
        stream: Option<&'s2 [Spanned<Token>]>,
        source: Option<SourceIdx>,
    ) -> ParseGuard<'s2, 'i2>
    where
        's: 's2,
        'i: 'i2,
        'this: 'd2 + 'i2 + 's2,
    {
        ParseGuard {
            diag_len: self.diagnostics.len(),
            diagnostics: self.diagnostics.clone(),
            committed: *self.index,
            stream: stream.unwrap_or(self.stream),
            source_idx: source.unwrap_or(self.source_idx),
            sources: self.sources.clone(),
            ast_table: self.ast_table.clone(),
            crate_table: self.crate_table.clone(),
            module_tree: Rc::clone(&self.module_tree),
            index,
        }
    }

    /// Commit the diagnostics to the current status.s
    pub fn commit_diag(&mut self) {
        self.diag_len = self.diagnostics.len();
    }

    /// Rollback all diagnostics added since the last commit.
    pub fn rollback_diag(&mut self) {
        self.diagnostics.cull(self.diag_len);
    }

    /// Manually commit the consumed tokens.
    pub fn commit(&mut self) {
        self.committed = *self.index;
    }

    /// Rollback the consumed tokens.
    pub fn rollback(&mut self) {
        *self.index = self.committed;
    }

    /// Consume a token.
    #[expect(clippy::should_implement_trait)]
    pub fn next(&mut self) -> ParseResult<Spanned<Token>> {
        match self.stream.get(*self.index) {
            Some(v) => {
                *self.index += 1;
                Ok(*v)
            }
            None => {
                let span = self
                    .stream
                    .get(self.index.saturating_sub(1))
                    .map(|x| x.span)
                    .unwrap_or(Span::new(0, 0));

                Err(ParseError::Eof(smallvec::smallvec![], span))
            }
        }
    }

    /// Get the next token without consuming it.
    pub fn peek(&self) -> ParseResult<Spanned<Token>> {
        self.stream.get(*self.index).copied().ok_or_else(|| {
            let span = self
                .stream
                .get(self.index.saturating_sub(1))
                .map(|x| x.span)
                .unwrap_or(Span::new(0, 0));

            ParseError::Eof(smallvec::smallvec![], span)
        })
    }

    /// Consume the next token if it is of the given kind.
    pub fn next_require(&mut self, kind: TokenKind) -> ParseResult<Spanned<Token>> {
        match self.stream.get(*self.index) {
            Some(v) if v.item.kind == kind => {
                *self.index += 1;
                Ok(*v)
            }
            Some(v) => Err(ParseError::TokenMismatch(smallvec::smallvec![kind], v.span)),
            None => {
                let span = self
                    .stream
                    .get(self.index.saturating_sub(1))
                    .map(|x| x.span)
                    .unwrap_or(Span::new(0, 0));

                Err(ParseError::Eof(smallvec::smallvec![kind], span))
            }
        }
    }

    /// See [`Self::peek`] and [`Self::next_require`].
    pub fn peek_require(&self, kind: TokenKind) -> ParseResult<Spanned<Token>> {
        match self.stream.get(*self.index) {
            Some(v) if v.item.kind == kind => Ok(*v),
            Some(v) => Err(ParseError::TokenMismatch(smallvec::smallvec![kind], v.span)),
            None => {
                let span = self
                    .stream
                    .get(self.index.saturating_sub(1))
                    .map(|x| x.span)
                    .unwrap_or(Span::new(0, 0));

                Err(ParseError::Eof(smallvec::smallvec![kind], span))
            }
        }
    }

    /// Get the nth token without consuming it.
    pub fn peek_n(&self, n: usize) -> ParseResult<Spanned<Token>> {
        match self.stream.get(*self.index + n) {
            Some(v) => Ok(*v),
            None => {
                let span = self
                    .stream
                    .get(self.index.saturating_sub(1))
                    .map(|x| x.span)
                    .unwrap_or(Span::new(0, 0));

                Err(ParseError::Eof(smallvec::smallvec![], span))
            }
        }
    }

    /// Execute the given parser and add a span to it.
    ///
    /// This function does not commit the result if `f` returns an error.
    pub fn spanning<F, T, E>(&mut self, f: F) -> Result<Spanned<T>, E>
    where
        for<'s2, 'i2> F: FnOnce(ParseGuard<'s2, 'i2>) -> Result<T, E>,
    {
        let mut index = *self.index;
        let guard = self.subguard(&mut index, None, None);

        let result = f(guard)?;

        let span = if *self.index == index {
            let next_token_span = self
                .stream
                .get(index)
                .map(|x| x.span)
                .unwrap_or(Span::new(0, 0));

            Span::new(next_token_span.start(), next_token_span.start())
        } else {
            self.stream[*self.index..index]
                .iter()
                .fold(self.stream[*self.index].span, |pre, t| pre.extend(t.span))
        };
        *self.index = index;

        Ok(Spanned::new(span, result))
    }

    /// Execute a parser and commit the result if it exits successfully.
    pub fn with<F, T, E>(&mut self, f: F) -> Result<T, E>
    where
        for<'s2, 'i2> F: FnOnce(ParseGuard<'s2, 'i2>) -> Result<T, E>,
    {
        let mut index = *self.index;
        let guard = self.subguard(&mut index, None, None);

        let result = f(guard)?;

        *self.index = index;
        Ok(result)
    }

    /// Parse a module from its source.
    ///
    /// Arguments:
    /// + `source`: the source of the module
    /// + `node`: the node ID of the module
    /// + `name`: the name of the module
    /// + `subdir`: whether the module is defined as `{module}/mod.rs` or `{module}.rs`
    pub fn parse_module(
        &mut self,
        source: SourceIdx,
        node: NodeId,
        name: Intern<str>,
        subdir: bool,
    ) -> ParseResult<()> {
        let mut index = *self.index;
        let lexed = Lexer::new(&self.sources, source, self.diagnostics.clone()).lex()?;

        let parent = Rc::clone(&self.module_tree);
        let mut guard = self.subguard(&mut index, Some(lexed.tokens()), Some(source));
        guard.module_tree = parent.child(name, subdir);

        let parse_result = guard.with(InlineModule::parse);
        if *guard.index != guard.stream.len() {
            return Err(ParseError::InputNotConsumed(self.peek()?.span));
        }

        self.ast_table.insert(source, node, parse_result?);

        Ok(())
    }
}

/// Paths.
pub mod path {
    use std::{fmt::Debug, ops::Deref};

    use lexer::{Intern, TokenKind};
    use span::Spanned;

    use crate::{KEYWORDS, Parse, ParseGuard, ParseResult};

    /// A path.
    #[derive(Debug, Clone, PartialEq)]
    pub struct Path {
        /// Each individual segment. In source, these are separated by `::`.
        pub segments: Vec<Spanned<Segment>>,
        /// Whether the path is fully qualified, i.e., *begins* with a `::`.
        pub is_fully_qualified: bool,
    }

    /// A path segment.
    #[derive(Clone, Copy, PartialEq)]
    pub struct Segment {
        // SAFETY: this is a pointer to an interned string
        ptr: *const u8,
        tagged_len: usize,
    }

    unsafe impl Send for Segment {}
    unsafe impl Sync for Segment {}

    impl Segment {
        /// The position of the `is_kw` bit.
        pub const SHIFT: usize = (usize::BITS - 1) as usize;
        /// The maximum size a segment can have.
        pub const MAX_SIZE: usize = 1 << Self::SHIFT;

        /// Create a new segment.
        pub fn new(segment: impl Into<Intern<str>>, is_kw: bool) -> Self {
            let segment = segment.into();
            debug_assert!(segment.len() < Self::MAX_SIZE);
            let ptr = segment.as_ptr();
            let len = segment.len();
            let tagged_len = if is_kw { len | 1 << Self::SHIFT } else { len };

            Self { ptr, tagged_len }
        }

        /// Whether this is a keyword or not.
        pub const fn is_kw(self) -> bool {
            (self.tagged_len as isize) < 0
        }

        /// Retrieve an interned representation of the underlying string.
        pub fn as_intern_str(&self) -> Intern<str> {
            const {
                assert!(size_of::<&'static str>() == size_of::<Intern<str>>());
            }

            // SAFETY: Intern<str> is simply a wrapper around &'static str. equality check occurs above.
            unsafe { core::mem::transmute(&**self) }
        }
    }

    impl Deref for Segment {
        type Target = str;

        #[inline]
        fn deref(&self) -> &Self::Target {
            let actual_len = self.tagged_len & !(1 << Self::SHIFT);
            unsafe { std::str::from_raw_parts(self.ptr, actual_len) }
        }
    }

    impl Debug for Segment {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("Segment")
                .field("segment", &self.deref())
                .field("is_kw", &self.is_kw())
                .finish()
        }
    }

    impl PartialEq<Intern<str>> for Segment {
        fn eq(&self, other: &Intern<str>) -> bool {
            self.ptr == other.as_ptr()
        }
    }

    impl PartialEq<Segment> for Intern<str> {
        fn eq(&self, other: &Segment) -> bool {
            other == self
        }
    }

    impl Path {
        /// Create a path that consists of only the first segment.
        #[cfg(test)]
        pub fn single(val: Spanned<Intern<str>>) -> Spanned<Self> {
            Spanned::new(
                val.span,
                Self {
                    segments: vec![val.map(|x| Segment::new(x, false))],
                    is_fully_qualified: false,
                },
            )
        }
    }

    impl Parse for Path {
        fn is_ok(&self) -> bool {
            self.segments.iter().all(|x| !x.item.is_kw())
        }

        fn parse<'source, 'index>(mut guard: ParseGuard<'source, 'index>) -> ParseResult<Self> {
            // using new guard so that it's atomic
            let is_fully_qualified = guard.spanning(consume_double_colon).is_ok();
            let first = guard.next_require(TokenKind::Ident)?.map(|x| x.symbol);
            let first_is_kw = KEYWORDS.iter().any(|x| ***x == first.item);
            let mut segments = vec![first.map(|x| Segment::new(x, first_is_kw))];

            loop {
                if guard.spanning(consume_double_colon).is_err() {
                    break;
                }

                let segment = guard.next_require(TokenKind::Ident)?.map(|x| x.symbol);
                let is_kw = KEYWORDS.iter().any(|x| ***x == segment.item);

                segments.push(segment.map(|x| Segment::new(x, is_kw)));
            }

            Ok(Self {
                segments,
                is_fully_qualified,
            })
        }
    }

    fn consume_double_colon(mut guard: ParseGuard) -> ParseResult<Spanned<()>> {
        guard
            .next_require(TokenKind::Colon)
            .and_then(|_| guard.next_require(TokenKind::Colon))
            .map(|x| x.map(|_| ()))
    }
}

/// A parser.
pub trait Parse: Sized {
    /// Attempt to parse an item.
    fn parse<'source, 'index>(guard: ParseGuard<'source, 'index>) -> ParseResult<Self>;

    /// Specify which state of this can be interpreted as a successfully parsed element.
    fn is_ok(&self) -> bool;
}

#[cfg(test)]
#[track_caller]
fn assert_eq<T>(input: impl Into<String>, other: Spanned<T>)
where
    T: Parse + PartialEq + std::fmt::Debug,
{
    assert_eq_custom_parser(T::parse, input, other.item, Some(other.span))
}

#[cfg(test)]
#[track_caller]
fn assert_eq_custom_parser<T, F, E>(clbk: F, input: impl Into<String>, other: T, span: Option<Span>)
where
    F: FnOnce(ParseGuard) -> Result<T, E>,
    T: PartialEq + std::fmt::Debug,
    E: std::fmt::Debug,
{
    use source::{SourceFile, SourceMap};

    let mut sources = SourceMap::new();
    let source_file = SourceFile::from_memory(input.into());
    let source_idx = sources.insert(source_file);
    let mut diagnostics = Diagnostics::default();

    let lexed = match Lexer::new(&sources, source_idx, diagnostics.clone()).lex() {
        Ok(r) => r,
        Err(_) => {
            eprintln!("Failed to lex input, diagnostics following: ");
            diagnostics.write_stderr(sources).unwrap();
            panic!()
        }
    };

    let ast_table = AstTable::default();
    let crate_table = CrateTable::default();
    let mut parser = Parser::new(
        sources.clone(),
        source_idx,
        diagnostics.clone(),
        ast_table.clone(),
        crate_table,
        Intern::from("test"),
    )
    .unwrap();
    let mut guard = parser.guard(lexed.tokens());

    // prob not the best way to do this
    if let Some(span) = span {
        let parse_res = guard.spanning(clbk);

        if *guard.index != lexed.tokens().len() {
            ParseError::InputNotConsumed(lexed.tokens().last().unwrap().span)
                .display(source_idx, diagnostics.clone());
        }

        let parse_res = match parse_res {
            Ok(v) => v,
            Err(e) => {
                eprintln!("Failed to parse input. Error: {e:#?}. Diagnostics: ");
                diagnostics.write_stdout(sources).unwrap();
                panic!();
            }
        };

        let other = Spanned::new(span, other);

        pretty_assertions::assert_eq!(other, parse_res)
    } else {
        let parsed = match guard.with(clbk) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("Failed to parse input. Error: {e:#?}. Diagnostics: ");
                diagnostics.write_stdout(sources).unwrap();
                panic!();
            }
        };

        if *guard.index != lexed.tokens().len() {
            diagnostics.push(Diagnostic::error(
                lexed
                    .tokens()
                    .first()
                    .unwrap()
                    .span
                    .extend(lexed.tokens().last().unwrap().span),
                "not all tokens were consumed",
                None,
                source_idx,
            ));
        }

        pretty_assertions::assert_eq!(other, parsed)
    };
}

#[cfg(test)]
mod tests {
    use lexer::Intern;
    use span::{Span, Spanned};

    use crate::{
        assert_eq,
        path::{Path, Segment},
    };

    #[test]
    fn parse_fq_path() {
        assert_eq(
            "::std::io",
            Spanned::new(
                Span::new(0, 9),
                Path {
                    is_fully_qualified: true,
                    segments: vec![
                        Spanned::new(Span::new(2, 5), Segment::new("std", false)),
                        Spanned::new(Span::new(7, 9), Segment::new("io", false)),
                    ],
                },
            ),
        );
    }

    #[test]
    fn parse_path() {
        assert_eq(
            "std::io",
            Spanned::new(
                Span::new(0, 7),
                Path {
                    is_fully_qualified: false,
                    segments: vec![
                        Spanned::new(Span::new(0, 3), Segment::new("std", false)),
                        Spanned::new(Span::new(5, 7), Segment::new("io", false)),
                    ],
                },
            ),
        )
    }

    #[test]
    fn parse_single_path() {
        assert_eq(
            "tmp",
            Path::single(Spanned::new(Span::new(0, 3), Intern::from("tmp"))),
        );
    }
}
