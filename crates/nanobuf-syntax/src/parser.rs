use nanobuf_diagnostics::{Diagnostic, Diagnostics, FileId, Span};

use crate::ast;
use crate::lexer;
use crate::token::{Token, TokenKind};

/// Parses one `.nb` file. Errors are reported into `diags`; the parser
/// recovers at line and declaration boundaries so several errors surface in
/// one pass. Returns `None` only when no usable `schema` header was found.
pub fn parse(file: FileId, text: &str, diags: &mut Diagnostics) -> Option<ast::File> {
    let tokens = lexer::lex(file, text, diags);
    Parser {
        file,
        text,
        tokens,
        pos: 0,
        diags,
    }
    .file()
}

struct Parser<'a> {
    file: FileId,
    text: &'a str,
    tokens: Vec<Token>,
    pos: usize,
    diags: &'a mut Diagnostics,
}

impl Parser<'_> {
    fn file(mut self) -> Option<ast::File> {
        self.skip_newlines();

        if self.at(TokenKind::Ident) && self.peek_text() == "syntax" {
            self.diags.push(
                Diagnostic::error("E0005", "this looks like a protobuf file")
                    .with_label(self.file, self.peek().span, "protobuf syntax")
                    .with_help("convert it with `nanoc migrate <file>.proto`"),
            );
            return None;
        }
        if !self.eat(TokenKind::Schema) {
            self.diags.push(
                Diagnostic::error("E0006", "missing `schema` declaration")
                    .with_label(self.file, self.peek().span, "expected `schema <name>` here")
                    .with_note("every nanobuf file starts with `schema <dotted.name>`"),
            );
            return None;
        }
        let package = self.path()?;
        self.terminator();

        let mut imports = Vec::new();
        let mut decls = Vec::new();
        loop {
            self.skip_newlines();
            let docs = self.docs();
            match self.peek().kind {
                TokenKind::Eof => {
                    self.dangling_docs(&docs);
                    break;
                }
                TokenKind::Import => {
                    if let Some(import) = self.import() {
                        imports.push(import);
                    } else {
                        self.sync_to_line_end();
                    }
                }
                TokenKind::Message => {
                    if let Some(m) = self.message(docs) {
                        decls.push(ast::Decl::Message(m));
                    }
                }
                TokenKind::Enum => {
                    if let Some(e) = self.enumeration(docs) {
                        decls.push(ast::Decl::Enum(e));
                    }
                }
                _ => {
                    let tok = self.peek();
                    self.diags.push(
                        Diagnostic::error(
                            "E0004",
                            format!("expected a declaration, found {}", tok.kind.describe()),
                        )
                        .with_label(
                            self.file,
                            tok.span,
                            "expected `message`, `enum`, or `import`",
                        ),
                    );
                    self.sync_to_decl();
                }
            }
        }
        Some(ast::File {
            package,
            imports,
            decls,
        })
    }

    fn import(&mut self) -> Option<ast::Import> {
        self.bump();
        let tok = self.expect(TokenKind::Str, "an import path like `\"common.nb\"`")?;
        let path = self.text[tok.span.start as usize + 1..tok.span.end as usize - 1].to_string();
        self.terminator();
        Some(ast::Import {
            path,
            span: tok.span,
        })
    }

    fn message(&mut self, docs: Vec<String>) -> Option<ast::MessageDecl> {
        let start = self.bump().span;
        let name = self.ident("a message name")?;
        self.expect(TokenKind::LBrace, "`{` to open the message body")?;
        self.terminator();

        let mut fields = Vec::new();
        let mut reserved = Vec::new();
        loop {
            self.skip_newlines();
            let docs = self.docs();
            match self.peek().kind {
                TokenKind::RBrace | TokenKind::Eof => {
                    self.dangling_docs(&docs);
                    break;
                }
                TokenKind::Reserved => self.reserved_ids(&mut reserved),
                TokenKind::Ident => {
                    if let Some(field) = self.field(docs) {
                        fields.push(field);
                    }
                }
                _ => {
                    let tok = self.peek();
                    self.diags.push(
                        Diagnostic::error(
                            "E0004",
                            format!("expected a field, found {}", tok.kind.describe()),
                        )
                        .with_label(
                            self.file,
                            tok.span,
                            "fields are written `name: type @id`",
                        ),
                    );
                    self.sync_to_line_end();
                }
            }
        }
        let end = self
            .expect(TokenKind::RBrace, "`}` to close the message")?
            .span;
        self.terminator();
        Some(ast::MessageDecl {
            docs,
            name,
            fields,
            reserved,
            span: start.join(end),
        })
    }

    /// Parses one field line. On failure the rest of the line is consumed,
    /// so the caller can continue with the next line.
    fn field(&mut self, docs: Vec<String>) -> Option<ast::FieldDecl> {
        let result = self.field_inner(docs);
        if result.is_none() {
            self.sync_to_line_end();
        }
        result
    }

    fn field_inner(&mut self, docs: Vec<String>) -> Option<ast::FieldDecl> {
        let name = self.ident("a field name")?;

        if self.at(TokenKind::Ident) {
            let (code, help) = match name.text.as_str() {
                "optional" => (
                    "E0007",
                    "nanobuf marks optional fields on the type: `name: type? @id`",
                ),
                "repeated" => ("E0007", "nanobuf lists are written `name: list<type> @id`"),
                _ => ("E0004", "fields are written `name: type @id`"),
            };
            self.diags.push(
                Diagnostic::error(
                    code,
                    format!("expected `:` after field name `{}`", name.text),
                )
                .with_label(self.file, self.peek().span, "expected `:` before this")
                .with_help(help),
            );
            return None;
        }

        self.expect(TokenKind::Colon, "`:` after the field name")?;
        let ty = self.type_expr()?;

        if self.at(TokenKind::Eq) {
            let span = self.peek().span;
            self.diags.push(
                Diagnostic::error("E0008", "field ids are written with `@`, not `=`")
                    .with_label(self.file, span, "write `@` here")
                    .with_help(format!("for example `{}: {ty} @1`", name.text)),
            );
            return None;
        }
        self.expect(TokenKind::At, "`@` introducing the field id")?;
        let (id, id_span) = self.int_u32()?;

        let proto = if self.eat(TokenKind::LBracket) {
            let key = self.ident("an attribute name")?;
            if key.text != "proto" {
                self.diags.push(
                    Diagnostic::error("E0009", format!("unknown attribute `{}`", key.text))
                        .with_label(self.file, key.span, "only `proto` is supported here"),
                );
                return None;
            }
            self.expect(TokenKind::Eq, "`=` after `proto`")?;
            let value = self.ident("a proto type like `int32`")?;
            self.expect(TokenKind::RBracket, "`]` to close the attribute")?;
            Some(value)
        } else {
            None
        };

        self.terminator();
        let span = name.span.join(id_span);
        Some(ast::FieldDecl {
            docs,
            name,
            ty,
            id,
            id_span,
            proto,
            span,
        })
    }

    fn type_expr(&mut self) -> Option<ast::TypeExpr> {
        let base = if self.at(TokenKind::Ident) && self.peek_text() == "list" {
            let start = self.bump().span;
            self.expect(TokenKind::Lt, "`<` after `list`")?;
            let elem = self.type_expr()?;
            let end = self
                .expect(TokenKind::Gt, "`>` to close the list type")?
                .span;
            ast::TypeExpr::List {
                elem: Box::new(elem),
                span: start.join(end),
            }
        } else {
            ast::TypeExpr::Named(self.path()?)
        };

        if self.at(TokenKind::Question) {
            let q = self.bump().span;
            if self.at(TokenKind::Question) {
                self.diags.push(
                    Diagnostic::error("E0010", "type is already optional").with_label(
                        self.file,
                        self.peek().span,
                        "second `?` is redundant",
                    ),
                );
                return None;
            }
            let span = base.span().join(q);
            return Some(ast::TypeExpr::Optional {
                inner: Box::new(base),
                span,
            });
        }
        Some(base)
    }

    fn reserved_ids(&mut self, out: &mut Vec<ast::ReservedId>) {
        self.bump();
        loop {
            if !self.at(TokenKind::At) && self.at(TokenKind::Int) {
                let span = self.peek().span;
                self.diags.push(
                    Diagnostic::error("E0008", "reserved ids are written with `@`").with_label(
                        self.file,
                        span,
                        "write `@` before the id",
                    ),
                );
                self.sync_to_line_end();
                return;
            }
            if self
                .expect(TokenKind::At, "`@` introducing a reserved id")
                .is_none()
            {
                self.sync_to_line_end();
                return;
            }
            let Some((id, span)) = self.int_u32() else {
                self.sync_to_line_end();
                return;
            };
            out.push(ast::ReservedId { id, span });
            if !self.eat(TokenKind::Comma) {
                break;
            }
        }
        self.terminator();
    }

    fn enumeration(&mut self, docs: Vec<String>) -> Option<ast::EnumDecl> {
        let start = self.bump().span;
        let name = self.ident("an enum name")?;
        let backing = if self.eat(TokenKind::Colon) {
            Some(self.ident("a backing type like `u8`")?)
        } else {
            None
        };
        self.expect(TokenKind::LBrace, "`{` to open the enum body")?;
        self.terminator();

        let mut variants = Vec::new();
        loop {
            self.skip_newlines();
            let docs = self.docs();
            if matches!(self.peek().kind, TokenKind::RBrace | TokenKind::Eof) {
                break;
            }
            let Some(variant) = self.variant(docs) else {
                self.sync_to_line_end();
                continue;
            };
            variants.push(variant);
        }
        let end = self
            .expect(TokenKind::RBrace, "`}` to close the enum")?
            .span;
        self.terminator();
        Some(ast::EnumDecl {
            docs,
            name,
            backing,
            variants,
            span: start.join(end),
        })
    }

    fn variant(&mut self, docs: Vec<String>) -> Option<ast::VariantDecl> {
        let name = self.ident("a variant name")?;
        self.expect(TokenKind::Eq, "`=` after the variant name")?;
        let negative = self.eat(TokenKind::Minus);
        let tok = self.expect(TokenKind::Int, "the variant value")?;
        let raw = self.int_value(tok)?;
        if raw > i64::MAX as u64 {
            self.overflow(tok.span);
            return None;
        }
        let value = if negative { -(raw as i64) } else { raw as i64 };
        self.terminator();
        let span = name.span.join(tok.span);
        Some(ast::VariantDecl {
            docs,
            name,
            value,
            value_span: tok.span,
            span,
        })
    }

    // Token helpers.

    fn peek(&self) -> Token {
        self.tokens[self.pos]
    }

    fn peek_text(&self) -> &str {
        let span = self.peek().span;
        &self.text[span.start as usize..span.end as usize]
    }

    fn at(&self, kind: TokenKind) -> bool {
        self.peek().kind == kind
    }

    fn bump(&mut self) -> Token {
        let tok = self.peek();
        if tok.kind != TokenKind::Eof {
            self.pos += 1;
        }
        tok
    }

    fn eat(&mut self, kind: TokenKind) -> bool {
        if self.at(kind) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, kind: TokenKind, what: &str) -> Option<Token> {
        if self.at(kind) {
            return Some(self.bump());
        }
        let tok = self.peek();
        self.diags.push(
            Diagnostic::error(
                "E0004",
                format!("expected {what}, found {}", tok.kind.describe()),
            )
            .with_label(self.file, tok.span, format!("expected {what}")),
        );
        None
    }

    fn ident(&mut self, what: &str) -> Option<ast::Ident> {
        let tok = self.expect(TokenKind::Ident, what)?;
        Some(ast::Ident {
            text: self.token_text(tok).to_string(),
            span: tok.span,
        })
    }

    fn path(&mut self) -> Option<ast::Path> {
        let first = self.ident("a name")?;
        let mut span = first.span;
        let mut segments = vec![first];
        while self.eat(TokenKind::Dot) {
            let seg = self.ident("a name after `.`")?;
            span = span.join(seg.span);
            segments.push(seg);
        }
        Some(ast::Path { segments, span })
    }

    fn int_u32(&mut self) -> Option<(u32, Span)> {
        let tok = self.expect(TokenKind::Int, "an id like `1`")?;
        let value = self.int_value(tok)?;
        if value > u32::MAX as u64 {
            self.overflow(tok.span);
            return None;
        }
        Some((value as u32, tok.span))
    }

    fn int_value(&mut self, tok: Token) -> Option<u64> {
        let text = self.token_text(tok).replace('_', "");
        let result = match text.strip_prefix("0x") {
            Some(hex) => u64::from_str_radix(hex, 16),
            None => text.parse(),
        };
        match result {
            Ok(v) => Some(v),
            Err(_) => {
                self.overflow(tok.span);
                None
            }
        }
    }

    fn overflow(&mut self, span: Span) {
        self.diags.push(
            Diagnostic::error("E0003", "integer is too large").with_label(
                self.file,
                span,
                "does not fit",
            ),
        );
    }

    fn token_text(&self, tok: Token) -> &str {
        &self.text[tok.span.start as usize..tok.span.end as usize]
    }

    fn dangling_docs(&mut self, docs: &[String]) {
        if !docs.is_empty() {
            self.diags.push(
                Diagnostic::warning("W0001", "doc comment is not attached to a declaration")
                    .with_label(self.file, self.peek().span, "nothing to document here"),
            );
        }
    }

    fn docs(&mut self) -> Vec<String> {
        let mut docs = Vec::new();
        loop {
            match self.peek().kind {
                TokenKind::DocComment => {
                    let tok = self.bump();
                    let text = self.token_text(tok);
                    docs.push(text.trim_start_matches('/').trim().to_string());
                }
                TokenKind::Newline if !docs.is_empty() => {
                    self.pos += 1;
                }
                _ => return docs,
            }
        }
    }

    /// Consumes the end of a statement: a newline, or a position where the
    /// enclosing construct ends. A `;` gets a dedicated error.
    fn terminator(&mut self) {
        match self.peek().kind {
            TokenKind::Newline => {
                self.pos += 1;
            }
            TokenKind::RBrace | TokenKind::Eof => {}
            TokenKind::Semi => {
                let span = self.peek().span;
                self.diags.push(
                    Diagnostic::error("E0011", "statements end at the newline").with_label(
                        self.file,
                        span,
                        "remove this `;`",
                    ),
                );
                self.sync_to_line_end();
            }
            _ => {
                let tok = self.peek();
                self.diags.push(
                    Diagnostic::error(
                        "E0004",
                        format!("expected end of line, found {}", tok.kind.describe()),
                    )
                    .with_label(
                        self.file,
                        tok.span,
                        "unexpected trailing input",
                    ),
                );
                self.sync_to_line_end();
            }
        }
    }

    fn skip_newlines(&mut self) {
        while self.at(TokenKind::Newline) {
            self.pos += 1;
        }
    }

    /// Skips to just after the current line, stopping before `}` / EOF.
    fn sync_to_line_end(&mut self) {
        loop {
            match self.peek().kind {
                TokenKind::Newline => {
                    self.pos += 1;
                    return;
                }
                TokenKind::RBrace | TokenKind::Eof => return,
                _ => self.pos += 1,
            }
        }
    }

    /// Skips to the next top-level declaration keyword or EOF.
    fn sync_to_decl(&mut self) {
        loop {
            match self.peek().kind {
                TokenKind::Message | TokenKind::Enum | TokenKind::Import | TokenKind::Eof => {
                    return;
                }
                _ => self.pos += 1,
            }
        }
    }
}
