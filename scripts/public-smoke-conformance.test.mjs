import assert from "node:assert/strict";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import test from "node:test";

import { parseArgs, runPublicSmoke } from "./public-smoke-conformance.mjs";

test("parseArgs accepts explicit project paths", () => {
  const options = parseArgs([
    "--project",
    "next=/repos/next",
    "--clone",
    "--out-dir",
    "target/out",
  ]);

  assert.equal(options.clone, true);
  assert.equal(options.outDir, "target/out");
  assert.equal(options.projectPaths.get("next"), "/repos/next");
});

test("runPublicSmoke skips missing projects without network opt-in", () => {
  const dir = mkdtempSync(join(tmpdir(), "fallow-public-smoke-"));
  try {
    const manifest = join(dir, "manifest.json");
    writeFileSync(
      manifest,
      JSON.stringify({
        projects: [
          {
            id: "next",
            label: "Next.js",
            category: "next",
            repo: "vercel/next.js",
            ref: "v16.2.1",
            command: ["dead-code"],
            expected_kind: "dead-code",
          },
        ],
      }),
    );

    const report = runPublicSmoke({
      manifest,
      outDir: join(dir, "out"),
      cacheDir: join(dir, "cache"),
      clone: false,
      projectPaths: new Map(),
      rootDir: null,
      fallowBin: null,
    });

    assert.deepEqual(report.summary, {
      total: 1,
      passed: 0,
      failed: 0,
      skipped: 1,
    });
    assert.equal(report.projects[0].status, "skipped");
    assert.equal(report.projects[0].repo, "vercel/next.js");
    assert.equal("root" in report.projects[0], false);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
