#!/usr/bin/env bash
set -euo pipefail

# Real-world performance benchmark for CI.
#
# Clones 8 open-source projects, runs fallow on each (cold + warm cache),
# and outputs timing results as benchmark-action compatible JSON.
#
# Usage:
#   ./bench-ci.sh [--fallow-bin PATH] [--clone-dir DIR] [--runs N]
#
# Output:
#   benchmark-action JSON to stdout
#   Human-readable summary to stderr

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

FALLOW_BIN=""
CLONE_DIR="/tmp/fallow-bench-ci"
RUNS=3
export FALLOW_QUIET="${FALLOW_QUIET:-1}"
PROJECT_TIMEOUT_SECONDS="${PROJECT_TIMEOUT_SECONDS:-120}"
QUERY_MAX_COLD_MS="${QUERY_MAX_COLD_MS:-5000}"
TIMEOUT_BIN=""
TIMEOUT_ARGS=()

if command -v timeout >/dev/null 2>&1; then
    TIMEOUT_BIN="timeout"
elif command -v gtimeout >/dev/null 2>&1; then
    TIMEOUT_BIN="gtimeout"
fi
if [[ -n "${TIMEOUT_BIN}" ]] && "${TIMEOUT_BIN}" --help 2>&1 | rg -q -- "--foreground"; then
    TIMEOUT_ARGS=(--foreground)
fi

# Parse arguments
while [[ $# -gt 0 ]]; do
    case "$1" in
        --fallow-bin)   FALLOW_BIN="$2";  shift 2 ;;
        --fallow-bin=*) FALLOW_BIN="${1#*=}"; shift ;;
        --clone-dir)    CLONE_DIR="$2";   shift 2 ;;
        --clone-dir=*)  CLONE_DIR="${1#*=}"; shift ;;
        --runs)         RUNS="$2";        shift 2 ;;
        --runs=*)       RUNS="${1#*=}";   shift ;;
        *) echo "Unknown argument: $1" >&2; exit 1 ;;
    esac
done

# ---------------------------------------------------------------------------
# Project list, same as download-fixtures.mjs and conformance/run-all.sh
# ---------------------------------------------------------------------------

PROJECTS=(
    "preact     preactjs/preact      10.25.4         npm"
    "fastify    fastify/fastify      v5.2.1          npm"
    "zod        colinhacks/zod       v3.24.2         npm"
    "vue-core   vuejs/core           v3.5.30         pnpm"
    "svelte     sveltejs/svelte      svelte@5.54.1   pnpm"
    "query      TanStack/query       v5.90.3         pnpm"
    "vite       vitejs/vite          v8.0.1          pnpm"
    "next.js    vercel/next.js       v16.2.1         pnpm"
)

# ---------------------------------------------------------------------------
# Find fallow binary
# ---------------------------------------------------------------------------

if [[ -z "${FALLOW_BIN}" ]]; then
    if command -v fallow &>/dev/null; then
        FALLOW_BIN="fallow"
    else
        for candidate in \
            "${REPO_ROOT}/target/release/fallow" \
            "${REPO_ROOT}/target/debug/fallow"; do
            if [[ -x "${candidate}" ]]; then
                FALLOW_BIN="${candidate}"
                break
            fi
        done
    fi
fi

if [[ -z "${FALLOW_BIN}" ]]; then
    echo "Error: fallow binary not found. Build with 'cargo build --release' or pass --fallow-bin PATH" >&2
    exit 1
fi

if [[ "${FALLOW_BIN}" != /* ]] && [[ "${FALLOW_BIN}" == */* ]]; then
    FALLOW_BIN="$(cd "$(dirname "${FALLOW_BIN}")" && pwd)/$(basename "${FALLOW_BIN}")"
fi

if ! "${FALLOW_BIN}" --version &>/dev/null; then
    echo "Error: fallow binary at '${FALLOW_BIN}' does not work" >&2
    exit 1
fi

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

clone_project() {
    local repo="$1" tag="$2" dest="$3"
    if [[ -d "${dest}/.git" ]]; then
        echo "    Already cloned" >&2
        return 0
    fi
    git clone --depth 1 --branch "${tag}" --single-branch \
        "https://github.com/${repo}.git" "${dest}" 2>/dev/null
}

install_deps() {
    local dir="$1" pm="$2"
    if [[ -d "${dir}/node_modules" ]]; then
        return 0
    fi
    echo "    Installing dependencies (${pm})..." >&2
    if [[ "${pm}" == "pnpm" ]]; then
        (cd "${dir}" && pnpm install --no-frozen-lockfile --ignore-scripts >/dev/null 2>/dev/null) || true
    else
        (cd "${dir}" && npm install --ignore-scripts --no-audit --no-fund >/dev/null 2>/dev/null) || true
    fi
}

clear_cache() {
    local dir="$1"
    rm -rf "${dir}/.fallow"
}

# Returns elapsed time in milliseconds
# Sets: ELAPSED_MS
run_fallow() {
    local dir="$1"; shift

    if [[ -n "${TIMEOUT_BIN}" ]]; then
        "${TIMEOUT_BIN}" "${TIMEOUT_ARGS[@]}" "${PROJECT_TIMEOUT_SECONDS}" \
            "${FALLOW_BIN}" --quiet --format json "$@" --root "${dir}" \
            >/dev/null 2>/dev/null
    else
        "${FALLOW_BIN}" --quiet --format json "$@" --root "${dir}" \
            >/dev/null 2>/dev/null
    fi
}

time_fallow() {
    local dir="$1"; shift
    local start end run_status
    start=$(date +%s%N 2>/dev/null || python3 -c "import time; print(int(time.time()*1e9))")

    if run_fallow "${dir}" "$@"; then
        run_status=0
    else
        run_status=$?
    fi

    end=$(date +%s%N 2>/dev/null || python3 -c "import time; print(int(time.time()*1e9))")

    ELAPSED_MS=$(( (end - start) / 1000000 ))
    return "${run_status}"
}

median() {
    python3 -c "
import sys
vals = sorted(int(x) for x in sys.argv[1:])
mid = len(vals) // 2
print(vals[mid] if len(vals) % 2 == 1 else (vals[mid-1] + vals[mid]) // 2)
" "$@"
}

fmt_ms() {
    local ms="$1"
    if [[ ${ms} -lt 1000 ]]; then
        echo "${ms}ms"
    else
        python3 -c "print(f'{${ms}/1000:.2f}s')"
    fi
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

echo "=== Real-World Performance Benchmark ===" >&2
echo "Fallow:     ${FALLOW_BIN}" >&2
echo "Projects:   ${#PROJECTS[@]}" >&2
echo "Runs:       ${RUNS}" >&2
echo "" >&2

mkdir -p "${CLONE_DIR}"

# Collect benchmark entries as JSONL (one JSON object per line)
BENCH_JSONL=$(mktemp)
trap 'rm -rf "${BENCH_JSONL}"' EXIT

for entry in "${PROJECTS[@]}"; do
    name=$(echo "${entry}" | awk '{print $1}')
    repo=$(echo "${entry}" | awk '{print $2}')
    tag=$(echo "${entry}"  | awk '{print $3}')
    pm=$(echo "${entry}"   | awk '{print $4}')

    dest="${CLONE_DIR}/${name}"

    echo "--- [${name}] (${repo} @ ${tag}) ---" >&2

    # Clone
    if ! clone_project "${repo}" "${tag}" "${dest}"; then
        echo "    SKIP: clone failed" >&2
        continue
    fi

    # Install deps
    install_deps "${dest}" "${pm}"

    # --- Cold runs (no cache) ---
    cold_times=()
    for (( i=0; i<RUNS; i++ )); do
        clear_cache "${dest}"
        if ! time_fallow "${dest}" --no-cache; then
            echo "    FAIL: fallow timed out or errored during cold run after ${PROJECT_TIMEOUT_SECONDS}s" >&2
            exit 1
        fi
        cold_times+=("${ELAPSED_MS}")
    done
    cold_median=$(median "${cold_times[@]}")

    # --- Warm runs (with cache) ---
    clear_cache "${dest}"
    # Populate cache
    if ! run_fallow "${dest}"; then
        echo "    FAIL: fallow timed out or errored while warming cache after ${PROJECT_TIMEOUT_SECONDS}s" >&2
        exit 1
    fi
    # Measure
    warm_times=()
    for (( i=0; i<RUNS; i++ )); do
        if ! time_fallow "${dest}"; then
            echo "    FAIL: fallow timed out or errored during warm run after ${PROJECT_TIMEOUT_SECONDS}s" >&2
            exit 1
        fi
        warm_times+=("${ELAPSED_MS}")
    done
    warm_median=$(median "${warm_times[@]}")
    clear_cache "${dest}"

    echo "    Cold: $(fmt_ms "${cold_median}") (median of ${RUNS})" >&2
    echo "    Warm: $(fmt_ms "${warm_median}") (median of ${RUNS})" >&2
    echo "    Runs: cold=[${cold_times[*]}] warm=[${warm_times[*]}]" >&2
    if [[ "${name}" == "query" && "${cold_median}" -gt "${QUERY_MAX_COLD_MS}" ]]; then
        echo "    FAIL: query cold median ${cold_median}ms exceeds ${QUERY_MAX_COLD_MS}ms budget" >&2
        exit 1
    fi
    echo "" >&2

    # Append entries as JSONL
    python3 -c "
import json
for entry in [
    {'name': '${name} (cold)', 'unit': 'ms', 'value': ${cold_median}},
    {'name': '${name} (warm)', 'unit': 'ms', 'value': ${warm_median}},
]:
    print(json.dumps(entry))
" >> "${BENCH_JSONL}"
done

# Combine JSONL into benchmark-action JSON array
python3 -c "
import json, sys
data = [json.loads(line) for line in open(sys.argv[1]) if line.strip()]
print(json.dumps(data, indent=2))
" "${BENCH_JSONL}"
