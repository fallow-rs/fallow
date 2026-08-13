#!/usr/bin/env node

import { assertTypescriptBackendResolvable } from "./src/backend-preflight.mjs";

try {
  assertTypescriptBackendResolvable();
  const { run } = await import("./src/cli.mjs");
  await run({ input: process.stdin, output: process.stdout, args: process.argv.slice(2) });
} catch (error) {
  const message = error instanceof Error ? error.message : String(error);
  process.stderr.write(`fallow-type-aware: ${message}\n`);
  process.exitCode = 2;
}
