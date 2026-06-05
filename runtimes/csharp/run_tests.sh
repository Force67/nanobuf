#!/usr/bin/env bash
# Generates C# code for the conformance schema and runs the conformance test.
set -euo pipefail

cd "$(dirname "$0")/../.."

# `gen/` is covered by the repository .gitignore.
cargo run -q -p nanoc -- build conformance/schema/telemetry.nb \
    --lang csharp --out runtimes/csharp/ConformanceTest/gen

dotnet run --project runtimes/csharp/ConformanceTest -c Release
