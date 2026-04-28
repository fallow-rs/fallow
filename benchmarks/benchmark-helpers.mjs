import { spawnSync } from 'node:child_process';
import { existsSync, readdirSync, rmSync, statSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import os from 'node:os';

export const benchmarkDir = dirname(fileURLToPath(import.meta.url));
export const rootDir = resolve(benchmarkDir, '..');

export function parseBenchmarkArgs(args = process.argv.slice(2)) {
  const hasFilter = args.includes('--synthetic') || args.includes('--real-world');
  return {
    runSynthetic: args.includes('--synthetic') || !hasFilter,
    runRealWorld: args.includes('--real-world') || !hasFilter,
    RUNS: parseInt(args.find(a => a.startsWith('--runs='))?.split('=')[1] ?? '5'),
    WARMUP: parseInt(args.find(a => a.startsWith('--warmup='))?.split('=')[1] ?? '2'),
  };
}

export function buildFallowRelease() {
  console.log('Building fallow (release)...');
  const buildResult = spawnSync('cargo', ['build', '--release'], { cwd: rootDir, stdio: 'pipe', timeout: 300000 });
  if (buildResult.status !== 0) { console.error('Build failed:', buildResult.stderr?.toString()); process.exit(1); }
  return join(rootDir, 'target', 'release', 'fallow');
}

export function getVersion(cmd, cmdArgs = ['--version']) {
  return spawnSync(cmd, cmdArgs, { stdio: 'pipe' }).stdout?.toString().trim();
}

export function printEnvironment(rustVersion) {
  const cpus = os.cpus();
  console.log('Environment:');
  console.log(`  CPU:     ${cpus[0].model.trim()} (${cpus.length} logical cores)`);
  console.log(`  RAM:     ${(os.totalmem() / 1024 / 1024 / 1024).toFixed(1)} GB`);
  console.log(`  OS:      ${os.platform()} ${os.release()} ${os.arch()}`);
  console.log(`  Node:    ${process.version}`);
  console.log(`  Rust:    ${rustVersion}`);
  console.log('');
}

export function countSourceFiles(dir, ignored = ['node_modules', '.git', 'dist']) {
  let count = 0;
  const walk = d => {
    try {
      for (const e of readdirSync(d)) {
        if (ignored.includes(e)) continue;
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

export function timeRun(cmd, cmdArgs, cwd, timeout = 300000) {
  const start = performance.now();
  const result = spawnSync(cmd, cmdArgs, {
    cwd,
    stdio: 'pipe',
    timeout,
    maxBuffer: 50 * 1024 * 1024,
    env: { ...process.env, NO_COLOR: '1', FORCE_COLOR: '0' },
  });
  return {
    elapsed: performance.now() - start,
    status: result.status,
    signal: result.signal,
    stdout: result.stdout?.toString() ?? '',
    stderr: result.stderr?.toString() ?? '',
  };
}

export function timeRunWithMemory(cmd, cmdArgs, cwd, timeout = 300000) {
  const isLinux = process.platform === 'linux';
  const timeBin = '/usr/bin/time';
  const timeArgs = isLinux ? ['-v', cmd, ...cmdArgs] : ['-l', cmd, ...cmdArgs];

  const start = performance.now();
  const result = spawnSync(timeBin, timeArgs, {
    cwd,
    stdio: 'pipe',
    timeout,
    maxBuffer: 50 * 1024 * 1024,
    env: { ...process.env, NO_COLOR: '1', FORCE_COLOR: '0' },
  });
  const elapsed = performance.now() - start;
  const stderr = result.stderr?.toString() ?? '';

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
    signal: result.signal,
    stdout: result.stdout?.toString() ?? '',
    stderr,
    peakRssBytes,
  };
}

export function stats(times) {
  const sorted = [...times].sort((a, b) => a - b);
  const mid = Math.floor(sorted.length / 2);
  const median = sorted.length % 2 === 0 ? (sorted[mid - 1] + sorted[mid]) / 2 : sorted[mid];
  return {
    min: sorted[0],
    max: sorted.at(-1),
    mean: sorted.reduce((a, b) => a + b, 0) / sorted.length,
    median,
  };
}

export function fmt(ms) {
  return ms < 1000 ? `${ms.toFixed(0)}ms` : `${(ms / 1000).toFixed(2)}s`;
}

export function fmtMem(bytes) {
  if (bytes === 0) return '?';
  const mb = bytes / 1024 / 1024;
  return mb < 1024 ? `${mb.toFixed(1)} MB` : `${(mb / 1024).toFixed(2)} GB`;
}

export function clearFallowCache(dir) {
  const cacheDir = join(dir, '.fallow');
  if (existsSync(cacheDir)) rmSync(cacheDir, { recursive: true });
}

export function packageProjects(dir, order = null) {
  const projects = readdirSync(dir).filter(x => existsSync(join(dir, x, 'package.json')));
  return order ? projects.sort((a, b) => order.indexOf(a) - order.indexOf(b)) : projects.sort();
}

export function runProjectBenchmarks({ enabled, dir, missingMessage, heading, results, benchmarkProject, order = null }) {
  if (!enabled) return;
  if (!existsSync(dir)) {
    console.log(missingMessage);
  } else {
    console.log(heading);
    for (const p of packageProjects(dir, order)) results.push(benchmarkProject(p, join(dir, p)));
  }
}
