#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import { existsSync, readdirSync, statSync, rmSync } from "node:fs";
import { join, resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import os from "node:os";

const __dirname = dirname(fileURLToPath(import.meta.url));
const rootDir = resolve(__dirname, "..");
const args = process.argv.slice(2);
const hasFilter = args.includes("--synthetic") || args.includes("--real-world");
const runSynthetic = args.includes("--synthetic") || !hasFilter;
const runRealWorld = args.includes("--real-world") || !hasFilter;
const RUNS = parseInt(args.find((a) => a.startsWith("--runs="))?.split("=")[1] ?? "5");
const WARMUP = parseInt(args.find((a) => a.startsWith("--warmup="))?.split("=")[1] ?? "2");
const projectsArg = args.find((a) => a.startsWith("--projects="))?.split("=")[1];
const projectFilter = projectsArg
  ? new Set(
      projectsArg
        .split(",")
        .map((s) => s.trim())
        .filter(Boolean),
    )
  : null;

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

function timeRun(cmd, cmdArgs, cwd) {
  const start = performance.now();
  const result = spawnSync(cmd, cmdArgs, {
    cwd,
    stdio: "pipe",
    timeout: 300000,
    maxBuffer: 50 * 1024 * 1024,
    env: { ...process.env, NO_COLOR: "1", FORCE_COLOR: "0" },
  });
  return {
    elapsed: performance.now() - start,
    status: result.status,
    signal: result.signal,
    stdout: result.stdout?.toString() ?? "",
    stderr: result.stderr?.toString() ?? "",
  };
}

function timeRunWithMemory(cmd, cmdArgs, cwd) {
  const isLinux = process.platform === "linux";
  const timeBin = "/usr/bin/time";
  const timeArgs = isLinux ? ["-v", cmd, ...cmdArgs] : ["-l", cmd, ...cmdArgs];

  const start = performance.now();
  const result = spawnSync(timeBin, timeArgs, {
    cwd,
    stdio: "pipe",
    timeout: 300000,
    maxBuffer: 50 * 1024 * 1024,
    env: { ...process.env, NO_COLOR: "1", FORCE_COLOR: "0" },
  });
  const elapsed = performance.now() - start;
  const stderr = result.stderr?.toString() ?? "";

  let peakRssBytes = 0;
  if (isLinux) {
    const match = stderr.match(/Maximum resident set size \(kbytes\): (\d+)/);
    if (match) peakRssBytes = parseInt(match[1]) * 1024;
  } else {
    // macOS: reports in bytes
    const match = stderr.match(/(\d+)\s+maximum resident set size/);
    if (match) peakRssBytes = parseInt(match[1]);
  }

  // stdout for fallow comes from the time wrapper child process.
  const stdout = result.stdout?.toString() ?? "";

  return { elapsed, status: result.status, signal: result.signal, stdout, stderr, peakRssBytes };
}

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

function parseJsonReport(stdout) {
  const trimmed = stdout.replace(/^\uFEFF/, "").trim();
  if (!trimmed) return { ok: false, reason: "no JSON output" };
  try {
    const data = JSON.parse(trimmed);
    if (data === null || (typeof data !== "object" && !Array.isArray(data))) {
      return { ok: false, reason: "unexpected JSON shape" };
    }
    return { ok: true, data };
  } catch (error) {
    return { ok: false, reason: `invalid JSON output (${String(error.message).split("\n")[0]})` };
  }
}

function countIssues(data) {
  if (Array.isArray(data)) return data.length;
  let count = 0;
  for (const value of Object.values(data)) {
    if (Array.isArray(value)) count += value.length;
  }
  return count;
}

function summarizeBenchmarkRun(result) {
  const parsed = parseJsonReport(result.stdout);
  if (!parsed.ok) {
    const detail = firstDiagnosticLine(result.stderr) ?? firstDiagnosticLine(result.stdout);
    return {
      valid: false,
      issues: "error",
      error: detail ? `${parsed.reason}; ${detail}` : parsed.reason,
    };
  }
  if (result.status !== 0 && result.status !== 1) {
    const detail = result.signal
      ? `terminated by ${result.signal}`
      : `exit ${result.status ?? "unknown"}`;
    return { valid: false, issues: "error", error: detail };
  }
  return { valid: true, issues: countIssues(parsed.data), error: null };
}

function stats(times) {
  const sorted = [...times].toSorted((a, b) => a - b);
  const mid = Math.floor(sorted.length / 2);
  const median = sorted.length % 2 === 0 ? (sorted[mid - 1] + sorted[mid]) / 2 : sorted[mid];
  return {
    min: sorted[0],
    max: sorted.at(-1),
    mean: sorted.reduce((a, b) => a + b, 0) / sorted.length,
    median,
  };
}

function fmt(ms) {
  return ms < 1000 ? `${ms.toFixed(0)}ms` : `${(ms / 1000).toFixed(2)}s`;
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
    const fSummary = summarizeBenchmarkRun(fr);
    if (!fSummary.valid) throw new Error(`[${name}] fallow cold run failed: ${fSummary.error}`);
    fTimesCold.push(fr.elapsed);
    if (i === 0) {
      fIssues = fSummary.issues;
      fPeakRss = fr.peakRssBytes;
    }
    const kr = timeRunWithMemory(knipBin, kArgs, dir);
    const kSummary = summarizeBenchmarkRun(kr);
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
  // Populate cache
  const populate = timeRun(fallowBin, fArgsWarm, dir);
  const populateSummary = summarizeBenchmarkRun(populate);
  if (!populateSummary.valid)
    throw new Error(`[${name}] fallow cache warm-up failed: ${populateSummary.error}`);
  // Warmup runs (same shape as cold path) to settle OS / Spotlight noise
  for (let i = 0; i < WARMUP; i++) {
    timeRun(fallowBin, fArgsWarm, dir);
  }
  // Benchmark warm runs
  const fTimesWarm = [];
  for (let i = 0; i < RUNS; i++) {
    const fr = timeRun(fallowBin, fArgsWarm, dir);
    const fSummary = summarizeBenchmarkRun(fr);
    if (!fSummary.valid) throw new Error(`[${name}] fallow warm run failed: ${fSummary.error}`);
    fTimesWarm.push(fr.elapsed);
  }
  clearFallowCache(dir);

  const fsCold = stats(fTimesCold),
    fsWarm = stats(fTimesWarm);
  const ks = kTimes.length > 0 ? stats(kTimes) : null;
  const relative = ks ? relativeSpeed(fsCold.median, ks.median, "knip") : "--";
  const speedupCold = ks ? ks.median / fsCold.median : null;
  const speedupWarm = ks ? ks.median / fsWarm.median : null;
  const cacheSpeedup = fsCold.median / fsWarm.median;

  const rows = [
    {
      Tool: "fallow (cold)",
      Min: fmt(fsCold.min),
      Mean: fmt(fsCold.mean),
      Median: fmt(fsCold.median),
      Max: fmt(fsCold.max),
      Relative: relative,
      Memory: fmtMem(fPeakRss),
      Issues: fIssues,
    },
    {
      Tool: "fallow (warm)",
      Min: fmt(fsWarm.min),
      Mean: fmt(fsWarm.mean),
      Median: fmt(fsWarm.median),
      Max: fmt(fsWarm.max),
      Relative: ks ? relativeSpeed(fsWarm.median, ks.median, "knip") : "--",
      Memory: "-",
      Issues: fIssues,
    },
  ];
  if (ks) {
    rows.push({
      Tool: "knip",
      Min: fmt(ks.min),
      Mean: fmt(ks.mean),
      Median: fmt(ks.median),
      Max: fmt(ks.max),
      Relative: relative,
      Memory: fmtMem(kPeakRss),
      Issues: kIssues,
    });
  } else {
    rows.push({
      Tool: "knip",
      Min: "--",
      Mean: "--",
      Median: "--",
      Max: "--",
      Relative: "--",
      Memory: "--",
      Issues: kIssues,
    });
  }
  console.table(rows);
  console.log(`  Cache speedup: ${cacheSpeedup.toFixed(2)}x (warm vs cold)`);
  console.log(`  fallow cold: [${fTimesCold.map((t) => t.toFixed(0)).join(", ")}]`);
  console.log(`  fallow warm: [${fTimesWarm.map((t) => t.toFixed(0)).join(", ")}]`);
  console.log(
    `  knip:        ${kTimes.length > 0 ? `[${kTimes.map((t) => t.toFixed(0)).join(", ")}]` : `[error: ${kErrorReason ?? kIssues}]`}`,
  );
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
  const errorProjects = results.filter((r) => r.kError);
  if (errorProjects.length > 0) {
    console.log(`\nknip errors:`);
    for (const project of errorProjects) console.log(`  ${project.name}: ${project.kErrorReason}`);
  }
  console.log(
    `Average cache effect:              ${(results.reduce((s, r) => s + r.cacheSpeedup, 0) / results.length).toFixed(2)}x\n`,
  );
}
