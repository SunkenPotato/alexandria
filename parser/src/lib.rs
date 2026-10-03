//! The parser for alexandria. See [`Parser`] for the entrypoint.

pub mod expr;
pub mod item;
pub mod stmt;

use std::rc::Rc;

use diagnostic::{Diagnostic, Diagnostics};
use lexer::{Intern, LexError, Lexer, Token, TokenKind};
use node::NodeId;
use smallvec::SmallVec;
use source::{LoadedModule, ModuleTree, SOURCE_EXTENSION, SourceFileError, SourceIdx, SourceMap};
use span::{Span, Spanned};

use crate::{
    ast_table::AstTable,
    crate_table::{CrateId, CrateTable},
    item::InlineModule,
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

/// Check whether the given symbol is a keyword.
pub fn is_keyword(symbol: Intern<str>) -> bool {
    KEYWORDS.iter().any(|kw| ***kw == symbol)
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

        /// Check whether the AST of the given file is in the table.
        pub fn contains(&self, source: SourceIdx) -> bool {
            self.raw.source_map.contains_key(&source)
        }

        /// Check whether the table is empty.
        pub fn is_empty(&self) -> bool {
            self.raw.source_map.is_empty()
        }
    }
}

/// Implementation of the [`CrateTable`].
///
/// Crates are parsed sequentially: every crate is registered up front, `include crate` requests
/// a crate, and the driver parses requested crates until none are left
/// (see [`CrateTable::next_requested`]).
pub mod crate_table {
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex, MutexGuard},
    };

    use index_vec::IndexVec;
    use lexer::Intern;
    use source::SourceIdx;

    index_vec::define_index_type! {
        /// A unique identifier for a crate.
        pub struct CrateId = u32;
    }

    /// The parse state of a crate.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum CrateState {
        /// The crate is known, but nothing has requested it yet.
        Registered,
        /// The crate has been requested, but not parsed yet.
        Requested,
        /// The crate is being parsed.
        Parsing,
        /// The crate was parsed successfully.
        Parsed,
        /// Parsing the crate failed.
        Failed,
    }

    /// Information about a crate.
    #[derive(Clone, Debug)]
    pub struct CrateEntry {
        /// The name of the crate.
        pub name: Intern<str>,
        /// The entrypoint file of the crate.
        pub root: SourceIdx,
        /// The parse state of the crate.
        pub state: CrateState,
    }

    /// A registry for crates.
    #[derive(Clone, Debug, Default)]
    pub struct CrateTable {
        raw: Arc<Mutex<CrateTableRaw>>,
    }

    #[derive(Debug, Default)]
    struct CrateTableRaw {
        crates: IndexVec<CrateId, CrateEntry>,
        names: HashMap<Intern<str>, CrateId>,
    }

    impl CrateTable {
        fn lock(&self) -> MutexGuard<'_, CrateTableRaw> {
            self.raw.lock().unwrap_or_else(|poison| poison.into_inner())
        }

        /// Register a crate. Returns [`None`] if a crate with the same name already exists.
        pub fn insert(&self, name: Intern<str>, root: SourceIdx) -> Option<CrateId> {
            let mut raw = self.lock();

            if raw.names.contains_key(&name) {
                return None;
            }

            let id = raw.crates.push(CrateEntry {
                name,
                root,
                state: CrateState::Registered,
            });
            raw.names.insert(name, id);
            Some(id)
        }

        /// Retrieve the ID of a crate by its name.
        pub fn id_by_name(&self, name: Intern<str>) -> Option<CrateId> {
            self.lock().names.get(&name).copied()
        }

        /// Retrieve information about a crate.
        pub fn get(&self, id: CrateId) -> CrateEntry {
            self.lock().crates[id].clone()
        }

        /// Request a crate to be parsed. This does nothing if the crate was already requested.
        pub fn request(&self, id: CrateId) {
            let entry = &mut self.lock().crates[id];
            if entry.state == CrateState::Registered {
                entry.state = CrateState::Requested;
            }
        }

        /// Take the next requested crate and mark it as being parsed.
        pub fn next_requested(&self) -> Option<CrateId> {
            let mut raw = self.lock();
            let (id, entry) = raw
                .crates
                .iter_mut_enumerated()
                .find(|(_, x)| x.state == CrateState::Requested)?;

            entry.state = CrateState::Parsing;
            Some(id)
        }

        /// Mark a crate as finished.
        pub fn finish(&self, id: CrateId, success: bool) {
            self.lock().crates[id].state = if success {
                CrateState::Parsed
            } else {
                CrateState::Failed
            };
        }
    }
}

/// A parser error. Most errors are expressed via diagnostics instead of this
#[derive(Clone, Debug)]
pub enum ParseError {
    /// Expected one of the given tokens at the specified location.
    TokenMismatch(SmallVec<[TokenKind; 6]>, Span),
    /// The end of the input was reached. The variant stores the expected tokens and the location.
    Eof(SmallVec<[TokenKind; 3]>, Span),
    /// Expected the keyword at that location.
    ExpectedKw(Intern<str>, Span),
    /// A keyword was used where an identifier was expected.
    KwAsIdent(Intern<str>, Span),
    /// Expected an item at that location.
    ExpectedItem(Span),
    /// An error was produced by the lexer/tokenizer.
    LexError(LexError),
    /// The entire input was not consumed fully.
    ///
    /// The span points to the first token that was not consumed.
    InputNotConsumed(Span),
}

impl From<LexError> for ParseError {
    fn from(value: LexError) -> Self {
        Self::LexError(value)
    }
}

impl ParseError {
    /// Convert this error into a diagnostic and add it to the given pool.
    pub fn display(&self, source: SourceIdx, diag: &Diagnostics) {
        let (span, msg) = match self {
            Self::ExpectedKw(kw, span) => (span, format!("expected keyword `{kw}`")),
            Self::KwAsIdent(kw, span) => (
                span,
                format!("expected an identifier, found keyword `{kw}`"),
            ),
            Self::ExpectedItem(span) => (
                span,
                "expected an item (`func`, `product`, `sum`, `module`, `const`, `static`, \
                 `import` or `include`)"
                    .to_owned(),
            ),
            Self::TokenMismatch(tokens, span) => (span, format!("expected {}", describe(tokens))),
            Self::Eof(tokens, span) if tokens.is_empty() => {
                (span, "unexpected end of file".to_owned())
            }
            Self::Eof(tokens, span) => (
                span,
                format!("expected {}, found end of file", describe(tokens)),
            ),
            Self::InputNotConsumed(span) => (span, "unexpected token".to_owned()),
            // the lexer already pushes diagnostics
            Self::LexError(_) => return,
        };

        diag.push(Diagnostic::error(*span, msg, None, source));
    }
}

fn describe(tokens: &[TokenKind]) -> String {
    match tokens {
        [] => "a token".to_owned(),
        [one] => one.to_string(),
        [init @ .., last] => {
            let init: Vec<_> = init.iter().map(ToString::to_string).collect();
            format!("one of {} or {last}", init.join(", "))
        }
    }
}

/// State shared by every parser of a compilation.
#[derive(Clone, Debug)]
struct ParseContext {
    diagnostics: Diagnostics,
    sources: SourceMap,
    ast_table: AstTable,
    crate_table: CrateTable,
}

impl ParseContext {
    /// Lex and parse an entire file.
    fn parse_source(
        &self,
        source_idx: SourceIdx,
        module_tree: Option<Rc<ModuleTree>>,
    ) -> ParseResult<InlineModule> {
        let lexed = Lexer::new(&self.sources, source_idx, self.diagnostics.clone()).lex()?;
        let mut index = 0;
        let mut guard = ParseGuard {
            ctx: self.clone(),
            index: &mut index,
            stream: lexed.tokens(),
            source_idx,
            module_tree,
        };

        let module = guard.with(InlineModule::parse)?;

        if let Ok(token) = guard.peek() {
            return Err(ParseError::InputNotConsumed(token.span));
        }

        Ok(module)
    }
}

/// The parser. See [`Parser::parse`].
pub struct Parser {
    crate_id: CrateId,
    entrypoint: SourceIdx,
    ctx: ParseContext,
}

impl Parser {
    /// Create a new parser to parse the given crate.
    pub fn new(
        sources: SourceMap,
        crate_id: CrateId,
        diagnostics: Diagnostics,
        ast_table: AstTable,
        crate_table: CrateTable,
    ) -> Self {
        let entrypoint = crate_table.get(crate_id).root;

        Self {
            crate_id,
            entrypoint,
            ctx: ParseContext {
                diagnostics,
                sources,
                ast_table,
                crate_table,
            },
        }
    }

    /// Parse the crate. This inserts the result into the AST table, reports errors as
    /// diagnostics and marks the crate as finished in the crate table.
    ///
    /// Returns whether parsing succeeded.
    pub fn parse(self) -> bool {
        let module_tree = self.ctx.sources[self.entrypoint]
            .source()
            .and_then(std::path::Path::parent)
            .map(|dir| ModuleTree::new(dir.to_owned()));

        let success = match self.ctx.parse_source(self.entrypoint, module_tree) {
            Ok(module) => {
                self.ctx
                    .ast_table
                    .insert(self.entrypoint, NodeId::new(), module);
                true
            }
            Err(e) => {
                e.display(self.entrypoint, &self.ctx.diagnostics);
                false
            }
        };

        self.ctx.crate_table.finish(self.crate_id, success);
        success
    }
}

/// A parser guard for parsing a specific element.
#[derive(Debug)]
pub struct ParseGuard<'s, 'i> {
    ctx: ParseContext,
    index: &'i mut usize,
    stream: &'s [Spanned<Token>],
    source_idx: SourceIdx,
    /// The module tree of the current file. This is [`None`] for in-memory sources.
    module_tree: Option<Rc<ModuleTree>>,
}

impl<'s> ParseGuard<'s, '_> {
    fn subguard<'i2>(&self, index: &'i2 mut usize) -> ParseGuard<'s, 'i2> {
        ParseGuard {
            ctx: self.ctx.clone(),
            index,
            stream: self.stream,
            source_idx: self.source_idx,
            module_tree: self.module_tree.clone(),
        }
    }

    /// Emit a diagnostic in the current file.
    fn emit(&self, diagnostic: Diagnostic) {
        self.ctx.diagnostics.push(diagnostic);
    }

    /// The span used for errors at the end of the input.
    fn eof_span(&self) -> Span {
        self.stream
            .get(self.index.saturating_sub(1))
            .or(self.stream.last())
            .map(|x| Span::new(x.span.stop(), x.span.stop()))
            .unwrap_or(Span::new(0, 0))
    }

    fn eof(&self, expected: SmallVec<[TokenKind; 3]>) -> ParseError {
        ParseError::Eof(expected, self.eof_span())
    }

    /// Consume a token.
    #[expect(clippy::should_implement_trait)]
    pub fn next(&mut self) -> ParseResult<Spanned<Token>> {
        let token = self.peek()?;
        *self.index += 1;
        Ok(token)
    }

    /// Get the next token without consuming it.
    pub fn peek(&self) -> ParseResult<Spanned<Token>> {
        self.peek_n(0)
    }

    /// Get the nth token without consuming it.
    pub fn peek_n(&self, n: usize) -> ParseResult<Spanned<Token>> {
        self.stream
            .get(*self.index + n)
            .copied()
            .ok_or_else(|| self.eof(SmallVec::new()))
    }

    /// Consume the next token if it is of the given kind.
    pub fn next_require(&mut self, kind: TokenKind) -> ParseResult<Spanned<Token>> {
        let token = self.peek_require(kind)?;
        *self.index += 1;
        Ok(token)
    }

    /// See [`Self::peek`] and [`Self::next_require`].
    pub fn peek_require(&self, kind: TokenKind) -> ParseResult<Spanned<Token>> {
        match self.stream.get(*self.index) {
            Some(v) if v.item.kind == kind => Ok(*v),
            Some(v) => Err(ParseError::TokenMismatch(smallvec::smallvec![kind], v.span)),
            None => Err(self.eof(smallvec::smallvec![kind])),
        }
    }

    /// Check whether the next token is of the given kind.
    pub fn peek_kind(&self, kind: TokenKind) -> bool {
        self.peek_require(kind).is_ok()
    }

    /// Check whether the nth token is the given keyword.
    pub fn peek_n_kw(&self, n: usize, kw: Intern<str>) -> bool {
        self.peek_n(n)
            .is_ok_and(|x| x.item.kind == TokenKind::Ident && x.item.symbol == kw)
    }

    /// Check whether the next token is the given keyword.
    pub fn peek_kw(&self, kw: Intern<str>) -> bool {
        self.peek_n_kw(0, kw)
    }

    /// Consume the given keyword.
    pub fn expect_kw(&mut self, kw: Intern<str>) -> ParseResult<Span> {
        match self.peek() {
            Ok(token) if token.item.kind == TokenKind::Ident && token.item.symbol == kw => {
                *self.index += 1;
                Ok(token.span)
            }
            Ok(token) => Err(ParseError::ExpectedKw(kw, token.span)),
            Err(_) => Err(self.eof(smallvec::smallvec![TokenKind::Ident])),
        }
    }

    /// Consume an identifier that is not a keyword.
    pub fn expect_ident(&mut self) -> ParseResult<Spanned<Intern<str>>> {
        let token = self.peek_require(TokenKind::Ident)?;
        if is_keyword(token.item.symbol) {
            return Err(ParseError::KwAsIdent(token.item.symbol, token.span));
        }

        *self.index += 1;
        Ok(token.map(|x| x.symbol))
    }

    /// Consume the next token if it is of the given kind and directly follows `prev` (i.e.,
    /// without any whitespace in between). Used for multi-character operators.
    pub fn next_adjacent(&mut self, prev: Span, kind: TokenKind) -> Option<Spanned<Token>> {
        let token = self.peek_require(kind).ok()?;
        if token.span.start() != prev.stop() {
            return None;
        }

        *self.index += 1;
        Some(token)
    }

    /// Parse a list of `item`s separated by commas and delimited by `open` and `close`.
    /// A trailing comma is allowed.
    pub fn parse_delimited<T, F>(
        &mut self,
        open: TokenKind,
        close: TokenKind,
        mut item: F,
    ) -> ParseResult<Vec<Spanned<T>>>
    where
        F: for<'i2> FnMut(ParseGuard<'s, 'i2>) -> ParseResult<T>,
    {
        self.next_require(open)?;
        let mut items = vec![];

        loop {
            if self.next_require(close).is_ok() {
                break;
            }

            items.push(self.spanning(&mut item)?);

            match self.peek() {
                Ok(t) if t.item.kind == TokenKind::Comma => *self.index += 1,
                Ok(t) if t.item.kind == close => {
                    *self.index += 1;
                    break;
                }
                Ok(t) => {
                    return Err(ParseError::TokenMismatch(
                        smallvec::smallvec![TokenKind::Comma, close],
                        t.span,
                    ));
                }
                Err(_) => return Err(self.eof(smallvec::smallvec![TokenKind::Comma, close])),
            }
        }

        Ok(items)
    }

    /// Execute the given parser and add a span to it.
    ///
    /// If `f` returns an error, neither the consumed tokens nor the diagnostics it emitted are
    /// kept.
    pub fn spanning<F, T, E>(&mut self, f: F) -> Result<Spanned<T>, E>
    where
        F: for<'i2> FnOnce(ParseGuard<'s, 'i2>) -> Result<T, E>,
    {
        let start = *self.index;
        let mut index = start;
        let diag_len = self.ctx.diagnostics.len();

        let result = match f(self.subguard(&mut index)) {
            Ok(v) => v,
            Err(e) => {
                self.ctx.diagnostics.cull(diag_len);
                return Err(e);
            }
        };

        let span = if index == start {
            let pos = self
                .stream
                .get(start)
                .map(|x| x.span.start())
                .unwrap_or(self.eof_span().stop());

            Span::new(pos, pos)
        } else {
            self.stream[start].span.extend(self.stream[index - 1].span)
        };
        *self.index = index;

        Ok(Spanned::new(span, result))
    }

    /// Execute a parser and commit the result if it exits successfully.
    ///
    /// If `f` returns an error, neither the consumed tokens nor the diagnostics it emitted are
    /// kept.
    pub fn with<F, T, E>(&mut self, f: F) -> Result<T, E>
    where
        F: for<'i2> FnOnce(ParseGuard<'s, 'i2>) -> Result<T, E>,
    {
        self.spanning(f).map(|x| x.item)
    }

    /// Load and parse the file of the module `ident` (declared by `include ident;`).
    ///
    /// Errors are reported as diagnostics.
    fn load_module(&self, ident: Spanned<Intern<str>>, node: NodeId) {
        let error = |msg: String| {
            self.emit(Diagnostic::error(ident.span, msg, None, self.source_idx));
        };

        let Some(module_tree) = &self.module_tree else {
            return error(format!(
                "cannot include module `{}` from a source that is not on disk",
                ident.item
            ));
        };

        match self.ctx.sources.load_from_mod(module_tree, &ident.item) {
            Ok(LoadedModule {
                idx,
                subdir,
                fresh: true,
            }) => {
                let tree = module_tree.child(ident.item, subdir);
                let module = self.ctx.parse_source(idx, Some(tree)).unwrap_or_else(|e| {
                    e.display(idx, &self.ctx.diagnostics);
                    InlineModule::default()
                });

                self.ctx.ast_table.insert(idx, node, module);
            }
            Ok(LoadedModule { fresh: false, .. }) => error(format!(
                "the file of module `{}` is already part of the compilation",
                ident.item
            )),
            Err(SourceFileError::NoMatches) => error(format!(
                "failed to find file `{name}.{ext}` or `{name}/mod.{ext}`",
                name = ident.item,
                ext = SOURCE_EXTENSION,
            )),
            Err(e) => error(format!(
                "failed to read source for module `{}`: {e}",
                ident.item
            )),
        }
    }
}

/// Paths.
pub mod path {
    use std::ops::Deref;

    use lexer::{Intern, TokenKind};
    use span::Spanned;

    use crate::{CRATE, Parse, ParseError, ParseGuard, ParseResult, SUPER, is_keyword};

    /// A path.
    #[derive(Debug, Clone, PartialEq)]
    pub struct Path {
        /// Each individual segment. In source, these are separated by `::`.
        pub segments: Vec<Spanned<Segment>>,
        /// Whether the path is fully qualified, i.e., *begins* with a `::`.
        pub is_fully_qualified: bool,
    }

    /// A path segment.
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
    pub struct Segment {
        name: Intern<str>,
        is_kw: bool,
    }

    impl Segment {
        /// Create a new segment.
        pub fn new(segment: impl Into<Intern<str>>, is_kw: bool) -> Self {
            Self {
                name: segment.into(),
                is_kw,
            }
        }

        /// Whether this is a keyword (`crate` or `super`) or not.
        pub const fn is_kw(self) -> bool {
            self.is_kw
        }

        /// Retrieve an interned representation of the underlying string.
        pub const fn as_intern_str(&self) -> Intern<str> {
            self.name
        }
    }

    impl Deref for Segment {
        type Target = str;

        #[inline]
        fn deref(&self) -> &Self::Target {
            &self.name
        }
    }

    impl PartialEq<Intern<str>> for Segment {
        fn eq(&self, other: &Intern<str>) -> bool {
            self.name == *other
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
        fn parse<'source, 'index>(mut guard: ParseGuard<'source, 'index>) -> ParseResult<Self> {
            let is_fully_qualified = guard.with(consume_double_colon).is_ok();
            let mut segments = vec![];

            loop {
                let token = guard.next_require(TokenKind::Ident)?;
                let name = token.item.symbol;
                let is_kw = is_keyword(name);

                // only `crate` and `super` may be used, and only as the first segment
                let kw_allowed = segments.is_empty()
                    && !is_fully_qualified
                    && (name == *CRATE || name == *SUPER);

                if is_kw && !kw_allowed {
                    return Err(ParseError::KwAsIdent(name, token.span));
                }

                segments.push(Spanned::new(token.span, Segment::new(name, is_kw)));

                if guard.with(consume_double_colon).is_err() {
                    break;
                }
            }

            Ok(Self {
                segments,
                is_fully_qualified,
            })
        }
    }

    fn consume_double_colon(mut guard: ParseGuard) -> ParseResult<()> {
        let first = guard.next_require(TokenKind::Colon)?;
        guard
            .next_adjacent(first.span, TokenKind::Colon)
            .map(|_| ())
            .ok_or(ParseError::TokenMismatch(
                smallvec::smallvec![TokenKind::Colon],
                first.span,
            ))
    }
}

/// A parser.
pub trait Parse: Sized {
    /// Attempt to parse an item.
    fn parse<'source, 'index>(guard: ParseGuard<'source, 'index>) -> ParseResult<Self>;
}

#[cfg(test)]
#[track_caller]
fn assert_eq<T>(input: impl Into<String>, other: Spanned<T>)
where
    T: Parse + PartialEq + std::fmt::Debug,
{
    assert_eq_custom_parser(T::parse, input, other.item, Some(other.span))
}

/// Parse `input` with `clbk`, and assert that all input was consumed and that the result equals
/// `other` (and its span equals `span`, if given).
#[cfg(test)]
#[track_caller]
fn assert_eq_custom_parser<T, F, E>(clbk: F, input: impl Into<String>, other: T, span: Option<Span>)
where
    F: FnOnce(ParseGuard) -> Result<T, E>,
    T: PartialEq + std::fmt::Debug,
    E: std::fmt::Debug,
{
    use source::SourceFile;

    let sources = SourceMap::new();
    let source_idx = sources.insert(SourceFile::from_memory(input.into()));
    let diagnostics = Diagnostics::default();

    let Ok(lexed) = Lexer::new(&sources, source_idx, diagnostics.clone()).lex() else {
        diagnostics.write_stderr(&sources).unwrap();
        panic!("failed to lex input");
    };

    let mut index = 0;
    let mut guard = ParseGuard {
        ctx: ParseContext {
            diagnostics: diagnostics.clone(),
            sources: sources.clone(),
            ast_table: AstTable::default(),
            crate_table: CrateTable::default(),
        },
        index: &mut index,
        stream: lexed.tokens(),
        source_idx,
        module_tree: None,
    };

    let parsed = match guard.spanning(clbk) {
        Ok(v) => v,
        Err(e) => {
            diagnostics.write_stderr(&sources).unwrap();
            panic!("failed to parse input: {e:#?}");
        }
    };

    assert_eq!(
        *guard.index,
        lexed.tokens().len(),
        "not all tokens were consumed"
    );

    pretty_assertions::assert_eq!(other, parsed.item);
    if let Some(span) = span {
        pretty_assertions::assert_eq!(span, parsed.span);
    }
}

/// Parse `input` with `clbk` and return the error it fails with.
#[cfg(test)]
#[track_caller]
fn parse_err<T, F>(clbk: F, input: &str) -> (ParseError, Vec<String>)
where
    F: FnOnce(ParseGuard) -> ParseResult<T>,
    T: std::fmt::Debug,
{
    use source::SourceFile;

    let sources = SourceMap::new();
    let source_idx = sources.insert(SourceFile::from_memory(input.to_owned()));
    let diagnostics = Diagnostics::default();
    let lexed = Lexer::new(&sources, source_idx, diagnostics.clone())
        .lex()
        .unwrap();

    let mut index = 0;
    let mut guard = ParseGuard {
        ctx: ParseContext {
            diagnostics: diagnostics.clone(),
            sources: sources.clone(),
            ast_table: AstTable::default(),
            crate_table: CrateTable::default(),
        },
        index: &mut index,
        stream: lexed.tokens(),
        source_idx,
        module_tree: None,
    };

    let err = guard.with(clbk).expect_err("parse should fail");
    err.display(source_idx, &diagnostics);
    let mut out = vec![];
    diagnostics.write(&sources, &mut out).unwrap();
    let rendered = String::from_utf8(out).unwrap();

    (err, rendered.lines().map(ToOwned::to_owned).collect())
}

#[cfg(test)]
mod tests {
    use lexer::Intern;
    use span::{Span, Spanned};

    use crate::{
        ParseError, assert_eq, parse_err,
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

    #[test]
    fn parse_super_path() {
        assert_eq(
            "super::x",
            Spanned::new(
                Span::new(0, 8),
                Path {
                    is_fully_qualified: false,
                    segments: vec![
                        Spanned::new(Span::new(0, 5), Segment::new("super", true)),
                        Spanned::new(Span::new(7, 8), Segment::new("x", false)),
                    ],
                },
            ),
        );
    }

    #[test]
    fn keyword_in_path_is_rejected() {
        let (err, _) = parse_err(<Path as crate::Parse>::parse, "a::loop");
        assert!(matches!(err, ParseError::KwAsIdent(..)), "{err:?}");
    }
}
