# Examples

One schema, [`sensor.nb`](sensor.nb), used from every supported language.
Each program encodes a reading, reads it back through a zero-copy view,
and decodes it into an owned value.

Generate the bindings once (the Rust example generates its own at build
time):

```sh
cargo run -p nanoc -- build examples/sensor.nb --lang python,cpp,csharp --out examples/gen
```

Then, from the repository root:

```sh
# Rust
cargo run -p nanobuf-example

# Python
PYTHONPATH=runtimes/python:examples/gen python3 examples/python/main.py

# C++
g++ -std=c++17 -I runtimes/cpp -I examples/gen examples/cpp/main.cc -o examples/gen/sensor
examples/gen/sensor

# C#
dotnet run --project examples/csharp
```

Every program prints the same thing:

```
encoded 62 bytes
probe 17 at greenhouse-3: 21.5 C (good)
battery 75%
samples: [2150, 2152, 2149]
round trip ok
```
