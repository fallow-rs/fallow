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

function parseFallowCycles(stdout) {
  try {
    const data = JSON.parse(stdout);
    return data.circular_dependencies?.length ?? 0;
  } catch {
    return "?";
  }
}

function parseMadgeCycles(stdout) {
  try {
    const data = JSON.parse(stdout);
    return Array.isArray(data) ? data.length : "?";
  } catch {
    return "?";
  }
}

function parseDpdmCycles(stdout) {
  try {
    const data = JSON.parse(stdout);
    return data.circulars?.length ?? "?";
  } catch {
    return "?";
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
  const fallowTimes = [],
    madgeTimes = [],
    dpdmTimes = [];
  let fallowCycles = "?",
    madgeCycles = "?",
    dpdmCycles = "?";
  let fallowRss = 0,
    madgeRss = 0,
    dpdmRss = 0;

  for (let i = 0; i < RUNS; i++) {
    // fallow
    const fr = timeRunWithMemory(fallowBin, fallowArgs, dir);
    fallowTimes.push(fr.elapsed);
    if (i === 0) {
      fallowCycles = parseFallowCycles(fr.stdout);
      fallowRss = fr.peakRssBytes;
    }

    // madge
    if (hasMadge) {
      const mr = timeRunWithMemory(madgeBin, madgeArgs, dir);
      madgeTimes.push(mr.elapsed);
      if (i === 0) {
        madgeCycles = parseMadgeCycles(mr.stdout);
        madgeRss = mr.peakRssBytes;
      }
    }

    // dpdm
    if (hasDpdm) {
      const dr = timeRunWithMemory(dpdmBin, dpdmArgs, dir);
      dpdmTimes.push(dr.elapsed);
      if (i === 0) {
        try {
          dpdmCycles = parseDpdmCycles(readFileSync(dpdmOutputFile, "utf8"));
        } catch {
          dpdmCycles = "?";
        }
        dpdmRss = dr.peakRssBytes;
      }
      if (existsSync(dpdmOutputFile)) rmSync(dpdmOutputFile);
    }
  }

  const fs = stats(fallowTimes);
  const rows = [
    {
      Tool: "fallow",
      Min: fmt(fs.min),
      Mean: fmt(fs.mean),
      Median: fmt(fs.median),
      Max: fmt(fs.max),
      Speedup: "--",
      Memory: fmtMem(fallowRss),
      Cycles: fallowCycles,
    },
  ];

  const result = { name, files, fallow: fs, fallowCycles, fallowRss };

  if (hasMadge && madgeTimes.length > 0) {
    const ms = stats(madgeTimes);
    const speedup = ms.median / fs.median;
    rows.push({
      Tool: "madge",
      Min: fmt(ms.min),
      Mean: fmt(ms.mean),
      Median: fmt(ms.median),
      Max: fmt(ms.max),
      Speedup: `1/${speedup.toFixed(1)}x`,
      Memory: fmtMem(madgeRss),
      Cycles: madgeCycles,
    });
    result.madge = ms;
    result.madgeSpeedup = speedup;
    result.madgeCycles = madgeCycles;
    result.madgeRss = madgeRss;
  }

  if (hasDpdm && dpdmTimes.length > 0) {
    const ds = stats(dpdmTimes);
    const speedup = ds.median / fs.median;
    rows.push({
      Tool: "dpdm",
      Min: fmt(ds.min),
      Mean: fmt(ds.mean),
      Median: fmt(ds.median),
      Max: fmt(ds.max),
      Speedup: `1/${speedup.toFixed(1)}x`,
      Memory: fmtMem(dpdmRss),
      Cycles: dpdmCycles,
    });
    result.dpdm = ds;
    result.dpdmSpeedup = speedup;
    result.dpdmCycles = dpdmCycles;
    result.dpdmRss = dpdmRss;
  }

  console.table(rows);
  console.log(`  fallow: [${fallowTimes.map((t) => t.toFixed(0)).join(", ")}]`);
  if (hasMadge && madgeTimes.length > 0)
    console.log(`  madge:  [${madgeTimes.map((t) => t.toFixed(0)).join(", ")}]`);
  if (hasDpdm && dpdmTimes.length > 0)
    console.log(`  dpdm:   [${dpdmTimes.map((t) => t.toFixed(0)).join(", ")}]`);
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
      .toSorted())
      results.push(benchmarkProject(p, join(d, p)));
  }
}

if (results.length > 0) {
  console.log("\n=== Summary ===\n");
  const summaryRows = results.map((r) => {
    const row = {
      Project: r.name,
      Files: r.files,
      "Fallow (median)": fmt(r.fallow.median),
      "Fallow cycles": r.fallowCycles,
      "Fallow RSS": fmtMem(r.fallowRss),
    };
    if (r.madge) {
      row["madge (median)"] = fmt(r.madge.median);
      row["vs madge"] = `${r.madgeSpeedup.toFixed(1)}x`;
    }
    if (r.dpdm) {
      row["dpdm (median)"] = fmt(r.dpdm.median);
      row["vs dpdm"] = `${r.dpdmSpeedup.toFixed(1)}x`;
    }
    return row;
  });
  console.table(summaryRows);

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
