#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync } from "node:fs";
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
} = readBenchArgs(args, {
  runs: "5",
  warmup: "2",
});

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

// Detect available tools
const madgeBin = join(__dirname, "node_modules", ".bin", "madge");
const dpdmBin = join(__dirname, "node_modules", ".bin", "dpdm");
const hasMadge = existsSync(madgeBin);
const hasDpdm = existsSync(dpdmBin);

if (!hasMadge && !hasDpdm) {
  console.error("Neither madge nor dpdm found. Run: cd benchmarks && npm install");
  process.exit(1);
}

const fallowVersion = spawnSync(fallowBin, ["--version"], { stdio: "pipe" })
  .stdout?.toString()
  .trim();
const madgeVersion = hasMadge
  ? spawnSync(madgeBin, ["--version"], { stdio: "pipe" }).stdout?.toString().trim()
  : "n/a";
const dpdmVersion = hasDpdm
  ? spawnSync(dpdmBin, ["--version"], { stdio: "pipe" }).stdout?.toString().trim()
  : "n/a";
const rustVersion = spawnSync("rustc", ["--version"], { stdio: "pipe" }).stdout?.toString().trim();

console.log(`\n=== Fallow vs madge/dpdm: Circular Dependency Detection ===\n`);
printEnvironment();
console.log(`Tools:`);
console.log(`  fallow dead-code --circular-deps  ${fallowVersion}`);
if (hasMadge) console.log(`  madge --circular              ${madgeVersion}`);
if (hasDpdm) console.log(`  dpdm                          ${dpdmVersion}`);
console.log(`Config: ${RUNS} runs, ${WARMUP} warmup\n`);

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

const stdoutDir = mkdtempSync(join(os.tmpdir(), "fallow-bench-circular-"));
process.on("exit", () => rmSync(stdoutDir, { recursive: true, force: true }));

const timeRunWithMemory = (cmd, cmdArgs, cwd) =>
  timeSample(cmd, cmdArgs, {
    cwd,
    timeout: 600000,
    memory: true,
    stdoutFile: join(stdoutDir, "stdout"),
  });

const UNEXPECTED_SHAPE = { ok: false, error: "unexpected JSON shape" };

const objectReport = (stdout) => {
  const report = parseJsonReport(stdout);
  return report.ok && Array.isArray(report.value) ? UNEXPECTED_SHAPE : report;
};

const arrayReport = (stdout) => {
  const report = parseJsonReport(stdout);
  return report.ok && !Array.isArray(report.value) ? UNEXPECTED_SHAPE : report;
};

const dpdmReport = (outputFile) => {
  let text;
  try {
    text = readFileSync(outputFile, "utf8");
  } catch {
    return { ok: false, error: "no report file" };
  }
  const report = parseJsonReport(text);
  return report.ok && !Array.isArray(report.value.circulars) ? UNEXPECTED_SHAPE : report;
};

const toolSamples = (tool) => ({ tool, times: [], firstFailure: null, cycles: "?", rss: 0 });

const recordSample = (samples, run, outcome, countCycles) => {
  samples.firstFailure ??= outcome.reason;
  if (!outcome.valid) return;
  samples.times.push(run.elapsed);
  if (samples.times.length > 1) return;
  samples.cycles = countCycles();
  samples.rss = run.peakRssBytes;
};

const addComparison = (result, rows, samples) => {
  const { tool } = samples;
  const summary = sampleSummary(samples.times, RUNS);
  const speedup = summary.complete ? summary.stats.median / result.fallow.median : null;
  rows.push({
    Tool: tool,
    ...timeCells(summary.stats),
    Speedup: speedup === null ? "incomplete" : `1/${speedup.toFixed(1)}x`,
    Memory: fmtMem(samples.rss),
    Cycles: samples.cycles,
  });
  result[tool] = summary.stats;
  result[`${tool}Cycles`] = samples.cycles;
  result[`${tool}Rss`] = samples.rss;
  result[`${tool}Speedup`] = speedup;
  if (speedup === null) result.incomplete.push({ tool, summary, reason: samples.firstFailure });
};

const printSamples = (samples) => {
  const label = `${samples.tool}:`.padEnd(7);
  if (samples.times.length > 0) {
    console.log(`  ${label} [${samples.times.map((t) => t.toFixed(0)).join(", ")}]`);
  }
  const failure = samples.firstFailure ? ` (first failure: ${samples.firstFailure})` : "";
  console.log(`  ${samples.tool} runs: ${samples.times.length}/${RUNS} ok${failure}`);
};

const summaryCells = (result, tool) => {
  if (result[tool] === undefined) return {};
  const speedup = result[`${tool}Speedup`];
  return {
    [`${tool} (median)`]: result[tool] ? fmt(result[tool].median) : "--",
    [`vs ${tool}`]: speedup === null ? "incomplete" : `${speedup.toFixed(1)}x`,
  };
};

function fmtMem(bytes) {
  if (bytes === 0) return "?";
  const mb = bytes / 1024 / 1024;
  return mb < 1024 ? `${mb.toFixed(1)} MB` : `${(mb / 1024).toFixed(2)} GB`;
}

function benchmarkProject(name, dir) {
  const files = countSourceFiles(dir);
  const hasTsConfig = existsSync(join(dir, "tsconfig.json"));
  console.log(`### ${name} (${files} source files)\n`);

  // fallow: JSON output, only circular deps, no cache
  const fallowArgs = ["dead-code", "--format", "json", "--quiet", "--no-cache", "--circular-deps"];

  // madge: circular detection with JSON output
  const madgeArgs = [
    "--circular",
    "--json",
    "--extensions",
    "ts,tsx,js,jsx,mjs,cjs",
    "--no-spinner",
  ];
  if (hasTsConfig) madgeArgs.push("--ts-config", "tsconfig.json");
  madgeArgs.push("src/");

  // dpdm: circular detection with JSON output to stdout
  const dpdmOutputFile = join(dir, ".dpdm-output.json");
  const dpdmArgs = ["--no-tree", "--no-warning", "--no-progress", "--output", dpdmOutputFile];
  if (hasTsConfig) dpdmArgs.push("--tsconfig", "tsconfig.json");
  dpdmArgs.push("src/index.ts");

  // Warmup
  for (let i = 0; i < WARMUP; i++) {
    timeRunWithMemory(fallowBin, fallowArgs, dir);
    if (hasMadge) timeRunWithMemory(madgeBin, madgeArgs, dir);
    if (hasDpdm) {
      timeRunWithMemory(dpdmBin, dpdmArgs, dir);
      if (existsSync(dpdmOutputFile)) rmSync(dpdmOutputFile);
    }
  }

  // --- Measured runs ---
  const fallowTimes = [];
  let fallowCycles = "?",
    fallowRss = 0;
  const madge = toolSamples("madge");
  const dpdm = toolSamples("dpdm");

  for (let i = 0; i < RUNS; i++) {
    const fr = timeRunWithMemory(fallowBin, fallowArgs, dir);
    const fallowReport = objectReport(fr.stdout);
    const fallowOutcome = sampleOutcome(fr, fallowReport);
    if (!fallowOutcome.valid)
      throw new Error(`[${name}] fallow run failed: ${fallowOutcome.reason}`);
    fallowTimes.push(fr.elapsed);
    if (i === 0) {
      fallowCycles = fallowReport.value.circular_dependencies?.length ?? 0;
      fallowRss = fr.peakRssBytes;
    }

    if (hasMadge) {
      const mr = timeRunWithMemory(madgeBin, madgeArgs, dir);
      const report = arrayReport(mr.stdout);
      recordSample(madge, mr, sampleOutcome(mr, report), () => report.value.length);
    }

    if (hasDpdm) {
      const dr = timeRunWithMemory(dpdmBin, dpdmArgs, dir);
      const report = dpdmReport(dpdmOutputFile);
      if (existsSync(dpdmOutputFile)) rmSync(dpdmOutputFile);
      recordSample(dpdm, dr, sampleOutcome(dr, report), () => report.value.circulars.length);
    }
  }

  const fs = stats(fallowTimes);
  const rows = [
    {
      Tool: "fallow",
      ...timeCells(fs),
      Speedup: "--",
      Memory: fmtMem(fallowRss),
      Cycles: fallowCycles,
    },
  ];
  const result = { name, files, fallow: fs, fallowCycles, fallowRss, incomplete: [] };
  const compared = [hasMadge && madge, hasDpdm && dpdm].filter(Boolean);
  for (const samples of compared) addComparison(result, rows, samples);

  console.table(rows);
  console.log(`  fallow: [${fallowTimes.map((t) => t.toFixed(0)).join(", ")}]`);
  for (const samples of compared) printSamples(samples);
  console.log("");

  return result;
}

const results = [];

if (runSynthetic) {
  const d = join(__dirname, "fixtures", "synthetic-circular");
  if (!existsSync(d)) {
    console.log("No synthetic circular fixtures. Run: npm run generate:circular\n");
  } else {
    console.log("--- Synthetic Projects (Circular Dependencies) ---\n");
    const order = ["tiny", "small", "medium", "large", "xlarge"];
    for (const p of readdirSync(d)
      .filter((x) => existsSync(join(d, x, "package.json")))
      .filter((x) => !projectFilter || projectFilter.has(x))
      .toSorted((a, b) => order.indexOf(a) - order.indexOf(b)))
      results.push(benchmarkProject(p, join(d, p)));
  }
}

if (runRealWorld) {
  const d = join(__dirname, "fixtures", "real-world");
  if (!existsSync(d)) {
    console.log("No real-world fixtures. Run: npm run download-fixtures\n");
  } else {
    console.log("--- Real-World Projects (Circular Dependencies) ---\n");
    for (const p of readdirSync(d)
      .filter((x) => existsSync(join(d, x, "package.json")))
      .filter((x) => !projectFilter || projectFilter.has(x))
      .toSorted())
      results.push(benchmarkProject(p, join(d, p)));
  }
}

if (results.length === 0) {
  console.error("No project was benchmarked. Generate or download the fixtures first.");
  process.exit(1);
}
if (results.length > 0) {
  console.log("\n=== Summary ===\n");
  const summaryRows = results.map((r) => ({
    Project: r.name,
    Files: r.files,
    "Fallow (median)": fmt(r.fallow.median),
    "Fallow cycles": r.fallowCycles,
    "Fallow RSS": fmtMem(r.fallowRss),
    ...summaryCells(r, "madge"),
    ...summaryCells(r, "dpdm"),
  }));
  console.table(summaryRows);
  for (const r of results) {
    for (const { tool, summary, reason } of r.incomplete) {
      const detail = reason ? `: ${reason}` : "";
      console.log(
        `Incomplete: ${r.name} ${tool} ${summary.successful}/${summary.requested} runs ok${detail}`,
      );
    }
  }

  if (results.some((r) => r.madgeSpeedup)) {
    const avgMadge =
      results.filter((r) => r.madgeSpeedup).reduce((s, r) => s + r.madgeSpeedup, 0) /
      results.filter((r) => r.madgeSpeedup).length;
    console.log(`Average speedup vs madge: ${avgMadge.toFixed(1)}x faster`);
  }
  if (results.some((r) => r.dpdmSpeedup)) {
    const avgDpdm =
      results.filter((r) => r.dpdmSpeedup).reduce((s, r) => s + r.dpdmSpeedup, 0) /
      results.filter((r) => r.dpdmSpeedup).length;
    console.log(`Average speedup vs dpdm: ${avgDpdm.toFixed(1)}x faster`);
  }
  console.log("");
}
