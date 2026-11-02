import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
  cachedFallow,
  changedRangesFromDiff,
  compactFinding,
  isCensusPath,
  matchPrecision,
  parseArgs,
  parseFixCommit,
  recallHits,
  scopeForFile,
  selectAdvisories,
  sparseCheckoutArgs,
  summarizeRecall,
} from "./security-corpus.mjs";

const repo = {
  name: "demo",
  include: ["packages/demo/src"],
  exclude: ["packages/demo/src/vendored-file.ts"],
};

const finding = (path, line, category = "dangerous-html") => ({
  finding_id: `security:${category}:${path}:${line}:1`,
  kind: "tainted-sink",
  category,
  path,
  line,
  col: 1,
  severity: "medium",
  candidate: { source_kind: null },
});

test("parseArgs selects the phases and the cache directory", () => {
  const options = parseArgs(["--phase", "recall", "--cache-dir", "/cache", "--limit", "5"]);
  assert.equal(options.phase, "recall");
  assert.equal(options.cacheDir, "/cache");
  assert.equal(options.limit, 5);
  assert.throws(() => parseArgs(["--phase", "nope"]), /--phase/);
  assert.throws(() => parseArgs(["--limit", "x"]), /--limit/);
});

test("parseFixCommit splits a slug and a commit", () => {
  assert.deepEqual(parseFixCommit("withastro/astro@a4ffbfaa"), {
    slug: "withastro/astro",
    sha: "a4ffbfaa",
  });
  assert.equal(parseFixCommit("not-a-commit"), null);
});

test("changedRangesFromDiff keeps old-side ranges of source files only", () => {
  const diff = [
    "diff --git a/src/a.ts b/src/a.ts",
    "--- a/src/a.ts",
    "+++ b/src/a.ts",
    "@@ -10,3 +10,4 @@ fn",
    "@@ -40 +41,2 @@",
    "@@ -60,0 +62,5 @@",
    "diff --git a/README.md b/README.md",
    "--- a/README.md",
    "+++ b/README.md",
    "@@ -1,2 +1,2 @@",
    "diff --git a/src/new.ts b/src/new.ts",
    "--- /dev/null",
    "+++ b/src/new.ts",
    "@@ -0,0 +1,9 @@",
  ].join("\n");
  const ranges = changedRangesFromDiff(diff);
  assert.deepEqual([...ranges.keys()], ["src/a.ts"]);
  assert.deepEqual(ranges.get("src/a.ts"), [
    { start: 10, end: 12 },
    { start: 40, end: 40 },
    { start: 60, end: 61 },
  ]);
});

test("isCensusPath applies the include, exclude and test-directory rules", () => {
  assert.equal(isCensusPath("packages/demo/src/a.ts", repo), true);
  assert.equal(isCensusPath("packages/demo/src/__tests__/a.ts", repo), false);
  assert.equal(isCensusPath("packages/demo/src/a.test.ts", repo), false);
  assert.equal(isCensusPath("packages/demo/src/types.d.ts", repo), false);
  assert.equal(isCensusPath("packages/demo/src/vendored-file.ts", repo), false);
  assert.equal(isCensusPath("packages/other/src/a.ts", repo), false);
});

test("matchPrecision joins candidates to classified sinks and lists the rest", () => {
  const sites = [
    {
      id: "demo:001",
      group: "injection",
      kind: "html-property",
      file: "packages/demo/src/a.ts",
      line: 5,
      endLine: 5,
    },
    {
      id: "demo:002",
      group: "injection",
      kind: "eval",
      file: "packages/demo/src/b.ts",
      line: 9,
      endLine: 11,
    },
  ];
  const labels = {
    "demo:001": { category: "user-controlled" },
    "demo:002": { category: "constant" },
  };
  const findings = [
    finding("packages/demo/src/a.ts", 5),
    finding("packages/demo/src/c.ts", 3, "ssrf"),
    finding("packages/demo/src/__tests__/x.ts", 3, "ssrf"),
  ];
  const result = matchPrecision({ repo, sites, labels, findings });
  assert.equal(result.sinks.length, 2);
  assert.deepEqual(
    result.sinks.map((sink) => [sink.id, sink.category, sink.matches.length]),
    [
      ["demo:001", "user-controlled", 1],
      ["demo:002", "constant", 0],
    ],
  );
  assert.deepEqual(
    result.extras.map((extra) => extra.path),
    ["packages/demo/src/c.ts"],
  );
  assert.equal(result.outOfScope, 1);
});

test("recallHits reports a line hit inside the slack and a file hit outside it", () => {
  const ranges = new Map([["src/a.ts", [{ start: 20, end: 22 }]]]);
  const hits = recallHits(
    [finding("src/a.ts", 25), finding("src/a.ts", 90), finding("src/b.ts", 21)],
    ranges,
    3,
  );
  assert.equal(hits.lineHits.length, 1);
  assert.equal(hits.lineHits[0].line, 25);
  assert.equal(hits.fileHits.length, 2);
});

test("summarizeRecall groups advisory outcomes by class", () => {
  const summary = summarizeRecall([
    { ghsa: "A", class: "xss", status: "analyzed", lineHit: true, fileHit: true },
    { ghsa: "B", class: "xss", status: "analyzed", lineHit: false, fileHit: true },
    { ghsa: "C", class: "ssrf", status: "no-source-changes", lineHit: false, fileHit: false },
  ]);
  assert.deepEqual(summary.get("xss"), {
    advisories: 2,
    analyzed: 2,
    lineHit: 1,
    fileHit: 2,
    skipped: 0,
  });
  assert.deepEqual(summary.get("ssrf"), {
    advisories: 1,
    analyzed: 0,
    lineHit: 0,
    fileHit: 0,
    skipped: 1,
  });
});

test("scopeForFile picks the package root and narrows a scope that is too large", () => {
  const tree = [
    "package.json",
    "packages/next/package.json",
    "packages/next/src/server/a.ts",
    "packages/next/src/server/b.ts",
    "src/vs/base/a.ts",
    "src/vs/base/b.ts",
    "src/vs/editor/c.ts",
  ];
  const roots = new Set(["", "packages/next"]);
  assert.equal(scopeForFile("packages/next/src/server/a.ts", tree, roots, 10), "packages/next");
  assert.equal(scopeForFile("src/vs/editor/c.ts", tree, roots, 10), "");
  assert.equal(scopeForFile("src/vs/editor/c.ts", tree, roots, 2), "src/vs/editor");
});

test("sparseCheckoutArgs disables the sparse checkout for a root scope", () => {
  assert.deepEqual(sparseCheckoutArgs([""]), ["sparse-checkout", "disable"]);
  assert.deepEqual(sparseCheckoutArgs(["packages/a", "src/b"]), [
    "sparse-checkout",
    "set",
    "--cone",
    "packages/a",
    "src/b",
  ]);
});

test("compactFinding falls back to the kind when a finding has no category", () => {
  const leak = compactFinding({
    finding_id: "x",
    kind: "client-server-leak",
    path: "src/a.tsx",
    line: 4,
    col: 9,
    severity: "high",
    evidence: "e",
  });
  assert.equal(leak.category, "client-server-leak");
  assert.equal(
    compactFinding({ ...finding("src/a.ts", 1), evidence: "e" }).category,
    "dangerous-html",
  );
});

test("cachedFallow does not cache a failed scan", () => {
  const dir = mkdtempSync(join(tmpdir(), "security-corpus-test-"));
  try {
    const rawPath = join(dir, "raw", "scan.json");
    let calls = 0;
    const failing = () => {
      calls += 1;
      return { ok: false, error: "spawn ENOENT" };
    };
    assert.equal(cachedFallow(rawPath, false, failing).ok, false);
    assert.equal(cachedFallow(rawPath, false, failing).ok, false);
    assert.equal(calls, 2);
    const ok = () => ({ ok: true, findings: [] });
    assert.equal(cachedFallow(rawPath, false, ok).ok, true);
    assert.equal(cachedFallow(rawPath, false, failing).ok, true);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("selectAdvisories applies the class, repo and limit filters in order", () => {
  const advisory = (ghsa, project, fixCommits) => ({ ghsa, project, summary: ghsa, fixCommits });
  const ultracite = {
    advisories: {
      advisories: [
        advisory("A", "astro", ["withastro/astro@aaaaaaa"]),
        advisory("B", "hono", ["honojs/hono@bbbbbbb"]),
        advisory("C", "astro", ["withastro/astro@ccccccc"]),
        advisory("D", "astro", ["withastro/astro@ddddddd"]),
        advisory("E", "astro", []),
      ],
    },
    advisoryLabels: {
      A: { class: "xss" },
      B: { class: "xss" },
      C: { class: "ssrf" },
      D: { class: "xss" },
    },
  };
  const options = {
    ...parseArgs([]),
    classes: new Set(["xss"]),
    repos: new Set(["astro"]),
    limit: 1,
  };
  const result = selectAdvisories(ultracite, options);
  assert.deepEqual(
    result.selected.map((row) => [row.ghsa, row.commits[0].sha]),
    [["A", "aaaaaaa"]],
  );
  assert.deepEqual(
    result.dropped.map((row) => [row.ghsa, row.reason]),
    [
      ["B", "repo filter"],
      ["C", "class filter"],
      ["D", "limit"],
    ],
  );
  assert.equal(result.withoutFix, 1);
});
