//! Generates Rust bindings for `examples/sensor.nb` at build time.

use std::path::Path;

use nanobuf_diagnostics::{Diagnostics, SourceMap};
use nanobuf_schema::Unit;

fn main() {
    println!("cargo::rerun-if-changed=../sensor.nb");

    let text = std::fs::read_to_string("../sensor.nb").expect("examples/sensor.nb");
    let mut sources = SourceMap::new();
    let mut diags = Diagnostics::new();
    let file = sources.add("../sensor.nb", text.clone());
    let ast = nanobuf_syntax::parse(file, &text, &mut diags)
        .unwrap_or_else(|| panic!("{}", diags.render(&sources, false)));
    let schema = nanobuf_schema::analyze(&[Unit { file, ast }], &mut diags)
        .unwrap_or_else(|| panic!("{}", diags.render(&sources, false)));

    let files = nanobuf_codegen::generate(&schema, nanobuf_codegen::Lang::Rust, "sensor")
        .expect("rust generation succeeds");
    let out_dir = std::env::var("OUT_DIR").unwrap();
    for file in files {
        std::fs::write(Path::new(&out_dir).join(file.name), file.content).unwrap();
    }
}
