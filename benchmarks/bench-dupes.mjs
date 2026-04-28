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
  timeRun,
  timeRunWithMemory,
} from './benchmark-helpers.mjs';

const __dirname = benchmarkDir;
const { runSynthetic, runRealWorld, RUNS, WARMUP } = parseBenchmarkArgs();
const RUN_TIMEOUT = 600000;

const fallowBin = buildFallowRelease();
const jscpdBin = join(__dirname, 'node_modules', '.bin', 'jscpd');
if (!existsSync(jscpdBin)) { console.error('jscpd not found. Run: cd benchmarks && npm install'); process.exit(1); }

const fallowVersion = getVersion(fallowBin);
const jscpdVersion = getVersion(jscpdBin);
const rustVersion = getVersion('rustc');

console.log(`\n=== Fallow Dupes vs jscpd Benchmark Suite ===\n`);
printEnvironment(rustVersion);
console.log(`Tools:\n  fallow dupes  ${fallowVersion}\n  jscpd         ${jscpdVersion}\nConfig: ${RUNS} runs, ${WARMUP} warmup\n`);

function parseFallowCloneCount(stdout) {
  try {
    const data = JSON.parse(stdout);
    return {
      groups: data.stats?.clone_groups ?? data.clone_groups?.length ?? '?',
      instances: data.stats?.clone_instances ?? '?',
      pct: data.stats?.duplication_percentage?.toFixed(1) ?? '?',
    };
  } catch { return { groups: '?', instances: '?', pct: '?' }; }
}

function parseJscpdCloneCount(reportDir) {
  try {
    const reportPath = join(reportDir, 'jscpd-report.json');
    if (!existsSync(reportPath)) return { groups: '?', instances: '?', pct: '?' };
    const data = JSON.parse(readFileSync(reportPath, 'utf8'));
    const stats = data.statistics?.total;
    return {
      groups: data.duplicates?.length ?? '?',
      instances: stats?.clones ?? '?',
      pct: stats?.percentage?.toFixed(1) ?? '?',
    };
  } catch { return { groups: '?', instances: '?', pct: '?' }; }
}

function benchmarkProject(name, dir) {
  const files = countSourceFiles(dir, ['node_modules', '.git', 'dist', 'report']);
  console.log(`### ${name} (${files} source files)\n`);

  // fallow dupes: JSON output, no cache (cold)
  const fArgsCold = ['dupes', '--format', 'json', '--no-cache'];

  // jscpd: JSON reporter, output to temp dir
  const jscpdReportDir = join(dir, 'report');
  const jArgs = [
    '--reporters', 'json',
    '--format', 'typescript,javascript',
    '--output', jscpdReportDir,
    '--min-tokens', '50',
    '--min-lines', '5',
    '--ignore', '**/node_modules/**,**/dist/**,**/.git/**',
    '--silent',
    '.',
  ];

  // Warmup
  for (let i = 0; i < WARMUP; i++) {
    timeRun(fallowBin, fArgsCold, dir, RUN_TIMEOUT);
    if (existsSync(jscpdReportDir)) rmSync(jscpdReportDir, { recursive: true });
    timeRun(jscpdBin, jArgs, dir, RUN_TIMEOUT);
    if (existsSync(jscpdReportDir)) rmSync(jscpdReportDir, { recursive: true });
  }

  // --- Cold runs ---
  const fTimesCold = [], jTimes = [];
  let fClones = { groups: '?', instances: '?', pct: '?' };
  let jClones = { groups: '?', instances: '?', pct: '?' };
  let fPeakRss = 0, jPeakRss = 0;

  for (let i = 0; i < RUNS; i++) {
    const fr = timeRunWithMemory(fallowBin, fArgsCold, dir, RUN_TIMEOUT);
    fTimesCold.push(fr.elapsed);
    if (i === 0) { fClones = parseFallowCloneCount(fr.stdout); fPeakRss = fr.peakRssBytes; }

    if (existsSync(jscpdReportDir)) rmSync(jscpdReportDir, { recursive: true });
    const jr = timeRunWithMemory(jscpdBin, jArgs, dir, RUN_TIMEOUT);
    jTimes.push(jr.elapsed);
    if (i === 0) { jClones = parseJscpdCloneCount(jscpdReportDir); jPeakRss = jr.peakRssBytes; }
    if (existsSync(jscpdReportDir)) rmSync(jscpdReportDir, { recursive: true });
  }

  const fsCold = stats(fTimesCold), js = stats(jTimes);
  const speedup = js.median / fsCold.median;

  console.table([
    { Tool: 'fallow dupes', Min: fmt(fsCold.min), Mean: fmt(fsCold.mean), Median: fmt(fsCold.median), Max: fmt(fsCold.max), Speedup: `${speedup.toFixed(1)}x`, Memory: fmtMem(fPeakRss), 'Clone Groups': fClones.groups, 'Dup %': `${fClones.pct}%` },
    { Tool: 'jscpd',        Min: fmt(js.min),     Mean: fmt(js.mean),     Median: fmt(js.median),     Max: fmt(js.max),     Speedup: '1.0x',                       Memory: fmtMem(jPeakRss), 'Clone Groups': jClones.groups, 'Dup %': `${jClones.pct}%` },
  ]);
  console.log(`  fallow: [${fTimesCold.map(t => t.toFixed(0)).join(', ')}]`);
  console.log(`  jscpd:  [${jTimes.map(t => t.toFixed(0)).join(', ')}]\n`);

  return { name, files, fallow: fsCold, jscpd: js, speedup, fClones, jClones, fPeakRss, jPeakRss };
}

const results = [];

runProjectBenchmarks({
  enabled: runSynthetic,
  dir: join(__dirname, 'fixtures', 'synthetic-dupes'),
  missingMessage: 'No synthetic dupes fixtures. Run: npm run generate:dupes\n',
  heading: '--- Synthetic Projects (Duplication) ---\n',
  results,
  benchmarkProject,
  order: ['tiny', 'small', 'medium', 'large', 'xlarge'],
});

runProjectBenchmarks({
  enabled: runRealWorld,
  dir: join(__dirname, 'fixtures', 'real-world'),
  missingMessage: 'No real-world fixtures. Run: npm run download-fixtures\n',
  heading: '--- Real-World Projects (Duplication) ---\n',
  results,
  benchmarkProject,
});

if (results.length > 0) {
  console.log('\n=== Summary ===\n');
  console.table(results.map(r => ({
    Project: r.name,
    Files: r.files,
    'Fallow (median)': fmt(r.fallow.median),
    'jscpd (median)': fmt(r.jscpd.median),
    Speedup: `${r.speedup.toFixed(1)}x`,
    'Fallow RSS': fmtMem(r.fPeakRss),
    'jscpd RSS': fmtMem(r.jPeakRss),
    'Fallow clones': r.fClones.groups,
    'jscpd clones': r.jClones.groups,
  })));
  console.log(`Average speedup: ${(results.reduce((s, r) => s + r.speedup, 0) / results.length).toFixed(1)}x faster\n`);
}
