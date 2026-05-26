#!/usr/bin/env node
import { mkdirSync, writeFileSync, existsSync, rmSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const fixturesDir = join(__dirname, "fixtures", "synthetic-dupes");

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
  { name: "tiny", files: 10, dupeGroups: 2, linesPerBlock: 15 },
  { name: "small", files: 50, dupeGroups: 8, linesPerBlock: 20 },
  { name: "medium", files: 200, dupeGroups: 25, linesPerBlock: 25 },
  { name: "large", files: 1000, dupeGroups: 80, linesPerBlock: 30 },
  { name: "xlarge", files: 5000, dupeGroups: 300, linesPerBlock: 30 },
];

const DIRS = ["components", "utils", "hooks", "services", "types", "models", "helpers", "lib"];
const TYPES = ["string", "number", "boolean", "string[]", "Record<string, unknown>"];
const STATUSES = ["active", "inactive", "pending", "archived", "deleted"];
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
const ENTITIES = ["User", "Order", "Product", "Invoice", "Payment", "Session", "Account", "Report"];

/** Generate a realistic duplicated code block */
function generateCodeBlock(rand, blockId, linesPerBlock) {
  const entity = ENTITIES[Math.floor(rand() * ENTITIES.length)];
  const action = ACTIONS[Math.floor(rand() * ACTIONS.length)];
  const status = STATUSES[Math.floor(rand() * STATUSES.length)];
  const type = TYPES[Math.floor(rand() * TYPES.length)];

  const lines = [];
  lines.push(
    `export const ${action}${entity}Data = (input: ${type}): Record<string, unknown> => {`,
  );
  lines.push(`  const result: Record<string, unknown> = {};`);
  lines.push(`  const timestamp = Date.now();`);
  lines.push(`  const id = \`${entity.toLowerCase()}_\${timestamp}\`;`);
  lines.push(``);
  lines.push(`  if (!input) {`);
  lines.push(`    throw new Error('${entity} input is required for ${action}');`);
  lines.push(`  }`);
  lines.push(``);
  lines.push(`  result.id = id;`);
  lines.push(`  result.status = '${status}';`);
  lines.push(`  result.createdAt = new Date(timestamp).toISOString();`);
  lines.push(`  result.updatedAt = new Date(timestamp).toISOString();`);

  // Pad to reach target line count with realistic-looking processing logic
  const extraLines = linesPerBlock - lines.length - 3; // reserve 3 for closing
  for (let i = 0; i < extraLines; i++) {
    const fieldIdx = i % 6;
    switch (fieldIdx) {
      case 0:
        lines.push(
          `  result.field_${blockId}_${i} = String(input).slice(0, ${10 + Math.floor(rand() * 90)});`,
        );
        break;
      case 1:
        lines.push(`  result.computed_${i} = timestamp + ${Math.floor(rand() * 10000)};`);
        break;
      case 2:
        lines.push(`  result.flag_${i} = ${rand() > 0.5 ? "true" : "false"};`);
        break;
      case 3:
        lines.push(`  result.label_${i} = '${status}_${Math.floor(rand() * 1000)}';`);
        break;
      case 4:
        lines.push(`  result.count_${i} = Math.max(0, ${Math.floor(rand() * 100)});`);
        break;
      case 5:
        lines.push(`  result.hash_${i} = id + '_' + String(${Math.floor(rand() * 999)});`);
        break;
    }
  }

  lines.push(``);
  lines.push(`  return result;`);
  lines.push(`};`);

  return lines.join("\n");
}

/** Generate a unique (non-duplicated) code block */
function generateUniqueBlock(rand, fileId) {
  const entity = ENTITIES[Math.floor(rand() * ENTITIES.length)];
  const action = ACTIONS[Math.floor(rand() * ACTIONS.length)];
  const lines = [
    `export const unique_${action}_${fileId} = (value: string): string => {`,
    `  const key = '${entity.toLowerCase()}_${fileId}';`,
    `  return \`\${key}:\${value.trim()}\`;`,
    `};`,
  ];
  return lines.join("\n");
}

function generateProject(size) {
  const { name, files: fileCount, dupeGroups, linesPerBlock } = size;
  const projectDir = join(fixturesDir, name);
  if (existsSync(projectDir)) rmSync(projectDir, { recursive: true });
  const rand = mulberry32(42 + fileCount);
  const srcDir = join(projectDir, "src");
  for (const dir of DIRS) mkdirSync(join(srcDir, dir), { recursive: true });

  // Pre-generate duplicated code blocks
  const blocks = [];
  for (let g = 0; g < dupeGroups; g++) {
    blocks.push(generateCodeBlock(rand, g, linesPerBlock));
  }

  // Decide which files get duplicated blocks: ~40% of files get at least one dupe
  const dupeFileCount = Math.floor(fileCount * 0.4);
  const dupeAssignments = new Map(); // fileIndex -> [blockIndex, ...]

  // Each dupe group appears in 2-4 files
  for (let g = 0; g < dupeGroups; g++) {
    const instanceCount = 2 + Math.floor(rand() * 3); // 2–4 copies
    const usedFiles = new Set();
    for (let c = 0; c < instanceCount; c++) {
      let fileIdx;
      let attempts = 0;
      do {
        fileIdx = Math.floor(rand() * dupeFileCount);
        attempts++;
      } while (usedFiles.has(fileIdx) && attempts < 20);
      usedFiles.add(fileIdx);
      if (!dupeAssignments.has(fileIdx)) dupeAssignments.set(fileIdx, []);
      dupeAssignments.get(fileIdx).push(g);
    }
  }

  let totalLines = 0;
  let totalDuplicatedBlocks = 0;

  for (let i = 0; i < fileCount; i++) {
    const dir = DIRS[i % DIRS.length];
    const filePath = join(srcDir, dir, `module-${i}.ts`);
    const parts = [];

    // Add unique content first
    parts.push(generateUniqueBlock(rand, i));
    parts.push("");

    // If this file has dupe assignments, add those blocks
    const assignments = dupeAssignments.get(i) ?? [];
    for (const blockIdx of assignments) {
      parts.push(blocks[blockIdx]);
      parts.push("");
      totalDuplicatedBlocks++;
    }

    // Add some more unique filler content
    const extraFunctions = 1 + Math.floor(rand() * 2);
    for (let f = 0; f < extraFunctions; f++) {
      parts.push(generateUniqueBlock(rand, i * 100 + f));
      parts.push("");
    }

    const content = parts.join("\n");
    totalLines += content.split("\n").length;
    writeFileSync(filePath, content);
  }

  // Entry point
  const entryContent = [
    `// Entry point for ${name} dupes benchmark project`,
    `export const PROJECT_NAME = '${name}';`,
    "",
  ].join("\n");
  writeFileSync(join(srcDir, "index.ts"), entryContent);

  writeFileSync(
    join(projectDir, "package.json"),
    JSON.stringify(
      {
        name: `bench-dupes-${name}`,
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

  return { name, fileCount, dupeGroups, totalDuplicatedBlocks, totalLines };
}

console.log("Generating synthetic duplication benchmark fixtures...\n");
for (const size of SIZES) {
  const start = performance.now();
  const stats = generateProject(size);
  const elapsed = performance.now() - start;
  console.log(
    `  ${stats.name.padEnd(8)} ${String(stats.fileCount).padStart(5)} files  ${String(stats.dupeGroups).padStart(4)} clone groups  ${String(stats.totalDuplicatedBlocks).padStart(5)} dupe blocks  ${String(stats.totalLines).padStart(7)} lines  (${elapsed.toFixed(0)}ms)`,
  );
}
console.log("\nDone. Run: npm run bench:dupes:synthetic");
