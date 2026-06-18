#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import { existsSync, readdirSync, statSync, readFileSync, rmSync } from "node:fs";
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
const jscpdBin = join(__dirname, "node_modules", ".bin", "jscpd");
if (!existsSync(jscpdBin)) {
  console.error("jscpd not found. Run: cd benchmarks && npm install");
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

function timeRun(cmd, cmdArgs, cwd) {
  const start = performance.now();
  const result = spawnSync(cmd, cmdArgs, {
    cwd,
    stdio: "pipe",
    timeout: 600000,
    maxBuffer: 50 * 1024 * 1024,
    env: { ...process.env, NO_COLOR: "1", FORCE_COLOR: "0" },
  });
  return {
    elapsed: performance.now() - start,
    status: result.status,
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
    timeout: 600000,
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
    const match = stderr.match(/(\d+)\s+maximum resident set size/);
    if (match) peakRssBytes = parseInt(match[1]);
  }

  return {
    elapsed,
    status: result.status,
    stdout: result.stdout?.toString() ?? "",
    stderr,
    peakRssBytes,
  };
}

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

function relativeSpeed(fallowMedian, jscpdMedian) {
  if (fallowMedian <= jscpdMedian) {
    return `fallow ${formatMultiplier(jscpdMedian / fallowMedian)}`;
  }

  return `jscpd ${formatMultiplier(fallowMedian / jscpdMedian)}`;
}

function formatMultiplier(value) {
  return `${value.toFixed(1)}x`;
}

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
    "typescript,javascript",
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
    if (existsSync(jscpdReportDir)) rmSync(jscpdReportDir, { recursive: true });
    timeRun(jscpdBin, jArgs, dir);
    if (existsSync(jscpdReportDir)) rmSync(jscpdReportDir, { recursive: true });
  }

  // --- Cold runs ---
  const fTimesCold = [],
    jTimes = [];
  let fClones = { groups: "?", instances: "?", pct: "?" };
  let jClones = { groups: "?", instances: "?", pct: "?" };
  let fPeakRss = 0,
    jPeakRss = 0;

  for (let i = 0; i < RUNS; i++) {
    const fr = timeRunWithMemory(fallowBin, fArgsCold, dir);
    fTimesCold.push(fr.elapsed);
    if (i === 0) {
      fClones = parseFallowCloneCount(fr.stdout);
      fPeakRss = fr.peakRssBytes;
    }

    if (existsSync(jscpdReportDir)) rmSync(jscpdReportDir, { recursive: true });
    const jr = timeRunWithMemory(jscpdBin, jArgs, dir);
    jTimes.push(jr.elapsed);
    if (i === 0) {
      jClones = parseJscpdCloneCount(jscpdReportDir);
      jPeakRss = jr.peakRssBytes;
    }
    if (existsSync(jscpdReportDir)) rmSync(jscpdReportDir, { recursive: true });
  }

  const fsCold = stats(fTimesCold),
    js = stats(jTimes);
  const relative = relativeSpeed(fsCold.median, js.median);

  console.table([
    {
      Tool: "fallow dupes",
      Min: fmt(fsCold.min),
      Mean: fmt(fsCold.mean),
      Median: fmt(fsCold.median),
      Max: fmt(fsCold.max),
      Relative: relative,
      Memory: fmtMem(fPeakRss),
      "Clone Groups": fClones.groups,
      "Dup %": `${fClones.pct}%`,
    },
    {
      Tool: "jscpd",
      Min: fmt(js.min),
      Mean: fmt(js.mean),
      Median: fmt(js.median),
      Max: fmt(js.max),
      Relative: relative,
      Memory: fmtMem(jPeakRss),
      "Clone Groups": jClones.groups,
      "Dup %": `${jClones.pct}%`,
    },
  ]);
  console.log(`  fallow: [${fTimesCold.map((t) => t.toFixed(0)).join(", ")}]`);
  console.log(`  jscpd:  [${jTimes.map((t) => t.toFixed(0)).join(", ")}]\n`);

  return { name, files, fallow: fsCold, jscpd: js, relative, fClones, jClones, fPeakRss, jPeakRss };
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
      "jscpd (median)": fmt(r.jscpd.median),
      "Faster tool": r.relative,
      "Fallow RSS": fmtMem(r.fPeakRss),
      "jscpd RSS": fmtMem(r.jPeakRss),
      "Fallow clones": r.fClones.groups,
      "jscpd clones": r.jClones.groups,
    })),
  );
}
