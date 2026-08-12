#!/usr/bin/env bash
#
# Ecosystem test runner for fallow
#
# Tests fallow against real-world open-source JS/TS projects to catch crashes
# and regressions. Exit code 0 or 1 from fallow is expected (issues found),
# but exit code 2+ or signals indicate a crash and fail this script.
#
# Usage:
#   ./tests/ecosystem/run.sh [--fallow-bin PATH]
#
# Environment:
#   FALLOW_BIN       — path to fallow binary (default: cargo-built release binary)
#   ECOSYSTEM_DIR    — directory for cloned repos (default: /tmp/fallow-ecosystem)
#   FALLOW_QUIET     — set to 1 to suppress progress bars (default: 1)

set -euo pipefail

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

FALLOW_BIN="${FALLOW_BIN:-}"
ECOSYSTEM_DIR="${ECOSYSTEM_DIR:-/tmp/fallow-ecosystem}"
export FALLOW_QUIET="${FALLOW_QUIET:-1}"

# Parse arguments
while [[ $# -gt 0 ]]; do
    case "$1" in
        --fallow-bin)
            FALLOW_BIN="$2"
            shift 2
            ;;
        --fallow-bin=*)
            FALLOW_BIN="${1#*=}"
            shift
            ;;
        *)
            echo "Unknown argument: $1" >&2
            exit 1
            ;;
    esac
done

# Build or locate fallow binary
if [[ -z "$FALLOW_BIN" ]]; then
    echo "==> Building fallow (release)..."
    cargo build --release -p fallow-cli --manifest-path "$REPO_ROOT/Cargo.toml"
    FALLOW_BIN="$REPO_ROOT/target/release/fallow"
fi

if [[ ! -x "$FALLOW_BIN" ]]; then
    echo "ERROR: fallow binary not found or not executable: $FALLOW_BIN" >&2
    exit 1
fi

echo "==> Using fallow binary: $FALLOW_BIN"
echo "==> Ecosystem directory: $ECOSYSTEM_DIR"

mkdir -p "$ECOSYSTEM_DIR"

# ---------------------------------------------------------------------------
# Project list
# ---------------------------------------------------------------------------
# Format: "org/repo  branch  subdirectory  install_cmd"
# Use "." for subdirectory to analyze the root.
# Use "-" for install_cmd to skip npm install.

PROJECTS=(
    "vercel/next.js            canary   .  pnpm install --no-frozen-lockfile --ignore-scripts 2>/dev/null || true"
    "vitejs/vite               main     .  pnpm install --no-frozen-lockfile --ignore-scripts"
    "vuejs/core                main     .  pnpm install --no-frozen-lockfile --ignore-scripts"
    "sveltejs/svelte           main     .  pnpm install --no-frozen-lockfile --ignore-scripts"
    "remix-run/remix           main     .  pnpm install --no-frozen-lockfile --ignore-scripts"
    "trpc/trpc                 main     .  pnpm install --no-frozen-lockfile --ignore-scripts"
    "t3-oss/create-t3-app      main     .  pnpm install --no-frozen-lockfile --ignore-scripts"
    "TanStack/query            main     .  pnpm install --no-frozen-lockfile --ignore-scripts"
    "jestjs/jest               main     .  yarn install"
    "storybookjs/storybook     next     .  pnpm install --no-frozen-lockfile --ignore-scripts 2>/dev/null || true"
    "tailwindlabs/tailwindcss  main     .  npm install --ignore-scripts 2>/dev/null || true"
    "prisma/prisma             main     .  pnpm install --no-frozen-lockfile --ignore-scripts 2>/dev/null || true"
)

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

passed=0
issues_found=0
crashed=0
skipped=0
total=${#PROJECTS[@]}
crash_projects=()

clone_project() {
    local repo="$1" branch="$2" dest="$3"

    if [[ -d "$dest/.git" ]]; then
        echo "    Already cloned, pulling latest..."
        git -C "$dest" fetch --depth 1 origin "$branch" 2>/dev/null && \
        git -C "$dest" reset --hard "origin/$branch" 2>/dev/null || true
        return 0
    fi

    # Cloning third-party repositories makes transient network and TLS errors
    # the dominant failure mode here, so one blip should not fail the run.
    local attempt
    for attempt in 1 2 3; do
        if git clone --depth 1 --branch "$branch" --single-branch \
            "https://github.com/$repo.git" "$dest" 2>/dev/null; then
            return 0
        fi
        rm -rf "$dest"
        [[ "$attempt" -eq 3 ]] && return 1
        sleep "$((attempt * 5))"
    done
}

install_deps() {
    local dir="$1" cmd="$2"
    local install_log
    install_log="$(mktemp)"

    if [[ "$cmd" == "-" ]]; then
        return 0
    fi

    echo "    Installing dependencies..."
    if (cd "$dir" && eval "$cmd") >"$install_log" 2>&1; then
        tail -5 "$install_log"
        rm -f "$install_log"
        return 0
    fi

    tail -5 "$install_log"
    rm -f "$install_log"
    return 1
}

run_fallow() {
    local project_dir="$1" name="$2" output_file="$3"
    local exit_code=0
    local stderr_file="${output_file%.json}.stderr.log"

    # Run fallow with a timeout (5 minutes per project)
    if command -v timeout &>/dev/null; then
        timeout 300 "$FALLOW_BIN" dead-code --format json --quiet --root "$project_dir" \
            > "$output_file" 2> "$stderr_file" || exit_code=$?
    elif command -v gtimeout &>/dev/null; then
        gtimeout 300 "$FALLOW_BIN" dead-code --format json --quiet --root "$project_dir" \
            > "$output_file" 2> "$stderr_file" || exit_code=$?
    else
        # No timeout command available, run without timeout
        "$FALLOW_BIN" dead-code --format json --quiet --root "$project_dir" \
            > "$output_file" 2> "$stderr_file" || exit_code=$?
    fi

    # timeout returns 124 on timeout, treat as crash
    if [[ $exit_code -eq 124 ]]; then
        echo "    TIMEOUT (exceeded 5 minutes)"
        return 2
    fi

    return "$exit_code"
}

# ---------------------------------------------------------------------------
# Main loop
# ---------------------------------------------------------------------------

echo ""
echo "==> Testing fallow against ${total} projects"
echo "==========================================="

results_dir="$ECOSYSTEM_DIR/results"
mkdir -p "$results_dir"

for entry in "${PROJECTS[@]}"; do
    # Parse fields (whitespace-delimited, install_cmd may contain spaces)
    repo=$(echo "$entry" | awk '{print $1}')
    branch=$(echo "$entry" | awk '{print $2}')
    subdir=$(echo "$entry" | awk '{print $3}')
    install_cmd=$(echo "$entry" | awk '{for(i=4;i<=NF;i++) printf "%s ", $i; print ""}' | sed 's/ *$//')

    name=$(basename "$repo")
    dest="$ECOSYSTEM_DIR/$name"
    project_dir="$dest"
    if [[ "$subdir" != "." ]]; then
        project_dir="$dest/$subdir"
    fi

    echo ""
    echo "--- [$name] ($repo @ $branch) ---"

    # Clone
    if ! clone_project "$repo" "$branch" "$dest"; then
        echo "    SKIP: clone failed"
        skipped=$((skipped + 1))
        continue
    fi

    # Install
    if ! install_deps "$project_dir" "$install_cmd"; then
        echo "    SKIP: install failed"
        skipped=$((skipped + 1))
        continue
    fi

    # Run fallow
    output_file="$results_dir/${name}.json"
    exit_code=0
    run_fallow "$project_dir" "$name" "$output_file" || exit_code=$?

    if [[ $exit_code -eq 0 ]]; then
        echo "    PASS (no issues)"
        passed=$((passed + 1))
    elif [[ $exit_code -eq 1 ]]; then
        echo "    PASS (issues found — expected)"
        issues_found=$((issues_found + 1))
    else
        echo "    CRASH (exit code $exit_code)"
        crashed=$((crashed + 1))
        crash_projects+=("$name (exit $exit_code)")
    fi
done

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------

echo ""
echo "==========================================="
echo "ECOSYSTEM TEST SUMMARY"
echo "==========================================="
echo "Total projects:    $total"
echo "Passed (clean):    $passed"
echo "Passed (issues):   $issues_found"
echo "Crashed:           $crashed"
echo "Skipped:           $skipped"
echo ""

if [[ $crashed -gt 0 ]]; then
    echo "CRASHED PROJECTS:"
    for p in "${crash_projects[@]}"; do
        echo "  - $p"
    done
    echo ""
    echo "Results saved to: $results_dir/"
    echo ""
    echo "FAIL: $crashed project(s) caused fallow to crash"
    exit 1
fi

if [[ $skipped -eq $total ]]; then
    echo "FAIL: all projects were skipped (network issue?)"
    exit 1
fi

echo "Results saved to: $results_dir/"
echo ""
echo "OK: no crashes detected"
exit 0
