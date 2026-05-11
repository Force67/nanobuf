//! Fixed-section layout assignment.
//!
//! Implements the normative algorithm from `docs/wire-format.md`: fields are
//! placed in ascending id order by a hole-filling bump allocator, so adding
//! a field with a fresh id never moves existing fields.

use crate::ir::{BitAddr, DataSlot, EnumId, Field, FieldLayout, ScalarType, Type};

/// Byte and bit allocator for one message's fixed section.
pub struct Allocator {
    cursor: u32,
    /// Free padding ranges `(offset, len)`, sorted by offset.
    holes: Vec<(u32, u32)>,
    /// Current bit pool byte and the number of bits handed out from it.
    bits: Option<(u32, u8)>,
}

impl Allocator {
    pub fn new() -> Self {
        // The cursor starts after the two `fixed_len` header bytes.
        Self {
            cursor: 2,
            holes: Vec::new(),
            bits: None,
        }
    }

    /// Allocates `size` bytes at `align`, reusing padding holes first.
    pub fn alloc(&mut self, size: u32, align: u32) -> u32 {
        for i in 0..self.holes.len() {
            let (hole_start, hole_len) = self.holes[i];
            let start = hole_start.next_multiple_of(align);
            if start + size <= hole_start + hole_len {
                self.holes.remove(i);
                let mut insert = i;
                if start > hole_start {
                    self.holes.insert(insert, (hole_start, start - hole_start));
                    insert += 1;
                }
                let tail = hole_start + hole_len - (start + size);
                if tail > 0 {
                    self.holes.insert(insert, (start + size, tail));
                }
                return start;
            }
        }
        let start = self.cursor.next_multiple_of(align);
        if start > self.cursor {
            self.holes.push((self.cursor, start - self.cursor));
        }
        self.cursor = start + size;
        start
    }

    /// Allocates one bit from the pool, growing it by a byte when full.
    pub fn alloc_bit(&mut self) -> BitAddr {
        match self.bits {
            Some((byte, used)) if used < 8 => {
                self.bits = Some((byte, used + 1));
                BitAddr {
                    byte: byte as u16,
                    bit: used,
                }
            }
            _ => {
                let byte = self.alloc(1, 1);
                self.bits = Some((byte, 1));
                BitAddr {
                    byte: byte as u16,
                    bit: 0,
                }
            }
        }
    }

    /// Bytes used so far, including the header. Never less than 2.
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> u32 {
        self.cursor
    }

    /// Unused interior bytes (padding holes).
    pub fn padding(&self) -> u32 {
        self.holes.iter().map(|&(_, len)| len).sum()
    }
}

impl Default for Allocator {
    fn default() -> Self {
        Self::new()
    }
}

/// Width in bytes of an enum's backing type, looked up via `backing`.
pub type EnumWidth<'a> = &'a dyn Fn(EnumId) -> ScalarType;

/// Assigns layout to `fields` (which must be sorted by ascending id) and
/// returns `(fixed_len, padding)`. Sizes exceeding `u16` are the caller's
/// responsibility to reject.
pub fn assign(fields: &mut [Field], enum_backing: EnumWidth<'_>) -> (u32, u32) {
    let mut alloc = Allocator::new();
    for field in fields.iter_mut() {
        let presence = needs_presence_bit(field).then(|| alloc.alloc_bit());
        let data = match field.ty {
            Type::Scalar(ScalarType::Bool) => DataSlot::Bit(alloc.alloc_bit()),
            Type::Scalar(scalar) => place_bytes(&mut alloc, scalar.size()),
            Type::Enum(id) => place_bytes(&mut alloc, enum_backing(id).size()),
            Type::String | Type::Bytes | Type::List(_) | Type::Message(_) => DataSlot::Offset {
                offset: alloc.alloc(4, 4) as u16,
            },
        };
        field.layout = FieldLayout { presence, data };
    }
    (alloc.len(), alloc.padding())
}

fn place_bytes(alloc: &mut Allocator, size: u32) -> DataSlot {
    DataSlot::Bytes {
        offset: alloc.alloc(size, size) as u16,
        size: size as u8,
    }
}

fn needs_presence_bit(field: &Field) -> bool {
    field.optional && !field.ty.is_heap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alloc_bumps_and_aligns() {
        let mut a = Allocator::new();
        assert_eq!(a.alloc(1, 1), 2);
        assert_eq!(a.alloc(4, 4), 4);
        assert_eq!(a.alloc(8, 8), 8);
        assert_eq!(a.len(), 16);
        assert_eq!(a.padding(), 1);
    }

    #[test]
    fn alloc_fills_holes_first() {
        let mut a = Allocator::new();
        a.alloc(8, 8); // leaves hole at 2..8
        assert_eq!(a.alloc(4, 4), 4);
        assert_eq!(a.alloc(2, 2), 2);
        assert_eq!(a.padding(), 0);
        assert_eq!(a.len(), 16);
    }

    #[test]
    fn alloc_splits_holes_with_alignment() {
        let mut a = Allocator::new();
        a.alloc(8, 8); // hole 2..8
        assert_eq!(a.alloc(1, 1), 2); // hole 3..8
        assert_eq!(a.alloc(2, 2), 4); // holes 3..4 and 6..8
        assert_eq!(a.alloc(1, 1), 3);
        assert_eq!(a.alloc(2, 2), 6);
        assert_eq!(a.padding(), 0);
    }

    #[test]
    fn bits_share_a_pool_byte() {
        let mut a = Allocator::new();
        let b0 = a.alloc_bit();
        let b1 = a.alloc_bit();
        assert_eq!((b0.byte, b0.bit), (2, 0));
        assert_eq!((b1.byte, b1.bit), (2, 1));
        assert_eq!(a.len(), 3);
    }

    #[test]
    fn ninth_bit_opens_a_new_pool_byte() {
        let mut a = Allocator::new();
        for _ in 0..8 {
            a.alloc_bit();
        }
        let b = a.alloc_bit();
        assert_eq!((b.byte, b.bit), (3, 0));
        assert_eq!(a.len(), 4);
    }

    #[test]
    fn empty_fixed_section_is_just_the_header() {
        assert_eq!(Allocator::new().len(), 2);
    }
}
