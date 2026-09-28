#!/usr/bin/env bash
#
# Train a profile-guided optimization (PGO) profile for the fallow release
# binaries.
#
# Usage:
#   scripts/pgo-train.sh <instrumented-fallow-multicall> <fixtures-dir> <output.profdata>
#
# The binary must be a fallow-multicall build with `-Cprofile-generate`. The
# fixtures directory must contain the training fixtures from
# `benchmarks/download-fixtures.mjs`. The script runs `check`, `dupes` and
# `health` on each training fixture, plus one short `lsp-server` session and one
# short `mcp-server` session. It then merges the raw profiles with the
# `llvm-profdata` of the active rustc toolchain (`rustup component add
# llvm-tools`). Set `LLVM_PROFDATA` to use another `llvm-profdata`.
#
# The held-out fixtures (query, vite, astro) stay out of training on purpose.
# pgo-validate.yml measures the benefit on them.
#
# The script does not write into the fixtures: the analysis cache goes to a
# temporary directory.

set -euo pipefail

readonly TRAIN_FIXTURES=(preact fastify zod vue-core svelte)
readonly TRAIN_COMMANDS=(check dupes health)
# The LSP and MCP sessions use this fixture as the project root.
readonly SESSION_FIXTURE=preact
readonly SESSION_TIMEOUT_MS=180000

fail() {
  echo "pgo-train: error: $*" >&2
  exit 1
}

log() {
  echo "pgo-train: $*" >&2
}

if [ "$#" -ne 3 ]; then
  echo "usage: $0 <instrumented-fallow-multicall> <fixtures-dir> <output.profdata>" >&2
  exit 2
fi

absolute_path() {
  local dir
  dir="$(cd "$(dirname "$1")" && pwd)"
  echo "$dir/$(basename "$1")"
}

[ -d "$2" ] || fail "fixtures directory $2 does not exist"
BIN="$(absolute_path "$1")"
FIXTURES="$(cd "$2" && pwd)"
OUTPUT="$(absolute_path "$3")"

[ -x "$BIN" ] || fail "binary $BIN is missing or not executable"
command -v node >/dev/null 2>&1 || fail "node is required for the LSP and MCP sessions"
for fixture in "${TRAIN_FIXTURES[@]}"; do
  [ -f "$FIXTURES/$fixture/package.json" ] ||
    fail "fixture $FIXTURES/$fixture is missing; run node benchmarks/download-fixtures.mjs"
done

find_llvm_profdata() {
  if [ -n "${LLVM_PROFDATA:-}" ]; then
    echo "$LLVM_PROFDATA"
    return
  fi
  local sysroot host
  sysroot="$(rustc --print sysroot)"
  host="$(rustc -vV | sed -n 's/^host: //p')"
  echo "$sysroot/lib/rustlib/$host/bin/llvm-profdata"
}

PROFDATA_TOOL="$(find_llvm_profdata)"
[ -x "$PROFDATA_TOOL" ] ||
  fail "llvm-profdata not found at $PROFDATA_TOOL; run: rustup component add llvm-tools"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
RAW="$WORK/raw"
mkdir -p "$RAW" "$WORK/out"

# %p gives each process its own raw profile, so no two runs share a file.
export LLVM_PROFILE_FILE="$RAW/fallow-%p.profraw"
# An absolute cache directory keeps every CLI run out of the fixture tree. The
# LSP server does not read FALLOW_CACHE_DIR, so its session gets a config file
# with an absolute `cache.dir` instead.
export FALLOW_CACHE_DIR="$WORK/cache"
LSP_CONFIG="$WORK/lsp-config.json"
printf '{ "cache": { "dir": "%s" } }\n' "$WORK/lsp-cache" >"$LSP_CONFIG"
# Files newer than this marker show a write into a fixture.
START_MARKER="$WORK/start"
touch "$START_MARKER"

fixture_writes() {
  local fixture
  for fixture in "${TRAIN_FIXTURES[@]}"; do
    find "$FIXTURES/$fixture" -name node_modules -prune -o -newer "$START_MARKER" -print
  done
}

raw_count() {
  find "$RAW" -name '*.profraw' -type f | wc -l | tr -d ' '
}

# Fallow exits 0 without error findings and 1 with error findings. Any other
# exit is an execution error or a signal, and a partial profile is not valid.
check_exit() {
  local label="$1" code="$2" err="$3"
  if [ "$code" -eq 0 ] || [ "$code" -eq 1 ]; then
    return
  fi
  if [ "$code" -gt 128 ]; then
    echo "pgo-train: $label stopped on signal $((code - 128))" >&2
  fi
  tail -20 "$err" >&2 || true
  fail "$label exited with code $code"
}

run_cli() {
  local fixture="$1" command="$2"
  local label="$fixture $command"
  local out="$WORK/out/$fixture-$command.json" err="$WORK/out/$fixture-$command.err"
  local before code
  before="$(raw_count)"
  set +e
  (cd "$FIXTURES/$fixture" && "$BIN" "$command" --format json --quiet --no-cache >"$out" 2>"$err")
  code=$?
  set -e
  check_exit "$label" "$code" "$err"
  [ -s "$out" ] || fail "$label wrote no output"
  [ "$(head -c 1 "$out")" = "{" ] || fail "$label did not write a JSON object"
  [ "$(raw_count)" -gt "$before" ] || fail "$label wrote no raw profile"
  log "$label: exit $code"
}

# One stdio session: `lsp-server` or `mcp-server`. The node client sends the
# messages, waits for each answer, and exits non-zero on a timeout or a
# protocol error. The server must then exit on its own.
run_session() {
  local mode="$1"
  local label="$mode session"
  local err="$WORK/out/$mode.err"
  local before code
  before="$(raw_count)"
  set +e
  node - "$BIN" "$mode" "$FIXTURES/$SESSION_FIXTURE" "$SESSION_TIMEOUT_MS" "$LSP_CONFIG" \
    2>"$err" <<'JS'
const { spawn } = require("node:child_process");
const { readdirSync } = require("node:fs");
const { join } = require("node:path");
const { pathToFileURL } = require("node:url");

const [bin, mode, root, timeoutMs, configPath] = process.argv.slice(2);
const child = spawn(bin, [mode], { cwd: root, stdio: ["pipe", "pipe", "inherit"] });
const timer = setTimeout(() => {
  console.error(`${mode}: no answer within ${timeoutMs} ms`);
  child.kill("SIGKILL");
  process.exit(3);
}, Number(timeoutMs));

let buffer = Buffer.alloc(0);
const waiters = [];
const onMessage = (message) => {
  const index = waiters.findIndex((waiter) => waiter.match(message));
  if (index !== -1) waiters.splice(index, 1)[0].resolve(message);
};
const waitFor = (match) => new Promise((resolve) => waiters.push({ match, resolve }));

const lsp = mode === "lsp-server";
child.stdout.on("data", (chunk) => {
  buffer = Buffer.concat([buffer, chunk]);
  for (;;) {
    if (lsp) {
      const headerEnd = buffer.indexOf("\r\n\r\n");
      if (headerEnd === -1) return;
      const length = Number(/Content-Length: (\d+)/i.exec(buffer.subarray(0, headerEnd))[1]);
      if (buffer.length < headerEnd + 4 + length) return;
      onMessage(JSON.parse(buffer.subarray(headerEnd + 4, headerEnd + 4 + length)));
      buffer = buffer.subarray(headerEnd + 4 + length);
    } else {
      const lineEnd = buffer.indexOf("\n");
      if (lineEnd === -1) return;
      const line = buffer.subarray(0, lineEnd).toString().trim();
      buffer = buffer.subarray(lineEnd + 1);
      if (line) onMessage(JSON.parse(line));
    }
  }
});

const send = (message) => {
  const body = JSON.stringify({ jsonrpc: "2.0", ...message });
  child.stdin.write(lsp ? `Content-Length: ${Buffer.byteLength(body)}\r\n\r\n${body}` : `${body}\n`);
};
const request = async (id, method, params) => {
  const answer = waitFor((message) => message.id === id && !message.method);
  send({ id, method, params });
  const message = await answer;
  if (message.error) throw new Error(`${method}: ${JSON.stringify(message.error)}`);
  return message.result;
};

const firstSourceFile = (dir) => {
  const entry = readdirSync(dir, { withFileTypes: true }).find(
    (item) => item.isFile() && /\.(?:[cm]?[jt]sx?)$/.test(item.name),
  );
  if (!entry) throw new Error(`no source file in ${dir}`);
  return join(dir, entry.name);
};

const runLsp = async () => {
  const rootUri = pathToFileURL(root).href;
  await request(1, "initialize", {
    processId: process.pid,
    rootUri,
    capabilities: {},
    initializationOptions: { configPath },
  });
  send({ method: "initialized", params: {} });
  const file = firstSourceFile(join(root, "src"));
  const uri = pathToFileURL(file).href;
  const published = waitFor((message) => message.method === "textDocument/publishDiagnostics");
  // Opening the first document starts the workspace analysis.
  send({
    method: "textDocument/didOpen",
    params: { textDocument: { uri, languageId: "javascript", version: 1, text: "" } },
  });
  await published;
  await request(2, "textDocument/codeLens", { textDocument: { uri } });
  await request(3, "shutdown", null);
  send({ method: "exit", params: null });
};

const runMcp = async () => {
  await request(1, "initialize", {
    protocolVersion: "2025-06-18",
    capabilities: {},
    clientInfo: { name: "pgo-train", version: "1.0.0" },
  });
  send({ method: "notifications/initialized" });
  const result = await request(2, "tools/list", {});
  if (!Array.isArray(result?.tools) || result.tools.length === 0) {
    throw new Error("tools/list returned no tools");
  }
  child.stdin.end();
};

child.on("exit", (code, signal) => {
  clearTimeout(timer);
  if (signal) {
    console.error(`${mode}: stopped on signal ${signal}`);
    process.exit(4);
  }
  process.exit(code === 0 ? 0 : 5);
});

(lsp ? runLsp() : runMcp()).catch((error) => {
  console.error(`${mode}: ${error.message}`);
  child.kill("SIGKILL");
  process.exit(6);
});
JS
  code=$?
  set -e
  if [ "$code" -ne 0 ]; then
    tail -20 "$err" >&2 || true
    fail "$label failed with code $code"
  fi
  [ "$(raw_count)" -gt "$before" ] || fail "$label wrote no raw profile"
  log "$label: exit 0"
}

for fixture in "${TRAIN_FIXTURES[@]}"; do
  for command in "${TRAIN_COMMANDS[@]}"; do
    run_cli "$fixture" "$command"
  done
done
run_session lsp-server
run_session mcp-server

written="$(fixture_writes | head -5)"
[ -z "$written" ] || fail "training wrote into the fixtures: $written"

log "merging $(raw_count) raw profiles with $PROFDATA_TOOL"
mkdir -p "$(dirname "$OUTPUT")"
"$PROFDATA_TOOL" merge -o "$OUTPUT" "$RAW"
[ -s "$OUTPUT" ] || fail "merged profile $OUTPUT is empty"

functions="$("$PROFDATA_TOOL" show "$OUTPUT" | sed -n 's/^Total functions: //p')"
[ -n "$functions" ] && [ "$functions" -gt 0 ] ||
  fail "merged profile $OUTPUT has no function counts"

log "wrote $OUTPUT ($(wc -c <"$OUTPUT" | tr -d ' ') bytes, $functions functions)"
