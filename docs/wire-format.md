# Nanobuf wire format, version 1

All multi-byte values are little-endian. There is no buffer-level framing: a
serialized root message is the packet. Transports that need length framing
provide their own.

## Message encoding

A message is a contiguous byte range: a *fixed section* followed by a *heap*.

```
[fixed_len: u16][fixed fields ...][heap entries ...]
```

`fixed_len` is the byte length of the fixed section *including* the two
header bytes. `fixed_len < 2` is invalid. The fixed section holds scalar
values, bit fields, and `u32` offset slots at compile-time-assigned offsets.
The heap holds variable-size data referenced by those slots.

Offsets in slots are byte offsets **from the start of the message** (the
position of `fixed_len`). Slot value `0` means the field is absent.

## Fixed section contents

| field type            | storage                                  |
|-----------------------|------------------------------------------|
| `bool`                | 1 bit                                    |
| `u8 i8`               | 1 byte                                   |
| `u16 i16`             | 2 bytes, 2-aligned                       |
| `u32 i32 f32`         | 4 bytes, 4-aligned                       |
| `u64 i64 f64`         | 8 bytes, 8-aligned                       |
| enum                  | backing type width                       |
| `string bytes list T` | `u32` offset slot, 4-aligned             |
| message               | `u32` offset slot, 4-aligned             |

Optional scalars, enums, and bools additionally carry a presence bit.
Optional heap fields do not: slot `0` already encodes absence. A
non-optional `string`/`bytes`/`list` with slot `0` reads as empty.

Bits (bools and presence flags) are packed into *bit pool* bytes allocated
inside the fixed section like ordinary 1-byte fields; bit 0 is the least
significant bit.

Alignment is relative to the message start. Writers place nested messages at
8-aligned heap positions so message-relative alignment equals buffer
alignment when the root starts 8-aligned.

## Heap entries

| entry                       | encoding                                       |
|-----------------------------|------------------------------------------------|
| `string` / `bytes`          | `[len: u32][payload]`, strings are UTF-8       |
| `list<scalar>` `list<enum>` | `[count: u32][count * element]`, elements naturally aligned |
| message                     | `[len: u32][message bytes]`, 8-aligned payload |
| `list<string|bytes|message>`| `[count: u32][count * u32 offset]`, offsets message-relative, `0` = absent element is invalid; writers emit empty entries |

## Layout assignment

The compiler assigns offsets once per schema; generated code in every
language uses identical offsets. The algorithm is normative:

1. State: byte cursor starting at 2, a list of free holes (padding),
   and a bit pool (current byte address + bits used).
2. Process fields in **ascending field id**. For each field, allocate the
   presence bit first (if any), then the data slot.
3. `alloc(size, align)`: scan holes in address order and take the first that
   fits at an aligned address, splitting remainders back into holes;
   otherwise pad the cursor up to `align` (recording the padding as a hole)
   and bump it by `size`.
4. `alloc_bit()`: use the next free bit in the pool; if the pool is full or
   empty, `alloc(1, 1)` a new pool byte.
5. `fixed_len` is the final cursor value. It must not exceed 65535.

Because allocation is keyed on field id and holes are only ever consumed,
adding a field with a fresh id never moves existing fields.

## Compatibility rules

* Field ids are never reused or retyped. Removing a field requires a
  `reserved` declaration for its id.
* New reader, old data: any read past `fixed_len` yields the default
  (zero / false / empty / absent).
* Old reader, new data: trailing fixed bytes and unreferenced heap entries
  are ignored. Re-serializing through an old schema drops unknown fields;
  forwarding the original bytes preserves them.
* Enums are open: readers preserve unknown numeric values.

## Limits

* Fixed section: ≤ 65535 bytes.
* Message size: < 4 GiB (`u32` offsets).
* Decoders must bounds-check every heap dereference against the message
  byte range and reject `fixed_len` larger than the available bytes.

## Proto3 migration mode

For migration, generated code can also read and write the proto3 wire
format using the field ids as proto field numbers. Scalar mapping on the
proto wire: unsigned integers and `bool` use varint, signed integers use
zigzag varint by default (`[proto = int32]`-style attributes override this
for schemas imported from `.proto` files), floats use fixed32/fixed64,
strings/bytes/messages are length-delimited, and scalar lists are packed.
This mode exists only to talk to legacy proto3 peers during migration.
