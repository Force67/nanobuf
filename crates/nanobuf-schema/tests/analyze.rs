use nanobuf_diagnostics::{Diagnostics, Severity, SourceMap};
use nanobuf_schema::{
    BitAddr, DataSlot, ElemType, ProtoEncoding, ScalarType, Schema, Type, Unit, analyze,
};

fn analyze_sources(sources_text: &[&str]) -> (Option<Schema>, Vec<String>) {
    let mut sources = SourceMap::new();
    let mut diags = Diagnostics::new();
    let mut units = Vec::new();
    for (i, text) in sources_text.iter().enumerate() {
        let file = sources.add(format!("t{i}.nb"), *text);
        let ast = nanobuf_syntax::parse(file, text, &mut diags).expect("test input must parse");
        units.push(Unit { file, ast });
    }
    assert!(!diags.has_errors(), "{}", diags.render(&sources, false));
    let schema = analyze(&units, &mut diags);
    let rendered: Vec<String> = diags
        .iter()
        .map(|d| {
            format!(
                "{}{}",
                d.code,
                if d.severity == Severity::Hint {
                    ":hint"
                } else {
                    ""
                }
            )
        })
        .collect();
    (schema, rendered)
}

fn ok(text: &str) -> Schema {
    let (schema, diags) = analyze_sources(&[text]);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    schema.expect("expected analysis to succeed")
}

fn codes(text: &str) -> Vec<String> {
    analyze_sources(&[text]).1
}

fn field<'a>(schema: &'a Schema, message: &str, name: &str) -> &'a nanobuf_schema::Field {
    let m = schema.messages.iter().find(|m| m.name == message).unwrap();
    m.fields.iter().find(|f| f.name == name).unwrap()
}

fn data_offset(schema: &Schema, message: &str, name: &str) -> u16 {
    match field(schema, message, name).layout.data {
        DataSlot::Bytes { offset, .. } | DataSlot::Offset { offset } => offset,
        DataSlot::Bit(_) => panic!("bit slot"),
    }
}

#[test]
fn resolves_types_layout_and_packages() {
    // This layout is intentionally hint-worthy (ids are not size-sorted);
    // only the offsets matter here.
    let (schema, diags) = analyze_sources(&[r#"schema demo

enum Status: u16 {
  unknown = 0
  ok = 1
}

message Header {
  source: u32 @1
}

message Packet {
  sequence: u64 @1
  status: Status @2
  active: bool @3
  temperature: f64? @4
  name: string @5
  samples: list<u16> @6
  header: Header @7
}
"#]);
    assert!(diags.iter().all(|d| d.ends_with(":hint")), "{diags:?}");
    let schema = schema.expect("expected analysis to succeed");
    assert_eq!(schema.packages(), vec!["demo"]);

    // Field ids place: seq u64 -> bumps to 8 (hole 2..8); status u16 -> 2;
    // active bool -> bit pool byte at 4; temperature presence bit shares the
    // pool, its f64 lands at 16; offset slots follow 4-aligned.
    assert_eq!(data_offset(&schema, "Packet", "sequence"), 8);
    assert_eq!(data_offset(&schema, "Packet", "status"), 2);
    assert_eq!(
        field(&schema, "Packet", "active").layout.data,
        DataSlot::Bit(BitAddr { byte: 4, bit: 0 })
    );
    assert_eq!(
        field(&schema, "Packet", "temperature").layout.presence,
        Some(BitAddr { byte: 4, bit: 1 })
    );
    assert_eq!(data_offset(&schema, "Packet", "temperature"), 16);
    assert_eq!(data_offset(&schema, "Packet", "name"), 24);
    assert_eq!(data_offset(&schema, "Packet", "samples"), 28);
    assert_eq!(data_offset(&schema, "Packet", "header"), 32);

    assert_eq!(
        field(&schema, "Packet", "status").ty,
        Type::Enum(nanobuf_schema::EnumId(0))
    );
    assert!(field(&schema, "Packet", "temperature").optional);
    assert!(!field(&schema, "Packet", "header").optional);
    assert_eq!(
        field(&schema, "Packet", "samples").ty,
        Type::List(ElemType::Scalar(ScalarType::U16))
    );
}

#[test]
fn adding_a_field_never_moves_existing_ones() {
    let base = "schema a\nmessage M {\n  a: u8 @1\n  b: u64 @2\n  c: string @3\n  d: bool @4\n}\n";
    let extended = "schema a\nmessage M {\n  a: u8 @1\n  b: u64 @2\n  c: string @3\n  d: bool @4\n  e: u32 @5\n  f: bool @6\n}\n";
    let old = ok(base);
    let (new, _) = analyze_sources(&[extended]);
    let new = new.unwrap();
    for name in ["a", "b", "c", "d"] {
        assert_eq!(
            field(&old, "M", name).layout,
            field(&new, "M", name).layout,
            "field {name} moved"
        );
    }
    assert!(new.messages[0].fixed_len >= old.messages[0].fixed_len);
}

#[test]
fn new_fields_fill_old_padding_holes() {
    let schema = ok("schema a\nmessage M {\n  b: u64 @2\n  e: u16 @5\n}\n");
    // u64 leaves hole 2..8; u16 fills part of it.
    assert_eq!(data_offset(&schema, "M", "e"), 2);
    assert_eq!(schema.messages[0].fixed_len, 16);
}

#[test]
fn bools_pack_into_shared_bytes() {
    let schema =
        ok("schema a\nmessage M {\n  a: bool @1\n  b: bool @2\n  c: bool? @3\n  d: u8 @4\n}\n");
    let a = field(&schema, "M", "a");
    let b = field(&schema, "M", "b");
    let c = field(&schema, "M", "c");
    assert_eq!(a.layout.data, DataSlot::Bit(BitAddr { byte: 2, bit: 0 }));
    assert_eq!(b.layout.data, DataSlot::Bit(BitAddr { byte: 2, bit: 1 }));
    assert_eq!(c.layout.presence, Some(BitAddr { byte: 2, bit: 2 }));
    assert_eq!(c.layout.data, DataSlot::Bit(BitAddr { byte: 2, bit: 3 }));
    // 2 header + 1 bit byte + 1 u8: nothing wasted.
    assert_eq!(schema.messages[0].fixed_len, 4);
    assert_eq!(schema.messages[0].padding, 0);
}

#[test]
fn optional_heap_fields_use_no_presence_bit() {
    let schema = ok("schema a\nmessage M {\n  s: string? @1\n  b: bytes? @2\n}\n");
    assert_eq!(field(&schema, "M", "s").layout.presence, None);
    assert!(field(&schema, "M", "s").optional);
    // Header (2) + alignment hole (2) + two 4-byte offset slots.
    assert_eq!(schema.messages[0].fixed_len, 12);
}

#[test]
fn cross_package_references_resolve() {
    let (schema, diags) = analyze_sources(&[
        "schema common\nmessage Header {\n  source: u32 @1\n}\n",
        "schema demo\nimport \"common.nb\"\nmessage Packet {\n  header: common.Header @1\n}\n",
    ]);
    assert!(diags.is_empty(), "{diags:?}");
    let schema = schema.unwrap();
    assert_eq!(schema.packages(), vec!["common", "demo"]);
    assert!(matches!(
        field(&schema, "Packet", "header").ty,
        Type::Message(_)
    ));
}

#[test]
fn proto_defaults_follow_type() {
    let schema = ok(
        "schema a\nmessage M {\n  a: u32 @1\n  b: i32 @2\n  c: f64 @3\n  d: string @4\n  e: i32 @5 [proto = int32]\n  f: list<i32> @6 [proto = sfixed32]\n}\n",
    );
    assert_eq!(field(&schema, "M", "a").proto, ProtoEncoding::Varint);
    assert_eq!(field(&schema, "M", "b").proto, ProtoEncoding::ZigZag);
    assert_eq!(field(&schema, "M", "c").proto, ProtoEncoding::Fixed);
    assert_eq!(
        field(&schema, "M", "d").proto,
        ProtoEncoding::LengthDelimited
    );
    assert_eq!(field(&schema, "M", "e").proto, ProtoEncoding::Varint);
    assert_eq!(field(&schema, "M", "f").proto, ProtoEncoding::Fixed);
}

#[test]
fn self_recursive_messages_are_boxed() {
    let schema = ok("schema a\nmessage Tree {\n  left: Tree @1\n  value: u32 @2\n}\n");
    assert!(field(&schema, "Tree", "left").boxed);
}

#[test]
fn mutual_recursion_is_boxed_but_diamonds_are_not() {
    let schema = ok(
        "schema a\nmessage A {\n  b: B @1\n}\nmessage B {\n  a: A @1\n}\nmessage C {\n  a: A @1\n  b: B @2\n}\n",
    );
    assert!(field(&schema, "A", "b").boxed);
    assert!(field(&schema, "B", "a").boxed);
    assert!(!field(&schema, "C", "a").boxed);
    assert!(!field(&schema, "C", "b").boxed);
}

#[test]
fn list_indirection_does_not_box() {
    let schema = ok("schema a\nmessage Node {\n  children: list<Node> @1\n}\n");
    assert!(!field(&schema, "Node", "children").boxed);
}

#[test]
fn message_order_places_dependencies_first() {
    let schema =
        ok("schema a\nmessage Outer {\n  inner: Inner @1\n}\nmessage Inner {\n  x: u32 @1\n}\n");
    let order = schema.message_order();
    let names: Vec<&str> = order
        .iter()
        .map(|&id| schema.message(id).name.as_str())
        .collect();
    assert_eq!(names, vec!["Inner", "Outer"]);
}

#[test]
fn padding_hint_fires_when_renumbering_would_shrink() {
    // In id order: u8 at 2, u64 at 8 (hole 3..8), u32 fills 4..8, u16 cannot
    // fit the 1-byte hole at 3 and lands at 16, for 18 bytes total.
    // Renumbered largest-first the same fields pack into 17 bytes.
    let (_, diags) = analyze_sources(&[
        "schema a\nmessage M {\n  a: u8 @1\n  b: u64 @2\n  c: u32 @3\n  d: u16 @4\n}\n",
    ]);
    assert_eq!(diags, vec!["H0001:hint"]);
}

#[test]
fn tight_layout_gets_no_hint() {
    let (_, diags) = analyze_sources(&["schema a\nmessage M {\n  b: u64 @1\n  a: u8 @2\n}\n"]);
    // u64 at 8 leaves hole 2..8 which u8 fills partially; remaining padding
    // cannot be removed by reordering (header is 2 bytes), so stay silent.
    assert!(diags.iter().all(|d| d != "H0001:hint"), "{diags:?}");
}

#[test]
fn empty_message_is_two_header_bytes() {
    let schema = ok("schema a\nmessage Empty {\n}\n");
    assert_eq!(schema.messages[0].fixed_len, 2);
}

// Error cases.

#[test]
fn duplicate_type_name() {
    assert_eq!(
        codes("schema a\nmessage M {\n}\nenum M: u8 {\n  z = 0\n}\n"),
        vec!["E0100"]
    );
}

#[test]
fn builtin_shadow_rejected() {
    assert!(codes("schema a\nmessage string {\n}\n").contains(&"E0101".to_string()));
}

#[test]
fn invalid_enum_backing() {
    assert_eq!(
        codes("schema a\nenum E: u64 {\n  z = 0\n}\n"),
        vec!["E0102"]
    );
}

#[test]
fn duplicate_variant_name_and_value() {
    assert_eq!(
        codes("schema a\nenum E: u8 {\n  z = 0\n  z = 1\n}\n"),
        vec!["E0103"]
    );
    assert_eq!(
        codes("schema a\nenum E: u8 {\n  z = 0\n  y = 0\n}\n"),
        vec!["E0104"]
    );
}

#[test]
fn variant_out_of_backing_range() {
    assert_eq!(
        codes("schema a\nenum E: u8 {\n  z = 0\n  big = 256\n}\n"),
        vec!["E0105"]
    );
    assert_eq!(
        codes("schema a\nenum E: u8 {\n  z = 0\n  neg = -1\n}\n"),
        vec!["E0105"]
    );
}

#[test]
fn empty_enum_rejected() {
    assert_eq!(codes("schema a\nenum E: u8 {\n}\n"), vec!["E0106"]);
}

#[test]
fn missing_zero_variant_warns() {
    assert_eq!(
        codes("schema a\nenum E: u8 {\n  one = 1\n}\n"),
        vec!["W0002"]
    );
}

#[test]
fn unknown_type_with_suggestion() {
    let mut sources = SourceMap::new();
    let text = "schema a\nmessage M {\n  x: Statu @1\n}\nenum Status: u8 {\n  unknown = 0\n}\n";
    let file = sources.add("t.nb", text);
    let mut diags = Diagnostics::new();
    let ast = nanobuf_syntax::parse(file, text, &mut diags).unwrap();
    assert!(analyze(&[Unit { file, ast }], &mut diags).is_none());
    let rendered = diags.render(&sources, false);
    assert!(rendered.contains("E0107"), "{rendered}");
    assert!(rendered.contains("did you mean `Status`?"), "{rendered}");
}

#[test]
fn proto_type_names_get_targeted_error() {
    let mut sources = SourceMap::new();
    let text = "schema a\nmessage M {\n  x: int32 @1\n}\n";
    let file = sources.add("t.nb", text);
    let mut diags = Diagnostics::new();
    let ast = nanobuf_syntax::parse(file, text, &mut diags).unwrap();
    assert!(analyze(&[Unit { file, ast }], &mut diags).is_none());
    let rendered = diags.render(&sources, false);
    assert!(rendered.contains("nanobuf calls this `i32`"), "{rendered}");
}

#[test]
fn duplicate_field_name_and_id() {
    assert_eq!(
        codes("schema a\nmessage M {\n  x: u8 @1\n  x: u8 @2\n}\n"),
        vec!["E0108"]
    );
    assert_eq!(
        codes("schema a\nmessage M {\n  x: u8 @1\n  y: u8 @1\n}\n"),
        vec!["E0109"]
    );
}

#[test]
fn field_id_range_enforced() {
    assert_eq!(
        codes("schema a\nmessage M {\n  x: u8 @0\n}\n"),
        vec!["E0110"]
    );
    assert_eq!(
        codes("schema a\nmessage M {\n  x: u8 @536870912\n}\n"),
        vec!["E0110"]
    );
    assert_eq!(
        codes("schema a\nmessage M {\n  x: u8 @19000\n}\n"),
        vec!["W0003"]
    );
}

#[test]
fn reserved_id_use_rejected() {
    assert_eq!(
        codes("schema a\nmessage M {\n  reserved @3\n  x: u8 @3\n}\n"),
        vec!["E0111"]
    );
}

#[test]
fn optional_message_and_list_rejected() {
    assert_eq!(
        codes("schema a\nmessage M {\n  m: N? @1\n}\nmessage N {\n}\n"),
        vec!["E0112"]
    );
    assert_eq!(
        codes("schema a\nmessage M {\n  l: list<u8>? @1\n}\n"),
        vec!["E0113"]
    );
}

#[test]
fn nested_and_optional_list_elements_rejected() {
    assert_eq!(
        codes("schema a\nmessage M {\n  l: list<list<u8>> @1\n}\n"),
        vec!["E0114"]
    );
    assert_eq!(
        codes("schema a\nmessage M {\n  l: list<u8?> @1\n}\n"),
        vec!["E0115"]
    );
}

#[test]
fn proto_attr_validation() {
    assert_eq!(
        codes("schema a\nmessage M {\n  x: u32 @1 [proto = int32]\n}\n"),
        vec!["E0116"]
    );
    assert_eq!(
        codes("schema a\nmessage M {\n  x: i32 @1 [proto = banana]\n}\n"),
        vec!["E0117"]
    );
    assert_eq!(
        codes("schema a\nmessage M {\n  s: string @1 [proto = int32]\n}\n"),
        vec!["E0116"]
    );
}

#[test]
fn naming_style_warnings() {
    assert_eq!(codes("schema a\nmessage my_msg {\n}\n"), vec!["W0004"]);
    assert_eq!(
        codes("schema a\nmessage M {\n  camelCase: u8 @1\n}\n"),
        vec!["W0005"]
    );
    assert_eq!(
        codes("schema a\nenum E: u8 {\n  Zero = 0\n}\n"),
        vec!["W0005"]
    );
}

#[test]
fn fixed_section_size_limit_enforced() {
    let mut text = String::from("schema a\nmessage M {\n");
    for i in 1..=8200 {
        text.push_str(&format!("  f{i}: u64 @{i}\n"));
    }
    text.push_str("}\n");
    assert_eq!(codes(&text), vec!["E0118"]);
}
