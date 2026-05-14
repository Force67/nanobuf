use std::marker::PhantomData;

use crate::error::DecodeError;
use crate::scalar::Scalar;

/// Zero-copy view over one serialized message.
///
/// Scalar accessors are infallible: anything past the writer's fixed
/// section reads as zero, which is how old data yields defaults for new
/// fields. Heap accessors bounds-check the referenced entry and fail with
/// [`DecodeError`] on corrupt input.
#[derive(Clone, Copy, Debug)]
pub struct View<'a> {
    data: &'a [u8],
    fixed_len: usize,
}

impl<'a> View<'a> {
    pub fn parse(data: &'a [u8]) -> Result<View<'a>, DecodeError> {
        if data.len() < 2 {
            return Err(DecodeError::Header);
        }
        let fixed_len = u16::from_le_bytes([data[0], data[1]]) as usize;
        if fixed_len < 2 {
            return Err(DecodeError::Header);
        }
        if fixed_len > data.len() {
            return Err(DecodeError::Truncated);
        }
        Ok(View { data, fixed_len })
    }

    /// The raw message bytes, suitable for re-framing or forwarding.
    pub fn data(&self) -> &'a [u8] {
        self.data
    }

    /// Reads a fixed-section scalar, or zero when the writer's schema was
    /// older and the slot does not exist.
    #[inline]
    pub fn scalar_at<T: Scalar + Default>(&self, offset: usize) -> T {
        match self.data.get(offset..offset + T::SIZE) {
            Some(bytes) if offset + T::SIZE <= self.fixed_len => T::read(bytes),
            _ => T::default(),
        }
    }

    #[inline]
    pub fn bit_at(&self, byte: usize, bit: u8) -> bool {
        byte < self.fixed_len && (self.data[byte] >> bit) & 1 != 0
    }

    /// Resolves an offset slot to a message-relative heap offset.
    /// `None` when the slot is zero or beyond the writer's fixed section.
    #[inline]
    fn offset_at(&self, slot: usize) -> Option<usize> {
        if slot + 4 > self.fixed_len {
            return None;
        }
        match self.scalar_at::<u32>(slot) {
            0 => None,
            offset => Some(offset as usize),
        }
    }

    /// Reads the `[len: u32][payload]` heap entry at `entry`.
    fn entry_at(&self, entry: usize) -> Result<&'a [u8], DecodeError> {
        let len_bytes = self
            .data
            .get(entry..entry + 4)
            .ok_or(DecodeError::Truncated)?;
        let len = u32::from_le_bytes(len_bytes.try_into().unwrap()) as usize;
        self.data
            .get(entry + 4..entry + 4 + len)
            .ok_or(DecodeError::Truncated)
    }

    /// `bytes` or optional-`bytes` field. `None` means absent.
    pub fn bytes_at(&self, slot: usize) -> Result<Option<&'a [u8]>, DecodeError> {
        match self.offset_at(slot) {
            Some(entry) => self.entry_at(entry).map(Some),
            None => Ok(None),
        }
    }

    /// `string` or optional-`string` field. `None` means absent.
    pub fn str_at(&self, slot: usize) -> Result<Option<&'a str>, DecodeError> {
        match self.bytes_at(slot)? {
            Some(bytes) => std::str::from_utf8(bytes)
                .map(Some)
                .map_err(|_| DecodeError::Utf8),
            None => Ok(None),
        }
    }

    /// Message-typed field. `None` means absent.
    pub fn message_at(&self, slot: usize) -> Result<Option<View<'a>>, DecodeError> {
        match self.bytes_at(slot)? {
            Some(bytes) => View::parse(bytes).map(Some),
            None => Ok(None),
        }
    }

    /// `list<scalar>` or `list<enum>` field. Absent reads as empty.
    pub fn scalar_list_at<T: Scalar>(&self, slot: usize) -> Result<ScalarList<'a, T>, DecodeError> {
        let Some(entry) = self.offset_at(slot) else {
            return Ok(ScalarList::empty());
        };
        let count_bytes = self
            .data
            .get(entry..entry + 4)
            .ok_or(DecodeError::Truncated)?;
        let count = u32::from_le_bytes(count_bytes.try_into().unwrap()) as usize;
        let payload_len = count.checked_mul(T::SIZE).ok_or(DecodeError::Truncated)?;
        let payload = self
            .data
            .get(entry + 4..entry + 4 + payload_len)
            .ok_or(DecodeError::Truncated)?;
        Ok(ScalarList {
            payload,
            count,
            _marker: PhantomData,
        })
    }

    /// `list<string|bytes|message>` field. Absent reads as empty.
    pub fn offset_list_at(&self, slot: usize) -> Result<OffsetList<'a>, DecodeError> {
        let Some(entry) = self.offset_at(slot) else {
            return Ok(OffsetList {
                view: *self,
                offsets: &[],
                count: 0,
            });
        };
        let count_bytes = self
            .data
            .get(entry..entry + 4)
            .ok_or(DecodeError::Truncated)?;
        let count = u32::from_le_bytes(count_bytes.try_into().unwrap()) as usize;
        let offsets_len = count.checked_mul(4).ok_or(DecodeError::Truncated)?;
        let offsets = self
            .data
            .get(entry + 4..entry + 4 + offsets_len)
            .ok_or(DecodeError::Truncated)?;
        Ok(OffsetList {
            view: *self,
            offsets,
            count,
        })
    }
}

/// Zero-copy list of fixed-width elements.
#[derive(Clone, Copy)]
pub struct ScalarList<'a, T: Scalar> {
    payload: &'a [u8],
    count: usize,
    _marker: PhantomData<T>,
}

impl<'a, T: Scalar> ScalarList<'a, T> {
    pub fn empty() -> Self {
        ScalarList {
            payload: &[],
            count: 0,
            _marker: PhantomData,
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    #[inline]
    pub fn get(&self, index: usize) -> Option<T> {
        (index < self.count).then(|| T::read(&self.payload[index * T::SIZE..]))
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = T> + 'a {
        let payload = self.payload;
        (0..self.count).map(move |i| T::read(&payload[i * T::SIZE..]))
    }

    pub fn to_vec(&self) -> Vec<T> {
        self.iter().collect()
    }
}

/// List of heap entries (strings, bytes, or messages), resolved lazily.
#[derive(Clone, Copy)]
pub struct OffsetList<'a> {
    view: View<'a>,
    offsets: &'a [u8],
    count: usize,
}

impl<'a> OffsetList<'a> {
    pub fn empty() -> Self {
        OffsetList {
            view: View {
                data: &[0, 0],
                fixed_len: 2,
            },
            offsets: &[],
            count: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Raw payload of element `index`.
    pub fn bytes(&self, index: usize) -> Result<&'a [u8], DecodeError> {
        if index >= self.count {
            return Err(DecodeError::Truncated);
        }
        let offset = u32::from_le_bytes(self.offsets[index * 4..index * 4 + 4].try_into().unwrap());
        if offset == 0 {
            return Err(DecodeError::Offset);
        }
        self.view.entry_at(offset as usize)
    }

    pub fn str(&self, index: usize) -> Result<&'a str, DecodeError> {
        std::str::from_utf8(self.bytes(index)?).map_err(|_| DecodeError::Utf8)
    }

    pub fn message(&self, index: usize) -> Result<View<'a>, DecodeError> {
        View::parse(self.bytes(index)?)
    }
}

/// Conversion from a raw [`View`]; implemented by generated view types so
/// [`MessageList`] can yield them.
pub trait FromView<'a>: Sized {
    fn from_view(view: View<'a>) -> Self;
}

/// Typed `list<string>` reader.
#[derive(Clone, Copy)]
pub struct StrList<'a>(pub OffsetList<'a>);

impl<'a> StrList<'a> {
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn get(&self, index: usize) -> Result<&'a str, DecodeError> {
        self.0.str(index)
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = Result<&'a str, DecodeError>> {
        let list = *self;
        (0..self.len()).map(move |i| list.get(i))
    }

    pub fn to_vec(&self) -> Result<Vec<String>, DecodeError> {
        self.iter().map(|item| item.map(str::to_string)).collect()
    }
}

/// Typed `list<bytes>` reader.
#[derive(Clone, Copy)]
pub struct BytesList<'a>(pub OffsetList<'a>);

impl<'a> BytesList<'a> {
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn get(&self, index: usize) -> Result<&'a [u8], DecodeError> {
        self.0.bytes(index)
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = Result<&'a [u8], DecodeError>> {
        let list = *self;
        (0..self.len()).map(move |i| list.get(i))
    }

    pub fn to_vec(&self) -> Result<Vec<Vec<u8>>, DecodeError> {
        self.iter().map(|item| item.map(<[u8]>::to_vec)).collect()
    }
}

/// Typed `list<message>` reader yielding generated view types.
pub struct MessageList<'a, T> {
    list: OffsetList<'a>,
    _marker: PhantomData<T>,
}

impl<'a, T> Clone for MessageList<'a, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<'a, T> Copy for MessageList<'a, T> {}

impl<'a, T: FromView<'a>> MessageList<'a, T> {
    pub fn new(list: OffsetList<'a>) -> Self {
        MessageList {
            list,
            _marker: PhantomData,
        }
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub fn get(&self, index: usize) -> Result<T, DecodeError> {
        self.list.message(index).map(T::from_view)
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = Result<T, DecodeError>> {
        let list = *self;
        (0..self.len()).map(move |i| list.get(i))
    }
}
