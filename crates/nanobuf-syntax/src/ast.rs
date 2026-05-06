//! Syntax tree produced by the parser. Names are unresolved; every node
//! carries the spans needed for later diagnostics.

use std::fmt;

use nanobuf_diagnostics::Span;

#[derive(Clone, Debug, PartialEq)]
pub struct Ident {
    pub text: String,
    pub span: Span,
}

/// Dotted name such as `common.Header`.
#[derive(Clone, Debug, PartialEq)]
pub struct Path {
    pub segments: Vec<Ident>,
    pub span: Span,
}

impl Path {
    pub fn dotted(&self) -> String {
        self.segments
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join(".")
    }
}

#[derive(Debug, PartialEq)]
pub struct File {
    pub package: Path,
    pub imports: Vec<Import>,
    pub decls: Vec<Decl>,
}

#[derive(Debug, PartialEq)]
pub struct Import {
    pub path: String,
    pub span: Span,
}

#[derive(Debug, PartialEq)]
pub enum Decl {
    Message(MessageDecl),
    Enum(EnumDecl),
}

#[derive(Debug, PartialEq)]
pub struct MessageDecl {
    pub docs: Vec<String>,
    pub name: Ident,
    pub fields: Vec<FieldDecl>,
    pub reserved: Vec<ReservedId>,
    pub span: Span,
}

#[derive(Debug, PartialEq)]
pub struct ReservedId {
    pub id: u32,
    pub span: Span,
}

#[derive(Debug, PartialEq)]
pub struct FieldDecl {
    pub docs: Vec<String>,
    pub name: Ident,
    pub ty: TypeExpr,
    pub id: u32,
    pub id_span: Span,
    /// Proto3 wire encoding override from a `[proto = ...]` attribute.
    pub proto: Option<Ident>,
    pub span: Span,
}

#[derive(Debug, PartialEq)]
pub enum TypeExpr {
    Named(Path),
    List { elem: Box<TypeExpr>, span: Span },
    Optional { inner: Box<TypeExpr>, span: Span },
}

impl TypeExpr {
    pub fn span(&self) -> Span {
        match self {
            TypeExpr::Named(path) => path.span,
            TypeExpr::List { span, .. } | TypeExpr::Optional { span, .. } => *span,
        }
    }
}

impl fmt::Display for TypeExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TypeExpr::Named(path) => write!(f, "{}", path.dotted()),
            TypeExpr::List { elem, .. } => write!(f, "list<{elem}>"),
            TypeExpr::Optional { inner, .. } => write!(f, "{inner}?"),
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct EnumDecl {
    pub docs: Vec<String>,
    pub name: Ident,
    /// Backing type name, e.g. `u8`. Defaults to `u8` when absent.
    pub backing: Option<Ident>,
    pub variants: Vec<VariantDecl>,
    pub span: Span,
}

#[derive(Debug, PartialEq)]
pub struct VariantDecl {
    pub docs: Vec<String>,
    pub name: Ident,
    pub value: i64,
    pub value_span: Span,
    pub span: Span,
}
