import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import test from "node:test";

import { makeTempDir } from "./lib/temp-dir.mjs";
import {
  main,
  parseArgs,
  runPublicSmoke,
  summarizeFallowOutput,
} from "./public-smoke-conformance.mjs";

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

test("main fails when no project ran", () => {
  const dir = mkdtempSync(join(tmpdir(), "fallow-public-smoke-"));
  try {
    const manifest = join(dir, "manifest.json");
    writeFileSync(
      manifest,
      JSON.stringify({
        projects: [{ id: "next", category: "next", repo: "vercel/next.js", ref: "v16.2.1" }],
      }),
    );
    const outDir = join(dir, "out");

    const exitCode = main(["--manifest", manifest, "--out-dir", outDir]);

    assert.equal(exitCode, 2);
    const markdown = readFileSync(join(outDir, "public-smoke-summary.md"), "utf8");
    assert.match(markdown, /^Status: no project ran$/m);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("parseArgs accepts inline project and option values", () => {
  const options = parseArgs(["--project=next=/repos/next", "--out-dir=target/out"]);

  assert.equal(options.projectPaths.get("next"), "/repos/next");
  assert.equal(options.outDir, "target/out");
});

test("parseArgs rejects a project without a path", () => {
  assert.throws(
    () => parseArgs(["--project", "next"]),
    /--project must look like id=\/path\/to\/project/,
  );
});

test("runPublicSmoke reports a fallow binary that does not start as a failed project", () => {
  const dir = makeTempDir("fallow-public-smoke-");
  const manifest = join(dir, "manifest.json");
  writeFileSync(
    manifest,
    JSON.stringify({
      projects: [{ id: "next", category: "next", repo: "vercel/next.js", ref: "v16.2.1" }],
    }),
  );

  const report = runPublicSmoke({
    manifest,
    outDir: join(dir, "out"),
    cacheDir: join(dir, "cache"),
    clone: false,
    projectPaths: new Map([["next", dir]]),
    rootDir: null,
    fallowBin: join(dir, "missing-fallow"),
  });

  assert.equal(report.summary.failed, 1);
  assert.match(report.projects[0].reason, /failed to start/);
});

test("summarizeFallowOutput counts findings from the summary, not from root arrays", () => {
  const output = JSON.stringify({
    kind: "dead-code",
    schema_version: 1,
    total_issues: 1,
    summary: { total_issues: 1, unused_files: 1 },
    unused_files: [{}],
    next_steps: [{}],
    workspace_diagnostics: [{}],
  });

  const summary = summarizeFallowOutput(output);

  assert.equal(summary.total_issues, 1);
  assert.deepEqual(summary.issue_counts, { unused_files: 1 });
});
