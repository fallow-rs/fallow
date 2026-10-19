#!/usr/bin/env node
// Fail when an `[[IgnoredVulns]]` id in a tracked osv-scanner.toml no longer
// matches an advisory in the lockfiles next to that file. A stale ignore hides
// nothing, but scripts/generate-vex.mjs turns it into a wrong `not_affected`
// statement in .vex/openvex.json.
//
// The scan uses an empty config file, because `--no-ignore` does not bypass
// the IgnoredVulns entries. `PackageOverrides` entries are out of scope.
import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { pathToFileURL } from "node:url";

export const LOCKFILE_NAMES = [
  "Cargo.lock",
  "bun.lock",
  "package-lock.json",
  "pnpm-lock.yaml",
  "yarn.lock",
];

// osv-scanner exits 0 when it finds no advisory and 1 when it finds one.
const SCAN_OK_EXIT_CODES = new Set([0, 1]);

/** Return the `id` of each `[[IgnoredVulns]]` entry in an osv-scanner.toml. */
export const parseIgnoredIds = (toml) =>
  toml
    .split(/^\[\[IgnoredVulns\]\]\s*$/mu)
    .slice(1)
    .map((block) => /^id\s*=\s*"([^"]+)"/mu.exec(block)?.[1])
    .filter((id) => id !== undefined);

const entryIds = (entry) => [entry.id, ...(entry.ids ?? []), ...(entry.aliases ?? [])];

/** Return every advisory id and alias in an osv-scanner JSON report. */
export const collectAdvisoryIds = (report) =>
  new Set(
    (report.results ?? [])
      .flatMap((result) => result.packages ?? [])
      .flatMap((pkg) => [...(pkg.vulnerabilities ?? []), ...(pkg.groups ?? [])])
      .flatMap(entryIds)
      .filter((id) => id !== undefined),
  );

/** Return the ignore ids that match no advisory in the given reports. */
export const findStaleIgnores = (ignoredIds, reports) => {
  const found = new Set();
  for (const report of reports) {
    for (const id of collectAdvisoryIds(report)) found.add(id);
  }
  return ignoredIds.filter((id) => !found.has(id));
};

const scanLockfile = (lockfile, emptyConfig) => {
  const run = spawnSync(
    "osv-scanner",
    ["scan", "source", "--config", emptyConfig, "--format", "json", "-L", lockfile],
    { encoding: "utf8", maxBuffer: 256 * 1024 * 1024 },
  );
  if (run.error) throw new Error(`osv-scanner did not start: ${run.error.message}`);
  if (!SCAN_OK_EXIT_CODES.has(run.status)) {
    throw new Error(`osv-scanner failed on ${lockfile} (exit ${run.status}):\n${run.stderr}`);
  }
  return JSON.parse(run.stdout);
};

const main = () => {
  const tomlFiles = execFileSync(
    "git",
    ["ls-files", "--", "**/osv-scanner.toml", "osv-scanner.toml"],
    {
      encoding: "utf8",
    },
  )
    .split("\n")
    .filter(Boolean)
    .toSorted();

  const tempDir = mkdtempSync(join(tmpdir(), "check-osv-ignores-"));
  const emptyConfig = join(tempDir, "osv-scanner.toml");
  writeFileSync(emptyConfig, "");

  const problems = [];
  try {
    for (const file of tomlFiles) {
      const ignoredIds = parseIgnoredIds(readFileSync(file, "utf8"));
      if (ignoredIds.length === 0) continue;
      const lockfiles = LOCKFILE_NAMES.map((name) => join(dirname(file), name)).filter((path) =>
        existsSync(path),
      );
      if (lockfiles.length === 0) {
        problems.push(`${file}: no lockfile next to this file, so its ignores apply to nothing`);
        continue;
      }
      const reports = lockfiles.map((lockfile) => scanLockfile(lockfile, emptyConfig));
      for (const id of findStaleIgnores(ignoredIds, reports)) {
        problems.push(`${file}: ${id} matches no advisory in ${lockfiles.join(", ")}`);
      }
    }
  } finally {
    rmSync(tempDir, { recursive: true, force: true });
  }

  if (problems.length > 0) {
    console.error("Stale osv-scanner ignores:");
    for (const problem of problems) console.error(`  ${problem}`);
    console.error("Remove each stale entry, then run: node scripts/generate-vex.mjs");
    process.exit(1);
  }
  console.log(`All IgnoredVulns entries match an advisory (${tomlFiles.length} files checked).`);
};

if (import.meta.url === pathToFileURL(process.argv[1]).href) main();
