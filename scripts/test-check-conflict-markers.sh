#!/usr/bin/env bash
# Self-test for scripts/check-conflict-markers.sh.
# Run: bash scripts/test-check-conflict-markers.sh
set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CHECK="$SCRIPT_DIR/check-conflict-markers.sh"
FIX="$(mktemp -d)"
trap 'rm -rf "$FIX"' EXIT

fail=0
check() { # description, expected_exit, actual_exit
  if [ "$2" != "$3" ]; then fail=1; echo "FAIL: $1 (expected exit $2, got $3)"; fi
}

cd "$FIX"
git init -q
git config user.email t@example.com; git config user.name t; git config commit.gpgsign false
start=$(printf '<%.0s' 1 2 3 4 5 6 7)
end=$(printf '>%.0s' 1 2 3 4 5 6 7)

printf 'Title\n=======\ntext with <<<<<<< inside\n' > clean.md
git add -A && git commit -qm init

sh "$CHECK" >/dev/null 2>&1
check "committed clean tree" 0 $?
sh "$CHECK" --staged >/dev/null 2>&1
check "staged nothing" 0 $?

printf '%s HEAD\nours\n=======\ntheirs\n%s branch\n' "$start" "$end" > conflict.txt
git add conflict.txt
sh "$CHECK" --staged >/dev/null 2>&1
check "staged conflict" 1 $?
out=$(sh "$CHECK" --staged 2>&1)
case "$out" in *"conflict.txt: $start HEAD"*) ;; *) fail=1; echo "FAIL: staged hit names the file: $out" ;; esac

git commit -qm conflict
sh "$CHECK" >/dev/null 2>&1
check "committed conflict" 1 $?

printf 'only %s mid-line\n' "$start" > midline.txt
git rm -q conflict.txt && git add midline.txt
sh "$CHECK" --staged >/dev/null 2>&1
check "staged mid-line marker" 0 $?

if [ "$fail" -ne 0 ]; then exit 1; fi
echo "check-conflict-markers self-test passed"
