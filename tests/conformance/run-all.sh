#!/usr/bin/env bash
set -euo pipefail

# Conformance test runner for multiple real-world projects.
#
# Clones projects, runs fallow + knip on each, and produces an aggregated
# conformance report.
#
# Usage:
#   ./run-all.sh [--fallow-bin PATH] [--clone-dir DIR] [--timeout SECS]
#
# Output:
#   Aggregated JSON report to stdout
#   Human-readable per-project summaries to stderr

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

FALLOW_BIN=""
CLONE_DIR="/tmp/fallow-conformance"
TIMEOUT=300
export FALLOW_QUIET="${FALLOW_QUIET:-1}"

# Parse arguments
while [[ $# -gt 0 ]]; do
    case "$1" in
        --fallow-bin)   FALLOW_BIN="$2";  shift 2 ;;
        --fallow-bin=*) FALLOW_BIN="${1#*=}"; shift ;;
        --clone-dir)    CLONE_DIR="$2";   shift 2 ;;
        --clone-dir=*)  CLONE_DIR="${1#*=}"; shift ;;
        --timeout)      TIMEOUT="$2";     shift 2 ;;
        --timeout=*)    TIMEOUT="${1#*=}"; shift ;;
        *) echo "Unknown argument: $1" >&2; exit 1 ;;
    esac
done

# ---------------------------------------------------------------------------
# Project list — same as benchmarks/download-fixtures.mjs
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
    echo "Error: fallow binary not found. Build with 'cargo build' or pass --fallow-bin PATH" >&2
    exit 1
fi

# Resolve to absolute path
if [[ "${FALLOW_BIN}" != /* ]] && [[ "${FALLOW_BIN}" == */* ]]; then
    FALLOW_BIN="$(cd "$(dirname "${FALLOW_BIN}")" && pwd)/$(basename "${FALLOW_BIN}")"
fi

if ! "${FALLOW_BIN}" --version &>/dev/null; then
    echo "Error: fallow binary at '${FALLOW_BIN}' does not work" >&2
    exit 1
fi

if ! command -v npx &>/dev/null; then
    echo "Error: npx not found. Install Node.js to run knip" >&2
    exit 1
fi

# ---------------------------------------------------------------------------
# Timeout helper
# ---------------------------------------------------------------------------

timeout_cmd() {
    local secs="$1"; shift
    if command -v timeout &>/dev/null; then
        timeout "${secs}" "$@"
    elif command -v gtimeout &>/dev/null; then
        gtimeout "${secs}" "$@"
    else
        "$@"
    fi
}

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

run_single_project() {
    local name="$1" dir="$2" out_dir="$3"
    local fallow_out="${out_dir}/${name}-fallow.json"
    local knip_out="${out_dir}/${name}-knip.json"
    local report_out="${out_dir}/${name}-report.json"

    # Run fallow
    echo "    Running fallow..." >&2
    local fallow_exit=0
    timeout_cmd "${TIMEOUT}" "${FALLOW_BIN}" check --format json --root "${dir}" \
        > "${fallow_out}" 2>/dev/null || fallow_exit=$?

    if [[ ${fallow_exit} -eq 124 ]]; then
        echo "    fallow TIMEOUT" >&2
        return 1
    fi
    if [[ ${fallow_exit} -ge 2 ]]; then
        echo "    fallow ERROR (exit ${fallow_exit})" >&2
        return 1
    fi
    echo "    fallow done (exit ${fallow_exit})" >&2

    # Validate fallow output
    if ! python3 -c "import json; json.load(open('${fallow_out}'))" 2>/dev/null; then
        echo "    fallow output is not valid JSON" >&2
        return 1
    fi

    # Run knip
    echo "    Running knip..." >&2
    local knip_exit=0
    (cd "${dir}" && timeout_cmd "${TIMEOUT}" npx --yes "knip@${KNIP_VERSION:-6.32.0}" --reporter json \
        > "${knip_out}" 2>/dev/null) || knip_exit=$?

    if [[ ${knip_exit} -eq 124 ]]; then
        echo "    knip TIMEOUT" >&2
        return 1
    fi
    if [[ ${knip_exit} -ge 2 ]]; then
        echo "    knip ERROR (exit ${knip_exit})" >&2
        return 1
    fi
    echo "    knip done (exit ${knip_exit})" >&2

    # Validate knip output
    if ! python3 -c "import json; json.load(open('${knip_out}'))" 2>/dev/null; then
        echo "    knip output is not valid JSON" >&2
        return 1
    fi

    # Compare
    if ! python3 "${SCRIPT_DIR}/compare.py" "${fallow_out}" "${knip_out}" "${dir}" > "${report_out}"; then
        echo "    compare.py failed" >&2
        return 1
    fi
    return 0
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

echo "=== Conformance Test Suite ===" >&2
echo "Fallow:     ${FALLOW_BIN}" >&2
echo "Projects:   ${#PROJECTS[@]}" >&2
echo "Timeout:    ${TIMEOUT}s per tool" >&2
echo "" >&2

mkdir -p "${CLONE_DIR}"

TMPDIR_CONFORM="$(mktemp -d)"
trap 'rm -rf "${TMPDIR_CONFORM}"' EXIT

succeeded=0
failed=0
skipped_names=()

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
        skipped_names+=("${name}")
        failed=$((failed + 1))
        continue
    fi

    # Install deps
    install_deps "${dest}" "${pm}"

    # Run conformance
    if run_single_project "${name}" "${dest}" "${TMPDIR_CONFORM}"; then
        succeeded=$((succeeded + 1))

        # Print per-project summary to stderr
        python3 -c "
import json, sys
with open('${TMPDIR_CONFORM}/${name}-report.json') as f:
    r = json.load(f)
s = r['summary']
print(f\"    Agreement: {s['agreement_pct']}% ({s['agreed']}/{s['agreed']+s['fallow_only']+s['knip_only']})\", file=sys.stderr)
" 2>/dev/null || true
    else
        echo "    SKIP: tool error" >&2
        skipped_names+=("${name}")
        failed=$((failed + 1))
    fi

    echo "" >&2
done

# ---------------------------------------------------------------------------
# Aggregate
# ---------------------------------------------------------------------------

echo "=== Summary ===" >&2
echo "Succeeded: ${succeeded}/${#PROJECTS[@]}" >&2
if [[ ${#skipped_names[@]} -gt 0 ]]; then
    echo "Skipped:   ${skipped_names[*]}" >&2
fi
echo "" >&2

if [[ ${succeeded} -eq 0 ]]; then
    echo "Error: no projects completed successfully" >&2
    exit 1
fi

# Aggregate per-project reports
python3 "${SCRIPT_DIR}/aggregate.py" "${TMPDIR_CONFORM}"
