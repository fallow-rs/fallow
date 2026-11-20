#!/usr/bin/env node
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { basename, join, resolve } from "node:path";

import { cargoFallowBin } from "./cargo-target.mjs";
import { runCliMain } from "./cli-main.mjs";
import { parseOptions } from "./lib/options.mjs";
import { fallowIssueCounts, fallowIssueTotal } from "./lib/report-counts.mjs";
import { parseJsonReport, runTool } from "./lib/tool-run.mjs";
import { pathToFileURL } from "node:url";

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

const OPTION_SPEC = {
  "--manifest": { key: "manifest", type: "string" },
  "--out-dir": { key: "outDir", type: "string" },
  "--cache-dir": { key: "cacheDir", type: "string" },
  "--root-dir": { key: "rootDir", type: "string" },
  "--fallow-bin": { key: "fallowBin", type: "string" },
  "--project": { key: "projects", type: "string", repeat: true },
  "--clone": { key: "clone", type: "flag" },
};

export const parseArgs = (argv) => {
  const { projects = [], ...options } = parseOptions(argv, OPTION_SPEC, createDefaultOptions());
  return { ...options, projectPaths: new Map(projects.map(splitProjectPath)) };
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
  for (const candidate of [
    cargoFallowBin(process.cwd(), "release"),
    cargoFallowBin(process.cwd()),
  ]) {
    if (existsSync(candidate)) {
      return candidate;
    }
  }
  return "fallow";
};

const MAX_REASON_LENGTH = 500;

const cloneProject = (project, options) => {
  const dir = resolve(options.cacheDir, project.id);
  if (existsSync(join(dir, ".git"))) {
    return { ok: true, dir };
  }
  mkdirSync(options.cacheDir, { recursive: true });
  const result = runTool("git", [
    "clone",
    "--depth",
    "1",
    "--branch",
    project.ref,
    "--single-branch",
    `https://github.com/${project.repo}.git`,
    dir,
  ]);
  if (!result.ok) {
    const detail = result.stderr.trim() || result.reason;
    return { ok: false, dir, reason: `clone failed for ${project.id}: ${detail}` };
  }
  return { ok: true, dir };
};

/**
 * Summarizes the JSON output of one Fallow command: its kind, schema version,
 * `total_issues` and the category counts of its `summary`. Throws when the
 * output is not a JSON object.
 */
export const summarizeFallowOutput = (output) => {
  const parsed = parseJsonReport(output);
  if (!parsed.ok) throw new Error(parsed.error);
  const report = parsed.value;
  return {
    kind: typeof report.kind === "string" ? report.kind : null,
    schema_version: report.schema_version ?? null,
    total_issues: fallowIssueTotal(report),
    issue_counts: fallowIssueCounts(report),
  };
};

const runFallowCommand = (command) =>
  runTool(command[0], command.slice(1), { maxBuffer: 64 * 1024 * 1024, okStatuses: [0, 1] });

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
  reason: reason.slice(0, MAX_REASON_LENGTH),
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

  if (!result.ok) {
    return failedProjectResult(project, root, result.stderr.trim() || result.reason);
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
  if (root) return { ok: true, dir: root };
  return options.clone ? cloneProject(project, options) : null;
};

const skippedProjectResult = (project) => ({
  ...projectReportFields(project),
  status: "skipped",
  reason: "no local project path; pass --project id=/path, --root-dir, or --clone",
  command: commandForReport(project),
});

const runManifestProject = (project, options, fallowBin) => {
  const root = resolveRunnableRoot(project, options);
  if (!root) return skippedProjectResult(project);
  if (!root.ok) return failedProjectResult(project, root.dir, root.reason);
  return runProject(project, root.dir, fallowBin);
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

// A run where every project is skipped compared nothing, so it must not read
// as a pass.
const reportStatus = (summary) => {
  if (summary.failed > 0) {
    return { label: "fail", exitCode: 1 };
  }
  if (summary.passed === 0) {
    return { label: "no project ran", exitCode: 2 };
  }
  return { label: "pass", exitCode: 0 };
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
    `Status: ${reportStatus(report.summary).label}`,
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
  const status = reportStatus(report.summary);
  if (status.exitCode === 2) {
    console.error("public smoke: no project ran; pass --clone, --root-dir, or --project id=/path");
  }
  return status.exitCode;
};

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  runCliMain(main);
}
