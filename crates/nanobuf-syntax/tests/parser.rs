use nanobuf_diagnostics::{Diagnostics, SourceMap};
use nanobuf_syntax::ast::{Decl, File, TypeExpr};

fn parse_ok(text: &str) -> File {
    let mut sources = SourceMap::new();
    let file = sources.add("t.nb", text);
    let mut diags = Diagnostics::new();
    let ast = nanobuf_syntax::parse(file, text, &mut diags);
    assert!(diags.is_empty(), "{}", diags.render(&sources, false));
    ast.expect("expected a parsed file")
}

fn parse_err(text: &str) -> (Option<File>, Vec<&'static str>) {
    let mut sources = SourceMap::new();
    let file = sources.add("t.nb", text);
    let mut diags = Diagnostics::new();
    let ast = nanobuf_syntax::parse(file, text, &mut diags);
    let codes: Vec<_> = diags.iter().map(|d| d.code).collect();
    assert!(!codes.is_empty(), "expected at least one diagnostic");
    (ast, codes)
}

fn message(file: &File, index: usize) -> &nanobuf_syntax::ast::MessageDecl {
    match &file.decls[index] {
        Decl::Message(m) => m,
        other => panic!("expected message, got {other:?}"),
    }
}

#[test]
fn parses_full_schema() {
    let file = parse_ok(
        r#"schema demo.telemetry

import "common.nb"

/// Health of a link.
enum Status: u8 {
  ok = 0
  degraded = 1
}

/// One sample frame.
message Packet {
  sequence: u32 @1
  status: Status @2
  temperature: f64? @3
  name: string @4
  samples: list<u16> @5
  header: common.Header @6

  reserved @7, @8
}
"#,
    );
    assert_eq!(file.package.dotted(), "demo.telemetry");
    assert_eq!(file.imports.len(), 1);
    assert_eq!(file.imports[0].path, "common.nb");

    let Decl::Enum(status) = &file.decls[0] else {
        panic!()
    };
    assert_eq!(status.name.text, "Status");
    assert_eq!(status.backing.as_ref().unwrap().text, "u8");
    assert_eq!(status.docs, vec!["Health of a link."]);
    assert_eq!(status.variants.len(), 2);
    assert_eq!(status.variants[0].name.text, "ok");
    assert_eq!(status.variants[1].value, 1);

    let packet = message(&file, 1);
    assert_eq!(packet.docs, vec!["One sample frame."]);
    assert_eq!(packet.fields.len(), 6);
    assert_eq!(packet.fields[0].name.text, "sequence");
    assert_eq!(packet.fields[0].id, 1);
    assert_eq!(packet.fields[2].ty.to_string(), "f64?");
    assert_eq!(packet.fields[4].ty.to_string(), "list<u16>");
    assert_eq!(packet.fields[5].ty.to_string(), "common.Header");
    assert_eq!(
        packet.reserved.iter().map(|r| r.id).collect::<Vec<_>>(),
        vec![7, 8]
    );
}

#[test]
fn parses_enum_without_backing_and_negative_values() {
    let file = parse_ok("schema a\nenum E {\n  neg = -2\n  hex = 0x10\n}\n");
    let Decl::Enum(e) = &file.decls[0] else {
        panic!()
    };
    assert!(e.backing.is_none());
    assert_eq!(e.variants[0].value, -2);
    assert_eq!(e.variants[1].value, 16);
}

#[test]
fn parses_proto_attribute() {
    let file = parse_ok("schema a\nmessage M {\n  count: i32 @1 [proto = int32]\n}\n");
    let m = message(&file, 0);
    assert_eq!(m.fields[0].proto.as_ref().unwrap().text, "int32");
}

#[test]
fn parses_field_docs() {
    let file =
        parse_ok("schema a\nmessage M {\n  /// Counts things.\n  /// Slowly.\n  n: u8 @1\n}\n");
    let m = message(&file, 0);
    assert_eq!(m.fields[0].docs, vec!["Counts things.", "Slowly."]);
}

#[test]
fn parses_empty_message_and_compact_file() {
    let file = parse_ok("schema a\nmessage M {\n}");
    assert!(message(&file, 0).fields.is_empty());
}

#[test]
fn missing_schema_is_an_error() {
    let (ast, codes) = parse_err("message M {\n}\n");
    assert!(ast.is_none());
    assert_eq!(codes, vec!["E0006"]);
}

#[test]
fn proto_file_detected() {
    let (ast, codes) = parse_err("syntax = \"proto3\";\nmessage M {}\n");
    assert!(ast.is_none());
    assert_eq!(codes, vec!["E0005"]);
}

#[test]
fn proto3_optional_keyword_gets_targeted_help() {
    let (_, codes) = parse_err("schema a\nmessage M {\n  optional string name @1\n}\n");
    assert_eq!(codes, vec!["E0007"]);
}

#[test]
fn proto3_repeated_keyword_gets_targeted_help() {
    let (_, codes) = parse_err("schema a\nmessage M {\n  repeated u32 ids @1\n}\n");
    assert_eq!(codes, vec!["E0007"]);
}

#[test]
fn equals_field_id_gets_targeted_help() {
    let (_, codes) = parse_err("schema a\nmessage M {\n  name: string = 1\n}\n");
    assert_eq!(codes, vec!["E0008"]);
}

#[test]
fn semicolon_gets_targeted_error() {
    let (_, codes) = parse_err("schema a\nmessage M {\n  name: string @1;\n}\n");
    assert_eq!(codes, vec!["E0011"]);
}

#[test]
fn double_optional_rejected() {
    let (_, codes) = parse_err("schema a\nmessage M {\n  x: u32?? @1\n}\n");
    assert_eq!(codes, vec!["E0010"]);
}

#[test]
fn unknown_attribute_rejected() {
    let (_, codes) = parse_err("schema a\nmessage M {\n  x: u32 @1 [packed = true]\n}\n");
    assert_eq!(codes, vec!["E0009"]);
}

#[test]
fn reserved_without_at_gets_help() {
    let (_, codes) = parse_err("schema a\nmessage M {\n  reserved 7\n}\n");
    assert_eq!(codes, vec!["E0008"]);
}

#[test]
fn field_id_overflow_rejected() {
    let (_, codes) = parse_err("schema a\nmessage M {\n  x: u32 @99999999999\n}\n");
    assert_eq!(codes, vec!["E0003"]);
}

#[test]
fn recovers_and_reports_multiple_errors() {
    let (ast, codes) = parse_err(
        "schema a\nmessage M {\n  x u32 @1\n  y: u32 @2\n  z: = 3\n}\nmessage N {\n  ok: bool @1\n}\n",
    );
    assert_eq!(codes.len(), 2, "{codes:?}");
    let file = ast.unwrap();
    assert_eq!(file.decls.len(), 2);
    let m = message(&file, 0);
    assert_eq!(m.fields.len(), 1);
    assert_eq!(m.fields[0].name.text, "y");
    assert_eq!(message(&file, 1).fields.len(), 1);
}

#[test]
fn recovers_from_garbage_at_top_level() {
    let (ast, codes) = parse_err("schema a\n42\nmessage M {\n  x: u32 @1\n}\n");
    assert_eq!(codes, vec!["E0004"]);
    assert_eq!(ast.unwrap().decls.len(), 1);
}

#[test]
fn dangling_doc_comment_warns() {
    let (_, codes) = parse_err("schema a\nmessage M {\n  x: u32 @1\n  /// lost\n}\n");
    assert_eq!(codes, vec!["W0001"]);
}

#[test]
fn unterminated_message_reports_missing_brace() {
    let (_, codes) = parse_err("schema a\nmessage M {\n  x: u32 @1\n");
    assert_eq!(codes, vec!["E0004"]);
}

#[test]
fn nested_list_parses_for_later_validation() {
    let file = parse_ok("schema a\nmessage M {\n  x: list<list<u8>> @1\n}\n");
    let m = message(&file, 0);
    assert!(matches!(
        &m.fields[0].ty,
        TypeExpr::List { elem, .. } if matches!(**elem, TypeExpr::List { .. })
    ));
}

#[test]
fn optional_list_parses_for_later_validation() {
    let file = parse_ok("schema a\nmessage M {\n  x: list<u8>? @1\n}\n");
    assert_eq!(message(&file, 0).fields[0].ty.to_string(), "list<u8>?");
}
