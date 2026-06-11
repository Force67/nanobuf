# Nanobuf IDL

Nanobuf schemas live in `.nb` files. Statements are newline-terminated.
`//` starts a line comment; `///` doc comments attach to the following
declaration and are carried into generated code.

```
schema demo.telemetry

import "common.nb"

/// Health of a downstream link.
enum Status: u8 {
  ok = 0
  degraded = 1
  failed = 2
}

/// One telemetry sample frame.
message Packet {
  sequence: u32 @1
  status: Status @2
  temperature: f64? @3
  name: string @4
  samples: list<u16> @5
  header: common.Header @6

  reserved @7, @8
}
```

## Declarations

* `schema <dotted name>`: required, first statement. Names the package.
* `import "<path>"`: makes another file's types available, referenced as
  `<their package>.<Type>`.
* `enum Name: <backing> { variant = value ... }`: backing is one of
  `u8 u16 u32 i8 i16 i32` (default `u8`). Enums are open on the wire;
  a `0`-valued variant is recommended (warning if missing).
* `message Name { fields... }`

## Fields

`name: type @id`: ids start at 1, are unique per message, and are the
unit of wire compatibility. `reserved @n, @m` retires ids permanently.

Types: `bool`, `u8 u16 u32 u64`, `i8 i16 i32 i64`, `f16 f32 f64`, `string`,
`bytes`, `list<T>`, enum and message names. `T?` marks a scalar, enum,
`string`, or `bytes` field optional (absent is distinguishable from the
default). Message-typed fields are always optional; writing `?` on them is
an error. Lists cannot be optional or nested; wrap the element in a message.

A field may carry a proto migration attribute: `count: i32 @3 [proto = int32]`
pins the proto3 wire encoding for migration mode (`int32`, `sint32`,
`fixed32`, `int64`, `sint64`, `fixed64`). Only schemas migrated from
`.proto` files normally need this.

## Compatibility

Adding a field with a fresh id is always compatible in both directions.
Never change a field's type or reuse an id; add a new field and reserve the
old id instead. `nanoc` enforces id uniqueness and reservation within a
schema.
