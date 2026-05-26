#!/usr/bin/env node
import { mkdirSync, writeFileSync, existsSync, rmSync } from "node:fs";
import { join, dirname, relative } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const fixturesDir = join(__dirname, "fixtures", "synthetic-circular");

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
  { name: "tiny", files: 10, cycleCount: 2, maxCycleLen: 3 },
  { name: "small", files: 50, cycleCount: 5, maxCycleLen: 4 },
  { name: "medium", files: 200, cycleCount: 15, maxCycleLen: 5 },
  { name: "large", files: 1000, cycleCount: 50, maxCycleLen: 6 },
  { name: "xlarge", files: 5000, cycleCount: 200, maxCycleLen: 8 },
];

const DIRS = ["components", "utils", "hooks", "services", "types", "models", "helpers", "lib"];
const ENTITIES = ["User", "Order", "Product", "Invoice", "Payment", "Session", "Account", "Report"];
const ACTIONS = [
  "validate",
  "transform",
  "process",
  "normalize",
  "sanitize",
  "format",
  "parse",
  "convert",
];

function filePath(i) {
  const dir = DIRS[i % DIRS.length];
  return `src/${dir}/module-${i}.ts`;
}

function relImport(fromIdx, toIdx) {
  const from = filePath(fromIdx);
  const to = filePath(toIdx);
  let rel = relative(dirname(from), to).replace(/\.ts$/, "");
  if (!rel.startsWith(".")) rel = "./" + rel;
  return rel;
}

function generateProject(size) {
  const { name, files: fileCount, cycleCount, maxCycleLen } = size;
  const projectDir = join(fixturesDir, name);
  if (existsSync(projectDir)) rmSync(projectDir, { recursive: true });
  const rand = mulberry32(42 + fileCount);
  const srcDir = join(projectDir, "src");
  for (const dir of DIRS) mkdirSync(join(srcDir, dir), { recursive: true });

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
    if (deps.size > 0) lines.push("");

    // Exported function that uses imports
    lines.push(`export const export_${i} = (input: string): string => {`);
    if (deps.size > 0) {
      const depArr = [...deps];
      lines.push(`  const deps = [${depArr.map((d) => `export_${d}(input)`).join(", ")}];`);
      lines.push(`  return deps.join('_');`);
    } else {
      lines.push(`  return '${entity}_${action}_' + input;`);
    }
    lines.push(`};`);
    lines.push("");

    // Extra exports for realistic file size
    lines.push(`export const ${action}${entity}_${i} = (value: number): number => {`);
    lines.push(`  return value * ${i + 1} + ${Math.floor(rand() * 100)};`);
    lines.push(`};`);
    lines.push("");
    lines.push(`export interface ${entity}Config_${i} {`);
    lines.push(`  readonly id: string;`);
    lines.push(`  readonly name: string;`);
    lines.push(`  readonly enabled: boolean;`);
    lines.push(`}`);

    const content = lines.join("\n") + "\n";
    totalLines += content.split("\n").length;
    mkdirSync(dirname(fp), { recursive: true });
    writeFileSync(fp, content);
  }

  // Entry point that imports from several files
  const entryImports = [];
  const importCount = Math.min(20, Math.floor(fileCount * 0.1));
  for (let i = 0; i < importCount; i++) {
    const idx = Math.floor(rand() * fileCount);
    entryImports.push(
      `export { export_${idx} } from './${filePath(idx)
        .replace(/^src\//, "")
        .replace(/\.ts$/, "")}';`,
    );
  }
  writeFileSync(
    join(srcDir, "index.ts"),
    [`// Entry point for ${name} circular dependency benchmark`, ...entryImports, ""].join("\n"),
  );

  writeFileSync(
    join(projectDir, "package.json"),
    JSON.stringify(
      {
        name: `bench-circular-${name}`,
        version: "1.0.0",
        private: true,
        main: "src/index.ts",
      },
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

  return { name, fileCount, actualCycles, totalLines };
}

console.log("Generating synthetic circular dependency benchmark fixtures...\n");
for (const size of SIZES) {
  const start = performance.now();
  const stats = generateProject(size);
  const elapsed = performance.now() - start;
  console.log(
    `  ${stats.name.padEnd(8)} ${String(stats.fileCount).padStart(5)} files  ${String(stats.actualCycles).padStart(4)} cycles  ${String(stats.totalLines).padStart(7)} lines  (${elapsed.toFixed(0)}ms)`,
  );
}
console.log("\nDone. Run: npm run bench:circular:synthetic");
