use crate::scalar::Scalar;

/// Single-buffer message writer.
///
/// Generated `write` implementations call [`Writer::begin`] to reserve
/// their fixed section, patch field slots, and append heap entries.
/// Nested messages are written depth-first into the same buffer, so one
/// encode performs at most one allocation.
pub struct Writer {
    buf: Vec<u8>,
    /// Start of the current message; heap offsets are relative to it.
    base: usize,
}

impl Writer {
    pub fn new() -> Self {
        Writer {
            buf: Vec::new(),
            base: 0,
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Writer {
            buf: Vec::with_capacity(capacity),
            base: 0,
        }
    }

    pub fn into_vec(self) -> Vec<u8> {
        self.buf
    }

    /// Starts a message at the current position: writes the header and
    /// zero-fills the rest of the fixed section.
    pub fn begin(&mut self, fixed_len: u16) {
        self.base = self.buf.len();
        self.buf.extend_from_slice(&fixed_len.to_le_bytes());
        self.buf.resize(self.base + fixed_len as usize, 0);
    }

    /// Writes a scalar into the fixed section.
    #[inline]
    pub fn put_scalar<T: Scalar>(&mut self, offset: u16, value: T) {
        let at = self.base + offset as usize;
        value.store(&mut self.buf[at..]);
    }

    #[inline]
    pub fn put_bit(&mut self, byte: u16, bit: u8, value: bool) {
        if value {
            self.buf[self.base + byte as usize] |= 1 << bit;
        }
    }

    /// Appends a `[len][payload]` heap entry and returns its
    /// message-relative offset.
    pub fn heap_bytes(&mut self, payload: &[u8]) -> u32 {
        let entry = self.buf.len();
        self.buf
            .extend_from_slice(&(payload.len() as u32).to_le_bytes());
        self.buf.extend_from_slice(payload);
        (entry - self.base) as u32
    }

    /// Appends a nested message entry; `write_body` serializes the child.
    /// Returns the entry's message-relative offset.
    pub fn heap_message(&mut self, write_body: impl FnOnce(&mut Writer)) -> u32 {
        // Message payloads are 8-aligned relative to the enclosing message.
        self.pad_payload_to(8);
        let entry = self.buf.len();
        let offset = (entry - self.base) as u32;
        self.buf.extend_from_slice(&[0; 4]);
        let outer_base = self.base;
        let body_start = self.buf.len();
        write_body(self);
        let len = (self.buf.len() - body_start) as u32;
        self.buf[entry..entry + 4].copy_from_slice(&len.to_le_bytes());
        self.base = outer_base;
        offset
    }

    /// Patches an offset slot in the fixed section.
    pub fn set_slot(&mut self, slot: u16, heap_offset: u32) {
        let at = self.base + slot as usize;
        self.buf[at..at + 4].copy_from_slice(&heap_offset.to_le_bytes());
    }

    /// `string`/`bytes` field: writes the entry unless the value is empty
    /// and `force` is false (absent and empty read identically then).
    pub fn put_bytes(&mut self, slot: u16, payload: &[u8], force: bool) {
        if payload.is_empty() && !force {
            return;
        }
        let offset = self.heap_bytes(payload);
        self.set_slot(slot, offset);
    }

    /// Message-typed field.
    pub fn put_message(&mut self, slot: u16, write_body: impl FnOnce(&mut Writer)) {
        let offset = self.heap_message(write_body);
        self.set_slot(slot, offset);
    }

    /// `list<scalar>` / `list<enum>` field. Empty lists are absent.
    pub fn put_scalar_list<T: Scalar>(&mut self, slot: u16, items: &[T]) {
        if items.is_empty() {
            return;
        }
        self.pad_payload_to(T::SIZE.max(1));
        let entry = self.buf.len();
        self.buf
            .extend_from_slice(&(items.len() as u32).to_le_bytes());
        for &item in items {
            item.write(&mut self.buf);
        }
        self.set_slot(slot, (entry - self.base) as u32);
    }

    /// `list<string|bytes|message>` field: writes the offset array, then
    /// one entry per item via `write_item` (which returns each entry's
    /// message-relative offset, e.g. from [`Writer::heap_bytes`]).
    pub fn put_offset_list<T>(
        &mut self,
        slot: u16,
        items: &[T],
        mut write_item: impl FnMut(&mut Writer, &T) -> u32,
    ) {
        if items.is_empty() {
            return;
        }
        let entry = self.buf.len();
        self.buf
            .extend_from_slice(&(items.len() as u32).to_le_bytes());
        let table = self.buf.len();
        self.buf.resize(table + items.len() * 4, 0);
        for (i, item) in items.iter().enumerate() {
            let offset = write_item(self, item);
            let at = table + i * 4;
            self.buf[at..at + 4].copy_from_slice(&offset.to_le_bytes());
        }
        self.set_slot(slot, (entry - self.base) as u32);
    }

    /// Pads so the next entry's payload (after its 4-byte length/count
    /// prefix) lands `align`-aligned relative to the message start.
    fn pad_payload_to(&mut self, align: usize) {
        let payload_pos = self.buf.len() - self.base + 4;
        let target = payload_pos.next_multiple_of(align);
        self.buf.resize(self.buf.len() + (target - payload_pos), 0);
    }
}

impl Default for Writer {
    fn default() -> Self {
        Self::new()
    }
}
