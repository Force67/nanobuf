//! Code generators for nanobuf schemas.
//!
//! All layout decisions live in the schema IR; backends only translate it
//! into source text, so every language reads and writes identical bytes.

mod cpp;
mod csharp;
pub mod names;
mod python;
mod rust;
pub mod writer;

use nanobuf_schema::Schema;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    Rust,
    Python,
    CSharp,
    Cpp,
}

impl Lang {
    pub fn parse(name: &str) -> Option<Lang> {
        Some(match name {
            "rust" => Lang::Rust,
            "python" => Lang::Python,
            "csharp" => Lang::CSharp,
            "cpp" => Lang::Cpp,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Lang::Rust => "rust",
            Lang::Python => "python",
            Lang::CSharp => "csharp",
            Lang::Cpp => "cpp",
        }
    }
}

pub struct GeneratedFile {
    /// File name relative to the output directory.
    pub name: String,
    pub content: String,
}

/// Generates code for `schema`. `stem` names the output file(s), typically
/// the root schema file's stem.
pub fn generate(schema: &Schema, lang: Lang, stem: &str) -> Result<Vec<GeneratedFile>, String> {
    check_module_collisions(schema)?;
    let file = match lang {
        Lang::Rust => GeneratedFile {
            name: format!("{stem}.rs"),
            content: rust::generate(schema),
        },
        Lang::Python => GeneratedFile {
            name: format!("{stem}.py"),
            content: python::generate(schema),
        },
        Lang::CSharp => GeneratedFile {
            name: format!("{stem}.cs"),
            content: csharp::generate(schema),
        },
        Lang::Cpp => GeneratedFile {
            name: format!("{stem}.nb.h"),
            content: cpp::generate(schema),
        },
    };
    Ok(vec![file])
}

fn check_module_collisions(schema: &Schema) -> Result<(), String> {
    let packages = schema.packages();
    for (i, a) in packages.iter().enumerate() {
        for b in &packages[i + 1..] {
            if names::rust_module(a) == names::rust_module(b) {
                return Err(format!(
                    "packages `{a}` and `{b}` map to the same generated module name"
                ));
            }
        }
    }
    Ok(())
}
