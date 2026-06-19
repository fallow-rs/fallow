#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readdirSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";
import { performance } from "node:perf_hooks";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const rootDir = resolve(__dirname, "..");
const args = process.argv.slice(2);
const RUNS = Number.parseInt(args.find((arg) => arg.startsWith("--runs="))?.split("=")[1] ?? "1");
const SKIP_BUILD = args.includes("--skip-build");
const SAMPLE_COUNT = Number.parseInt(
  args.find((arg) => arg.startsWith("--samples="))?.split("=")[1] ?? "0",
);
const writeReprosArg = args.find((arg) => arg.startsWith("--write-repros="))?.split("=")[1];
const writeReprosDir = writeReprosArg ? resolve(writeReprosArg) : null;
const projectsArg = args.find((arg) => arg.startsWith("--projects="))?.split("=")[1];
const projectFilter = projectsArg
  ? new Set(
      projectsArg
        .split(",")
        .map((project) => project.trim())
        .filter(Boolean),
    )
  : null;

const fallowBin = join(rootDir, "target", "release", "fallow");

if (!SKIP_BUILD) {
  const build = spawnSync("cargo", ["build", "--release"], {
    cwd: rootDir,
    stdio: "pipe",
    timeout: 300000,
  });
  if (build.status !== 0) {
    console.error(build.stderr?.toString() ?? "release build failed");
    process.exit(1);
  }
}

if (!existsSync(fallowBin)) {
  console.error("release binary not found. Run without --skip-build first.");
  process.exit(1);
}

const fixturesRoot = join(__dirname, "fixtures", "real-world");
if (!existsSync(fixturesRoot)) {
  console.error("real-world fixtures not found. Run: npm run download-fixtures");
  process.exit(1);
}

const projects = readdirSync(fixturesRoot)
  .filter((name) => existsSync(join(fixturesRoot, name, "package.json")))
  .filter((name) => !projectFilter || projectFilter.has(name))
  .toSorted();

if (projects.length === 0) {
  console.error("no projects matched");
  process.exit(1);
}

const results = projects.map((project) => compareProject(project, join(fixturesRoot, project)));

console.table(
  results.map((result) => ({
    Project: result.project,
    Files: result.files,
    "Default median": fmt(result.defaultMedian),
    "Rolling median": fmt(result.rollingMedian),
    Speedup: `${(result.defaultMedian / result.rollingMedian).toFixed(2)}x`,
    "Exact missing": result.exactMissing,
    "Exact extra": result.exactExtra,
    "Coverage missing": result.coverageMissing,
    "Coverage extra": result.coverageExtra,
  })),
);

for (const result of results) {
  if (SAMPLE_COUNT > 0 && result.samples.hasDrift) {
    printSamples(result);
  }
  if (writeReprosDir && result.samples.hasDrift) {
    writeSampleRepro(result);
  }
}

function compareProject(project, dir) {
  const defaultRuns = [];
  const rollingRuns = [];

  for (let index = 0; index < RUNS; index++) {
    defaultRuns.push(runFallow(dir, false));
    rollingRuns.push(runFallow(dir, true));
  }

  const defaultLines = parseCompact(defaultRuns.at(-1).stdout);
  const rollingLines = parseCompact(rollingRuns.at(-1).stdout);
  const comparison = compareLines(defaultLines, rollingLines);

  return {
    project,
    files: countSourceFiles(dir),
    defaultMedian: median(defaultRuns.map((run) => run.elapsed)),
    rollingMedian: median(rollingRuns.map((run) => run.elapsed)),
    exactMissing: comparison.missing.length,
    exactExtra: comparison.extra.length,
    coverageMissing: comparison.coverageMissing.length,
    coverageExtra: comparison.coverageExtra.length,
    dir,
    samples: comparison,
  };
}

function runFallow(dir, rolling) {
  const env = rolling ? { ...process.env, FALLOW_DUPES_ROLLING: "1" } : process.env;
  const start = performance.now();
  const result = spawnSync(
    fallowBin,
    ["dupes", "--format", "compact", "--quiet", "--no-cache", "--root", dir],
    {
      cwd: rootDir,
      env,
      stdio: "pipe",
      timeout: 600000,
      maxBuffer: 100 * 1024 * 1024,
    },
  );
  const elapsed = performance.now() - start;
  if (result.status !== 0) {
    console.error(result.stderr?.toString() ?? "fallow dupes failed");
    process.exit(result.status ?? 1);
  }
  return { elapsed, stdout: result.stdout?.toString() ?? "" };
}

function parseCompact(stdout) {
  return stdout
    .trim()
    .split("\n")
    .filter(Boolean)
    .map((line) => {
      const normalized = line.replace(/^clone-group-\d+:/, "");
      const match = normalized.match(/^(.*):(\d+)-(\d+):(\d+)tokens$/);
      if (!match) {
        throw new Error(`unexpected compact line: ${line}`);
      }
      return {
        key: normalized,
        file: match[1],
        start: Number.parseInt(match[2]),
        end: Number.parseInt(match[3]),
        tokens: Number.parseInt(match[4]),
      };
    });
}

function compareLines(defaultLines, rollingLines) {
  const defaultSet = new Set(defaultLines.map((line) => line.key));
  const rollingSet = new Set(rollingLines.map((line) => line.key));
  const missing = defaultLines.filter((line) => !rollingSet.has(line.key));
  const extra = rollingLines.filter((line) => !defaultSet.has(line.key));
  const defaultByFile = groupByFile(defaultLines);
  const rollingByFile = groupByFile(rollingLines);

  return {
    missing,
    extra,
    coverageMissing: uncoveredBy(missing, rollingByFile),
    coverageExtra: uncoveredBy(extra, defaultByFile),
    hasDrift: missing.length > 0 || extra.length > 0,
  };
}

function printSamples(result) {
  console.log(`\n### ${result.project} drift samples`);
  console.log("Top missing files:", formatTopFiles(result.samples.missing));
  console.log("Top extra files:", formatTopFiles(result.samples.extra));
  printLineSamples("Coverage missing", result.samples.coverageMissing);
  printLineSamples("Coverage extra", result.samples.coverageExtra);
}

function formatTopFiles(lines) {
  const counts = new Map();
  for (const line of lines) {
    counts.set(line.file, (counts.get(line.file) ?? 0) + 1);
  }
  return [...counts]
    .toSorted((left, right) => right[1] - left[1] || left[0].localeCompare(right[0]))
    .slice(0, SAMPLE_COUNT)
    .map(([file, count]) => `${file} (${count})`)
    .join(", ");
}

function printLineSamples(label, lines) {
  console.log(`${label}:`);
  for (const line of lines.slice(0, SAMPLE_COUNT)) {
    console.log(`  ${line.file}:${line.start}-${line.end}:${line.tokens}tokens`);
  }
}

function writeSampleRepro(result) {
  const sampleLimit = SAMPLE_COUNT > 0 ? SAMPLE_COUNT : 10;
  const lines = [
    ...result.samples.missing.slice(0, sampleLimit),
    ...result.samples.extra.slice(0, sampleLimit),
    ...result.samples.coverageMissing.slice(0, sampleLimit),
    ...result.samples.coverageExtra.slice(0, sampleLimit),
  ];
  const files = [...new Set(lines.map((line) => line.file))].toSorted();
  if (files.length === 0) {
    return;
  }

  const reproDir = join(writeReprosDir, result.project);
  rmSync(reproDir, { recursive: true, force: true });
  mkdirSync(reproDir, { recursive: true });
  writeFileSync(
    join(reproDir, "package.json"),
    `${JSON.stringify({ name: `${result.project}-dupes-repro`, version: "1.0.0" }, null, 2)}\n`,
  );

  let copied = 0;
  for (const file of files) {
    const source = resolveSampleSource(result.dir, file);
    if (!source) {
      console.warn(`Skipping missing sampled file: ${file}`);
      continue;
    }
    const target = join(reproDir, decodeSamplePath(file));
    mkdirSync(dirname(target), { recursive: true });
    copyFileSync(source, target);
    copied += 1;
  }

  console.log(`Wrote ${copied} sampled repro files to ${reproDir}`);
}

function resolveSampleSource(root, file) {
  const direct = join(root, file);
  if (existsSync(direct)) {
    return direct;
  }
  const decoded = join(root, decodeSamplePath(file));
  return existsSync(decoded) ? decoded : null;
}

function decodeSamplePath(file) {
  try {
    return decodeURIComponent(file);
  } catch {
    return file;
  }
}

function groupByFile(lines) {
  const grouped = new Map();
  for (const line of lines) {
    const group = grouped.get(line.file) ?? [];
    group.push(line);
    grouped.set(line.file, group);
  }
  return grouped;
}

function uncoveredBy(lines, otherByFile) {
  const uncovered = [];
  for (const line of lines) {
    const candidates = otherByFile.get(line.file) ?? [];
    if (!candidates.some((other) => other.start <= line.start && other.end >= line.end)) {
      uncovered.push(line);
    }
  }
  return uncovered;
}

function countSourceFiles(dir) {
  let count = 0;
  const walk = (current) => {
    for (const entry of readdirSync(current)) {
      if (["node_modules", ".git", "dist", "report"].includes(entry)) continue;
      const path = join(current, entry);
      const stat = statSync(path);
      if (stat.isDirectory()) {
        walk(path);
      } else if (/\.(ts|tsx|js|jsx|mjs|cjs)$/.test(entry)) {
        count += 1;
      }
    }
  };
  walk(dir);
  return count;
}

function median(values) {
  const sorted = [...values].toSorted((left, right) => left - right);
  const middle = Math.floor(sorted.length / 2);
  return sorted.length % 2 === 0 ? (sorted[middle - 1] + sorted[middle]) / 2 : sorted[middle];
}

function fmt(ms) {
  return ms < 1000 ? `${ms.toFixed(0)}ms` : `${(ms / 1000).toFixed(2)}s`;
}
