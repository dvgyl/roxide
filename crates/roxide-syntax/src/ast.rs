//! Heap-free AST mirroring R language objects 1:1.

use std::ops::Range;

use roxide_diag::Span;

/// Index into an [`Ast`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct NodeId(pub u32);

/// String literal encoding tag produced by the lexer.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum StrEncoding {
    /// Pure ASCII or native encoding.
    Native,
    /// Explicitly UTF-8 (`\u`/`\U` escape or non-ASCII UTF-8 source).
    Utf8,
    /// Raw byte escapes (`\x` / octal with high bytes).
    Bytes,
}

/// Literal constant value in an AST node.
#[derive(Clone, PartialEq, Debug)]
pub enum Const {
    /// `NULL`
    Null,
    /// Logical literal (`TRUE` -> `Some(true)`, `FALSE` -> `Some(false)`, `NA` -> `None`).
    Lgl(Option<bool>),
    /// Integer literal (`1L`, `NA_integer_`). `i32::MIN` represents `NA_integer_`.
    Int(i32),
    /// Double literal (`1.5`, `Inf`, `NaN`, `NA_real_`). Exact raw IEEE 754 bits preserved.
    Real(u64),
    /// Complex literal (`2i`, `NA_complex_`). Real and imaginary IEEE 754 bit patterns.
    Complex {
        /// Real part bit pattern.
        re_bits: u64,
        /// Imaginary part bit pattern.
        im_bits: u64,
    },
    /// Character string literal (`"..."`, `'...'`, `r"(...)"`).
    Str {
        /// Raw decoded bytes (without trailing NUL).
        bytes: Box<[u8]>,
        /// Encoding tag.
        enc: StrEncoding,
    },
    /// `NA_character_` literal.
    NaChar,
}

/// Name tag on a call argument or formal parameter.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ArgName {
    /// Syntactic or backtick-quoted symbol name.
    Sym(Box<str>),
    /// String-quoted argument tag (`f("a" = 1)`).
    Str(Box<str>),
    /// `NULL = ...` argument tag.
    Null,
}

/// A single argument in a call or formal parameter in a `function` definition.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Arg {
    /// Optional tag (`name = value`).
    pub name: Option<ArgName>,
    /// Optional value (`None` for an empty slot like `x[, 1]` or a formal without default).
    pub value: Option<NodeId>,
    /// Source span of this argument.
    pub span: Span,
}

/// One node in an [`Ast`].
#[derive(Clone, PartialEq, Debug)]
pub enum Node {
    /// Literal constant.
    Const(Const),
    /// Identifier / symbol (`x`, `` `a b` ``, `+`).
    Sym(Box<str>),
    /// Empty argument placeholder (`R_MissingArg`).
    Empty,
    /// Function/operator/control-flow call (`func(args...)`).
    Call {
        /// Callee expression (often a symbol like `+`, `<-`, `if`, `{`, `(`).
        func: NodeId,
        /// Slice range into [`Ast::args`].
        args: Range<u32>,
    },
    /// Function definition (`function(formals) body`, also target of `\(...)`).
    Function {
        /// Formal parameters slice range into [`Ast::args`].
        formals: Range<u32>,
        /// Function body expression.
        body: NodeId,
        /// Full source span (for 4th element `srcref`).
        span: Span,
    },
}

/// Arena storing all AST nodes, spans, and argument lists for a parsed unit.
#[derive(Clone, Default, Debug)]
pub struct Ast {
    nodes: Vec<Node>,
    spans: Vec<Span>,
    args: Vec<Arg>,
}

impl Ast {
    /// Create an empty [`Ast`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Push a node with its source [`Span`] and return its [`NodeId`].
    pub fn push_node(&mut self, node: Node, span: Span) -> NodeId {
        let id = NodeId(self.nodes.len() as u32);
        self.nodes.push(node);
        self.spans.push(span);
        id
    }

    /// Append a batch of [`Arg`] entries and return their index range.
    pub fn push_args(&mut self, args: impl IntoIterator<Item = Arg>) -> Range<u32> {
        let start = self.args.len() as u32;
        self.args.extend(args);
        let end = self.args.len() as u32;
        start..end
    }

    /// Borrow a node by [`NodeId`].
    #[must_use]
    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.0 as usize]
    }

    /// Return the [`Span`] associated with a [`NodeId`].
    #[must_use]
    pub fn span(&self, id: NodeId) -> Span {
        self.spans[id.0 as usize]
    }

    /// Borrow an argument slice by range.
    #[must_use]
    pub fn args(&self, range: Range<u32>) -> &[Arg] {
        &self.args[range.start as usize..range.end as usize]
    }

    pub(crate) fn replace_arg(&mut self, index: u32, value: NodeId) {
        let span = self.span(value);
        let arg = &mut self.args[index as usize];
        arg.value = Some(value);
        if arg.name.is_none() {
            arg.span = span;
        }
    }

    pub(crate) fn set_span(&mut self, id: NodeId, span: Span) {
        self.spans[id.0 as usize] = span;
    }
}
