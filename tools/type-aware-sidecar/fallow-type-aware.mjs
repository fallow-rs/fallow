#!/usr/bin/env node

import { installWindowsChildProcessPolicy } from "./src/windows-child-process.mjs";

installWindowsChildProcessPolicy();

try {
  const { assertTypescriptBackendResolvable } = await import("./src/backend-preflight.mjs");
  assertTypescriptBackendResolvable();
  const { run } = await import("./src/cli.mjs");
  await run({ input: process.stdin, output: process.stdout, args: process.argv.slice(2) });
} catch (error) {
  const message = error instanceof Error ? error.message : String(error);
  process.stderr.write(`fallow-type-aware: ${message}\n`);
  process.exitCode = 2;
}
