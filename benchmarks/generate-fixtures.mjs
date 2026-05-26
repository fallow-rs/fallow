#!/usr/bin/env node
import { mkdirSync, writeFileSync, existsSync, rmSync } from "node:fs";
import { join, dirname, relative } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const fixturesDir = join(__dirname, "fixtures", "synthetic");

function mulberry32(seed) {
  return function () {
    seed |= 0;
    seed = (seed + 0x6d2b79f5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const SIZES = [
  { name: "tiny", files: 10, exportsPerFile: 3 },
  { name: "small", files: 50, exportsPerFile: 4 },
  { name: "medium", files: 200, exportsPerFile: 4 },
  { name: "large", files: 1000, exportsPerFile: 5 },
  { name: "xlarge", files: 5000, exportsPerFile: 5 },
];

const DIRS = ["components", "utils", "hooks", "services", "types", "models", "helpers", "lib"];
const TYPES = ["string", "number", "boolean", "string[]", "Record<string, unknown>"];
const STATUSES = ["active", "inactive", "pending", "archived", "deleted"];

function generateProject(size) {
  const { name, files: fileCount, exportsPerFile } = size;
  const projectDir = join(fixturesDir, name);
  if (existsSync(projectDir)) rmSync(projectDir, { recursive: true });
  const rand = mulberry32(42 + fileCount);
  const srcDir = join(projectDir, "src");
  for (const dir of DIRS) mkdirSync(join(srcDir, dir), { recursive: true });

  const usedCount = Math.floor(fileCount * 0.8);
  const fileInfos = [];
  for (let i = 0; i < fileCount; i++) {
    const dir = DIRS[i % DIRS.length];
    const exports = [];
    for (let e = 0; e < exportsPerFile; e++) {
      const kind = e === 0 ? "interface" : e === 1 ? "type" : e === 2 ? "function" : "const";
      exports.push({
        name: `${kind === "interface" ? "I" : kind === "type" ? "T" : kind === "function" ? "fn" : "val"}_${i}_${e}`,
        kind,
      });
    }
    fileInfos.push({
      id: i,
      path: `src/${dir}/module-${i}.ts`,
      dir,
      exports,
      imports: [],
      isUsed: i < usedCount,
    });
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
      let target =
        rand() < 0.7 && i > 5
          ? Math.floor(rand() * Math.min(i, usedCount))
          : Math.floor(rand() * usedCount);
      if (target !== i && !fileInfos[i].imports.includes(target)) fileInfos[i].imports.push(target);
    }
  }

  const importedExports = new Set();
  for (const file of fileInfos) {
    for (const targetIdx of file.imports) {
      const target = fileInfos[targetIdx];
      const count = 1 + Math.floor(rand() * 2);
      for (let e = 0; e < count && e < target.exports.length; e++)
        importedExports.add(`${targetIdx}:${target.exports[e].name}`);
    }
  }
  for (const targetIdx of entryImports)
    importedExports.add(`${targetIdx}:${fileInfos[targetIdx].exports[0].name}`);

  for (const file of fileInfos) {
    const fullPath = join(projectDir, file.path);
    let content = "";
    for (const targetIdx of file.imports) {
      const target = fileInfos[targetIdx];
      const importedNames = [];
      const count = 1 + Math.floor(rand() * 2);
      for (let e = 0; e < count && e < target.exports.length; e++) {
        const exp = target.exports[e];
        importedNames.push(
          exp.kind === "type" || exp.kind === "interface" ? `type ${exp.name}` : exp.name,
        );
      }
      content += `import { ${importedNames.join(", ")} } from '${relativePath(file.path, target.path)}';\n`;
    }
    if (file.imports.length > 0) content += "\n";
    for (const exp of file.exports) {
      switch (exp.kind) {
        case "interface":
          content += `export interface ${exp.name} {\n  id: number;\n  name: string;\n  status: '${STATUSES[Math.floor(rand() * STATUSES.length)]}';\n  value: ${TYPES[Math.floor(rand() * TYPES.length)]};\n}\n\n`;
          break;
        case "type":
          content += `export type ${exp.name} = '${STATUSES[Math.floor(rand() * STATUSES.length)]}' | '${STATUSES[Math.floor(rand() * STATUSES.length)]}';\n\n`;
          break;
        case "function":
          content += `export const ${exp.name} = (input: string): string => {\n  return input.toUpperCase();\n};\n\n`;
          break;
        case "const":
          content += `export const ${exp.name} = ${Math.floor(rand() * 1000)};\n\n`;
          break;
      }
    }
    writeFileSync(fullPath, content);
  }

  let entryContent = "";
  for (const targetIdx of entryImports) {
    const target = fileInfos[targetIdx];
    const exp = target.exports[0];
    const importName =
      exp.kind === "type" || exp.kind === "interface" ? `type ${exp.name}` : exp.name;
    entryContent += `import { ${importName} } from '${relativePath("src/index.ts", target.path)}';\n`;
  }
  entryContent += "\n";
  for (const targetIdx of entryImports) {
    const exp = fileInfos[targetIdx].exports[0];
    if (exp.kind !== "type" && exp.kind !== "interface")
      entryContent += `console.log(${exp.name});\n`;
  }
  writeFileSync(join(srcDir, "index.ts"), entryContent);
  writeFileSync(
    join(projectDir, "package.json"),
    JSON.stringify(
      { name: `bench-${name}`, version: "1.0.0", private: true, main: "src/index.ts" },
      null,
      2,
    ) + "\n",
  );
  writeFileSync(
    join(projectDir, "tsconfig.json"),
    JSON.stringify(
      {
        compilerOptions: {
          target: "ES2022",
          module: "ESNext",
          moduleResolution: "bundler",
          strict: true,
          esModuleInterop: true,
          skipLibCheck: true,
          outDir: "dist",
          rootDir: "src",
          declaration: true,
          baseUrl: ".",
        },
        include: ["src"],
      },
      null,
      2,
    ) + "\n",
  );

  const totalExports = fileInfos.reduce((s, f) => s + f.exports.length, 0);
  return {
    name,
    fileCount,
    totalExports,
    unusedFiles: fileInfos.filter((f) => !f.isUsed).length,
    unusedExports: totalExports - importedExports.size,
  };
}

function relativePath(fromFile, toFile) {
  const fromDir = dirname(fromFile);
  let rel = relative(fromDir, toFile).replace(/\.ts$/, "");
  if (!rel.startsWith(".")) rel = "./" + rel;
  return rel;
}

console.log("Generating synthetic fixture projects...\n");
for (const size of SIZES) {
  const start = performance.now();
  const stats = generateProject(size);
  const elapsed = performance.now() - start;
  console.log(
    `  ${stats.name.padEnd(8)} ${String(stats.fileCount).padStart(5)} files  ${String(stats.totalExports).padStart(6)} exports  ${String(stats.unusedFiles).padStart(4)} unused files  ${String(stats.unusedExports).padStart(5)} unused exports  (${elapsed.toFixed(0)}ms)`,
  );
}
console.log("\nDone. Run: npm run bench:synthetic");
