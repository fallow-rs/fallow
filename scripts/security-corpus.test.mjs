import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
  aggregateAdvisory,
  cachedFallow,
  changedRangesFromDiff,
  compactFinding,
  guardPrecisionRepo,
  isCensusPath,
  matchPrecision,
  parseArgs,
  parseDatasetSpec,
  parseFixCommit,
  persistenceCell,
  persistingLineHits,
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

const lines = (prefix, count) => Array.from({ length: count }, (_, i) => `${prefix}line ${i}`);

test("changedRangesFromDiff keeps old-side ranges of source files only", () => {
  const diff = [
    "diff --git a/src/a.ts b/src/a.ts",
    "--- a/src/a.ts",
    "+++ b/src/a.ts",
    "@@ -10,3 +10,4 @@ fn",
    ...lines("-", 3),
    ...lines("+", 4),
    "@@ -40 +41,2 @@",
    "-x",
    ...lines("+", 2),
    "@@ -60,0 +62,5 @@",
    ...lines("+", 5),
    "diff --git a/README.md b/README.md",
    "--- a/README.md",
    "+++ b/README.md",
    "@@ -1,2 +1,2 @@",
    ...lines("-", 2),
    ...lines("+", 2),
    "diff --git a/src/new.ts b/src/new.ts",
    "--- /dev/null",
    "+++ b/src/new.ts",
    "@@ -0,0 +1,9 @@",
    ...lines("+", 9),
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
  const dataset = {
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
  const result = selectAdvisories(dataset, options);
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

test("parseArgs rejects a zero limit and accepts a zero slack", () => {
  assert.throws(() => parseArgs(["--limit", "0"]), /--limit requires an integer of at least 1/);
  assert.throws(() => parseArgs(["--timeout-secs", "0"]), /--timeout-secs/);
  assert.throws(() => parseArgs(["--slack", "-1"]), /--slack requires an integer of at least 0/);
  assert.equal(parseArgs(["--slack", "0"]).slack, 0);
});

test("changedRangesFromDiff reads removed lines that look like file headers as hunk lines", () => {
  const diff = [
    "diff --git a/x.ts b/x.ts",
    "--- a/x.ts",
    "+++ b/x.ts",
    "@@ -5,3 +5,1 @@",
    "--- a/not.ts",
    "-- a/y.ts",
    "-b",
    "+c",
    "@@ -20 +19 @@",
    "-q",
    "\\ No newline at end of file",
    "+r",
    "diff --git a/z.ts b/z.ts",
    "--- a/z.ts",
    "+++ b/z.ts",
    "@@ -1 +1 @@",
    "-a",
    "+b",
  ].join("\n");
  const ranges = changedRangesFromDiff(diff);
  assert.deepEqual([...ranges.keys()], ["x.ts", "z.ts"]);
  assert.deepEqual(ranges.get("x.ts"), [
    { start: 5, end: 7 },
    { start: 20, end: 20 },
  ]);
});

test("cachedFallow scans again when the cache key differs", () => {
  const dir = mkdtempSync(join(tmpdir(), "security-corpus-test-"));
  try {
    const rawPath = join(dir, "raw", "scan.json");
    let calls = 0;
    const produce = () => {
      calls += 1;
      return { ok: true, findings: [] };
    };
    cachedFallow(rawPath, false, produce, { fallow: "a", scopes: ["src"] });
    cachedFallow(rawPath, false, produce, { fallow: "a", scopes: ["src"] });
    assert.equal(calls, 1);
    cachedFallow(rawPath, false, produce, { fallow: "b", scopes: ["src"] });
    assert.equal(calls, 2);
    cachedFallow(rawPath, false, produce, { fallow: "b", scopes: ["lib"] });
    assert.equal(calls, 3);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("persistingLineHits returns null when the fix scan failed", () => {
  const hits = [finding("src/a.ts", 3)];
  assert.equal(persistingLineHits(hits, { ok: false, error: "timed out" }), null);
  assert.deepEqual(persistingLineHits(hits, { ok: true, findings: [] }), []);
  assert.deepEqual(persistingLineHits(hits, { ok: true, findings: [finding("src/a.ts", 9)] }), [
    hits[0].finding_id,
  ]);
});

test("a failed fix scan marks line hits as unknown and records the error", () => {
  const hit = finding("src/a.ts", 3);
  const row = aggregateAdvisory(
    {
      ghsa: "GHSA-x",
      project: "demo",
      class: "xss",
      lint: null,
      summary: "s",
      commits: [{ slug: "o/r", sha: "abc" }],
    },
    new Map([
      [
        "abc",
        {
          status: "analyzed",
          fileHits: [],
          lineHits: [hit],
          persistingAfterFix: null,
          fixError: "timed out",
        },
      ],
    ]),
  );
  assert.equal(row.lineHits[0].persistsAfterFix, null);
  assert.equal(row.commits[0].fixError, "timed out");
  assert.equal(persistenceCell(row.lineHits), "0/1 (1 unknown: fix scan failed)");
  assert.equal(persistenceCell([{ persistsAfterFix: true }, { persistsAfterFix: false }]), "1/2");
});

test("guardPrecisionRepo turns a thrown error into an error entry", () => {
  const entry = guardPrecisionRepo({ name: "zod" }, () => {
    throw new Error("git fetch failed");
  });
  assert.equal(entry.repo, "zod");
  assert.equal(entry.error, "git fetch failed");
  assert.equal(typeof entry.sha, "string");
});

test("parseDatasetSpec reads a pinned owner/repo@sha and rejects other values", () => {
  const sha = "0123456789abcdef0123456789abcdef01234567";
  assert.deepEqual(parseDatasetSpec(`acme/sink-study@${sha}`), { slug: "acme/sink-study", sha });
  assert.throws(() => parseDatasetSpec(undefined), /--dataset-dir/);
  assert.throws(() => parseDatasetSpec("acme/sink-study"), /owner\/repo@/);
  assert.throws(() => parseDatasetSpec("acme/sink-study@main"), /owner\/repo@/);
});

test("parseArgs reads --dataset-dir", () => {
  assert.equal(parseArgs(["--dataset-dir", "/data/study"]).datasetDir, "/data/study");
});

test("matchPrecision joins a script element sink to the createElement line before it", () => {
  const file = "packages/demo/src/loader.ts";
  const site = (id, line) => ({
    id,
    group: "injection",
    kind: "script-element",
    file,
    line,
    endLine: line,
  });
  const scriptSink = (line) => ({
    ...finding(file, line, "code-injection"),
    evidence:
      "Non-literal code assigned to HTMLScriptElement.src (the browser runs it as a script).",
  });
  const sites = [site("demo:010", 10), site("demo:011", 40)];
  const findings = [scriptSink(17), scriptSink(45), finding(file, 20, "code-injection")];
  const result = matchPrecision({ repo, sites, labels: {}, findings });
  assert.deepEqual(
    result.sinks.map((sink) => [sink.id, sink.matches.map((match) => match.line)]),
    [
      ["demo:010", [17]],
      ["demo:011", [45]],
    ],
  );
  assert.deepEqual(
    result.extras.map((extra) => extra.line),
    [20],
  );
});
