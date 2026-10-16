import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const scriptPath = fileURLToPath(new URL("./package-type-aware.mjs", import.meta.url));
const extensionRoot = join(dirname(scriptPath), "..");

test("package-type-aware.mjs starts as a script and rejects an unknown target", () => {
  const root = mkdtempSync(join(tmpdir(), "fallow-package-type-aware-start-"));
  const destination = join(root, "payload");
  try {
    // An unknown target fails before the script copies or removes any file.
    const result = spawnSync(process.execPath, [scriptPath, "linux-armhf", destination], {
      cwd: extensionRoot,
      encoding: "utf8",
      env: { PATH: process.env.PATH ?? "" },
    });
    assert.equal(result.status, 1, result.stderr);
    assert.match(result.stderr, /unsupported VSIX target linux-armhf; expected one of universal,/u);
    assert.equal(existsSync(destination), false);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
