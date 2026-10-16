import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";

const REPO_ROOT = resolve(import.meta.dirname, "..");
const SCRIPT = "scripts/semantic-clone-candle-conformance.mjs";

test("semantic-clone-candle-conformance.mjs loads the corpus when Node starts it", () => {
  const dir = mkdtempSync(join(tmpdir(), "fallow-candle-start-"));
  try {
    // The sidecar path does not exist, so the run stops before any model work.
    const missingBin = join(dir, "missing-fallow-similar-code");
    const result = spawnSync(process.execPath, [SCRIPT, "--sidecar-bin", missingBin, "--pretty"], {
      cwd: REPO_ROOT,
      encoding: "utf8",
      env: { PATH: process.env.PATH ?? "" },
    });

    assert.equal(result.status, 1, result.stderr);
    assert.equal(result.stdout, "");
    assert.match(result.stderr, /^failed to start the production Candle sidecar: .*ENOENT/u);
  } finally {
    rmSync(dir, { force: true, recursive: true });
  }
});
