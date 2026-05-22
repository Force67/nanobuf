use nanobuf_diagnostics::{Diagnostics, SourceMap};

fn migrate_ok(text: &str) -> String {
    let mut sources = SourceMap::new();
    let file = sources.add("t.proto", text);
    let mut diags = Diagnostics::new();
    let result = nanobuf_proto::migrate(file, text, &mut diags);
    assert!(!diags.has_errors(), "{}", diags.render(&sources, false));
    result.expect("migration should succeed")
}

fn migrate_err(text: &str) -> Vec<&'static str> {
    let mut sources = SourceMap::new();
    let file = sources.add("t.proto", text);
    let mut diags = Diagnostics::new();
    let result = nanobuf_proto::migrate(file, text, &mut diags);
    assert!(result.is_none());
    diags.iter().map(|d| d.code).collect()
}

const KITCHEN_SINK: &str = r#"
syntax = "proto3";

package demo.legacy;

import "common.proto";

option java_multiple_files = true;

// Plain comment.
/* Block comment. */
enum Level {
  LEVEL_UNSPECIFIED = 0;
  LEVEL_HIGH = 1;
}

message Reading {
  int32 raw = 1;
  sint32 delta = 2;
  fixed64 stamp = 3;
  repeated int32 history = 4 [packed = true];
  optional string label = 5;
  Level level = 6;
  common.Header header = 7;
  bytes payload = 8;
  double exact = 9;

  reserved 10, 12 to 14;
  reserved "old_name";

  message Inner {
    bool set = 1;
  }
  Inner inner = 15;
}
"#;

#[test]
fn migrates_the_kitchen_sink() {
    let nb = migrate_ok(KITCHEN_SINK);
    let expected = r#"schema demo.legacy

import "common.nb"

enum Level: i32 {
  level_unspecified = 0
  level_high = 1
}

message ReadingInner {
  set: bool @1
}

message Reading {
  raw: i32 @1 [proto = int32]
  delta: i32 @2
  stamp: u64 @3 [proto = fixed64]
  history: list<i32> @4 [proto = int32]
  label: string? @5
  level: Level @6
  header: common.Header @7
  payload: bytes @8
  exact: f64 @9
  inner: ReadingInner @15

  reserved @10, @12, @13, @14
}
"#;
    assert_eq!(nb, expected);
}

#[test]
fn migrated_output_compiles_as_nanobuf() {
    let common = migrate_ok(
        "syntax = \"proto3\";\npackage common;\nmessage Header {\n  uint32 source = 1;\n}\n",
    );
    let nb = migrate_ok(KITCHEN_SINK);

    let mut sources = SourceMap::new();
    let mut diags = Diagnostics::new();
    let mut units = Vec::new();
    for (name, text) in [("common.nb", &common), ("t.nb", &nb)] {
        let file = sources.add(name, text.clone());
        let ast = nanobuf_syntax::parse(file, text, &mut diags)
            .unwrap_or_else(|| panic!("{}", diags.render(&sources, false)));
        units.push(nanobuf_schema::Unit { file, ast });
    }
    let schema = nanobuf_schema::analyze(&units, &mut diags);
    assert!(
        !diags.has_errors(),
        "migrated schema does not compile:\n{}",
        diags.render(&sources, false)
    );
    let schema = schema.unwrap();
    assert_eq!(schema.messages.len(), 3);
    // Wire encodings carried over for migration mode.
    let reading = schema
        .messages
        .iter()
        .find(|m| m.name == "Reading")
        .unwrap();
    let raw = reading.fields.iter().find(|f| f.name == "raw").unwrap();
    assert_eq!(raw.proto, nanobuf_schema::ProtoEncoding::Varint);
    let delta = reading.fields.iter().find(|f| f.name == "delta").unwrap();
    assert_eq!(delta.proto, nanobuf_schema::ProtoEncoding::ZigZag);
    let stamp = reading.fields.iter().find(|f| f.name == "stamp").unwrap();
    assert_eq!(stamp.proto, nanobuf_schema::ProtoEncoding::Fixed);
}

#[test]
fn proto2_files_are_rejected() {
    assert_eq!(
        migrate_err("syntax = \"proto2\";\npackage a;\n"),
        vec!["E0204"]
    );
    assert_eq!(migrate_err("package a;\n"), vec!["E0204"]);
}

#[test]
fn missing_package_is_rejected() {
    assert_eq!(
        migrate_err("syntax = \"proto3\";\nmessage M {\n}\n"),
        vec!["E0206"]
    );
}

#[test]
fn oneof_and_map_are_rejected_with_guidance() {
    assert_eq!(
        migrate_err(
            "syntax = \"proto3\";\npackage a;\nmessage M {\n  oneof kind {\n    int32 a = 1;\n  }\n}\n"
        ),
        vec!["E0207"]
    );
    assert_eq!(
        migrate_err(
            "syntax = \"proto3\";\npackage a;\nmessage M {\n  map<string, int32> m = 1;\n}\n"
        ),
        vec!["E0208"]
    );
}

#[test]
fn services_are_skipped_with_warning() {
    let mut sources = SourceMap::new();
    let text = "syntax = \"proto3\";\npackage a;\nservice S {\n  rpc Get (X) returns (Y);\n}\nmessage X {\n}\n";
    let file = sources.add("t.proto", text);
    let mut diags = Diagnostics::new();
    let nb = nanobuf_proto::migrate(file, text, &mut diags).unwrap();
    assert!(diags.iter().any(|d| d.code == "W0201"));
    assert!(nb.contains("message X"));
    assert!(!nb.contains("service"));
}

#[test]
fn unterminated_constructs_error() {
    assert!(migrate_err("syntax = \"proto3\";\npackage a;\nmessage M {\n").contains(&"E0212"));
    assert!(migrate_err("syntax = \"proto3\";\npackage a;\n/* open").contains(&"E0201"));
}
