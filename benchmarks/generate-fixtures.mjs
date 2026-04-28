#!/usr/bin/env node
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import {
  benchmarkDir,
  relativeTsImport,
  resetProject,
  runGenerator,
  STANDARD_DIRS,
  writePackageManifest,
  writeTsConfig,
} from './fixture-generator-helpers.mjs';

const fixturesDir = join(benchmarkDir, 'fixtures', 'synthetic');

const SIZES = [
  { name: 'tiny', files: 10, exportsPerFile: 3 },
  { name: 'small', files: 50, exportsPerFile: 4 },
  { name: 'medium', files: 200, exportsPerFile: 4 },
  { name: 'large', files: 1000, exportsPerFile: 5 },
  { name: 'xlarge', files: 5000, exportsPerFile: 5 },
];

const DIRS = STANDARD_DIRS;
const TYPES = ['string','number','boolean','string[]','Record<string, unknown>'];
const STATUSES = ['active','inactive','pending','archived','deleted'];

function generateProject(size) {
  const { name, files: fileCount, exportsPerFile } = size;
  const projectDir = join(fixturesDir, name);
  const { rand, srcDir } = resetProject(projectDir, 42 + fileCount, DIRS);

  const usedCount = Math.floor(fileCount * 0.8);
  const fileInfos = [];
  for (let i = 0; i < fileCount; i++) {
    const dir = DIRS[i % DIRS.length];
    const exports = [];
    for (let e = 0; e < exportsPerFile; e++) {
      const kind = e === 0 ? 'interface' : e === 1 ? 'type' : e === 2 ? 'function' : 'const';
      exports.push({ name: `${kind === 'interface' ? 'I' : kind === 'type' ? 'T' : kind === 'function' ? 'fn' : 'val'}_${i}_${e}`, kind });
    }
    fileInfos.push({ id: i, path: `src/${dir}/module-${i}.ts`, dir, exports, imports: [], isUsed: i < usedCount });
  }

  const entryImportCount = Math.min(Math.floor(fileCount * 0.05) + 2, usedCount);
  const entryImports = [];
  for (let i = 0; i < entryImportCount; i++) {
    const targetIdx = 1 + Math.floor(rand() * Math.min(usedCount - 1, 20));
    if (!entryImports.includes(targetIdx)) entryImports.push(targetIdx);
  }

  for (let i = 1; i < usedCount; i++) {
    const importCount = 1 + Math.floor(rand() * 3);
    for (let j = 0; j < importCount; j++) {
      let target = rand() < 0.7 && i > 5 ? Math.floor(rand() * Math.min(i, usedCount)) : Math.floor(rand() * usedCount);
      if (target !== i && !fileInfos[i].imports.includes(target)) fileInfos[i].imports.push(target);
    }
  }

  const importedExports = new Set();
  for (const file of fileInfos) {
    for (const targetIdx of file.imports) {
      const target = fileInfos[targetIdx];
      const count = 1 + Math.floor(rand() * 2);
      for (let e = 0; e < count && e < target.exports.length; e++) importedExports.add(`${targetIdx}:${target.exports[e].name}`);
    }
  }
  for (const targetIdx of entryImports) importedExports.add(`${targetIdx}:${fileInfos[targetIdx].exports[0].name}`);

  for (const file of fileInfos) {
    const fullPath = join(projectDir, file.path);
    let content = '';
    for (const targetIdx of file.imports) {
      const target = fileInfos[targetIdx];
      const importedNames = [];
      const count = 1 + Math.floor(rand() * 2);
      for (let e = 0; e < count && e < target.exports.length; e++) {
        const exp = target.exports[e];
        importedNames.push(exp.kind === 'type' || exp.kind === 'interface' ? `type ${exp.name}` : exp.name);
      }
      content += `import { ${importedNames.join(', ')} } from '${relativeTsImport(file.path, target.path)}';\n`;
    }
    if (file.imports.length > 0) content += '\n';
    for (const exp of file.exports) {
      switch (exp.kind) {
        case 'interface': content += `export interface ${exp.name} {\n  id: number;\n  name: string;\n  status: '${STATUSES[Math.floor(rand() * STATUSES.length)]}';\n  value: ${TYPES[Math.floor(rand() * TYPES.length)]};\n}\n\n`; break;
        case 'type': content += `export type ${exp.name} = '${STATUSES[Math.floor(rand() * STATUSES.length)]}' | '${STATUSES[Math.floor(rand() * STATUSES.length)]}';\n\n`; break;
        case 'function': content += `export const ${exp.name} = (input: string): string => {\n  return input.toUpperCase();\n};\n\n`; break;
        case 'const': content += `export const ${exp.name} = ${Math.floor(rand() * 1000)};\n\n`; break;
      }
    }
    writeFileSync(fullPath, content);
  }

  let entryContent = '';
  for (const targetIdx of entryImports) {
    const target = fileInfos[targetIdx]; const exp = target.exports[0];
    const importName = exp.kind === 'type' || exp.kind === 'interface' ? `type ${exp.name}` : exp.name;
    entryContent += `import { ${importName} } from '${relativeTsImport('src/index.ts', target.path)}';\n`;
  }
  entryContent += '\n';
  for (const targetIdx of entryImports) {
    const exp = fileInfos[targetIdx].exports[0];
    if (exp.kind !== 'type' && exp.kind !== 'interface') entryContent += `console.log(${exp.name});\n`;
  }
  writeFileSync(join(srcDir, 'index.ts'), entryContent);
  writePackageManifest(projectDir, `bench-${name}`);
  writeTsConfig(projectDir);

  const totalExports = fileInfos.reduce((s, f) => s + f.exports.length, 0);
  return { name, fileCount, totalExports, unusedFiles: fileInfos.filter(f => !f.isUsed).length, unusedExports: totalExports - importedExports.size };
}

runGenerator({
  heading: 'Generating synthetic fixture projects...',
  sizes: SIZES,
  generateProject,
  formatStats: (stats, elapsed) => `  ${stats.name.padEnd(8)} ${String(stats.fileCount).padStart(5)} files  ${String(stats.totalExports).padStart(6)} exports  ${String(stats.unusedFiles).padStart(4)} unused files  ${String(stats.unusedExports).padStart(5)} unused exports  (${elapsed.toFixed(0)}ms)`,
  doneCommand: 'npm run bench:synthetic',
});
