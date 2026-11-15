#!/usr/bin/env bash
# Self-test for scripts/ci-with-resource-log.sh. Asserts that the wrapper keeps
# the exit code and the output of the command, prints resource lines, and stops
# its sampler with the command.
# Run: bash scripts/test-ci-with-resource-log.sh
set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WRAP="$SCRIPT_DIR/ci-with-resource-log.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
# A wrapper run that takes longer than this has a sampler that did not stop.
LIMIT_SECONDS=15

pass=0
fail=0
check() { # description, expected, actual
  if [ "$2" = "$3" ]; then pass=$((pass + 1)); else fail=$((fail + 1)); echo "FAIL: $1 (expected $2, got $3)"; fi
}

# Runs the wrapper with an interval and a command. A reader in the background
# copies the output through a pipe, like a CI step does. Sets `out` to the
# output and `status` to the exit code, or to "hung" when the output stays open
# past the limit (a sampler that keeps running holds the pipe open).
run_wrapped() { # interval, command...
  local interval="$1"
  shift
  rm -f "$TMP/out" "$TMP/status"
  ( (RESOURCE_LOG_INTERVAL="$interval" bash "$WRAP" "$@"; echo $? >"$TMP/status") | cat >"$TMP/out" ) &
  local reader=$!
  local waited=0
  while kill -0 "$reader" 2>/dev/null && [ "$waited" -lt "$LIMIT_SECONDS" ]; do
    sleep 1
    waited=$((waited + 1))
  done
  if kill -0 "$reader" 2>/dev/null; then
    kill "$reader" 2>/dev/null
    status=hung
  else
    status="$(cat "$TMP/status" 2>/dev/null || echo missing)"
  fi
  out="$(cat "$TMP/out" 2>/dev/null)"
}

resource_lines() { printf '%s\n' "$out" | grep -c '^resources:.*disk_available_mb=[0-9]'; }

# 1. A passing command: exit code 0, its output stays, resource lines appear.
run_wrapped 1 sh -c 'echo built; sleep 2'
check "pass exit code" 0 "$status"
check "command output kept" 1 "$(printf '%s\n' "$out" | grep -c '^built$')"
check "interval and final resource lines" 1 "$([ "$(resource_lines)" -ge 2 ] && echo 1 || echo 0)"
if [ -r /proc/meminfo ]; then
  check "memory and swap on Linux" 1 "$(printf '%s\n' "$out" | grep -c -m1 'mem_available_mb=[0-9]* swap_used_mb=[0-9]*')"
  check "load average on Linux" 1 "$(printf '%s\n' "$out" | grep -c -m1 'load_1m=[0-9.]* disk_available_mb=')"
fi

# 2. A failing command: the wrapper returns its exit code.
run_wrapped 1 sh -c 'exit 7'
check "failure exit code" 7 "$status"

# 3. The sampler stops with the command and keeps no pipe open. With a 30 s
# interval, the output has the first line and the final line.
run_wrapped 30 sleep 2
check "sampler stops with the command" 0 "$status"
check "first and final resource lines" 2 "$(resource_lines)"

# 4. An invalid interval uses the default, so the sampler does not loop fast.
run_wrapped abc sleep 2
check "invalid interval uses the default" 2 "$(resource_lines)"
run_wrapped 0 sleep 2
check "zero interval uses the default" 2 "$(resource_lines)"

# 5. No command: usage error.
bash "$WRAP" >/dev/null 2>&1
check "usage error" 2 $?

echo "ci-with-resource-log: $pass passed, $fail failed"
[ "$fail" -eq 0 ]
