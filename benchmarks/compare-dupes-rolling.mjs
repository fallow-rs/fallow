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

import { inlineOption, readBenchCounts } from "../scripts/lib/options.mjs";

const __dirname = dirname(fileURLToPath(import.meta.url));
const rootDir = resolve(__dirname, "..");
const fallowBin = join(rootDir, "target", "release", "fallow");
const ROLLING_ENV = "FALLOW_DUPES_ROLLING";
// `fallow dupes --format compact` prints one line per clone instance:
// code-duplication:<path>:<start>-<end>:fingerprint=...,group=N,tokens=N,...
// The path may contain a colon, so the range anchors on the `fingerprint=` field.
const COMPACT_LINE = /^code-duplication:(.+):(\d+)-(\d+):(fingerprint=.*)$/;

let SAMPLE_COUNT = 0;
let writeReprosDir = null;

const fail = (message) => {
  console.error(message);
  process.exit(1);
};

const parseOptions = (args) => {
  const counts = readBenchCounts(
    args,
    { runs: "1", samples: "0" },
    { flags: ["--skip-build"], values: ["--write-repros", "--projects"] },
  );
  const writeReprosArg = inlineOption(args, "--write-repros");
  const projectsArg = inlineOption(args, "--projects");
  const projectNames = (projectsArg ?? "")
    .split(",")
    .map((project) => project.trim())
    .filter(Boolean);
  return {
    runs: counts.runs,
    samples: counts.samples,
    skipBuild: args.includes("--skip-build"),
    writeReprosDir: writeReprosArg ? resolve(writeReprosArg) : null,
    projectFilter: projectsArg ? new Set(projectNames) : null,
  };
};

const ensureBinary = (skipBuild) => {
  if (!skipBuild) {
    const build = spawnSync("cargo", ["build", "--release"], {
      cwd: rootDir,
      stdio: "pipe",
      timeout: 300000,
    });
    if (build.status !== 0) fail(build.stderr?.toString() ?? "release build failed");
  }
  if (!existsSync(fallowBin)) fail("release binary not found. Run without --skip-build first.");
};

const listProjects = (projectFilter) => {
  const fixturesRoot = join(__dirname, "fixtures", "real-world");
  if (!existsSync(fixturesRoot)) {
    fail("real-world fixtures not found. Run: npm run download-fixtures");
  }
  const projects = readdirSync(fixturesRoot)
    .filter((name) => existsSync(join(fixturesRoot, name, "package.json")))
    .filter((name) => !projectFilter || projectFilter.has(name))
    .toSorted();
  if (projects.length === 0) fail("no projects matched");
  return projects.map((project) => ({ project, dir: join(fixturesRoot, project) }));
};

const printResults = (results) => {
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
  for (const result of results.filter((entry) => entry.samples.hasDrift)) {
    if (SAMPLE_COUNT > 0) printSamples(result);
    if (writeReprosDir) writeSampleRepro(result);
  }
};

const main = () => {
  const options = parseOptions(process.argv.slice(2));
  SAMPLE_COUNT = options.samples;
  writeReprosDir = options.writeReprosDir;
  ensureBinary(options.skipBuild);
  const results = listProjects(options.projectFilter).map(({ project, dir }) =>
    compareProject(project, dir, options.runs),
  );
  printResults(results);
};

function compareProject(project, dir, runs) {
  const defaultRuns = [];
  const rollingRuns = [];

  for (let index = 0; index < runs; index++) {
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

/**
 * Gives the environment of one run. The engine turns the rolling detector on
 * when the variable exists, whatever its value, so the reference run removes it.
 */
export const fallowEnv = (rolling, baseEnv = process.env) => {
  const env = { ...baseEnv };
  if (rolling) {
    env[ROLLING_ENV] = "1";
  } else {
    delete env[ROLLING_ENV];
  }
  return env;
};

function runFallow(dir, rolling) {
  const start = performance.now();
  const result = spawnSync(
    fallowBin,
    ["dupes", "--format", "compact", "--quiet", "--no-cache", "--root", dir],
    {
      cwd: rootDir,
      env: fallowEnv(rolling),
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

/**
 * Reads the clone instances of a compact duplication report. The key leaves
 * out the group number, because the two detectors can number groups apart.
 */
export const parseCompact = (stdout) =>
  stdout
    .trim()
    .split("\n")
    .filter(Boolean)
    .map((line) => {
      const match = line.match(COMPACT_LINE);
      const fields = match ? new Map(match[4].split(",").map((field) => field.split("="))) : null;
      const tokens = Number.parseInt(fields?.get("tokens") ?? "", 10);
      if (!match || Number.isNaN(tokens)) {
        throw new Error(`unexpected compact line: ${line}`);
      }
      const [, file, start, end] = match;
      return {
        key: `${file}:${start}-${end}:${tokens}tokens`,
        file,
        start: Number.parseInt(start, 10),
        end: Number.parseInt(end, 10),
        tokens,
      };
    });

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

if (process.argv[1] !== undefined && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main();
}
