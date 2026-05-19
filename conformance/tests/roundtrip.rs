use nanobuf_conformance::demo_telemetry::{
    Everything, EverythingView, Status, Tree, TreeView, Wide,
};
use nanobuf_conformance::{sample_everything, sample_tree};

#[test]
fn nanobuf_round_trip_preserves_everything() {
    let original = sample_everything();
    let bytes = original.encode();
    let decoded = Everything::decode(&bytes).unwrap();
    assert_eq!(decoded, original);
}

#[test]
fn view_accessors_match_owned_values() {
    let original = sample_everything();
    let bytes = original.encode();
    let v = EverythingView::parse(&bytes).unwrap();

    assert!(v.flag());
    assert_eq!(v.tiny(), 7);
    assert_eq!(v.small(), -1234);
    assert_eq!(v.medium(), 0xdead_beef);
    assert_eq!(v.big(), u64::MAX / 3);
    assert_eq!(v.sneg(), -5_000_000_000);
    assert_eq!(v.ratio(), 1.25);
    assert_eq!(v.precise(), -2.5e10);
    assert_eq!(v.status(), Status::OK);
    assert_eq!(v.wide(), Wide::NEG);
    assert_eq!(v.name().unwrap(), "telemetry probe");
    assert_eq!(v.blob().unwrap(), &[0, 1, 2, 254, 255]);
    assert_eq!(v.maybe_flag(), Some(false));
    assert_eq!(v.maybe_small(), Some(-7));
    assert_eq!(v.maybe_precise(), None);
    assert_eq!(v.maybe_status(), Some(Status::DEGRADED));
    assert_eq!(v.maybe_name().unwrap(), Some(""));
    assert_eq!(v.maybe_blob().unwrap(), None);
    assert_eq!(v.numbers().unwrap().to_vec(), vec![1, 2, 3, 65535]);
    assert_eq!(v.floats().unwrap().to_vec(), vec![0.5, -0.5]);
    assert_eq!(
        v.statuses().unwrap().to_vec(),
        vec![Status::OK, Status::UNKNOWN, Status(200)]
    );
    assert_eq!(
        v.tags().unwrap().to_vec().unwrap(),
        vec!["a", "", "long tag value"]
    );
    assert_eq!(
        v.blobs().unwrap().to_vec().unwrap(),
        vec![vec![1u8], vec![], vec![2, 3]]
    );
    let headers = v.headers().unwrap();
    assert_eq!(headers.len(), 2);
    assert_eq!(headers.get(0).unwrap().source(), 1);
    assert_eq!(headers.get(0).unwrap().region().unwrap(), "eu");
    assert_eq!(headers.get(1).unwrap().source(), 2);
    let header = v.header().unwrap().unwrap();
    assert_eq!(header.source(), 42);
    assert_eq!(header.region().unwrap(), "us-east");
    assert_eq!(v.fixed_int(), -123456);
    assert_eq!(v.raw_int(), -42);
    assert_eq!(v.flags().unwrap().to_vec(), vec![true, false, true]);
}

#[test]
fn enum_names_and_unknown_values() {
    assert_eq!(Status::OK.name(), Some("ok"));
    assert_eq!(Status(200).name(), None);
    assert_eq!(Wide::NEG.0, -5);
    assert_eq!(Status::default(), Status::UNKNOWN);
}

#[test]
fn default_message_encodes_to_fixed_section_only() {
    let bytes = Everything::default().encode();
    let decoded = Everything::decode(&bytes).unwrap();
    assert_eq!(decoded, Everything::default());
    // No heap entries: the buffer is exactly the fixed section.
    let fixed_len = u16::from_le_bytes([bytes[0], bytes[1]]) as usize;
    assert_eq!(bytes.len(), fixed_len);
}

#[test]
fn optional_default_distinction_survives() {
    let message = Everything {
        maybe_flag: Some(false),
        maybe_small: Some(0),
        maybe_name: Some(String::new()),
        ..Everything::default()
    };
    let decoded = Everything::decode(&message.encode()).unwrap();
    assert_eq!(decoded.maybe_flag, Some(false));
    assert_eq!(decoded.maybe_small, Some(0));
    assert_eq!(decoded.maybe_name, Some(String::new()));
    assert_eq!(decoded.maybe_precise, None);
    assert_eq!(decoded.maybe_blob, None);
}

#[test]
fn recursive_trees_round_trip() {
    let tree = sample_tree();
    let bytes = tree.encode();
    assert_eq!(Tree::decode(&bytes).unwrap(), tree);

    let v = TreeView::parse(&bytes).unwrap();
    assert_eq!(v.value(), 1);
    let left = v.left().unwrap().unwrap();
    assert_eq!(left.value(), 2);
    assert_eq!(left.right().unwrap().unwrap().value(), 3);
    assert!(v.right().unwrap().is_none());
}

#[test]
fn sub_message_views_can_be_forwarded() {
    let bytes = sample_everything().encode();
    let v = EverythingView::parse(&bytes).unwrap();
    let header_bytes = v.header().unwrap().unwrap().data().to_vec();
    let header = nanobuf_conformance::common::Header::decode(&header_bytes).unwrap();
    assert_eq!(header.source, 42);
    assert_eq!(header.region, "us-east");
}

#[test]
fn proto3_round_trip_preserves_everything() {
    let original = sample_everything();
    let bytes = original.encode_proto3();
    let decoded = Everything::decode_proto3(&bytes).unwrap();
    assert_eq!(decoded, original);
}

#[test]
fn proto3_round_trips_recursive_trees() {
    let tree = sample_tree();
    let decoded = Tree::decode_proto3(&tree.encode_proto3()).unwrap();
    assert_eq!(decoded, tree);
}

#[test]
fn proto3_decoder_skips_unknown_fields() {
    let mut bytes = sample_everything().encode_proto3();
    // Field 999, varint 5, then field 998, length-delimited "xy".
    nanobuf::proto::write_varint_field(&mut bytes, 999, 5);
    nanobuf::proto::write_len_field(&mut bytes, 998, b"xy");
    let decoded = Everything::decode_proto3(&bytes).unwrap();
    assert_eq!(decoded, sample_everything());
}

#[test]
fn proto3_default_message_is_empty() {
    assert!(Everything::default().encode_proto3().is_empty());
}

#[test]
fn corrupt_buffers_error_cleanly() {
    let bytes = sample_everything().encode();
    // Truncate at various points; parse or accessors must error, not panic.
    for cut in [0, 1, 3, 10, bytes.len() / 2] {
        let cut_bytes = &bytes[..cut];
        if let Ok(v) = EverythingView::parse(cut_bytes) {
            let _ = v.name();
            let _ = v.headers().map(|h| h.get(0));
            let _ = v.to_owned();
        }
    }
}
