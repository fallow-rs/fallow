const test = require("node:test");
const assert = require("node:assert/strict");
const os = require("node:os");
const { spawnSync } = require("node:child_process");
const path = require("node:path");

const RUN_BINARY = path.join(__dirname, "run-binary.js");

// Run a child that installs guardBrokenStdout, then emits a synthetic stdout
// 'error' with the given code. Node delivers a broken-pipe failure as exactly
// this event ("Emitted 'error' event on Socket instance"), so emitting it is a
// faithful reproduction of `fallow --version | head` without needing a live
// pipe or an installed @fallow-cli platform package. Requiring run-binary.js
// has no side effects beyond defining functions, so no binary is resolved.
function runGuardChild(errorCode) {
  const script =
    `const { guardBrokenStdout } = require(${JSON.stringify(RUN_BINARY)});` +
    `guardBrokenStdout();` +
    `process.stdout.emit("error", Object.assign(new Error("write ${errorCode}"), { code: "${errorCode}" }));` +
    // Reached only if the guard neither exited (EPIPE) nor rethrew (other).
    `process.exit(42);`;
  return spawnSync(process.execPath, ["-e", script], { encoding: "utf8" });
}

test("guardBrokenStdout swallows EPIPE on stdout and exits 0", () => {
  const res = runGuardChild("EPIPE");
  assert.equal(res.status, 0, "EPIPE on stdout should exit 0 cleanly, not crash");
  assert.doesNotMatch(res.stderr, /EPIPE/, "no EPIPE stack trace on stderr");
});

test("guardBrokenStdout rethrows non-EPIPE stdout errors (exit 1)", () => {
  const res = runGuardChild("ENOSPC");
  assert.equal(res.status, 1, "a non-EPIPE stdout error must surface, not be swallowed");
  // Match the thrown error's header ("Error: write ENOSPC"), not just the
  // message substring: a missing-guard TypeError would leak the script source
  // (`new Error("write ENOSPC")`) into its code frame and match a looser regex,
  // masking a regression. The colon-space header only appears on a real rethrow.
  assert.match(res.stderr, /Error: write ENOSPC/, "the rethrown error reaches stderr");
  assert.doesNotMatch(res.stderr, /is not a function/, "guard must be present, not absent");
});

test("isVersionQuery recognizes --version, -V, and -v as the first argument", () => {
  const { isVersionQuery } = require(RUN_BINARY);
  assert.equal(isVersionQuery(["node", "fallow", "--version"]), true);
  assert.equal(isVersionQuery(["node", "fallow", "-V"]), true);
  assert.equal(
    isVersionQuery(["node", "fallow", "-v"]),
    true,
    "-v must append the verified line too",
  );
  assert.equal(isVersionQuery(["node", "fallow"]), false);
  assert.equal(isVersionQuery(["node", "fallow", "dead-code"]), false);
  assert.equal(
    isVersionQuery(["node", "fallow", "dead-code", "-v"]),
    false,
    "-v only counts as the first arg",
  );
});

test("describeVerified annotates the resolved version's signing status", () => {
  const { describeVerified } = require(RUN_BINARY);
  const ok = { ok: true, sentinelPath: "/c/s" };
  // Signed-era version: appended as `signed`.
  assert.match(
    describeVerified(ok, "2.83.0"),
    /verified: yes \(sentinel \/c\/s\); fallow 2\.83\.0 signed/,
  );
  // Pre-signing version: the fleet pre-flight signal, most useful on skip.
  const skipped = { skipped: true, reason: "FALLOW_SKIP_BINARY_VERIFY is set" };
  assert.match(
    describeVerified(skipped, "2.76.0"),
    /verified: skipped \(.*\); fallow 2\.76\.0 unsigned \(predates 2\.77\.0\)/,
  );
  // Unknown / unreadable version: no annotation, version line stays intact.
  assert.equal(describeVerified(ok, undefined), "verified: yes (sentinel /c/s)");
  assert.equal(describeVerified(ok, ""), "verified: yes (sentinel /c/s)");
});

test("exitCodeForChildFailure preserves status codes and maps signal deaths", () => {
  const { exitCodeForChildFailure } = require(RUN_BINARY);
  assert.equal(exitCodeForChildFailure({ status: 3 }), 3);
  assert.equal(exitCodeForChildFailure({ status: 0 }), 0);
  assert.equal(
    exitCodeForChildFailure({ status: null, signal: "SIGSEGV" }),
    128 + os.constants.signals.SIGSEGV,
  );
  assert.equal(
    exitCodeForChildFailure({ status: null, signal: "SIGKILL" }),
    128 + os.constants.signals.SIGKILL,
  );
  assert.equal(exitCodeForChildFailure({ status: null, signal: undefined }), 1);
  assert.equal(exitCodeForChildFailure({ status: null, signal: "NOT_A_SIGNAL" }), 1);
});
