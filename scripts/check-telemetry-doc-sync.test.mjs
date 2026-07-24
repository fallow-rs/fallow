import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

test("telemetry documentation parity fails closed when companions are absent", () => {
  const root = mkdtempSync(join(tmpdir(), "fallow-telemetry-parity-"));
  const result = spawnSync("python3", ["scripts/check_telemetry_doc_sync.py"], {
    cwd: new URL("..", import.meta.url),
    encoding: "utf8",
    env: {
      ...process.env,
      FALLOW_DOCS_DIR: join(root, "missing-docs"),
      FALLOW_SKILLS_DIR: join(root, "missing-skills"),
    },
  });

  assert.equal(result.status, 1);
  assert.match(result.stderr, /expected companion doc not found/u);
});
