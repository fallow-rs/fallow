#!/usr/bin/env node
import { existsSync, readFileSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import {
  benchmarkDir,
  buildFallowRelease,
  countSourceFiles,
  fmt,
  fmtMem,
  getVersion,
  parseBenchmarkArgs,
  printEnvironment,
  runProjectBenchmarks,
  stats,
  timeRunWithMemory,
} from './benchmark-helpers.mjs';

const __dirname = benchmarkDir;
const { runSynthetic, runRealWorld, RUNS, WARMUP } = parseBenchmarkArgs();
const RUN_TIMEOUT = 600000;

const fallowBin = buildFallowRelease();

// Detect available tools
const madgeBin = join(__dirname, 'node_modules', '.bin', 'madge');
const dpdmBin = join(__dirname, 'node_modules', '.bin', 'dpdm');
const hasMadge = existsSync(madgeBin);
const hasDpdm = existsSync(dpdmBin);

if (!hasMadge && !hasDpdm) {
  console.error('Neither madge nor dpdm found. Run: cd benchmarks && npm install');
  process.exit(1);
}

const fallowVersion = getVersion(fallowBin);
const madgeVersion = hasMadge ? getVersion(madgeBin) : 'n/a';
const dpdmVersion = hasDpdm ? getVersion(dpdmBin) : 'n/a';
const rustVersion = getVersion('rustc');

console.log(`\n=== Fallow vs madge/dpdm — Circular Dependency Detection ===\n`);
printEnvironment(rustVersion);
console.log(`Tools:`);
console.log(`  fallow dead-code --circular-deps  ${fallowVersion}`);
if (hasMadge) console.log(`  madge --circular              ${madgeVersion}`);
if (hasDpdm) console.log(`  dpdm                          ${dpdmVersion}`);
console.log(`Config: ${RUNS} runs, ${WARMUP} warmup\n`);

function parseFallowCycles(stdout) {
  try {
    const data = JSON.parse(stdout);
    return data.circular_dependencies?.length ?? 0;
  } catch { return '?'; }
}

function parseMadgeCycles(stdout) {
  try {
    const data = JSON.parse(stdout);
    return Array.isArray(data) ? data.length : '?';
  } catch { return '?'; }
}

function parseDpdmCycles(stdout) {
  try {
    const data = JSON.parse(stdout);
    return data.circulars?.length ?? '?';
  } catch { return '?'; }
}

function benchmarkProject(name, dir) {
  const files = countSourceFiles(dir, ['node_modules', '.git', 'dist', 'report']);
  const hasTsConfig = existsSync(join(dir, 'tsconfig.json'));
  console.log(`### ${name} (${files} source files)\n`);

  // fallow: JSON output, only circular deps, no cache
  const fallowArgs = ['dead-code', '--format', 'json', '--quiet', '--no-cache', '--circular-deps'];

  // madge: circular detection with JSON output
  const madgeArgs = ['--circular', '--json', '--extensions', 'ts,tsx,js,jsx,mjs,cjs', '--no-spinner'];
  if (hasTsConfig) madgeArgs.push('--ts-config', 'tsconfig.json');
  madgeArgs.push('src/');

  // dpdm: circular detection with JSON output to stdout
  const dpdmOutputFile = join(dir, '.dpdm-output.json');
  const dpdmArgs = ['--no-tree', '--no-warning', '--no-progress', '--output', dpdmOutputFile];
  if (hasTsConfig) dpdmArgs.push('--tsconfig', 'tsconfig.json');
  dpdmArgs.push('src/index.ts');

  // Warmup
  for (let i = 0; i < WARMUP; i++) {
    timeRunWithMemory(fallowBin, fallowArgs, dir, RUN_TIMEOUT);
    if (hasMadge) timeRunWithMemory(madgeBin, madgeArgs, dir, RUN_TIMEOUT);
    if (hasDpdm) {
      timeRunWithMemory(dpdmBin, dpdmArgs, dir, RUN_TIMEOUT);
      if (existsSync(dpdmOutputFile)) rmSync(dpdmOutputFile);
    }
  }

  // --- Measured runs ---
  const fallowTimes = [], madgeTimes = [], dpdmTimes = [];
  let fallowCycles = '?', madgeCycles = '?', dpdmCycles = '?';
  let fallowRss = 0, madgeRss = 0, dpdmRss = 0;

  for (let i = 0; i < RUNS; i++) {
    // fallow
    const fr = timeRunWithMemory(fallowBin, fallowArgs, dir, RUN_TIMEOUT);
    fallowTimes.push(fr.elapsed);
    if (i === 0) { fallowCycles = parseFallowCycles(fr.stdout); fallowRss = fr.peakRssBytes; }

    // madge
    if (hasMadge) {
      const mr = timeRunWithMemory(madgeBin, madgeArgs, dir, RUN_TIMEOUT);
      madgeTimes.push(mr.elapsed);
      if (i === 0) { madgeCycles = parseMadgeCycles(mr.stdout); madgeRss = mr.peakRssBytes; }
    }

    // dpdm
    if (hasDpdm) {
      const dr = timeRunWithMemory(dpdmBin, dpdmArgs, dir, RUN_TIMEOUT);
      dpdmTimes.push(dr.elapsed);
      if (i === 0) {
        try {
          dpdmCycles = parseDpdmCycles(readFileSync(dpdmOutputFile, 'utf8'));
        } catch { dpdmCycles = '?'; }
        dpdmRss = dr.peakRssBytes;
      }
      if (existsSync(dpdmOutputFile)) rmSync(dpdmOutputFile);
    }
  }

  const fs = stats(fallowTimes);
  const rows = [
    { Tool: 'fallow', Min: fmt(fs.min), Mean: fmt(fs.mean), Median: fmt(fs.median), Max: fmt(fs.max), Speedup: '—', Memory: fmtMem(fallowRss), Cycles: fallowCycles },
  ];

  const result = { name, files, fallow: fs, fallowCycles, fallowRss };

  if (hasMadge && madgeTimes.length > 0) {
    const ms = stats(madgeTimes);
    const speedup = ms.median / fs.median;
    rows.push({ Tool: 'madge', Min: fmt(ms.min), Mean: fmt(ms.mean), Median: fmt(ms.median), Max: fmt(ms.max), Speedup: `1/${speedup.toFixed(1)}x`, Memory: fmtMem(madgeRss), Cycles: madgeCycles });
    result.madge = ms;
    result.madgeSpeedup = speedup;
    result.madgeCycles = madgeCycles;
    result.madgeRss = madgeRss;
  }

  if (hasDpdm && dpdmTimes.length > 0) {
    const ds = stats(dpdmTimes);
    const speedup = ds.median / fs.median;
    rows.push({ Tool: 'dpdm', Min: fmt(ds.min), Mean: fmt(ds.mean), Median: fmt(ds.median), Max: fmt(ds.max), Speedup: `1/${speedup.toFixed(1)}x`, Memory: fmtMem(dpdmRss), Cycles: dpdmCycles });
    result.dpdm = ds;
    result.dpdmSpeedup = speedup;
    result.dpdmCycles = dpdmCycles;
    result.dpdmRss = dpdmRss;
  }

  console.table(rows);
  console.log(`  fallow: [${fallowTimes.map(t => t.toFixed(0)).join(', ')}]`);
  if (hasMadge && madgeTimes.length > 0) console.log(`  madge:  [${madgeTimes.map(t => t.toFixed(0)).join(', ')}]`);
  if (hasDpdm && dpdmTimes.length > 0) console.log(`  dpdm:   [${dpdmTimes.map(t => t.toFixed(0)).join(', ')}]`);
  console.log('');

  return result;
}

const results = [];

runProjectBenchmarks({
  enabled: runSynthetic,
  dir: join(__dirname, 'fixtures', 'synthetic-circular'),
  missingMessage: 'No synthetic circular fixtures. Run: npm run generate:circular\n',
  heading: '--- Synthetic Projects (Circular Dependencies) ---\n',
  results,
  benchmarkProject,
  order: ['tiny', 'small', 'medium', 'large', 'xlarge'],
});

runProjectBenchmarks({
  enabled: runRealWorld,
  dir: join(__dirname, 'fixtures', 'real-world'),
  missingMessage: 'No real-world fixtures. Run: npm run download-fixtures\n',
  heading: '--- Real-World Projects (Circular Dependencies) ---\n',
  results,
  benchmarkProject,
});

if (results.length > 0) {
  console.log('\n=== Summary ===\n');
  const summaryRows = results.map(r => {
    const row = {
      Project: r.name,
      Files: r.files,
      'Fallow (median)': fmt(r.fallow.median),
      'Fallow cycles': r.fallowCycles,
      'Fallow RSS': fmtMem(r.fallowRss),
    };
    if (r.madge) {
      row['madge (median)'] = fmt(r.madge.median);
      row['vs madge'] = `${r.madgeSpeedup.toFixed(1)}x`;
    }
    if (r.dpdm) {
      row['dpdm (median)'] = fmt(r.dpdm.median);
      row['vs dpdm'] = `${r.dpdmSpeedup.toFixed(1)}x`;
    }
    return row;
  });
  console.table(summaryRows);

  if (results.some(r => r.madgeSpeedup)) {
    const avgMadge = results.filter(r => r.madgeSpeedup).reduce((s, r) => s + r.madgeSpeedup, 0) / results.filter(r => r.madgeSpeedup).length;
    console.log(`Average speedup vs madge: ${avgMadge.toFixed(1)}x faster`);
  }
  if (results.some(r => r.dpdmSpeedup)) {
    const avgDpdm = results.filter(r => r.dpdmSpeedup).reduce((s, r) => s + r.dpdmSpeedup, 0) / results.filter(r => r.dpdmSpeedup).length;
    console.log(`Average speedup vs dpdm: ${avgDpdm.toFixed(1)}x faster`);
  }
  console.log('');
}
