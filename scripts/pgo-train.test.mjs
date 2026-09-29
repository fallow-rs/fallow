import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

const SCRIPT = "scripts/pgo-train.sh";

const withWorkDir = (run) => {
  const dir = mkdtempSync(join(tmpdir(), "pgo-train-test-"));
  try {
    const bin = join(dir, "fallow-multicall");
    writeFileSync(bin, "#!/bin/sh\nexit 0\n");
    chmodSync(bin, 0o755);
    const fixtures = join(dir, "fixtures");
    mkdirSync(fixtures);
    run({ dir, bin, fixtures });
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
};

const train = (bin, fixtures, output) =>
  spawnSync("bash", [SCRIPT, bin, fixtures, output], { encoding: "utf8" });

test("training creates a missing output directory before it resolves the output path", () => {
  withWorkDir(({ dir, bin, fixtures }) => {
    const outputDir = join(dir, "pgo-profile");
    // The empty fixtures directory stops the script after the path checks.
    const result = train(bin, fixtures, join(outputDir, "fallow.profdata"));
    assert.equal(result.status, 1);
    assert.match(result.stderr, /fixture .* is missing/u);
    assert.ok(existsSync(outputDir), "the script must create the output directory");
  });
});

test("training stops when it cannot create the output directory", () => {
  withWorkDir(({ dir, bin, fixtures }) => {
    const file = join(dir, "not-a-directory");
    writeFileSync(file, "");
    const result = train(bin, fixtures, join(file, "fallow.profdata"));
    assert.equal(result.status, 1);
    assert.match(result.stderr, /cannot create the output directory/u);
  });
});
