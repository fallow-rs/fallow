#!/usr/bin/env bash
set -euo pipefail

# Compare audit runtime between two fallow binaries on the same project.
#
# The script accepts exit code 0 or 1 from `fallow audit`, because both are
# valid audit outcomes. Exit code 2 or higher is treated as a failed run.
#
# Usage:
#   benchmarks/compare-audit-runtime.sh \
#     --old-fallow-bin /tmp/fallow-old \
#     --new-fallow-bin ./target/release/fallow \
#     --root /path/to/large/project \
#     --root /path/to/another/large/project \
#     --base origin/main \
#     --runs 5 \
#     --min-source-files 1000 \
#     --max-regression-pct 5 \
#     --output /tmp/fallow-audit-runtime.json

OLD_FALLOW_BIN=""
NEW_FALLOW_BIN=""
ROOTS=()
BASE_REF="HEAD"
RUNS=3
MIN_SOURCE_FILES=0
MAX_REGRESSION_PCT=""
OUTPUT_PATH=""
ALLOW_EMPTY_AUDIT=false
EXTRA_ARGS=()

checksum_bin() {
    local bin="$1"
    if command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "${bin}" | awk '{print $1}'
    else
        sha256sum "${bin}" | awk '{print $1}'
    fi
}

count_source_files() {
    local root="$1"
    python3 - "${root}" <<'PY'
import os
import sys

SOURCE_EXTENSIONS = {
    ".astro",
    ".cjs",
    ".css",
    ".cts",
    ".js",
    ".jsx",
    ".mjs",
    ".mts",
    ".svelte",
    ".ts",
    ".tsx",
    ".vue",
}
SKIP_DIRS = {
    ".cache",
    ".fallow",
    ".git",
    ".next",
    ".nuxt",
    ".parcel-cache",
    ".svelte-kit",
    ".turbo",
    ".vite",
    "coverage",
    "dist",
    "node_modules",
    "out",
    "storybook-static",
}

count = 0
for current_root, dirs, files in os.walk(sys.argv[1]):
    dirs[:] = [name for name in dirs if name not in SKIP_DIRS]
    for name in files:
        _, ext = os.path.splitext(name)
        if ext.lower() in SOURCE_EXTENSIONS:
            count += 1
print(count)
PY
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --old-fallow-bin) OLD_FALLOW_BIN="$2"; shift 2 ;;
        --old-fallow-bin=*) OLD_FALLOW_BIN="${1#*=}"; shift ;;
        --new-fallow-bin) NEW_FALLOW_BIN="$2"; shift 2 ;;
        --new-fallow-bin=*) NEW_FALLOW_BIN="${1#*=}"; shift ;;
        --root) ROOTS+=("$2"); shift 2 ;;
        --root=*) ROOTS+=("${1#*=}"); shift ;;
        --base) BASE_REF="$2"; shift 2 ;;
        --base=*) BASE_REF="${1#*=}"; shift ;;
        --runs) RUNS="$2"; shift 2 ;;
        --runs=*) RUNS="${1#*=}"; shift ;;
        --min-source-files) MIN_SOURCE_FILES="$2"; shift 2 ;;
        --min-source-files=*) MIN_SOURCE_FILES="${1#*=}"; shift ;;
        --max-regression-pct) MAX_REGRESSION_PCT="$2"; shift 2 ;;
        --max-regression-pct=*) MAX_REGRESSION_PCT="${1#*=}"; shift ;;
        --output) OUTPUT_PATH="$2"; shift 2 ;;
        --output=*) OUTPUT_PATH="${1#*=}"; shift ;;
        --allow-empty-audit) ALLOW_EMPTY_AUDIT=true; shift ;;
        --) shift; EXTRA_ARGS+=("$@"); break ;;
        *)
            echo "Unknown argument: $1" >&2
            exit 2
            ;;
    esac
done

if [[ -z "${OLD_FALLOW_BIN}" || -z "${NEW_FALLOW_BIN}" || "${#ROOTS[@]}" -eq 0 ]]; then
    echo "Usage: $0 --old-fallow-bin PATH --new-fallow-bin PATH --root PATH [--root PATH...] [--base REF] [--runs N] [--max-regression-pct N] [--allow-empty-audit] [-- extra audit args]" >&2
    exit 2
fi

if ! [[ "${RUNS}" =~ ^[1-9][0-9]*$ ]]; then
    echo "--runs must be a positive integer" >&2
    exit 2
fi

if ! [[ "${MIN_SOURCE_FILES}" =~ ^[0-9]+$ ]]; then
    echo "--min-source-files must be a non-negative integer" >&2
    exit 2
fi

if [[ -n "${MAX_REGRESSION_PCT}" ]] && ! [[ "${MAX_REGRESSION_PCT}" =~ ^[0-9]+([.][0-9]+)?$ ]]; then
    echo "--max-regression-pct must be a non-negative number" >&2
    exit 2
fi

for bin in "${OLD_FALLOW_BIN}" "${NEW_FALLOW_BIN}"; do
    if [[ ! -x "${bin}" ]]; then
        echo "fallow binary is not executable: ${bin}" >&2
        exit 2
    fi
    "${bin}" --version >/dev/null
done

OLD_FALLOW_SHA="$(checksum_bin "${OLD_FALLOW_BIN}")"
NEW_FALLOW_SHA="$(checksum_bin "${NEW_FALLOW_BIN}")"
if [[ "${OLD_FALLOW_SHA}" == "${NEW_FALLOW_SHA}" ]]; then
    echo "old and new fallow binaries have the same SHA-256: ${OLD_FALLOW_SHA}" >&2
    echo "Refusing to compare identical binaries." >&2
    exit 2
fi

SOURCE_FILE_COUNTS=()
for root in "${ROOTS[@]}"; do
    if [[ ! -d "${root}" ]]; then
        echo "project root does not exist: ${root}" >&2
        exit 2
    fi
    if ! git -C "${root}" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
        echo "project root is not inside a git worktree: ${root}" >&2
        exit 2
    fi
    if ! git -C "${root}" rev-parse --verify --quiet "${BASE_REF}^{commit}" >/dev/null; then
        echo "base ref is not a commit for ${root}: ${BASE_REF}" >&2
        exit 2
    fi
    source_file_count="$(count_source_files "${root}")"
    SOURCE_FILE_COUNTS+=("${source_file_count}")
    if (( source_file_count < MIN_SOURCE_FILES )); then
        echo "project root has ${source_file_count} source files, below --min-source-files ${MIN_SOURCE_FILES}: ${root}" >&2
        exit 2
    fi
done

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "${TMP_DIR}"' EXIT

now_ns() {
    local value
    value="$(date +%s%N 2>/dev/null || true)"
    if [[ "${value}" =~ ^[0-9]+$ ]]; then
        echo "${value}"
        return
    fi
    python3 -c 'import time; print(int(time.time() * 1_000_000_000))'
}

median() {
    python3 - "$@" <<'PY'
import sys
values = sorted(int(value) for value in sys.argv[1:])
mid = len(values) // 2
print(values[mid] if len(values) % 2 else (values[mid - 1] + values[mid]) // 2)
PY
}

run_one() {
    local label="$1"
    local bin="$2"
    local root_index="$3"
    local root="$4"
    local run_index="$5"
    local stdout_path="${TMP_DIR}/${label}-${root_index}-${run_index}.json"
    local stderr_path="${TMP_DIR}/${label}-${root_index}-${run_index}.stderr"
    local start_ns end_ns status

    start_ns="$(now_ns)"
    set +e
    "${bin}" audit \
        --base "${BASE_REF}" \
        --format json \
        --quiet \
        --no-cache \
        --root "${root}" \
        "${EXTRA_ARGS[@]}" \
        >"${stdout_path}" \
        2>"${stderr_path}"
    status=$?
    set -e
    end_ns="$(now_ns)"

    if (( status > 1 )); then
        echo "${label} root ${root_index} run ${run_index} failed with exit code ${status}" >&2
        tail -40 "${stderr_path}" >&2 || true
        exit "${status}"
    fi

    python3 -m json.tool "${stdout_path}" >/dev/null
    if [[ "${ALLOW_EMPTY_AUDIT}" != true ]]; then
        changed_files_count="$(python3 - "${stdout_path}" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    data = json.load(handle)
print(int(data.get("changed_files_count") or 0))
PY
)"
        if (( changed_files_count == 0 )); then
            echo "${label} root ${root_index} run ${run_index} reported changed_files_count=0" >&2
            echo "Use a base ref with changed files, or pass --allow-empty-audit for a smoke-only run." >&2
            exit 2
        fi
    fi
    echo $(((end_ns - start_ns) / 1000000))
}

root_label() {
    local root="$1"
    basename "${root}"
}

canonicalize() {
    local input="$1"
    local output="$2"
    python3 - "${input}" "${output}" <<'PY'
import json
import sys

VOLATILE_KEYS = {
    "analysis_run_id",
    "elapsed",
    "elapsed_ms",
    "head_sha",
    "telemetry_analysis_run_id",
    "timings",
}

def scrub(value):
    if isinstance(value, dict):
        return {
            key: scrub(child)
            for key, child in sorted(value.items())
            if key not in VOLATILE_KEYS
        }
    if isinstance(value, list):
        return [scrub(child) for child in value]
    return value

with open(sys.argv[1], encoding="utf-8") as handle:
    data = json.load(handle)
with open(sys.argv[2], "w", encoding="utf-8") as handle:
    json.dump(scrub(data), handle, sort_keys=True, separators=(",", ":"))
PY
}

PROJECT_JSONL="${TMP_DIR}/projects.jsonl"

echo "Comparing audit runtime" >&2
echo "  old:  ${OLD_FALLOW_BIN}" >&2
echo "  new:  ${NEW_FALLOW_BIN}" >&2
echo "  old_sha256: ${OLD_FALLOW_SHA}" >&2
echo "  new_sha256: ${NEW_FALLOW_SHA}" >&2
echo "  base: ${BASE_REF}" >&2
echo "  runs: ${RUNS}" >&2
echo "  min_source_files: ${MIN_SOURCE_FILES}" >&2
if [[ -n "${MAX_REGRESSION_PCT}" ]]; then
    echo "  max_regression_pct: ${MAX_REGRESSION_PCT}" >&2
fi

overall_semantic_match=true
overall_regression_ok=true

for root_index in "${!ROOTS[@]}"; do
    root="${ROOTS[${root_index}]}"
    source_file_count="${SOURCE_FILE_COUNTS[${root_index}]}"
    label="$(root_label "${root}")"
    old_times=()
    new_times=()
    semantic_match=true

    echo "  root[$((root_index + 1))]: ${root} (${source_file_count} source files)" >&2
    for ((run = 1; run <= RUNS; run++)); do
        old_ms="$(run_one old "${OLD_FALLOW_BIN}" "${root_index}" "${root}" "${run}")"
        new_ms="$(run_one new "${NEW_FALLOW_BIN}" "${root_index}" "${root}" "${run}")"
        old_times+=("${old_ms}")
        new_times+=("${new_ms}")
        echo "    run ${run}: old=${old_ms}ms new=${new_ms}ms" >&2

        canonicalize "${TMP_DIR}/old-${root_index}-${run}.json" "${TMP_DIR}/old-${root_index}-${run}.canonical.json"
        canonicalize "${TMP_DIR}/new-${root_index}-${run}.json" "${TMP_DIR}/new-${root_index}-${run}.canonical.json"
        if ! cmp -s "${TMP_DIR}/old-${root_index}-${run}.canonical.json" "${TMP_DIR}/new-${root_index}-${run}.canonical.json"; then
            semantic_match=false
            overall_semantic_match=false
            echo "Semantic JSON differs for ${root} run ${run} after volatile fields were removed" >&2
            diff -u "${TMP_DIR}/old-${root_index}-${run}.canonical.json" "${TMP_DIR}/new-${root_index}-${run}.canonical.json" \
                | head -80 >&2 || true
        fi
    done

    old_median="$(median "${old_times[@]}")"
    new_median="$(median "${new_times[@]}")"

    project_json="$(python3 - "${label}" "${root}" "${source_file_count}" "${old_median}" "${new_median}" "${semantic_match}" "${MAX_REGRESSION_PCT}" "${old_times[*]}" "${new_times[*]}" <<'PY'
import json
import sys

label = sys.argv[1]
root = sys.argv[2]
source_file_count = int(sys.argv[3])
old_median = int(sys.argv[4])
new_median = int(sys.argv[5])
semantic_match = sys.argv[6] == "true"
max_regression_pct = sys.argv[7]
old_times = [int(value) for value in sys.argv[8].split()]
new_times = [int(value) for value in sys.argv[9].split()]
speedup = old_median / new_median if new_median else None
regression_pct = ((new_median - old_median) / old_median * 100.0) if old_median else 0.0
regression_ok = True
if max_regression_pct:
    regression_ok = regression_pct <= float(max_regression_pct)

print(json.dumps({
    "label": label,
    "root": root,
    "source_file_count": source_file_count,
    "old_median": old_median,
    "new_median": new_median,
    "speedup": speedup,
    "regression_pct": regression_pct,
    "regression_ok": regression_ok,
    "semantic_match": semantic_match,
    "old_runs": old_times,
    "new_runs": new_times,
}, sort_keys=True, separators=(",", ":")))
PY
)"
    echo "${project_json}" >>"${PROJECT_JSONL}"
    if [[ "$(python3 -c 'import json,sys; print(json.loads(sys.argv[1])["regression_ok"])' "${project_json}")" != "True" ]]; then
        overall_regression_ok=false
    fi
done

SUMMARY_PATH="${TMP_DIR}/summary.json"
python3 - "${OLD_FALLOW_SHA}" "${NEW_FALLOW_SHA}" "${BASE_REF}" "${RUNS}" "${MIN_SOURCE_FILES}" "${MAX_REGRESSION_PCT}" "${overall_semantic_match}" "${overall_regression_ok}" "${PROJECT_JSONL}" >"${SUMMARY_PATH}" <<'PY'
import json
import sys

old_sha256 = sys.argv[1]
new_sha256 = sys.argv[2]
base_ref = sys.argv[3]
runs = int(sys.argv[4])
min_source_files = int(sys.argv[5])
max_regression_pct = sys.argv[6]
semantic_match = sys.argv[7] == "true"
regression_ok = sys.argv[8] == "true"
projects_path = sys.argv[9]

with open(projects_path, encoding="utf-8") as handle:
    projects = [json.loads(line) for line in handle if line.strip()]

total_old_median = sum(project["old_median"] for project in projects)
total_new_median = sum(project["new_median"] for project in projects)
speedup = total_old_median / total_new_median if total_new_median else None

print(json.dumps({
    "name": "audit old-vs-new runtime",
    "unit": "ms",
    "old_sha256": old_sha256,
    "new_sha256": new_sha256,
    "base_ref": base_ref,
    "runs": runs,
    "min_source_files": min_source_files,
    "max_regression_pct": float(max_regression_pct) if max_regression_pct else None,
    "total_source_file_count": sum(project["source_file_count"] for project in projects),
    "old_median": projects[0]["old_median"] if len(projects) == 1 else total_old_median,
    "new_median": projects[0]["new_median"] if len(projects) == 1 else total_new_median,
    "speedup": projects[0]["speedup"] if len(projects) == 1 else speedup,
    "semantic_match": semantic_match,
    "regression_ok": regression_ok,
    "projects": projects,
}, indent=2))
PY

cat "${SUMMARY_PATH}"
if [[ -n "${OUTPUT_PATH}" ]]; then
    cp "${SUMMARY_PATH}" "${OUTPUT_PATH}"
    echo "Wrote audit runtime comparison to ${OUTPUT_PATH}" >&2
fi

if [[ "${overall_semantic_match}" != "true" || "${overall_regression_ok}" != "true" ]]; then
    exit 1
fi
