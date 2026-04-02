#!/usr/bin/env bash
# Performance benchmark suite for OxiCode.
# Usage: ./scripts/benchmark.sh [--quick]

set -euo pipefail

QUICK="${1:-}"

echo "=== OxiCode Performance Benchmarks ==="
echo ""

# 1. Binary startup time (cold start)
echo "--- Startup Time (--version, cold start) ---"
cargo build --release --quiet 2>/dev/null
BINARY="target/release/oxicode"
if [ -f "$BINARY" ]; then
    for i in 1 2 3; do
        # Flush disk cache on macOS (needs sudo) or just measure warm
        time "$BINARY" --version 2>/dev/null
    done
else
    echo "Release binary not found. Run: cargo build --release"
fi
echo ""

# 2. Binary size
echo "--- Binary Size ---"
if [ -f "$BINARY" ]; then
    ls -lh "$BINARY" | awk '{print $5, $9}'
    if command -v strip &>/dev/null; then
        STRIPPED=$(mktemp)
        cp "$BINARY" "$STRIPPED"
        strip "$STRIPPED"
        echo "Stripped: $(ls -lh "$STRIPPED" | awk '{print $5}')"
        rm -f "$STRIPPED"
    fi
else
    echo "Release binary not found."
fi
echo ""

# 3. Criterion benchmarks
echo "--- Criterion Benchmarks ---"
if [ "$QUICK" = "--quick" ]; then
    echo "Skipping criterion (--quick mode)"
else
    cargo bench --package oxicode-cli 2>&1 | grep -E "time:|bench"
fi
echo ""

echo "=== Done ==="
