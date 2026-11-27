import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { countMissingFunctions } from "./pgo-profile-match.mjs";

const SCRIPT = fileURLToPath(new URL("./pgo-build-log.mjs", import.meta.url));

const warning = (name) =>
  `warning: fallow_core.abc-cgu.0: no profile data available for function ${name} Hash = 1 up to 0 count discarded`;

const withDir = (body) => {
  const dir = mkdtempSync(join(tmpdir(), "pgo build log "));
  try {
    body(dir);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
};

test("the log keeps every line and the console drops the missing-function warnings", () => {
  withDir((dir) => {
    const log = join(dir, "build.log");
    const input = [
      "   Compiling fallow-core v1.0.0",
      warning("a"),
      "",
      warning("b"),
      "",
      "warning: unused variable: `x`",
      "",
      "    Finished `release` profile",
      "",
    ].join("\n");
    const run = spawnSync(process.execPath, [SCRIPT, log], { input, encoding: "utf8" });
    assert.equal(run.status, 0);
    assert.equal(readFileSync(log, "utf8"), input);
    assert.equal(countMissingFunctions(readFileSync(log, "utf8")), 2);
    assert.doesNotMatch(run.stdout, /no profile data available/u);
    assert.match(run.stdout, /Compiling fallow-core/u);
    assert.match(run.stdout, /warning: unused variable: `x`\n\n/u);
    assert.match(run.stdout, /Finished `release` profile/u);
    assert.match(run.stdout, /2 PGO missing-function warnings written to .*build\.log/u);
  });
});

test("the command exits 2 without a log path", () => {
  const run = spawnSync(process.execPath, [SCRIPT], { input: "", encoding: "utf8" });
  assert.equal(run.status, 2);
});
