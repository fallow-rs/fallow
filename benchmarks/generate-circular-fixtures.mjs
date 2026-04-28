#!/usr/bin/env node
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import {
  benchmarkDir,
  relativeTsImport,
  resetProject,
  runGenerator,
  STANDARD_DIRS,
  writePackageManifest,
  writeTsConfig,
} from './fixture-generator-helpers.mjs';

const fixturesDir = join(benchmarkDir, 'fixtures', 'synthetic-circular');

const SIZES = [
  { name: 'tiny',   files: 10,   cycleCount: 2,   maxCycleLen: 3  },
  { name: 'small',  files: 50,   cycleCount: 5,   maxCycleLen: 4  },
  { name: 'medium', files: 200,  cycleCount: 15,  maxCycleLen: 5  },
  { name: 'large',  files: 1000, cycleCount: 50,  maxCycleLen: 6  },
  { name: 'xlarge', files: 5000, cycleCount: 200, maxCycleLen: 8  },
];

const DIRS = STANDARD_DIRS;
const ENTITIES = ['User', 'Order', 'Product', 'Invoice', 'Payment', 'Session', 'Account', 'Report'];
const ACTIONS = ['validate', 'transform', 'process', 'normalize', 'sanitize', 'format', 'parse', 'convert'];

function filePath(i) {
  const dir = DIRS[i % DIRS.length];
  return `src/${dir}/module-${i}.ts`;
}

function relImport(fromIdx, toIdx) {
  return relativeTsImport(filePath(fromIdx), filePath(toIdx));
}

function generateProject(size) {
  const { name, files: fileCount, cycleCount, maxCycleLen } = size;
  const projectDir = join(fixturesDir, name);
  const { rand, srcDir } = resetProject(projectDir, 42 + fileCount, DIRS);

  // Build import graph: each file imports from a few others (acyclic forward references)
  const imports = new Map(); // fileIndex -> Set<fileIndex>
  for (let i = 0; i < fileCount; i++) imports.set(i, new Set());

  // Create a base acyclic graph: each file imports from 1-3 earlier files
  for (let i = 1; i < fileCount; i++) {
    const count = 1 + Math.floor(rand() * Math.min(3, i));
    for (let c = 0; c < count; c++) {
      const target = Math.floor(rand() * i);
      imports.get(i).add(target);
    }
  }

  // Inject circular dependencies: create back-edges to form cycles
  const usedInCycles = new Set();
  let actualCycles = 0;

  for (let c = 0; c < cycleCount && actualCycles < cycleCount; c++) {
    const cycleLen = 2 + Math.floor(rand() * (maxCycleLen - 1));
    // Pick random files for the cycle, avoiding overlap with existing cycles
    const candidates = [];
    let attempts = 0;
    while (candidates.length < cycleLen && attempts < cycleLen * 10) {
      const idx = Math.floor(rand() * fileCount);
      if (!usedInCycles.has(idx) && !candidates.includes(idx)) {
        candidates.push(idx);
      }
      attempts++;
    }
    if (candidates.length < 2) continue;

    // Form the cycle: 0→1→2→...→n→0
    for (let i = 0; i < candidates.length; i++) {
      const from = candidates[i];
      const to = candidates[(i + 1) % candidates.length];
      imports.get(from).add(to);
      usedInCycles.add(from);
    }
    actualCycles++;
  }

  // Generate source files
  let totalLines = 0;
  for (let i = 0; i < fileCount; i++) {
    const fp = join(projectDir, filePath(i));
    const entity = ENTITIES[i % ENTITIES.length];
    const action = ACTIONS[i % ACTIONS.length];
    const deps = imports.get(i);

    const lines = [];

    // Import statements
    for (const dep of deps) {
      lines.push(`import { export_${dep} } from '${relImport(i, dep)}';`);
    }
    if (deps.size > 0) lines.push('');

    // Exported function that uses imports
    lines.push(`export const export_${i} = (input: string): string => {`);
    if (deps.size > 0) {
      const depArr = [...deps];
      lines.push(`  const deps = [${depArr.map(d => `export_${d}(input)`).join(', ')}];`);
      lines.push(`  return deps.join('_');`);
    } else {
      lines.push(`  return '${entity}_${action}_' + input;`);
    }
    lines.push(`};`);
    lines.push('');

    // Extra exports for realistic file size
    lines.push(`export const ${action}${entity}_${i} = (value: number): number => {`);
    lines.push(`  return value * ${i + 1} + ${Math.floor(rand() * 100)};`);
    lines.push(`};`);
    lines.push('');
    lines.push(`export interface ${entity}Config_${i} {`);
    lines.push(`  readonly id: string;`);
    lines.push(`  readonly name: string;`);
    lines.push(`  readonly enabled: boolean;`);
    lines.push(`}`);

    const content = lines.join('\n') + '\n';
    totalLines += content.split('\n').length;
    mkdirSync(dirname(fp), { recursive: true });
    writeFileSync(fp, content);
  }

  // Entry point that imports from several files
  const entryImports = [];
  const importCount = Math.min(20, Math.floor(fileCount * 0.1));
  for (let i = 0; i < importCount; i++) {
    const idx = Math.floor(rand() * fileCount);
    entryImports.push(`export { export_${idx} } from './${filePath(idx).replace(/^src\//, '').replace(/\.ts$/, '')}';`);
  }
  writeFileSync(join(srcDir, 'index.ts'), [
    `// Entry point for ${name} circular dependency benchmark`,
    ...entryImports,
    '',
  ].join('\n'));

  writePackageManifest(projectDir, `bench-circular-${name}`);
  writeTsConfig(projectDir);

  return { name, fileCount, actualCycles, totalLines };
}

runGenerator({
  heading: 'Generating synthetic circular dependency benchmark fixtures...',
  sizes: SIZES,
  generateProject,
  formatStats: (stats, elapsed) => `  ${stats.name.padEnd(8)} ${String(stats.fileCount).padStart(5)} files  ${String(stats.actualCycles).padStart(4)} cycles  ${String(stats.totalLines).padStart(7)} lines  (${elapsed.toFixed(0)}ms)`,
  doneCommand: 'npm run bench:circular:synthetic',
});
