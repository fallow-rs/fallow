import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const scriptPath = fileURLToPath(new URL("./package-vsix-variants.mjs", import.meta.url));
const extensionRoot = join(dirname(scriptPath), "..");

test("package-vsix-variants.mjs starts as a script and rejects an unknown argument", () => {
  const root = mkdtempSync(join(tmpdir(), "fallow-package-vsix-variants-start-"));
  const outputDirectory = join(root, "vsix");
  try {
    // Argument parsing fails before the script builds, stages, or packages anything.
    const result = spawnSync(
      process.execPath,
      [scriptPath, "--output-dir", outputDirectory, "--unknown-flag"],
      {
        cwd: extensionRoot,
        encoding: "utf8",
        env: { PATH: process.env.PATH ?? "" },
      },
    );
    assert.equal(result.status, 1, result.stderr);
    assert.match(result.stderr, /unknown package variants argument --unknown-flag/u);
    assert.equal(result.stdout, "");
    assert.equal(existsSync(outputDirectory), false);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
