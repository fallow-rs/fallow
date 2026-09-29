import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { countMissingFunctions, matchFailure } from "./pgo-profile-match.mjs";

const SCRIPT = fileURLToPath(new URL("./pgo-profile-match.mjs", import.meta.url));

const warning = (name) =>
  `warning: fallow_core.abc-cgu.0: no profile data available for function ${name} Hash = 1 up to 0 count discarded`;

test("only the missing-function warnings count", () => {
  const log = [warning("a"), "", "   Compiling fallow-core v1.0.0", warning("b")].join("\r\n");
  assert.equal(countMissingFunctions(log), 2);
  assert.equal(countMissingFunctions(""), 0);
});

// 2646 and 30846 come from local aarch64-apple-darwin builds with one profile:
// the package set of the training build, and fallow-cli alone.
test("a matching build stays at or under the missing ratio", () => {
  assert.equal(matchFailure({ missing: 2646, functions: 60656, maxMissingRatio: 0.2 }), null);
  assert.equal(matchFailure({ missing: 20, functions: 100, maxMissingRatio: 0.2 }), null);
});

test("a build that another profile trained fails the check", () => {
  const failure = matchFailure({ missing: 30846, functions: 60656, maxMissingRatio: 0.2 });
  assert.match(failure, /50\.9% of the 60656 profile functions/u);
  assert.match(failure, /does not match this build/u);
});

test("a profile without functions fails the check", () => {
  assert.match(matchFailure({ missing: 0, functions: 0, maxMissingRatio: 0.2 }), /no functions/u);
});

test("the command exits 1 on a mismatch and 2 without the required options", () => {
  const dir = mkdtempSync(join(tmpdir(), "pgo profile match "));
  try {
    const log = join(dir, "build.log");
    writeFileSync(log, `${warning("a")}\n${warning("b")}\n`);
    const run = (args) => spawnSync(process.execPath, [SCRIPT, ...args], { encoding: "utf8" });
    assert.equal(run(["--log", log, "--functions", "100"]).status, 0);
    const mismatch = run(["--log", log, "--functions", "4"]);
    assert.equal(mismatch.status, 1);
    assert.match(mismatch.stdout, /Profile match: fail/u);
    assert.equal(run([]).status, 2);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
