//! Exercises Writer and View together the way generated code uses them,
//! including the forward/backward compatibility behavior.

use nanobuf::{DecodeError, View, Writer};

// Hand-assigned layout used across these tests:
//   0..2   fixed_len header (= 24)
//   2      bit byte: bit 0 = active, bit 1 = temperature presence
//   3      u8 count
//   4..8   u32 sequence
//   8..16  f64 temperature
//   16..20 string slot (name)
//   20..24 list<u16> slot (samples)
const FIXED_LEN: u16 = 24;

struct Packet {
    active: bool,
    count: u8,
    sequence: u32,
    temperature: Option<f64>,
    name: String,
    samples: Vec<u16>,
}

impl Packet {
    fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();
        self.write(&mut w);
        w.into_vec()
    }

    fn write(&self, w: &mut Writer) {
        w.begin(FIXED_LEN);
        w.put_bit(2, 0, self.active);
        w.put_scalar(3, self.count);
        w.put_scalar(4, self.sequence);
        if let Some(t) = self.temperature {
            w.put_bit(2, 1, true);
            w.put_scalar(8, t);
        }
        w.put_bytes(16, self.name.as_bytes(), false);
        w.put_scalar_list(20, &self.samples);
    }
}

fn sample() -> Packet {
    Packet {
        active: true,
        count: 7,
        sequence: 0xaabbccdd,
        temperature: Some(20.5),
        name: "probe".into(),
        samples: vec![1, 2, 300],
    }
}

#[test]
fn round_trips_through_view() {
    let bytes = sample().encode();
    let v = View::parse(&bytes).unwrap();
    assert!(v.bit_at(2, 0));
    assert!(v.bit_at(2, 1));
    assert_eq!(v.scalar_at::<u8>(3), 7);
    assert_eq!(v.scalar_at::<u32>(4), 0xaabbccdd);
    assert_eq!(v.scalar_at::<f64>(8), 20.5);
    assert_eq!(v.str_at(16), Ok(Some("probe")));
    let samples = v.scalar_list_at::<u16>(20).unwrap();
    assert_eq!(samples.to_vec(), vec![1, 2, 300]);
    assert_eq!(samples.get(2), Some(300));
    assert_eq!(samples.get(3), None);
}

#[test]
fn golden_bytes_for_minimal_message() {
    let packet = Packet {
        active: false,
        count: 0,
        sequence: 1,
        temperature: None,
        name: String::new(),
        samples: Vec::new(),
    };
    let bytes = packet.encode();
    let mut expected = vec![0u8; 24];
    expected[0] = 24; // fixed_len
    expected[4] = 1; // sequence, little-endian
    assert_eq!(bytes, expected);
}

#[test]
fn empty_string_without_force_reads_absent() {
    let mut packet = sample();
    packet.name = String::new();
    let bytes = packet.encode();
    let v = View::parse(&bytes).unwrap();
    assert_eq!(v.str_at(16), Ok(None));
}

#[test]
fn forced_empty_string_reads_present() {
    let mut w = Writer::new();
    w.begin(FIXED_LEN);
    w.put_bytes(16, b"", true);
    let bytes = w.into_vec();
    let v = View::parse(&bytes).unwrap();
    assert_eq!(v.str_at(16), Ok(Some("")));
}

#[test]
fn new_reader_sees_defaults_in_old_data() {
    // An "old writer" knew only the first 8 bytes (header + bits + count +
    // sequence). The new reader's offsets all lie beyond its fixed_len.
    let mut w = Writer::new();
    w.begin(8);
    w.put_bit(2, 0, true);
    w.put_scalar(4, 42u32);
    let bytes = w.into_vec();

    let v = View::parse(&bytes).unwrap();
    assert!(v.bit_at(2, 0));
    assert_eq!(v.scalar_at::<u32>(4), 42);
    // Fields the old writer never had:
    assert_eq!(v.scalar_at::<f64>(8), 0.0);
    assert!(!v.bit_at(2, 1) || v.scalar_at::<f64>(8) == 0.0);
    assert_eq!(v.str_at(16), Ok(None));
    assert!(v.scalar_list_at::<u16>(20).unwrap().is_empty());
}

#[test]
fn old_reader_ignores_new_trailing_fields() {
    // A "new writer" appended a u32 at offset 24..28.
    let mut w = Writer::new();
    w.begin(28);
    w.put_scalar(4, 42u32);
    w.put_scalar(24, 99u32);
    w.put_bytes(16, b"probe", false);
    let bytes = w.into_vec();

    // The old reader only knows offsets below 24; everything it reads is
    // unaffected by the new field.
    let v = View::parse(&bytes).unwrap();
    assert_eq!(v.scalar_at::<u32>(4), 42);
    assert_eq!(v.str_at(16), Ok(Some("probe")));
}

#[test]
fn nested_messages_are_self_contained_and_aligned() {
    let mut w = Writer::new();
    w.begin(8); // header + bits pad + slot at 4
    w.put_message(4, |w| {
        w.begin(8);
        w.put_scalar(4, 7u32);
    });
    let bytes = w.into_vec();

    let outer = View::parse(&bytes).unwrap();
    let inner = outer.message_at(4).unwrap().unwrap();
    assert_eq!(inner.scalar_at::<u32>(4), 7);
    // The child payload is 8-aligned relative to the message start.
    let slot = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    assert_eq!((slot + 4) % 8, 0);
    // The child is a contiguous, independently parseable slice.
    assert!(View::parse(inner.data()).is_ok());
}

#[test]
fn scalar_list_payloads_are_element_aligned() {
    let mut w = Writer::new();
    w.begin(8);
    w.put_scalar_list(4, &[1.5f64, 2.5]);
    let bytes = w.into_vec();
    let slot = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    assert_eq!((slot + 4) % 8, 0);
    let v = View::parse(&bytes).unwrap();
    assert_eq!(v.scalar_list_at::<f64>(4).unwrap().to_vec(), vec![1.5, 2.5]);
}

#[test]
fn offset_lists_round_trip_strings_and_messages() {
    let names = vec!["a".to_string(), String::new(), "ccc".to_string()];
    let mut w = Writer::new();
    w.begin(12); // slots at 4 and 8
    w.put_offset_list(4, &names, |w, name| w.heap_bytes(name.as_bytes()));
    let values = [3u32, 9u32];
    w.put_offset_list(8, &values, |w, &value| {
        w.heap_message(|w| {
            w.begin(8);
            w.put_scalar(4, value);
        })
    });
    let bytes = w.into_vec();

    let v = View::parse(&bytes).unwrap();
    let list = v.offset_list_at(4).unwrap();
    assert_eq!(list.len(), 3);
    assert_eq!(list.str(0), Ok("a"));
    assert_eq!(list.str(1), Ok(""));
    assert_eq!(list.str(2), Ok("ccc"));
    assert_eq!(list.bytes(3), Err(DecodeError::Truncated));

    let messages = v.offset_list_at(8).unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages.message(0).unwrap().scalar_at::<u32>(4), 3);
    assert_eq!(messages.message(1).unwrap().scalar_at::<u32>(4), 9);

    let absent = v.offset_list_at(2);
    assert!(absent.is_err() || absent.unwrap().is_empty());
}

#[test]
fn parse_rejects_bad_headers() {
    assert_eq!(View::parse(&[]).unwrap_err(), DecodeError::Header);
    assert_eq!(View::parse(&[5]).unwrap_err(), DecodeError::Header);
    assert_eq!(View::parse(&[1, 0]).unwrap_err(), DecodeError::Header);
    assert_eq!(
        View::parse(&[10, 0, 0]).unwrap_err(),
        DecodeError::Truncated
    );
    assert!(View::parse(&[2, 0]).is_ok());
}

#[test]
fn corrupt_heap_offsets_are_rejected_not_panicking() {
    // Slot at 4 points past the end of the buffer.
    let mut bytes = vec![8, 0, 0, 0, 200, 0, 0, 0];
    let v = View::parse(&bytes).unwrap();
    assert_eq!(v.bytes_at(4), Err(DecodeError::Truncated));

    // Entry length runs past the end.
    bytes = vec![8, 0, 0, 0, 8, 0, 0, 0, 255, 0, 0, 0];
    let v = View::parse(&bytes).unwrap();
    assert_eq!(v.bytes_at(4), Err(DecodeError::Truncated));

    // List count overflows the buffer.
    bytes = vec![8, 0, 0, 0, 8, 0, 0, 0, 255, 255, 255, 255];
    let v = View::parse(&bytes).unwrap();
    assert!(v.scalar_list_at::<u64>(4).is_err());
    assert!(v.offset_list_at(4).is_err());
}

#[test]
fn invalid_utf8_is_an_error_not_a_panic() {
    let mut w = Writer::new();
    w.begin(8);
    w.put_bytes(4, &[0xff, 0xfe], false);
    let bytes = w.into_vec();
    let v = View::parse(&bytes).unwrap();
    assert_eq!(v.str_at(4), Err(DecodeError::Utf8));
    assert_eq!(v.bytes_at(4), Ok(Some(&[0xff, 0xfe][..])));
}

#[test]
fn views_can_be_reencoded_from_a_sub_message_slice() {
    // Forwarding: extract a nested message and treat it as a root packet.
    let mut w = Writer::new();
    w.begin(8);
    w.put_message(4, |w| {
        w.begin(8);
        w.put_bytes(4, b"inner", false);
    });
    let bytes = w.into_vec();
    let inner = View::parse(&bytes).unwrap().message_at(4).unwrap().unwrap();
    let forwarded = inner.data().to_vec();
    let v = View::parse(&forwarded).unwrap();
    assert_eq!(v.str_at(4), Ok(Some("inner")));
}
