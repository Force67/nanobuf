//! Name resolution, validation, and layout assignment.

use std::collections::HashMap;

use nanobuf_diagnostics::{Diagnostic, Diagnostics, FileId, Span};
use nanobuf_syntax::ast;

use crate::ir::{
    DataSlot, ElemType, Enum, EnumId, Field, FieldLayout, Message, MessageId, ProtoEncoding,
    ScalarType, Schema, Type, Variant,
};
use crate::layout;

/// One parsed file plus its diagnostics handle.
pub struct Unit {
    pub file: FileId,
    pub ast: ast::File,
}

const PRIMITIVES: &[(&str, Type)] = &[
    ("bool", Type::Scalar(ScalarType::Bool)),
    ("u8", Type::Scalar(ScalarType::U8)),
    ("u16", Type::Scalar(ScalarType::U16)),
    ("u32", Type::Scalar(ScalarType::U32)),
    ("u64", Type::Scalar(ScalarType::U64)),
    ("i8", Type::Scalar(ScalarType::I8)),
    ("i16", Type::Scalar(ScalarType::I16)),
    ("i32", Type::Scalar(ScalarType::I32)),
    ("i64", Type::Scalar(ScalarType::I64)),
    ("f16", Type::Scalar(ScalarType::F16)),
    ("f32", Type::Scalar(ScalarType::F32)),
    ("f64", Type::Scalar(ScalarType::F64)),
    ("string", Type::String),
    ("bytes", Type::Bytes),
];

/// Protobuf scalar spellings, for targeted migration errors.
const PROTO_ALIASES: &[(&str, &str)] = &[
    ("int32", "i32"),
    ("int64", "i64"),
    ("uint32", "u32"),
    ("uint64", "u64"),
    ("sint32", "i32"),
    ("sint64", "i64"),
    ("fixed32", "u32"),
    ("fixed64", "u64"),
    ("sfixed32", "i32"),
    ("sfixed64", "i64"),
    ("float", "f32"),
    ("double", "f64"),
];

/// `[proto = ...]` attribute values: name, required scalar, wire encoding.
const PROTO_ATTRS: &[(&str, ScalarType, ProtoEncoding)] = &[
    ("int32", ScalarType::I32, ProtoEncoding::Varint),
    ("sint32", ScalarType::I32, ProtoEncoding::ZigZag),
    ("sfixed32", ScalarType::I32, ProtoEncoding::Fixed),
    ("int64", ScalarType::I64, ProtoEncoding::Varint),
    ("sint64", ScalarType::I64, ProtoEncoding::ZigZag),
    ("sfixed64", ScalarType::I64, ProtoEncoding::Fixed),
    ("uint32", ScalarType::U32, ProtoEncoding::Varint),
    ("fixed32", ScalarType::U32, ProtoEncoding::Fixed),
    ("uint64", ScalarType::U64, ProtoEncoding::Varint),
    ("fixed64", ScalarType::U64, ProtoEncoding::Fixed),
];

const MAX_FIELD_ID: u32 = 536_870_911;

/// Resolves and validates parsed files into a laid-out [`Schema`].
/// Returns `None` if any error was reported.
pub fn analyze(units: &[Unit], diags: &mut Diagnostics) -> Option<Schema> {
    let errors_before = diags.iter().filter(|d| is_error(d)).count();
    let mut resolver = Resolver {
        diags,
        schema: Schema::default(),
        symbols: HashMap::new(),
        spans: Vec::new(),
    };
    resolver.collect(units);
    resolver.enums(units);
    resolver.messages(units);
    resolver.mark_boxed();
    resolver.padding_hints();

    let schema = resolver.schema;
    let errors_after = diags.iter().filter(|d| is_error(d)).count();
    (errors_after == errors_before).then_some(schema)
}

fn is_error(d: &Diagnostic) -> bool {
    d.severity == nanobuf_diagnostics::Severity::Error
}

#[derive(Clone, Copy)]
enum Symbol {
    Message(MessageId),
    Enum(EnumId),
}

struct Resolver<'a> {
    diags: &'a mut Diagnostics,
    schema: Schema,
    /// `(package, name)` to resolved symbol.
    symbols: HashMap<(String, String), Symbol>,
    /// Name span per message, indexed like `schema.messages`. For hints.
    spans: Vec<(FileId, Span)>,
}

impl Resolver<'_> {
    fn collect(&mut self, units: &[Unit]) {
        let mut first_seen: HashMap<(String, String), (FileId, Span)> = HashMap::new();
        for unit in units {
            let package = unit.ast.package.dotted();
            for decl in &unit.ast.decls {
                let name = match decl {
                    ast::Decl::Message(m) => &m.name,
                    ast::Decl::Enum(e) => &e.name,
                };
                self.check_type_name(unit.file, name);
                let key = (package.clone(), name.text.clone());
                if let Some(&(prev_file, prev_span)) = first_seen.get(&key) {
                    self.diags.push(
                        Diagnostic::error(
                            "E0100",
                            format!("`{}` is defined twice in package `{package}`", name.text),
                        )
                        .with_label(unit.file, name.span, "redefined here")
                        .with_label(
                            prev_file,
                            prev_span,
                            "first defined here",
                        ),
                    );
                    continue;
                }
                first_seen.insert(key.clone(), (unit.file, name.span));
                let symbol = match decl {
                    ast::Decl::Message(m) => {
                        let id = MessageId(self.schema.messages.len() as u32);
                        self.schema.messages.push(Message {
                            package: package.clone(),
                            name: m.name.text.clone(),
                            docs: m.docs.clone(),
                            fields: Vec::new(),
                            reserved: Vec::new(),
                            fixed_len: 2,
                            padding: 0,
                        });
                        self.spans.push((unit.file, m.name.span));
                        Symbol::Message(id)
                    }
                    ast::Decl::Enum(e) => {
                        let id = EnumId(self.schema.enums.len() as u32);
                        self.schema.enums.push(Enum {
                            package: package.clone(),
                            name: e.name.text.clone(),
                            docs: e.docs.clone(),
                            backing: ScalarType::U8,
                            variants: Vec::new(),
                        });
                        Symbol::Enum(id)
                    }
                };
                self.symbols.insert(key, symbol);
            }
        }
    }

    fn check_type_name(&mut self, file: FileId, name: &ast::Ident) {
        if PRIMITIVES.iter().any(|&(p, _)| p == name.text) {
            self.diags.push(
                Diagnostic::error(
                    "E0101",
                    format!("`{}` is a builtin type and cannot be redefined", name.text),
                )
                .with_label(file, name.span, "rename this type"),
            );
        } else if !name
            .text
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_uppercase())
            || name.text.contains('_')
        {
            self.diags.push(
                Diagnostic::warning(
                    "W0004",
                    format!("type `{}` should be named in UpperCamelCase", name.text),
                )
                .with_label(file, name.span, "rename to UpperCamelCase"),
            );
        }
    }

    fn check_member_name(&mut self, file: FileId, name: &ast::Ident) {
        if name.text.chars().any(|c| c.is_ascii_uppercase()) {
            self.diags.push(
                Diagnostic::warning(
                    "W0005",
                    format!("`{}` should be named in lower_snake_case", name.text),
                )
                .with_label(file, name.span, "rename to lower_snake_case"),
            );
        }
    }

    fn enums(&mut self, units: &[Unit]) {
        for unit in units {
            let package = unit.ast.package.dotted();
            for decl in &unit.ast.decls {
                let ast::Decl::Enum(decl) = decl else {
                    continue;
                };
                let Some(&Symbol::Enum(id)) =
                    self.symbols.get(&(package.clone(), decl.name.text.clone()))
                else {
                    continue; // duplicate definition, already reported
                };
                let backing = self.enum_backing(unit.file, decl);
                let variants = self.enum_variants(unit.file, decl, backing);
                let target = &mut self.schema.enums[id.0 as usize];
                target.backing = backing;
                target.variants = variants;
            }
        }
    }

    fn enum_backing(&mut self, file: FileId, decl: &ast::EnumDecl) -> ScalarType {
        let Some(ident) = &decl.backing else {
            return ScalarType::U8;
        };
        let allowed = [
            ScalarType::U8,
            ScalarType::U16,
            ScalarType::U32,
            ScalarType::I8,
            ScalarType::I16,
            ScalarType::I32,
        ];
        match allowed.iter().find(|s| s.name() == ident.text) {
            Some(&scalar) => scalar,
            None => {
                self.diags.push(
                    Diagnostic::error(
                        "E0102",
                        format!("`{}` is not a valid enum backing type", ident.text),
                    )
                    .with_label(
                        file,
                        ident.span,
                        "expected one of u8, u16, u32, i8, i16, i32",
                    ),
                );
                ScalarType::U8
            }
        }
    }

    fn enum_variants(
        &mut self,
        file: FileId,
        decl: &ast::EnumDecl,
        backing: ScalarType,
    ) -> Vec<Variant> {
        if decl.variants.is_empty() {
            self.diags.push(
                Diagnostic::error(
                    "E0106",
                    format!("enum `{}` has no variants", decl.name.text),
                )
                .with_label(file, decl.name.span, "add at least one variant"),
            );
            return Vec::new();
        }

        let (min, max) = backing.range().expect("enum backing is integral");
        let mut variants: Vec<Variant> = Vec::new();
        let mut seen_names: HashMap<&str, Span> = HashMap::new();
        let mut seen_values: HashMap<i64, Span> = HashMap::new();
        for v in &decl.variants {
            self.check_member_name(file, &v.name);
            if let Some(&prev) = seen_names.get(v.name.text.as_str()) {
                self.diags.push(
                    Diagnostic::error("E0103", format!("duplicate variant `{}`", v.name.text))
                        .with_label(file, v.name.span, "redefined here")
                        .with_label(file, prev, "first defined here"),
                );
                continue;
            }
            seen_names.insert(&v.name.text, v.name.span);
            if let Some(&prev) = seen_values.get(&v.value) {
                self.diags.push(
                    Diagnostic::error("E0104", format!("variant value {} is used twice", v.value))
                        .with_label(file, v.value_span, "reused here")
                        .with_label(file, prev, "first used here"),
                );
                continue;
            }
            seen_values.insert(v.value, v.value_span);
            if v.value < min || v.value > max {
                self.diags.push(
                    Diagnostic::error(
                        "E0105",
                        format!(
                            "value {} does not fit backing type `{}`",
                            v.value,
                            backing.name()
                        ),
                    )
                    .with_label(
                        file,
                        v.value_span,
                        format!("allowed range is {min}..={max}"),
                    ),
                );
                continue;
            }
            variants.push(Variant {
                name: v.name.text.clone(),
                docs: v.docs.clone(),
                value: v.value,
            });
        }

        if !variants.iter().any(|v| v.value == 0) {
            self.diags.push(
                Diagnostic::warning(
                    "W0002",
                    format!("enum `{}` has no variant with value 0", decl.name.text),
                )
                .with_label(
                    file,
                    decl.name.span,
                    "the zero value is what readers see for absent fields",
                )
                .with_help("add a variant like `unknown = 0`"),
            );
        }
        variants
    }

    fn messages(&mut self, units: &[Unit]) {
        for unit in units {
            let package = unit.ast.package.dotted();
            for decl in &unit.ast.decls {
                let ast::Decl::Message(decl) = decl else {
                    continue;
                };
                let Some(&Symbol::Message(id)) =
                    self.symbols.get(&(package.clone(), decl.name.text.clone()))
                else {
                    continue; // duplicate definition, already reported
                };
                let reserved = self.reserved_ids(unit.file, decl);
                let mut fields = self.fields(unit.file, &package, decl, &reserved);
                fields.sort_by_key(|f| f.id);

                let enums = &self.schema.enums;
                let (fixed_len, padding) =
                    layout::assign(&mut fields, &|id| enums[id.0 as usize].backing);
                if fixed_len > u16::MAX as u32 {
                    self.diags.push(
                        Diagnostic::error(
                            "E0118",
                            format!(
                                "fixed section of `{}` is {fixed_len} bytes; the limit is 65535",
                                decl.name.text
                            ),
                        )
                        .with_label(unit.file, decl.name.span, "too many fields")
                        .with_help("move groups of fields into nested messages"),
                    );
                    continue;
                }
                let target = &mut self.schema.messages[id.0 as usize];
                target.fields = fields;
                target.reserved = reserved;
                target.fixed_len = fixed_len as u16;
                target.padding = padding as u16;
            }
        }
    }

    fn reserved_ids(&mut self, file: FileId, decl: &ast::MessageDecl) -> Vec<u32> {
        let mut ids = Vec::new();
        for r in &decl.reserved {
            if self.check_id(file, r.id, r.span) {
                ids.push(r.id);
            }
        }
        ids
    }

    /// Validates a field or reserved id. Returns false when unusable.
    fn check_id(&mut self, file: FileId, id: u32, span: Span) -> bool {
        if id == 0 || id > MAX_FIELD_ID {
            self.diags.push(
                Diagnostic::error(
                    "E0110",
                    format!("field ids must be between 1 and {MAX_FIELD_ID}"),
                )
                .with_label(file, span, "out of range"),
            );
            return false;
        }
        if (19000..=19999).contains(&id) {
            self.diags.push(
                Diagnostic::warning("W0003", format!("field id {id} is reserved by protobuf"))
                    .with_label(file, span, "ids 19000-19999 break proto3 migration")
                    .with_help("pick an id outside 19000-19999"),
            );
        }
        true
    }

    fn fields(
        &mut self,
        file: FileId,
        package: &str,
        decl: &ast::MessageDecl,
        reserved: &[u32],
    ) -> Vec<Field> {
        let mut fields: Vec<Field> = Vec::new();
        let mut seen_names: HashMap<&str, Span> = HashMap::new();
        let mut seen_ids: HashMap<u32, Span> = HashMap::new();
        for f in &decl.fields {
            self.check_member_name(file, &f.name);
            if let Some(&prev) = seen_names.get(f.name.text.as_str()) {
                self.diags.push(
                    Diagnostic::error("E0108", format!("duplicate field `{}`", f.name.text))
                        .with_label(file, f.name.span, "redefined here")
                        .with_label(file, prev, "first defined here"),
                );
                continue;
            }
            seen_names.insert(&f.name.text, f.name.span);

            if !self.check_id(file, f.id, f.id_span) {
                continue;
            }
            if let Some(&prev) = seen_ids.get(&f.id) {
                self.diags.push(
                    Diagnostic::error("E0109", format!("field id @{} is used twice", f.id))
                        .with_label(file, f.id_span, "reused here")
                        .with_label(file, prev, "first used here")
                        .with_note("field ids identify fields on the wire and must be unique"),
                );
                continue;
            }
            seen_ids.insert(f.id, f.id_span);
            if reserved.contains(&f.id) {
                self.diags.push(
                    Diagnostic::error("E0111", format!("field id @{} is reserved", f.id))
                        .with_label(file, f.id_span, "listed in `reserved`")
                        .with_note("reserved ids belonged to deleted fields; reusing them breaks old readers"),
                );
                continue;
            }

            let Some((ty, optional)) = self.field_type(file, package, &f.ty) else {
                continue;
            };
            let Some(proto) = self.proto_encoding(file, ty, f.proto.as_ref()) else {
                continue;
            };
            fields.push(Field {
                name: f.name.text.clone(),
                docs: f.docs.clone(),
                id: f.id,
                ty,
                optional,
                boxed: false,
                proto,
                layout: FieldLayout {
                    presence: None,
                    data: DataSlot::Bytes { offset: 0, size: 0 },
                },
            });
        }
        fields
    }

    fn field_type(
        &mut self,
        file: FileId,
        package: &str,
        expr: &ast::TypeExpr,
    ) -> Option<(Type, bool)> {
        match expr {
            ast::TypeExpr::Optional { inner, span } => {
                let (ty, _) = self.field_type(file, package, inner)?;
                match ty {
                    Type::Message(_) => {
                        self.diags.push(
                            Diagnostic::error("E0112", "message fields are always optional")
                                .with_label(file, *span, "remove the `?`"),
                        );
                        None
                    }
                    Type::List(_) => {
                        self.diags.push(
                            Diagnostic::error("E0113", "lists cannot be optional")
                                .with_label(file, *span, "remove the `?`")
                                .with_help("absent and empty lists are the same on the wire"),
                        );
                        None
                    }
                    _ => Some((ty, true)),
                }
            }
            ast::TypeExpr::List { elem, .. } => {
                let elem_ty = match &**elem {
                    ast::TypeExpr::List { span, .. } => {
                        self.diags.push(
                            Diagnostic::error("E0114", "lists cannot contain lists")
                                .with_label(file, *span, "nested list")
                                .with_help("wrap the inner list in a message"),
                        );
                        return None;
                    }
                    ast::TypeExpr::Optional { span, .. } => {
                        self.diags.push(
                            Diagnostic::error("E0115", "list elements cannot be optional")
                                .with_label(file, *span, "remove the `?`"),
                        );
                        return None;
                    }
                    ast::TypeExpr::Named(path) => match self.named_type(file, package, path)? {
                        Type::Scalar(s) => ElemType::Scalar(s),
                        Type::Enum(id) => ElemType::Enum(id),
                        Type::String => ElemType::String,
                        Type::Bytes => ElemType::Bytes,
                        Type::Message(id) => ElemType::Message(id),
                        Type::List(_) => unreachable!("named type is never a list"),
                    },
                };
                Some((Type::List(elem_ty), false))
            }
            ast::TypeExpr::Named(path) => {
                let ty = self.named_type(file, package, path)?;
                Some((ty, false))
            }
        }
    }

    fn named_type(&mut self, file: FileId, package: &str, path: &ast::Path) -> Option<Type> {
        let name = path.dotted();
        if path.segments.len() == 1 {
            if let Some(&(_, ty)) = PRIMITIVES.iter().find(|&&(p, _)| p == name) {
                return Some(ty);
            }
            if let Some(&(_, replacement)) = PROTO_ALIASES.iter().find(|&&(p, _)| p == name) {
                self.diags.push(
                    Diagnostic::error("E0107", format!("`{name}` is a protobuf type name"))
                        .with_label(
                            file,
                            path.span,
                            format!("nanobuf calls this `{replacement}`"),
                        ),
                );
                return None;
            }
        }
        let (pkg, type_name) = match path.segments.split_last() {
            Some((last, [])) => (package.to_string(), last.text.clone()),
            Some((last, prefix)) => (
                prefix
                    .iter()
                    .map(|s| s.text.as_str())
                    .collect::<Vec<_>>()
                    .join("."),
                last.text.clone(),
            ),
            None => unreachable!("paths have at least one segment"),
        };
        match self.symbols.get(&(pkg, type_name)) {
            Some(&Symbol::Message(id)) => Some(Type::Message(id)),
            Some(&Symbol::Enum(id)) => Some(Type::Enum(id)),
            None => {
                let mut diag = Diagnostic::error("E0107", format!("unknown type `{name}`"))
                    .with_label(file, path.span, "not found");
                if let Some(suggestion) = self.closest_name(package, &name) {
                    diag = diag.with_help(format!("did you mean `{suggestion}`?"));
                }
                self.diags.push(diag);
                None
            }
        }
    }

    fn closest_name(&self, package: &str, name: &str) -> Option<String> {
        let mut candidates: Vec<String> = PRIMITIVES.iter().map(|&(p, _)| p.to_string()).collect();
        for (pkg, type_name) in self.symbols.keys() {
            if pkg == package {
                candidates.push(type_name.clone());
            }
            candidates.push(format!("{pkg}.{type_name}"));
        }
        candidates
            .into_iter()
            .map(|c| (levenshtein(name, &c), c))
            .filter(|&(d, _)| d <= 2 && d < name.len())
            .min_by_key(|&(d, _)| d)
            .map(|(_, c)| c)
    }

    fn proto_encoding(
        &mut self,
        file: FileId,
        ty: Type,
        attr: Option<&ast::Ident>,
    ) -> Option<ProtoEncoding> {
        let scalar = match ty {
            Type::Scalar(s) | Type::List(ElemType::Scalar(s)) => Some(s),
            _ => None,
        };
        if let Some(attr) = attr {
            let Some(&(_, required, encoding)) =
                PROTO_ATTRS.iter().find(|&&(n, _, _)| n == attr.text)
            else {
                self.diags.push(
                    Diagnostic::error(
                        "E0117",
                        format!("`{}` is not a proto integer encoding", attr.text),
                    )
                    .with_label(
                        file,
                        attr.span,
                        "expected one of int32, sint32, sfixed32, int64, sint64, sfixed64, \
                         uint32, fixed32, uint64, fixed64",
                    ),
                );
                return None;
            };
            if scalar != Some(required) {
                self.diags.push(
                    Diagnostic::error(
                        "E0116",
                        format!(
                            "`[proto = {}]` only applies to `{}` fields",
                            attr.text,
                            required.name()
                        ),
                    )
                    .with_label(
                        file,
                        attr.span,
                        "encoding does not match the field type",
                    ),
                );
                return None;
            }
            return Some(encoding);
        }
        Some(default_proto(ty))
    }

    fn mark_boxed(&mut self) {
        let n = self.schema.messages.len();
        let edges: Vec<Vec<usize>> = (0..n)
            .map(|i| {
                self.schema.messages[i]
                    .fields
                    .iter()
                    .filter_map(|f| match f.ty {
                        Type::Message(MessageId(j)) => Some(j as usize),
                        _ => None,
                    })
                    .collect()
            })
            .collect();
        for i in 0..n {
            for f in 0..self.schema.messages[i].fields.len() {
                if let Type::Message(MessageId(j)) = self.schema.messages[i].fields[f].ty
                    && reaches(&edges, j as usize, i)
                {
                    self.schema.messages[i].fields[f].boxed = true;
                }
            }
        }
    }

    /// Emits H0001 for messages whose fixed section could be smaller if the
    /// fields were renumbered.
    fn padding_hints(&mut self) {
        for index in 0..self.schema.messages.len() {
            let message = &self.schema.messages[index];
            if message.padding == 0 {
                continue;
            }
            let mut reordered: Vec<Field> = message.fields.clone();
            reordered.sort_by_key(|f| (std::cmp::Reverse(slot_align(self, f)), f.id));
            let enums = &self.schema.enums;
            let (best_len, _) = layout::assign(&mut reordered, &|id| enums[id.0 as usize].backing);

            let message = &self.schema.messages[index];
            if best_len >= message.fixed_len as u32 {
                continue;
            }
            let order: Vec<&str> = reordered.iter().map(|f| f.name.as_str()).collect();
            let (file, span) = self.spans[index];
            self.diags.push(
                Diagnostic::hint(
                    "H0001",
                    format!(
                        "`{}` carries {} padding byte(s); renumbering fields would shrink it from {} to {best_len} bytes",
                        message.name, message.padding, message.fixed_len,
                    ),
                )
                .with_label(file, span, "layout is determined by field id order")
                .with_note(format!("smallest layout assigns ascending ids in this order: {}", order.join(", ")))
                .with_help("renumbering changes the wire format; only do this before the schema ships"),
            );
        }
    }
}

fn slot_align(resolver: &Resolver<'_>, field: &Field) -> u32 {
    match field.ty {
        Type::Scalar(ScalarType::Bool) => 0,
        Type::Scalar(s) => s.size(),
        Type::Enum(id) => resolver.schema.enums[id.0 as usize].backing.size(),
        Type::String | Type::Bytes | Type::List(_) | Type::Message(_) => 4,
    }
}

fn default_proto(ty: Type) -> ProtoEncoding {
    match ty {
        Type::Scalar(s) | Type::List(ElemType::Scalar(s)) => {
            if s.is_float() {
                ProtoEncoding::Fixed
            } else if s.is_signed() {
                ProtoEncoding::ZigZag
            } else {
                ProtoEncoding::Varint
            }
        }
        Type::Enum(_) | Type::List(ElemType::Enum(_)) => ProtoEncoding::Varint,
        _ => ProtoEncoding::LengthDelimited,
    }
}

fn reaches(edges: &[Vec<usize>], from: usize, to: usize) -> bool {
    let mut stack = vec![from];
    let mut visited = vec![false; edges.len()];
    while let Some(node) = stack.pop() {
        if node == to {
            return true;
        }
        if std::mem::replace(&mut visited[node], true) {
            continue;
        }
        stack.extend(edges[node].iter().copied());
    }
    false
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut current = vec![0; b.len() + 1];
    for (i, &ca) in a.iter().enumerate() {
        current[0] = i + 1;
        for (j, &cb) in b.iter().enumerate() {
            let substitution = prev[j] + usize::from(ca != cb);
            current[j + 1] = substitution.min(prev[j + 1] + 1).min(current[j] + 1);
        }
        std::mem::swap(&mut prev, &mut current);
    }
    prev[b.len()]
}
