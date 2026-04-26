#!/usr/bin/env bash
set -euo pipefail

if [ -f "$HOME/.cargo/env" ]; then
  # rustup installs cargo outside PATH until the shell env is reloaded.
  # Source it here so the shared check script works in fresh shells too.
  # shellcheck disable=SC1090
  . "$HOME/.cargo/env"
fi

ROOT="$(git rev-parse --show-toplevel)"
NAPI_DIR="$ROOT/crates/napi"
INITIAL_STATUS_FILE="$(mktemp)"
FINAL_STATUS_FILE="$(mktemp)"

cleanup() {
  rm -f "$INITIAL_STATUS_FILE" "$FINAL_STATUS_FILE"
}

trap cleanup EXIT

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "error: required command '$1' is not installed or not on PATH" >&2
    exit 1
  fi
}

run_step() {
  echo "→ $1"
  shift
  "$@"
}

cd "$ROOT"

git status --porcelain=v1 > "$INITIAL_STATUS_FILE"

require_command cargo
require_command npm

run_step "cargo fmt" cargo fmt --all -- --check
run_step "cargo clippy" cargo clippy --workspace --all-targets -- -D warnings
run_step \
  "cargo clippy (test-sidecar-key feature)" \
  cargo clippy -p fallow-cli --features test-sidecar-key --all-targets -- -D warnings
run_step "cargo test" cargo test --workspace --all-targets
run_step \
  "cargo test (runtime coverage integration)" \
  cargo test -p fallow-cli --features test-sidecar-key --test runtime_coverage_tests
run_step "npm ci (crates/napi)" npm --prefix "$NAPI_DIR" ci --omit=optional
run_step "npm run build:debug (crates/napi)" npm --prefix "$NAPI_DIR" run build:debug
run_step "npm test (crates/napi)" npm --prefix "$NAPI_DIR" test

git status --porcelain=v1 > "$FINAL_STATUS_FILE"

if ! cmp -s "$INITIAL_STATUS_FILE" "$FINAL_STATUS_FILE"; then
  echo "error: check steps modified the working tree" >&2
  git status --short
  git diff --stat
  exit 1
fi

echo "=== All check steps passed ==="
