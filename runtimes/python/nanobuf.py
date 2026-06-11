"""Runtime support for nanobuf-generated Python modules.

Mirrors the Rust runtime (`crates/nanobuf/src`): a zero-copy `View` over one
serialized message, lazy list readers, a single-buffer `Writer`, and the
proto3 wire codec used in migration mode. Wire layout is decided by the
schema compiler; this module only reads and writes the offsets it is given.
"""

import math
import struct
from typing import Callable, Iterator, Optional, Sequence, Union

BytesLike = Union[bytes, bytearray, memoryview]

_U64_MASK = 0xFFFFFFFFFFFFFFFF


def half_round(value: float) -> float:
    """Rounds to the nearest binary16 value, ties to even.

    `f16` fields hold whatever float the caller assigned; the wire carries
    binary16, so rounding here is what every typed runtime does at
    assignment time. `struct` raises where IEEE rounds overflow to
    infinity, hence the fallback.
    """
    try:
        return struct.unpack("<e", struct.pack("<e", value))[0]
    except OverflowError:
        return math.inf if value > 0 else -math.inf


class DecodeError(Exception):
    """Failure while reading a nanobuf or proto3 buffer."""


class View:
    """Zero-copy view over one serialized message.

    Scalar accessors are infallible: anything past the writer's fixed
    section reads as zero, which is how old data yields defaults for new
    fields. Heap accessors bounds-check the referenced entry and raise
    `DecodeError` on corrupt input.
    """

    __slots__ = ("data", "fixed_len")

    def __init__(self, data: memoryview, fixed_len: int):
        self.data = data
        self.fixed_len = fixed_len

    @classmethod
    def parse(cls, data: BytesLike) -> "View":
        data = memoryview(data)
        if len(data) < 2:
            raise DecodeError("invalid message header")
        fixed_len = data[0] | data[1] << 8
        if fixed_len < 2:
            raise DecodeError("invalid message header")
        if fixed_len > len(data):
            raise DecodeError("buffer is truncated")
        return cls(data, fixed_len)

    def scalar_at(self, offset: int, fmt: str):
        """Reads a fixed-section scalar, or zero when the writer's schema
        was older and the slot does not exist."""
        if offset + struct.calcsize(fmt) > self.fixed_len:
            return 0.0 if fmt in ("f", "d") else 0
        return struct.unpack_from("<" + fmt, self.data, offset)[0]

    def bit_at(self, byte: int, bit: int) -> bool:
        return byte < self.fixed_len and self.data[byte] >> bit & 1 != 0

    def _offset_at(self, slot: int) -> Optional[int]:
        """Resolves an offset slot to a message-relative heap offset.
        None when the slot is zero or beyond the writer's fixed section."""
        if slot + 4 > self.fixed_len:
            return None
        offset = struct.unpack_from("<I", self.data, slot)[0]
        return offset if offset else None

    def _entry_at(self, entry: int) -> memoryview:
        """Reads the `[len: u32][payload]` heap entry at `entry`."""
        if entry + 4 > len(self.data):
            raise DecodeError("buffer is truncated")
        length = struct.unpack_from("<I", self.data, entry)[0]
        if entry + 4 + length > len(self.data):
            raise DecodeError("buffer is truncated")
        return self.data[entry + 4 : entry + 4 + length]

    def bytes_at(self, slot: int) -> Optional[bytes]:
        """`bytes` or optional-`bytes` field. None means absent."""
        entry = self._offset_at(slot)
        return None if entry is None else bytes(self._entry_at(entry))

    def str_at(self, slot: int) -> Optional[str]:
        """`string` or optional-`string` field. None means absent."""
        payload = self.bytes_at(slot)
        if payload is None:
            return None
        try:
            return payload.decode("utf-8")
        except UnicodeDecodeError:
            raise DecodeError("string field is not valid UTF-8") from None

    def message_at(self, slot: int) -> Optional["View"]:
        """Message-typed field. None means absent."""
        entry = self._offset_at(slot)
        return None if entry is None else View.parse(self._entry_at(entry))

    def scalar_list_at(
        self, slot: int, fmt: str, convert: Optional[Callable] = None
    ) -> "ScalarList":
        """`list<scalar>` or `list<enum>` field. Absent reads as empty."""
        entry = self._offset_at(slot)
        if entry is None:
            return ScalarList(memoryview(b""), 0, fmt, convert)
        if entry + 4 > len(self.data):
            raise DecodeError("buffer is truncated")
        count = struct.unpack_from("<I", self.data, entry)[0]
        end = entry + 4 + count * struct.calcsize(fmt)
        if end > len(self.data):
            raise DecodeError("buffer is truncated")
        return ScalarList(self.data[entry + 4 : end], count, fmt, convert)

    def offset_list_at(self, slot: int) -> "OffsetList":
        """`list<string|bytes|message>` field. Absent reads as empty."""
        entry = self._offset_at(slot)
        if entry is None:
            return OffsetList(self, memoryview(b""), 0)
        if entry + 4 > len(self.data):
            raise DecodeError("buffer is truncated")
        count = struct.unpack_from("<I", self.data, entry)[0]
        end = entry + 4 + count * 4
        if end > len(self.data):
            raise DecodeError("buffer is truncated")
        return OffsetList(self, self.data[entry + 4 : end], count)


class ScalarList:
    """Zero-copy list of fixed-width elements, decoded lazily."""

    __slots__ = ("_payload", "_count", "_fmt", "_convert")

    def __init__(
        self,
        payload: memoryview,
        count: int,
        fmt: str,
        convert: Optional[Callable] = None,
    ):
        self._payload = payload
        self._count = count
        self._fmt = fmt
        self._convert = convert

    def __len__(self) -> int:
        return self._count

    def __getitem__(self, index: int):
        if index < 0:
            index += self._count
        if not 0 <= index < self._count:
            raise IndexError(index)
        at = index * struct.calcsize(self._fmt)
        value = struct.unpack_from("<" + self._fmt, self._payload, at)[0]
        return self._convert(value) if self._convert else value

    def __iter__(self) -> Iterator:
        return (self[i] for i in range(self._count))

    def __eq__(self, other) -> bool:
        if not isinstance(other, (ScalarList, list, tuple)):
            return NotImplemented
        return len(self) == len(other) and all(a == b for a, b in zip(self, other))

    def __repr__(self) -> str:
        return repr(self.to_list())

    def to_list(self) -> list:
        return list(self)


class OffsetList:
    """List of heap entries (strings, bytes, or messages), resolved lazily."""

    __slots__ = ("_view", "_offsets", "_count")

    def __init__(self, view: View, offsets: memoryview, count: int):
        self._view = view
        self._offsets = offsets
        self._count = count

    def __len__(self) -> int:
        return self._count

    def _entry(self, index: int) -> memoryview:
        if index < 0:
            index += self._count
        if not 0 <= index < self._count:
            raise IndexError(index)
        offset = struct.unpack_from("<I", self._offsets, index * 4)[0]
        if offset == 0:
            raise DecodeError("malformed heap offset")
        return self._view._entry_at(offset)

    def bytes(self, index: int) -> bytes:
        """Raw payload of element `index`."""
        return bytes(self._entry(index))

    def str(self, index: int) -> str:
        try:
            return bytes(self._entry(index)).decode("utf-8")
        except UnicodeDecodeError:
            raise DecodeError("string field is not valid UTF-8") from None

    def message(self, index: int) -> View:
        return View.parse(self._entry(index))


class StrList:
    """Typed `list<string>` reader."""

    __slots__ = ("_list",)

    def __init__(self, entries: OffsetList):
        self._list = entries

    def __len__(self) -> int:
        return len(self._list)

    def __getitem__(self, index: int) -> str:
        return self._list.str(index)

    def __iter__(self) -> Iterator[str]:
        return (self[i] for i in range(len(self)))

    def __eq__(self, other) -> bool:
        if not isinstance(other, (StrList, list, tuple)):
            return NotImplemented
        return len(self) == len(other) and all(a == b for a, b in zip(self, other))

    def __repr__(self) -> str:
        return repr(self.to_list())

    def to_list(self) -> list:
        return list(self)


class BytesList:
    """Typed `list<bytes>` reader."""

    __slots__ = ("_list",)

    def __init__(self, entries: OffsetList):
        self._list = entries

    def __len__(self) -> int:
        return len(self._list)

    def __getitem__(self, index: int) -> bytes:
        return self._list.bytes(index)

    def __iter__(self) -> Iterator[bytes]:
        return (self[i] for i in range(len(self)))

    def __eq__(self, other) -> bool:
        if not isinstance(other, (BytesList, list, tuple)):
            return NotImplemented
        return len(self) == len(other) and all(a == b for a, b in zip(self, other))

    def __repr__(self) -> str:
        return repr(self.to_list())

    def to_list(self) -> list:
        return list(self)


class MessageList:
    """Typed `list<message>` reader yielding generated view types."""

    __slots__ = ("_list", "_view_type")

    def __init__(self, entries: OffsetList, view_type: Callable[[View], object]):
        self._list = entries
        self._view_type = view_type

    def __len__(self) -> int:
        return len(self._list)

    def __getitem__(self, index: int):
        return self._view_type(self._list.message(index))

    def __iter__(self) -> Iterator:
        return (self[i] for i in range(len(self)))


class Writer:
    """Single-buffer message writer.

    Generated `_write` implementations call `begin` to reserve their fixed
    section, patch field slots, and append heap entries. Nested messages
    are written depth-first into the same buffer.
    """

    __slots__ = ("_buf", "_base")

    def __init__(self):
        self._buf = bytearray()
        self._base = 0

    def finish(self) -> bytes:
        return bytes(self._buf)

    def begin(self, fixed_len: int) -> None:
        """Starts a message at the current position: writes the header and
        zero-fills the rest of the fixed section."""
        self._base = len(self._buf)
        self._buf += struct.pack("<H", fixed_len)
        self._buf += bytes(fixed_len - 2)

    def put_scalar(self, offset: int, fmt: str, value) -> None:
        """Writes a scalar into the fixed section."""
        if fmt == "e":
            value = half_round(value)
        struct.pack_into("<" + fmt, self._buf, self._base + offset, value)

    def put_bit(self, byte: int, bit: int, value: bool) -> None:
        if value:
            self._buf[self._base + byte] |= 1 << bit

    def heap_bytes(self, payload: BytesLike) -> int:
        """Appends a `[len][payload]` heap entry and returns its
        message-relative offset."""
        entry = len(self._buf)
        self._buf += struct.pack("<I", len(payload))
        self._buf += payload
        return entry - self._base

    def heap_message(self, write_body: Callable[["Writer"], None]) -> int:
        """Appends a nested message entry; `write_body` serializes the
        child. Returns the entry's message-relative offset."""
        # Message payloads are 8-aligned relative to the enclosing message.
        self._pad_payload_to(8)
        entry = len(self._buf)
        offset = entry - self._base
        self._buf += bytes(4)
        outer_base = self._base
        body_start = len(self._buf)
        write_body(self)
        struct.pack_into("<I", self._buf, entry, len(self._buf) - body_start)
        self._base = outer_base
        return offset

    def set_slot(self, slot: int, heap_offset: int) -> None:
        """Patches an offset slot in the fixed section."""
        struct.pack_into("<I", self._buf, self._base + slot, heap_offset)

    def put_bytes(self, slot: int, payload: BytesLike, force: bool) -> None:
        """`string`/`bytes` field: writes the entry unless the value is
        empty and `force` is false (absent and empty read identically)."""
        if not payload and not force:
            return
        self.set_slot(slot, self.heap_bytes(payload))

    def put_message(self, slot: int, write_body: Callable[["Writer"], None]) -> None:
        """Message-typed field."""
        self.set_slot(slot, self.heap_message(write_body))

    def put_scalar_list(self, slot: int, fmt: str, items: Sequence) -> None:
        """`list<scalar>` / `list<enum>` field. Empty lists are absent."""
        if not items:
            return
        self._pad_payload_to(struct.calcsize(fmt))
        entry = len(self._buf)
        self._buf += struct.pack("<I", len(items))
        for item in items:
            if fmt == "e":
                item = half_round(item)
            self._buf += struct.pack("<" + fmt, item)
        self.set_slot(slot, entry - self._base)

    def put_offset_list(
        self,
        slot: int,
        items: Sequence,
        write_item: Callable[["Writer", object], int],
    ) -> None:
        """`list<string|bytes|message>` field: writes the offset table,
        then one entry per item via `write_item` (which returns each
        entry's message-relative offset, e.g. from `heap_bytes`)."""
        if not items:
            return
        entry = len(self._buf)
        self._buf += struct.pack("<I", len(items))
        table = len(self._buf)
        self._buf += bytes(4 * len(items))
        for i, item in enumerate(items):
            struct.pack_into("<I", self._buf, table + i * 4, write_item(self, item))
        self.set_slot(slot, entry - self._base)

    def _pad_payload_to(self, align: int) -> None:
        """Pads so the next entry's payload (after its 4-byte length/count
        prefix) lands `align`-aligned relative to the message start."""
        payload_pos = len(self._buf) - self._base + 4
        self._buf += bytes(-payload_pos % align)


def read_varint(buf: BytesLike, pos: int) -> "tuple[int, int]":
    """Reads one varint at `pos`; returns `(value, new_pos)`."""
    value = 0
    for i in range(10):
        if pos >= len(buf):
            raise DecodeError("malformed varint")
        byte = buf[pos]
        pos += 1
        # The tenth byte may only carry the final bit of a u64.
        if i == 9 and byte > 1:
            raise DecodeError("malformed varint")
        value |= (byte & 0x7F) << i * 7
        if byte & 0x80 == 0:
            return value, pos
    raise DecodeError("malformed varint")


def write_varint(out: bytearray, value: int) -> None:
    """Appends `value` as a varint; negative values are sign-extended to
    64 bits first, matching proto3 int32/int64 encoding."""
    value &= _U64_MASK
    while value > 0x7F:
        out.append(value & 0x7F | 0x80)
        value >>= 7
    out.append(value)


def zigzag_encode(value: int) -> int:
    return (value << 1 ^ value >> 63) & _U64_MASK


def zigzag_decode(value: int) -> int:
    return value >> 1 ^ -(value & 1)


def to_signed(value: int, bits: int) -> int:
    """Reinterprets the low `bits` of `value` as two's complement."""
    sign = 1 << bits - 1
    return (value & sign - 1) - (value & sign)


class FieldReader:
    """Iterates `(field_number, wire_type, value)` triples of one proto3
    message. Varints yield ints; fixed and length-delimited values yield
    memoryviews. Unknown fields are skipped by ignoring the triple."""

    __slots__ = ("_buf", "_pos")

    def __init__(self, buf: BytesLike):
        self._buf = memoryview(buf)
        self._pos = 0

    def __iter__(self) -> "FieldReader":
        return self

    def __next__(self) -> "tuple[int, int, object]":
        if self._pos >= len(self._buf):
            raise StopIteration
        tag, self._pos = read_varint(self._buf, self._pos)
        field = tag >> 3
        wire_type = tag & 7
        if field == 0:
            raise DecodeError("unsupported wire type")
        if field > 0xFFFFFFFF:
            raise DecodeError("malformed varint")
        if wire_type == 0:
            value, self._pos = read_varint(self._buf, self._pos)
        elif wire_type == 1:
            value = self._take(8)
        elif wire_type == 2:
            length, self._pos = read_varint(self._buf, self._pos)
            value = self._take(length)
        elif wire_type == 5:
            value = self._take(4)
        else:
            raise DecodeError("unsupported wire type")
        return field, wire_type, value

    def _take(self, length: int) -> memoryview:
        end = self._pos + length
        if end > len(self._buf):
            raise DecodeError("buffer is truncated")
        value = self._buf[self._pos : end]
        self._pos = end
        return value


def expect_varint(wire_type: int, value) -> int:
    if wire_type != 0:
        raise DecodeError("unsupported wire type")
    return value


def expect_len(wire_type: int, value) -> memoryview:
    if wire_type != 2:
        raise DecodeError("unsupported wire type")
    return value


def expect_string(wire_type: int, value) -> str:
    try:
        return bytes(expect_len(wire_type, value)).decode("utf-8")
    except UnicodeDecodeError:
        raise DecodeError("string field is not valid UTF-8") from None


def expect_fixed(wire_type: int, value, fmt: str):
    """Expects a fixed32/fixed64 value and unpacks it as `fmt`."""
    if wire_type != (5 if struct.calcsize(fmt) == 4 else 1):
        raise DecodeError("unsupported wire type")
    return struct.unpack("<" + fmt, value)[0]


def read_packed_varints(wire_type: int, value) -> "list[int]":
    """Reads a packed varint list payload, or a single unpacked element."""
    if wire_type == 0:
        return [value]
    if wire_type != 2:
        raise DecodeError("unsupported wire type")
    items = []
    pos = 0
    while pos < len(value):
        item, pos = read_varint(value, pos)
        items.append(item)
    return items


def read_packed_fixed(wire_type: int, value, fmt: str) -> list:
    """Reads a packed fixed32/fixed64 list payload, or a single unpacked
    element, decoding each as `fmt`."""
    size = struct.calcsize(fmt)
    if wire_type == (5 if size == 4 else 1):
        return [struct.unpack("<" + fmt, value)[0]]
    if wire_type != 2:
        raise DecodeError("unsupported wire type")
    if len(value) % size != 0:
        raise DecodeError("buffer is truncated")
    return list(struct.unpack("<{}{}".format(len(value) // size, fmt), value))


def write_tag(out: bytearray, field: int, wire_type: int) -> None:
    write_varint(out, field << 3 | wire_type)


def write_varint_field(out: bytearray, field: int, value: int) -> None:
    write_tag(out, field, 0)
    write_varint(out, value)


def write_fixed(out: bytearray, fmt: str, value) -> None:
    out += struct.pack("<" + fmt, value)


def write_fixed_field(out: bytearray, field: int, fmt: str, value) -> None:
    write_tag(out, field, 5 if struct.calcsize(fmt) == 4 else 1)
    write_fixed(out, fmt, value)


def write_len_field(out: bytearray, field: int, payload: BytesLike) -> None:
    write_tag(out, field, 2)
    write_varint(out, len(payload))
    out += payload
