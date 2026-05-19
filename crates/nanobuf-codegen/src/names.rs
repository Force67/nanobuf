//! Case conversion and keyword escaping shared by the backends.

pub fn pascal(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut upper = true;
    for c in input.chars() {
        if c == '_' {
            upper = true;
        } else if upper {
            out.extend(c.to_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

pub fn camel(input: &str) -> String {
    let mut out = pascal(input);
    if let Some(first) = out.get(..1) {
        let lower = first.to_lowercase();
        out.replace_range(..1, &lower);
    }
    out
}

pub fn snake(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for (i, c) in input.chars().enumerate() {
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

pub fn shout(input: &str) -> String {
    snake(input).to_uppercase()
}

const RUST_KEYWORDS: &[&str] = &[
    "abstract", "as", "async", "await", "become", "box", "break", "const", "continue", "do", "dyn",
    "else", "enum", "extern", "false", "final", "fn", "for", "gen", "if", "impl", "in", "let",
    "loop", "macro", "match", "mod", "move", "mut", "override", "priv", "pub", "ref", "return",
    "static", "struct", "trait", "true", "try", "type", "unsafe", "use", "virtual", "where",
    "while", "yield",
];

/// Rust identifier for a schema member name, raw-escaped when needed.
pub fn rust_ident(name: &str) -> String {
    let name = snake(name);
    // These cannot be raw identifiers; fall back to a trailing underscore.
    if matches!(name.as_str(), "self" | "super" | "crate") {
        return format!("{name}_");
    }
    if RUST_KEYWORDS.contains(&name.as_str()) {
        return format!("r#{name}");
    }
    name
}

/// Rust module name for a package: `demo.telemetry` -> `demo_telemetry`.
pub fn rust_module(package: &str) -> String {
    rust_ident(&package.replace('.', "_"))
}

const PYTHON_KEYWORDS: &[&str] = &[
    "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class", "continue",
    "def", "del", "elif", "else", "except", "finally", "for", "from", "global", "if", "import",
    "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while",
    "with", "yield",
];

pub fn python_ident(name: &str) -> String {
    let name = snake(name);
    if PYTHON_KEYWORDS.contains(&name.as_str()) {
        return format!("{name}_");
    }
    name
}

const CSHARP_KEYWORDS: &[&str] = &[
    "abstract",
    "as",
    "base",
    "bool",
    "break",
    "byte",
    "case",
    "catch",
    "char",
    "checked",
    "class",
    "const",
    "continue",
    "decimal",
    "default",
    "delegate",
    "do",
    "double",
    "else",
    "enum",
    "event",
    "explicit",
    "extern",
    "false",
    "finally",
    "fixed",
    "float",
    "for",
    "foreach",
    "goto",
    "if",
    "implicit",
    "in",
    "int",
    "interface",
    "internal",
    "is",
    "lock",
    "long",
    "namespace",
    "new",
    "null",
    "object",
    "operator",
    "out",
    "override",
    "params",
    "private",
    "protected",
    "public",
    "readonly",
    "ref",
    "return",
    "sbyte",
    "sealed",
    "short",
    "sizeof",
    "stackalloc",
    "static",
    "string",
    "struct",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "uint",
    "ulong",
    "unchecked",
    "unsafe",
    "ushort",
    "using",
    "virtual",
    "void",
    "volatile",
    "while",
];

pub fn csharp_ident(name: &str) -> String {
    let name = pascal(name);
    if CSHARP_KEYWORDS.contains(&name.as_str()) {
        return format!("@{name}");
    }
    name
}

const CPP_KEYWORDS: &[&str] = &[
    "alignas",
    "alignof",
    "and",
    "asm",
    "auto",
    "bitand",
    "bitor",
    "bool",
    "break",
    "case",
    "catch",
    "char",
    "class",
    "compl",
    "concept",
    "const",
    "consteval",
    "constexpr",
    "constinit",
    "continue",
    "decltype",
    "default",
    "delete",
    "do",
    "double",
    "else",
    "enum",
    "explicit",
    "export",
    "extern",
    "false",
    "float",
    "for",
    "friend",
    "goto",
    "if",
    "inline",
    "int",
    "long",
    "mutable",
    "namespace",
    "new",
    "noexcept",
    "not",
    "nullptr",
    "operator",
    "or",
    "private",
    "protected",
    "public",
    "register",
    "requires",
    "return",
    "short",
    "signed",
    "sizeof",
    "static",
    "struct",
    "switch",
    "template",
    "this",
    "throw",
    "true",
    "try",
    "typedef",
    "typeid",
    "typename",
    "union",
    "unsigned",
    "using",
    "virtual",
    "void",
    "volatile",
    "while",
    "xor",
];

pub fn cpp_ident(name: &str) -> String {
    let name = snake(name);
    if CPP_KEYWORDS.contains(&name.as_str()) {
        return format!("{name}_");
    }
    name
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn case_conversions() {
        assert_eq!(pascal("max_speed"), "MaxSpeed");
        assert_eq!(pascal("Header"), "Header");
        assert_eq!(camel("max_speed"), "maxSpeed");
        assert_eq!(snake("MaxSpeed"), "max_speed");
        assert_eq!(snake("already_snake"), "already_snake");
        assert_eq!(shout("max_speed"), "MAX_SPEED");
    }

    #[test]
    fn keyword_escaping() {
        assert_eq!(rust_ident("type"), "r#type");
        assert_eq!(rust_ident("self"), "self_");
        assert_eq!(rust_ident("name"), "name");
        assert_eq!(python_ident("class"), "class_");
        assert_eq!(csharp_ident("string"), "String");
        assert_eq!(csharp_ident("event"), "Event");
        assert_eq!(cpp_ident("template"), "template_");
    }

    #[test]
    fn module_names() {
        assert_eq!(rust_module("demo.telemetry"), "demo_telemetry");
        assert_eq!(rust_module("common"), "common");
    }
}
