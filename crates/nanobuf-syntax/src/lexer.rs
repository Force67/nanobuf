use nanobuf_diagnostics::{Diagnostic, Diagnostics, FileId, Span};

use crate::token::{Token, TokenKind};

/// Tokenizes `text`, reporting lexical errors into `diags`.
///
/// The stream always ends with an `Eof` token. Runs of blank lines collapse
/// into a single `Newline`; leading newlines are dropped.
pub fn lex(file: FileId, text: &str, diags: &mut Diagnostics) -> Vec<Token> {
    Lexer {
        file,
        text,
        pos: 0,
        tokens: Vec::new(),
        diags,
    }
    .run()
}

struct Lexer<'a> {
    file: FileId,
    text: &'a str,
    pos: usize,
    tokens: Vec<Token>,
    diags: &'a mut Diagnostics,
}

impl Lexer<'_> {
    fn run(mut self) -> Vec<Token> {
        let bytes = self.text.as_bytes();
        while self.pos < bytes.len() {
            let start = self.pos;
            match bytes[self.pos] {
                b' ' | b'\t' | b'\r' => self.pos += 1,
                b'\n' => {
                    self.pos += 1;
                    if matches!(self.tokens.last(), Some(t) if t.kind != TokenKind::Newline) {
                        self.push(TokenKind::Newline, start);
                    }
                }
                b'/' => self.comment_or_error(start),
                b'"' => self.string(start),
                b'0'..=b'9' => self.number(start),
                b'A'..=b'Z' | b'a'..=b'z' | b'_' => self.ident(start),
                b => {
                    if let Some(kind) = punct(b) {
                        self.pos += 1;
                        self.push(kind, start);
                    } else {
                        let ch = self.text[start..].chars().next().unwrap();
                        self.pos += ch.len_utf8();
                        self.diags.push(
                            Diagnostic::error("E0001", format!("unexpected character `{ch}`"))
                                .with_label(self.file, self.span_from(start), "not valid here"),
                        );
                    }
                }
            }
        }
        // A trailing doc comment or statement may lack a final newline.
        if matches!(self.tokens.last(), Some(t) if t.kind != TokenKind::Newline) {
            self.push(TokenKind::Newline, self.pos);
        }
        self.push(TokenKind::Eof, self.pos);
        self.tokens
    }

    fn comment_or_error(&mut self, start: usize) {
        let rest = &self.text.as_bytes()[start..];
        if rest.get(1) != Some(&b'/') {
            self.pos += 1;
            self.diags.push(
                Diagnostic::error("E0001", "unexpected character `/`")
                    .with_label(self.file, self.span_from(start), "not valid here")
                    .with_help("comments start with `//`"),
            );
            return;
        }
        let is_doc = rest.get(2) == Some(&b'/') && rest.get(3) != Some(&b'/');
        while self.pos < self.text.len() && self.text.as_bytes()[self.pos] != b'\n' {
            self.pos += 1;
        }
        if is_doc {
            self.push(TokenKind::DocComment, start);
        }
    }

    fn string(&mut self, start: usize) {
        self.pos += 1;
        let bytes = self.text.as_bytes();
        while self.pos < bytes.len() && bytes[self.pos] != b'"' && bytes[self.pos] != b'\n' {
            self.pos += 1;
        }
        if self.pos < bytes.len() && bytes[self.pos] == b'"' {
            self.pos += 1;
            self.push(TokenKind::Str, start);
        } else {
            self.diags.push(
                Diagnostic::error("E0002", "unterminated string literal").with_label(
                    self.file,
                    self.span_from(start),
                    "opened here",
                ),
            );
        }
    }

    fn number(&mut self, start: usize) {
        let bytes = self.text.as_bytes();
        let hex = bytes[self.pos] == b'0' && bytes.get(self.pos + 1) == Some(&b'x');
        if hex {
            self.pos += 2;
        }
        while self.pos < bytes.len()
            && (bytes[self.pos].is_ascii_alphanumeric() || bytes[self.pos] == b'_')
        {
            self.pos += 1;
        }
        let body = if hex {
            &self.text[start + 2..self.pos]
        } else {
            &self.text[start..self.pos]
        };
        let radix = if hex { 16 } else { 10 };
        let valid = !body.is_empty()
            && body
                .bytes()
                .all(|b| (b as char).is_digit(radix) || b == b'_');
        if valid {
            self.push(TokenKind::Int, start);
        } else {
            self.diags.push(
                Diagnostic::error(
                    "E0003",
                    format!("invalid integer literal `{}`", &self.text[start..self.pos]),
                )
                .with_label(self.file, self.span_from(start), "not a valid number"),
            );
        }
    }

    fn ident(&mut self, start: usize) {
        let bytes = self.text.as_bytes();
        while self.pos < bytes.len()
            && (bytes[self.pos].is_ascii_alphanumeric() || bytes[self.pos] == b'_')
        {
            self.pos += 1;
        }
        let kind = match &self.text[start..self.pos] {
            "schema" => TokenKind::Schema,
            "import" => TokenKind::Import,
            "message" => TokenKind::Message,
            "enum" => TokenKind::Enum,
            "reserved" => TokenKind::Reserved,
            _ => TokenKind::Ident,
        };
        self.push(kind, start);
    }

    fn push(&mut self, kind: TokenKind, start: usize) {
        self.tokens.push(Token {
            kind,
            span: self.span_from(start),
        });
    }

    fn span_from(&self, start: usize) -> Span {
        Span::new(start as u32, self.pos as u32)
    }
}

fn punct(b: u8) -> Option<TokenKind> {
    Some(match b {
        b':' => TokenKind::Colon,
        b',' => TokenKind::Comma,
        b'.' => TokenKind::Dot,
        b'?' => TokenKind::Question,
        b'@' => TokenKind::At,
        b'=' => TokenKind::Eq,
        b'-' => TokenKind::Minus,
        b';' => TokenKind::Semi,
        b'{' => TokenKind::LBrace,
        b'}' => TokenKind::RBrace,
        b'<' => TokenKind::Lt,
        b'>' => TokenKind::Gt,
        b'[' => TokenKind::LBracket,
        b']' => TokenKind::RBracket,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use nanobuf_diagnostics::SourceMap;

    use super::*;

    fn lex_ok(text: &str) -> Vec<TokenKind> {
        let mut sources = SourceMap::new();
        let file = sources.add("t.nb", text);
        let mut diags = Diagnostics::new();
        let tokens = lex(file, text, &mut diags);
        assert!(diags.is_empty(), "{}", diags.render(&sources, false));
        tokens.iter().map(|t| t.kind).collect()
    }

    fn lex_err(text: &str) -> (Vec<TokenKind>, Vec<&'static str>) {
        let mut sources = SourceMap::new();
        let file = sources.add("t.nb", text);
        let mut diags = Diagnostics::new();
        let tokens = lex(file, text, &mut diags);
        (
            tokens.iter().map(|t| t.kind).collect(),
            diags.iter().map(|d| d.code).collect(),
        )
    }

    use TokenKind::*;

    #[test]
    fn lexes_field_line() {
        assert_eq!(
            lex_ok("sequence: u32 @1\n"),
            vec![Ident, Colon, Ident, At, Int, Newline, Eof]
        );
    }

    #[test]
    fn lexes_keywords_and_puncts() {
        assert_eq!(
            lex_ok("schema a.b\nmessage M { }\nenum E : u8\nimport \"x\"\nreserved @1, @2"),
            vec![
                Schema, Ident, Dot, Ident, Newline, Message, Ident, LBrace, RBrace, Newline, Enum,
                Ident, Colon, Ident, Newline, Import, Str, Newline, Reserved, At, Int, Comma, At,
                Int, Newline, Eof
            ]
        );
    }

    #[test]
    fn lexes_generics_optional_attrs() {
        assert_eq!(
            lex_ok("samples: list<u16>? @5 [proto = sint32]"),
            vec![
                Ident, Colon, Ident, Lt, Ident, Gt, Question, At, Int, LBracket, Ident, Eq, Ident,
                RBracket, Newline, Eof
            ]
        );
    }

    #[test]
    fn collapses_blank_lines_and_drops_leading() {
        assert_eq!(
            lex_ok("\n\n\na\n\n\nb\n\n"),
            vec![Ident, Newline, Ident, Newline, Eof]
        );
    }

    #[test]
    fn empty_input_is_just_eof() {
        assert_eq!(lex_ok(""), vec![Eof]);
        assert_eq!(lex_ok("   \n\t\n"), vec![Eof]);
    }

    #[test]
    fn plain_comments_are_skipped() {
        assert_eq!(
            lex_ok("a // trailing\n// whole line\nb"),
            vec![Ident, Newline, Ident, Newline, Eof]
        );
    }

    #[test]
    fn doc_comments_are_tokens_but_four_slashes_are_not() {
        assert_eq!(
            lex_ok("/// doc\na"),
            vec![DocComment, Newline, Ident, Newline, Eof]
        );
        assert_eq!(lex_ok("//// rule\na"), vec![Ident, Newline, Eof]);
    }

    #[test]
    fn doc_comment_span_covers_line() {
        let mut sources = SourceMap::new();
        let text = "/// hello\n";
        let file = sources.add("t.nb", text);
        let mut diags = Diagnostics::new();
        let tokens = lex(file, text, &mut diags);
        assert_eq!(
            &text[tokens[0].span.start as usize..tokens[0].span.end as usize],
            "/// hello"
        );
    }

    #[test]
    fn hex_and_decimal_ints() {
        assert_eq!(lex_ok("1 0xFF 1_000"), vec![Int, Int, Int, Newline, Eof]);
    }

    #[test]
    fn invalid_numbers_error() {
        assert_eq!(lex_err("1abc").1, vec!["E0003"]);
        assert_eq!(lex_err("0x").1, vec!["E0003"]);
        assert_eq!(lex_err("0xZZ").1, vec!["E0003"]);
    }

    #[test]
    fn unterminated_string_errors() {
        assert_eq!(lex_err("\"abc").1, vec!["E0002"]);
        assert_eq!(lex_err("\"abc\nx\"").1, vec!["E0002", "E0002"]);
    }

    #[test]
    fn unexpected_characters_error_and_continue() {
        let (tokens, codes) = lex_err("a $ b");
        assert_eq!(codes, vec!["E0001"]);
        assert_eq!(tokens, vec![Ident, Ident, Newline, Eof]);
        assert_eq!(lex_err("a / b").1, vec!["E0001"]);
        assert_eq!(lex_err("é").1, vec!["E0001"]);
    }

    #[test]
    fn missing_trailing_newline_is_synthesized() {
        assert_eq!(lex_ok("a"), vec![Ident, Newline, Eof]);
    }
}
