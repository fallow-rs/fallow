#!/usr/bin/env bash
# Self-test for scripts/ci-add-swap.sh. Covers the paths that add no swap, so
# it runs without root: invalid arguments, a missing directory and too little
# free space. The CodSpeed jobs in bench.yml run the path that adds swap.
# Run: bash scripts/test-ci-add-swap.sh
set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SWAP="$SCRIPT_DIR/ci-add-swap.sh"

pass=0
fail=0
check() { # description, expected, actual
  if [ "$2" = "$3" ]; then pass=$((pass + 1)); else fail=$((fail + 1)); echo "FAIL: $1 (expected $2, got $3)"; fi
}

bash "$SWAP" >/dev/null 2>&1
check "no arguments" 2 $?
bash "$SWAP" /tmp/x.swap 0 >/dev/null 2>&1
check "zero size" 2 $?
bash "$SWAP" /tmp/x.swap 8G >/dev/null 2>&1
check "size with unit" 2 $?

out="$(bash "$SWAP" /nonexistent-dir-for-swap-test/x.swap 1 2>&1)"
check "missing directory exit code" 0 $?
check "missing directory warning" 1 "$(printf '%s\n' "$out" | grep -c '^::warning::No extra swap: .* does not exist$')"
check "missing directory stops at the warning" 1 "$(printf '%s\n' "$out" | wc -l | tr -d ' ')"

dir="$(mktemp -d)"
trap 'rm -rf "$dir"' EXIT
out="$(bash "$SWAP" "$dir/x.swap" 999999 2>&1)"
check "low space exit code" 0 $?
check "low space warning" 1 "$(printf '%s\n' "$out" | grep -c '^::warning::No extra swap: .* GiB free, the swap file needs 1000003 GiB$')"
check "low space stops at the warning" 1 "$(printf '%s\n' "$out" | wc -l | tr -d ' ')"
check "low space creates no file" 0 "$([ -e "$dir/x.swap" ] && echo 1 || echo 0)"

echo "ci-add-swap: $pass passed, $fail failed"
[ "$fail" -eq 0 ]
