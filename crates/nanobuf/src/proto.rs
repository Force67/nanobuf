//! Proto3 wire format codec, used only in migration mode.

use crate::error::DecodeError;

/// Proto3 wire types. Groups (3 and 4) are not supported.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WireType {
    Varint = 0,
    I64 = 1,
    Len = 2,
    I32 = 5,
}

/// One decoded field value.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum WireValue<'a> {
    Varint(u64),
    I64(u64),
    Len(&'a [u8]),
    I32(u32),
}

pub fn read_varint(buf: &[u8], pos: &mut usize) -> Result<u64, DecodeError> {
    let mut value = 0u64;
    for i in 0..10 {
        let byte = *buf.get(*pos).ok_or(DecodeError::Varint)?;
        *pos += 1;
        // The tenth byte may only carry the final bit of a u64.
        if i == 9 && byte > 1 {
            return Err(DecodeError::Varint);
        }
        value |= u64::from(byte & 0x7f) << (i * 7);
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err(DecodeError::Varint)
}

pub fn write_varint(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

#[inline]
pub fn zigzag_encode(value: i64) -> u64 {
    ((value << 1) ^ (value >> 63)) as u64
}

#[inline]
pub fn zigzag_decode(value: u64) -> i64 {
    ((value >> 1) as i64) ^ -((value & 1) as i64)
}

/// Iterates `(field_number, value)` pairs of one proto3 message.
/// Unknown fields are simply skipped by ignoring the yielded value.
pub struct FieldReader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> FieldReader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        FieldReader { buf, pos: 0 }
    }

    #[allow(clippy::should_implement_trait)] // Iterator cannot yield Results this way.
    pub fn next(&mut self) -> Result<Option<(u32, WireValue<'a>)>, DecodeError> {
        if self.pos >= self.buf.len() {
            return Ok(None);
        }
        let tag = read_varint(self.buf, &mut self.pos)?;
        let field = u32::try_from(tag >> 3).map_err(|_| DecodeError::Varint)?;
        if field == 0 {
            return Err(DecodeError::WireType);
        }
        let value = match tag & 7 {
            0 => WireValue::Varint(read_varint(self.buf, &mut self.pos)?),
            1 => WireValue::I64(self.fixed::<8>().map(u64::from_le_bytes)?),
            2 => {
                let len = read_varint(self.buf, &mut self.pos)? as usize;
                let payload = self
                    .buf
                    .get(self.pos..self.pos + len)
                    .ok_or(DecodeError::Truncated)?;
                self.pos += len;
                WireValue::Len(payload)
            }
            5 => WireValue::I32(self.fixed::<4>().map(u32::from_le_bytes)?),
            _ => return Err(DecodeError::WireType),
        };
        Ok(Some((field, value)))
    }

    fn fixed<const N: usize>(&mut self) -> Result<[u8; N], DecodeError> {
        let bytes = self
            .buf
            .get(self.pos..self.pos + N)
            .ok_or(DecodeError::Truncated)?;
        self.pos += N;
        Ok(bytes.try_into().unwrap())
    }
}

pub fn write_tag(out: &mut Vec<u8>, field: u32, wire_type: WireType) {
    write_varint(out, (u64::from(field) << 3) | wire_type as u64);
}

pub fn write_varint_field(out: &mut Vec<u8>, field: u32, value: u64) {
    write_tag(out, field, WireType::Varint);
    write_varint(out, value);
}

pub fn write_i32_field(out: &mut Vec<u8>, field: u32, value: u32) {
    write_tag(out, field, WireType::I32);
    out.extend_from_slice(&value.to_le_bytes());
}

pub fn write_i64_field(out: &mut Vec<u8>, field: u32, value: u64) {
    write_tag(out, field, WireType::I64);
    out.extend_from_slice(&value.to_le_bytes());
}

pub fn write_len_field(out: &mut Vec<u8>, field: u32, payload: &[u8]) {
    write_tag(out, field, WireType::Len);
    write_varint(out, payload.len() as u64);
    out.extend_from_slice(payload);
}

/// Writes a length-delimited field whose payload is produced by `body`.
/// Used for nested messages and packed lists.
pub fn write_len_field_with(out: &mut Vec<u8>, field: u32, body: impl FnOnce(&mut Vec<u8>)) {
    let mut payload = Vec::new();
    body(&mut payload);
    write_len_field(out, field, &payload);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_round_trips() {
        for value in [0, 1, 127, 128, 300, u32::MAX as u64, u64::MAX] {
            let mut out = Vec::new();
            write_varint(&mut out, value);
            let mut pos = 0;
            assert_eq!(read_varint(&out, &mut pos), Ok(value));
            assert_eq!(pos, out.len());
        }
    }

    #[test]
    fn varint_rejects_truncation_and_overflow() {
        let mut pos = 0;
        assert_eq!(read_varint(&[0x80], &mut pos), Err(DecodeError::Varint));
        let mut pos = 0;
        assert_eq!(read_varint(&[0x80; 11], &mut pos), Err(DecodeError::Varint));
        let mut pos = 0;
        let mut too_big = vec![0xff; 9];
        too_big.push(0x02); // tenth byte may only be 0 or 1
        assert_eq!(read_varint(&too_big, &mut pos), Err(DecodeError::Varint));
    }

    #[test]
    fn zigzag_round_trips() {
        for value in [0, -1, 1, -2, i64::MIN, i64::MAX] {
            assert_eq!(zigzag_decode(zigzag_encode(value)), value);
        }
        assert_eq!(zigzag_encode(-1), 1);
        assert_eq!(zigzag_encode(1), 2);
    }

    #[test]
    fn field_reader_walks_all_wire_types() {
        let mut buf = Vec::new();
        write_varint_field(&mut buf, 1, 150);
        write_i32_field(&mut buf, 2, 0xdead_beef);
        write_i64_field(&mut buf, 3, 0x0102_0304_0506_0708);
        write_len_field(&mut buf, 4, b"hi");

        let mut reader = FieldReader::new(&buf);
        assert_eq!(reader.next(), Ok(Some((1, WireValue::Varint(150)))));
        assert_eq!(reader.next(), Ok(Some((2, WireValue::I32(0xdead_beef)))));
        assert_eq!(
            reader.next(),
            Ok(Some((3, WireValue::I64(0x0102_0304_0506_0708))))
        );
        assert_eq!(reader.next(), Ok(Some((4, WireValue::Len(b"hi")))));
        assert_eq!(reader.next(), Ok(None));
    }

    #[test]
    fn field_reader_rejects_groups_and_zero_fields() {
        // Field 1, wire type 3 (start group).
        let mut reader = FieldReader::new(&[0x0b]);
        assert_eq!(reader.next(), Err(DecodeError::WireType));
        // Field 0 is invalid.
        let mut reader = FieldReader::new(&[0x00, 0x00]);
        assert_eq!(reader.next(), Err(DecodeError::WireType));
    }

    #[test]
    fn field_reader_rejects_truncated_payloads() {
        let mut buf = Vec::new();
        write_tag(&mut buf, 1, WireType::Len);
        write_varint(&mut buf, 100);
        buf.push(0xaa);
        let mut reader = FieldReader::new(&buf);
        assert_eq!(reader.next(), Err(DecodeError::Truncated));

        let mut buf = Vec::new();
        write_tag(&mut buf, 1, WireType::I64);
        buf.extend_from_slice(&[1, 2, 3]);
        let mut reader = FieldReader::new(&buf);
        assert_eq!(reader.next(), Err(DecodeError::Truncated));
    }

    #[test]
    fn nested_length_delimited_writer() {
        let mut buf = Vec::new();
        write_len_field_with(&mut buf, 2, |inner| write_varint_field(inner, 1, 7));
        let mut reader = FieldReader::new(&buf);
        let Ok(Some((2, WireValue::Len(payload)))) = reader.next() else {
            panic!()
        };
        let mut inner = FieldReader::new(payload);
        assert_eq!(inner.next(), Ok(Some((1, WireValue::Varint(7)))));
    }
}
