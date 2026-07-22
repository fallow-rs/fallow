#!/usr/bin/env node

import { run } from "./src/cli.mjs";

try {
  await run({ input: process.stdin, output: process.stdout });
} catch (error) {
  const message = error instanceof Error ? error.message : String(error);
  process.stderr.write(`fallow-type-aware: ${message}\n`);
  process.exitCode = 2;
}
