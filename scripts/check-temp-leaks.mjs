#!/usr/bin/env node
// Runs a command and fails when the command leaves new `fallow-*` entries in
// the system temp directory. Test suites must remove their own temp folders,
// because a developer machine and a reused CI runner keep them.
//
// Usage: node scripts/check-temp-leaks.mjs -- <command> [args...]
//
// The exit code is the exit code of the command when it fails. When the
// command passes and new entries exist, the exit code is 1.

import { spawnSync } from "node:child_process";
import { readdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const PREFIX = "fallow-";

// These prefixes are designed to stay: the audit base cache has its own age
// garbage collection, and the mutation companion folders have their own
// cleanup.
const IGNORED_PREFIXES = ["fallow-audit-base-cache-", "fallow-mt-"];

/** Returns true when the temp entry name belongs to the guard. */
export const isTrackedEntry = (name) =>
  name.startsWith(PREFIX) && !IGNORED_PREFIXES.some((ignored) => name.startsWith(ignored));

/** Returns the set of tracked entry names in `dir`. */
export const snapshotTempEntries = (dir = tmpdir()) => {
  const names = readdirSync(dir).filter(isTrackedEntry);
  return new Set(names);
};

/** Returns the sorted names that are in `after` and not in `before`. */
export const newEntries = (before, after) =>
  [...after].filter((name) => !before.has(name)).toSorted();

const parseCommand = (argv) => {
  const separator = argv.indexOf("--");
  const command = separator === -1 ? argv : argv.slice(separator + 1);
  return command.length === 0 ? null : command;
};

const reportLeaks = (dir, leaked) => {
  console.error(`check-temp-leaks: the command left ${leaked.length} new entries in ${dir}:`);
  for (const name of leaked) console.error(`  ${join(dir, name)}`);
  console.error(
    "Remove each temp folder in the test that creates it, for example with makeTempDir() of scripts/lib/temp-dir.mjs.",
  );
};

// A failing command keeps its own exit code, so CI shows the real failure.
const exitCode = (status, leaked) => {
  if (status !== 0) return status ?? 1;
  return leaked.length > 0 ? 1 : 0;
};

const main = () => {
  const command = parseCommand(process.argv.slice(2));
  if (command === null) {
    console.error("usage: node scripts/check-temp-leaks.mjs -- <command> [args...]");
    return 2;
  }

  const dir = tmpdir();
  const before = snapshotTempEntries(dir);
  const result = spawnSync(command[0], command.slice(1), {
    stdio: "inherit",
    shell: process.platform === "win32",
  });
  if (result.error) {
    console.error(`check-temp-leaks: cannot run ${command[0]}: ${result.error.message}`);
    return 2;
  }
  const leaked = newEntries(before, snapshotTempEntries(dir));
  if (leaked.length > 0) reportLeaks(dir, leaked);
  return exitCode(result.status, leaked);
};

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  process.exitCode = main();
}
