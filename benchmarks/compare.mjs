#!/usr/bin/env node
/**
 * Deep comparison of fallow vs knip results on real-world projects.
 * Outputs detailed diffs per issue category.
 */
import { existsSync, readdirSync } from "node:fs";
import { join, resolve, dirname, isAbsolute, relative } from "node:path";
import { fileURLToPath } from "node:url";

import { inlineOption, readBenchCounts } from "../scripts/lib/options.mjs";
import { runTool } from "../scripts/lib/tool-run.mjs";

const __dirname = dirname(fileURLToPath(import.meta.url));
const rootDir = resolve(__dirname, "..");
const args = process.argv.slice(2);
readBenchCounts(args, {}, { values: ["--fixtures", "--fallow-bin"] });
const fixturesDir = resolve(
  inlineOption(args, "--fixtures") ?? join(__dirname, "fixtures", "real-world"),
);
const fallowBinOption =
  inlineOption(args, "--fallow-bin") ?? join(rootDir, "target", "release", "fallow");
// A bare name stays a name, so the run looks it up on PATH.
const fallowBin = /[\\/]/.test(fallowBinOption) ? resolve(fallowBinOption) : fallowBinOption;
const knipBin = join(__dirname, "node_modules", ".bin", "knip");

function normalizeProjectPath(projectDir, value) {
  if (typeof value === "string") {
    return isAbsolute(value) ? relative(projectDir, value) : value;
  }
  if (value && typeof value === "object") {
    for (const key of ["path", "file", "filename"]) {
      if (key in value) return normalizeProjectPath(projectDir, value[key]);
    }
    if ("location" in value) return normalizeProjectPath(projectDir, value.location);
  }
  return String(value);
}

function parseJsonOutput(stdout) {
  return JSON.parse(stdout.trimStart().replace(/^\uFEFF/, ""));
}

const run = (cmd, cmdArgs, cwd) =>
  runTool(cmd, cmdArgs, {
    cwd,
    timeout: 300000,
    maxBuffer: 50 * 1024 * 1024,
    env: { ...process.env, NO_COLOR: "1", FORCE_COLOR: "0" },
    okStatuses: [0, 1],
  });

const failedProject = (reason) => {
  console.log(`  FAILED: ${reason}`);
  return { ok: false, reason };
};

const parseOutput = (tool, parse, result, dir) => {
  try {
    return { ok: true, value: parse(result.stdout, dir) };
  } catch (error) {
    return { ok: false, reason: `${tool} parse error: ${error.message}` };
  }
};

function parseFallow(stdout, projectDir) {
  const data = parseJsonOutput(stdout);
  const result = {
    unused_files: new Set(),
    unused_exports: new Set(),
    unused_types: new Set(),
    unused_dependencies: new Set(),
    unused_dev_dependencies: new Set(),
    unresolved_imports: new Set(),
    unlisted_dependencies: new Set(),
    unused_enum_members: new Set(),
    duplicate_exports: new Set(),
  };
  for (const f of data.unused_files ?? [])
    result.unused_files.add(normalizeProjectPath(projectDir, f.path));
  for (const e of data.unused_exports ?? [])
    result.unused_exports.add(`${normalizeProjectPath(projectDir, e.path)}:${e.export_name}`);
  for (const t of data.unused_types ?? [])
    result.unused_types.add(`${normalizeProjectPath(projectDir, t.path)}:${t.export_name}`);
  for (const d of data.unused_dependencies ?? [])
    result.unused_dependencies.add(d.package_name ?? d.name);
  for (const d of data.unused_dev_dependencies ?? [])
    result.unused_dev_dependencies.add(d.package_name ?? d.name);
  for (const u of data.unresolved_imports ?? [])
    result.unresolved_imports.add(`${normalizeProjectPath(projectDir, u.path)}:${u.specifier}`);
  for (const u of data.unlisted_dependencies ?? [])
    result.unlisted_dependencies.add(
      u.package_name ?? `${normalizeProjectPath(projectDir, u.path)}:${u.specifier}`,
    );
  for (const e of data.unused_enum_members ?? [])
    result.unused_enum_members.add(
      `${normalizeProjectPath(projectDir, e.path)}:${e.enum_name}.${e.member_name}`,
    );
  for (const d of data.duplicate_exports ?? [])
    result.duplicate_exports.add(
      `${d.export_name}@${(d.locations ?? d.files ?? []).map((f) => normalizeProjectPath(projectDir, f)).join(",")}`,
    );
  return result;
}

function parseKnip(stdout, _projectDir) {
  const data = parseJsonOutput(stdout);
  const result = {
    unused_files: new Set(),
    unused_exports: new Set(),
    unused_types: new Set(),
    unused_dependencies: new Set(),
    unused_dev_dependencies: new Set(),
    unresolved_imports: new Set(),
    unlisted_dependencies: new Set(),
    unused_enum_members: new Set(),
    duplicate_exports: new Set(),
  };
  // Knip "files" = unused files
  for (const f of data.files ?? []) result.unused_files.add(f);
  // Knip "issues" = per-file issues
  for (const issue of data.issues ?? []) {
    for (const d of issue.dependencies ?? []) result.unused_dependencies.add(d.name);
    for (const d of issue.devDependencies ?? []) result.unused_dev_dependencies.add(d.name);
    for (const e of issue.exports ?? []) result.unused_exports.add(`${issue.file}:${e.name}`);
    for (const t of issue.types ?? []) result.unused_types.add(`${issue.file}:${t.name}`);
    for (const u of issue.unresolved ?? [])
      result.unresolved_imports.add(`${issue.file}:${u.name}`);
    for (const u of issue.unlisted ?? [])
      result.unlisted_dependencies.add(`${issue.file}:${u.name}`);
    for (const [enumName, members] of Object.entries(issue.enumMembers ?? {})) {
      for (const m of members)
        result.unused_enum_members.add(`${issue.file}:${enumName}.${m.name}`);
    }
    for (const d of issue.duplicates ?? []) result.duplicate_exports.add(`${d.name}@${issue.file}`);
  }
  return result;
}

function diffSets(fallowSet, knipSet) {
  const onlyFallow = [...fallowSet].filter((x) => !knipSet.has(x)).toSorted();
  const onlyKnip = [...knipSet].filter((x) => !fallowSet.has(x)).toSorted();
  const both = [...fallowSet].filter((x) => knipSet.has(x)).toSorted();
  return { onlyFallow, onlyKnip, both };
}

function printCategory(name, fallowSet, knipSet) {
  const { onlyFallow, onlyKnip, both } = diffSets(fallowSet, knipSet);
  if (both.length === 0 && onlyFallow.length === 0 && onlyKnip.length === 0) return;
  console.log(`\n  ### ${name}`);
  console.log(
    `  Both: ${both.length} | Only fallow: ${onlyFallow.length} | Only knip: ${onlyKnip.length}`,
  );
  if (onlyFallow.length > 0) {
    console.log(`  --- Only in fallow (${onlyFallow.length}):`);
    for (const item of onlyFallow.slice(0, 30)) console.log(`    + ${item}`);
    if (onlyFallow.length > 30) console.log(`    ... and ${onlyFallow.length - 30} more`);
  }
  if (onlyKnip.length > 0) {
    console.log(`  --- Only in knip (${onlyKnip.length}):`);
    for (const item of onlyKnip.slice(0, 30)) console.log(`    - ${item}`);
    if (onlyKnip.length > 30) console.log(`    ... and ${onlyKnip.length - 30} more`);
  }
}

function compareProject(name, dir) {
  console.log(`\n${"=".repeat(60)}`);
  console.log(`PROJECT: ${name}`);
  console.log(`${"=".repeat(60)}`);

  const fr = run(fallowBin, ["dead-code", "--quiet", "--format", "json", "--no-cache"], dir);
  if (!fr.ok) return failedProject(`fallow: ${fr.reason}`);
  const kr = run(knipBin, ["--reporter", "json"], dir);
  if (!kr.ok) return failedProject(`knip: ${kr.reason}`);

  const fallowParsed = parseOutput("fallow", parseFallow, fr, dir);
  if (!fallowParsed.ok) return failedProject(fallowParsed.reason);
  const knipParsed = parseOutput("knip", parseKnip, kr, dir);
  if (!knipParsed.ok) return failedProject(knipParsed.reason);
  const fallow = fallowParsed.value;
  const knip = knipParsed.value;

  const categories = [
    "unused_files",
    "unused_exports",
    "unused_types",
    "unused_dependencies",
    "unused_dev_dependencies",
    "unresolved_imports",
    "unlisted_dependencies",
    "unused_enum_members",
    "duplicate_exports",
  ];

  const summary = {};
  for (const cat of categories) {
    const { onlyFallow, onlyKnip, both } = diffSets(fallow[cat], knip[cat]);
    summary[cat] = { both: both.length, onlyFallow: onlyFallow.length, onlyKnip: onlyKnip.length };
    printCategory(cat, fallow[cat], knip[cat]);
  }
  return { ok: true, summary };
}

if (!existsSync(fixturesDir)) {
  console.error("No real-world fixtures. Run: npm run download-fixtures");
  process.exit(1);
}

const results = {};
for (const p of readdirSync(fixturesDir)
  .filter((x) => existsSync(join(fixturesDir, x, "package.json")))
  .toSorted()) {
  results[p] = compareProject(p, join(fixturesDir, p));
}

const printSummary = (project, summary) => {
  console.log(`${project}:`);
  for (const [cat, counts] of Object.entries(summary)) {
    if (counts.both === 0 && counts.onlyFallow === 0 && counts.onlyKnip === 0) continue;
    const knipComparable = counts.both + counts.onlyKnip;
    const pct =
      knipComparable > 0 ? `${((counts.both / knipComparable) * 100).toFixed(0)}%` : "n/a";
    console.log(
      `  ${cat}: agree=${counts.both} fallow-only=${counts.onlyFallow} knip-only=${counts.onlyKnip} (${pct} knip coverage)`,
    );
  }
};

// Final summary table
console.log(`\n${"=".repeat(60)}`);
console.log("OVERALL SUMMARY");
console.log(`${"=".repeat(60)}\n`);
const entries = Object.entries(results);
for (const [project, result] of entries) {
  if (result.ok) printSummary(project, result.summary);
}
const failed = entries.filter(([, result]) => !result.ok);
if (failed.length > 0) {
  console.log("\nFailed projects:");
  for (const [project, result] of failed) console.log(`  ${project}: ${result.reason}`);
}
if (failed.length === entries.length) {
  console.error("compare: no project gave a usable report");
  process.exit(1);
}
