#!/bin/sh
# Run the shipped musl release binaries inside an Alpine container.
#
# The release build checks the musl binaries only statically. This script
# executes them on a musl system with no glibc, so a binary that cannot start
# or cannot complete an analysis fails the release before publication.
#
# Usage (inside the container):
#   sh musl-smoke.sh <bin-dir> <platform> <fixture-dir> <out-dir>
#
#   <bin-dir>      Holds fallow-<platform>, fallow-lsp-<platform>,
#                  fallow-mcp-<platform> and fallow-similar-code-<platform>,
#                  as the build job uploads them in
#                  unsigned-binaries-<platform> and as the GitHub release
#                  ships them. It can be read-only.
#   <platform>     The npm platform name, for example linux-x64-musl.
#   <fixture-dir>  A small project to analyze. It can be read-only.
#   <out-dir>      A writable directory. The script writes each JSON report
#                  here, so the caller can parse it with a full JSON parser.
#
# Environment:
#   FALLOW_SMOKE_VERSION  When set, each --version line must show this version.
#   FALLOW_SMOKE_SKIP_MULTICALL=1
#                  Do not require fallow-multicall-<platform>. The build job
#                  uploads it (npm ships it as `fallow`), but the GitHub
#                  release does not, so a run against release assets sets this.
#
# The script uses only POSIX sh and BusyBox tools, because Alpine has no bash.

set -eu

if [ "$#" -ne 4 ]; then
  echo "usage: musl-smoke.sh <bin-dir> <platform> <fixture-dir> <out-dir>" >&2
  exit 2
fi

bin_dir=$1
platform=$2
fixture_dir=$3
out_dir=$4
expected_version=${FALLOW_SMOKE_VERSION:-}
skip_multicall=${FALLOW_SMOKE_SKIP_MULTICALL:-0}

fail() {
  echo "musl-smoke: $*" >&2
  exit 1
}

# Downloaded artifacts lose the executable bit and the mount is read-only, so
# copy each binary to a private directory and make it executable there.
work=$(mktemp -d)
mkdir "$work/bin"

stage() {
  source_path="$bin_dir/$1-$platform"
  [ -f "$source_path" ] || fail "missing binary $source_path"
  [ -s "$source_path" ] || fail "empty binary $source_path"
  cp "$source_path" "$work/bin/$1"
  chmod +x "$work/bin/$1"
}

check_version() {
  label=$1
  expected_name=$2
  shift 2
  if ! output=$("$@" --version 2>&1); then
    printf '%s\n' "$output" >&2
    fail "$label --version exited with an error"
  fi
  echo "$label --version: $output"
  case "$output" in
    "$expected_name "*) ;;
    *) fail "$label --version must start with '$expected_name ', got: $output" ;;
  esac
  if [ -n "$expected_version" ] && [ "$output" != "$expected_name $expected_version" ]; then
    fail "$label --version must be '$expected_name $expected_version', got: $output"
  fi
}

# Run one dead-code analysis of a writable copy of the fixture. Exit code 0
# means no findings and 1 means findings; any other code is a failure.
check_analysis() {
  label=$1
  report=$2
  shift 2
  project="$work/project-$report"
  cp -R "$fixture_dir" "$project"
  set +e
  (cd "$project" && "$@" dead-code --format json --quiet >"$out_dir/$report.json" 2>"$work/$report.stderr")
  status=$?
  set -e
  if [ "$status" -ne 0 ] && [ "$status" -ne 1 ]; then
    cat "$work/$report.stderr" >&2
    fail "$label dead-code exited with $status, expected 0 or 1"
  fi
  [ -s "$out_dir/$report.json" ] || fail "$label dead-code wrote no JSON"
  first=$(tr -d ' \t\r\n' <"$out_dir/$report.json" | cut -c1)
  [ "$first" = "{" ] || fail "$label dead-code output is not a JSON object"
  grep -q '"src/orphan.ts"' "$out_dir/$report.json" ||
    fail "$label dead-code did not report the unused fixture file src/orphan.ts"
  echo "$label dead-code: exit $status, $(wc -c <"$out_dir/$report.json" | tr -d ' ') bytes of JSON"
}

# Run the similar-code provider status command against an empty cache. The
# command needs no model and no network. It must start the provider and write
# the status contract with model_ready false.
check_similar_code_status() {
  cache="$work/similar-code-cache"
  mkdir "$cache"
  report="$out_dir/similar-code-status.json"
  if ! FALLOW_SIMILAR_CODE_CACHE_DIR="$cache" "$work/bin/fallow-similar-code" status --json \
    >"$report" 2>"$work/similar-code-status.stderr"; then
    cat "$work/similar-code-status.stderr" "$report" >&2
    fail "fallow-similar-code status exited with an error"
  fi
  first=$(tr -d ' \t\r\n' <"$report" | cut -c1)
  [ "$first" = "{" ] || fail "fallow-similar-code status output is not a JSON object"
  grep -q '"model_ready":false' "$report" ||
    fail "fallow-similar-code status must report model_ready false for an empty cache"
  echo "fallow-similar-code status: $(wc -c <"$report" | tr -d ' ') bytes of JSON, model_ready false"
}

echo "musl-smoke: $platform on $(uname -m), $(head -n1 /etc/alpine-release 2>/dev/null | sed 's/^/Alpine /')"
[ -d "$out_dir" ] || fail "missing output directory $out_dir"

stage fallow
stage fallow-lsp
stage fallow-mcp
stage fallow-similar-code

check_version "fallow" "fallow" "$work/bin/fallow"
check_analysis "fallow" "dead-code" "$work/bin/fallow"
check_version "fallow-mcp" "fallow-mcp" "$work/bin/fallow-mcp"
check_version "fallow-lsp" "fallow-lsp" "$work/bin/fallow-lsp"
check_version "fallow-similar-code" "fallow-similar-code" "$work/bin/fallow-similar-code"
check_similar_code_status

# npm ships the multicall binary as `fallow`. The npm `fallow-lsp` and
# `fallow-mcp` launchers start it with `lsp-server` and `mcp-server`.
if [ "$skip_multicall" = "1" ]; then
  echo "musl-smoke: fallow-multicall skipped (FALLOW_SMOKE_SKIP_MULTICALL=1)"
else
  stage fallow-multicall
  check_version "fallow-multicall" "fallow" "$work/bin/fallow-multicall"
  check_analysis "fallow-multicall" "dead-code-multicall" "$work/bin/fallow-multicall"
  check_version "fallow-multicall mcp-server" "fallow-mcp" "$work/bin/fallow-multicall" mcp-server
  check_version "fallow-multicall lsp-server" "fallow-lsp" "$work/bin/fallow-multicall" lsp-server
fi

echo "musl-smoke: all checks passed for $platform"
