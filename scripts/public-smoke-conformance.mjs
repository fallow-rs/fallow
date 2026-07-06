#!/usr/bin/env node
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { basename, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";

import { runCliMain } from "./cli-main.mjs";

const DEFAULT_MANIFEST = "scripts/public-smoke-projects.json";
const DEFAULT_OUT_DIR = "target/public-smoke-conformance";
const DEFAULT_CACHE_DIR = ".fallow/public-smoke/repos";

const createDefaultOptions = () => ({
  manifest: DEFAULT_MANIFEST,
  outDir: DEFAULT_OUT_DIR,
  cacheDir: DEFAULT_CACHE_DIR,
  clone: false,
  projectPaths: new Map(),
  rootDir: null,
  fallowBin: null,
});

const readValue = (argv, index, name) => {
  const value = argv[index + 1];
  if (!value) {
    throw new Error(`${name} requires a value`);
  }
  return value;
};

const optionSetters = new Map([
  ["--manifest", (options, value) => ({ ...options, manifest: value })],
  ["--out-dir", (options, value) => ({ ...options, outDir: value })],
  ["--cache-dir", (options, value) => ({ ...options, cacheDir: value })],
  ["--root-dir", (options, value) => ({ ...options, rootDir: value })],
  ["--fallow-bin", (options, value) => ({ ...options, fallowBin: value })],
]);

const setProjectPath = (options, value) => {
  const [id, path] = splitProjectPath(value);
  options.projectPaths.set(id, path);
  return options;
};

const parseArg = (argv, index, options) => {
  const arg = argv[index];
  if (arg === "--clone") {
    return { options: { ...options, clone: true }, nextIndex: index + 1 };
  }
  if (arg === "--project") {
    return { options: setProjectPath(options, readValue(argv, index, arg)), nextIndex: index + 2 };
  }
  if (arg.startsWith("--project=")) {
    return {
      options: setProjectPath(options, arg.slice("--project=".length)),
      nextIndex: index + 1,
    };
  }
  const setter = optionSetters.get(arg);
  if (setter) {
    return { options: setter(options, readValue(argv, index, arg)), nextIndex: index + 2 };
  }
  throw new Error(`Unknown argument: ${arg}`);
};

export const parseArgs = (argv) => {
  let options = createDefaultOptions();
  let index = 0;

  while (index < argv.length) {
    const parsed = parseArg(argv, index, options);
    options = parsed.options;
    index = parsed.nextIndex;
  }

  return options;
};

const splitProjectPath = (value) => {
  const separator = value.includes("=") ? "=" : ":";
  const index = value.indexOf(separator);
  if (index <= 0 || index === value.length - 1) {
    throw new Error("--project must look like id=/path/to/project");
  }
  return [value.slice(0, index), value.slice(index + 1)];
};

const loadManifest = (path) => JSON.parse(readFileSync(path, "utf8"));

const resolveProjectPath = (project, options) => {
  const explicit = options.projectPaths.get(project.id);
  if (explicit) {
    return resolve(explicit);
  }

  if (options.rootDir) {
    const candidate = resolve(options.rootDir, project.id);
    if (existsSync(candidate)) {
      return candidate;
    }
  }

  const cachePath = resolve(options.cacheDir, project.id);
  if (existsSync(cachePath)) {
    return cachePath;
  }

  return null;
};

const findFallowBin = (explicit) => {
  if (explicit) {
    return explicit;
  }
  for (const candidate of ["target/release/fallow", "target/debug/fallow"]) {
    if (existsSync(candidate)) {
      return resolve(candidate);
    }
  }
  return "fallow";
};

const cloneProject = (project, options) => {
  const destination = resolve(options.cacheDir, project.id);
  if (existsSync(join(destination, ".git"))) {
    return destination;
  }
  mkdirSync(options.cacheDir, { recursive: true });
  const result = spawnSync(
    "git",
    [
      "clone",
      "--depth",
      "1",
      "--branch",
      project.ref,
      "--single-branch",
      `https://github.com/${project.repo}.git`,
      destination,
    ],
    { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] },
  );
  if (result.status !== 0) {
    throw new Error(`clone failed for ${project.id}: ${result.stderr.trim()}`);
  }
  return destination;
};

const summarizeFallowOutput = (output) => {
  const parsed = JSON.parse(output);
  const summary = parsed.summary && typeof parsed.summary === "object" ? parsed.summary : {};
  const resultKeys = Object.entries(parsed)
    .filter(([, value]) => Array.isArray(value))
    .map(([key, value]) => [key, value.length])
    .filter(([, count]) => count > 0)
    .toSorted(([a], [b]) => a.localeCompare(b));

  return {
    kind: typeof parsed.kind === "string" ? parsed.kind : null,
    schema_version: parsed.schema_version ?? null,
    total_issues: Number.isFinite(summary.total_issues) ? summary.total_issues : null,
    issue_counts: Object.fromEntries(resultKeys),
  };
};

const runFallowCommand = (command) =>
  spawnSync(command[0], command.slice(1), {
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  });

const projectReportFields = (project) => ({
  id: project.id,
  label: project.label,
  category: project.category,
  repo: project.repo,
  ref: project.ref,
});

const failedProjectResult = (project, root, reason) => ({
  ...projectReportFields(project),
  status: "failed",
  reason,
  command: commandForReport(project),
  project_dir: basename(root),
});

const successfulProjectResult = (project, root, result, summary) => {
  const status =
    project.expected_kind && summary.kind !== project.expected_kind ? "failed" : "passed";
  return {
    ...projectReportFields(project),
    status,
    reason:
      status === "failed" ? `expected kind ${project.expected_kind}, got ${summary.kind}` : null,
    command: commandForReport(project),
    project_dir: basename(root),
    exit_code: result.status,
    summary,
  };
};

const runProject = (project, root, fallowBin) => {
  const command = [
    fallowBin,
    ...(project.command ?? ["dead-code"]),
    "--root",
    root,
    "--format",
    "json",
    "--quiet",
  ];
  const result = runFallowCommand(command);

  if (result.status === null || result.status >= 2) {
    return failedProjectResult(
      project,
      root,
      result.stderr.trim() || `fallow exited ${result.status}`,
    );
  }

  try {
    return successfulProjectResult(project, root, result, summarizeFallowOutput(result.stdout));
  } catch (error) {
    return failedProjectResult(
      project,
      root,
      error instanceof Error ? error.message : String(error),
    );
  }
};

const commandForReport = (project) =>
  `fallow ${(project.command ?? ["dead-code"]).join(" ")} --format json --quiet`;

const resolveRunnableRoot = (project, options) => {
  const root = resolveProjectPath(project, options);
  return root || (options.clone ? cloneProject(project, options) : null);
};

const skippedProjectResult = (project) => ({
  ...projectReportFields(project),
  status: "skipped",
  reason: "no local project path; pass --project id=/path, --root-dir, or --clone",
  command: commandForReport(project),
});

const runManifestProject = (project, options, fallowBin) => {
  const root = resolveRunnableRoot(project, options);
  return root ? runProject(project, root, fallowBin) : skippedProjectResult(project);
};

const countStatus = (results, status) =>
  results.filter((result) => result.status === status).length;

export const runPublicSmoke = (options) => {
  const manifest = loadManifest(options.manifest);
  const fallowBin = findFallowBin(options.fallowBin);
  const results = (manifest.projects ?? []).map((project) =>
    runManifestProject(project, options, fallowBin),
  );

  return {
    schema_version: 1,
    generated_at: new Date().toISOString(),
    artifact_policy: "compact summaries only; no source snippets and no absolute project paths",
    summary: {
      total: results.length,
      passed: countStatus(results, "passed"),
      failed: countStatus(results, "failed"),
      skipped: countStatus(results, "skipped"),
    },
    projects: results,
  };
};

const markdownProjectRow = (project) =>
  [
    project.id,
    project.category,
    project.status,
    project.summary?.kind ?? "-",
    project.summary?.total_issues ?? "-",
    project.reason ?? "-",
  ].join(" | ");

const renderMarkdown = (report) => {
  const rows = report.projects.map(markdownProjectRow);
  return [
    "# Public Smoke Conformance",
    "",
    `Status: ${report.summary.failed === 0 ? "pass" : "fail"}`,
    "",
    "| Project | Category | Status | Kind | Issues | Note |",
    "| --- | --- | --- | --- | ---: | --- |",
    ...rows.map((row) => `| ${row} |`),
    "",
    "Artifacts contain compact summaries only. Do not paste private project output into public docs.",
    "",
  ].join("\n");
};

export const main = (argv = process.argv.slice(2)) => {
  const options = parseArgs(argv);
  const report = runPublicSmoke(options);
  mkdirSync(options.outDir, { recursive: true });
  writeFileSync(
    join(options.outDir, "public-smoke-summary.json"),
    `${JSON.stringify(report, null, 2)}\n`,
  );
  writeFileSync(join(options.outDir, "public-smoke-summary.md"), renderMarkdown(report));
  console.log(
    `public smoke: ${report.summary.passed} passed, ${report.summary.failed} failed, ${report.summary.skipped} skipped`,
  );
  console.log(`artifacts: ${options.outDir}`);
  return report.summary.failed === 0 ? 0 : 1;
};

if (import.meta.url === `file://${process.argv[1]}`) {
  runCliMain(main);
}
