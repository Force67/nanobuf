#!/usr/bin/env bash
# Runs every test suite: Rust workspace plus each language runtime's
# conformance tests. Skips runtimes whose toolchain is not installed.
set -euo pipefail
cd "$(dirname "$0")/.."

echo "==> cargo test"
cargo test --quiet

echo "==> cargo clippy"
cargo clippy --quiet --all-targets

echo "==> cargo fmt --check"
cargo fmt --check

for runtime in python cpp csharp aki; do
    script="runtimes/$runtime/run_tests.sh"
    if [ "$runtime" = aki ] && ! command -v "${AKIKO:-akiko}" >/dev/null 2>&1; then
        echo "==> skipping aki (no akiko toolchain; set AKIKO)"
        continue
    fi
    if [ -x "$script" ] || [ -f "$script" ]; then
        echo "==> $script"
        bash "$script"
    else
        echo "==> skipping $runtime (no run_tests.sh)"
    fi
done

echo "all suites passed"
