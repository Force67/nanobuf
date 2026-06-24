#!/usr/bin/env bash
# Generates Python bindings for the conformance schema and runs the
# pytest suite against the golden fixtures.
#
# PYTHON overrides the interpreter (default python3; Windows runners expose it
# as `python`).
set -euo pipefail

cd "$(dirname "$0")/../.."

PYTHON="${PYTHON:-python3}"

gen_dir="$(mktemp -d)"
trap 'rm -rf "$gen_dir"' EXIT

cargo run -q -p nanoc -- build conformance/schema/telemetry.nb --lang python --out "$gen_dir"

PYTHONPATH="$PWD/runtimes/python:$gen_dir" "$PYTHON" -m pytest runtimes/python/tests -q
