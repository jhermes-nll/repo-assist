#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

echo "Building release binary..." >&2
cargo build --release

echo "Running ignored performance test..." >&2
output="$(cargo test --release --test performance -- --ignored --nocapture 2>&1)"
echo "$output" >&2

line="$(echo "$output" | grep -E '^PERF ' | tail -n 1)"
cpu="$(echo "$line" | sed -n 's/.*idle_cpu_pct=\([0-9.]*\).*/\1/p')"
rss="$(echo "$line" | sed -n 's/.*rss_mb=\([0-9.]*\).*/\1/p')"
p95="$(echo "$line" | sed -n 's/.*status_p95_ms=\([0-9.]*\).*/\1/p')"

echo
echo "| Metric | Value | Target |"
echo "|---|---|---|"
echo "| Idle daemon CPU | ${cpu}% | < 1% |"
echo "| Idle daemon RSS (5 worktrees) | ${rss} MB | < 50 MB |"
echo "| \`ra status\` p95 (warm, n=20) | ${p95} ms | < 100 ms |"
