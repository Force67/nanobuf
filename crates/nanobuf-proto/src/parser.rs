use nanobuf_diagnostics::{Diagnostic, Diagnostics, FileId};

use crate::lexer::{self, Token, TokenKind};
use crate::{Decl, ProtoEnum, ProtoField, ProtoFile, ProtoMessage};

/// Parses a proto3 file into the migration model. Nested message and enum
/// definitions are flattened (`Outer.Inner` becomes `OuterInner`) and
/// references to them are rewritten.
pub fn parse(file: FileId, text: &str, diags: &mut Diagnostics) -> Option<ProtoFile> {
    let tokens = lexer::lex(file, text, diags);
    let parser = Parser {
        file,
        text,
        tokens,
        pos: 0,
        diags,
        out: ProtoFile {
            package: String::new(),
            imports: Vec::new(),
            decls: Vec::new(),
        },
        nested: std::collections::HashMap::new(),
    };
    parser.file()
}

struct Parser<'a> {
    file: FileId,
    text: &'a str,
    tokens: Vec<Token>,
    pos: usize,
    diags: &'a mut Diagnostics,
    out: ProtoFile,
    /// Maps proto references (`Outer.Inner` and, where unambiguous, the
    /// bare `Inner`) to flattened type names.
    nested: std::collections::HashMap<String, String>,
}

impl Parser<'_> {
    fn file(mut self) -> Option<ProtoFile> {
        if !(self.eat_ident("syntax")
            && self.eat(TokenKind::Eq)
            && self.eat_str_value("proto3")
            && self.eat(TokenKind::Semi))
        {
            self.error(
                "E0204",
                "expected `syntax = \"proto3\";`",
                "only proto3 files can be migrated",
            );
            return None;
        }
        loop {
            match self.peek().kind {
                TokenKind::Eof => break,
                TokenKind::Ident => match self.peek_text() {
                    "package" => {
                        self.bump();
                        let mut name = self.ident("a package name")?.to_string();
                        while self.eat(TokenKind::Dot) {
                            name.push('.');
                            name.push_str(self.ident("a package segment")?);
                        }
                        self.expect(TokenKind::Semi, "`;` after the package name")?;
                        self.out.package = name;
                    }
                    "import" => {
                        self.bump();
                        // `public`/`weak` modifiers are irrelevant here.
                        if matches!(self.peek_text(), "public" | "weak") {
                            self.bump();
                        }
                        let path = self.string_literal("an import path")?;
                        self.expect(TokenKind::Semi, "`;` after the import")?;
                        self.out.imports.push(path);
                    }
                    "option" => self.skip_statement(),
                    "message" => {
                        self.bump();
                        self.message(String::new())?;
                    }
                    "enum" => {
                        self.bump();
                        self.enumeration(String::new())?;
                    }
                    "service" => {
                        self.diags.push(
                            Diagnostic::warning(
                                "W0201",
                                "services are not migrated; only message types are",
                            )
                            .with_label(
                                self.file,
                                self.peek().span,
                                "skipped",
                            ),
                        );
                        self.bump();
                        self.ident("a service name")?;
                        self.skip_block()?;
                    }
                    other => {
                        let message = format!("unexpected `{other}`");
                        self.error("E0205", &message, "expected a declaration");
                        return None;
                    }
                },
                _ => {
                    self.error("E0205", "unexpected token", "expected a declaration");
                    return None;
                }
            }
        }
        if self.out.package.is_empty() {
            self.error(
                "E0206",
                "proto file has no `package` declaration",
                "nanobuf schemas require a package name",
            );
            return None;
        }
        Some(self.out)
    }

    /// Parses a message body. `prefix` carries flattened outer type names.
    fn message(&mut self, prefix: String) -> Option<()> {
        let name = format!("{prefix}{}", self.ident("a message name")?);
        self.expect(TokenKind::LBrace, "`{` to open the message")?;
        let mut message = ProtoMessage {
            name: name.clone(),
            fields: Vec::new(),
            reserved: Vec::new(),
        };
        loop {
            match self.peek_text() {
                "" if self.peek().kind == TokenKind::RBrace => break,
                "message" => {
                    self.bump();
                    let dotted_prefix = format!("{name}.");
                    self.nested_scope(&dotted_prefix);
                    self.message(name.clone())?;
                }
                "enum" => {
                    self.bump();
                    self.nested_scope(&format!("{name}."));
                    self.enumeration(name.clone())?;
                }
                "reserved" => {
                    self.bump();
                    self.reserved(&mut message)?;
                }
                "oneof" => {
                    self.error(
                        "E0207",
                        "`oneof` is not supported",
                        "model alternatives as separate optional fields",
                    );
                    return None;
                }
                "map" => {
                    self.error(
                        "E0208",
                        "`map` fields are not supported",
                        "use a list of key/value messages instead",
                    );
                    return None;
                }
                "option" => self.skip_statement(),
                _ if self.peek().kind == TokenKind::RBrace => break,
                _ => {
                    let field = self.field()?;
                    message.fields.push(field);
                }
            }
        }
        self.expect(TokenKind::RBrace, "`}` to close the message")?;
        self.eat(TokenKind::Semi);
        self.out.decls.push(Decl::Message(message));
        Some(())
    }

    /// Records that the type about to be parsed under `dotted_prefix` is
    /// flattened, so later references can be rewritten.
    fn nested_scope(&mut self, dotted_prefix: &str) {
        let Some(tok) = self.tokens.get(self.pos) else {
            return;
        };
        if tok.kind != TokenKind::Ident {
            return;
        }
        let bare = self.text[tok.span.start as usize..tok.span.end as usize].to_string();
        let dotted = format!("{dotted_prefix}{bare}");
        let flattened = dotted.replace('.', "");
        self.nested.insert(dotted, flattened.clone());
        // Bare references resolve to the innermost defining scope in proto;
        // a single global mapping is enough when the name is unambiguous.
        self.nested.entry(bare).or_insert(flattened);
    }

    fn field(&mut self) -> Option<ProtoField> {
        let mut repeated = false;
        let mut optional = false;
        loop {
            match self.peek_text() {
                "repeated" => {
                    repeated = true;
                    self.bump();
                }
                "optional" => {
                    optional = true;
                    self.bump();
                }
                _ => break,
            }
        }
        let mut proto_type = self.ident("a field type")?.to_string();
        while self.eat(TokenKind::Dot) {
            proto_type.push('.');
            proto_type.push_str(self.ident("a type segment")?);
        }
        // References to flattened nested types lose their dots.
        if let Some(flattened) = self.nested.get(&proto_type) {
            proto_type = flattened.clone();
        }
        let name = self.ident("a field name")?.to_string();
        self.expect(TokenKind::Eq, "`=` before the field number")?;
        let id = self.int_u32()?;
        if self.eat(TokenKind::LBracket) {
            // Field options like [packed = true] don't change migration.
            let mut depth = 1;
            while depth > 0 {
                match self.bump().kind {
                    TokenKind::LBracket => depth += 1,
                    TokenKind::RBracket => depth -= 1,
                    TokenKind::Eof => {
                        self.error("E0209", "unterminated field options", "missing `]`");
                        return None;
                    }
                    _ => {}
                }
            }
        }
        self.expect(TokenKind::Semi, "`;` after the field")?;
        Some(ProtoField {
            name,
            proto_type,
            repeated,
            optional,
            id,
        })
    }

    fn enumeration(&mut self, prefix: String) -> Option<()> {
        let name = format!("{prefix}{}", self.ident("an enum name")?);
        self.expect(TokenKind::LBrace, "`{` to open the enum")?;
        let mut variants = Vec::new();
        while self.peek().kind != TokenKind::RBrace {
            if self.peek_text() == "option" || self.peek_text() == "reserved" {
                self.skip_statement();
                continue;
            }
            let variant = self.ident("a variant name")?.to_string();
            self.expect(TokenKind::Eq, "`=` before the variant value")?;
            let negative = self.eat(TokenKind::Minus);
            let value = self.int_u32()? as i64;
            let value = if negative { -value } else { value };
            // Enum value options like [deprecated = true].
            if self.eat(TokenKind::LBracket) {
                while !matches!(self.bump().kind, TokenKind::RBracket | TokenKind::Eof) {}
            }
            self.expect(TokenKind::Semi, "`;` after the variant")?;
            variants.push((variant, value));
        }
        self.expect(TokenKind::RBrace, "`}` to close the enum")?;
        self.eat(TokenKind::Semi);
        self.out
            .decls
            .push(Decl::Enum(ProtoEnum { name, variants }));
        Some(())
    }

    fn reserved(&mut self, message: &mut ProtoMessage) -> Option<()> {
        loop {
            match self.peek().kind {
                TokenKind::Str => {
                    // Reserved field *names* have no nanobuf equivalent;
                    // ids are what protect the wire.
                    self.bump();
                }
                TokenKind::Int => {
                    let start = self.int_u32()?;
                    if self.peek_text() == "to" {
                        self.bump();
                        if self.peek_text() == "max" {
                            self.bump();
                            self.diags.push(
                                Diagnostic::warning(
                                    "W0202",
                                    "`reserved N to max` is reduced to the listed ids",
                                )
                                .with_label(
                                    self.file,
                                    self.peek().span,
                                    "open range dropped",
                                ),
                            );
                            message.reserved.push(start);
                        } else {
                            let end = self.int_u32()?;
                            message.reserved.extend(start..=end);
                        }
                    } else {
                        message.reserved.push(start);
                    }
                }
                _ => {
                    self.error("E0210", "expected a reserved id or name", "in `reserved`");
                    return None;
                }
            }
            if !self.eat(TokenKind::Comma) {
                break;
            }
        }
        self.expect(TokenKind::Semi, "`;` after `reserved`")?;
        Some(())
    }

    /// Skips to the matching `;`, tolerating `{...}` option bodies.
    fn skip_statement(&mut self) {
        loop {
            match self.peek().kind {
                TokenKind::Semi => {
                    self.bump();
                    return;
                }
                TokenKind::LBrace => {
                    let _ = self.skip_block();
                    self.eat(TokenKind::Semi);
                    return;
                }
                TokenKind::Eof => return,
                _ => {
                    self.bump();
                }
            }
        }
    }

    /// Skips a balanced `{ ... }` block.
    fn skip_block(&mut self) -> Option<()> {
        self.expect(TokenKind::LBrace, "`{`")?;
        let mut depth = 1;
        while depth > 0 {
            match self.bump().kind {
                TokenKind::LBrace => depth += 1,
                TokenKind::RBrace => depth -= 1,
                TokenKind::Eof => {
                    self.error("E0211", "unterminated block", "missing `}`");
                    return None;
                }
                _ => {}
            }
        }
        Some(())
    }

    // Token helpers.

    fn peek(&self) -> Token {
        self.tokens[self.pos]
    }

    fn peek_text(&self) -> &str {
        let span = self.peek().span;
        &self.text[span.start as usize..span.end as usize]
    }

    fn bump(&mut self) -> Token {
        let tok = self.peek();
        if tok.kind != TokenKind::Eof {
            self.pos += 1;
        }
        tok
    }

    fn eat(&mut self, kind: TokenKind) -> bool {
        if self.peek().kind == kind {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn eat_ident(&mut self, text: &str) -> bool {
        if self.peek().kind == TokenKind::Ident && self.peek_text() == text {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn eat_str_value(&mut self, value: &str) -> bool {
        if self.peek().kind == TokenKind::Str {
            let span = self.peek().span;
            let text = &self.text[span.start as usize + 1..span.end as usize - 1];
            if text == value {
                self.pos += 1;
                return true;
            }
        }
        false
    }

    fn expect(&mut self, kind: TokenKind, what: &str) -> Option<Token> {
        if self.peek().kind == kind {
            return Some(self.bump());
        }
        let span = self.peek().span;
        self.diags.push(
            Diagnostic::error("E0212", format!("expected {what}")).with_label(
                self.file,
                span,
                format!("expected {what}"),
            ),
        );
        None
    }

    fn ident(&mut self, what: &str) -> Option<&str> {
        let tok = self.expect(TokenKind::Ident, what)?;
        Some(&self.text[tok.span.start as usize..tok.span.end as usize])
    }

    fn string_literal(&mut self, what: &str) -> Option<String> {
        let tok = self.expect(TokenKind::Str, what)?;
        Some(self.text[tok.span.start as usize + 1..tok.span.end as usize - 1].to_string())
    }

    fn int_u32(&mut self) -> Option<u32> {
        let tok = self.expect(TokenKind::Int, "a number")?;
        let text = &self.text[tok.span.start as usize..tok.span.end as usize];
        match text.parse() {
            Ok(value) => Some(value),
            Err(_) => {
                self.diags.push(
                    Diagnostic::error("E0213", format!("`{text}` is not a valid field number"))
                        .with_label(self.file, tok.span, "expected a positive integer"),
                );
                None
            }
        }
    }

    fn error(&mut self, code: &'static str, message: &str, label: &str) {
        let span = self.peek().span;
        self.diags
            .push(Diagnostic::error(code, message.to_string()).with_label(
                self.file,
                span,
                label.to_string(),
            ));
    }
}
