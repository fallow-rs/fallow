import { existsSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

export const benchmarkDir = dirname(fileURLToPath(import.meta.url));
export const STANDARD_DIRS = ['components', 'utils', 'hooks', 'services', 'types', 'models', 'helpers', 'lib'];

export function mulberry32(seed) {
  return function () {
    seed |= 0; seed = (seed + 0x6d2b79f5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

export function resetProject(projectDir, seed, dirs = STANDARD_DIRS) {
  if (existsSync(projectDir)) rmSync(projectDir, { recursive: true });
  const rand = mulberry32(seed);
  const srcDir = join(projectDir, 'src');
  for (const dir of dirs) mkdirSync(join(srcDir, dir), { recursive: true });
  return { rand, srcDir };
}

export function writePackageManifest(projectDir, packageName) {
  writeFileSync(join(projectDir, 'package.json'), JSON.stringify({
    name: packageName,
    version: '1.0.0',
    private: true,
    main: 'src/index.ts',
  }, null, 2) + '\n');
}

export function writeTsConfig(projectDir) {
  writeFileSync(join(projectDir, 'tsconfig.json'), JSON.stringify({
    compilerOptions: {
      target: 'ES2022', module: 'ESNext', moduleResolution: 'bundler',
      strict: true, esModuleInterop: true, skipLibCheck: true,
      outDir: 'dist', rootDir: 'src', declaration: true, baseUrl: '.',
    },
    include: ['src'],
  }, null, 2) + '\n');
}

export function relativeTsImport(fromFile, toFile) {
  const fromDir = dirname(fromFile);
  let rel = relative(fromDir, toFile).replace(/\.ts$/, '');
  if (!rel.startsWith('.')) rel = './' + rel;
  return rel;
}

export function runGenerator({ heading, sizes, generateProject, formatStats, doneCommand }) {
  console.log(`${heading}\n`);
  for (const size of sizes) {
    const start = performance.now();
    const stats = generateProject(size);
    const elapsed = performance.now() - start;
    console.log(formatStats(stats, elapsed));
  }
  console.log(`\nDone. Run: ${doneCommand}`);
}
