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
const DEFAULT_RUNS = 4;
const DEFAULT_WARMUPS = 1;
const DEFAULT_DISCOVERY_RUNS = 2;
const MAX_OUTPUT_BYTES = 256 * 1024 * 1024;
const TRUTH_STATUSES = new Set(["used", "unused", "indeterminate"]);
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
  "treats getter and setter declarations as one logical property",
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
  if (!isObject(manifest) || manifest.schema_version !== 1) {
    fail("manifest schema_version must be 1");
  }
  if (manifest.artifact_directory !== "target/type-aware-corpus") {
    fail("manifest artifact_directory must be target/type-aware-corpus");
  }
  const requiredGates = [
    "minimum_confirmation_precision",
    "minimum_used_recall",
    "minimum_correct_unused_retention",
    "maximum_abstention",
    "maximum_p95_marginal_overhead_ms",
    "maximum_p95_refined_rss_kb",
  ];
  if (!isObject(manifest.gates)) fail("manifest gates must be an object");
  for (const gate of requiredGates) {
    if (!Number.isFinite(manifest.gates[gate]) || manifest.gates[gate] < 0) {
      fail(`manifest gates.${gate} must be a non-negative number`);
    }
  }
  if (!Array.isArray(manifest.projects) || manifest.projects.length !== REQUIRED_PROJECTS.size) {
    fail(`manifest must contain exactly ${REQUIRED_PROJECTS.size} corpus projects`);
  }

  const seen = new Set();
  for (const project of manifest.projects) {
    if (!isObject(project)) fail("every manifest project must be an object");
    nonEmptyString(project.id, "project.id");
    if (seen.has(project.id)) fail(`duplicate project id: ${project.id}`);
    seen.add(project.id);
    const expectedRole = REQUIRED_PROJECTS.get(project.id);
    if (!expectedRole) fail(`unexpected corpus project: ${project.id}`);
    if (project.role !== expectedRole) {
      fail(`${project.id} role must be ${expectedRole}`);
    }
    nonEmptyString(project.label, `${project.id}.label`);
    nonEmptyString(project.repo, `${project.id}.repo`);
    nonEmptyString(project.ref, `${project.id}.ref`);
    if (!/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(project.repo)) {
      fail(`${project.id}.repo must be a public owner/repository slug`);
    }
    const fixture = normalizedRelativePath(project.fixture, `${project.id}.fixture`);
    const expectedFixture = `benchmarks/fixtures/real-world/${project.id}`;
    if (fixture !== expectedFixture) {
      fail(`${project.id}.fixture must be ${expectedFixture}`);
    }
    const expectedCandidates = project.role === "zero-control" ? "zero" : "nonzero";
    if (project.candidate_expectation !== expectedCandidates) {
      fail(`${project.id}.candidate_expectation must be ${expectedCandidates}`);
    }
    if (
      !Array.isArray(project.feature_buckets) ||
      project.feature_buckets.some((bucket) => typeof bucket !== "string" || bucket.trim() === "")
    ) {
      fail(`${project.id}.feature_buckets must be an array of non-empty strings`);
    }
    if (project.role === "accuracy-core" && project.feature_buckets.length === 0) {
      fail(`${project.id} must declare at least one semantic feature bucket`);
    }
    if (new Set(project.feature_buckets).size !== project.feature_buckets.length) {
      fail(`${project.id}.feature_buckets must be unique`);
    }
  }

  for (const id of REQUIRED_PROJECTS.keys()) {
    if (!seen.has(id)) fail(`manifest is missing required project: ${id}`);
  }
  return manifest;
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
  const command = argv[0];
  if (!command || command === "--help" || command === "-h") {
    return { command: "help" };
  }
  if (
    !new Set([
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
    ]).has(command)
  ) {
    fail(`unknown command: ${command}`);
  }
  const options = {
    command,
    manifest: DEFAULT_MANIFEST,
    outDir: DEFAULT_OUT_DIR,
    fallowBin: DEFAULT_FALLOW_BIN,
    sidecarBin: null,
    runs: DEFAULT_RUNS,
    warmups: DEFAULT_WARMUPS,
    discoveryRuns: DEFAULT_DISCOVERY_RUNS,
    projects: [],
    outDirExplicit: false,
  };
  for (let index = 1; index < argv.length; index += 1) {
    const option = argv[index];
    if (option === "--manifest") {
      options.manifest = resolve(readOptionValue(argv, index, option));
      index += 1;
    } else if (option === "--fallow-bin") {
      options.fallowBin = resolve(readOptionValue(argv, index, option));
      index += 1;
    } else if (option === "--sidecar-bin") {
      options.sidecarBin = resolve(readOptionValue(argv, index, option));
      index += 1;
    } else if (option === "--out-dir") {
      options.outDir = resolve(readOptionValue(argv, index, option));
      options.outDirExplicit = true;
      index += 1;
    } else if (option === "--discovery-runs") {
      options.discoveryRuns = parsePositiveInteger(readOptionValue(argv, index, option), option);
      index += 1;
    } else if (option === "--runs") {
      options.runs = parsePositiveInteger(readOptionValue(argv, index, option), option);
      index += 1;
    } else if (option === "--warmups") {
      options.warmups = parsePositiveInteger(readOptionValue(argv, index, option), option);
      index += 1;
    } else if (option === "--project") {
      options.projects.push(readOptionValue(argv, index, option));
      index += 1;
    } else {
      fail(`unknown option: ${option}`);
    }
  }
  return options;
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

Options:
  --manifest PATH     Default: benchmarks/type-aware-corpus.json
  --project ID        Select one project, repeatable
  --fallow-bin PATH   Default: target/release/fallow
  --sidecar-bin PATH  Required for discover, measure, focused, summarize, verify-publication, and supplemental
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
  if (
    options.projects.length > 0 &&
    new Set(["prepare", "discover", "measure"]).has(options.command) &&
    (!options.outDirExplicit || resolve(options.outDir) === DEFAULT_OUT_DIR)
  ) {
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

export const assertDependencyFreeFixture = (root, projectId, ignoredPaths = null) => {
  const ignored =
    ignoredPaths ??
    git(
      root,
      ["ls-files", "--others", "--ignored", "--exclude-standard", "--directory"],
      `${projectId} ignored dependency directory scan`,
    ).split("\n");
  const dependencyDirectories = ignored
    .map((entry) => entry.replaceAll("\\", "/").replace(/\/$/, ""))
    .filter((entry) =>
      entry.split("/").some((component) => DEPENDENCY_DIRECTORY_NAMES.has(component)),
    );
  for (const name of DEPENDENCY_DIRECTORY_NAMES) {
    if (existsSync(resolve(root, name))) dependencyDirectories.push(name);
  }
  const unique = [...new Set(dependencyDirectories)].toSorted();
  if (unique.length > 0) {
    fail(`${projectId} prepared fixture contains dependency directories: ${unique.join(", ")}`);
  }
};

const sourceFixtureRoot = (project) => {
  const root = resolve(REPO_ROOT, project.fixture);
  const relativeToRepo = relative(REPO_ROOT, root);
  if (relativeToRepo.startsWith(`..${sep}`) || relativeToRepo === "..") {
    fail(`${project.id} fixture resolves outside the repository`);
  }
  if (!existsSync(root) || !statSync(root).isDirectory()) {
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
  if (!existsSync(root) || !statSync(root).isDirectory()) {
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
  assertDependencyFreeFixture(root, project.id);
  return { root, commit: actualCommit };
};

const prepare = (projects, outDir) => {
  const prepared = [];
  for (const project of projects) {
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
    if (existsSync(preparedModules) && lstatSync(preparedModules).isSymbolicLink()) {
      unlinkSync(preparedModules);
    }
    const validated = validatePreparedFixture(project, outDir);
    prepared.push({
      id: project.id,
      repo: project.repo,
      ref: project.ref,
      commit: validated.commit,
    });
  }
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
  const children = new Map();
  const rssByPid = new Map();
  for (const [pid, ppid, rss] of rows) {
    rssByPid.set(pid, rss);
    const siblings = children.get(ppid) ?? [];
    siblings.push(pid);
    children.set(ppid, siblings);
  }
  const pending = [rootPid];
  const seen = new Set();
  let total = 0;
  while (pending.length > 0) {
    const pid = pending.pop();
    if (seen.has(pid)) continue;
    seen.add(pid);
    total += rssByPid.get(pid) ?? 0;
    pending.push(...(children.get(pid) ?? []));
  }
  return total;
};

const runProcess = (binary, args, cwd, sidecarBin) =>
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
    const sampler = setInterval(() => {
      const rss = descendantsRssKb(child.pid);
      if (rss !== null) peakRssKb = Math.max(peakRssKb, rss);
    }, 20);
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
  const candidates =
    output.unused_class_members ??
    output.check?.unused_class_members ??
    output.results?.unused_class_members;
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
  for (const field of ["line", "col"]) {
    if (!Number.isSafeInteger(fields[field]) || fields[field] < 0) {
      fail(`candidate.${field} must be a non-negative integer`);
    }
  }
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

const typeAwareMeta = (output) => output._meta?.type_aware ?? output.meta?.type_aware ?? null;

const parseMachineOutput = (stdout, projectId) => {
  let output;
  try {
    output = JSON.parse(stdout);
  } catch (error) {
    fail(
      `${projectId} emitted invalid JSON: ${error instanceof Error ? error.message : String(error)}`,
    );
  }
  if (!isObject(output) || (output.kind !== undefined && output.kind !== "dead-code")) {
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

const executeFallow = async ({ project, fallowBin, sidecarBin, refined, rawPath, outDir }) => {
  const root = projectRoot(project, outDir);
  const result = await runProcess(fallowBin, fallowArgs(root, refined), REPO_ROOT, sidecarBin);
  mkdirSync(dirname(rawPath), { recursive: true });
  writeFileSync(rawPath, result.stdout);
  writeFileSync(rawPath.replace(/\.json$/, ".stderr.txt"), result.stderr);
  if (result.status === null || result.status >= 2) {
    let detail = result.stderr.trim();
    try {
      const errorOutput = JSON.parse(result.stdout);
      if (typeof errorOutput.message === "string") detail = errorOutput.message;
    } catch {
      // The normal parse error below reports malformed stdout after successful exits.
    }
    fail(
      `${project.id} ${refined ? "refined" : "baseline"} run failed (${result.status ?? result.signal}): ${detail.slice(0, 2_000)}`,
    );
  }
  const parsed = parseMachineOutput(result.stdout, project.id);
  return { ...result, ...parsed };
};

const validateCandidateExpectation = (project, count) => {
  if (project.candidate_expectation === "zero" && count !== 0) {
    fail(`${project.id} zero-candidate control emitted ${count} candidates`);
  }
  if (project.candidate_expectation === "nonzero" && count === 0) {
    fail(`${project.id} accuracy-core project emitted no candidates`);
  }
};

const compareCandidateSets = (projectId, baseline, refined) => {
  const refinedKeys = new Set(refined.map((candidate) => candidate.key));
  const baselineKeys = new Set(baseline.map((candidate) => candidate.key));
  const additions = [...refinedKeys].filter((key) => !baselineKeys.has(key));
  if (additions.length > 0) {
    fail(`${projectId} refined output added candidates: ${additions.join(", ")}`);
  }
  return baseline.map((candidate) => ({
    ...candidate,
    semantic_status: refinedKeys.has(candidate.key) ? "retained" : "confirmed-used",
  }));
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

const buildProvenance = (manifestPath, projects, options) => ({
  generated_at: new Date().toISOString(),
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
  fixtures: projects.map((project) => {
    const { commit } = validatePreparedFixture(project, options.outDir);
    return { id: project.id, repo: project.repo, ref: project.ref, commit };
  }),
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

const discover = async (manifest, projects, options) => {
  ensureRuntimeInputs(options);
  const results = [];
  for (const project of projects) {
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
        rawPath: resolve(
          options.outDir,
          "discover",
          `${artifactProject}-${iteration + 1}-baseline.json`,
        ),
        outDir: options.outDir,
      });
      const currentRefined = await executeFallow({
        project,
        fallowBin: options.fallowBin,
        sidecarBin: options.sidecarBin,
        refined: true,
        rawPath: resolve(
          options.outDir,
          "discover",
          `${artifactProject}-${iteration + 1}-refined.json`,
        ),
        outDir: options.outDir,
      });
      const normalizedBaseline = normalizedRefinement(currentBaseline);
      const normalizedRefined = normalizedRefinement(currentRefined);
      if (expectedBaseline)
        requireSameNormalizedRun(project.id, "baseline", expectedBaseline, normalizedBaseline);
      if (expectedRefined)
        requireSameNormalizedRun(project.id, "refined", expectedRefined, normalizedRefined);
      expectedBaseline = normalizedBaseline;
      expectedRefined = normalizedRefined;
      baseline ??= currentBaseline;
      refined ??= currentRefined;
    }
    validateCandidateExpectation(project, baseline.candidates.length);
    const candidates = compareCandidateSets(project.id, baseline.candidates, refined.candidates);
    const metaCount = refined.typeAware?.candidate_count;
    if (Number.isSafeInteger(metaCount) && metaCount !== baseline.candidates.length) {
      fail(`${project.id} type-aware candidate_count does not match baseline output`);
    }
    results.push({
      id: project.id,
      role: project.role,
      feature_buckets: project.feature_buckets,
      baseline_candidate_count: baseline.candidates.length,
      refined_candidate_count: refined.candidates.length,
      confirmed_used_count: candidates.filter(
        ({ semantic_status }) => semantic_status === "confirmed-used",
      ).length,
      type_aware: refined.typeAware
        ? {
            protocol_version: refined.typeAware.protocol_version,
            sidecar_version: refined.typeAware.sidecar_version,
            backend_version: refined.typeAware.backend_version,
            confirmed_used_count: refined.typeAware.confirmed_used_count,
            unresolved_count: refined.typeAware.unresolved_count,
            abstained_count: refined.typeAware.abstained_count,
            abstention_reasons: refined.typeAware.abstention_reasons,
            elapsed_ms: refined.typeAware.elapsed_ms,
            phase_timings_ms: refined.typeAware.phase_timings_ms,
            projects: refined.typeAware.projects,
          }
        : null,
      candidates,
    });
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

const compactRun = (run, mode, iteration, warmup) => {
  const meta = run.typeAware;
  const projects = Array.isArray(meta?.projects) ? meta.projects : [];
  return {
    iteration,
    warmup,
    mode,
    wall_ms: run.wall_ms,
    peak_process_tree_rss_kb: run.peak_process_tree_rss_kb,
    candidate_count: run.candidates.length,
    refinement_ms: Number.isFinite(meta?.elapsed_ms) ? meta.elapsed_ms : null,
    program_count: Number.isSafeInteger(meta?.program_count)
      ? meta.program_count
      : projects.length || null,
    source_files_per_program: Array.isArray(meta?.source_files_per_program)
      ? meta.source_files_per_program
      : projects.map(({ source_file_count }) => source_file_count).filter(Number.isSafeInteger),
    phase_timings_ms: isObject(meta?.phase_timings_ms)
      ? meta.phase_timings_ms
      : isObject(meta?.phase_timings)
        ? meta.phase_timings
        : {},
    reason_counts: isObject(meta?.reason_counts)
      ? meta.reason_counts
      : isObject(meta?.abstention_reasons)
        ? meta.abstention_reasons
        : isObject(meta?.candidate_status_summary?.reason_counts)
          ? meta.candidate_status_summary.reason_counts
          : {},
  };
};

const measure = async (manifest, projects, options) => {
  ensureRuntimeInputs(options);
  const projectReports = [];
  for (const project of projects) {
    const projectRuns = [];
    const artifactProject = safeArtifactName(project.id);
    const phases = [
      ...Array.from({ length: options.warmups }, (_, iteration) => ({ warmup: true, iteration })),
      ...Array.from({ length: options.runs }, (_, iteration) => ({ warmup: false, iteration })),
    ];
    let expectedBaselineKeys = null;
    let expectedRefined = null;
    for (const phase of phases) {
      for (const mode of runOrderForIteration(phase.iteration)) {
        const refined = mode === "refined";
        const kind = phase.warmup ? "warmup" : "measured";
        const rawPath = resolve(
          options.outDir,
          "measure",
          artifactProject,
          `${kind}-${phase.iteration + 1}-${mode}.json`,
        );
        const run = await executeFallow({
          project,
          fallowBin: options.fallowBin,
          sidecarBin: options.sidecarBin,
          refined,
          rawPath,
          outDir: options.outDir,
        });
        if (!refined) {
          validateCandidateExpectation(project, run.candidates.length);
          const keys = run.candidates.map(({ key }) => key);
          if (
            expectedBaselineKeys &&
            JSON.stringify(keys) !== JSON.stringify(expectedBaselineKeys)
          ) {
            fail(`${project.id} baseline candidate keys changed between runs`);
          }
          expectedBaselineKeys = keys;
        } else if (expectedBaselineKeys) {
          const unexpected = run.candidates.filter(
            ({ key }) => !expectedBaselineKeys.includes(key),
          );
          if (unexpected.length > 0) fail(`${project.id} refined run added unexpected candidates`);
          const normalized = normalizedRefinement(run);
          if (expectedRefined)
            requireSameNormalizedRun(
              project.id,
              "refined measurement",
              expectedRefined,
              normalized,
            );
          expectedRefined = normalized;
        }
        projectRuns.push(compactRun(run, mode, phase.iteration, phase.warmup));
      }
    }
    projectReports.push({
      id: project.id,
      role: project.role,
      run_order: projectRuns.map(({ iteration, warmup, mode }) => ({ iteration, warmup, mode })),
      runs: projectRuns,
    });
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

const runSolutionConfigCliSmoke = (options) => {
  ensureFile(options.fallowBin, "fallow binary");
  const root = mkdtempSync(resolve(tmpdir(), "fallow-type-aware-solution-"));
  try {
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
    if (result.status !== 0 && result.status !== 1) {
      fail(`solution-tsconfig full CLI smoke failed: ${(result.stderr || result.stdout).trim()}`);
    }
    const output = JSON.parse(result.stdout);
    const metadata = output._meta?.type_aware;
    if (
      output.unused_class_members?.length !== 1 ||
      metadata?.selected_tsconfigs?.length !== 0 ||
      metadata?.projects?.length !== 0 ||
      metadata?.abstained_count !== 1 ||
      metadata?.abstention_reasons?.no_project !== 1
    ) {
      fail(
        `solution-tsconfig full CLI smoke did not fail closed with no-project abstention: ${JSON.stringify(
          {
            findings: output.unused_class_members?.length,
            metadata,
          },
        )}`,
      );
    }
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
};

const focused = (options) => {
  if (!options.sidecarBin) fail("--sidecar-bin is required for focused verification");
  ensureFile(options.sidecarBin, "type-aware sidecar");
  const sidecarRoot = resolve(REPO_ROOT, "tools/type-aware-sidecar");
  const result = spawnSync(process.execPath, ["--test", FOCUSED_TEST_PATH], {
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
  if (isObject(output._meta?.telemetry)) delete output._meta.telemetry.analysis_run_id;
  if (isObject(output._meta?.type_aware)) {
    delete output._meta.type_aware.elapsed_ms;
    delete output._meta.type_aware.phase_timings_ms;
  }
  return output;
};

export const validateSupplementalArtifactData = (artifact, decisions, context) => {
  if (!isObject(artifact) || artifact.schema_version !== 3) {
    fail("supplemental artifact schema_version must be 3");
  }
  if (
    !isObject(context) ||
    !/^[0-9a-f]{64}$/.test(context.fallowSha256) ||
    !/^[0-9a-f]{64}$/.test(context.sidecarSha256) ||
    typeof context.sourceRoot !== "string"
  ) {
    fail("supplemental validation requires runtime hashes and a source root");
  }
  if (
    artifact.artifacts?.fallow_sha256 !== context.fallowSha256 ||
    artifact.artifacts?.sidecar_sha256 !== context.sidecarSha256
  ) {
    fail("supplemental artifact runtime hashes do not match the validated binaries");
  }
  const confirmedKeys = artifact.result?.confirmed_candidate_keys;
  const reviewedKeys = artifact.independent_review?.reviewed_candidate_keys;
  if (
    !Array.isArray(confirmedKeys) ||
    !Array.isArray(reviewedKeys) ||
    JSON.stringify(confirmedKeys) !== JSON.stringify(confirmedKeys.toSorted()) ||
    JSON.stringify(reviewedKeys) !== JSON.stringify(reviewedKeys.toSorted()) ||
    new Set(confirmedKeys).size !== confirmedKeys.length ||
    new Set(reviewedKeys).size !== reviewedKeys.length
  ) {
    fail("supplemental candidate key sets must be sorted and unique");
  }
  const review = decisions.supplemental_reviews?.find(
    ({ project_id: projectId }) => projectId === "vitest",
  );
  if (
    !review ||
    artifact.project?.commit !== review.commit ||
    artifact.independent_review.verdict !== "approved" ||
    artifact.independent_review.reviewed_candidate_count !== reviewedKeys.length ||
    artifact.independent_review.reviewed_candidate_set_sha256 !==
      candidateSetDigest(reviewedKeys) ||
    JSON.stringify(reviewedKeys) !== JSON.stringify(review.candidate_keys) ||
    artifact.result.confirmed_used !== confirmedKeys.length ||
    artifact.result.confirmed_candidate_set_sha256 !== candidateSetDigest(confirmedKeys) ||
    !confirmedKeys.every((key) => reviewedKeys.includes(key)) ||
    artifact.independent_review.clean_run_is_subset !== true ||
    artifact.result.baseline_candidates - artifact.result.refined_candidates !==
      artifact.result.confirmed_used ||
    artifact.result.refined_candidates !==
      artifact.result.unresolved_retained + artifact.result.abstained_retained
  ) {
    fail("supplemental artifact counts, hashes, or review binding are inconsistent");
  }
  const sourceRuns = artifact.artifacts?.source_runs;
  if (!Array.isArray(sourceRuns) || sourceRuns.length !== 4) {
    fail("supplemental artifact requires two baseline and two refined source runs");
  }
  const seenPaths = new Set();
  for (const mode of ["baseline", "refined"]) {
    const matching = sourceRuns.filter((run) => run.mode === mode);
    if (
      matching.length !== 2 ||
      JSON.stringify(matching.map(({ iteration }) => iteration).toSorted()) !==
        JSON.stringify([0, 1]) ||
      new Set(matching.map(({ normalized_sha256: digest }) => digest)).size !== 1 ||
      matching.some(
        ({ normalized_sha256: normalizedSha256 }) => !/^[0-9a-f]{64}$/.test(normalizedSha256),
      )
    ) {
      fail(`supplemental ${mode} source runs are incomplete or nondeterministic`);
    }
    for (const run of matching) {
      const runPath = normalizedRelativePath(run.path, `supplemental ${mode} source path`);
      if (seenPaths.has(runPath)) fail(`duplicate supplemental source path: ${runPath}`);
      seenPaths.add(runPath);
      const absolutePath = resolve(context.sourceRoot, runPath);
      const relativeToRoot = relative(context.sourceRoot, absolutePath);
      if (relativeToRoot.startsWith(`..${sep}`) || relativeToRoot === "..") {
        fail(`supplemental source path escapes its root: ${runPath}`);
      }
      ensureFile(absolutePath, `supplemental ${mode} source run`);
      const normalizedDigest = createHash("sha256")
        .update(JSON.stringify(normalizeSupplementalOutput(readJson(absolutePath))))
        .digest("hex");
      if (normalizedDigest !== run.normalized_sha256) {
        fail(`supplemental ${mode} source hash does not match ${runPath}`);
      }
    }
  }
  return artifact;
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

const supplemental = async (options, publicationMode = "write") => {
  ensureRuntimeInputs(options);
  prepareSupplementalFixture();
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
  assertDependencyFreeFixture(SUPPLEMENTAL_VITEST_ROOT, "vitest");

  const rawDir = resolve(DEFAULT_OUT_DIR, "supplemental");
  const runs = [];
  for (let iteration = 0; iteration < 2; iteration += 1) {
    for (const mode of runOrderForIteration(iteration)) {
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
      if (result.status === null || result.status >= 2) {
        fail(`supplemental Vitest ${mode} run failed with ${result.status ?? result.signal}`);
      }
      runs.push({
        mode,
        iteration,
        rawPath,
        raw_sha256: sha256File(rawPath),
        ...parseMachineOutput(result.stdout, "vitest"),
      });
    }
  }

  for (const mode of ["baseline", "refined"]) {
    const matching = runs.filter((run) => run.mode === mode);
    const normalized = matching.map(({ output }) => normalizeSupplementalOutput(output));
    if (JSON.stringify(normalized[0]) !== JSON.stringify(normalized[1])) {
      fail(`supplemental Vitest ${mode} output is not deterministic`);
    }
  }
  const baseline = runs.find(({ mode, iteration }) => mode === "baseline" && iteration === 0);
  const refined = runs.find(({ mode, iteration }) => mode === "refined" && iteration === 0);
  const compared = compareCandidateSets("vitest", baseline.candidates, refined.candidates);
  const confirmedKeys = compared
    .filter(({ semantic_status: semanticStatus }) => semanticStatus === "confirmed-used")
    .map(({ key }) => key)
    .toSorted();
  const decisions = readJson(DEFAULT_ADJUDICATION, "adjudication decisions");
  const review = decisions.supplemental_reviews?.find(
    ({ project_id: projectId }) => projectId === "vitest",
  );
  if (!review || review.commit !== commit || review.verdict !== "approved") {
    fail("supplemental Vitest review decision is missing or stale");
  }
  const reviewedKeys = review.candidate_keys?.toSorted() ?? [];
  if (
    JSON.stringify(review.candidate_keys) !== JSON.stringify(reviewedKeys) ||
    new Set(reviewedKeys).size !== reviewedKeys.length ||
    review.candidate_count !== reviewedKeys.length ||
    review.candidate_set_sha256 !== candidateSetDigest(reviewedKeys)
  ) {
    fail("supplemental Vitest review candidate set is invalid");
  }
  const reviewedSet = new Set(reviewedKeys);
  const cleanRunIsSubset = confirmedKeys.every((key) => reviewedSet.has(key));
  if (!cleanRunIsSubset) fail("clean supplemental Vitest confirmations exceed the reviewed set");
  const meta = refined.typeAware;
  if (
    meta?.confirmed_used_count !== confirmedKeys.length ||
    meta?.candidate_count !== baseline.candidates.length ||
    meta?.unresolved_count + meta?.abstained_count + meta?.confirmed_used_count !==
      meta?.candidate_count
  ) {
    fail("supplemental Vitest type-aware metadata is inconsistent");
  }

  const artifact = {
    schema_version: 3,
    canonical_candidate_key: {
      algorithm: "sha256",
      prefix: "tac_",
      digest_hex_characters: 20,
      fields: ["project_id", "path", "parent_name", "member_name", "kind", "line", "col"],
      separator: "NUL",
    },
    project: {
      repo: "vitest-dev/vitest",
      commit,
      tracked_source_clean: true,
      dependency_installation: "none",
    },
    artifacts: {
      fallow_sha256: sha256File(options.fallowBin),
      sidecar_sha256: sidecarArtifactHash(options.sidecarBin),
      source_runs: runs
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
        ),
    },
    result: {
      baseline_candidates: baseline.candidates.length,
      refined_candidates: refined.candidates.length,
      confirmed_used: confirmedKeys.length,
      unresolved_retained: meta.unresolved_count,
      abstained_retained: meta.abstained_count,
      added_candidates: 0,
      normalized_runs: 2,
      deterministic: true,
      confirmed_candidate_set_sha256: candidateSetDigest(confirmedKeys),
      confirmed_candidate_keys: confirmedKeys,
    },
    independent_review: {
      verdict: review.verdict,
      reviewed_candidate_count: review.candidate_count,
      reviewed_candidate_set_sha256: review.candidate_set_sha256,
      reviewed_candidate_keys: reviewedKeys,
      clean_run_is_subset: cleanRunIsSubset,
      known_incorrect_removals: review.known_incorrect_removals,
      method: review.method,
    },
  };
  validateSupplementalArtifactData(artifact, decisions, {
    fallowSha256: sha256File(options.fallowBin),
    sidecarSha256: sidecarArtifactHash(options.sidecarBin),
    sourceRoot: REPO_ROOT,
  });
  if (publicationMode === "write") {
    writeJson(SUPPLEMENTAL_ARTIFACT, artifact);
  } else if (
    JSON.stringify(readJson(SUPPLEMENTAL_ARTIFACT, "tracked supplemental artifact")) !==
    JSON.stringify(artifact)
  ) {
    fail("tracked supplemental artifact has generator drift; rerun supplemental");
  }
  return artifact;
};

const readDiscovery = (outDir) => readJson(resolve(outDir, "discovery.json"), "discovery artifact");

const sourceLineEvidence = (root, candidate) => {
  const sourcePath = resolve(root, candidate.path);
  const relativeToRoot = relative(root, sourcePath);
  if (relativeToRoot.startsWith(`..${sep}`) || relativeToRoot === "..") {
    fail(`${candidate.key} source path escapes its fixture`);
  }
  let excerpt = "";
  if (existsSync(sourcePath) && statSync(sourcePath).isFile()) {
    const lines = readFileSync(sourcePath, "utf8").split(/\r?\n/);
    excerpt = (lines[Math.max(0, candidate.line - 1)] ?? "").trim();
  }
  return { path: candidate.path, line: candidate.line, col: candidate.col, excerpt };
};

const sourceLocationEvidence = (root, location) => {
  const sourcePath = resolve(root, location.path);
  const relativeToRoot = relative(root, sourcePath);
  if (relativeToRoot.startsWith(`..${sep}`) || relativeToRoot === "..") {
    fail(`use evidence path escapes its prepared fixture: ${location.path}`);
  }
  const lines = readFileSync(sourcePath, "utf8").split(/\r?\n/);
  const excerpt = (lines[location.line - 1] ?? "").trim();
  if (excerpt === "") fail(`use evidence has no source line: ${location.path}:${location.line}`);
  return { path: location.path, line: location.line, col: location.col, excerpt };
};

const collectCheckerEvidence = async (project, candidates, outDir) => {
  const [{ parseRequest: parseTypeAwareRequest }, { analyzeClassMemberUses }] = await Promise.all([
    import("../tools/type-aware-sidecar/src/protocol.mjs"),
    import("../tools/type-aware-sidecar/src/typescript-go.mjs"),
  ]);
  const root = projectRoot(project, outDir);
  const request = parseTypeAwareRequest({
    protocol_version: 2,
    operation: "class-member-uses",
    root,
    projects: [],
    candidates: candidates.map((candidate, id) => ({
      id,
      path: candidate.path,
      parent_name: candidate.parent_name,
      member_name: candidate.member_name,
      kind: candidate.kind,
      line: candidate.line,
      col: candidate.col,
    })),
  });
  const analysis = analyzeClassMemberUses(request);
  return new Map(
    [...analysis.confirmedUses].map(([id, locations]) => [
      candidates[id].key,
      locations.map((location) => sourceLocationEvidence(root, location)),
    ]),
  );
};

export const candidateFeatureBucketFields = (
  projectFeatureBuckets,
  previousEntry,
  previousSchemaVersion,
) => {
  const legacySuggestions =
    previousSchemaVersion === 1 ? previousEntry?.feature_buckets : undefined;
  return {
    suggested_feature_buckets: [
      ...(previousEntry?.suggested_feature_buckets ?? legacySuggestions ?? projectFeatureBuckets),
    ],
    adjudicated_feature_buckets:
      previousSchemaVersion === 2 ? [...(previousEntry?.adjudicated_feature_buckets ?? [])] : [],
  };
};

const LEDGER_REFRESH_RECOVERY =
  "Archive the old ledger or restore the discovery that created it, then retry.";

/** Validates that a ledger refresh cannot discard prior manual adjudication. */
export const indexLedgerForRefresh = (previous, discoveredKeys) => {
  if (previous === null) return new Map();
  if (
    !isObject(previous) ||
    (previous.schema_version !== 1 && previous.schema_version !== 2) ||
    !Array.isArray(previous.candidates)
  ) {
    fail(
      `existing evidence ledger must use schema_version 1 or 2 and contain a candidates array. ${LEDGER_REFRESH_RECOVERY}`,
    );
  }

  const previousByKey = new Map();
  for (const entry of previous.candidates) {
    if (!isObject(entry) || typeof entry.key !== "string" || entry.key.trim() === "") {
      fail(
        `every existing evidence ledger candidate must have a non-empty key. ${LEDGER_REFRESH_RECOVERY}`,
      );
    }
    if (previousByKey.has(entry.key)) {
      fail(
        `existing evidence ledger contains duplicate candidate key ${JSON.stringify(entry.key)}. ${LEDGER_REFRESH_RECOVERY}`,
      );
    }
    previousByKey.set(entry.key, entry);
  }

  for (const key of previousByKey.keys()) {
    if (!discoveredKeys.has(key)) {
      fail(
        `existing evidence ledger candidate key ${JSON.stringify(key)} is missing from the current discovery. ${LEDGER_REFRESH_RECOVERY}`,
      );
    }
  }
  return previousByKey;
};

const evidence = async (manifest, projects, options) => {
  const discovery = readDiscovery(options.outDir);
  const selected = new Map(projects.map((project) => [project.id, project]));
  const ledgerPath = resolve(options.outDir, "ledger.json");
  const previous = existsSync(ledgerPath) ? readJson(ledgerPath, "existing evidence ledger") : null;
  const discoveredKeys = new Set(
    (discovery.projects ?? []).flatMap((result) =>
      (result.candidates ?? []).map((entry) => entry.key),
    ),
  );
  const previousByKey = indexLedgerForRefresh(previous, discoveredKeys);
  const entries = [];
  for (const result of discovery.projects ?? []) {
    const project = selected.get(result.id);
    if (!project) continue;
    const root = projectRoot(project, options.outDir);
    const checkerEvidence = await collectCheckerEvidence(
      project,
      result.candidates ?? [],
      options.outDir,
    );
    const expectedConfirmed = new Set(
      (result.candidates ?? [])
        .filter(({ semantic_status: semanticStatus }) => semanticStatus === "confirmed-used")
        .map(({ key }) => key),
    );
    if (
      JSON.stringify([...checkerEvidence.keys()].toSorted()) !==
      JSON.stringify([...expectedConfirmed].toSorted())
    ) {
      fail(`${project.id} checker evidence does not match discovery; rerun discover first`);
    }
    for (const candidate of result.candidates ?? []) {
      const old = previousByKey.get(candidate.key);
      const featureBuckets = candidateFeatureBucketFields(
        project.feature_buckets,
        old,
        previous?.schema_version,
      );
      entries.push({
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
        ...featureBuckets,
        truth: old?.truth ?? "pending",
        source_evidence: {
          declaration: sourceLineEvidence(root, candidate),
          uses: checkerEvidence.get(candidate.key) ?? old?.source_evidence?.uses ?? [],
          notes: old?.source_evidence?.notes ?? null,
        },
      });
    }
  }
  entries.sort((left, right) => left.key.localeCompare(right.key));
  const ledger = {
    schema_version: 2,
    artifact_policy: "local adjudication only; never copy raw or private source into tracked files",
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
  const errors = [];
  const confirmedByProject = new Map();
  for (const entry of allEntries.filter(
    ({ semantic_status: semanticStatus }) => semanticStatus === "confirmed-used",
  )) {
    const entries = confirmedByProject.get(entry.project_id) ?? [];
    entries.push(entry);
    confirmedByProject.set(entry.project_id, entries);
  }
  if (!Array.isArray(reviews)) return ["independent reviews must be an array"];
  const seenProjects = new Set();
  for (const review of reviews) {
    const projectId = review?.project_id;
    if (typeof projectId !== "string" || !confirmedByProject.has(projectId)) {
      errors.push(
        `independent review references an unconfirmed project: ${projectId ?? "missing"}`,
      );
      continue;
    }
    if (seenProjects.has(projectId)) {
      errors.push(`independent review is duplicated for ${projectId}`);
      continue;
    }
    seenProjects.add(projectId);
    const entries = confirmedByProject.get(projectId);
    const keys = entries.map(({ key }) => key).toSorted();
    const expectedCandidateSetDigest = candidateSetDigest(keys);
    if (
      review.verdict !== "approved" ||
      review.candidate_count !== keys.length ||
      review.candidate_set_sha256 !== expectedCandidateSetDigest ||
      review.evidence_sha256 !== independentReviewDigest(entries)
    ) {
      errors.push(`independent review does not match confirmed evidence for ${projectId}`);
    }
  }
  for (const projectId of confirmedByProject.keys()) {
    if (!seenProjects.has(projectId)) {
      errors.push(`confirmed candidates for ${projectId} have no independent approved review`);
    }
  }
  return errors.toSorted();
};

const adjudicate = (outDir) => {
  const ledgerPath = resolve(outDir, "ledger.json");
  const ledger = readJson(ledgerPath, "evidence ledger");
  const decisions = readJson(DEFAULT_ADJUDICATION, "adjudication decisions");
  if (!isObject(decisions) || decisions.schema_version !== 1) {
    fail("adjudication decisions schema_version must be 1");
  }
  const entriesByKey = new Map(ledger.candidates.map((entry) => [entry.key, entry]));
  for (const key of Object.keys(decisions.known_unused ?? {})) {
    if (!entriesByKey.has(key)) fail(`adjudication references unknown unused candidate ${key}`);
  }
  for (const key of Object.keys(decisions.feature_overrides ?? {})) {
    if (!entriesByKey.has(key)) fail(`adjudication references unknown feature candidate ${key}`);
  }
  const reviewErrors = independentReviewErrors(ledger.candidates, decisions.independent_reviews);
  if (reviewErrors.length > 0) fail(reviewErrors.join("\n"));
  for (const entry of ledger.candidates) {
    const knownUnused = decisions.known_unused?.[entry.key];
    if (entry.semantic_status === "confirmed-used") {
      if (entry.source_evidence.uses.length === 0) {
        fail(`${entry.key} cannot be adjudicated used without concrete checker evidence`);
      }
      entry.truth = "used";
      entry.source_evidence.notes =
        "Exact declaration and checker-resolved source use reviewed against the pinned fixture.";
      entry.adjudicated_feature_buckets = decisions.feature_overrides?.[entry.key] ?? [
        decisions.default_confirmed_bucket,
      ];
    } else if (knownUnused) {
      entry.truth = "unused";
      entry.source_evidence.notes = knownUnused;
      entry.adjudicated_feature_buckets = ["known-unused-retention"];
    } else {
      entry.truth = "indeterminate";
      entry.source_evidence.notes =
        "No exact checker confirmation; retained conservatively without claiming that the member is unused.";
      entry.adjudicated_feature_buckets = [decisions.default_retained_bucket];
    }
  }
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
  isObject(location) &&
  typeof location.path === "string" &&
  location.path.trim() !== "" &&
  !isAbsolute(location.path) &&
  Number.isSafeInteger(location.line) &&
  location.line > 0 &&
  Number.isSafeInteger(location.col) &&
  location.col >= 0 &&
  typeof location.excerpt === "string" &&
  location.excerpt.trim() !== "";

const featureBucketListValid = (buckets) =>
  Array.isArray(buckets) &&
  buckets.length > 0 &&
  buckets.every((bucket) => typeof bucket === "string" && bucket.trim() !== "") &&
  new Set(buckets).size === buckets.length;

const optionalFeatureBucketListValid = (buckets) =>
  buckets === undefined || featureBucketListValid(buckets);

export const verifyLedgerData = (discovery, ledger) => {
  const errors = [];
  if (
    !isObject(discovery) ||
    discovery.schema_version !== 1 ||
    !Array.isArray(discovery.projects)
  ) {
    return ["discovery artifact is invalid"];
  }
  if (isObject(ledger) && ledger.schema_version === 1 && Array.isArray(ledger.candidates)) {
    return [
      "ledger schema_version 1 is outdated; run `npm run type-aware:corpus -- evidence` to migrate it",
    ];
  }
  if (!isObject(ledger) || ledger.schema_version !== 2 || !Array.isArray(ledger.candidates)) {
    return ["ledger artifact is invalid"];
  }
  const expected = new Map();
  for (const project of discovery.projects) {
    for (const candidate of project.candidates ?? []) {
      try {
        const computedKey = candidateKey(project.id, candidate);
        if (candidate.key !== computedKey) {
          errors.push(`${candidate.key}: discovery candidate key is not stable`);
        }
      } catch (error) {
        errors.push(
          `${candidate.key ?? "unknown"}: invalid discovery candidate: ${error instanceof Error ? error.message : String(error)}`,
        );
      }
      expected.set(candidate.key, { ...candidate, project_id: project.id });
    }
  }
  const actual = new Map();
  for (const entry of ledger.candidates) {
    if (!isObject(entry) || typeof entry.key !== "string") {
      errors.push("ledger entry is missing a candidate key");
      continue;
    }
    if (actual.has(entry.key)) errors.push(`${entry.key}: duplicate ledger entry`);
    actual.set(entry.key, entry);
  }
  for (const [key, candidate] of expected) {
    const entry = actual.get(key);
    if (!entry) {
      errors.push(`${key}: missing ledger entry`);
      continue;
    }
    if (entry.project_id !== candidate.project_id) {
      errors.push(`${key}: project_id does not match discovery`);
    }
    try {
      if (
        JSON.stringify(candidateFields(entry.candidate)) !==
        JSON.stringify(candidateFields(candidate))
      ) {
        errors.push(`${key}: candidate fields do not match discovery`);
      }
    } catch {
      errors.push(`${key}: ledger candidate fields are incomplete`);
    }
    if (!TRUTH_STATUSES.has(entry.truth))
      errors.push(`${key}: truth must be used, unused, or indeterminate`);
    if (entry.semantic_status === "confirmed-used" && entry.truth !== "used") {
      errors.push(`${key}: every confirmed removal must be adjudicated used`);
    }
    if (entry.semantic_status !== candidate.semantic_status)
      errors.push(`${key}: semantic_status does not match discovery`);
    if (
      !isObject(entry.source_evidence) ||
      !evidenceLocationValid(entry.source_evidence.declaration)
    ) {
      errors.push(`${key}: complete declaration source evidence is required`);
    } else if (entry.source_evidence.declaration.path !== candidate.path) {
      errors.push(`${key}: declaration evidence path must match the candidate path`);
    } else if (entry.source_evidence.declaration.line !== candidate.line) {
      errors.push(`${key}: declaration evidence line must match the candidate line`);
    }
    const uses = entry.source_evidence?.uses;
    if (!Array.isArray(uses)) errors.push(`${key}: source_evidence.uses must be an array`);
    else if (uses.some((use) => !evidenceLocationValid(use)))
      errors.push(`${key}: every use must have path, line, and excerpt`);
    if (
      (entry.truth === "used" || entry.semantic_status === "confirmed-used") &&
      uses?.length === 0
    ) {
      errors.push(`${key}: used or confirmed candidates require concrete use evidence`);
    }
    if (
      (entry.truth === "unused" || entry.truth === "indeterminate") &&
      (typeof entry.source_evidence?.notes !== "string" ||
        entry.source_evidence.notes.trim() === "")
    ) {
      errors.push(`${key}: unused or indeterminate truth requires adjudication notes`);
    }
    if (!optionalFeatureBucketListValid(entry.suggested_feature_buckets))
      errors.push(`${key}: suggested feature buckets must be non-empty and unique when present`);
    if (!featureBucketListValid(entry.adjudicated_feature_buckets))
      errors.push(`${key}: at least one explicitly adjudicated feature bucket is required`);
  }
  for (const key of actual.keys()) {
    if (!expected.has(key)) errors.push(`${key}: stale ledger entry is not present in discovery`);
  }
  return errors.toSorted();
};

const verifySourceEvidence = (manifest, ledger, outDir) => {
  const projects = new Map(manifest.projects.map((project) => [project.id, project]));
  const roots = new Map(
    manifest.projects.map((project) => [project.id, projectRoot(project, outDir)]),
  );
  const errors = [];
  for (const entry of ledger.candidates) {
    const project = projects.get(entry.project_id);
    if (!project) {
      errors.push(`${entry.key}: source project is not in the manifest`);
      continue;
    }
    const root = roots.get(project.id);
    const locations = [entry.source_evidence?.declaration, ...(entry.source_evidence?.uses ?? [])];
    for (const location of locations) {
      if (!evidenceLocationValid(location)) continue;
      const sourcePath = resolve(root, location.path);
      const relativeToRoot = relative(root, sourcePath);
      if (relativeToRoot.startsWith(`..${sep}`) || relativeToRoot === "..") {
        errors.push(`${entry.key}: evidence path escapes the prepared fixture`);
        continue;
      }
      if (!existsSync(sourcePath) || !statSync(sourcePath).isFile()) {
        errors.push(`${entry.key}: evidence source file is missing: ${location.path}`);
        continue;
      }
      const sourceLine = readFileSync(sourcePath, "utf8").split(/\r?\n/)[location.line - 1] ?? "";
      const actual = sourceLine.trim();
      if (actual !== location.excerpt) {
        errors.push(`${entry.key}: evidence excerpt is stale at ${location.path}:${location.line}`);
      }
      if (location.col > Buffer.byteLength(sourceLine, "utf8")) {
        errors.push(`${entry.key}: evidence column is outside ${location.path}:${location.line}`);
      }
    }
  }
  return errors;
};

const verifyLedger = (outDir, manifest, discovery = readDiscovery(outDir)) => {
  const ledger = readJson(resolve(outDir, "ledger.json"), "evidence ledger");
  const decisions = readJson(DEFAULT_ADJUDICATION, "adjudication decisions");
  if (JSON.stringify(discovery.corpus) !== JSON.stringify(ledger.corpus)) {
    fail("ledger corpus identity does not match discovery; refresh evidence");
  }
  const errors = [
    ...verifyLedgerData(discovery, ledger),
    ...verifySourceEvidence(manifest, ledger, outDir),
  ].toSorted();
  if (ledger.adjudication?.decisions_sha256 !== sha256File(DEFAULT_ADJUDICATION)) {
    errors.push("ledger adjudication does not match the checked-in review decisions");
  }
  if (ledger.adjudication?.independent_signoff?.verdict !== "approved") {
    errors.push("ledger requires an approved independent signoff");
  }
  const storedReviews = ledger.adjudication?.independent_signoff?.reviews;
  errors.push(...independentReviewErrors(ledger.candidates, storedReviews));
  if (JSON.stringify(storedReviews) !== JSON.stringify(decisions.independent_reviews)) {
    errors.push("ledger independent reviews do not match the checked-in review decisions");
  }
  if (errors.length > 0)
    fail(`ledger verification failed:\n${errors.map((error) => `- ${error}`).join("\n")}`);
  return { discovery, ledger };
};

const percentile = (values, ratio) => {
  if (values.length === 0) return null;
  const sorted = values.toSorted((left, right) => left - right);
  const index = (sorted.length - 1) * ratio;
  const lower = Math.floor(index);
  const upper = Math.ceil(index);
  const value =
    lower === upper
      ? sorted[lower]
      : sorted[lower] + (sorted[upper] - sorted[lower]) * (index - lower);
  return Math.round(value * 1_000) / 1_000;
};

const measuredRuns = (measurements, mode) =>
  measurements.projects.flatMap((project) =>
    project.runs.filter((run) => !run.warmup && run.mode === mode),
  );

export const validateMeasurements = (discovery, measurements) => {
  if (!isObject(measurements) || measurements.schema_version !== 1) {
    fail("measurement artifact schema_version must be 1");
  }
  if (JSON.stringify(discovery.corpus) !== JSON.stringify(measurements.corpus)) {
    fail("measurement corpus identity does not match discovery; rerun measure");
  }
  if (!Number.isSafeInteger(measurements.warmups) || measurements.warmups < 1) {
    fail("measurements require at least one warmup pair");
  }
  if (!Number.isSafeInteger(measurements.measured_pairs) || measurements.measured_pairs < 4) {
    fail("measurements require at least four measured pairs");
  }
  const expectedProjects = new Map(discovery.projects.map((project) => [project.id, project]));
  if (
    !Array.isArray(measurements.projects) ||
    measurements.projects.length !== expectedProjects.size
  ) {
    fail("measurements must contain every discovery project exactly once");
  }
  const seen = new Set();
  for (const project of measurements.projects) {
    const discovered = expectedProjects.get(project.id);
    if (!discovered || seen.has(project.id)) fail(`invalid measurement project ${project.id}`);
    seen.add(project.id);
    const expectedRunCount = 2 * (measurements.warmups + measurements.measured_pairs);
    if (!Array.isArray(project.runs) || project.runs.length !== expectedRunCount) {
      fail(`${project.id} measurements have an incomplete run matrix`);
    }
    const pairs = new Map();
    for (const run of project.runs) {
      if (!isObject(run) || typeof run.warmup !== "boolean") {
        fail(`${project.id} measurement warmup must be a boolean`);
      }
      if (run.mode !== "baseline" && run.mode !== "refined") {
        fail(`${project.id} measurement mode must be baseline or refined`);
      }
      const iterationLimit = run.warmup ? measurements.warmups : measurements.measured_pairs;
      if (
        !Number.isSafeInteger(run.iteration) ||
        run.iteration < 0 ||
        run.iteration >= iterationLimit
      ) {
        fail(`${project.id} measurement iteration is outside its expected range`);
      }
      if (!Number.isFinite(run.wall_ms) || run.wall_ms <= 0) {
        fail(`${project.id} measurement has an invalid wall time`);
      }
      if (
        process.platform !== "win32" &&
        (!Number.isFinite(run.peak_process_tree_rss_kb) || run.peak_process_tree_rss_kb <= 0)
      ) {
        fail(`${project.id} measurement has no process-tree RSS sample`);
      }
      const expectedCount =
        run.mode === "baseline"
          ? discovered.baseline_candidate_count
          : discovered.refined_candidate_count;
      if (run.candidate_count !== expectedCount) {
        fail(`${project.id} ${run.mode} candidate count drifted during measurement`);
      }
      const key = `${run.warmup ? "warmup" : "measured"}:${run.iteration}`;
      const modes = pairs.get(key) ?? new Set();
      if (modes.has(run.mode)) {
        fail(`${project.id} measurements contain a duplicate ${key}:${run.mode} run`);
      }
      modes.add(run.mode);
      pairs.set(key, modes);
    }
    if (
      pairs.size !== measurements.warmups + measurements.measured_pairs ||
      [...pairs.values()].some(
        (modes) => modes.size !== 2 || !modes.has("baseline") || !modes.has("refined"),
      )
    ) {
      fail(`${project.id} measurements are missing a baseline or refined pair`);
    }
  }
  const comparableProvenance = (provenance) => ({
    manifest_sha256: provenance?.manifest_sha256,
    fallow: provenance?.fallow,
    sidecar: provenance?.sidecar,
    runtime: provenance?.runtime,
    fixtures: provenance?.fixtures,
  });
  if (
    JSON.stringify(comparableProvenance(discovery.provenance)) !==
    JSON.stringify(comparableProvenance(measurements.provenance))
  ) {
    fail("discovery and measurement provenance differ; rerun both with the same artifacts");
  }
};

const validatePublicationProvenance = (discovery, manifest, options) => {
  ensureRuntimeInputs(options);
  const expected = {
    manifest_sha256: sha256File(options.manifest),
    fallow: {
      version: binaryVersion(options.fallowBin),
      sha256: sha256File(options.fallowBin),
    },
    sidecar: { sha256: sidecarArtifactHash(options.sidecarBin) },
    runtime: {
      platform: process.platform,
      architecture: process.arch,
      node: process.version,
    },
    fixtures: manifest.projects.map((project) => {
      const { commit } = validatePreparedFixture(project, options.outDir);
      return { id: project.id, repo: project.repo, ref: project.ref, commit };
    }),
  };
  const actual = {
    manifest_sha256: discovery.provenance?.manifest_sha256,
    fallow: discovery.provenance?.fallow,
    sidecar: discovery.provenance?.sidecar,
    runtime: discovery.provenance?.runtime,
    fixtures: discovery.provenance?.fixtures,
  };
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    fail("discovery provenance does not match current release artifacts and pinned fixtures");
  }
};

const aggregateReasonCounts = (runs) => {
  const counts = {};
  for (const run of runs) {
    for (const [reason, count] of Object.entries(run.reason_counts ?? {})) {
      if (Number.isFinite(count)) counts[reason] = (counts[reason] ?? 0) + count;
    }
  }
  return Object.fromEntries(
    Object.entries(counts).toSorted(([left], [right]) => left.localeCompare(right)),
  );
};

const aggregatePhaseTimings = (runs) => {
  const byPhase = new Map();
  for (const run of runs) {
    for (const [phase, duration] of Object.entries(run.phase_timings_ms ?? {})) {
      if (!Number.isFinite(duration)) continue;
      const values = byPhase.get(phase) ?? [];
      values.push(duration);
      byPhase.set(phase, values);
    }
  }
  return Object.fromEntries(
    [...byPhase.entries()]
      .toSorted(([left], [right]) => left.localeCompare(right))
      .map(([phase, values]) => [
        phase,
        { median: percentile(values, 0.5), p95: percentile(values, 0.95) },
      ]),
  );
};

const safeRatio = (numerator, denominator) =>
  denominator === 0 ? null : Math.round((numerator / denominator) * 1_000_000) / 1_000_000;

export const summarizeAdjudicatedFeatureBuckets = (entries) => {
  const candidates = entries
    .filter(
      ({ semantic_status: semanticStatus, truth }) =>
        semanticStatus === "confirmed-used" && truth === "used",
    )
    .map((entry) => ({
      key: entry.key,
      buckets: new Set(entry.adjudicated_feature_buckets ?? []),
    }));
  const confirmedFeatureBuckets = new Set(candidates.flatMap(({ buckets }) => [...buckets]));
  let multipleFeatureBuckets = false;
  for (let leftIndex = 0; leftIndex < candidates.length; leftIndex += 1) {
    const left = candidates[leftIndex];
    for (let rightIndex = leftIndex + 1; rightIndex < candidates.length; rightIndex += 1) {
      const right = candidates[rightIndex];
      if (left.key === right.key) continue;
      if (
        [...left.buckets].some((bucket) => [...right.buckets].some((other) => other !== bucket))
      ) {
        multipleFeatureBuckets = true;
        break;
      }
    }
    if (multipleFeatureBuckets) break;
  }
  return {
    confirmed_feature_buckets: [...confirmedFeatureBuckets].toSorted(),
    multiple_feature_buckets: multipleFeatureBuckets,
  };
};

const renderSummaryMarkdown = (summary) =>
  [
    "# Type-aware corpus summary",
    "",
    `Gate: ${summary.gate.go ? "GO" : "NO-GO"}`,
    "",
    `- Candidates: ${summary.accuracy.candidate_count}`,
    `- Confirmation precision: ${summary.accuracy.confirmation_precision ?? "n/a"}`,
    `- Used recall: ${summary.accuracy.used_recall ?? "n/a"}`,
    `- Correct-unused retention: ${summary.accuracy.correct_unused_retention ?? "n/a"}`,
    `- Abstention: ${summary.accuracy.abstention ?? "n/a"}`,
    `- Median marginal overhead: ${summary.performance.marginal_overhead_ms.median ?? "n/a"} ms`,
    `- P95 marginal overhead: ${summary.performance.marginal_overhead_ms.p95 ?? "n/a"} ms`,
    `- Independent repositories with confirmed value: ${summary.value.confirmed_repository_count}`,
    `- Semantic buckets with confirmed value: ${summary.value.confirmed_feature_buckets.join(", ") || "none"}`,
    "",
    "Raw machine output and source evidence remain under target/type-aware-corpus and are not tracked.",
    "",
  ].join("\n");

export const requirePublicationGo = (summary, publicationMode) => {
  if (publicationMode !== "verify" || summary.gate?.go === true) return;
  const failedChecks = Object.entries(summary.gate?.checks ?? {})
    .filter(([, passed]) => passed !== true)
    .map(([name]) => name)
    .toSorted();
  fail(`publication gate is NO-GO; failed checks: ${failedChecks.join(", ") || "unknown"}`);
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
  const focusedCasesPassed =
    focusedReport.schema_version === 1 &&
    focusedReport.passed === true &&
    focusedReport.fallow_sha256 === discovery.provenance?.fallow?.sha256 &&
    focusedReport.sidecar_sha256 === discovery.provenance?.sidecar?.sha256 &&
    focusedReport.test_sha256 === sha256File(FOCUSED_TEST_PATH) &&
    JSON.stringify(focusedReport.cases) === JSON.stringify([...REQUIRED_FOCUSED_CASES].toSorted());
  const entries = ledger.candidates;
  const confirmed = entries.filter(({ semantic_status }) => semantic_status === "confirmed-used");
  const retained = entries.filter(({ semantic_status }) => semantic_status === "retained");
  const used = entries.filter(({ truth }) => truth === "used");
  const unused = entries.filter(({ truth }) => truth === "unused");
  const correctConfirmed = confirmed.filter(({ truth }) => truth === "used");
  const incorrectConfirmed = confirmed.filter(({ truth }) => truth === "unused");
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
  const overheads = [];
  for (const project of measurements.projects) {
    const byPair = new Map();
    for (const run of project.runs.filter(({ warmup }) => !warmup)) {
      const pair = byPair.get(run.iteration) ?? {};
      pair[run.mode] = run.wall_ms;
      byPair.set(run.iteration, pair);
    }
    for (const pair of byPair.values()) {
      if (Number.isFinite(pair.baseline) && Number.isFinite(pair.refined))
        overheads.push(pair.refined - pair.baseline);
    }
  }
  const confirmedProjects = new Set(correctConfirmed.map(({ project_id }) => project_id));
  const featureBucketValue = summarizeAdjudicatedFeatureBuckets(entries);
  const zeroControlsClean = discovery.projects
    .filter(({ role }) => role === "zero-control")
    .every(({ baseline_candidate_count }) => baseline_candidate_count === 0);
  const confirmationPrecision = safeRatio(correctConfirmed.length, confirmed.length);
  const usedRecall = safeRatio(correctConfirmed.length, used.length);
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
  const independentSignoff = ledger.adjudication?.independent_signoff;
  const checks = {
    zero_incorrect_removals: incorrectConfirmed.length === 0,
    zero_controls_clean: zeroControlsClean,
    multiple_repositories: confirmedProjects.size >= 2,
    multiple_feature_buckets: featureBucketValue.multiple_feature_buckets,
    deterministic_output: discovery.determinism_runs >= 2,
    focused_cases: focusedCasesPassed,
    confirmation_precision:
      confirmationPrecision !== null &&
      confirmationPrecision >= thresholds.minimum_confirmation_precision,
    used_recall: usedRecall !== null && usedRecall >= thresholds.minimum_used_recall,
    correct_unused_retention:
      correctUnusedRetention !== null &&
      correctUnusedRetention >= thresholds.minimum_correct_unused_retention,
    abstention: abstention !== null && abstention <= thresholds.maximum_abstention,
    marginal_overhead:
      marginalOverheadP95 !== null &&
      marginalOverheadP95 <= thresholds.maximum_p95_marginal_overhead_ms,
    refined_rss:
      process.platform === "win32" ||
      (refinedRssP95 !== null && refinedRssP95 <= thresholds.maximum_p95_refined_rss_kb),
    independent_signoff: isObject(independentSignoff) && independentSignoff.verdict === "approved",
  };
  const summary = {
    schema_version: 2,
    gate: {
      go: Object.values(checks).every(Boolean),
      checks,
      thresholds,
      known_incorrect_removals: incorrectConfirmed.length,
      zero_controls_clean: zeroControlsClean,
      multiple_repositories: confirmedProjects.size >= 2,
      multiple_feature_buckets: featureBucketValue.multiple_feature_buckets,
    },
    accuracy: {
      candidate_count: entries.length,
      confirmed_used_count: confirmed.length,
      retained_count: retained.length,
      truth_counts: {
        used: used.length,
        unused: unused.length,
        indeterminate: entries.filter(({ truth }) => truth === "indeterminate").length,
      },
      confirmation_precision: confirmationPrecision,
      used_recall: usedRecall,
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
  if (summary.gate.go) {
    const publicationEvidence = {
      schema_version: 1,
      generated_from: {
        discovery_provenance: discovery.provenance,
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
      known_unused_retained: correctUnusedRetained.map((entry) => ({
        key: entry.key,
        project_id: entry.project_id,
        declaration: {
          path: entry.source_evidence.declaration.path,
          line: entry.source_evidence.declaration.line,
          col: entry.source_evidence.declaration.col,
        },
      })),
    };
    const evidencePath = resolve(REPO_ROOT, "benchmarks/type-aware-corpus-evidence.json");
    const summaryPath = resolve(REPO_ROOT, "benchmarks/type-aware-corpus-summary.md");
    const summaryMarkdown = renderSummaryMarkdown(summary);
    if (publicationMode === "write") {
      writeJson(evidencePath, publicationEvidence);
      writeFileSync(summaryPath, summaryMarkdown);
    } else if (
      JSON.stringify(readJson(evidencePath, "tracked corpus evidence")) !==
        JSON.stringify(publicationEvidence) ||
      readFileSync(summaryPath, "utf8") !== summaryMarkdown
    ) {
      fail("tracked corpus evidence or summary has generator drift; rerun summarize");
    }
  }
  return summary;
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
  if (options.command === "prepare") prepare(projects, options.outDir);
  else if (options.command === "discover") await discover(manifest, projects, options);
  else if (options.command === "measure") await measure(manifest, projects, options);
  else if (options.command === "focused") focused(options);
  else if (options.command === "evidence") await evidence(manifest, manifest.projects, options);
  else if (options.command === "adjudicate") adjudicate(options.outDir);
  else if (options.command === "verify-ledger") verifyLedger(options.outDir, manifest);
  else if (options.command === "summarize") summarize(manifest, options);
  else if (options.command === "verify-publication") {
    summarize(manifest, options, "verify");
    await supplemental(options, "verify");
  } else if (options.command === "supplemental") await supplemental(options);
  if (options.command === "verify-ledger") console.log("verify-ledger: evidence ledger is valid");
  else if (options.command === "verify-publication")
    console.log("verify-publication: tracked evidence matches the generator");
  else if (options.command === "supplemental")
    console.log(`supplemental: artifact written to ${SUPPLEMENTAL_ARTIFACT}`);
  else console.log(`${options.command}: artifacts written to ${options.outDir}`);
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
