#!/usr/bin/env bash
# Generates Python bindings for the conformance schema and runs the
# pytest suite against the golden fixtures.
set -euo pipefail

cd "$(dirname "$0")/../.."

gen_dir="$(mktemp -d)"
trap 'rm -rf "$gen_dir"' EXIT

cargo run -q -p nanoc -- build conformance/schema/telemetry.nb --lang python --out "$gen_dir"

PYTHONPATH="$PWD/runtimes/python:$gen_dir" python3 -m pytest runtimes/python/tests -q
