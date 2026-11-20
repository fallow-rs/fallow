#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import { existsSync, readdirSync, statSync, rmSync } from "node:fs";
import { join, resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import os from "node:os";

import {
  formatMs as fmt,
  sampleOutcome,
  sampleSummary,
  stats,
  timeCells,
  timeSample,
} from "../scripts/lib/bench-samples.mjs";
import { readBenchArgs } from "../scripts/lib/options.mjs";
import { fallowIssueTotal, knipIssueTotal } from "../scripts/lib/report-counts.mjs";
import { parseJsonReport } from "../scripts/lib/tool-run.mjs";

const __dirname = dirname(fileURLToPath(import.meta.url));
const rootDir = resolve(__dirname, "..");
const args = process.argv.slice(2);
const {
  runSynthetic,
  runRealWorld,
  projectFilter,
  runs: RUNS,
  warmup: WARMUP,
} = readBenchArgs(args, { runs: "5", warmup: "2" });

console.log("Building fallow (release)...");
// No timeout: a cold release build can take many minutes. Cargo writes its
// progress and errors straight to the terminal.
const buildResult = spawnSync("cargo", ["build", "--release"], {
  cwd: rootDir,
  stdio: ["ignore", "inherit", "inherit"],
});
if (buildResult.error || buildResult.status !== 0) {
  const reason = buildResult.error
    ? buildResult.error.message
    : buildResult.signal
      ? `cargo was stopped by ${buildResult.signal}`
      : `cargo exited with status ${buildResult.status}`;
  console.error(`Build failed: ${reason}`);
  process.exit(1);
}
const fallowBin = join(rootDir, "target", "release", "fallow");
const knipBin = join(__dirname, "node_modules", ".bin", "knip");
if (!existsSync(knipBin)) {
  console.error("knip not found. Run: cd benchmarks && npm install");
  process.exit(1);
}

const fallowVersion = spawnSync(fallowBin, ["--version"], { stdio: "pipe" })
  .stdout?.toString()
  .trim();
const knipVersion = spawnSync(knipBin, ["--version"], { stdio: "pipe" }).stdout?.toString().trim();
const rustVersion = spawnSync("rustc", ["--version"], { stdio: "pipe" }).stdout?.toString().trim();

console.log(`\n=== Fallow vs Knip Benchmark Suite ===\n`);
printEnvironment();
console.log(
  `Tools:\n  fallow   ${fallowVersion}\n  knip     ${knipVersion}\nConfig: ${RUNS} runs, ${WARMUP} warmup\n`,
);

function printEnvironment() {
  const cpus = os.cpus();
  console.log("Environment:");
  console.log(`  CPU:     ${cpus[0].model.trim()} (${cpus.length} logical cores)`);
  console.log(`  RAM:     ${(os.totalmem() / 1024 / 1024 / 1024).toFixed(1)} GB`);
  console.log(`  OS:      ${os.platform()} ${os.release()} ${os.arch()}`);
  console.log(`  Node:    ${process.version}`);
  console.log(`  Rust:    ${rustVersion}`);
  console.log("");
}

function countSourceFiles(dir) {
  let count = 0;
  const walk = (d) => {
    try {
      for (const e of readdirSync(d)) {
        if (["node_modules", ".git", "dist"].includes(e)) continue;
        const f = join(d, e);
        try {
          const s = statSync(f);
          if (s.isDirectory()) walk(f);
          else if (/\.(ts|tsx|js|jsx|mjs|cjs)$/.test(e)) count++;
        } catch {}
      }
    } catch {}
  };
  walk(dir);
  return count;
}

const timeRun = (cmd, cmdArgs, cwd) => timeSample(cmd, cmdArgs, { cwd, timeout: 300000 });
const timeRunWithMemory = (cmd, cmdArgs, cwd) =>
  timeSample(cmd, cmdArgs, { cwd, timeout: 300000, memory: true });

function firstDiagnosticLine(text) {
  const lines = text
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);
  return (
    lines.find((line) => /error|syntaxerror|exception|cannot|failed|timed out/i.test(line)) ??
    lines.find((line) => !line.startsWith("at ")) ??
    null
  );
}

const fallowIssueCount = (report) => fallowIssueTotal(report) ?? "?";

function summarizeBenchmarkRun(result, countIssues) {
  const report = parseJsonReport(result.stdout);
  const outcome = sampleOutcome(result, report);
  if (!outcome.valid) {
    const detail = firstDiagnosticLine(result.stderr) ?? firstDiagnosticLine(result.stdout);
    return {
      valid: false,
      issues: "error",
      error: detail ? `${outcome.reason}; ${detail}` : outcome.reason,
    };
  }
  return { valid: true, issues: countIssues(report.value), error: null };
}

function fmtMem(bytes) {
  if (bytes === 0) return "?";
  const mb = bytes / 1024 / 1024;
  return mb < 1024 ? `${mb.toFixed(1)} MB` : `${(mb / 1024).toFixed(2)} GB`;
}

function formatMultiplier(value) {
  return `${value.toFixed(1)}x`;
}

function relativeSpeed(fallowMedian, comparisonMedian, comparisonName) {
  if (fallowMedian <= comparisonMedian) {
    return `fallow ${formatMultiplier(comparisonMedian / fallowMedian)}`;
  }

  return `${comparisonName} ${formatMultiplier(fallowMedian / comparisonMedian)}`;
}

function clearFallowCache(dir) {
  const cacheDir = join(dir, ".fallow");
  if (existsSync(cacheDir)) rmSync(cacheDir, { recursive: true });
}

function benchmarkProject(name, dir) {
  const files = countSourceFiles(dir);
  console.log(`### ${name} (${files} source files)\n`);

  // Cold cache, no persisted fallow cache.
  const fArgsCold = ["dead-code", "--quiet", "--format", "json", "--no-cache"];
  const kArgs = ["--reporter", "json"];
  for (let i = 0; i < WARMUP; i++) {
    timeRun(fallowBin, fArgsCold, dir);
    timeRun(knipBin, kArgs, dir);
  }

  const fTimesCold = [],
    kTimes = [];
  let fIssues = "?",
    kIssues = "error",
    fPeakRss = 0,
    kPeakRss = 0;
  let kErrorReason = null;

  for (let i = 0; i < RUNS; i++) {
    const fr = timeRunWithMemory(fallowBin, fArgsCold, dir);
    const fSummary = summarizeBenchmarkRun(fr, fallowIssueCount);
    if (!fSummary.valid) throw new Error(`[${name}] fallow cold run failed: ${fSummary.error}`);
    fTimesCold.push(fr.elapsed);
    if (i === 0) {
      fIssues = fSummary.issues;
      fPeakRss = fr.peakRssBytes;
    }
    const kr = timeRunWithMemory(knipBin, kArgs, dir);
    const kSummary = summarizeBenchmarkRun(kr, knipIssueTotal);
    if (kSummary.valid) {
      kTimes.push(kr.elapsed);
      if (kIssues === "error") {
        kIssues = kSummary.issues;
        kPeakRss = kr.peakRssBytes;
      }
    } else if (kErrorReason == null) {
      kErrorReason = kSummary.error;
    }
  }

  // Warmup runs below settle the OS file cache + Spotlight indexing of cache.bin
  // so the first measured warm run is comparable to the cold loop warmups.
  clearFallowCache(dir);
  const fArgsWarm = ["dead-code", "--quiet", "--format", "json"];
  const populate = timeRun(fallowBin, fArgsWarm, dir);
  const populateSummary = summarizeBenchmarkRun(populate, fallowIssueCount);
  if (!populateSummary.valid)
    throw new Error(`[${name}] fallow cache warm-up failed: ${populateSummary.error}`);
  // Warmup runs (same shape as cold path) to settle OS / Spotlight noise
  for (let i = 0; i < WARMUP; i++) {
    timeRun(fallowBin, fArgsWarm, dir);
  }
  // Benchmark warm runs
  const fTimesWarm = [];
  let fWarmIssues = "?";
  for (let i = 0; i < RUNS; i++) {
    const fr = timeRun(fallowBin, fArgsWarm, dir);
    const fSummary = summarizeBenchmarkRun(fr, fallowIssueCount);
    if (!fSummary.valid) throw new Error(`[${name}] fallow warm run failed: ${fSummary.error}`);
    fTimesWarm.push(fr.elapsed);
    if (i === 0) fWarmIssues = fSummary.issues;
  }
  clearFallowCache(dir);

  const fsCold = stats(fTimesCold),
    fsWarm = stats(fTimesWarm);
  const kRuns = sampleSummary(kTimes, RUNS);
  const ks = kRuns.stats;
  const knipRelative = (fallowMedian) => {
    if (kRuns.complete) return relativeSpeed(fallowMedian, ks.median, "knip");
    return ks ? "incomplete" : "--";
  };
  const relative = knipRelative(fsCold.median);
  const speedupCold = kRuns.complete ? ks.median / fsCold.median : null;
  const speedupWarm = kRuns.complete ? ks.median / fsWarm.median : null;
  const cacheSpeedup = fsCold.median / fsWarm.median;

  const rows = [
    {
      Tool: "fallow (cold)",
      ...timeCells(fsCold),
      Relative: relative,
      Memory: fmtMem(fPeakRss),
      Issues: fIssues,
    },
    {
      Tool: "fallow (warm)",
      ...timeCells(fsWarm),
      Relative: knipRelative(fsWarm.median),
      Memory: "-",
      Issues: fWarmIssues,
    },
  ];
  rows.push({
    Tool: "knip",
    ...timeCells(ks),
    Relative: relative,
    Memory: ks ? fmtMem(kPeakRss) : "--",
    Issues: kIssues,
  });
  console.table(rows);
  console.log(`  Cache speedup: ${cacheSpeedup.toFixed(2)}x (warm vs cold)`);
  console.log(`  fallow cold: [${fTimesCold.map((t) => t.toFixed(0)).join(", ")}]`);
  console.log(`  fallow warm: [${fTimesWarm.map((t) => t.toFixed(0)).join(", ")}]`);
  console.log(
    `  knip:        ${kTimes.length > 0 ? `[${kTimes.map((t) => t.toFixed(0)).join(", ")}]` : `[error: ${kErrorReason ?? kIssues}]`}`,
  );
  const kFailure = kErrorReason ? ` (first failure: ${kErrorReason})` : "";
  console.log(`  knip runs: ${kRuns.successful}/${kRuns.requested} ok${kFailure}`);
  console.log("");

  return {
    name,
    files,
    fallowCold: fsCold,
    fallowWarm: fsWarm,
    knip: ks,
    relative,
    speedupCold,
    speedupWarm,
    cacheSpeedup,
    fIssues,
    kIssues,
    fPeakRss,
    kPeakRss,
    kError: !ks,
    kErrorReason,
    kRuns,
  };
}

const results = [];
if (runSynthetic) {
  const d = join(__dirname, "fixtures", "synthetic");
  if (!existsSync(d)) {
    console.log("No synthetic fixtures. Run: npm run generate\n");
  } else {
    console.log("--- Synthetic Projects ---\n");
    const order = ["tiny", "small", "medium", "large", "xlarge"];
    for (const p of readdirSync(d)
      .filter((x) => existsSync(join(d, x, "package.json")))
      .toSorted((a, b) => order.indexOf(a) - order.indexOf(b))) {
      if (projectFilter && !projectFilter.has(p)) continue;
      results.push(benchmarkProject(p, join(d, p)));
    }
  }
}
if (runRealWorld) {
  const d = join(__dirname, "fixtures", "real-world");
  if (!existsSync(d)) {
    console.log("No real-world fixtures. Run: npm run download-fixtures\n");
  } else {
    console.log("--- Real-World Projects ---\n");
    for (const p of readdirSync(d)
      .filter((x) => existsSync(join(d, x, "package.json")))
      .toSorted()) {
      if (projectFilter && !projectFilter.has(p)) continue;
      results.push(benchmarkProject(p, join(d, p)));
    }
  }
}
if (results.length === 0) {
  console.error("No project was benchmarked. Generate or download the fixtures first.");
  process.exit(1);
}
if (results.length > 0) {
  console.log("\n=== Summary ===\n");
  console.table(
    results.map((r) => ({
      Project: r.name,
      Files: r.files,
      "Fallow cold (median)": fmt(r.fallowCold.median),
      "Fallow warm (median)": fmt(r.fallowWarm.median),
      "knip (median)": r.knip ? fmt(r.knip.median) : "error",
      "Faster tool": r.relative,
      "Cache effect": `${r.cacheSpeedup.toFixed(2)}x`,
      "Fallow RSS": fmtMem(r.fPeakRss),
      "knip RSS": r.kError ? "--" : fmtMem(r.kPeakRss),
    })),
  );
  const valid = results.filter((r) => r.speedupCold != null);
  if (valid.length > 0) {
    console.log(
      `Average speedup vs knip (cold): ${(valid.reduce((s, r) => s + r.speedupCold, 0) / valid.length).toFixed(1)}x (${valid.length}/${results.length} projects)`,
    );
    console.log(
      `Average speedup vs knip (warm): ${(valid.reduce((s, r) => s + r.speedupWarm, 0) / valid.length).toFixed(1)}x`,
    );
  }
  const incompleteProjects = results.filter((r) => !r.kRuns.complete);
  if (incompleteProjects.length > 0) {
    console.log(`\nIncomplete knip runs:`);
    for (const project of incompleteProjects) {
      const { successful, requested } = project.kRuns;
      console.log(`  ${project.name}: ${successful}/${requested} ok: ${project.kErrorReason}`);
    }
  }
  console.log(
    `Average cache effect:              ${(results.reduce((s, r) => s + r.cacheSpeedup, 0) / results.length).toFixed(2)}x\n`,
  );
}
