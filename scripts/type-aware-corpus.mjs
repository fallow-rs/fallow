#!/usr/bin/env node

import { createHash } from "node:crypto";
import {
  existsSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  renameSync,
  rmSync,
  statSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { spawn, spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { basename, dirname, isAbsolute, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import {
  checkerEvidenceRequest as createCheckerEvidenceRequest,
  digestSet,
  normalizedEvidenceResponseDigest,
  requestDigest,
  runExactSidecarRequest as executeExactSidecarRequest,
} from "./type-aware-corpus/runner.mjs";
import {
  aggregatePhaseTimings,
  aggregateReasonCounts,
  percentile,
  pairedOverheads,
  renderSummaryMarkdown,
  requirePublicationGo as assertPublicationGo,
  safeRatio,
  summaryChecks,
  summarizeAdjudicatedFeatureBuckets,
} from "./type-aware-corpus/summary.mjs";
import {
  validateCapabilitiesArtifactData as validateCapabilitiesData,
  validateMeasurements as validateMeasurementData,
  validateSupplementalArtifactData as validateSupplementalData,
} from "./type-aware-corpus/validation.mjs";
import {
  candidateFeatureBucketFields as ledgerFeatureBucketFields,
  evidenceLocationValid as validEvidenceLocation,
  evidenceProducerErrors as validateEvidenceProducer,
  independentReviewErrors as validateIndependentReviews,
  indexLedgerForRefresh as indexRefreshLedger,
  verifyLedgerData as verifyLedgerArtifact,
} from "./type-aware-corpus/ledger.mjs";
import {
  parseArgs as parseCorpusArgs,
  validateManifest as validateCorpusManifest,
} from "./type-aware-corpus/config.mjs";

export { summarizeAdjudicatedFeatureBuckets };

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(SCRIPT_DIR, "..");
const DEFAULT_MANIFEST = resolve(REPO_ROOT, "benchmarks/type-aware-corpus.json");
const DEFAULT_ADJUDICATION = resolve(REPO_ROOT, "benchmarks/type-aware-adjudication.json");
const DEFAULT_OUT_DIR = resolve(REPO_ROOT, "target/type-aware-corpus");
const DEFAULT_FALLOW_BIN = resolve(REPO_ROOT, "target/release/fallow");
const SUPPLEMENTAL_VITEST_ROOT = resolve(DEFAULT_OUT_DIR, "supplemental/vitest");
const SUPPLEMENTAL_VITEST_REPO = "https://github.com/vitest-dev/vitest.git";
const SUPPLEMENTAL_VITEST_COMMIT = "8fbfcb054a07410e03bd37e6682d15ef5b240ad8";
const SUPPLEMENTAL_ARTIFACT = resolve(REPO_ROOT, "benchmarks/type-aware-supplemental-smoke.json");
const CAPABILITIES_ARTIFACT = resolve(
  REPO_ROOT,
  "benchmarks/type-aware-semantic-capabilities.json",
);
const DEFAULT_RUNS = 4;
const DEFAULT_WARMUPS = 1;
const DEFAULT_DISCOVERY_RUNS = 2;
const MAX_OUTPUT_BYTES = 256 * 1024 * 1024;
const MAX_SEMANTIC_RESPONSE_BYTES = 32 * 1024 * 1024;
const MAX_SEMANTIC_STDERR_BYTES = 16 * 1024;
const PROCESS_TREE_RSS_SAMPLE_INTERVAL_MS = 100;
const SEMANTIC_TIMEOUT_MS = 120_000;
const SEMANTIC_PROTOCOL_VERSION = 5;
const SEMANTIC_QUERY_STATUSES = new Set(["complete", "partial", "unavailable"]);
const TRUTH_STATUSES = new Set(["used", "preserved", "unused", "indeterminate"]);
const DEPENDENCY_DIRECTORY_NAMES = new Set(["node_modules", ".pnpm-store"]);
const FOCUSED_TEST_PATH = resolve(REPO_ROOT, "tools/type-aware-sidecar/test/sidecar.test.mjs");
const REQUIRED_FOCUSED_CASES = [
  "confirms a nested generic class-member use",
  "confirms a generic class member through mapped type and indexed access",
  "retains Angular template-only members without claiming a checker use",
  "retains Astro template-only members without claiming a checker use",
  "retains Vue template-only members without claiming a checker use",
  "finds a class member used only from an explicitly opened consumer project",
  "full CLI safely abstains for an explicit solution tsconfig",
  "protocol v5 discovers public entry points from a nested package project",
  "protocol v5 includes a direct test consumer in targeted tests",
  "protocol v5 finds semantic consumers and tests across selected projects",
  "protocol v5 confirms complete closed-world absence of static class-member references",
  "protocol v5 preserves required interface, abstract, and inherited contracts",
  "protocol v5 abstains for optional contracts, decorators, and dynamic member access",
  "treats getter and setter declarations as one logical property",
];
const REQUIRED_SEMANTIC_CAPABILITIES = [
  "dead-code-refinement",
  "semantic-symbol-trace",
  "public-api-surface",
  "semantic-impact-targeted-tests",
  "public-type-coupling",
];
const REQUIRED_PROJECTS = new Map([
  ["svelte", "accuracy-core"],
  ["vue-core", "accuracy-core"],
  ["astro", "accuracy-core"],
  ["typescript", "accuracy-core"],
  ["next.js", "accuracy-core"],
  ["preact", "accuracy-core"],
  ["query", "zero-control"],
  ["vite", "zero-control"],
  ["fastify", "zero-control"],
  ["zod", "zero-control"],
]);

const fail = (message) => {
  throw new Error(message);
};

const describeError = (error) => (error instanceof Error ? error.message : String(error));

const isObject = (value) => value !== null && typeof value === "object" && !Array.isArray(value);

const nonEmptyString = (value, field) => {
  if (typeof value !== "string" || value.trim() === "") {
    fail(`${field} must be a non-empty string`);
  }
};

const normalizedRelativePath = (value, field) => {
  nonEmptyString(value, field);
  const normalized = value.replaceAll("\\", "/").replace(/^\.\//, "");
  if (isAbsolute(value) || normalized === ".." || normalized.startsWith("../")) {
    fail(`${field} must be a project-relative path`);
  }
  return normalized;
};

export const validateManifest = (manifest) => {
  const requiredGates = [
    "minimum_confirmation_precision",
    "minimum_confirmation_yield",
    "minimum_correct_unused_retention",
    "maximum_abstention",
    "maximum_p95_marginal_overhead_ms",
    "maximum_p95_refined_rss_kb",
  ];
  return validateCorpusManifest(manifest, {
    fail,
    isObject,
    nonEmptyString,
    normalizedRelativePath,
    requiredGates,
    requiredProjects: REQUIRED_PROJECTS,
  });
};

const readJson = (path, description = path) => {
  try {
    return JSON.parse(readFileSync(path, "utf8"));
  } catch (error) {
    fail(
      `failed to read ${description}: ${error instanceof Error ? error.message : String(error)}`,
    );
  }
};

const writeJson = (path, value) => {
  mkdirSync(dirname(path), { recursive: true });
  const temporaryPath = `${path}.${process.pid}.tmp`;
  writeFileSync(temporaryPath, `${JSON.stringify(value, null, 2)}\n`);
  renameSync(temporaryPath, path);
};

const loadManifest = (path) => validateManifest(readJson(path, "corpus manifest"));

const safeArtifactName = (value) => value.replaceAll(/[^A-Za-z0-9_.-]/g, "_");

const parsePositiveInteger = (value, name) => {
  const parsed = Number.parseInt(value, 10);
  if (!Number.isSafeInteger(parsed) || parsed <= 0) fail(`${name} must be a positive integer`);
  return parsed;
};

const readOptionValue = (argv, index, option) => {
  const value = argv[index + 1];
  if (!value) fail(`${option} requires a value`);
  return value;
};

export const parseArgs = (argv) => {
  const commands = new Set([
    "prepare",
    "discover",
    "measure",
    "focused",
    "evidence",
    "adjudicate",
    "verify-ledger",
    "summarize",
    "verify-publication",
    "supplemental",
    "capabilities",
  ]);
  return parseCorpusArgs(argv, {
    commands,
    defaults: () => ({
      discoveryRuns: DEFAULT_DISCOVERY_RUNS,
      fallowBin: DEFAULT_FALLOW_BIN,
      manifest: DEFAULT_MANIFEST,
      outDir: DEFAULT_OUT_DIR,
      runs: DEFAULT_RUNS,
      sidecarBin: null,
      warmups: DEFAULT_WARMUPS,
    }),
    fail,
    helpCommands: new Set([undefined, "--help", "-h"]),
    parsePositiveInteger,
    readOptionValue,
    resolve,
  });
};

const usage = () => `Usage: node scripts/type-aware-corpus.mjs <command> [options]

Commands:
  prepare        Materialize clean worktrees at the pinned public refs
  discover       Run baseline and refined discovery for the pinned corpus
  measure        Warm up, then alternate baseline/refined release runs
  focused        Run the candidate-bearing semantic edge-case suite
  evidence       Create or refresh the manual evidence ledger template
  adjudicate     Apply the checked-in review decisions to the evidence ledger
  verify-ledger  Require truth status and source evidence for every candidate
  summarize      Verify the ledger and write compact gate metrics
  verify-publication  Check tracked evidence and summary for generator drift
  supplemental   Regenerate and verify the clean public Vitest smoke
  capabilities   Prove all five semantic capabilities on Astro and Vitest

Options:
  --manifest PATH     Default: benchmarks/type-aware-corpus.json
  --project ID        Select one project, repeatable
  --fallow-bin PATH   Default: target/release/fallow
  --sidecar-bin PATH  Required for discover, measure, focused, evidence, summarize, verify-publication, and supplemental
  --out-dir PATH      Isolated artifact directory, required with --project
  --discovery-runs N  Repeated normalized discovery runs, default: ${DEFAULT_DISCOVERY_RUNS}
  --runs N            Measured pairs per project, default: ${DEFAULT_RUNS}
  --warmups N         Warmup pairs per project, default: ${DEFAULT_WARMUPS}

Canonical artifacts are written below target/type-aware-corpus. Corpus projects are read-only.`;

const selectProjects = (manifest, selectedIds) => {
  if (selectedIds.length === 0) return manifest.projects;
  const selected = new Set(selectedIds);
  for (const id of selected) {
    if (!manifest.projects.some((project) => project.id === id)) fail(`unknown project: ${id}`);
  }
  return manifest.projects.filter((project) => selected.has(project.id));
};

export const validatePartialOutput = (options) => {
  const partialCanonicalRun = [
    options.projects.length > 0,
    new Set(["prepare", "discover", "measure"]).has(options.command),
    [!options.outDirExplicit, resolve(options.outDir) === DEFAULT_OUT_DIR].some(Boolean),
  ].every(Boolean);
  if (partialCanonicalRun) {
    fail("partial corpus runs require a non-canonical --out-dir");
  }
};

const ensureFile = (path, description) => {
  if (!existsSync(path) || !statSync(path).isFile()) fail(`${description} not found: ${path}`);
};

const git = (cwd, args, description) => {
  const result = spawnSync("git", args, {
    cwd,
    encoding: "utf8",
    maxBuffer: 8 * 1024 * 1024,
  });
  if (result.status !== 0) {
    const detail = result.stderr.trim() || result.stdout.trim();
    fail(`${description} failed: ${detail.slice(0, 2_000)}`);
  }
  return result.stdout.trim();
};

const normalizeDependencyDirectoryPath = (entry) => entry.replaceAll("\\", "/").replace(/\/$/, "");

const containsDependencyDirectory = (entry) =>
  entry.split("/").some((component) => DEPENDENCY_DIRECTORY_NAMES.has(component));

export const assertNoFixtureDependencyDirectories = (root, projectId, ignoredPaths = null) => {
  const ignored =
    ignoredPaths ??
    git(
      root,
      ["ls-files", "--others", "--ignored", "--exclude-standard", "--directory"],
      `${projectId} ignored dependency directory scan`,
    ).split("\n");
  const dependencyDirectories = ignored
    .map(normalizeDependencyDirectoryPath)
    .filter(containsDependencyDirectory);
  const directDependencyDirectories = [...DEPENDENCY_DIRECTORY_NAMES].filter((name) =>
    existsSync(resolve(root, name)),
  );
  const unique = [
    ...new Set([...dependencyDirectories, ...directDependencyDirectories]),
  ].toSorted();
  if (unique.length > 0) {
    fail(`${projectId} prepared fixture contains dependency directories: ${unique.join(", ")}`);
  }
};

const ancestorDirectories = (root) => {
  const directories = [];
  let ancestor = dirname(resolve(root));
  while (true) {
    directories.push(ancestor);
    const parent = dirname(ancestor);
    if (parent === ancestor) return directories;
    ancestor = parent;
  }
};

const containsInstalledDependencies = (root) =>
  [...DEPENDENCY_DIRECTORY_NAMES].some((name) => existsSync(resolve(root, name)));

const existingDirectory = (path) => (existsSync(path) ? statSync(path).isDirectory() : false);
const existingFile = (path) => (existsSync(path) ? statSync(path).isFile() : false);

export const fixtureDependencyEnvironment = (
  root,
  projectId,
  approvedDependencyRoot = REPO_ROOT,
) => {
  const dependencyRoots = ancestorDirectories(root).filter(containsInstalledDependencies);
  if (dependencyRoots.length === 0) {
    return { dependency_installation: "none", dependency_root: null };
  }
  const approved = resolve(approvedDependencyRoot);
  if (dependencyRoots.some((dependencyRoot) => dependencyRoot !== approved)) {
    fail(
      `${projectId} resolves dependencies from an unapproved ancestor: ${dependencyRoots.join(", ")}`,
    );
  }
  return {
    dependency_installation: "ancestor-workspace",
    dependency_root: approved,
  };
};

const sourceFixtureRoot = (project) => {
  const root = resolve(REPO_ROOT, project.fixture);
  const relativeToRepo = relative(REPO_ROOT, root);
  if ([relativeToRepo.startsWith(`..${sep}`), relativeToRepo === ".."].some(Boolean)) {
    fail(`${project.id} fixture resolves outside the repository`);
  }
  if (!existingDirectory(root)) {
    fail(`${project.id} fixture is missing: ${project.fixture}`);
  }
  return root;
};

const preparedFixtureRoot = (project, outDir) => resolve(outDir, "fixtures", project.id);

const fixtureCommit = (project) =>
  git(
    sourceFixtureRoot(project),
    ["rev-parse", "--verify", `${project.ref}^{commit}`],
    `${project.id} pinned ref ${project.ref}`,
  );

const validatePreparedFixture = (project, outDir) => {
  const root = preparedFixtureRoot(project, outDir);
  if (!existingDirectory(root)) {
    fail(`${project.id} prepared fixture is missing; run type-aware corpus prepare first`);
  }
  const expectedCommit = fixtureCommit(project);
  const actualCommit = git(root, ["rev-parse", "HEAD"], `${project.id} prepared fixture HEAD`);
  if (actualCommit !== expectedCommit) {
    fail(`${project.id} prepared fixture is at ${actualCommit}, expected ${expectedCommit}`);
  }
  const dirty = git(
    root,
    ["status", "--porcelain", "--untracked-files=all"],
    `${project.id} prepared fixture status`,
  );
  if (dirty !== "") fail(`${project.id} prepared fixture has tracked modifications`);
  assertNoFixtureDependencyDirectories(root, project.id);
  return {
    root,
    commit: actualCommit,
    dependencyEnvironment: fixtureDependencyEnvironment(root, project.id),
  };
};

const prepareProject = (project, outDir) => {
  const sourceRoot = sourceFixtureRoot(project);
  const root = preparedFixtureRoot(project, outDir);
  const commit = fixtureCommit(project);
  if (!existsSync(root)) {
    mkdirSync(dirname(root), { recursive: true });
    git(
      sourceRoot,
      ["worktree", "add", "--detach", root, commit],
      `${project.id} clean worktree preparation`,
    );
  }
  const preparedModules = resolve(root, "node_modules");
  const linkedModules = existsSync(preparedModules)
    ? lstatSync(preparedModules).isSymbolicLink()
    : false;
  if (linkedModules) {
    unlinkSync(preparedModules);
  }
  const validated = validatePreparedFixture(project, outDir);
  return {
    id: project.id,
    repo: project.repo,
    ref: project.ref,
    commit: validated.commit,
    dependency_environment: publishDependencyEnvironment(validated.dependencyEnvironment),
  };
};

const prepare = (projects, outDir) => {
  const prepared = projects.map((project) => prepareProject(project, outDir));
  writeJson(resolve(outDir, "fixtures.json"), { schema_version: 1, projects: prepared });
  return prepared;
};

const projectRoot = (project, outDir) => validatePreparedFixture(project, outDir).root;

const minimalEnvironment = (sidecarBin) => {
  const environment = {};
  for (const key of ["PATH", "HOME", "TMPDIR", "TMP", "TEMP", "SYSTEMROOT", "ComSpec", "PATHEXT"]) {
    if (process.env[key]) environment[key] = process.env[key];
  }
  environment.FALLOW_FORMAT = "json";
  environment.FALLOW_QUIET = "1";
  if (sidecarBin) environment.FALLOW_TYPE_AWARE_BIN = sidecarBin;
  return environment;
};

const processTreeIndexes = (rows) => {
  const children = new Map();
  const rssByPid = new Map();
  rows.forEach(([pid, ppid, rss]) => {
    rssByPid.set(pid, rss);
    const siblings = arrayOrEmpty(children.get(ppid));
    siblings.push(pid);
    children.set(ppid, siblings);
  });
  return { children, rssByPid };
};

const processTreeRss = (rootPid, children, rssByPid) => {
  const pending = [rootPid];
  const seen = new Set();
  let total = 0;
  while (pending.length > 0) {
    const pid = pending.pop();
    if (seen.has(pid)) continue;
    seen.add(pid);
    total += integerOrFallback(rssByPid.get(pid), 0);
    pending.push(...arrayOrEmpty(children.get(pid)));
  }
  return total;
};

const descendantsRssKb = (rootPid) => {
  if (process.platform === "win32") return null;
  const result = spawnSync("ps", ["-axo", "pid=,ppid=,rss="], {
    encoding: "utf8",
    maxBuffer: 8 * 1024 * 1024,
  });
  if (result.status !== 0) return null;
  const rows = result.stdout
    .trim()
    .split("\n")
    .map((line) => line.trim().split(/\s+/).map(Number))
    .filter(
      ([pid, ppid, rss]) => Number.isFinite(pid) && Number.isFinite(ppid) && Number.isFinite(rss),
    );
  const { children, rssByPid } = processTreeIndexes(rows);
  return processTreeRss(rootPid, children, rssByPid);
};

export const runProcess = (binary, args, cwd, sidecarBin) =>
  new Promise((accept, reject) => {
    const started = process.hrtime.bigint();
    const child = spawn(binary, args, {
      cwd,
      env: minimalEnvironment(sidecarBin),
      stdio: ["ignore", "pipe", "pipe"],
    });
    const stdout = [];
    const stderr = [];
    let stdoutBytes = 0;
    let stderrBytes = 0;
    let peakRssKb = 0;
    let killedForOutput = false;
    const collect = (chunks, chunk, stream) => {
      const nextBytes =
        stream === "stdout" ? stdoutBytes + chunk.length : stderrBytes + chunk.length;
      if (nextBytes > MAX_OUTPUT_BYTES) {
        killedForOutput = true;
        child.kill("SIGKILL");
        return;
      }
      chunks.push(chunk);
      if (stream === "stdout") stdoutBytes = nextBytes;
      else stderrBytes = nextBytes;
    };
    child.stdout.on("data", (chunk) => collect(stdout, chunk, "stdout"));
    child.stderr.on("data", (chunk) => collect(stderr, chunk, "stderr"));
    child.on("error", reject);
    const sampleProcessTreeRss = () => {
      const rss = descendantsRssKb(child.pid);
      if (rss !== null) peakRssKb = Math.max(peakRssKb, rss);
    };
    sampleProcessTreeRss();
    const sampler = setInterval(sampleProcessTreeRss, PROCESS_TREE_RSS_SAMPLE_INTERVAL_MS);
    child.on("close", (status, signal) => {
      clearInterval(sampler);
      const finalRss = descendantsRssKb(child.pid);
      if (finalRss !== null) peakRssKb = Math.max(peakRssKb, finalRss);
      const wallMs = Number(process.hrtime.bigint() - started) / 1_000_000;
      if (killedForOutput) reject(new Error(`fallow output exceeded ${MAX_OUTPUT_BYTES} bytes`));
      else
        accept({
          status,
          signal,
          stdout: Buffer.concat(stdout).toString("utf8"),
          stderr: Buffer.concat(stderr).toString("utf8"),
          wall_ms: wallMs,
          peak_process_tree_rss_kb: peakRssKb || null,
        });
    });
  });

const findCandidates = (output) => {
  const candidates = [
    output.unused_class_members,
    Object(output.check).unused_class_members,
    Object(output.results).unused_class_members,
  ].find((value) => value !== undefined);
  if (!Array.isArray(candidates)) fail("machine output has no unused_class_members array");
  return candidates;
};

const candidateFields = (candidate) => {
  if (!isObject(candidate)) fail("unused class-member candidate must be an object");
  const fields = {
    path: normalizedRelativePath(candidate.path, "candidate.path"),
    parent_name: candidate.parent_name,
    member_name: candidate.member_name,
    kind: candidate.kind,
    line: candidate.line,
    col: candidate.col,
  };
  for (const field of ["parent_name", "member_name", "kind"]) {
    nonEmptyString(fields[field], `candidate.${field}`);
  }
  ["line", "col"].forEach((field) => {
    if (![Number.isSafeInteger(fields[field]), fields[field] >= 0].every(Boolean)) {
      fail(`candidate.${field} must be a non-negative integer`);
    }
  });
  return fields;
};

export const candidateKey = (projectId, candidate) => {
  nonEmptyString(projectId, "projectId");
  const fields = candidateFields(candidate);
  const identity = [
    projectId,
    fields.path,
    fields.parent_name,
    fields.member_name,
    fields.kind,
    String(fields.line),
    String(fields.col),
  ].join("\0");
  return `tac_${createHash("sha256").update(identity).digest("hex").slice(0, 20)}`;
};

const candidateSetDigest = (keys) =>
  createHash("sha256")
    .update(`${[...keys].toSorted().join("\n")}\n`)
    .digest("hex");

const indexedCandidates = (projectId, output) => {
  const candidates = findCandidates(output)
    .map((candidate) => {
      const fields = candidateFields(candidate);
      return { key: candidateKey(projectId, fields), ...fields };
    })
    .toSorted((left, right) => left.key.localeCompare(right.key));
  const keys = candidates.map((candidate) => candidate.key);
  if (new Set(keys).size !== keys.length) fail(`${projectId} emitted duplicate candidate keys`);
  return candidates;
};

const typeAwareMeta = (output) =>
  [Object(output._meta).type_aware, Object(output.meta).type_aware, null].find(
    (value) => value !== undefined,
  );

const parseMachineOutput = (stdout, projectId) => {
  let output;
  try {
    output = JSON.parse(stdout);
  } catch (error) {
    fail(
      `${projectId} emitted invalid JSON: ${error instanceof Error ? error.message : String(error)}`,
    );
  }
  const validEnvelope = [isObject(output), [undefined, "dead-code"].includes(output.kind)].every(
    Boolean,
  );
  if (!validEnvelope) {
    fail(`${projectId} emitted an unexpected machine-output envelope`);
  }
  return {
    output,
    candidates: indexedCandidates(projectId, output),
    typeAware: typeAwareMeta(output),
  };
};

const fallowArgs = (root, refined) => [
  "dead-code",
  "--root",
  root,
  "--unused-class-members",
  ...(refined ? ["--type-aware"] : []),
  "--format",
  "json",
  "--quiet",
  "--no-cache",
];

const machineErrorDetail = (result) => {
  let detail = result.stderr.trim();
  try {
    const errorOutput = JSON.parse(result.stdout);
    if (typeof errorOutput.message === "string") detail = errorOutput.message;
  } catch {
    return detail;
  }
  return detail;
};

const persistProcessResult = (rawPath, result, failureMessage) => {
  mkdirSync(dirname(rawPath), { recursive: true });
  writeFileSync(rawPath, result.stdout);
  writeFileSync(rawPath.replace(/\.json$/, ".stderr.txt"), result.stderr);
  if ([result.status === null, result.status >= 2].some(Boolean)) {
    fail(failureMessage(result));
  }
};

const executeFallow = async ({ project, fallowBin, sidecarBin, refined, rawPath, outDir }) => {
  const root = projectRoot(project, outDir);
  const result = await runProcess(fallowBin, fallowArgs(root, refined), REPO_ROOT, sidecarBin);
  persistProcessResult(rawPath, result, (failedResult) => {
    const detail = machineErrorDetail(result);
    const mode = refined ? "refined" : "baseline";
    const outcome = firstDefined([failedResult.status, failedResult.signal]);
    return `${project.id} ${mode} run failed (${outcome}): ${detail.slice(0, 2_000)}`;
  });
  const parsed = parseMachineOutput(result.stdout, project.id);
  return { ...result, ...parsed };
};

const validateCandidateExpectation = (project, count) => {
  if ([project.candidate_expectation === "zero", count !== 0].every(Boolean)) {
    fail(`${project.id} zero-candidate control emitted ${count} candidates`);
  }
  if ([project.candidate_expectation === "nonzero", count === 0].every(Boolean)) {
    fail(`${project.id} accuracy-core project emitted no candidates`);
  }
};

const decisionCandidate = (decision) => {
  const subject = Object(decision.subject);
  return {
    path: subject.path,
    parent_name: subject.owner,
    member_name: subject.local_name,
    kind: subject.declaration_kind,
    line: subject.line,
    col: subject.col,
  };
};

const indexCandidateDecisions = (projectId, metadata) => {
  const decisions = new Map();
  for (const decision of arrayOrEmpty(Object(metadata).candidate_decisions)) {
    const key = candidateKey(projectId, decisionCandidate(decision));
    if (decisions.has(key)) fail(`${projectId} emitted duplicate semantic decisions for ${key}`);
    decisions.set(key, decision);
  }
  return decisions;
};

export const compareCandidateSets = (projectId, baseline, refined, metadata) => {
  const refinedKeys = new Set(refined.map((candidate) => candidate.key));
  const baselineKeys = new Set(baseline.map((candidate) => candidate.key));
  const decisions = indexCandidateDecisions(projectId, metadata);
  const additions = [...refinedKeys].filter((key) => !baselineKeys.has(key));
  if (additions.length > 0) {
    fail(`${projectId} refined output added candidates: ${additions.join(", ")}`);
  }
  return baseline.map((candidate) => {
    const decision = decisions.get(candidate.key);
    if (!decision) fail(`${projectId} emitted no semantic decision for ${candidate.key}`);
    const removesFinding = ["confirmed-used", "contract-preserved"].includes(decision.decision);
    if (removesFinding === refinedKeys.has(candidate.key)) {
      fail(
        `${projectId} semantic decision ${decision.decision} disagrees with the refined finding set for ${candidate.key}`,
      );
    }
    return {
      ...candidate,
      semantic_status: removesFinding ? decision.decision : "retained",
      semantic_decision: decision.decision,
      semantic_completeness: decision.status,
      owning_projects: arrayOrEmpty(decision.owning_projects),
      contract: decision.contract ?? null,
    };
  });
};

const ensureRuntimeInputs = (options) => {
  ensureFile(options.fallowBin, "release fallow binary");
  if (!options.sidecarBin) fail("--sidecar-bin is required for an explicit sidecar override");
  ensureFile(options.sidecarBin, "type-aware sidecar");
};

const corpusIdentity = (projects) =>
  projects.map(({ id, repo, ref, role, candidate_expectation }) => ({
    id,
    repo,
    ref,
    role,
    candidate_expectation,
  }));

const sha256File = (path) => createHash("sha256").update(readFileSync(path)).digest("hex");

const dependencyTypeTreeHashes = new Map();

const dependencyTypeTreeHash = (root) => {
  const cached = dependencyTypeTreeHashes.get(root);
  if (cached) return cached;
  const dependencyRoot = resolve(root, "node_modules");
  const paths = filesBelow(dependencyRoot).filter(
    (path) =>
      basename(path) === "package.json" ||
      path.endsWith(".d.ts") ||
      path.endsWith(".d.mts") ||
      path.endsWith(".d.cts"),
  );
  const digest = createHash("sha256");
  for (const path of paths) {
    digest.update(relative(dependencyRoot, path).replaceAll("\\", "/"));
    digest.update("\0");
    digest.update(readFileSync(path));
    digest.update("\0");
  }
  const hash = digest.digest("hex");
  dependencyTypeTreeHashes.set(root, hash);
  return hash;
};

const publishDependencyEnvironment = (environment) => {
  if (environment.dependency_root === null) {
    return {
      dependency_installation: environment.dependency_installation,
      path_base: "fallow-workspace-root",
      dependency_root: null,
      lockfile: null,
      lockfile_sha256: null,
      type_declaration_tree_sha256: null,
    };
  }
  const lockfile = resolve(environment.dependency_root, "package-lock.json");
  return {
    dependency_installation: environment.dependency_installation,
    path_base: "fallow-workspace-root",
    dependency_root: relative(REPO_ROOT, environment.dependency_root).replaceAll("\\", "/") || ".",
    lockfile: relative(REPO_ROOT, lockfile).replaceAll("\\", "/"),
    lockfile_sha256: sha256File(lockfile),
    type_declaration_tree_sha256: dependencyTypeTreeHash(environment.dependency_root),
  };
};

const filesBelow = (root) => {
  if (!existsSync(root)) return [];
  const files = [];
  const visit = (directory) => {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const path = resolve(directory, entry.name);
      if (entry.isDirectory()) visit(path);
      else if (entry.isFile()) files.push(path);
    }
  };
  visit(root);
  return files.toSorted();
};

const sidecarArtifactHash = (sidecarBin) => {
  const root = dirname(sidecarBin);
  const paths = [
    sidecarBin,
    resolve(root, "package.json"),
    resolve(root, "package-lock.json"),
    ...filesBelow(resolve(root, "src")),
    ...filesBelow(resolve(root, "node_modules/typescript")),
    ...filesBelow(resolve(root, "node_modules/@typescript")),
  ].filter((path) => existsSync(path) && statSync(path).isFile());
  const digest = createHash("sha256");
  for (const path of [...new Set(paths)].toSorted()) {
    digest.update(relative(root, path).replaceAll("\\", "/"));
    digest.update("\0");
    digest.update(readFileSync(path));
    digest.update("\0");
  }
  if (paths.length === 0) fail(`no sidecar runtime files found beside ${basename(sidecarBin)}`);
  return digest.digest("hex");
};

const binaryVersion = (binary) => {
  const result = spawnSync(binary, ["--version"], {
    encoding: "utf8",
    env: minimalEnvironment(null),
    maxBuffer: 1024 * 1024,
  });
  if (result.status !== 0) fail(`failed to read release binary version from ${binary}`);
  const output = result.stdout.trim();
  try {
    const parsed = JSON.parse(output);
    if (typeof parsed.message === "string") return parsed.message;
  } catch {
    // Human version output is already the desired value.
  }
  return output;
};

const currentPublicationProvenance = (manifestPath, projects, options) => {
  const preparedProjects = projects.map((project) => ({
    project,
    validated: validatePreparedFixture(project, options.outDir),
  }));
  return {
    manifest_sha256: sha256File(manifestPath),
    fallow: {
      version: binaryVersion(options.fallowBin),
      sha256: sha256File(options.fallowBin),
    },
    sidecar: {
      sha256: sidecarArtifactHash(options.sidecarBin),
    },
    runtime: {
      platform: process.platform,
      architecture: process.arch,
      node: process.version,
    },
    dependency_environments: Object.fromEntries(
      preparedProjects.map(({ project, validated }) => {
        return [project.id, publishDependencyEnvironment(validated.dependencyEnvironment)];
      }),
    ),
    fixtures: preparedProjects.map(({ project, validated }) => {
      return { id: project.id, repo: project.repo, ref: project.ref, commit: validated.commit };
    }),
  };
};

const buildProvenance = (manifestPath, projects, options) => ({
  generated_at: new Date().toISOString(),
  ...currentPublicationProvenance(manifestPath, projects, options),
});

const normalizedRefinement = (run) => {
  const meta = structuredClone(run.typeAware);
  if (isObject(meta)) {
    delete meta.elapsed_ms;
    delete meta.phase_timings_ms;
    delete meta.phase_timings;
  }
  return { candidates: run.candidates, type_aware: meta };
};

const requireSameNormalizedRun = (projectId, mode, expected, actual) => {
  if (JSON.stringify(expected) !== JSON.stringify(actual)) {
    fail(`${projectId} ${mode} normalized output changed between repeated runs`);
  }
};

export const requireCompletePublicationCorpus = (manifest, discovery) => {
  const expected = corpusIdentity(manifest.projects);
  if (!isObject(discovery) || JSON.stringify(discovery.corpus) !== JSON.stringify(expected)) {
    fail(
      "publication gate requires the complete manifest corpus; rerun discover and measure without `--project`, then run evidence before summarize",
    );
  }
};

const discoveryRawPath = (options, artifactProject, iteration, mode) =>
  resolve(options.outDir, "discover", `${artifactProject}-${iteration + 1}-${mode}.json`);

export const selectFirstCompletedRun = (previous, current) => previous ?? current;

const runDiscoveryIterations = async (project, options) => {
  const artifactProject = safeArtifactName(project.id);
  let baseline = null;
  let refined = null;
  let expectedBaseline = null;
  let expectedRefined = null;
  for (let iteration = 0; iteration < options.discoveryRuns; iteration += 1) {
    const currentBaseline = await executeFallow({
      project,
      fallowBin: options.fallowBin,
      sidecarBin: options.sidecarBin,
      refined: false,
      rawPath: discoveryRawPath(options, artifactProject, iteration, "baseline"),
      outDir: options.outDir,
    });
    const currentRefined = await executeFallow({
      project,
      fallowBin: options.fallowBin,
      sidecarBin: options.sidecarBin,
      refined: true,
      rawPath: discoveryRawPath(options, artifactProject, iteration, "refined"),
      outDir: options.outDir,
    });
    const normalizedBaseline = normalizedRefinement(currentBaseline);
    const normalizedRefined = normalizedRefinement(currentRefined);
    if (expectedBaseline) {
      requireSameNormalizedRun(project.id, "baseline", expectedBaseline, normalizedBaseline);
    }
    if (expectedRefined) {
      requireSameNormalizedRun(project.id, "refined", expectedRefined, normalizedRefined);
    }
    expectedBaseline = normalizedBaseline;
    expectedRefined = normalizedRefined;
    baseline = selectFirstCompletedRun(baseline, currentBaseline);
    refined = selectFirstCompletedRun(refined, currentRefined);
  }
  return { baseline, refined };
};

const compactTypeAwareMetadata = (metadata) => {
  if (!metadata) return null;
  return {
    protocol_version: metadata.protocol_version,
    sidecar_version: metadata.sidecar_version,
    backend_version: metadata.backend_version,
    candidate_count: metadata.candidate_count,
    confirmed_used_count: metadata.confirmed_used_count,
    contract_preserved_count: metadata.contract_preserved_count,
    no_static_references_count: metadata.no_static_references_count,
    fix_eligible_count: metadata.fix_eligible_count,
    unresolved_count: metadata.unresolved_count,
    abstained_count: metadata.abstained_count,
    abstention_reasons: metadata.abstention_reasons,
    elapsed_ms: metadata.elapsed_ms,
    phase_timings_ms: metadata.phase_timings_ms,
    projects: metadata.projects,
  };
};

const discoverProject = async (project, options) => {
  const { baseline, refined } = await runDiscoveryIterations(project, options);
  validateCandidateExpectation(project, baseline.candidates.length);
  const candidates = compareCandidateSets(
    project.id,
    baseline.candidates,
    refined.candidates,
    refined.typeAware,
  );
  const metaCount = Object(refined.typeAware).candidate_count;
  const mismatchedCount = [
    Number.isSafeInteger(metaCount),
    metaCount !== baseline.candidates.length,
  ].every(Boolean);
  if (mismatchedCount) {
    fail(`${project.id} type-aware candidate_count does not match baseline output`);
  }
  return {
    id: project.id,
    role: project.role,
    feature_buckets: project.feature_buckets,
    baseline_candidate_count: baseline.candidates.length,
    refined_candidate_count: refined.candidates.length,
    confirmed_used_count: candidates.filter(
      ({ semantic_status: status }) => status === "confirmed-used",
    ).length,
    contract_preserved_count: candidates.filter(
      ({ semantic_status: status }) => status === "contract-preserved",
    ).length,
    type_aware: compactTypeAwareMetadata(refined.typeAware),
    candidates,
  };
};

const discover = async (manifest, projects, options) => {
  ensureRuntimeInputs(options);
  const results = [];
  for (const project of projects) {
    results.push(await discoverProject(project, options));
  }
  const report = {
    schema_version: 1,
    corpus: corpusIdentity(projects),
    provenance: buildProvenance(options.manifest, projects, options),
    determinism_runs: options.discoveryRuns,
    projects: results,
  };
  writeJson(resolve(options.outDir, "discovery.json"), report);
  return report;
};

export const runOrderForIteration = (iteration) => {
  if (!Number.isSafeInteger(iteration) || iteration < 0) fail("iteration must be non-negative");
  return iteration % 2 === 0 ? ["baseline", "refined"] : ["refined", "baseline"];
};

const arrayOrEmpty = (value) => (Array.isArray(value) ? value : []);
const finiteOrNull = (value) => (Number.isFinite(value) ? value : null);
const integerOrFallback = (value, fallback) => (Number.isSafeInteger(value) ? value : fallback);
const firstObject = (values) => values.find(isObject) ?? {};

const compactRun = (run, mode, iteration, warmup) => {
  const meta = Object(run.typeAware);
  const projects = arrayOrEmpty(meta.projects);
  const projectCount = projects.length === 0 ? null : projects.length;
  return {
    iteration,
    warmup,
    mode,
    wall_ms: run.wall_ms,
    peak_process_tree_rss_kb: run.peak_process_tree_rss_kb,
    candidate_count: run.candidates.length,
    refinement_ms: finiteOrNull(meta.elapsed_ms),
    program_count: integerOrFallback(meta.program_count, projectCount),
    source_files_per_program: Array.isArray(meta.source_files_per_program)
      ? meta.source_files_per_program
      : projects.map(({ source_file_count }) => source_file_count).filter(Number.isSafeInteger),
    phase_timings_ms: firstObject([meta.phase_timings_ms, meta.phase_timings]),
    reason_counts: firstObject([
      meta.reason_counts,
      meta.abstention_reasons,
      Object(meta.candidate_status_summary).reason_counts,
    ]),
  };
};

const measurementPhases = (options) => [
  ...Array.from({ length: options.warmups }, (_, iteration) => ({ warmup: true, iteration })),
  ...Array.from({ length: options.runs }, (_, iteration) => ({ warmup: false, iteration })),
];

const measurementRawPath = (options, artifactProject, phase, mode) => {
  const kind = phase.warmup ? "warmup" : "measured";
  return resolve(
    options.outDir,
    "measure",
    artifactProject,
    `${kind}-${phase.iteration + 1}-${mode}.json`,
  );
};

const validateBaselineMeasurement = (project, run, state) => {
  validateCandidateExpectation(project, run.candidates.length);
  const keys = run.candidates.map(({ key }) => key);
  const changed = [
    Array.isArray(state.baselineKeys),
    JSON.stringify(keys) !== JSON.stringify(state.baselineKeys),
  ].every(Boolean);
  if (changed) fail(`${project.id} baseline candidate keys changed between runs`);
  state.baselineKeys = keys;
};

const validateRefinedMeasurement = (project, run, state) => {
  if (!Array.isArray(state.baselineKeys)) return;
  const unexpected = run.candidates.filter(({ key }) => !state.baselineKeys.includes(key));
  if (unexpected.length > 0) fail(`${project.id} refined run added unexpected candidates`);
  const normalized = normalizedRefinement(run);
  if (state.refined) {
    requireSameNormalizedRun(project.id, "refined measurement", state.refined, normalized);
  }
  state.refined = normalized;
};

const runMeasurement = async (project, options, artifactProject, phase, mode, state) => {
  const refined = mode === "refined";
  const run = await executeFallow({
    project,
    fallowBin: options.fallowBin,
    sidecarBin: options.sidecarBin,
    refined,
    rawPath: measurementRawPath(options, artifactProject, phase, mode),
    outDir: options.outDir,
  });
  const validators = {
    baseline: validateBaselineMeasurement,
    refined: validateRefinedMeasurement,
  };
  validators[mode](project, run, state);
  return compactRun(run, mode, phase.iteration, phase.warmup);
};

const measureProject = async (project, options) => {
  const projectRuns = [];
  const artifactProject = safeArtifactName(project.id);
  const state = { baselineKeys: null, refined: null };
  for (const phase of measurementPhases(options)) {
    for (const mode of runOrderForIteration(phase.iteration)) {
      projectRuns.push(await runMeasurement(project, options, artifactProject, phase, mode, state));
    }
  }
  return {
    id: project.id,
    role: project.role,
    run_order: projectRuns.map(({ iteration, warmup, mode }) => ({ iteration, warmup, mode })),
    runs: projectRuns,
  };
};

const measure = async (manifest, projects, options) => {
  ensureRuntimeInputs(options);
  const projectReports = [];
  for (const project of projects) {
    projectReports.push(await measureProject(project, options));
  }
  const report = {
    schema_version: 1,
    corpus: corpusIdentity(projects),
    provenance: buildProvenance(options.manifest, projects, options),
    warmups: options.warmups,
    measured_pairs: options.runs,
    projects: projectReports,
  };
  writeJson(resolve(options.outDir, "measurements.json"), report);
  return report;
};

const writeSolutionConfigFixture = (root) => {
  writeFileSync(
    resolve(root, "package.json"),
    `${JSON.stringify({ type: "module", main: "src/index.ts" })}\n`,
  );
  writeFileSync(
    resolve(root, "tsconfig.json"),
    `${JSON.stringify({ files: [], references: [{ path: "packages/lib" }] })}\n`,
  );
  mkdirSync(resolve(root, "packages/lib/src"), { recursive: true });
  mkdirSync(resolve(root, "src"), { recursive: true });
  writeFileSync(
    resolve(root, "packages/lib/tsconfig.json"),
    `${JSON.stringify({ compilerOptions: { composite: true, strict: true }, include: ["src"] })}\n`,
  );
  writeFileSync(
    resolve(root, "packages/lib/src/client.ts"),
    "export class Client {\n  used(): void {}\n  execute(): void {}\n}\nnew Client().used();\n",
  );
  writeFileSync(
    resolve(root, "src/index.ts"),
    'import { Client } from "../packages/lib/src/client.js";\nnew Client().used();\n',
  );
};

const validateSolutionConfigOutput = (output) => {
  const metadata = Object(Object(output)._meta).type_aware;
  const meta = Object(metadata);
  const valid = [
    arrayOrEmpty(output.unused_class_members).length === 1,
    arrayOrEmpty(meta.selected_tsconfigs).length === 0,
    arrayOrEmpty(meta.projects).length === 0,
    meta.unresolved_count === 1,
    meta.abstained_count === 0,
    Object(meta.abstention_reasons).no_project === 1,
    arrayOrEmpty(meta.candidate_decisions).some(
      ({ decision, reason_code: reasonCode }) =>
        decision === "retained-unresolved" && reasonCode === "no-project",
    ),
  ].every(Boolean);
  if (!valid) {
    fail(
      `solution-tsconfig full CLI smoke did not fail closed with an unresolved no-project decision: ${JSON.stringify(
        { findings: arrayOrEmpty(output.unused_class_members).length, metadata },
      )}`,
    );
  }
};

const runSolutionConfigCliSmoke = (options) => {
  ensureFile(options.fallowBin, "fallow binary");
  const root = mkdtempSync(resolve(tmpdir(), "fallow-type-aware-solution-"));
  try {
    writeSolutionConfigFixture(root);
    const result = spawnSync(
      options.fallowBin,
      [
        "dead-code",
        "--root",
        root,
        "--unused-class-members",
        "--type-aware",
        "--type-aware-project",
        "tsconfig.json",
        "--format",
        "json",
        "--quiet",
        "--no-cache",
      ],
      {
        cwd: root,
        encoding: "utf8",
        env: minimalEnvironment(options.sidecarBin),
        maxBuffer: 16 * 1024 * 1024,
      },
    );
    if (![0, 1].includes(result.status)) {
      fail(
        `solution-tsconfig full CLI smoke failed with exit ${result.status}: stdout=${result.stdout.trim()} stderr=${result.stderr.trim()}`,
      );
    }
    const output = JSON.parse(result.stdout);
    validateSolutionConfigOutput(output);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
};

const focused = (options) => {
  if (!options.sidecarBin) fail("--sidecar-bin is required for focused verification");
  ensureFile(options.sidecarBin, "type-aware sidecar");
  const sidecarRoot = resolve(REPO_ROOT, "tools/type-aware-sidecar");
  const result = spawnSync(process.execPath, ["--test", "--test-reporter=tap", FOCUSED_TEST_PATH], {
    cwd: sidecarRoot,
    encoding: "utf8",
    env: minimalEnvironment(options.sidecarBin),
    maxBuffer: 16 * 1024 * 1024,
  });
  mkdirSync(resolve(options.outDir, "focused"), { recursive: true });
  writeFileSync(resolve(options.outDir, "focused", "stdout.txt"), result.stdout);
  writeFileSync(resolve(options.outDir, "focused", "stderr.txt"), result.stderr);
  if (result.status !== 0) fail("candidate-bearing semantic edge-case suite failed");
  const completedCases = new Set(
    [...result.stdout.matchAll(/^# Subtest: (.+)$/gm)].map((match) => match[1]),
  );
  runSolutionConfigCliSmoke(options);
  completedCases.add("full CLI safely abstains for an explicit solution tsconfig");
  const missingCases = REQUIRED_FOCUSED_CASES.filter((name) => !completedCases.has(name));
  if (missingCases.length > 0) {
    fail(`focused suite is missing required cases: ${missingCases.join(", ")}`);
  }
  const report = {
    schema_version: 1,
    passed: true,
    fallow_sha256: sha256File(options.fallowBin),
    sidecar_sha256: sidecarArtifactHash(options.sidecarBin),
    test_sha256: sha256File(FOCUSED_TEST_PATH),
    cases: [...REQUIRED_FOCUSED_CASES].toSorted(),
    runtime: { platform: process.platform, architecture: process.arch, node: process.version },
  };
  writeJson(resolve(options.outDir, "focused.json"), report);
  return report;
};

export const normalizeSupplementalOutput = (value) => {
  const output = structuredClone(value);
  delete output.elapsed_ms;
  const metadata = Object(output._meta);
  if (isObject(metadata.telemetry)) delete metadata.telemetry.analysis_run_id;
  if (isObject(metadata.type_aware)) {
    delete output._meta.type_aware.elapsed_ms;
    delete output._meta.type_aware.phase_timings_ms;
  }
  return output;
};

export const validateSupplementalArtifactData = (artifact, decisions, context) => {
  const normalizedFileDigest = (path) =>
    createHash("sha256")
      .update(JSON.stringify(normalizeSupplementalOutput(readJson(path))))
      .digest("hex");
  return validateSupplementalData(artifact, decisions, context, {
    candidateSetDigest,
    ensureFile,
    fail,
    isObject,
    normalizedFileDigest,
    normalizedRelativePath,
    relative,
    resolve,
    sep,
  });
};

const prepareSupplementalFixture = () => {
  if (!existsSync(SUPPLEMENTAL_VITEST_ROOT)) {
    mkdirSync(dirname(SUPPLEMENTAL_VITEST_ROOT), { recursive: true });
    git(
      REPO_ROOT,
      [
        "clone",
        "--filter=blob:none",
        "--no-checkout",
        SUPPLEMENTAL_VITEST_REPO,
        SUPPLEMENTAL_VITEST_ROOT,
      ],
      "supplemental Vitest clone",
    );
    git(
      SUPPLEMENTAL_VITEST_ROOT,
      ["checkout", "--detach", SUPPLEMENTAL_VITEST_COMMIT],
      "supplemental Vitest checkout",
    );
  }
};

const persistTrackedJson = (path, description, artifact, publicationMode, driftMessage) => {
  if (publicationMode === "write") {
    writeJson(path, artifact);
    return;
  }
  const tracked = readJson(path, description);
  if (JSON.stringify(tracked) !== JSON.stringify(artifact)) fail(driftMessage);
};

const validateSupplementalFixture = () => {
  ensureFile(resolve(SUPPLEMENTAL_VITEST_ROOT, "package.json"), "supplemental Vitest fixture");
  const commit = git(SUPPLEMENTAL_VITEST_ROOT, ["rev-parse", "HEAD"], "Vitest fixture HEAD");
  if (commit !== SUPPLEMENTAL_VITEST_COMMIT) {
    fail(`Vitest fixture is at ${commit}, expected ${SUPPLEMENTAL_VITEST_COMMIT}`);
  }
  const dirty = git(
    SUPPLEMENTAL_VITEST_ROOT,
    ["status", "--porcelain", "--untracked-files=all"],
    "Vitest fixture status",
  );
  if (dirty !== "") fail("supplemental Vitest fixture has tracked modifications");
  assertNoFixtureDependencyDirectories(SUPPLEMENTAL_VITEST_ROOT, "vitest");
  return {
    commit,
    dependencyEnvironment: publishDependencyEnvironment(
      fixtureDependencyEnvironment(SUPPLEMENTAL_VITEST_ROOT, "vitest"),
    ),
  };
};

const runSupplementalMode = async (options, rawDir, iteration, mode) => {
  const refined = mode === "refined";
  const suffix = iteration === 0 ? "" : "-2";
  const rawPath = resolve(rawDir, `vitest-${mode}${suffix}.json`);
  const result = await runProcess(
    options.fallowBin,
    fallowArgs(SUPPLEMENTAL_VITEST_ROOT, refined),
    REPO_ROOT,
    options.sidecarBin,
  );
  writeFileSync(rawPath, result.stdout);
  writeFileSync(rawPath.replace(/\.json$/, ".err"), result.stderr);
  if ([result.status === null, result.status >= 2].some(Boolean)) {
    fail(
      `supplemental Vitest ${mode} run failed with ${firstDefined([result.status, result.signal])}`,
    );
  }
  return {
    mode,
    iteration,
    rawPath,
    raw_sha256: sha256File(rawPath),
    ...parseMachineOutput(result.stdout, "vitest"),
  };
};

const collectSupplementalRuns = async (options) => {
  const rawDir = resolve(DEFAULT_OUT_DIR, "supplemental");
  const runs = [];
  for (let iteration = 0; iteration < 2; iteration += 1) {
    for (const mode of runOrderForIteration(iteration)) {
      runs.push(await runSupplementalMode(options, rawDir, iteration, mode));
    }
  }
  return runs;
};

const validateSupplementalDeterminism = (runs) => {
  ["baseline", "refined"].forEach((mode) => {
    const normalized = runs
      .filter((run) => run.mode === mode)
      .map(({ output }) => normalizeSupplementalOutput(output));
    if (JSON.stringify(normalized[0]) !== JSON.stringify(normalized[1])) {
      fail(`supplemental Vitest ${mode} output is not deterministic`);
    }
  });
};

const initialSupplementalRun = (runs, expectedMode) =>
  runs.find(({ mode, iteration }) => [mode === expectedMode, iteration === 0].every(Boolean));

const supplementalConfirmedKeys = (baseline, refined) =>
  compareCandidateSets("vitest", baseline.candidates, refined.candidates, refined.typeAware)
    .filter(({ semantic_status: status }) => status === "confirmed-used")
    .map(({ key }) => key)
    .toSorted();

const validateSupplementalReview = (decisions, commit, confirmedKeys) => {
  const review = decisions.supplemental_reviews.find(
    ({ project_id: projectId }) => projectId === "vitest",
  );
  if (
    ![
      Boolean(review),
      Object(review).commit === commit,
      Object(review).verdict === "approved",
    ].every(Boolean)
  ) {
    fail("supplemental Vitest review decision is missing or stale");
  }
  const reviewedKeys = arrayOrEmpty(review.candidate_keys).toSorted();
  const validCandidateSet = [
    JSON.stringify(review.candidate_keys) === JSON.stringify(reviewedKeys),
    new Set(reviewedKeys).size === reviewedKeys.length,
    review.candidate_count === reviewedKeys.length,
    review.candidate_set_sha256 === candidateSetDigest(reviewedKeys),
  ].every(Boolean);
  if (!validCandidateSet) fail("supplemental Vitest review candidate set is invalid");
  const reviewedSet = new Set(reviewedKeys);
  const cleanRunIsSubset = confirmedKeys.every((key) => reviewedSet.has(key));
  if (!cleanRunIsSubset) fail("clean supplemental Vitest confirmations exceed the reviewed set");
  return { cleanRunIsSubset, review, reviewedKeys };
};

const validateSupplementalMetadata = (meta, baseline, confirmedKeys) => {
  const valid = [
    meta.confirmed_used_count === confirmedKeys.length,
    meta.candidate_count === baseline.candidates.length,
    meta.unresolved_count +
      meta.abstained_count +
      meta.confirmed_used_count +
      meta.contract_preserved_count +
      meta.no_static_references_count ===
      meta.candidate_count,
  ].every(Boolean);
  if (!valid) fail("supplemental Vitest type-aware metadata is inconsistent");
};

const supplementalSourceRuns = (runs) =>
  runs
    .map(({ mode, iteration, rawPath, output }) => ({
      mode,
      iteration,
      path: relative(REPO_ROOT, rawPath).replaceAll("\\", "/"),
      normalized_sha256: createHash("sha256")
        .update(JSON.stringify(normalizeSupplementalOutput(output)))
        .digest("hex"),
    }))
    .toSorted((left, right) =>
      `${left.mode}:${left.iteration}`.localeCompare(`${right.mode}:${right.iteration}`),
    );

const buildSupplementalArtifact = (context) => ({
  schema_version: 6,
  canonical_candidate_key: {
    algorithm: "sha256",
    prefix: "tac_",
    digest_hex_characters: 20,
    fields: ["project_id", "path", "parent_name", "member_name", "kind", "line", "col"],
    separator: "NUL",
  },
  project: {
    repo: "vitest-dev/vitest",
    commit: context.commit,
    tracked_source_clean: true,
    dependency_environment: context.dependencyEnvironment,
  },
  artifacts: {
    fallow_sha256: sha256File(context.options.fallowBin),
    sidecar_sha256: sidecarArtifactHash(context.options.sidecarBin),
    source_runs: supplementalSourceRuns(context.runs),
  },
  result: {
    baseline_candidates: context.baseline.candidates.length,
    refined_candidates: context.refined.candidates.length,
    confirmed_used: context.confirmedKeys.length,
    contract_preserved: context.meta.contract_preserved_count,
    no_static_references: context.meta.no_static_references_count,
    fix_eligible: context.meta.fix_eligible_count,
    unresolved_retained: context.meta.unresolved_count,
    abstained_retained: context.meta.abstained_count,
    added_candidates: 0,
    normalized_runs: 2,
    deterministic: true,
    confirmed_candidate_set_sha256: candidateSetDigest(context.confirmedKeys),
    confirmed_candidate_keys: context.confirmedKeys,
  },
  independent_review: {
    verdict: context.review.verdict,
    reviewed_candidate_count: context.review.candidate_count,
    reviewed_candidate_set_sha256: context.review.candidate_set_sha256,
    reviewed_candidate_keys: context.reviewedKeys,
    clean_run_is_subset: context.cleanRunIsSubset,
    known_incorrect_removals: context.review.known_incorrect_removals,
    method: context.review.method,
  },
});

const supplemental = async (options, publicationMode = "write") => {
  ensureRuntimeInputs(options);
  prepareSupplementalFixture();
  const { commit, dependencyEnvironment } = validateSupplementalFixture();
  const runs = await collectSupplementalRuns(options);
  validateSupplementalDeterminism(runs);
  const baseline = initialSupplementalRun(runs, "baseline");
  const refined = initialSupplementalRun(runs, "refined");
  const confirmedKeys = supplementalConfirmedKeys(baseline, refined);
  const decisions = readJson(DEFAULT_ADJUDICATION, "adjudication decisions");
  const reviewContext = validateSupplementalReview(decisions, commit, confirmedKeys);
  const meta = refined.typeAware;
  validateSupplementalMetadata(meta, baseline, confirmedKeys);
  const artifact = buildSupplementalArtifact({
    baseline,
    commit,
    confirmedKeys,
    dependencyEnvironment,
    meta,
    options,
    refined,
    runs,
    ...reviewContext,
  });
  validateSupplementalArtifactData(artifact, decisions, {
    fallowSha256: sha256File(options.fallowBin),
    sidecarSha256: sidecarArtifactHash(options.sidecarBin),
    sourceRoot: REPO_ROOT,
    dependencyEnvironment,
  });
  persistTrackedJson(
    SUPPLEMENTAL_ARTIFACT,
    "tracked supplemental artifact",
    artifact,
    publicationMode,
    "tracked supplemental artifact has generator drift; rerun supplemental",
  );
  return artifact;
};

const readDiscovery = (outDir) => readJson(resolve(outDir, "discovery.json"), "discovery artifact");

const sourceLineEvidence = (root, candidate) => {
  const sourcePath = resolve(root, candidate.path);
  const relativeToRoot = relative(root, sourcePath);
  if ([relativeToRoot.startsWith(`..${sep}`), relativeToRoot === ".."].some(Boolean)) {
    fail(`${candidate.key} source path escapes its fixture`);
  }
  let excerpt = "";
  if (existingFile(sourcePath)) {
    const lines = readFileSync(sourcePath, "utf8").split(/\r?\n/);
    excerpt = (lines[Math.max(0, candidate.line - 1)] ?? "").trim();
  }
  return { path: candidate.path, line: candidate.line, col: candidate.col, excerpt };
};

const sourceLocationEvidence = (root, location) => {
  const sourcePath = resolve(root, location.path);
  const relativeToRoot = relative(root, sourcePath);
  if ([relativeToRoot.startsWith(`..${sep}`), relativeToRoot === ".."].some(Boolean)) {
    fail(`use evidence path escapes its prepared fixture: ${location.path}`);
  }
  const lines = readFileSync(sourcePath, "utf8").split(/\r?\n/);
  const excerpt = (lines[location.line - 1] ?? "").trim();
  if (excerpt === "") fail(`use evidence has no source line: ${location.path}:${location.line}`);
  return { path: location.path, line: location.line, col: location.col, excerpt };
};

export const runExactSidecarRequest = ({
  sidecarBin,
  request,
  stdoutPath,
  stderrPath,
  timeoutMs = SEMANTIC_TIMEOUT_MS,
  maxResponseBytes = MAX_SEMANTIC_RESPONSE_BYTES,
}) =>
  executeExactSidecarRequest({
    sidecarBin,
    request,
    stdoutPath,
    stderrPath,
    timeoutMs,
    maxResponseBytes,
    dependencies: {
      fail,
      isObject,
      minimalEnvironment,
      normalizedRelativePath,
      protocolVersion: SEMANTIC_PROTOCOL_VERSION,
      queryStatuses: SEMANTIC_QUERY_STATUSES,
      maximumStderrBytes: MAX_SEMANTIC_STDERR_BYTES,
    },
  });

const checkerEvidenceRequest = (project, candidates, outDir) =>
  createCheckerEvidenceRequest({
    project,
    candidates,
    outDir,
    projectRoot,
    protocolVersion: SEMANTIC_PROTOCOL_VERSION,
  });

const collectCheckerEvidence = async (project, candidates, outDir, sidecarBin) => {
  const { root, request } = checkerEvidenceRequest(project, candidates, outDir);
  const artifactProject = safeArtifactName(project.id);
  const response = runExactSidecarRequest({
    sidecarBin,
    request,
    stdoutPath: resolve(outDir, "evidence", `${artifactProject}.sidecar.json`),
    stderrPath: resolve(outDir, "evidence", `${artifactProject}.sidecar.stderr.txt`),
  });
  return {
    requestDigest: requestDigest(request),
    responseDigest: normalizedEvidenceResponseDigest(response),
    evidence: new Map(
      response.results
        .filter(({ assertion }) => assertion === "confirmed-used")
        .map(({ query_id: queryId, evidence: locations }) => [
          candidates[queryId].key,
          locations.map((location) => sourceLocationEvidence(root, location)),
        ]),
    ),
  };
};

const firstNonEmptyLineEvidence = (root, path) => {
  const sourcePath = resolve(root, path);
  const relativeToRoot = relative(root, sourcePath);
  if (relativeToRoot.startsWith(`..${sep}`) || relativeToRoot === "..") {
    fail(`source evidence path escapes its prepared fixture: ${path}`);
  }
  const lines = readFileSync(sourcePath, "utf8").split(/\r?\n/);
  const lineIndex = lines.findIndex((line) => line.trim() !== "");
  if (lineIndex < 0) fail(`source evidence file is empty: ${path}`);
  return sourceLocationEvidence(root, { path, line: lineIndex + 1, col: 0 });
};

const parseCapabilityOutput = (stdout, id) => {
  let output;
  try {
    output = JSON.parse(stdout);
  } catch (error) {
    fail(`${id} capability run emitted invalid JSON: ${describeError(error)}`);
  }
  if (!isObject(output)) fail(`${id} capability run emitted an invalid output envelope`);
  return output;
};

const capabilityProcess = async (options, id, root, args) => {
  const rawPath = resolve(DEFAULT_OUT_DIR, "capabilities", `${id}.json`);
  const result = await runProcess(
    options.fallowBin,
    [...args, "--format", "json", "--quiet", "--no-cache"],
    REPO_ROOT,
    options.sidecarBin,
  );
  persistProcessResult(
    rawPath,
    result,
    (failedResult) =>
      `${id} capability run failed with ${firstDefined([
        failedResult.status,
        failedResult.signal,
      ])}`,
  );
  const output = parseCapabilityOutput(result.stdout, id);
  return { output, root };
};

const compactPrograms = (output) =>
  arrayOrEmpty(Object(Object(output._meta).type_aware).projects).map(
    ({ config, source, status, source_file_count: sourceFileCount, program_reused: reused }) => ({
      config,
      source,
      status,
      source_file_count: sourceFileCount,
      program_reused: reused,
    }),
  );

const compactRefinement = (candidate, confirmedCount, evidence, reviewed) => ({
  assertion: "confirmed-used",
  confirmed_used_count: confirmedCount,
  candidate: {
    key: candidate.key,
    path: candidate.path,
    parent_name: candidate.parent_name,
    member_name: candidate.member_name,
    kind: candidate.kind,
    line: candidate.line,
    col: candidate.col,
  },
  reviewed,
  source_evidence: evidence,
});

const semanticCapabilityProof = (root, inspectOutput, couplingOutput) => {
  const evidence = Object(inspectOutput.evidence);
  const trace = Object(evidence.semantic_trace).data;
  const api = Object(evidence.api_surface).data;
  const impact = Object(evidence.symbol_impact).data;
  const targetedTests = Object(evidence.targeted_tests).data;
  const coupling = Object(Object(couplingOutput._meta).type_aware).type_coupling;
  if (![trace, api, impact, coupling].every(isObject)) {
    fail("semantic capability run omitted a required result section");
  }
  const traceEvidence = trace.references
    .slice(0, 3)
    .map((reference) => sourceLocationEvidence(root, reference));
  const apiEvidence = api.entries
    .slice(0, 3)
    .map(({ exposed }) => sourceLocationEvidence(root, exposed));
  const impactEvidence = [
    ...impact.direct_consumers.slice(0, 2).map(({ path }) => firstNonEmptyLineEvidence(root, path)),
    ...impact.targeted_tests.slice(0, 2).map(({ path }) => firstNonEmptyLineEvidence(root, path)),
  ];
  const couplingEdges = coupling.files.flatMap((file) => arrayOrEmpty(file.edges));
  const couplingEvidence = couplingEdges
    .slice(0, 3)
    .map(({ evidence: location }) => sourceLocationEvidence(root, location));
  return {
    programs: {
      inspect: compactPrograms(inspectOutput),
      coupling: compactPrograms(couplingOutput),
    },
    capabilities: {
      "semantic-symbol-trace": {
        assertion: trace.assertion,
        status: trace.status,
        selected_project: trace.selected_project,
        total_reference_count: trace.total_reference_count,
        checker_evidence_count: trace.checker_evidence_count,
        graph_evidence_count: trace.graph_evidence_count,
        source_evidence: traceEvidence,
      },
      "public-api-surface": {
        assertion: api.assertion,
        status: api.status,
        public_entry_sample_count: api.entries.length,
        private_type_leak_sample_count: api.private_type_leaks.length,
        omissions: arrayOrEmpty(api.omissions),
        source_evidence: apiEvidence,
      },
      "semantic-impact-targeted-tests": {
        assertion: impact.assertion,
        status: impact.status,
        direct_consumer_count: impact.total_direct_consumer_count,
        affected_file_count: impact.total_affected_file_count,
        targeted_test_count: impact.total_targeted_test_count,
        targeted_tests: firstDefined([Object(targetedTests).tests, impact.targeted_tests]),
        confidence: impact.confidence,
        source_evidence: impactEvidence,
      },
      "public-type-coupling": {
        assertion: coupling.assertion,
        status: coupling.status,
        summary: coupling.summary,
        top_contributors: coupling.top_contributors,
        cycles: coupling.cycles,
        source_evidence: couplingEvidence,
      },
    },
  };
};

const refinementProofs = (manifest, options) => {
  const discovery = readDiscovery(options.outDir);
  const astro = discovery.projects.find(({ id }) => id === "astro");
  if (!astro) fail("capability proof requires the Astro corpus discovery");
  const astroCandidate = astro.candidates.find(
    ({ semantic_status: semanticStatus }) => semanticStatus === "confirmed-used",
  );
  if (!astroCandidate) fail("Astro discovery has no confirmed semantic refinement");
  const ledger = readJson(resolve(options.outDir, "ledger.json"), "corpus evidence ledger");
  const astroLedger = ledger.candidates.find(({ key }) => key === astroCandidate.key);
  const astroRoot = projectRoot(
    manifest.projects.find(({ id }) => id === "astro"),
    options.outDir,
  );
  const astroEvidence = [
    sourceLineEvidence(astroRoot, astroCandidate),
    ...arrayOrEmpty(Object(Object(astroLedger).source_evidence).uses).slice(0, 2),
  ];

  const supplementalArtifact = validateSupplementalArtifactData(
    readJson(SUPPLEMENTAL_ARTIFACT, "tracked supplemental artifact"),
    readJson(DEFAULT_ADJUDICATION, "adjudication decisions"),
    {
      fallowSha256: sha256File(options.fallowBin),
      sidecarSha256: sidecarArtifactHash(options.sidecarBin),
      sourceRoot: REPO_ROOT,
      dependencyEnvironment: publishDependencyEnvironment(
        fixtureDependencyEnvironment(SUPPLEMENTAL_VITEST_ROOT, "vitest"),
      ),
    },
  );
  const baseline = parseMachineOutput(
    readFileSync(resolve(DEFAULT_OUT_DIR, "supplemental/vitest-baseline.json"), "utf8"),
    "vitest",
  );
  const refined = parseMachineOutput(
    readFileSync(resolve(DEFAULT_OUT_DIR, "supplemental/vitest-refined.json"), "utf8"),
    "vitest",
  );
  const reviewedKeys = new Set(supplementalArtifact.independent_review.reviewed_candidate_keys);
  const vitestCandidate = compareCandidateSets(
    "vitest",
    baseline.candidates,
    refined.candidates,
    refined.typeAware,
  ).find(
    ({ key, semantic_status: semanticStatus }) =>
      semanticStatus === "confirmed-used" && reviewedKeys.has(key),
  );
  if (!vitestCandidate) fail("Vitest supplemental proof has no reviewed semantic refinement");

  return {
    astro: compactRefinement(
      astroCandidate,
      astro.confirmed_used_count,
      astroEvidence,
      Object(astroLedger).truth === "used",
    ),
    vitest: compactRefinement(
      vitestCandidate,
      supplementalArtifact.result.confirmed_used,
      [sourceLineEvidence(SUPPLEMENTAL_VITEST_ROOT, vitestCandidate)],
      true,
    ),
  };
};

const validateCapabilityEvidence = (root, evidence, capability) => {
  if (![Array.isArray(evidence), evidence.length > 0].every(Boolean)) {
    fail(`${capability} requires concrete source evidence`);
  }
  for (const location of evidence) {
    const current = sourceLocationEvidence(root, location);
    if (JSON.stringify(current) !== JSON.stringify(location)) {
      fail(`${capability} source evidence no longer matches the pinned source`);
    }
  }
};

/** Validates that all five semantic capabilities add non-compiler, non-linter value. */
export const validateCapabilitiesArtifactData = (artifact, context) => {
  return validateCapabilitiesData(artifact, context, {
    fail,
    isObject,
    requiredCapabilities: REQUIRED_SEMANTIC_CAPABILITIES,
    validateEvidence: validateCapabilityEvidence,
  });
};

const capabilityDependencyEnvironments = (roots) => {
  const environments = {};
  roots.forEach(([id, root]) => {
    const dirty = git(root, ["status", "--porcelain", "--untracked-files=all"], `${id} status`);
    if (dirty !== "") fail(`${id} capability fixture has tracked modifications`);
    assertNoFixtureDependencyDirectories(root, id);
    environments[id] = publishDependencyEnvironment(fixtureDependencyEnvironment(root, id));
  });
  return environments;
};

const capabilities = async (manifest, options, publicationMode = "write") => {
  ensureRuntimeInputs(options);
  prepareSupplementalFixture();
  const astroProject = manifest.projects.find(({ id }) => id === "astro");
  if (!astroProject) fail("capability proof requires the Astro corpus project");
  const astroRoot = projectRoot(astroProject, options.outDir);
  validatePreparedFixture(astroProject, options.outDir);
  const vitestCommit = git(
    SUPPLEMENTAL_VITEST_ROOT,
    ["rev-parse", "HEAD"],
    "Vitest capability fixture HEAD",
  );
  if (vitestCommit !== SUPPLEMENTAL_VITEST_COMMIT) {
    fail(`Vitest capability fixture is at ${vitestCommit}, expected ${SUPPLEMENTAL_VITEST_COMMIT}`);
  }
  const dependencyEnvironments = capabilityDependencyEnvironments([
    ["astro", astroRoot],
    ["vitest", SUPPLEMENTAL_VITEST_ROOT],
  ]);

  const astroInspect = await capabilityProcess(options, "astro-inspect", astroRoot, [
    "inspect",
    "--root",
    astroRoot,
    "--symbol",
    "packages/telemetry/src/index.ts:AstroTelemetry",
    "--type-aware",
    "--type-aware-project",
    "packages/telemetry/tsconfig.build.json",
    "--type-aware-project",
    "packages/telemetry/tsconfig.test.json",
  ]);
  const vitestInspect = await capabilityProcess(
    options,
    "vitest-inspect",
    SUPPLEMENTAL_VITEST_ROOT,
    [
      "inspect",
      "--root",
      SUPPLEMENTAL_VITEST_ROOT,
      "--symbol",
      "packages/vitest/src/public/config.ts:defineConfig",
      "--type-aware",
      "--type-aware-project",
      "packages/vitest/tsconfig.json",
      "--type-aware-project",
      "test/tsconfig.json",
    ],
  );
  const astroCoupling = await capabilityProcess(options, "astro-coupling", astroRoot, [
    "health",
    "--root",
    astroRoot,
    "--type-aware",
    "--type-coupling",
    "--type-aware-project",
    "packages/astro/tsconfig.build.json",
  ]);
  const vitestCoupling = await capabilityProcess(
    options,
    "vitest-coupling",
    SUPPLEMENTAL_VITEST_ROOT,
    [
      "health",
      "--root",
      SUPPLEMENTAL_VITEST_ROOT,
      "--type-aware",
      "--type-coupling",
      "--type-aware-project",
      "packages/vitest/tsconfig.json",
    ],
  );
  const refinements = refinementProofs(manifest, options);
  const astroSemantic = semanticCapabilityProof(
    astroRoot,
    astroInspect.output,
    astroCoupling.output,
  );
  const vitestSemantic = semanticCapabilityProof(
    SUPPLEMENTAL_VITEST_ROOT,
    vitestInspect.output,
    vitestCoupling.output,
  );
  const artifact = {
    schema_version: 3,
    purpose:
      "Prove semantic codebase-intelligence capabilities that are not compiler diagnostics or syntax/style lint rules.",
    excludes: ["compiler-diagnostics", "syntax-and-style-lint-rules"],
    artifacts: {
      fallow_sha256: sha256File(options.fallowBin),
      sidecar_sha256: sidecarArtifactHash(options.sidecarBin),
    },
    coverage: {
      capability_ids: REQUIRED_SEMANTIC_CAPABILITIES,
      repository_count: 2,
      all_capabilities_proven_on_each_repository: true,
    },
    repositories: [
      {
        id: "astro",
        repo: astroProject.repo,
        commit: git(astroRoot, ["rev-parse", "HEAD"], "Astro capability fixture HEAD"),
        tracked_source_clean: true,
        dependency_environment: dependencyEnvironments.astro,
        programs: astroSemantic.programs,
        capabilities: {
          "dead-code-refinement": refinements.astro,
          ...astroSemantic.capabilities,
        },
      },
      {
        id: "vitest",
        repo: "vitest-dev/vitest",
        commit: vitestCommit,
        tracked_source_clean: true,
        dependency_environment: dependencyEnvironments.vitest,
        programs: vitestSemantic.programs,
        capabilities: {
          "dead-code-refinement": refinements.vitest,
          ...vitestSemantic.capabilities,
        },
      },
    ],
  };
  validateCapabilitiesArtifactData(artifact, {
    fallowSha256: sha256File(options.fallowBin),
    sidecarSha256: sidecarArtifactHash(options.sidecarBin),
    roots: { astro: astroRoot, vitest: SUPPLEMENTAL_VITEST_ROOT },
    commits: {
      astro: artifact.repositories[0].commit,
      vitest: SUPPLEMENTAL_VITEST_COMMIT,
    },
    dependencyEnvironments,
  });
  persistTrackedJson(
    CAPABILITIES_ARTIFACT,
    "tracked capabilities artifact",
    artifact,
    publicationMode,
    "tracked semantic capabilities artifact has generator drift; rerun capabilities",
  );
  return artifact;
};

export const candidateFeatureBucketFields = (
  projectFeatureBuckets,
  previousEntry,
  previousSchemaVersion,
) => {
  return ledgerFeatureBucketFields(projectFeatureBuckets, previousEntry, previousSchemaVersion);
};

const LEDGER_REFRESH_RECOVERY =
  "Archive the old ledger or restore the discovery that created it, then retry.";

/** Validates that a ledger refresh cannot discard prior manual adjudication. */
export const indexLedgerForRefresh = (previous, discoveredKeys) => {
  return indexRefreshLedger(previous, discoveredKeys, LEDGER_REFRESH_RECOVERY, {
    fail,
    isObject,
  });
};

export const validateEvidenceProducerHash = (discovery, sidecarSha256) => {
  if (discovery.provenance?.sidecar?.sha256 !== sidecarSha256) {
    fail("evidence sidecar does not match discovery provenance; rerun discover first");
  }
};

export const evidenceProducerErrors = (
  discovery,
  producer,
  expectedRequestSetSha256,
  expectedResponseSetSha256,
) => {
  return validateEvidenceProducer(
    discovery,
    producer,
    expectedRequestSetSha256,
    expectedResponseSetSha256,
    { isObject, protocolVersion: SEMANTIC_PROTOCOL_VERSION },
  );
};

const expectedEvidenceRequestSetDigest = (discovery, projects, outDir) => {
  const selected = new Map(projects.map((project) => [project.id, project]));
  const digests = discovery.projects
    .filter((result) => selected.has(result.id))
    .map((result) => {
      const project = selected.get(result.id);
      const candidates = result.candidates.filter(
        ({ semantic_status: semanticStatus }) => semanticStatus === "confirmed-used",
      );
      const { request } = checkerEvidenceRequest(project, candidates, outDir);
      return `${project.id}:${requestDigest(request)}`;
    });
  return digestSet(digests);
};

const storedEvidenceResponseSetDigest = (discovery, outDir) =>
  digestSet(
    (discovery.projects ?? []).map((project) => {
      const path = resolve(outDir, "evidence", `${safeArtifactName(project.id)}.sidecar.json`);
      return `${project.id}:${normalizedEvidenceResponseDigest(
        readJson(path, `${project.id} raw evidence response`),
      )}`;
    }),
  );

const firstDefined = (values) => values.find((value) => value !== undefined);

const evidenceLedgerEntry = (
  project,
  candidate,
  root,
  checkerEvidence,
  previous,
  schemaVersion,
) => {
  const old = Object(previous);
  const oldEvidence = Object(old.source_evidence);
  const featureBuckets = candidateFeatureBucketFields(
    project.feature_buckets,
    previous,
    schemaVersion,
  );
  return {
    key: candidate.key,
    project_id: project.id,
    candidate: {
      path: candidate.path,
      parent_name: candidate.parent_name,
      member_name: candidate.member_name,
      kind: candidate.kind,
      line: candidate.line,
      col: candidate.col,
    },
    semantic_status: candidate.semantic_status,
    semantic_decision: candidate.semantic_decision,
    semantic_completeness: candidate.semantic_completeness,
    owning_projects: candidate.owning_projects,
    contract:
      candidate.contract == null
        ? null
        : {
            relation: candidate.contract.relation,
            optional: candidate.contract.optional,
            declaration: sourceLineEvidence(root, candidate.contract.declaration),
          },
    ...featureBuckets,
    truth: firstDefined([old.truth, "pending"]),
    source_evidence: {
      declaration: sourceLineEvidence(root, candidate),
      uses: firstDefined([checkerEvidence.get(candidate.key), oldEvidence.uses, []]),
      notes: firstDefined([oldEvidence.notes, null]),
    },
  };
};

const collectProjectEvidence = async (project, result, options, previousByKey, previousSchema) => {
  const confirmedCandidates = result.candidates.filter(
    ({ semantic_status: status }) => status === "confirmed-used",
  );
  const root = projectRoot(project, options.outDir);
  const checkerResult = await collectCheckerEvidence(
    project,
    confirmedCandidates,
    options.outDir,
    options.sidecarBin,
  );
  const evidenceKeys = [...checkerResult.evidence.keys()].toSorted();
  const expectedKeys = confirmedCandidates.map(({ key }) => key).toSorted();
  if (JSON.stringify(evidenceKeys) !== JSON.stringify(expectedKeys)) {
    fail(`${project.id} checker evidence does not match discovery; rerun discover first`);
  }
  return {
    entries: result.candidates.map((candidate) =>
      evidenceLedgerEntry(
        project,
        candidate,
        root,
        checkerResult.evidence,
        previousByKey.get(candidate.key),
        previousSchema,
      ),
    ),
    requestDigest: `${project.id}:${checkerResult.requestDigest}`,
    responseDigest: `${project.id}:${checkerResult.responseDigest}`,
  };
};

const readPreviousLedger = (ledgerPath) =>
  existsSync(ledgerPath) ? readJson(ledgerPath, "existing evidence ledger") : null;

const discoveryCandidateKeys = (discovery) =>
  new Set(discovery.projects.flatMap((result) => result.candidates.map((entry) => entry.key)));

const evidence = async (manifest, projects, options) => {
  ensureRuntimeInputs(options);
  const discovery = readDiscovery(options.outDir);
  const sidecarSha256 = sidecarArtifactHash(options.sidecarBin);
  validateEvidenceProducerHash(discovery, sidecarSha256);
  const selected = new Map(projects.map((project) => [project.id, project]));
  const ledgerPath = resolve(options.outDir, "ledger.json");
  const previous = readPreviousLedger(ledgerPath);
  const discoveredKeys = discoveryCandidateKeys(discovery);
  const previousByKey = indexLedgerForRefresh(previous, discoveredKeys);
  const entries = [];
  const requestDigests = [];
  const responseDigests = [];
  const selectedResults = discovery.projects.filter(({ id }) => selected.has(id));
  for (const result of selectedResults) {
    const project = selected.get(result.id);
    const collected = await collectProjectEvidence(
      project,
      result,
      options,
      previousByKey,
      Object(previous).schema_version,
    );
    entries.push(...collected.entries);
    requestDigests.push(collected.requestDigest);
    responseDigests.push(collected.responseDigest);
  }
  entries.sort((left, right) => left.key.localeCompare(right.key));
  const ledger = {
    schema_version: 2,
    artifact_policy: "local adjudication only; never copy raw or private source into tracked files",
    producer: {
      protocol_version: SEMANTIC_PROTOCOL_VERSION,
      sidecar_sha256: sidecarSha256,
      request_set_sha256: digestSet(requestDigests),
      response_set_sha256: digestSet(responseDigests),
    },
    adjudication: null,
    corpus: discovery.corpus,
    candidates: entries,
  };
  writeJson(ledgerPath, ledger);
  return ledger;
};

export const independentReviewDigest = (entries) => {
  const records = entries
    .map((entry) => ({
      key: entry.key,
      candidate: {
        path: entry.candidate.path,
        parent_name: entry.candidate.parent_name,
        member_name: entry.candidate.member_name,
        kind: entry.candidate.kind,
        line: entry.candidate.line,
        col: entry.candidate.col,
      },
      declaration: {
        path: entry.source_evidence.declaration.path,
        line: entry.source_evidence.declaration.line,
        col: entry.source_evidence.declaration.col,
        excerpt: entry.source_evidence.declaration.excerpt,
      },
      semantic_status: entry.semantic_status,
      contract:
        entry.contract == null
          ? null
          : {
              relation: entry.contract.relation,
              optional: entry.contract.optional,
              declaration: entry.contract.declaration,
            },
      uses: entry.source_evidence.uses
        .map(({ path: usePath, line, col, excerpt }) => ({
          path: usePath,
          line,
          col,
          excerpt,
        }))
        .toSorted((left, right) => JSON.stringify(left).localeCompare(JSON.stringify(right))),
    }))
    .toSorted((left, right) => left.key.localeCompare(right.key));
  return createHash("sha256")
    .update(`${JSON.stringify(records)}\n`)
    .digest("hex");
};

export const independentReviewErrors = (allEntries, reviews) => {
  return validateIndependentReviews(allEntries, reviews, {
    candidateSetDigest,
    independentReviewDigest,
  });
};

const validateDecisionReferences = (decisions, entriesByKey) => {
  Object.keys(Object(decisions.known_unused)).forEach((key) => {
    if (!entriesByKey.has(key)) fail(`adjudication references unknown unused candidate ${key}`);
  });
  Object.keys(Object(decisions.feature_overrides)).forEach((key) => {
    if (!entriesByKey.has(key)) fail(`adjudication references unknown feature candidate ${key}`);
  });
};

const adjudicateConfirmed = (entry, decisions) => {
  if (entry.source_evidence.uses.length === 0) {
    fail(`${entry.key} cannot be adjudicated used without concrete checker evidence`);
  }
  entry.truth = "used";
  entry.source_evidence.notes =
    "Exact declaration and checker-resolved source use reviewed against the pinned fixture.";
  entry.adjudicated_feature_buckets = firstDefined([
    Object(decisions.feature_overrides)[entry.key],
    [decisions.default_confirmed_bucket],
  ]);
};

const adjudicateContract = (entry) => {
  if (!isObject(entry.contract) || !isObject(entry.contract.declaration)) {
    fail(`${entry.key} cannot be adjudicated preserved without contract evidence`);
  }
  entry.truth = "preserved";
  entry.source_evidence.notes =
    "The implementation and inherited declarations were reviewed; deletion would break a required contract or remove overriding behavior.";
  entry.adjudicated_feature_buckets = [`contract-${entry.contract.relation}`];
};

const adjudicateRetained = (entry, knownUnused, decisions) => {
  if (knownUnused) {
    entry.truth = "unused";
    entry.source_evidence.notes = knownUnused;
    entry.adjudicated_feature_buckets = ["known-unused-retention"];
    return;
  }
  entry.truth = "indeterminate";
  entry.source_evidence.notes =
    "No exact checker confirmation; retained conservatively without claiming that the member is unused.";
  entry.adjudicated_feature_buckets = [decisions.default_retained_bucket];
};

const adjudicateEntry = (entry, decisions) => {
  if (entry.semantic_status === "confirmed-used") {
    adjudicateConfirmed(entry, decisions);
    return;
  }
  if (entry.semantic_status === "contract-preserved") {
    adjudicateContract(entry);
    return;
  }
  adjudicateRetained(entry, Object(decisions.known_unused)[entry.key], decisions);
};

const adjudicate = (outDir) => {
  const ledgerPath = resolve(outDir, "ledger.json");
  const ledger = readJson(ledgerPath, "evidence ledger");
  const decisions = readJson(DEFAULT_ADJUDICATION, "adjudication decisions");
  if (!isObject(decisions) || decisions.schema_version !== 1) {
    fail("adjudication decisions schema_version must be 1");
  }
  const entriesByKey = new Map(ledger.candidates.map((entry) => [entry.key, entry]));
  validateDecisionReferences(decisions, entriesByKey);
  const reviewErrors = independentReviewErrors(ledger.candidates, decisions.independent_reviews);
  if (reviewErrors.length > 0) fail(reviewErrors.join("\n"));
  ledger.candidates.forEach((entry) => adjudicateEntry(entry, decisions));
  ledger.adjudication = {
    decisions_sha256: sha256File(DEFAULT_ADJUDICATION),
    reviewed_on: decisions.reviewed_on,
    reviewer: decisions.reviewer,
    method: decisions.method,
    independent_signoff: {
      verdict: "approved",
      reviews: decisions.independent_reviews,
    },
  };
  writeJson(ledgerPath, ledger);
  return ledger;
};

const evidenceLocationValid = (location) =>
  validEvidenceLocation(location, { isAbsolute, isObject });

export const verifyLedgerData = (discovery, ledger) => {
  return verifyLedgerArtifact(discovery, ledger, {
    candidateFields,
    candidateKey,
    isAbsolute,
    isObject,
    truthStatuses: TRUTH_STATUSES,
  });
};

const evidenceSourcePath = (entry, location, root, errors) => {
  const sourcePath = resolve(root, location.path);
  const relativeToRoot = relative(root, sourcePath);
  if ([relativeToRoot.startsWith(`..${sep}`), relativeToRoot === ".."].some(Boolean)) {
    errors.push(`${entry.key}: evidence path escapes the prepared fixture`);
    return null;
  }
  const sourceIsFile = existsSync(sourcePath) ? statSync(sourcePath).isFile() : false;
  if (!sourceIsFile) {
    errors.push(`${entry.key}: evidence source file is missing: ${location.path}`);
    return null;
  }
  return sourcePath;
};

const verifyEvidenceExcerpt = (entry, location, sourcePath, errors) => {
  const sourceLine = readFileSync(sourcePath, "utf8").split(/\r?\n/)[location.line - 1] ?? "";
  if (sourceLine.trim() !== location.excerpt) {
    errors.push(`${entry.key}: evidence excerpt is stale at ${location.path}:${location.line}`);
  }
  if (location.col > Buffer.byteLength(sourceLine, "utf8")) {
    errors.push(`${entry.key}: evidence column is outside ${location.path}:${location.line}`);
  }
};

const verifyEvidenceLocation = (entry, location, root, errors) => {
  if (!evidenceLocationValid(location)) return;
  const sourcePath = evidenceSourcePath(entry, location, root, errors);
  if (sourcePath === null) return;
  verifyEvidenceExcerpt(entry, location, sourcePath, errors);
};

const verifyEntrySourceEvidence = (entry, projects, roots, errors) => {
  const project = projects.get(entry.project_id);
  if (!project) {
    errors.push(`${entry.key}: source project is not in the manifest`);
    return;
  }
  const sourceEvidence = Object(entry.source_evidence);
  const locations = [
    sourceEvidence.declaration,
    ...arrayOrEmpty(sourceEvidence.uses),
    ...(entry.contract == null ? [] : [Object(entry.contract).declaration]),
  ];
  locations.forEach((location) =>
    verifyEvidenceLocation(entry, location, roots.get(project.id), errors),
  );
};

const verifySourceEvidence = (manifest, ledger, outDir) => {
  const projects = new Map(manifest.projects.map((project) => [project.id, project]));
  const roots = new Map(
    manifest.projects.map((project) => [project.id, projectRoot(project, outDir)]),
  );
  const errors = [];
  ledger.candidates.forEach((entry) => verifyEntrySourceEvidence(entry, projects, roots, errors));
  return errors;
};

const verifyLedgerAdjudication = (ledger, decisions, errors) => {
  const adjudication = Object(ledger.adjudication);
  const signoff = Object(adjudication.independent_signoff);
  if (adjudication.decisions_sha256 !== sha256File(DEFAULT_ADJUDICATION)) {
    errors.push("ledger adjudication does not match the checked-in review decisions");
  }
  if (signoff.verdict !== "approved") {
    errors.push("ledger requires an approved independent signoff");
  }
  const storedReviews = signoff.reviews;
  errors.push(...independentReviewErrors(ledger.candidates, storedReviews));
  if (JSON.stringify(storedReviews) !== JSON.stringify(decisions.independent_reviews)) {
    errors.push("ledger independent reviews do not match the checked-in review decisions");
  }
};

const verifyLedger = (outDir, manifest, discovery = readDiscovery(outDir)) => {
  const ledger = readJson(resolve(outDir, "ledger.json"), "evidence ledger");
  const decisions = readJson(DEFAULT_ADJUDICATION, "adjudication decisions");
  if (JSON.stringify(discovery.corpus) !== JSON.stringify(ledger.corpus)) {
    fail("ledger corpus identity does not match discovery; refresh evidence");
  }
  const errors = [
    ...verifyLedgerData(discovery, ledger),
    ...evidenceProducerErrors(
      discovery,
      ledger.producer,
      expectedEvidenceRequestSetDigest(discovery, manifest.projects, outDir),
      storedEvidenceResponseSetDigest(discovery, outDir),
    ),
    ...verifySourceEvidence(manifest, ledger, outDir),
  ].toSorted();
  verifyLedgerAdjudication(ledger, decisions, errors);
  if (errors.length > 0)
    fail(`ledger verification failed:\n${errors.map((error) => `- ${error}`).join("\n")}`);
  return { discovery, ledger };
};

const measuredRuns = (measurements, mode) =>
  measurements.projects.flatMap((project) =>
    project.runs.filter((run) => !run.warmup && run.mode === mode),
  );

const focusedCasesPassed = (focusedReport, discovery) =>
  [
    focusedReport.schema_version === 1,
    focusedReport.passed === true,
    focusedReport.fallow_sha256 === discovery.provenance.fallow.sha256,
    focusedReport.sidecar_sha256 === discovery.provenance.sidecar.sha256,
    focusedReport.test_sha256 === sha256File(FOCUSED_TEST_PATH),
    JSON.stringify(focusedReport.cases) === JSON.stringify([...REQUIRED_FOCUSED_CASES].toSorted()),
  ].every(Boolean);

const addSupplementalConfirmation = (confirmedProjects, artifact) => {
  if (
    [artifact.result.confirmed_used > 0, artifact.independent_review.verdict === "approved"].every(
      Boolean,
    )
  ) {
    confirmedProjects.add("vitest");
  }
};

const publicationEvidence = (
  summary,
  discovery,
  ledger,
  correctConfirmed,
  correctContracts,
  retainedUnused,
) => ({
  schema_version: 1,
  generated_from: {
    discovery_provenance: discovery.provenance,
    evidence_producer: ledger.producer,
    adjudication: ledger.adjudication,
  },
  summary,
  confirmations: correctConfirmed.map((entry) => ({
    key: entry.key,
    project_id: entry.project_id,
    declaration: {
      path: entry.source_evidence.declaration.path,
      line: entry.source_evidence.declaration.line,
      col: entry.source_evidence.declaration.col,
    },
    uses: entry.source_evidence.uses
      .map(({ path: usePath, line, col }) => ({ path: usePath, line, col }))
      .toSorted((left, right) => JSON.stringify(left).localeCompare(JSON.stringify(right))),
    feature_buckets: entry.adjudicated_feature_buckets,
  })),
  contract_preservations: correctContracts.map((entry) => ({
    key: entry.key,
    project_id: entry.project_id,
    declaration: {
      path: entry.source_evidence.declaration.path,
      line: entry.source_evidence.declaration.line,
      col: entry.source_evidence.declaration.col,
    },
    contract: entry.contract,
  })),
  known_unused_retained: retainedUnused.map((entry) => ({
    key: entry.key,
    project_id: entry.project_id,
    declaration: {
      path: entry.source_evidence.declaration.path,
      line: entry.source_evidence.declaration.line,
      col: entry.source_evidence.declaration.col,
    },
  })),
});

const persistPublicationEvidence = (summary, publicationArtifact, publicationMode) => {
  if (!summary.gate.go) return;
  const evidencePath = resolve(REPO_ROOT, "benchmarks/type-aware-corpus-evidence.json");
  const summaryPath = resolve(REPO_ROOT, "benchmarks/type-aware-corpus-summary.md");
  const summaryMarkdown = renderSummaryMarkdown(summary);
  if (publicationMode === "write") {
    writeJson(evidencePath, publicationArtifact);
    writeFileSync(summaryPath, summaryMarkdown);
    return;
  }
  const trackedMatches = [
    JSON.stringify(readJson(evidencePath, "tracked corpus evidence")) ===
      JSON.stringify(publicationArtifact),
    readFileSync(summaryPath, "utf8") === summaryMarkdown,
  ].every(Boolean);
  if (!trackedMatches) {
    fail("tracked corpus evidence or summary has generator drift; rerun summarize");
  }
};

export const validateMeasurements = (discovery, measurements) => {
  validateMeasurementData(discovery, measurements, {
    fail,
    isObject,
    platform: process.platform,
  });
};

const validatePublicationProvenance = (discovery, manifest, options) => {
  ensureRuntimeInputs(options);
  const expected = currentPublicationProvenance(options.manifest, manifest.projects, options);
  const provenance = Object(discovery.provenance);
  const actual = {
    manifest_sha256: provenance.manifest_sha256,
    fallow: provenance.fallow,
    sidecar: provenance.sidecar,
    runtime: provenance.runtime,
    dependency_environments: provenance.dependency_environments,
    fixtures: provenance.fixtures,
  };
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    fail("discovery provenance does not match current release artifacts and pinned fixtures");
  }
};

export const requirePublicationGo = (summary, publicationMode) => {
  assertPublicationGo(summary, publicationMode, fail);
};

const summarize = (manifest, options, publicationMode = "write") => {
  const outDir = options.outDir;
  const discovery = readDiscovery(outDir);
  requireCompletePublicationCorpus(manifest, discovery);
  validatePublicationProvenance(discovery, manifest, options);
  const { ledger } = verifyLedger(outDir, manifest, discovery);
  const measurements = readJson(resolve(outDir, "measurements.json"), "measurements artifact");
  validateMeasurements(discovery, measurements);
  const focusedReport = readJson(resolve(outDir, "focused.json"), "focused verification artifact");
  const focusedVerificationPassed = focusedCasesPassed(focusedReport, discovery);
  const entries = ledger.candidates;
  const confirmed = entries.filter(({ semantic_status }) => semantic_status === "confirmed-used");
  const contracts = entries.filter(
    ({ semantic_status }) => semantic_status === "contract-preserved",
  );
  const retained = entries.filter(({ semantic_status }) => semantic_status === "retained");
  const used = entries.filter(({ truth }) => truth === "used");
  const preserved = entries.filter(({ truth }) => truth === "preserved");
  const unused = entries.filter(({ truth }) => truth === "unused");
  const correctConfirmed = confirmed.filter(({ truth }) => truth === "used");
  const incorrectConfirmed = confirmed.filter(({ truth }) => truth === "unused");
  const correctContracts = contracts.filter(({ truth }) => truth === "preserved");
  const incorrectContracts = contracts.filter(({ truth }) => truth !== "preserved");
  const correctUnusedRetained = retained.filter(({ truth }) => truth === "unused");
  const abstainedCount = discovery.projects.reduce(
    (total, project) => total + (project.type_aware?.abstained_count ?? 0),
    0,
  );
  const unresolvedCount = discovery.projects.reduce(
    (total, project) => total + (project.type_aware?.unresolved_count ?? 0),
    0,
  );
  const baselineRuns = measuredRuns(measurements, "baseline");
  const refinedRuns = measuredRuns(measurements, "refined");
  const overheads = pairedOverheads(measurements.projects);
  const confirmedProjects = new Set(
    [...correctConfirmed, ...correctContracts].map(({ project_id }) => project_id),
  );
  const adjudication = readJson(DEFAULT_ADJUDICATION, "adjudication decisions");
  const supplementalArtifact = validateSupplementalArtifactData(
    readJson(SUPPLEMENTAL_ARTIFACT, "tracked supplemental artifact"),
    adjudication,
    {
      fallowSha256: discovery.provenance.fallow.sha256,
      sidecarSha256: discovery.provenance.sidecar.sha256,
      sourceRoot: REPO_ROOT,
      dependencyEnvironment: publishDependencyEnvironment(
        fixtureDependencyEnvironment(SUPPLEMENTAL_VITEST_ROOT, "vitest"),
      ),
    },
  );
  addSupplementalConfirmation(confirmedProjects, supplementalArtifact);
  const featureBucketValue = summarizeAdjudicatedFeatureBuckets(entries);
  const zeroControlsClean = discovery.projects
    .filter(({ role }) => role === "zero-control")
    .every(({ baseline_candidate_count }) => baseline_candidate_count === 0);
  const confirmationPrecision = safeRatio(correctConfirmed.length, confirmed.length);
  const confirmationYield = safeRatio(correctConfirmed.length, entries.length);
  const adjudicatedTruthCount = used.length + preserved.length + unused.length;
  const adjudicatedTruthCoverage = safeRatio(adjudicatedTruthCount, entries.length);
  const correctUnusedRetention = safeRatio(correctUnusedRetained.length, unused.length);
  const abstention = safeRatio(abstainedCount, entries.length);
  const marginalOverheadP95 = percentile(overheads, 0.95);
  const refinedRssP95 = percentile(
    refinedRuns
      .map(({ peak_process_tree_rss_kb }) => peak_process_tree_rss_kb)
      .filter(Number.isFinite),
    0.95,
  );
  const thresholds = manifest.gates;
  const independentSignoff = Object(ledger.adjudication).independent_signoff;
  const checks = summaryChecks({
    abstention,
    confirmationPrecision,
    confirmationYield,
    confirmedProjectCount: confirmedProjects.size,
    correctUnusedRetention,
    determinismRuns: discovery.determinism_runs,
    featureBucketValue,
    focusedCasesPassed: focusedVerificationPassed,
    incorrectConfirmedCount: incorrectConfirmed.length + incorrectContracts.length,
    independentSignoff,
    isObject,
    marginalOverheadP95,
    platform: process.platform,
    refinedRssP95,
    thresholds,
    zeroControlsClean,
  });
  const summary = {
    schema_version: 3,
    gate: {
      go: Object.values(checks).every(Boolean),
      checks,
      thresholds,
      known_incorrect_removals: incorrectConfirmed.length + incorrectContracts.length,
      zero_controls_clean: zeroControlsClean,
      multiple_repositories: confirmedProjects.size >= 2,
      multiple_feature_buckets: featureBucketValue.multiple_feature_buckets,
    },
    accuracy: {
      candidate_count: entries.length,
      confirmed_used_count: confirmed.length,
      contract_preserved_count: contracts.length,
      retained_count: retained.length,
      truth_counts: {
        used: used.length,
        preserved: preserved.length,
        unused: unused.length,
        indeterminate: entries.filter(({ truth }) => truth === "indeterminate").length,
      },
      confirmation_precision: confirmationPrecision,
      confirmation_yield: confirmationYield,
      safe_resolution_yield: safeRatio(
        correctConfirmed.length + correctContracts.length,
        entries.length,
      ),
      adjudicated_truth_count: adjudicatedTruthCount,
      adjudicated_truth_coverage: adjudicatedTruthCoverage,
      adjudication_scope:
        "Confirmed uses, required contracts, and reviewed declaration-only negatives; indeterminate candidates are excluded and corpus-wide recall is not claimed.",
      correct_unused_retention: correctUnusedRetention,
      abstained_count: abstainedCount,
      abstention,
      unresolved_count: unresolvedCount,
      unresolved: safeRatio(unresolvedCount, entries.length),
    },
    value: {
      confirmed_repository_count: confirmedProjects.size,
      confirmed_repositories: [...confirmedProjects].toSorted(),
      confirmed_feature_buckets: featureBucketValue.confirmed_feature_buckets,
    },
    performance: {
      baseline_wall_ms: {
        median: percentile(
          baselineRuns.map(({ wall_ms }) => wall_ms),
          0.5,
        ),
        p95: percentile(
          baselineRuns.map(({ wall_ms }) => wall_ms),
          0.95,
        ),
      },
      refined_wall_ms: {
        median: percentile(
          refinedRuns.map(({ wall_ms }) => wall_ms),
          0.5,
        ),
        p95: percentile(
          refinedRuns.map(({ wall_ms }) => wall_ms),
          0.95,
        ),
      },
      marginal_overhead_ms: {
        median: percentile(overheads, 0.5),
        p95: marginalOverheadP95,
      },
      process_tree_peak_rss_kb: {
        baseline_p95: percentile(
          baselineRuns
            .map(({ peak_process_tree_rss_kb }) => peak_process_tree_rss_kb)
            .filter(Number.isFinite),
          0.95,
        ),
        refined_p95: refinedRssP95,
      },
      program_counts: refinedRuns
        .map(({ program_count }) => program_count)
        .filter(Number.isSafeInteger),
      source_files_per_program: refinedRuns.flatMap(
        ({ source_files_per_program }) => source_files_per_program ?? [],
      ),
      phase_timings_ms: aggregatePhaseTimings(refinedRuns),
      reason_counts: aggregateReasonCounts(refinedRuns),
    },
  };
  requirePublicationGo(summary, publicationMode);
  writeJson(resolve(outDir, "summary.json"), summary);
  writeFileSync(resolve(outDir, "summary.md"), renderSummaryMarkdown(summary));
  const publicationArtifact = publicationEvidence(
    summary,
    discovery,
    ledger,
    correctConfirmed,
    correctContracts,
    correctUnusedRetained,
  );
  persistPublicationEvidence(summary, publicationArtifact, publicationMode);
  return summary;
};

const verifyPublication = async (manifest, options) => {
  summarize(manifest, options, "verify");
  await supplemental(options, "verify");
  await capabilities(manifest, options, "verify");
};

const executeCommand = async (manifest, projects, options) => {
  const handlers = {
    prepare: () => prepare(projects, options.outDir),
    discover: () => discover(manifest, projects, options),
    measure: () => measure(manifest, projects, options),
    focused: () => focused(options),
    evidence: () => evidence(manifest, manifest.projects, options),
    adjudicate: () => adjudicate(options.outDir),
    "verify-ledger": () => verifyLedger(options.outDir, manifest),
    summarize: () => summarize(manifest, options),
    "verify-publication": () => verifyPublication(manifest, options),
    supplemental: () => supplemental(options),
    capabilities: () => capabilities(manifest, options),
  };
  await handlers[options.command]();
};

const successMessage = (options) => {
  const messages = {
    "verify-ledger": "verify-ledger: evidence ledger is valid",
    "verify-publication": "verify-publication: tracked evidence matches the generator",
    supplemental: `supplemental: artifact written to ${SUPPLEMENTAL_ARTIFACT}`,
    capabilities: `capabilities: artifact written to ${CAPABILITIES_ARTIFACT}`,
  };
  return messages[options.command] ?? `${options.command}: artifacts written to ${options.outDir}`;
};

export const main = async (argv = process.argv.slice(2)) => {
  const options = parseArgs(argv);
  if (options.command === "help") {
    console.log(usage());
    return 0;
  }
  const manifest = loadManifest(options.manifest);
  const projects = selectProjects(manifest, options.projects);
  validatePartialOutput(options);
  mkdirSync(options.outDir, { recursive: true });
  await executeCommand(manifest, projects, options);
  console.log(successMessage(options));
  return 0;
};

if (import.meta.url === `file://${process.argv[1]}`) {
  main().then(
    (code) => {
      process.exitCode = code;
      return code;
    },
    (error) => {
      console.error(error instanceof Error ? error.message : String(error));
      process.exitCode = 1;
      return 1;
    },
  );
}
