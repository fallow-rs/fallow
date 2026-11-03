#!/usr/bin/env node
// Security corpus harness: measures `fallow security` against the public
// classified data of a public security-rules sink study. See
// docs/security-corpus.md for the method and the run instructions.
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath, pathToFileURL } from "node:url";

import { cargoFallowBin } from "./cargo-target.mjs";
import { runCliMain } from "./cli-main.mjs";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");

/** The environment variable that names the dataset repository as `owner/repo@sha`. */
export const DATASET_ENV = "FALLOW_SECURITY_CORPUS_DATASET";
const DATASET_DATA_DIR = "research/corpus/results/security-rules";
const DATASET_REPOS_FILE = "research/corpus/scripts/repos.json";

/**
 * The commits that the sink census of the dataset measured. The dataset repo
 * pins them in `research/corpus/scripts/clone.sh`; the harness keeps a copy
 * here because it reads that script as data and never runs it.
 */
export const MEASURED_COMMITS = {
  zod: "284243fb0d6532665afdc9239188773bb624f73c",
  hono: "f78a245abc393368583085e19c7f5b28b460bf51",
  trpc: "796700d8653b3832409fff997e6f5789519b9ea7",
  vite: "3b72997d42e7dd2b31898d0e68461e3025e836b6",
  astro: "9050e3819c51b0383cced278c278bc6f45c63468",
  excalidraw: "4c00f31ddcc20086bac1020428819fe0b9a30bce",
  "next.js": "d8bfc3c994670c367e6e31853b78f42e8b7fba85",
  vscode: "959031245ebb1fe077e0d512e1397c0ae82006e4",
};

const PHASES = new Set(["precision", "recall", "all"]);
const DEFAULT_OUT_DIR = ".plans/security-corpus";
const DEFAULT_SLACK = 3;
const DEFAULT_MAX_SCOPE_FILES = 4000;
const DEFAULT_TIMEOUT_SECS = 900;
const SPAWN_MAX_BUFFER = 1024 * 1024 * 1024;
const LICENSE_HEADER_LINES = 40;
const EXAMPLES_PER_CATEGORY = 8;

// The same source-file and exclusion rules as the census of the dataset
// (research/corpus/scripts/collect-files.cjs), so both sides count the same files.
const SOURCE_EXT = /\.(?:ts|tsx|mts|cts|js|jsx|mjs|cjs)$/;
const EXCLUDED_DIR_NAMES = new Set([
  "node_modules",
  ".git",
  "dist",
  "build-output",
  "__tests__",
  "__tests_dts__",
  "__test__",
  "__mocks__",
  "__fixtures__",
  "__snapshots__",
  "test",
  "tests",
  "fixtures",
  "fixture",
  "benchmarks",
  "bench",
  "examples",
  "example",
  "vendor",
  "vendored",
  "compiled",
  "bundles",
  "polyfills",
]);
const EXCLUDED_FILE_PATTERNS = [
  /\.d\.[cm]?ts$/,
  /\.(?:test|spec)\.[cm]?[jt]sx?$/,
  /\.(?:bench|benchmark)\.[cm]?[jt]sx?$/,
  /\.stories\.[cm]?[jt]sx?$/,
  /(?:^|[-_.])test-?utils?\.[cm]?[jt]sx?$/i,
];
const LICENSE_RE =
  /(?:MIT License|The MIT License|Copyright \(c\)|@copyright|Permission is hereby granted|Licensed under the Apache)/i;

const createDefaultOptions = () => ({
  phase: "all",
  cacheDir: join(tmpdir(), "fallow-security-corpus"),
  datasetDir: null,
  fallowBin: null,
  outDir: DEFAULT_OUT_DIR,
  limit: null,
  classes: null,
  repos: null,
  slack: DEFAULT_SLACK,
  maxScopeFiles: DEFAULT_MAX_SCOPE_FILES,
  timeoutSecs: DEFAULT_TIMEOUT_SECS,
  rerun: false,
});

const readValue = (argv, index, name) => {
  const value = argv[index + 1];
  if (!value) {
    throw new Error(`${name} requires a value`);
  }
  return value;
};

const intAtLeast = (name, value, min) => {
  const parsed = Number(value);
  if (!Number.isInteger(parsed) || parsed < min) {
    throw new Error(`${name} requires an integer of at least ${min}, got ${value}`);
  }
  return parsed;
};

const csv = (value) =>
  value
    .split(",")
    .map((entry) => entry.trim())
    .filter(Boolean);

const optionSetters = new Map([
  [
    "--phase",
    (options, value) => {
      if (!PHASES.has(value)) {
        throw new Error(`--phase must be one of ${[...PHASES].join(", ")}`);
      }
      return { ...options, phase: value };
    },
  ],
  ["--cache-dir", (options, value) => ({ ...options, cacheDir: value })],
  ["--dataset-dir", (options, value) => ({ ...options, datasetDir: value })],
  ["--fallow-bin", (options, value) => ({ ...options, fallowBin: value })],
  ["--out-dir", (options, value) => ({ ...options, outDir: value })],
  ["--limit", (options, value) => ({ ...options, limit: intAtLeast("--limit", value, 1) })],
  ["--class", (options, value) => ({ ...options, classes: new Set(csv(value)) })],
  ["--repos", (options, value) => ({ ...options, repos: new Set(csv(value)) })],
  ["--slack", (options, value) => ({ ...options, slack: intAtLeast("--slack", value, 0) })],
  [
    "--max-scope-files",
    (options, value) => ({ ...options, maxScopeFiles: intAtLeast("--max-scope-files", value, 1) }),
  ],
  [
    "--timeout-secs",
    (options, value) => ({ ...options, timeoutSecs: intAtLeast("--timeout-secs", value, 1) }),
  ],
]);

/** Parses the command-line flags of the harness. */
export const parseArgs = (argv) => {
  let options = createDefaultOptions();
  let index = 0;
  while (index < argv.length) {
    const arg = argv[index];
    if (arg === "--rerun") {
      options = { ...options, rerun: true };
      index += 1;
      continue;
    }
    const setter = optionSetters.get(arg);
    if (!setter) {
      throw new Error(`Unknown argument: ${arg}`);
    }
    options = setter(options, readValue(argv, index, arg));
    index += 2;
  }
  return options;
};

/** Splits a dataset `owner/repo@sha` fix reference. */
export const parseFixCommit = (value) => {
  const match = /^([\w.-]+\/[\w.-]+)@([0-9a-f]{7,40})$/.exec(value);
  return match ? { slug: match[1], sha: match[2] } : null;
};

const isSourcePath = (path) => {
  const segments = path.split("/");
  const name = segments.at(-1) ?? "";
  if (!SOURCE_EXT.test(name)) {
    return false;
  }
  if (segments.slice(0, -1).some((segment) => EXCLUDED_DIR_NAMES.has(segment))) {
    return false;
  }
  return !EXCLUDED_FILE_PATTERNS.some((pattern) => pattern.test(name));
};

const isUnder = (path, dir) => path === dir || path.startsWith(`${dir.replace(/\/$/, "")}/`);

/** Reports whether a repo-relative path is a file that the census of the dataset measured. */
export const isCensusPath = (path, repo) =>
  isSourcePath(path) &&
  repo.include.some((dir) => isUnder(path, dir)) &&
  !repo.exclude.some((dir) => isUnder(path, dir));

const HUNK_RE = /^@@ -(\d+)(?:,(\d+))? \+\d+(?:,\d+)? @@/;

const oldSideSourcePath = (line) => {
  const path = line.slice(4);
  return path.startsWith("a/") && isSourcePath(path.slice(2)) ? path.slice(2) : null;
};

const HUNK_FULL_RE = /^@@ -\d+(?:,(\d+))? \+\d+(?:,(\d+))? @@/;

const hunkLineCounts = (line) => {
  const match = HUNK_FULL_RE.exec(line);
  return {
    old: match[1] === undefined ? 1 : Number(match[1]),
    new: match[2] === undefined ? 1 : Number(match[2]),
  };
};

const consumeHunkLine = (remaining, line) => {
  if (line.startsWith("-")) {
    return { ...remaining, old: remaining.old - 1 };
  }
  if (line.startsWith("+")) {
    return { ...remaining, new: remaining.new - 1 };
  }
  if (line.startsWith(" ")) {
    return { old: remaining.old - 1, new: remaining.new - 1 };
  }
  return remaining;
};

const hunkOldRange = (hunk) => {
  const start = Number(hunk[1]);
  const count = hunk[2] === undefined ? 1 : Number(hunk[2]);
  return count === 0 ? { start, end: start + 1 } : { start, end: start + count - 1 };
};

/**
 * Reads a zero-context unified diff and returns, per changed source file, the
 * line ranges on the old (vulnerable) side. A pure insertion maps to the old
 * line before it and the line after it, because the defect is the missing code.
 */
export const changedRangesFromDiff = (diff) => {
  const ranges = new Map();
  let current = null;
  let remaining = { old: 0, new: 0 };
  for (const line of diff.split("\n")) {
    if (remaining.old > 0 || remaining.new > 0) {
      remaining = consumeHunkLine(remaining, line);
      continue;
    }
    if (line.startsWith("diff --git ")) {
      current = null;
      continue;
    }
    if (line.startsWith("--- ")) {
      current = oldSideSourcePath(line);
      continue;
    }
    const hunk = HUNK_RE.exec(line);
    if (hunk) {
      remaining = hunkLineCounts(line);
      if (current !== null) {
        ranges.set(current, [...(ranges.get(current) ?? []), hunkOldRange(hunk)]);
      }
    }
  }
  return ranges;
};

const findingInSpan = (finding, path, start, end, slack) =>
  finding.path === path && finding.line >= start - slack && finding.line <= end + slack;

const SCRIPT_ELEMENT_KIND = "script-element";
const SCRIPT_ELEMENT_EVIDENCE = "HTMLScriptElement.";
const SCRIPT_ELEMENT_WINDOW = 60;

/**
 * The dataset anchors a script element sink on its `createElement` line, but
 * fallow reports the later `src` or `text` assignment. A script element
 * candidate therefore joins the nearest script element site above it in the
 * same file, within a fixed window.
 */
const isScriptElementSink = (finding, site, sites) => {
  if (site.kind !== SCRIPT_ELEMENT_KIND || finding.path !== site.file) return false;
  if (!finding.evidence?.includes(SCRIPT_ELEMENT_EVIDENCE)) return false;
  if (finding.line < site.line || finding.line > site.line + SCRIPT_ELEMENT_WINDOW) return false;
  return !sites.some(
    (other) =>
      other.kind === SCRIPT_ELEMENT_KIND &&
      other.file === site.file &&
      other.line > site.line &&
      other.line <= finding.line,
  );
};

/**
 * Joins fallow candidates to the classified sink sites of one repository.
 * `sinks` holds the injection-group sites (the data-flow classification),
 * `otherSites` the remaining groups, and `extras` the in-scope candidates
 * that match no site at all.
 */
export const matchPrecision = ({ repo, sites, labels, findings, slack = 0, isExcludedFile }) => {
  const inScope = findings.filter(
    (finding) => isCensusPath(finding.path, repo) && !isExcludedFile?.(finding.path),
  );
  const matched = new Set();
  const joined = sites.map((site) => {
    const matches = inScope.filter(
      (finding) =>
        findingInSpan(finding, site.file, site.line, site.endLine ?? site.line, slack) ||
        isScriptElementSink(finding, site, sites),
    );
    for (const match of matches) {
      matched.add(match);
    }
    return {
      id: site.id,
      group: site.group,
      kind: site.kind,
      file: site.file,
      line: site.line,
      code: site.code,
      category: labels[site.id]?.category ?? "unlabelled",
      source: labels[site.id]?.source ?? null,
      matches,
    };
  });
  return {
    sinks: joined.filter((site) => site.group === "injection"),
    otherSites: joined.filter((site) => site.group !== "injection"),
    extras: inScope.filter((finding) => !matched.has(finding)),
    inScope: inScope.length,
    outOfScope: findings.length - inScope.length,
  };
};

/** Finds the candidates that land on a changed file and on a changed line range. */
export const recallHits = (findings, ranges, slack) => {
  const fileHits = findings.filter((finding) => ranges.has(finding.path));
  const lineHits = fileHits.filter((finding) =>
    ranges
      .get(finding.path)
      .some((range) => findingInSpan(finding, finding.path, range.start, range.end, slack)),
  );
  return { fileHits, lineHits };
};

/** Groups per-advisory recall outcomes by advisory class. */
export const summarizeRecall = (rows) => {
  const summary = new Map();
  for (const row of rows) {
    const entry = summary.get(row.class) ?? {
      advisories: 0,
      analyzed: 0,
      lineHit: 0,
      fileHit: 0,
      skipped: 0,
    };
    entry.advisories += 1;
    if (row.status === "analyzed") {
      entry.analyzed += 1;
      entry.lineHit += row.lineHit ? 1 : 0;
      entry.fileHit += row.fileHit ? 1 : 0;
    } else {
      entry.skipped += 1;
    }
    summary.set(row.class, entry);
  }
  return new Map([...summary.entries()].toSorted((a, b) => b[1].advisories - a[1].advisories));
};

// ---------------------------------------------------------------------------
// Process helpers

const run = (command, args, { cwd, timeoutSecs } = {}) => {
  const result = spawnSync(command, args, {
    cwd,
    encoding: "utf8",
    maxBuffer: SPAWN_MAX_BUFFER,
    timeout: timeoutSecs ? timeoutSecs * 1000 : undefined,
    env: { ...process.env, GIT_TERMINAL_PROMPT: "0" },
  });
  return {
    ok: result.status === 0,
    status: result.status,
    stdout: result.stdout ?? "",
    stderr: result.stderr ?? String(result.error ?? ""),
  };
};

const git = (dir, args, extra = {}) => run("git", ["-C", dir, ...args], extra);

const gitOrThrow = (dir, args) => {
  const result = git(dir, args);
  if (!result.ok) {
    throw new Error(`git ${args.join(" ")} failed in ${dir}: ${result.stderr.trim()}`);
  }
  return result.stdout;
};

const ensureRepo = (dir, slug) => {
  if (existsSync(join(dir, ".git"))) {
    return;
  }
  mkdirSync(dir, { recursive: true });
  gitOrThrow(dir, ["init", "-q"]);
  gitOrThrow(dir, ["remote", "add", "origin", `https://github.com/${slug}`]);
};

const hasCommit = (dir, sha) => git(dir, ["cat-file", "-e", `${sha}^{commit}`]).ok;

// Partial (blob-less) fetches keep every clone small; git downloads only the
// blobs that a diff or a sparse checkout reads.
const fetchCommit = (dir, sha, depth) => {
  if (hasCommit(dir, sha) && (depth === 1 || hasCommit(dir, `${sha}^`))) {
    return;
  }
  gitOrThrow(dir, ["fetch", "-q", `--depth=${depth}`, "--filter=blob:none", "origin", sha]);
};

/**
 * Returns the git arguments that limit the checkout to `dirs`. A cone pattern
 * of "." keeps only the root files, so a root scope disables the sparse checkout.
 */
export const sparseCheckoutArgs = (dirs) =>
  dirs.includes("")
    ? ["sparse-checkout", "disable"]
    : ["sparse-checkout", "set", "--cone", ...dirs];

const checkoutSparse = (dir, sha, dirs) => {
  gitOrThrow(dir, sparseCheckoutArgs(dirs));
  gitOrThrow(dir, ["checkout", "-q", "--force", "--detach", sha]);
};

// ---------------------------------------------------------------------------
// Fallow

/**
 * Keeps the finding fields that the report reads. A finding without a category
 * (for example `client-server-leak`) uses its kind. The function is idempotent,
 * so cached scans pass through it again on read.
 */
export const compactFinding = (finding) => ({
  finding_id: finding.finding_id,
  kind: finding.kind,
  category: finding.category ?? finding.kind,
  cwe: finding.cwe,
  path: finding.path,
  line: finding.line,
  col: finding.col,
  severity: finding.severity,
  source_backed: finding.source_backed,
  source_kind: finding.candidate?.source_kind ?? finding.source_kind ?? null,
  evidence: finding.evidence,
});

const runFallow = (fallowBin, cwd, timeoutSecs) => {
  const result = run(fallowBin, ["security", "--format", "json", "--surface", "--quiet"], {
    cwd,
    timeoutSecs,
  });
  if (result.status !== 0 && result.status !== 1) {
    return { ok: false, error: (result.stderr || `exit ${result.status}`).trim().slice(-2000) };
  }
  const report = JSON.parse(result.stdout);
  return {
    ok: true,
    findings: (report.security_findings ?? []).map(compactFinding),
    attackSurface: report.attack_surface?.length ?? 0,
    unresolvedEdgeFiles: report.unresolved_edge_files ?? 0,
    unresolvedCalleeSites: report.unresolved_callee_sites ?? 0,
  };
};

const readCache = (rawPath, key) => {
  if (!existsSync(rawPath)) {
    return null;
  }
  const cached = JSON.parse(readFileSync(rawPath, "utf8"));
  if (!cached.ok || JSON.stringify(cached.cacheKey ?? null) !== JSON.stringify(key)) {
    return null;
  }
  return { ...cached, findings: cached.findings.map(compactFinding) };
};

/**
 * Returns the cached scan at `rawPath` when its cache key equals `key`, or
 * runs `produce` and caches its result with the key. The key holds the fallow
 * binary hash, the commit and the scopes, so a rebuild or a scope change
 * scans again. A failed scan is not cached, so the next run tries it again.
 */
export const cachedFallow = (rawPath, rerun, produce, key = null) => {
  const cached = rerun ? null : readCache(rawPath, key);
  if (cached) {
    return cached;
  }
  const result = produce();
  if (result.ok) {
    mkdirSync(dirname(rawPath), { recursive: true });
    writeFileSync(rawPath, `${JSON.stringify({ ...result, cacheKey: key })}\n`);
  }
  return result;
};

const fileSha256 = (path) => createHash("sha256").update(readFileSync(path)).digest("hex");

// ---------------------------------------------------------------------------
// Dataset

/**
 * Reads the dataset repository from an `owner/repo@sha` value. The sha must be a
 * full commit id, so a run always reads the same data.
 */
export const parseDatasetSpec = (value) => {
  const match = /^([\w.-]+\/[\w.-]+)@([0-9a-f]{40})$/.exec(value ?? "");
  if (!match) {
    throw new Error(
      `Set ${DATASET_ENV} to owner/repo@<40-character sha>, or pass --dataset-dir <dir>`,
    );
  }
  return { slug: match[1], sha: match[2] };
};

const loadDataset = (options) => {
  let dir = options.datasetDir;
  if (!dir) {
    const { slug, sha } = parseDatasetSpec(process.env[DATASET_ENV]);
    dir = join(options.cacheDir, "dataset");
    ensureRepo(dir, slug);
    fetchCommit(dir, sha, 1);
    checkoutSparse(dir, sha, [DATASET_DATA_DIR, dirname(DATASET_REPOS_FILE)]);
  }
  const data = (name) => JSON.parse(readFileSync(join(dir, DATASET_DATA_DIR, name), "utf8"));
  const repos = JSON.parse(readFileSync(join(dir, DATASET_REPOS_FILE), "utf8")).repos;
  return {
    dir,
    repos: repos.filter((repo) => MEASURED_COMMITS[repo.name]),
    sample: data("sample.json"),
    classifications: data("classifications.json"),
    advisories: data("advisories.json"),
    advisoryLabels: data("advisory-classifications.json").labels,
  };
};

// ---------------------------------------------------------------------------
// Precision phase

const hasThirdPartyLicense = (absPath, ownCopyright) => {
  if (!existsSync(absPath)) {
    return false;
  }
  const head = readFileSync(absPath, "utf8").split("\n").slice(0, LICENSE_HEADER_LINES).join("\n");
  if (!LICENSE_RE.test(head)) {
    return false;
  }
  return !(ownCopyright && new RegExp(ownCopyright).test(head));
};

const runPrecisionRepo = (repo, dataset, options, fallowBin) => {
  const sha = MEASURED_COMMITS[repo.name];
  const dir = join(options.cacheDir, "precision", repo.name);
  ensureRepo(dir, repo.github);
  fetchCommit(dir, sha, 1);
  checkoutSparse(dir, sha, repo.include);
  const started = Date.now();
  const scan = cachedFallow(
    join(options.outDir, "raw", `precision-${repo.name}.json`),
    options.rerun,
    () => runFallow(fallowBin, dir, options.timeoutSecs),
    { fallow: options.fallowHash, commit: sha, scopes: repo.include },
  );
  const elapsedMs = Date.now() - started;
  if (!scan.ok) {
    return { repo: repo.name, sha, error: scan.error };
  }
  const sites = dataset.sample.sites.filter((site) => site.repo === repo.name);
  const match = matchPrecision({
    repo,
    sites,
    labels: dataset.classifications.labels,
    findings: scan.findings,
    slack: 1,
    isExcludedFile: (path) => hasThirdPartyLicense(join(dir, path), repo.ownCopyright),
  });
  return {
    repo: repo.name,
    sha,
    elapsedMs,
    findings: scan.findings.length,
    unresolvedCalleeSites: scan.unresolvedCalleeSites,
    ...match,
  };
};

/** Runs one precision repo and turns a thrown error into an error entry, so the run continues. */
export const guardPrecisionRepo = (repo, runRepo) => {
  try {
    return runRepo(repo);
  } catch (error) {
    return {
      repo: repo.name,
      sha: MEASURED_COMMITS[repo.name] ?? "unknown",
      error: String(error.message ?? error),
    };
  }
};

// ---------------------------------------------------------------------------
// Recall phase

const historyDir = (options, slug) => join(options.cacheDir, "history", slug.replace("/", "__"));

const packageRoots = (files) => {
  const roots = new Set();
  for (const file of files) {
    if (file === "package.json" || file.endsWith("/package.json")) {
      roots.add(file === "package.json" ? "" : file.slice(0, -"/package.json".length));
    }
  }
  return roots;
};

const ancestors = (path) => {
  const parts = path.split("/").slice(0, -1);
  return parts.map((_, index) => parts.slice(0, index + 1).join("/"));
};

/**
 * Picks the checkout scope for one changed file: the nearest package root,
 * narrowed toward the file while the scope holds more than `maxFiles`
 * source files. This keeps monorepo roots such as VS Code affordable.
 */
export const scopeForFile = (file, treeFiles, roots, maxFiles) => {
  const chain = ["", ...ancestors(file)];
  let index = 0;
  for (let i = chain.length - 1; i >= 0; i -= 1) {
    if (roots.has(chain[i])) {
      index = i;
      break;
    }
  }
  const count = (dir) =>
    treeFiles.filter((path) => (dir === "" || isUnder(path, dir)) && SOURCE_EXT.test(path)).length;
  while (index < chain.length - 1 && count(chain[index]) > maxFiles) {
    index += 1;
  }
  return chain[index];
};

const collapseScopes = (scopes) => {
  const sorted = [...new Set(scopes)].toSorted((a, b) => a.length - b.length);
  const kept = [];
  for (const scope of sorted) {
    if (!kept.some((dir) => dir === "" || isUnder(scope, dir))) {
      kept.push(scope);
    }
  }
  return kept;
};

const findingKey = (finding) => `${finding.category}\u0000${finding.path}\u0000${finding.evidence}`;

/**
 * Returns the finding ids of the line hits that the fix scan still reports.
 * Returns null when the fix scan failed, because persistence is then unknown.
 */
export const persistingLineHits = (lineHits, fixScan) => {
  if (!fixScan.ok) {
    return null;
  }
  const after = new Set(fixScan.findings.map(findingKey));
  return lineHits.filter((finding) => after.has(findingKey(finding))).map((f) => f.finding_id);
};

const scanFixCommit = (dir, commit, scopes, options, fallowBin) => {
  try {
    return cachedFallow(
      join(options.outDir, "raw", `fix-${commit.sha}.json`),
      options.rerun,
      () => {
        checkoutSparse(dir, commit.sha, scopes);
        return runFallow(fallowBin, dir, options.timeoutSecs);
      },
      { fallow: options.fallowHash, commit: commit.sha, scopes },
    );
  } catch (error) {
    return { ok: false, error: String(error.message ?? error) };
  }
};

const runRecallCommit = (commit, options, fallowBin) => {
  const dir = historyDir(options, commit.slug);
  ensureRepo(dir, commit.slug);
  fetchCommit(dir, commit.sha, 2);
  const parentResult = git(dir, ["rev-parse", `${commit.sha}^`]);
  if (!parentResult.ok) {
    return { status: "no-parent" };
  }
  const parent = parentResult.stdout.trim();
  const changed = gitOrThrow(dir, ["diff", "--name-only", "--no-renames", parent, commit.sha])
    .split("\n")
    .filter((path) => path && isSourcePath(path));
  if (changed.length === 0) {
    return { status: "no-source-changes", parent };
  }
  const ranges = changedRangesFromDiff(
    gitOrThrow(dir, [
      "diff",
      "-U0",
      "--no-color",
      "--no-renames",
      parent,
      commit.sha,
      "--",
      ...changed,
    ]),
  );
  if (ranges.size === 0) {
    return { status: "only-new-files", parent };
  }
  const treeFiles = gitOrThrow(dir, ["ls-tree", "-r", "--name-only", parent])
    .split("\n")
    .filter(Boolean);
  const roots = packageRoots(treeFiles);
  const scopes = collapseScopes(
    [...ranges.keys()].map((file) => scopeForFile(file, treeFiles, roots, options.maxScopeFiles)),
  );
  const scan = cachedFallow(
    join(options.outDir, "raw", `recall-${commit.sha}.json`),
    options.rerun,
    () => {
      checkoutSparse(dir, parent, scopes);
      return runFallow(fallowBin, dir, options.timeoutSecs);
    },
    { fallow: options.fallowHash, commit: parent, scopes },
  );
  if (!scan.ok) {
    return { status: "fallow-error", parent, scopes, error: scan.error };
  }
  const hits = recallHits(scan.findings, ranges, options.slack);
  const fixScan =
    hits.lineHits.length > 0
      ? scanFixCommit(dir, commit, scopes, options, fallowBin)
      : { ok: true, findings: [] };
  return {
    status: "analyzed",
    parent,
    scopes,
    changedFiles: [...ranges.keys()],
    findings: scan.findings.length,
    fileHits: hits.fileHits,
    lineHits: hits.lineHits,
    persistingAfterFix: persistingLineHits(hits.lineHits, fixScan),
    fixError: fixScan.ok ? undefined : fixScan.error,
  };
};

const dropReason = (base, options, selectedCount) => {
  if (options.classes && !options.classes.has(base.class)) {
    return "class filter";
  }
  if (options.repos && !options.repos.has(base.project)) {
    return "repo filter";
  }
  if (options.limit !== null && selectedCount >= options.limit) {
    return "limit";
  }
  return null;
};

/** Splits the advisories with a fix commit into the selected set and the dropped set. */
export const selectAdvisories = (dataset, options) => {
  const dropped = [];
  const selected = [];
  const withFix = dataset.advisories.advisories
    .map((advisory) => ({
      advisory,
      commits: (advisory.fixCommits ?? []).map(parseFixCommit).filter(Boolean),
    }))
    .filter(({ commits }) => commits.length > 0);
  for (const { advisory, commits } of withFix) {
    const label = dataset.advisoryLabels[advisory.ghsa] ?? {};
    const base = {
      ghsa: advisory.ghsa,
      project: advisory.project,
      class: label.class ?? "unlabelled",
    };
    const reason = dropReason(base, options, selected.length);
    if (reason) {
      dropped.push({ ...base, reason });
    } else {
      selected.push({ ...base, lint: label.lint ?? null, summary: advisory.summary, commits });
    }
  }
  return {
    selected,
    dropped,
    withoutFix: dataset.advisories.advisories.length - withFix.length,
  };
};

const markPersistence = (result) =>
  result.lineHits.map((finding) => ({
    ...finding,
    persistsAfterFix:
      result.persistingAfterFix === null
        ? null
        : result.persistingAfterFix.includes(finding.finding_id),
  }));

/** Joins the commit results of one advisory into one report row. */
export const aggregateAdvisory = (advisory, commitResults) => {
  const results = advisory.commits.map((commit) => ({
    ...commit,
    ...commitResults.get(commit.sha),
  }));
  const analyzed = results.filter((result) => result.status === "analyzed");
  const lineHits = analyzed.flatMap(markPersistence);
  return {
    ghsa: advisory.ghsa,
    project: advisory.project,
    class: advisory.class,
    lint: advisory.lint,
    summary: advisory.summary,
    status: analyzed.length > 0 ? "analyzed" : (results[0]?.status ?? "unknown"),
    commits: results.map((result) => ({
      slug: result.slug,
      sha: result.sha,
      status: result.status,
      scopes: result.scopes,
      error: result.error,
      fixError: result.fixError,
    })),
    fileHit: analyzed.some((result) => result.fileHits.length > 0),
    lineHit: lineHits.length > 0,
    lineHits,
    fileHitCount: analyzed.reduce((sum, result) => sum + result.fileHits.length, 0),
  };
};

const runRecall = (dataset, options, fallowBin) => {
  const { selected, dropped, withoutFix } = selectAdvisories(dataset, options);
  const commitResults = new Map();
  const unique = [...new Map(selected.flatMap((a) => a.commits).map((c) => [c.sha, c])).values()];
  unique.forEach((commit, index) => {
    console.error(
      `[recall ${index + 1}/${unique.length}] ${commit.slug}@${commit.sha.slice(0, 10)}`,
    );
    try {
      commitResults.set(commit.sha, runRecallCommit(commit, options, fallowBin));
    } catch (error) {
      commitResults.set(commit.sha, {
        status: "harness-error",
        error: String(error.message ?? error),
      });
    }
  });
  const rows = selected.map((advisory) => aggregateAdvisory(advisory, commitResults));
  return { rows, dropped, withoutFix, commits: unique.length };
};

// ---------------------------------------------------------------------------
// Report

const table = (headers, rows) =>
  [
    `| ${headers.join(" | ")} |`,
    `| ${headers.map(() => "---").join(" | ")} |`,
    ...rows.map(
      (row) =>
        `| ${row.map((cell) => String(cell).replaceAll("|", "\\|").replaceAll("\n", " ")).join(" | ")} |`,
    ),
  ].join("\n");

/** Renders the "still present after fix" cell. A line hit whose fix scan failed counts as unknown. */
export const persistenceCell = (lineHits) => {
  const present = lineHits.filter((finding) => finding.persistsAfterFix === true).length;
  const unknown = lineHits.filter((finding) => finding.persistsAfterFix === null).length;
  const cell = `${present}/${lineHits.length}`;
  return unknown > 0 ? `${cell} (${unknown} unknown: fix scan failed)` : cell;
};

const loc = (finding) => `\`${finding.path}:${finding.line}\``;

const countBy = (items, key) => {
  const counts = new Map();
  for (const item of items) {
    counts.set(key(item), (counts.get(key(item)) ?? 0) + 1);
  }
  return [...counts.entries()].toSorted((a, b) => b[1] - a[1]);
};

const renderPrecision = (precision) => {
  const out = ["## Precision run", ""];
  const ok = precision.filter((entry) => !entry.error);
  out.push(
    table(
      ["Repo", "Commit", "Findings", "In census scope", "Unresolved callee sites", "Seconds"],
      precision.map((entry) =>
        entry.error
          ? [entry.repo, entry.sha.slice(0, 10), "error", entry.error.slice(0, 120), "", ""]
          : [
              entry.repo,
              entry.sha.slice(0, 10),
              entry.findings,
              entry.inScope,
              entry.unresolvedCalleeSites,
              (entry.elapsedMs / 1000).toFixed(1),
            ],
      ),
    ),
    "",
  );
  const sinks = ok.flatMap((entry) => entry.sinks.map((sink) => ({ ...sink, repo: entry.repo })));
  out.push("### Classified injection sinks", "");
  out.push(
    table(
      ["Data-flow category", "Sinks", "Matched by fallow", "Matched categories"],
      countBy(sinks, (sink) => sink.category).map(([category, total]) => {
        const inCategory = sinks.filter((sink) => sink.category === category);
        const matched = inCategory.filter((sink) => sink.matches.length > 0);
        const categories = countBy(
          matched.flatMap((sink) => sink.matches),
          (finding) => finding.category,
        )
          .map(([name, n]) => `${name} (${n})`)
          .join(", ");
        return [category, total, matched.length, categories || "none"];
      }),
    ),
    "",
  );
  out.push("### Sinks by kind", "");
  out.push(
    table(
      ["Dataset sink kind", "Sinks", "Matched by fallow"],
      countBy(sinks, (sink) => sink.kind).map(([kind, total]) => [
        kind,
        total,
        sinks.filter((sink) => sink.kind === kind && sink.matches.length > 0).length,
      ]),
    ),
    "",
  );
  out.push("### Every injection sink", "");
  out.push(
    table(
      ["Id", "Kind", "Category", "Location", "Fallow match"],
      sinks.map((sink) => [
        sink.id,
        sink.kind,
        sink.category,
        `\`${sink.file}:${sink.line}\``,
        sink.matches
          .map(
            (finding) =>
              `${finding.category} ${finding.severity}${finding.source_kind ? ` (${finding.source_kind})` : ""}`,
          )
          .join("; ") || "none",
      ]),
    ),
    "",
  );
  const other = ok.flatMap((entry) => entry.otherSites);
  out.push("### Other dataset site groups", "");
  out.push(
    table(
      ["Group", "Category", "Sites", "Matched by fallow"],
      countBy(other, (site) => `${site.group}\u0000${site.category}`).map(([key, total]) => {
        const [group, category] = key.split("\u0000");
        const matched = other.filter(
          (site) => site.group === group && site.category === category && site.matches.length > 0,
        );
        return [group, category, total, matched.length];
      }),
    ),
    "",
  );
  const extras = ok.flatMap((entry) =>
    entry.extras.map((finding) => ({ ...finding, repo: entry.repo })),
  );
  out.push("### Fallow candidates outside the dataset sink list", "");
  out.push(
    table(
      ["Category", "Candidates", "Repos", "Examples"],
      countBy(extras, (finding) => finding.category).map(([category, total]) => {
        const items = extras.filter((finding) => finding.category === category);
        const repos = countBy(items, (finding) => finding.repo)
          .map(([name, n]) => `${name} ${n}`)
          .join(", ");
        const examples = items
          .slice(0, EXAMPLES_PER_CATEGORY)
          .map((finding) => `${finding.repo} ${loc(finding)}`)
          .join("<br>");
        return [category, total, repos, examples];
      }),
    ),
    "",
  );
  return out.join("\n");
};

const renderRecall = (recall) => {
  const out = ["## Recall run", ""];
  out.push(
    `Advisories with a fix commit selected: ${recall.rows.length}. Unique fix commits: ${recall.commits}. ` +
      `Advisories without a fix commit: ${recall.withoutFix}. Dropped by filters: ${recall.dropped.length}.`,
    "",
  );
  const summary = summarizeRecall(recall.rows);
  out.push(
    table(
      ["Class", "Advisories", "Analyzed", "Line hit", "File hit", "Skipped"],
      [...summary.entries()].map(([name, entry]) => [
        name,
        entry.advisories,
        entry.analyzed,
        entry.lineHit,
        entry.fileHit,
        entry.skipped,
      ]),
    ),
    "",
  );
  const statuses = countBy(
    recall.rows.flatMap((row) => row.commits),
    (commit) => commit.status,
  );
  out.push("Commit outcomes: " + statuses.map(([status, n]) => `${status} ${n}`).join(", "), "");
  out.push("### Line hits", "");
  out.push(
    table(
      [
        "Advisory",
        "Project",
        "Class",
        "Dataset lint label",
        "Candidates on changed lines",
        "Still present after fix",
      ],
      recall.rows
        .filter((row) => row.lineHit)
        .map((row) => [
          row.ghsa,
          row.project,
          row.class,
          row.lint,
          row.lineHits.map((finding) => `${finding.category} ${loc(finding)}`).join("<br>"),
          persistenceCell(row.lineHits),
        ]),
    ),
    "",
  );
  out.push("### Every advisory", "");
  out.push(
    table(
      ["Advisory", "Project", "Class", "Status", "File hits", "Line hit", "Scopes", "Summary"],
      recall.rows.map((row) => [
        row.ghsa,
        row.project,
        row.class,
        row.status,
        row.fileHitCount,
        row.lineHit ? "yes" : "no",
        [...new Set(row.commits.flatMap((commit) => commit.scopes ?? []))].join(", ") || "-",
        row.summary,
      ]),
    ),
    "",
  );
  const errors = recall.rows.flatMap((row) =>
    row.commits.flatMap((commit) =>
      [
        commit.error ? [commit.status, commit.error] : null,
        commit.fixError ? ["fix-scan-error", commit.fixError] : null,
      ]
        .filter(Boolean)
        .map(([status, error]) => [row.ghsa, commit.sha.slice(0, 10), status, error.slice(0, 200)]),
    ),
  );
  if (errors.length > 0) {
    out.push("### Errors", "", table(["Advisory", "Commit", "Status", "Error"], errors), "");
  }
  if (recall.dropped.length > 0) {
    out.push(
      "### Dropped advisories",
      "",
      table(
        ["Advisory", "Project", "Class", "Reason"],
        recall.dropped.map((row) => [row.ghsa, row.project, row.class, row.reason]),
      ),
      "",
    );
  }
  return out.join("\n");
};

const renderReport = ({ meta, precision, recall }) =>
  [
    "# Security corpus report",
    "",
    table(
      ["Field", "Value"],
      Object.entries(meta).map(([key, value]) => [
        key,
        typeof value === "string" ? value : JSON.stringify(value),
      ]),
    ),
    "",
    precision ? renderPrecision(precision) : "",
    recall ? renderRecall(recall) : "",
  ].join("\n");

// ---------------------------------------------------------------------------
// Entry point

/** Runs the selected phases and writes `results.json` and `report.md` into the output directory. */
export const runSecurityCorpus = (options) => {
  const fallowBin = resolve(options.fallowBin ?? cargoFallowBin(REPO_ROOT, "release"));
  if (!existsSync(fallowBin)) {
    throw new Error(
      `fallow binary not found at ${fallowBin}; run cargo build --release -p fallow-cli`,
    );
  }
  mkdirSync(options.outDir, { recursive: true });
  options = { ...options, fallowHash: fileSha256(fallowBin) };
  const dataset = loadDataset(options);
  const meta = {
    generated: new Date().toISOString(),
    fallow: run(fallowBin, ["--version"]).stdout.trim(),
    fallowSha256: options.fallowHash,
    fallowCommit: git(REPO_ROOT, ["rev-parse", "HEAD"]).stdout.trim(),
    dataset: options.datasetDir ?? process.env[DATASET_ENV],
    commits: MEASURED_COMMITS,
    slack: options.slack,
    maxScopeFiles: options.maxScopeFiles,
    limit: options.limit,
    classes: options.classes ? [...options.classes] : null,
    repos: options.repos ? [...options.repos] : null,
  };
  const repos = dataset.repos.filter((repo) => !options.repos || options.repos.has(repo.name));
  const precision =
    options.phase === "recall"
      ? null
      : repos.map((repo) => {
          console.error(`[precision] ${repo.name}`);
          return guardPrecisionRepo(repo, (entry) =>
            runPrecisionRepo(entry, dataset, options, fallowBin),
          );
        });
  const recall = options.phase === "precision" ? null : runRecall(dataset, options, fallowBin);
  writeFileSync(
    join(options.outDir, "results.json"),
    `${JSON.stringify({ meta, precision, recall }, null, 2)}\n`,
  );
  writeFileSync(join(options.outDir, "report.md"), renderReport({ meta, precision, recall }));
  console.error(`wrote ${join(options.outDir, "report.md")}`);
  return 0;
};

export const main = (argv = process.argv.slice(2)) => runSecurityCorpus(parseArgs(argv));

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  runCliMain(main);
}
