use nanobuf_diagnostics::{Diagnostic, Diagnostics, FileId, Span};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TokenKind {
    Ident,
    Int,
    Str,
    Eq,
    Semi,
    Comma,
    Dot,
    Minus,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Lt,
    Gt,
    LParen,
    RParen,
    Eof,
}

#[derive(Clone, Copy, Debug)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

/// Tokenizes proto3 source. Skips `//` and `/* */` comments.
pub fn lex(file: FileId, text: &str, diags: &mut Diagnostics) -> Vec<Token> {
    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    let mut pos = 0;
    while pos < bytes.len() {
        let start = pos;
        match bytes[pos] {
            b' ' | b'\t' | b'\r' | b'\n' => pos += 1,
            b'/' if bytes.get(pos + 1) == Some(&b'/') => {
                while pos < bytes.len() && bytes[pos] != b'\n' {
                    pos += 1;
                }
            }
            b'/' if bytes.get(pos + 1) == Some(&b'*') => {
                pos += 2;
                while pos + 1 < bytes.len() && &bytes[pos..pos + 2] != b"*/" {
                    pos += 1;
                }
                if pos + 1 >= bytes.len() {
                    diags.push(
                        Diagnostic::error("E0201", "unterminated block comment").with_label(
                            file,
                            Span::new(start as u32, start as u32 + 2),
                            "opened here",
                        ),
                    );
                    break;
                }
                pos += 2;
            }
            b'"' | b'\'' => {
                let quote = bytes[pos];
                pos += 1;
                while pos < bytes.len() && bytes[pos] != quote && bytes[pos] != b'\n' {
                    pos += if bytes[pos] == b'\\' { 2 } else { 1 };
                }
                if pos < bytes.len() && bytes[pos] == quote {
                    pos += 1;
                    tokens.push(token(TokenKind::Str, start, pos));
                } else {
                    diags.push(
                        Diagnostic::error("E0202", "unterminated string literal").with_label(
                            file,
                            Span::new(start as u32, pos as u32),
                            "opened here",
                        ),
                    );
                }
            }
            b'0'..=b'9' => {
                while pos < bytes.len()
                    && (bytes[pos].is_ascii_alphanumeric() || bytes[pos] == b'.')
                {
                    pos += 1;
                }
                tokens.push(token(TokenKind::Int, start, pos));
            }
            b'A'..=b'Z' | b'a'..=b'z' | b'_' => {
                while pos < bytes.len()
                    && (bytes[pos].is_ascii_alphanumeric() || bytes[pos] == b'_')
                {
                    pos += 1;
                }
                tokens.push(token(TokenKind::Ident, start, pos));
            }
            b => {
                pos += 1;
                let kind = match b {
                    b'=' => Some(TokenKind::Eq),
                    b';' => Some(TokenKind::Semi),
                    b',' => Some(TokenKind::Comma),
                    b'.' => Some(TokenKind::Dot),
                    b'-' => Some(TokenKind::Minus),
                    b'{' => Some(TokenKind::LBrace),
                    b'}' => Some(TokenKind::RBrace),
                    b'[' => Some(TokenKind::LBracket),
                    b']' => Some(TokenKind::RBracket),
                    b'<' => Some(TokenKind::Lt),
                    b'>' => Some(TokenKind::Gt),
                    b'(' => Some(TokenKind::LParen),
                    b')' => Some(TokenKind::RParen),
                    _ => None,
                };
                match kind {
                    Some(kind) => tokens.push(token(kind, start, pos)),
                    None => {
                        let ch = text[start..].chars().next().unwrap();
                        pos = start + ch.len_utf8();
                        diags.push(
                            Diagnostic::error("E0203", format!("unexpected character `{ch}`"))
                                .with_label(
                                    file,
                                    Span::new(start as u32, pos as u32),
                                    "not valid here",
                                ),
                        );
                    }
                }
            }
        }
    }
    tokens.push(token(TokenKind::Eof, pos, pos));
    tokens
}

fn token(kind: TokenKind, start: usize, end: usize) -> Token {
    Token {
        kind,
        span: Span::new(start as u32, end as u32),
    }
}
