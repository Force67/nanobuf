//! Proto3 frontend for migration.
//!
//! [`migrate`] parses a proto3 `.proto` file and emits equivalent nanobuf
//! IDL. Proto integer encodings that differ from nanobuf's defaults are
//! preserved with `[proto = ...]` attributes so migration-mode wire bytes
//! stay byte-compatible with the original schema.

mod lexer;
mod parser;

use nanobuf_diagnostics::{Diagnostics, FileId};

/// Converts proto3 source to nanobuf IDL source. Returns `None` when
/// errors were reported into `diags`.
pub fn migrate(file: FileId, text: &str, diags: &mut Diagnostics) -> Option<String> {
    let proto_file = parser::parse(file, text, diags)?;
    if diags.has_errors() {
        return None;
    }
    Some(render(&proto_file))
}

/// A parsed proto3 file, reduced to what nanobuf can express.
struct ProtoFile {
    package: String,
    imports: Vec<String>,
    decls: Vec<Decl>,
}

enum Decl {
    Message(ProtoMessage),
    Enum(ProtoEnum),
}

struct ProtoMessage {
    /// Flattened name: nested `Outer.Inner` becomes `OuterInner`.
    name: String,
    fields: Vec<ProtoField>,
    reserved: Vec<u32>,
}

struct ProtoField {
    name: String,
    /// Proto type name, possibly a user-defined type reference.
    proto_type: String,
    repeated: bool,
    optional: bool,
    id: u32,
}

struct ProtoEnum {
    name: String,
    variants: Vec<(String, i64)>,
}

/// Scalar mapping: proto type, nanobuf type, attribute needed to keep the
/// proto3 wire encoding (None when nanobuf's default already matches).
const SCALARS: &[(&str, &str, Option<&str>)] = &[
    ("double", "f64", None),
    ("float", "f32", None),
    ("int32", "i32", Some("int32")),
    ("int64", "i64", Some("int64")),
    ("uint32", "u32", None),
    ("uint64", "u64", None),
    ("sint32", "i32", None),
    ("sint64", "i64", None),
    ("fixed32", "u32", Some("fixed32")),
    ("fixed64", "u64", Some("fixed64")),
    ("sfixed32", "i32", Some("sfixed32")),
    ("sfixed64", "i64", Some("sfixed64")),
    ("bool", "bool", None),
    ("string", "string", None),
    ("bytes", "bytes", None),
];

fn render(file: &ProtoFile) -> String {
    let mut out = String::new();
    out.push_str(&format!("schema {}\n", file.package));
    for import in &file.imports {
        let nb = import.trim_end_matches(".proto");
        out.push_str(&format!("\nimport \"{nb}.nb\""));
    }
    if !file.imports.is_empty() {
        out.push('\n');
    }
    for decl in &file.decls {
        out.push('\n');
        match decl {
            Decl::Enum(e) => render_enum(&mut out, e),
            Decl::Message(m) => render_message(&mut out, m),
        }
    }
    out
}

fn render_enum(out: &mut String, e: &ProtoEnum) {
    out.push_str(&format!("enum {}: i32 {{\n", e.name));
    for (name, value) in &e.variants {
        out.push_str(&format!("  {} = {value}\n", variant_name(name)));
    }
    out.push_str("}\n");
}

/// Proto variants are SHOUTY_CASE; nanobuf uses lower_snake_case.
fn variant_name(name: &str) -> String {
    name.to_lowercase()
}

fn render_message(out: &mut String, m: &ProtoMessage) {
    out.push_str(&format!("message {} {{\n", m.name));
    for f in &m.fields {
        let (base, attr) = match SCALARS.iter().find(|&&(p, _, _)| p == f.proto_type) {
            Some(&(_, nb, attr)) => (nb.to_string(), attr),
            // User-defined type: already flattened/qualified by the parser.
            None => (f.proto_type.clone(), None),
        };
        let ty = if f.repeated {
            format!("list<{base}>")
        } else if f.optional && SCALARS.iter().any(|&(p, _, _)| p == f.proto_type) {
            format!("{base}?")
        } else {
            base
        };
        out.push_str(&format!("  {}: {ty} @{}", field_name(&f.name), f.id));
        if let Some(attr) = attr {
            out.push_str(&format!(" [proto = {attr}]"));
        }
        out.push('\n');
    }
    if !m.reserved.is_empty() {
        let ids: Vec<String> = m.reserved.iter().map(|id| format!("@{id}")).collect();
        out.push_str(&format!("\n  reserved {}\n", ids.join(", ")));
    }
    out.push_str("}\n");
}

/// Proto fields are usually snake_case already; camelCase gets converted.
fn field_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 && !out.ends_with('_') {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}
