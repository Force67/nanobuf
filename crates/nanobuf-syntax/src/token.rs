use nanobuf_diagnostics::Span;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TokenKind {
    Ident,
    Int,
    Str,
    DocComment,

    Schema,
    Import,
    Message,
    Enum,
    Reserved,

    Colon,
    Comma,
    Dot,
    Question,
    At,
    Eq,
    Minus,
    Semi,
    LBrace,
    RBrace,
    Lt,
    Gt,
    LBracket,
    RBracket,

    Newline,
    Eof,
}

impl TokenKind {
    /// Human-readable name for error messages.
    pub fn describe(self) -> &'static str {
        match self {
            TokenKind::Ident => "an identifier",
            TokenKind::Int => "an integer",
            TokenKind::Str => "a string literal",
            TokenKind::DocComment => "a doc comment",
            TokenKind::Schema => "`schema`",
            TokenKind::Import => "`import`",
            TokenKind::Message => "`message`",
            TokenKind::Enum => "`enum`",
            TokenKind::Reserved => "`reserved`",
            TokenKind::Colon => "`:`",
            TokenKind::Comma => "`,`",
            TokenKind::Dot => "`.`",
            TokenKind::Question => "`?`",
            TokenKind::At => "`@`",
            TokenKind::Eq => "`=`",
            TokenKind::Minus => "`-`",
            TokenKind::Semi => "`;`",
            TokenKind::LBrace => "`{`",
            TokenKind::RBrace => "`}`",
            TokenKind::Lt => "`<`",
            TokenKind::Gt => "`>`",
            TokenKind::LBracket => "`[`",
            TokenKind::RBracket => "`]`",
            TokenKind::Newline => "end of line",
            TokenKind::Eof => "end of file",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}
