#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import { existsSync, readdirSync, statSync, readFileSync, rmSync } from "node:fs";
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
const JS_WEB_FORMATS = "typescript,javascript,tsx,jsx";

console.log("Building fallow (release)...");
const buildResult = spawnSync("cargo", ["build", "--release"], {
  cwd: rootDir,
  stdio: "pipe",
  timeout: 300000,
});
if (buildResult.status !== 0) {
  console.error("Build failed:", buildResult.stderr?.toString());
  process.exit(1);
}
const fallowBin = join(rootDir, "target", "release", "fallow");
const jscpdBin = join(__dirname, "node_modules", ".bin", "jscpd");
if (!existsSync(jscpdBin)) {
  console.error("jscpd not found. Run: cd benchmarks && npm install");
  process.exit(1);
}

const packageLock = JSON.parse(readFileSync(join(__dirname, "package-lock.json"), "utf8"));
const actualJscpdPackage = JSON.parse(
  readFileSync(join(__dirname, "node_modules", "jscpd", "package.json"), "utf8"),
);
const expectedJscpdVersion = packageLock.packages?.["node_modules/jscpd"]?.version;
if (expectedJscpdVersion && actualJscpdPackage.version !== expectedJscpdVersion) {
  console.error(
    `jscpd version mismatch: node_modules has ${actualJscpdPackage.version}, package-lock expects ${expectedJscpdVersion}. Run: npm ci --prefix benchmarks`,
  );
  process.exit(1);
}

const fallowVersion = spawnSync(fallowBin, ["--version"], { stdio: "pipe" })
  .stdout?.toString()
  .trim();
const jscpdVersion = spawnSync(jscpdBin, ["--version"], { stdio: "pipe" })
  .stdout?.toString()
  .trim();
const rustVersion = spawnSync("rustc", ["--version"], { stdio: "pipe" }).stdout?.toString().trim();

console.log(`\n=== Fallow Dupes vs jscpd Benchmark Suite ===\n`);
printEnvironment();
console.log(
  `Tools:\n  fallow dupes  ${fallowVersion}\n  jscpd         ${jscpdVersion}\nConfig: ${RUNS} runs, ${WARMUP} warmup\n`,
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
        if (["node_modules", ".git", "dist", "report"].includes(e)) continue;
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

const timeRun = (cmd, cmdArgs, cwd) => timeSample(cmd, cmdArgs, { cwd, timeout: 600000 });
const timeRunWithMemory = (cmd, cmdArgs, cwd) =>
  timeSample(cmd, cmdArgs, { cwd, timeout: 600000, memory: true });

function parseFallowCloneCount(stdout) {
  try {
    const data = JSON.parse(stdout);
    return {
      groups: data.stats?.clone_groups ?? data.clone_groups?.length ?? "?",
      instances: data.stats?.clone_instances ?? "?",
      pct: data.stats?.duplication_percentage?.toFixed(1) ?? "?",
    };
  } catch {
    return { groups: "?", instances: "?", pct: "?" };
  }
}

function parseJscpdCloneCount(reportDir) {
  try {
    const reportPath = join(reportDir, "jscpd-report.json");
    if (!existsSync(reportPath)) return { groups: "?", instances: "?", pct: "?" };
    const data = JSON.parse(readFileSync(reportPath, "utf8"));
    const jscpdStats = data.statistics?.total;
    return {
      groups: data.duplicates?.length ?? "?",
      instances: jscpdStats?.clones ?? "?",
      pct: jscpdStats?.percentage?.toFixed(1) ?? "?",
    };
  } catch {
    return { groups: "?", instances: "?", pct: "?" };
  }
}

const fallowReport = (stdout) => {
  const report = parseJsonReport(stdout);
  return report.ok && Array.isArray(report.value)
    ? { ok: false, error: "unexpected JSON shape" }
    : report;
};

const jscpdReport = (reportDir) => {
  let text;
  try {
    text = readFileSync(join(reportDir, "jscpd-report.json"), "utf8");
  } catch {
    return { ok: false, error: "no report file" };
  }
  return parseJsonReport(text);
};

function fmtMem(bytes) {
  if (bytes === 0) return "?";
  const mb = bytes / 1024 / 1024;
  return mb < 1024 ? `${mb.toFixed(1)} MB` : `${(mb / 1024).toFixed(2)} GB`;
}

function relativeSpeed(fallowMedian, jscpdMedian) {
  if (fallowMedian <= jscpdMedian) {
    return `fallow ${formatMultiplier(jscpdMedian / fallowMedian)}`;
  }

  return `jscpd ${formatMultiplier(fallowMedian / jscpdMedian)}`;
}

function formatMultiplier(value) {
  return `${value.toFixed(1)}x`;
}

const clearReportDir = (reportDir) => {
  if (existsSync(reportDir)) rmSync(reportDir, { recursive: true });
};

const runJscpdSample = (samples, jArgs, dir, reportDir) => {
  clearReportDir(reportDir);
  const run = timeRunWithMemory(jscpdBin, jArgs, dir);
  const outcome = sampleOutcome(run, jscpdReport(reportDir));
  if (outcome.valid) {
    samples.times.push(run.elapsed);
    if (samples.times.length === 1) {
      samples.clones = parseJscpdCloneCount(reportDir);
      samples.rss = run.peakRssBytes;
    }
  } else {
    samples.firstFailure ??= outcome.reason;
  }
  clearReportDir(reportDir);
};

function benchmarkProject(name, dir) {
  const files = countSourceFiles(dir);
  console.log(`### ${name} (${files} source files)\n`);

  // fallow dupes: JSON output, no cache (cold)
  const fArgsCold = ["dupes", "--format", "json", "--no-cache"];

  // jscpd: JSON reporter, output to temp dir
  const jscpdReportDir = join(dir, "report");
  const jArgs = [
    "--reporters",
    "json",
    "--format",
    JS_WEB_FORMATS,
    "--output",
    jscpdReportDir,
    "--min-tokens",
    "50",
    "--min-lines",
    "5",
    "--ignore",
    "**/node_modules/**,**/dist/**,**/.git/**",
    "--silent",
    ".",
  ];

  // Warmup
  for (let i = 0; i < WARMUP; i++) {
    timeRun(fallowBin, fArgsCold, dir);
    clearReportDir(jscpdReportDir);
    timeRun(jscpdBin, jArgs, dir);
    clearReportDir(jscpdReportDir);
  }

  // --- Cold runs ---
  const fTimesCold = [];
  let fClones = { groups: "?", instances: "?", pct: "?" };
  let fPeakRss = 0;
  const jscpd = {
    times: [],
    firstFailure: null,
    clones: { groups: "?", instances: "?", pct: "?" },
    rss: 0,
  };

  for (let i = 0; i < RUNS; i++) {
    const fr = timeRunWithMemory(fallowBin, fArgsCold, dir);
    const fOutcome = sampleOutcome(fr, fallowReport(fr.stdout));
    if (!fOutcome.valid) throw new Error(`[${name}] fallow run failed: ${fOutcome.reason}`);
    fTimesCold.push(fr.elapsed);
    if (i === 0) {
      fClones = parseFallowCloneCount(fr.stdout);
      fPeakRss = fr.peakRssBytes;
    }

    runJscpdSample(jscpd, jArgs, dir, jscpdReportDir);
  }

  const fsCold = stats(fTimesCold);
  const jSummary = sampleSummary(jscpd.times, RUNS);
  const js = jSummary.stats;
  const relative = jSummary.complete ? relativeSpeed(fsCold.median, js.median) : "incomplete";

  console.table([
    {
      Tool: "fallow dupes",
      ...timeCells(fsCold),
      Relative: relative,
      Memory: fmtMem(fPeakRss),
      "Clone Groups": fClones.groups,
      "Dup %": `${fClones.pct}%`,
    },
    {
      Tool: "jscpd",
      ...timeCells(js),
      Relative: relative,
      Memory: fmtMem(jscpd.rss),
      "Clone Groups": jscpd.clones.groups,
      "Dup %": `${jscpd.clones.pct}%`,
    },
  ]);
  console.log(`  fallow: [${fTimesCold.map((t) => t.toFixed(0)).join(", ")}]`);
  console.log(`  jscpd:  [${jscpd.times.map((t) => t.toFixed(0)).join(", ")}]`);
  const failure = jscpd.firstFailure ? ` (first failure: ${jscpd.firstFailure})` : "";
  console.log(`  jscpd runs: ${jscpd.times.length}/${RUNS} ok${failure}\n`);

  return {
    name,
    files,
    fallow: fsCold,
    jscpd: js,
    jscpdRuns: jSummary,
    jscpdFailure: jscpd.firstFailure,
    relative,
    fClones,
    jClones: jscpd.clones,
    fPeakRss,
    jPeakRss: jscpd.rss,
  };
}

const results = [];

if (runSynthetic) {
  const d = join(__dirname, "fixtures", "synthetic-dupes");
  if (!existsSync(d)) {
    console.log("No synthetic dupes fixtures. Run: npm run generate:dupes\n");
  } else {
    console.log("--- Synthetic Projects (Duplication) ---\n");
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
    console.log("--- Real-World Projects (Duplication) ---\n");
    for (const p of readdirSync(d)
      .filter((x) => existsSync(join(d, x, "package.json")))
      .toSorted()) {
      if (projectFilter && !projectFilter.has(p)) continue;
      results.push(benchmarkProject(p, join(d, p)));
    }
  }
}

if (results.length > 0) {
  console.log("\n=== Summary ===\n");
  console.table(
    results.map((r) => ({
      Project: r.name,
      Files: r.files,
      "Fallow cold (median)": fmt(r.fallow.median),
      "jscpd (median)": timeCells(r.jscpd).Median,
      "Faster tool": r.relative,
      "Fallow RSS": fmtMem(r.fPeakRss),
      "jscpd RSS": fmtMem(r.jPeakRss),
      "Fallow clones": r.fClones.groups,
      "jscpd clones": r.jClones.groups,
    })),
  );
  for (const r of results.filter((result) => !result.jscpdRuns.complete)) {
    const { successful, requested } = r.jscpdRuns;
    console.log(
      `Incomplete: ${r.name} jscpd ${successful}/${requested} runs ok: ${r.jscpdFailure}`,
    );
  }
}
