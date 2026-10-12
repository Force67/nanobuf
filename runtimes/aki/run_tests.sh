#!/usr/bin/env bash
# Generates the aki conformance modules from the shared schema into the test
# package, then runs the aki conformance test against the golden fixtures.
#
# AKIKO is the aki toolchain driver (default: akiko on PATH).
set -euo pipefail

cd "$(dirname "$0")/../.."

AKIKO="${AKIKO:-akiko}"
gen_dir="runtimes/aki/tests/gen"

# Regenerate from scratch so a removed message cannot leave a stale module.
rm -rf "$gen_dir"
cargo run -q -p nanoc -- build conformance/schema/telemetry.nb --lang aki --out "$gen_dir"

# Arguments after `--` reach the test program, which reads the fixtures from
# the directory given here.
"$AKIKO" test runtimes/aki -- "$PWD/conformance/fixtures"
