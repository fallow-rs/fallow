#!/usr/bin/env bash
# Runs a command and prints the available memory, the used swap, the load
# average and the available disk of the runner at an interval while the command
# runs, and once at the end. A hosted runner that runs out of memory or disk, or
# is too busy to answer, stops without an error from the command, so the last
# lines of the log show the cause.
#
# Usage: bash scripts/ci-with-resource-log.sh <command> [args...]
# RESOURCE_LOG_INTERVAL sets the interval in seconds (default 30). A value that
# is not a positive whole number uses the default.
# The exit code is the exit code of the command.
set -u

if [ "$#" -eq 0 ]; then
  echo "usage: bash scripts/ci-with-resource-log.sh <command> [args...]" >&2
  exit 2
fi

interval="${RESOURCE_LOG_INTERVAL:-30}"
[[ "$interval" =~ ^[1-9][0-9]*$ ]] || interval=30

sample() {
  local line="resources:"
  if [ -r /proc/meminfo ]; then
    line="$line $(awk '
      /^MemAvailable:/ { available = $2 }
      /^SwapTotal:/ { swap_total = $2 }
      /^SwapFree:/ { swap_free = $2 }
      END { printf "mem_available_mb=%d swap_used_mb=%d", available / 1024, (swap_total - swap_free) / 1024 }
    ' /proc/meminfo)"
  fi
  if [ -r /proc/loadavg ]; then
    line="$line load_1m=$(cut -d ' ' -f 1 /proc/loadavg)"
  fi
  line="$line $(df -Pk . | awk 'NR == 2 { printf "disk_available_mb=%d", $4 / 1024 }')"
  echo "$line"
}

# The sampler also stops its sleep, so no process keeps the output open after
# the command ends.
(
  sleeper=""
  trap '[ -n "$sleeper" ] && kill "$sleeper" 2>/dev/null; exit 0' TERM
  while :; do
    sample
    sleep "$interval" &
    sleeper=$!
    wait "$sleeper"
  done
) &
sampler=$!
trap 'kill "$sampler" 2>/dev/null' EXIT

"$@"
status=$?
kill "$sampler" 2>/dev/null
sample
exit "$status"
