#!/usr/bin/env bash
# Generates the conformance header, builds the C++ conformance test under
# ASan/UBSan, and runs it against the golden fixtures.
set -euo pipefail

cd "$(dirname "$0")/../.."

build_dir="${CARGO_TARGET_DIR:-target}/cpp-conformance"
mkdir -p "$build_dir"

cargo run -q -p nanoc -- build conformance/schema/telemetry.nb --lang cpp --out "$build_dir"

g++ -std=c++17 -Wall -Wextra -Werror -fsanitize=address,undefined -O1 \
  -I runtimes/cpp -I "$build_dir" \
  runtimes/cpp/conformance_test.cc -o "$build_dir/conformance_test"

"$build_dir/conformance_test" conformance/fixtures
