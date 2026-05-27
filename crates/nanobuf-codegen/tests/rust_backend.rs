use nanobuf_diagnostics::{Diagnostics, SourceMap};
use nanobuf_schema::Unit;

fn generate_rust(text: &str) -> String {
    let mut sources = SourceMap::new();
    let mut diags = Diagnostics::new();
    let file = sources.add("t.nb", text);
    let ast = nanobuf_syntax::parse(file, text, &mut diags).unwrap();
    let schema = nanobuf_schema::analyze(&[Unit { file, ast }], &mut diags)
        .unwrap_or_else(|| panic!("{}", diags.render(&sources, false)));
    let files = nanobuf_codegen::generate(&schema, nanobuf_codegen::Lang::Rust, "out").unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].name, "out.rs");
    files[0].content.clone()
}

#[test]
fn output_is_deterministic() {
    let text = "schema a\nmessage M {\n  x: u32 @1\n  s: string @2\n}\n";
    assert_eq!(generate_rust(text), generate_rust(text));
}

#[test]
fn generates_expected_api_surface() {
    let code = generate_rust(
        "schema demo\nenum Status: u8 {\n  unknown = 0\n  ok = 1\n}\nmessage Packet {\n  status: Status @1\n  name: string @2\n  maybe: f64? @3\n}\n",
    );
    for expected in [
        "pub mod demo {",
        "pub struct Status(pub u8);",
        "pub const UNKNOWN: Status = Status(0);",
        "impl nanobuf::Scalar for Status {",
        "pub struct Packet {",
        "pub status: Status,",
        "pub maybe: Option<f64>,",
        "pub fn encode(&self) -> Vec<u8> {",
        "pub fn decode(data: &[u8]) -> Result<Packet, nanobuf::DecodeError> {",
        "pub struct PacketView<'a> {",
        "pub fn encode_proto3(&self) -> Vec<u8> {",
        "pub fn decode_proto3(data: &[u8]) -> Result<Packet, nanobuf::DecodeError> {",
        "impl<'a> nanobuf::FromView<'a> for PacketView<'a> {",
    ] {
        assert!(code.contains(expected), "missing `{expected}` in:\n{code}");
    }
}

#[test]
fn doc_comments_are_carried_through() {
    let code = generate_rust(
        "schema a\n/// The packet.\nmessage Packet {\n  /// Field doc.\n  x: u32 @1\n}\n",
    );
    assert!(code.contains("/// The packet."));
    assert!(code.contains("/// Field doc."));
}

#[test]
fn keyword_field_names_are_escaped() {
    let code = generate_rust("schema a\nmessage M {\n  type: u32 @1\n  match: bool @2\n}\n");
    assert!(code.contains("pub r#type: u32,"), "{code}");
    assert!(code.contains("pub r#match: bool,"), "{code}");
}

#[test]
fn reserved_view_method_names_get_prefixed() {
    let code = generate_rust("schema a\nmessage M {\n  data: bytes @1\n  parse: u32 @2\n}\n");
    assert!(code.contains("pub fn get_data(&self)"), "{code}");
    assert!(code.contains("pub fn get_parse(&self)"), "{code}");
    // The owned struct keeps the plain field names.
    assert!(code.contains("pub data: Vec<u8>,"), "{code}");
}

#[test]
fn recursive_fields_are_boxed() {
    let code = generate_rust("schema a\nmessage Tree {\n  left: Tree @1\n}\n");
    assert!(code.contains("pub left: Option<Box<Tree>>,"), "{code}");
}
