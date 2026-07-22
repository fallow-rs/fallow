#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { spawn, spawnSync } from "node:child_process";
import { dirname, isAbsolute, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(SCRIPT_DIR, "..");
const DEFAULT_MANIFEST = resolve(REPO_ROOT, "benchmarks/type-aware-corpus.json");
const DEFAULT_OUT_DIR = resolve(REPO_ROOT, "target/type-aware-corpus");
const DEFAULT_FALLOW_BIN = resolve(REPO_ROOT, "target/release/fallow");
const DEFAULT_RUNS = 4;
const DEFAULT_WARMUPS = 1;
const MAX_OUTPUT_BYTES = 256 * 1024 * 1024;
const TRUTH_STATUSES = new Set(["used", "unused", "indeterminate"]);
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
  writeFileSync(path, `${JSON.stringify(value, null, 2)}\n`);
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
  if (!new Set(["discover", "measure", "evidence", "verify-ledger", "summarize"]).has(command)) {
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
    projects: [],
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
  discover       Run baseline and refined discovery for the pinned corpus
  measure        Warm up, then alternate baseline/refined release runs
  evidence       Create or refresh the manual evidence ledger template
  verify-ledger  Require truth status and source evidence for every candidate
  summarize      Verify the ledger and write compact gate metrics

Options:
  --manifest PATH     Default: benchmarks/type-aware-corpus.json
  --project ID        Select one project, repeatable
  --fallow-bin PATH   Default: target/release/fallow
  --sidecar-bin PATH  Required for discover and measure
  --runs N            Measured pairs per project, default: ${DEFAULT_RUNS}
  --warmups N         Warmup pairs per project, default: ${DEFAULT_WARMUPS}

All artifacts are written below target/type-aware-corpus. Corpus projects are read-only.`;

const selectProjects = (manifest, selectedIds) => {
  if (selectedIds.length === 0) return manifest.projects;
  const selected = new Set(selectedIds);
  for (const id of selected) {
    if (!manifest.projects.some((project) => project.id === id)) fail(`unknown project: ${id}`);
  }
  return manifest.projects.filter((project) => selected.has(project.id));
};

const ensureFile = (path, description) => {
  if (!existsSync(path) || !statSync(path).isFile()) fail(`${description} not found: ${path}`);
};

const projectRoot = (project) => {
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

const executeFallow = async ({ project, fallowBin, sidecarBin, refined, rawPath }) => {
  const root = projectRoot(project);
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
    const baseline = await executeFallow({
      project,
      fallowBin: options.fallowBin,
      sidecarBin: options.sidecarBin,
      refined: false,
      rawPath: resolve(options.outDir, "discover", `${artifactProject}-baseline.json`),
    });
    const refined = await executeFallow({
      project,
      fallowBin: options.fallowBin,
      sidecarBin: options.sidecarBin,
      refined: true,
      rawPath: resolve(options.outDir, "discover", `${artifactProject}-refined.json`),
    });
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
    warmups: options.warmups,
    measured_pairs: options.runs,
    projects: projectReports,
  };
  writeJson(resolve(options.outDir, "measurements.json"), report);
  return report;
};

const readDiscovery = (outDir) => readJson(resolve(outDir, "discovery.json"), "discovery artifact");

const sourceLineEvidence = (project, candidate) => {
  const root = projectRoot(project);
  const sourcePath = resolve(root, candidate.path);
  const relativeToRoot = relative(root, sourcePath);
  if (relativeToRoot.startsWith(`..${sep}`) || relativeToRoot === "..") {
    fail(`${candidate.key} source path escapes its fixture`);
  }
  let excerpt = "";
  if (existsSync(sourcePath) && statSync(sourcePath).isFile()) {
    const lines = readFileSync(sourcePath, "utf8").split(/\r?\n/);
    excerpt = (lines[Math.max(0, candidate.line - 1)] ?? "").trim().slice(0, 240);
  }
  return { path: candidate.path, line: candidate.line, excerpt };
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

const evidence = (manifest, projects, options) => {
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
        source_evidence: old?.source_evidence ?? {
          declaration: sourceLineEvidence(project, candidate),
          uses: [],
          notes: null,
        },
      });
    }
  }
  entries.sort((left, right) => left.key.localeCompare(right.key));
  const ledger = {
    schema_version: 2,
    artifact_policy: "local adjudication only; never copy raw or private source into tracked files",
    corpus: discovery.corpus,
    candidates: entries,
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
    if (entry.semantic_status !== candidate.semantic_status)
      errors.push(`${key}: semantic_status does not match discovery`);
    if (
      !isObject(entry.source_evidence) ||
      !evidenceLocationValid(entry.source_evidence.declaration)
    ) {
      errors.push(`${key}: complete declaration source evidence is required`);
    } else if (entry.source_evidence.declaration.path !== candidate.path) {
      errors.push(`${key}: declaration evidence path must match the candidate path`);
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

const verifyLedger = (outDir, discovery = readDiscovery(outDir)) => {
  const ledger = readJson(resolve(outDir, "ledger.json"), "evidence ledger");
  if (JSON.stringify(discovery.corpus) !== JSON.stringify(ledger.corpus)) {
    fail("ledger corpus identity does not match discovery; refresh evidence");
  }
  const errors = verifyLedgerData(discovery, ledger);
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

const summarize = (manifest, outDir) => {
  const discovery = readDiscovery(outDir);
  requireCompletePublicationCorpus(manifest, discovery);
  const { ledger } = verifyLedger(outDir, discovery);
  const measurements = readJson(resolve(outDir, "measurements.json"), "measurements artifact");
  if (JSON.stringify(discovery.corpus) !== JSON.stringify(measurements.corpus)) {
    fail("measurement corpus identity does not match discovery; rerun measure");
  }
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
  const summary = {
    schema_version: 2,
    gate: {
      go:
        incorrectConfirmed.length === 0 &&
        zeroControlsClean &&
        confirmedProjects.size >= 2 &&
        featureBucketValue.multiple_feature_buckets,
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
      confirmation_precision: safeRatio(correctConfirmed.length, confirmed.length),
      used_recall: safeRatio(correctConfirmed.length, used.length),
      correct_unused_retention: safeRatio(correctUnusedRetained.length, unused.length),
      abstained_count: abstainedCount,
      abstention: safeRatio(abstainedCount, entries.length),
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
        p95: percentile(overheads, 0.95),
      },
      process_tree_peak_rss_kb: {
        baseline_p95: percentile(
          baselineRuns
            .map(({ peak_process_tree_rss_kb }) => peak_process_tree_rss_kb)
            .filter(Number.isFinite),
          0.95,
        ),
        refined_p95: percentile(
          refinedRuns
            .map(({ peak_process_tree_rss_kb }) => peak_process_tree_rss_kb)
            .filter(Number.isFinite),
          0.95,
        ),
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
  writeJson(resolve(outDir, "summary.json"), summary);
  writeFileSync(resolve(outDir, "summary.md"), renderSummaryMarkdown(summary));
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
  mkdirSync(options.outDir, { recursive: true });
  if (options.command === "discover") await discover(manifest, projects, options);
  else if (options.command === "measure") await measure(manifest, projects, options);
  else if (options.command === "evidence") evidence(manifest, manifest.projects, options);
  else if (options.command === "verify-ledger") verifyLedger(options.outDir);
  else if (options.command === "summarize") summarize(manifest, options.outDir);
  console.log(`${options.command}: artifacts written to target/type-aware-corpus`);
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
