#!/usr/bin/env bash
# Generates the conformance header, builds the C++ conformance test under
# ASan/UBSan, and runs it against the golden fixtures.
#
# Overridable so CI can sweep compilers and language standards:
#   CXX         compiler        (default g++)
#   CXX_STD     -std value      (default c++17)
#   NANOBUF_SAN 1/0 sanitizers  (default 1; set 0 where asan/ubsan is absent)
set -euo pipefail

cd "$(dirname "$0")/../.."

build_dir="${CARGO_TARGET_DIR:-target}/cpp-conformance"
mkdir -p "$build_dir"

cargo run -q -p nanoc -- build conformance/schema/telemetry.nb --lang cpp --out "$build_dir"

CXX="${CXX:-g++}"
CXX_STD="${CXX_STD:-c++17}"
san="-fsanitize=address,undefined"
[ "${NANOBUF_SAN:-1}" = "0" ] && san=""

# shellcheck disable=SC2086  # $san is a single flag token or empty, no splitting
"$CXX" -std="$CXX_STD" -Wall -Wextra -Werror $san -O1 \
  -I runtimes/cpp -I "$build_dir" \
  runtimes/cpp/conformance_test.cc -o "$build_dir/conformance_test"

"$build_dir/conformance_test" conformance/fixtures
