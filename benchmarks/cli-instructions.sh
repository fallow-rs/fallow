#!/usr/bin/env bash
#
# Whole-binary instruction counts on pinned public projects.
#
# The CodSpeed exec harness runs the release binary under CPU simulation, so
# each (project, command, cache state) gets an instruction-based value that
# does not change with the machine load. This script owns the corpus and the
# benchmark list. `.github/workflows/bench-cli-instructions.yml` calls it.
#
# Subcommands:
#   prepare   Clone the corpus at pinned commits, write the bench config and
#             fill the warm caches. Fails when a benchmark command does not
#             exit 0, because the exec harness rejects a non-zero exit.
#   config    Print the CodSpeed config (codspeed.yml) for the benchmarks.
#   counters  Print the --performance work counters of each dead-code run as
#             github-action-benchmark JSON (customSmallerIsBetter).
#   commands  Print one benchmark per line: name, then the command.
#
# Usage:
#   benchmarks/cli-instructions.sh prepare  --fallow-bin target/release/fallow
#   benchmarks/cli-instructions.sh config   --fallow-bin target/release/fallow > codspeed.yml
#   benchmarks/cli-instructions.sh counters --fallow-bin target/release/fallow > counters.json
#
# Options:
#   --fallow-bin PATH   The fallow binary (required).
#   --work-dir DIR      Corpus and config directory (default: target/cli-instructions).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

# name, GitHub repository, pinned commit. Public projects only. The commits
# are the release tags that benchmarks/bench-ci.sh also uses. Change a commit
# only on purpose, because every value of that project moves with it.
PROJECTS=(
    "preact   preactjs/preact  055cc5b8c62326fbb0fcaccb9816504e82f121b8"
    "zod      colinhacks/zod   e30870369d5b8f31ff4d0130d4439fd997deb523"
    "vue-core vuejs/core       fdd863f617f98c3d41cb8b2401d8e550d8a44d34"
)

# Commands per project. The audit base is the parent commit, so the clones
# fetch a depth of 2.
COMMANDS=("dead-code" "audit")
CACHE_STATES=("cold" "warm")

FALLOW_BIN=""
WORK_DIR="${REPO_ROOT}/target/cli-instructions"

subcommand="${1:-}"
if [[ -z "${subcommand}" ]]; then
    echo "Usage: $0 prepare|config|counters|commands --fallow-bin PATH [--work-dir DIR]" >&2
    exit 2
fi
shift

while [[ $# -gt 0 ]]; do
    case "$1" in
        --fallow-bin)   FALLOW_BIN="$2"; shift 2 ;;
        --fallow-bin=*) FALLOW_BIN="${1#*=}"; shift ;;
        --work-dir)     WORK_DIR="$2"; shift 2 ;;
        --work-dir=*)   WORK_DIR="${1#*=}"; shift ;;
        *) echo "Unknown argument: $1" >&2; exit 2 ;;
    esac
done

if [[ -z "${FALLOW_BIN}" ]]; then
    echo "Error: --fallow-bin is required" >&2
    exit 2
fi

absolute_path() {
    local path="$1"
    if [[ "${path}" != /* ]]; then
        path="$(pwd)/${path}"
    fi
    printf '%s\n' "${path}"
}

FALLOW_BIN="$(absolute_path "${FALLOW_BIN}")"
WORK_DIR="$(absolute_path "${WORK_DIR}")"
CONFIG_FILE="${WORK_DIR}/fallow-bench.json"

# Set BENCH_ARGS to the argument list for one benchmark.
# One thread keeps the instruction count stable: rayon splits work on steals.
# The CLI does not read RAYON_NUM_THREADS, so the flag is explicit.
BENCH_ARGS=()
benchmark_args() {
    local project_dir="$1" command="$2" state="$3"
    BENCH_ARGS=("${FALLOW_BIN}" "${command}" --quiet --format json --threads 1
        --config "${CONFIG_FILE}" --root "${project_dir}")
    if [[ "${command}" == "audit" ]]; then
        BENCH_ARGS+=(--base HEAD~1)
    fi
    if [[ "${state}" == "cold" ]]; then
        BENCH_ARGS+=(--no-cache)
    fi
}

# Call "$1 name project_dir command state" for each benchmark.
for_each_benchmark() {
    local callback="$1" entry name repo sha command state
    for entry in "${PROJECTS[@]}"; do
        read -r name repo sha <<< "${entry}"
        for command in "${COMMANDS[@]}"; do
            for state in "${CACHE_STATES[@]}"; do
                "${callback}" "cli ${name} ${command} (${state})" "${WORK_DIR}/${name}" "${command}" "${state}"
            done
        done
    done
}

clone_project() {
    local name="$1" repo="$2" sha="$3"
    local dest="${WORK_DIR}/${name}"
    if [[ "$(git -C "${dest}" rev-parse HEAD 2>/dev/null || true)" != "${sha}" ]]; then
        rm -rf "${dest}"
        git init -q "${dest}"
        git -C "${dest}" fetch -q --depth 2 "https://github.com/${repo}.git" "${sha}"
        git -C "${dest}" -c advice.detachedHead=false checkout -q FETCH_HEAD
    fi
    # Start each prepare from a clean tree and an empty cache.
    git -C "${dest}" clean -q -fdx
}

# The default config makes error-severity findings exit 1, and the exec
# harness rejects a non-zero exit. The bench config keeps every default and
# only turns each "error" rule into "warn", so the analysis work is the same.
write_bench_config() {
    "${FALLOW_BIN}" config-schema | python3 -c '
import json, sys
rules = json.load(sys.stdin)["properties"]["rules"]["default"]
warned = {rule: "warn" for rule, severity in rules.items() if severity == "error"}
print(json.dumps({"rules": warned}, indent=2, sort_keys=True))
' > "${CONFIG_FILE}"
}

run_checked() {
    local name="$1" project_dir="$2" command="$3" state="$4"
    benchmark_args "${project_dir}" "${command}" "${state}"
    if ! "${BENCH_ARGS[@]}" > /dev/null; then
        echo "Error: '${name}' did not exit 0; the exec harness would reject it" >&2
        return 1
    fi
    echo "  ok: ${name}" >&2
}

cmd_prepare() {
    local entry name repo sha
    mkdir -p "${WORK_DIR}"
    write_bench_config
    for entry in "${PROJECTS[@]}"; do
        read -r name repo sha <<< "${entry}"
        echo "Cloning ${name} at ${sha}" >&2
        clone_project "${name}" "${repo}" "${sha}"
    done
    # The warm benchmarks run after this, so this run fills their caches. Each
    # cold run uses --no-cache and does not read or change them.
    for_each_benchmark run_checked
}

print_config_entry() {
    local name="$1" project_dir="$2" command="$3" state="$4"
    local quoted
    benchmark_args "${project_dir}" "${command}" "${state}"
    quoted="$(python3 -c 'import shlex, sys; print(shlex.join(sys.argv[1:]))' "${BENCH_ARGS[@]}")"
    printf '  - name: %s\n    exec: %s\n' \
        "$(python3 -c 'import json, sys; print(json.dumps(sys.argv[1]))' "${name}")" \
        "$(python3 -c 'import json, sys; print(json.dumps(sys.argv[1]))' "${quoted}")"
}

cmd_config() {
    echo "# Generated by benchmarks/cli-instructions.sh config. Do not edit."
    echo "benchmarks:"
    for_each_benchmark print_config_entry
}

print_command() {
    local name="$1" project_dir="$2" command="$3" state="$4"
    benchmark_args "${project_dir}" "${command}" "${state}"
    printf '%s\t%s\n' "${name}" "${BENCH_ARGS[*]}"
}

cmd_commands() {
    for_each_benchmark print_command
}

COUNTERS_TMP=""

collect_counters() {
    local name="$1" project_dir="$2" command="$3" state="$4"
    if [[ "${command}" != "dead-code" ]]; then
        return 0
    fi
    benchmark_args "${project_dir}" "${command}" "${state}"
    "${BENCH_ARGS[@]}" --performance > /dev/null 2> "${COUNTERS_TMP}/stderr.txt"
    python3 - "${name}" "${COUNTERS_TMP}/stderr.txt" >> "${COUNTERS_TMP}/entries.jsonl" <<'PY'
import json, sys
name, path = sys.argv[1], sys.argv[2]
text = open(path, encoding="utf-8").read()
start = text.find("{")
if start < 0:
    sys.exit(f"no --performance JSON for {name}")
report, _ = json.JSONDecoder().raw_decode(text, start)
for key, value in report["counters"].items():
    print(json.dumps({"name": f"{name}: {key}", "unit": "count", "value": value}))
PY
}

cmd_counters() {
    COUNTERS_TMP="$(mktemp -d)"
    trap 'rm -rf "${COUNTERS_TMP}"' EXIT
    : > "${COUNTERS_TMP}/entries.jsonl"
    for_each_benchmark collect_counters
    python3 -c '
import json, sys
print(json.dumps([json.loads(line) for line in open(sys.argv[1], encoding="utf-8")], indent=2))
' "${COUNTERS_TMP}/entries.jsonl"
}

case "${subcommand}" in
    prepare)  cmd_prepare ;;
    config)   cmd_config ;;
    counters) cmd_counters ;;
    commands) cmd_commands ;;
    *) echo "Unknown subcommand: ${subcommand}" >&2; exit 2 ;;
esac
