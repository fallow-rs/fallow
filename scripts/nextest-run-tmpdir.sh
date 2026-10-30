#!/bin/sh
# Nextest setup script: give each test run its own TMPDIR.
#
# Each `fallow audit` in the test suite creates a base snapshot cache in the
# temp directory, and each later audit probes every cache entry it finds
# there. The test fixtures are deleted after each test, so their cache
# entries stay for the 30-day age limit. In a shared temp directory these
# entries grow with each local run, and one audit then spends seconds on the
# probe. A private directory per run keeps the probe at the entries of this
# run only.
#
# The script also removes the directories of runs that started more than one
# day ago, so the directories do not grow without limit.
set -eu

base="${TMPDIR:-/tmp}"
base="${base%/}"
# `-name` comes first: the name test needs no stat call, and a shared temp
# directory can hold tens of thousands of entries.
find "$base" -maxdepth 1 -name 'fallow-nextest.*' -type d -mmin +1440 \
  -exec rm -rf {} + 2>/dev/null || true
dir="$(mktemp -d "$base/fallow-nextest.XXXXXX")"
echo "TMPDIR=$dir" >> "$NEXTEST_ENV"
