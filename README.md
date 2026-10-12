# nanobuf

A compact, zero-copy serialization format with a fast schema compiler.
Nanobuf reads proto3 (treated as the legacy format, for migration) and emits
packets that decode without parsing: field access on a received buffer is a
bounds-checked offset read, never a deserialization pass.

```
schema demo.telemetry

enum Status: u8 {
  unknown = 0
  ok = 1
}

/// One telemetry frame.
message Packet {
  sequence: u64 @1
  status: Status @2
  temperature: f64? @3
  name: string @4
  samples: list<u16> @5
}
```

```rust
let bytes = Packet { sequence: 7, name: "probe".into(), ..Default::default() }.encode();

// Zero-copy read: no allocation, no parse step.
let view = PacketView::parse(&bytes)?;
assert_eq!(view.sequence(), 7);
assert_eq!(view.name()?, "probe");
```

## Why not protobuf

* **Zero-copy reads.** Scalars live at compile-time-known offsets; strings,
  lists, and sub-messages sit behind bounds-checked offsets. Lists of
  scalars are contiguous memory. A sub-message is a contiguous slice you
  can forward without re-encoding.
* **Tighter packing.** Bools and presence flags are single bits. The
  compiler lays out fields with a hole-filling allocator and tells you
  (`hint[H0001]`) when renumbering fields would shrink the packet further.
* **Layout is the compiler's job.** Every backend generates code against
  the same computed offsets, so all languages read and write identical
  bytes; the conformance suite enforces this byte-for-byte.
* **Compatibility without tags.** Each message carries one `u16` fixed
  section size. Old readers ignore trailing new fields; new readers see
  zero/absent defaults in old data. Adding a field with a fresh id is
  always compatible in both directions.

## Migration from proto3

```sh
nanoc migrate legacy.proto          # writes legacy.nb
nanoc build legacy.nb --lang rust   # generated types speak BOTH formats
```

Generated types carry `encode_proto3`/`decode_proto3` codecs that stay
byte-compatible with the original `.proto` (integer encodings are preserved
via `[proto = ...]` attributes), so services can migrate one peer at a time
and drop the proto3 path afterwards. Proto3 mode exists only for migration.

## Toolchain

```sh
nanoc check schema.nb                            # diagnostics, warnings, layout hints
nanoc build schema.nb --lang rust,python,csharp,cpp,aki -o gen/
nanoc layout schema.nb                           # wire layout table per message
nanoc migrate legacy.proto                       # proto3 -> nanobuf IDL
```

Diagnostics are rustc-style with source snippets, error codes, and
fix-it help, including targeted messages for proto3 syntax habits.

## Repository layout

| crate | what it does |
|---|---|
| `crates/nanobuf-diagnostics` | spans, source maps, terminal rendering |
| `crates/nanobuf-syntax` | `.nb` lexer and parser with error recovery |
| `crates/nanobuf-schema` | resolution, validation, wire layout, hints |
| `crates/nanobuf-proto` | proto3 frontend (`nanoc migrate`) |
| `crates/nanobuf-codegen` | Rust, Python, C#, C++, and aki backends |
| `crates/nanobuf` | Rust runtime: zero-copy views, writer, proto3 codec |
| `crates/nanoc` | the CLI |
| `runtimes/` | Python, C++, C#, and aki runtime libraries |
| `conformance/` | shared schemas, golden fixtures, cross-language tests |
| `examples/` | one schema used from Rust, Python, C++, and C# |

The wire format and IDL are specified in [docs/wire-format.md](docs/wire-format.md)
and [docs/idl.md](docs/idl.md).

## Testing

```sh
cargo test                  # compiler, runtime, Rust conformance
scripts/test-all.sh         # plus Python, C++, and C# conformance suites
```

The conformance suites in every language must produce byte-identical
encodings for the shared sample messages (`conformance/fixtures/`).
