import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const scriptPath = fileURLToPath(new URL("./verify-packaged-type-aware.mjs", import.meta.url));
const extensionRoot = join(dirname(scriptPath), "..");

test("verify-packaged-type-aware.mjs starts as a script and rejects an unknown argument", () => {
  const root = mkdtempSync(join(tmpdir(), "fallow-verify-packaged-type-aware-start-"));
  try {
    // Argument parsing fails before the script reads the empty extension root.
    const result = spawnSync(
      process.execPath,
      [scriptPath, root, join(root, "missing.vsix"), "--unknown-flag"],
      {
        cwd: extensionRoot,
        encoding: "utf8",
        env: { PATH: process.env.PATH ?? "" },
      },
    );
    assert.equal(result.status, 1, result.stderr);
    assert.match(result.stderr, /unknown verifier argument --unknown-flag/u);
    assert.equal(result.stdout, "");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
