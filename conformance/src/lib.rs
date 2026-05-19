//! Generated conformance types plus the shared sample values used by every
//! language's test suite.

include!(concat!(env!("OUT_DIR"), "/gen.rs"));

use common::Header;
use demo_telemetry::{Everything, Status, Tree, Wide};

/// The canonical sample message. Mirrored byte-for-byte by the other
/// language test suites; change those together with this.
pub fn sample_everything() -> Everything {
    Everything {
        flag: true,
        tiny: 7,
        small: -1234,
        medium: 0xdead_beef,
        big: u64::MAX / 3,
        sneg: -5_000_000_000,
        ratio: 1.25,
        precise: -2.5e10,
        status: Status::OK,
        wide: Wide::NEG,
        name: "telemetry probe".to_string(),
        blob: vec![0, 1, 2, 254, 255],
        maybe_flag: Some(false),
        maybe_small: Some(-7),
        maybe_precise: None,
        maybe_status: Some(Status::DEGRADED),
        maybe_name: Some(String::new()),
        maybe_blob: None,
        numbers: vec![1, 2, 3, 65535],
        floats: vec![0.5, -0.5],
        statuses: vec![Status::OK, Status::UNKNOWN, Status(200)],
        tags: vec!["a".to_string(), String::new(), "long tag value".to_string()],
        blobs: vec![vec![1], vec![], vec![2, 3]],
        headers: vec![
            Header {
                source: 1,
                region: "eu".to_string(),
            },
            Header {
                source: 2,
                region: String::new(),
            },
        ],
        header: Some(Header {
            source: 42,
            region: "us-east".to_string(),
        }),
        fixed_int: -123456,
        raw_int: -42,
        flags: vec![true, false, true],
    }
}

pub fn sample_tree() -> Tree {
    Tree {
        value: 1,
        left: Some(Box::new(Tree {
            value: 2,
            left: None,
            right: Some(Box::new(Tree {
                value: 3,
                left: None,
                right: None,
            })),
        })),
        right: None,
    }
}
