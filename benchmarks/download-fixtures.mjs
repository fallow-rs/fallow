#!/usr/bin/env node
import { existsSync, mkdirSync, readdirSync, statSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { execFileSync } from "node:child_process";

const __dirname = dirname(fileURLToPath(import.meta.url));
const fixturesDir = join(__dirname, "fixtures", "real-world");

const FIXTURES = [
  // Small projects (< 300 source files)
  { name: "preact", repo: "https://github.com/preactjs/preact.git", tag: "10.25.4" },
  { name: "fastify", repo: "https://github.com/fastify/fastify.git", tag: "v5.2.1" },
  { name: "zod", repo: "https://github.com/colinhacks/zod.git", tag: "v3.24.2" },
  // Large monorepo projects (1000+ source files)
  { name: "vue-core", repo: "https://github.com/vuejs/core.git", tag: "v3.5.30", pm: "pnpm" },
  {
    name: "svelte",
    repo: "https://github.com/sveltejs/svelte.git",
    tag: "svelte@5.54.1",
    pm: "pnpm",
  },
  { name: "query", repo: "https://github.com/TanStack/query.git", tag: "v5.90.3", pm: "pnpm" },
  { name: "vite", repo: "https://github.com/vitejs/vite.git", tag: "v8.0.1", pm: "pnpm" },
  // XL monorepo (10,000+ source files)
  { name: "next.js", repo: "https://github.com/vercel/next.js.git", tag: "v16.2.1", pm: "pnpm" },
  { name: "astro", repo: "https://github.com/withastro/astro.git", tag: "astro@6.3.1", pm: "pnpm" },
  { name: "typescript", repo: "https://github.com/microsoft/TypeScript.git", tag: "v5.9.3" },
];

function countSourceFiles(dir) {
  let count = 0;
  const walk = (d) => {
    try {
      for (const entry of readdirSync(d)) {
        if (["node_modules", ".git", "dist"].includes(entry)) continue;
        const full = join(d, entry);
        try {
          const stat = statSync(full);
          if (stat.isDirectory()) walk(full);
          else if (/\.(ts|tsx|js|jsx|mjs|cjs)$/.test(entry)) count++;
        } catch {
          /* skip */
        }
      }
    } catch {
      /* skip */
    }
  };
  walk(dir);
  return count;
}

if (!existsSync(fixturesDir)) mkdirSync(fixturesDir, { recursive: true });

console.log("Downloading real-world projects for benchmarking...\n");
let allOk = true;

for (const fixture of FIXTURES) {
  const dest = join(fixturesDir, fixture.name);
  if (existsSync(dest)) {
    console.log(`  ${fixture.name}: already exists, skipping`);
    continue;
  }

  console.log(`  ${fixture.name}: cloning ${fixture.repo} @ ${fixture.tag}...`);
  try {
    execFileSync("git", ["clone", "--depth", "1", "--branch", fixture.tag, fixture.repo, dest], {
      stdio: "pipe",
      timeout: 120_000,
    });
    console.log(`  ${fixture.name}: installing dependencies...`);
    if (fixture.pm === "pnpm") {
      execFileSync("pnpm", ["install", "--no-frozen-lockfile", "--ignore-scripts"], {
        cwd: dest,
        stdio: "pipe",
        timeout: 300_000,
      });
    } else {
      execFileSync("npm", ["install", "--ignore-scripts", "--no-audit", "--no-fund"], {
        cwd: dest,
        stdio: "pipe",
        timeout: 300_000,
      });
    }
    console.log(`  ${fixture.name}: ready (${countSourceFiles(dest)} source files)`);
  } catch (err) {
    console.error(`  ${fixture.name}: FAILED - ${err.message}`);
    allOk = false;
  }
}

console.log();
if (allOk) console.log("All fixtures ready! Run: npm run bench:real-world");
else {
  console.error("Some downloads failed.");
  process.exit(1);
}
