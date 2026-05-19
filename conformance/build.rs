//! Generates Rust code from the conformance schemas at build time.

use std::path::Path;

use nanobuf_diagnostics::{Diagnostics, SourceMap};
use nanobuf_schema::Unit;

fn main() {
    println!("cargo::rerun-if-changed=schema");

    let mut sources = SourceMap::new();
    let mut diags = Diagnostics::new();
    let mut units = Vec::new();
    for name in ["schema/common.nb", "schema/telemetry.nb"] {
        let text = std::fs::read_to_string(name).expect(name);
        let file = sources.add(name, text.clone());
        let ast = nanobuf_syntax::parse(file, &text, &mut diags)
            .unwrap_or_else(|| panic!("{}", diags.render(&sources, false)));
        units.push(Unit { file, ast });
    }
    let schema = nanobuf_schema::analyze(&units, &mut diags)
        .unwrap_or_else(|| panic!("{}", diags.render(&sources, false)));

    let files = nanobuf_codegen::generate(&schema, nanobuf_codegen::Lang::Rust, "gen")
        .expect("rust generation succeeds");
    let out_dir = std::env::var("OUT_DIR").unwrap();
    for file in files {
        std::fs::write(Path::new(&out_dir).join(file.name), file.content).unwrap();
    }
}
