#!/usr/bin/env bash
set -euo pipefail

# Conformance fixture verifier: runs fallow dead-code on each fixture and
# compares the JSON output against the expected.json file.
#
# Usage:
#   ./verify-fixtures.sh [--fallow-bin PATH]
#
# Each fixture directory must contain:
#   - package.json (and source files)
#   - expected.json with the subset of fields to verify
#
# The comparison checks:
#   - total_issues count
#   - Exact match on (path, export_name) tuples for each issue type
#   - Circular dependency chain files and lengths
#   - Duplicate export names and locations
#
# Only fixtures with expected.json are tested (the original "basic" fixture
# is a fallow-vs-knip comparison, not a self-contained expectation test).

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
FIXTURES_DIR="${SCRIPT_DIR}/fixtures"
FALLOW_BIN=""

# Parse arguments
while [[ $# -gt 0 ]]; do
    case "$1" in
        --fallow-bin)   FALLOW_BIN="$2";  shift 2 ;;
        --fallow-bin=*) FALLOW_BIN="${1#*=}"; shift ;;
        *) echo "Unknown argument: $1" >&2; exit 1 ;;
    esac
done

# Find fallow binary
if [[ -z "${FALLOW_BIN}" ]]; then
    if command -v fallow &>/dev/null; then
        FALLOW_BIN="fallow"
    else
        for candidate in \
            "${CARGO_TARGET_DIR:-${REPO_ROOT}/target}/release/fallow" \
            "${CARGO_TARGET_DIR:-${REPO_ROOT}/target}/debug/fallow"; do
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

echo "=== Conformance Fixture Verification ===" >&2
echo "Fallow: ${FALLOW_BIN}" >&2
echo "" >&2

passed=0
failed=0
skipped=0
failures=()

for fixture_dir in "${FIXTURES_DIR}"/*/; do
    fixture_name="$(basename "${fixture_dir}")"
    expected_file="${fixture_dir}/expected.json"

    # Skip fixtures without expected.json
    if [[ ! -f "${expected_file}" ]]; then
        skipped=$((skipped + 1))
        continue
    fi

    echo -n "  ${fixture_name} ... " >&2

    # Run fallow
    actual_file="$(mktemp)"
    fallow_exit=0
    cd "${fixture_dir}"
    "${FALLOW_BIN}" check --format json > "${actual_file}" 2>/dev/null || fallow_exit=$?

    # Exit 1 = issues found (expected), exit 2 = error
    if [[ ${fallow_exit} -ge 2 ]]; then
        echo "ERROR (fallow exit ${fallow_exit})" >&2
        failed=$((failed + 1))
        failures+=("${fixture_name}: fallow error (exit ${fallow_exit})")
        rm -f "${actual_file}"
        continue
    fi

    # Compare using Python
    result="$(python3 "${SCRIPT_DIR}/verify-expected.py" "${actual_file}" "${expected_file}" 2>&1)" || true

    if echo "${result}" | grep -q "^PASS$"; then
        echo "PASS" >&2
        passed=$((passed + 1))
    else
        echo "FAIL" >&2
        echo "${result}" | sed 's/^/    /' >&2
        failed=$((failed + 1))
        failures+=("${fixture_name}")
    fi

    rm -f "${actual_file}"
done

echo "" >&2
echo "=== Results ===" >&2
echo "  Passed:  ${passed}" >&2
echo "  Failed:  ${failed}" >&2
echo "  Skipped: ${skipped}" >&2

if [[ ${#failures[@]} -gt 0 ]]; then
    echo "" >&2
    echo "  Failed fixtures:" >&2
    for f in "${failures[@]}"; do
        echo "    - ${f}" >&2
    done
fi

if [[ ${failed} -gt 0 ]]; then
    exit 1
fi
