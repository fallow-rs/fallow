#!/usr/bin/env node
/**
 * Re-record the coverage producer corpus from the pinned producers.
 *
 * It runs the four pinned JavaScript coverage producers, records what each one
 * emits for every probe, and rewrites the corpus provenance. No map in the
 * corpus is ever hand-written: if it is not machine-recorded from a pinned
 * producer, it does not belong here.
 *
 *   node scripts/coverage-producer-refresh.mjs --check   compare, never write
 *   node scripts/coverage-producer-refresh.mjs           re-record
 *
 * Both modes report the same drift, and both exit non-zero on the one kind of
 * drift that costs the corpus evidence: a producer that stops emitting a map
 * for a probe. Writing that away would shrink the matrix, and a smaller matrix
 * passes greener. Removing a row is a manifest edit a human makes on purpose.
 *
 * The census is NOT rewritten to match observed behavior. A row that already
 * has a census keeps it: the harness prints the delta and stops. Rewriting an
 * expectation to whatever the code happens to do rebuilds the exact failure
 * this corpus exists to prevent. Only a genuinely new row is seeded, and a
 * seeded unattributed unit lands with a null `rationale`, which the manifest
 * loader rejects until a maintainer writes down why that producer emits no
 * record.
 *
 * Prerequisite:
 *   npm ci --prefix tests/coverage-producer-corpus/producers \
 *     --no-audit --no-fund --ignore-scripts
 */

import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { assertLocalResolution } from "./assert-local-resolution.mjs";
import {
  CORPUS_CONFIG_FILE,
  buildCensus,
  compareCensus,
  equivalenceClasses,
  sha256,
} from "./coverage-producer-conformance.mjs";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const CORPUS_ROOT = resolve(REPO_ROOT, "tests/coverage-producer-corpus");
const MANIFEST = join(CORPUS_ROOT, "manifest.json");
const PRODUCERS_ROOT = join(CORPUS_ROOT, "producers");
const DEFAULT_FALLOW_BIN = resolve(REPO_ROOT, "target/debug/fallow");

const INSTALL_COMMAND =
  "npm ci --prefix tests/coverage-producer-corpus/producers --no-audit --no-fund --ignore-scripts";

const fail = (message) => {
  throw new Error(message);
};

export const parseArgs = (argv) => {
  const options = { check: false, fallowBin: process.env.FALLOW_BIN ?? DEFAULT_FALLOW_BIN };
  for (const argument of argv) {
    if (argument === "--check") {
      options.check = true;
    } else {
      fail(`unknown argument: ${argument}`);
    }
  }
  return options;
};

/**
 * Refuse to record against an install this checkout does not own.
 *
 * Node walks ancestor `node_modules` directories, so a worktree nested inside
 * another checkout would silently record against the outer install's producer
 * versions while the manifest claims the pinned ones.
 */
const assertPinnedProducers = (packages) => {
  for (const dependency of packages) {
    assertLocalResolution({
      dependency,
      installCommand: INSTALL_COMMAND,
      repoRoot: PRODUCERS_ROOT,
      resolveFrom: join(PRODUCERS_ROOT, "package.json"),
    });
  }
};

/** Serialized form every recorded map is committed in. */
export const serializeMap = (map) => `${JSON.stringify(map, null, 2)}\n`;

/**
 * Rebase a producer's `FileCoverage` onto the corpus virtual path.
 *
 * Every recorded map is keyed by `/fallow/corpus/<probe>` and the harness runs
 * with `--coverage-root /fallow/corpus`, which is the same rebasing a real
 * CI-recorded map needs. Recording an absolute local path would make the
 * corpus machine-specific.
 */
export const normalizeMap = (fileCoverage, virtualPath) => ({
  [virtualPath]: { ...fileCoverage, path: virtualPath },
});

const recordRow = async (producer, fixture) => {
  const driver = await import(pathToFileURL(resolve(CORPUS_ROOT, producer.driver)).href);
  const sourcePath = resolve(CORPUS_ROOT, fixture.file);
  const source = readFileSync(sourcePath, "utf8");
  const fileCoverage = await driver.emit({
    filename: fixture.virtual_path,
    source,
    sourcePath,
    typescript: fixture.file.endsWith(".ts"),
  });
  return fileCoverage === null ? null : normalizeMap(fileCoverage, fixture.virtual_path);
};

const invokeFallow = (binary, root, coveragePath, coverageRoot) => {
  const result = spawnSync(
    binary,
    [
      "health",
      "--root",
      root,
      // Same corpus-owned config the gate uses. Without it Fallow walks up into
      // this repository's own `.fallowrc.json` and the census stops being a
      // statement about the matcher.
      "--config",
      join(CORPUS_ROOT, CORPUS_CONFIG_FILE),
      "--complexity",
      "--coverage",
      coveragePath,
      "--coverage-root",
      coverageRoot,
      "--max-cyclomatic",
      "9999",
      "--max-cognitive",
      "9999",
      "--max-crap",
      "1",
      "--format",
      "json",
      "--quiet",
      "--no-cache",
    ],
    { encoding: "utf8", maxBuffer: 32 * 1024 * 1024, timeout: 60_000 },
  );
  if (result.status !== 0 && result.status !== 1) {
    fail(`fallow exited ${result.status}: ${(result.stderr ?? "").trim()}`);
  }
  return JSON.parse(result.stdout);
};

const seedCensusUnit = (unit) =>
  unit.coverage_source === "istanbul"
    ? {
        name: unit.name,
        line: unit.line,
        col: unit.col,
        coverage_source: "istanbul",
        coverage_pct: unit.coverage_pct,
      }
    : {
        name: unit.name,
        line: unit.line,
        col: unit.col,
        coverage_source: "estimated",
        rationale: null,
      };

export const main = async (
  argv = process.argv.slice(2),
  write = (text) => process.stdout.write(text),
) => {
  const options = parseArgs(argv);
  const manifest = JSON.parse(readFileSync(MANIFEST, "utf8"));
  const producerPackages = [...new Set(manifest.producers.map((producer) => producer.package))];
  assertPinnedProducers(producerPackages);
  if (!existsSync(join(CORPUS_ROOT, CORPUS_CONFIG_FILE))) {
    fail(`the corpus config ${CORPUS_CONFIG_FILE} is missing; the census would not be comparable`);
  }

  // `node_pin` is provenance, not a gate, and both modes treat it the same
  // way. Two rows derive their record set from V8 ScriptCoverage, which is a
  // property of the Node version rather than of the producer version, so a
  // different Node is worth naming when geometry differs. It is not worth
  // refusing over: the recorded maps are byte-identical on Node 22.21.1,
  // 22.23.2, 24.18.0 and 26.7.0, and the byte comparison below is what
  // actually detects a V8 change, with a better message than a version
  // mismatch could give.
  const nodeMoved = process.version !== manifest.recorded.node_pin;
  if (nodeMoved) {
    process.stderr.write(
      `[coverage-producers] the corpus was recorded on Node ${manifest.recorded.node_pin} and ` +
        `this is ${process.version}. v8-to-istanbul and ast-v8-to-istanbul derive their record ` +
        "set from V8 ScriptCoverage, so treat a geometry difference below as possibly a Node " +
        `difference until it reproduces on ${manifest.recorded.node_pin}.\n`,
    );
  }

  const recorded = new Map();
  const drift = [];

  for (const producer of manifest.producers) {
    for (const fixture of manifest.fixtures) {
      const key = `${producer.id}/${fixture.id}`;
      const map = await recordRow(producer, fixture);
      if (map === null) {
        recorded.set(key, null);
        continue;
      }
      recorded.set(key, serializeMap(map));
    }
  }

  const declared = new Set(manifest.maps.map((entry) => `${entry.producer}/${entry.fixture}`));
  const vanished = [];
  for (const [key, contents] of recorded) {
    const [producerId, fixtureId] = key.split("/");
    const file = `maps/${producerId}/${fixtureId}.json`;
    const absolute = join(CORPUS_ROOT, file);
    if (contents === null) {
      if (declared.has(key)) {
        vanished.push(key);
        drift.push(`${key}: producer no longer emits a map for this probe`);
      }
      continue;
    }
    if (!declared.has(key)) {
      drift.push(`${key}: producer now emits a map this corpus does not record`);
      continue;
    }
    const committed = existsSync(absolute) ? readFileSync(absolute, "utf8") : null;
    if (committed !== contents) {
      drift.push(`${key}: recorded geometry differs from the committed map`);
    }
  }

  for (const line of drift) {
    process.stderr.write(`[coverage-producers] ${line}\n`);
  }

  // A row that lost its map is evidence the corpus used to hold. Re-recording
  // over it would drop the row from `maps[]` and from the census, orphan the
  // file on disk, and print a smaller matrix that passes greener than the one
  // it replaced. Both modes refuse, and retiring a row stays a manifest edit a
  // human makes on purpose.
  if (vanished.length > 0) {
    process.stderr.write(
      `[coverage-producers] rows that lost their map: ${vanished.join(", ")}. Refusing to ` +
        "re-record: this would retire the row, and the invariants it carries, with a green " +
        "build. Check the producer bump first. If the producer genuinely stopped covering the " +
        "probe, delete its maps[] entry, its census row and its file under maps/ by hand, and " +
        "say so in the commit.\n",
    );
    return 1;
  }

  if (options.check) {
    if (drift.length > 0) {
      process.stderr.write(
        "[coverage-producers] The pinned producers no longer emit the recorded geometry. " +
          "Run npm run refresh:coverage-producers, review the map diff, and land it when the " +
          "census is unchanged.\n",
      );
      return 1;
    }
    write("coverage producer maps match the pinned producers\n");
    return 0;
  }

  for (const [key, contents] of recorded) {
    if (contents === null) {
      continue;
    }
    const [producerId, fixtureId] = key.split("/");
    mkdirSync(join(CORPUS_ROOT, "maps", producerId), { recursive: true });
    writeFileSync(join(CORPUS_ROOT, `maps/${producerId}/${fixtureId}.json`), contents);
  }

  if (nodeMoved) {
    write(
      `node_pin ${manifest.recorded.node_pin} -> ${process.version}: the corpus is now recorded ` +
        "on this Node. Land that manifest line deliberately, or re-record on the pinned Node.\n",
    );
  }
  manifest.recorded.node_pin = process.version;
  manifest.recorded.platform = `${process.platform}-${process.arch}`;
  manifest.recorded.producers_lock_sha256 = sha256(
    readFileSync(join(CORPUS_ROOT, manifest.recorded.producers_lock_file)),
  );
  for (const fixture of manifest.fixtures) {
    fixture.sha256 = sha256(readFileSync(resolve(CORPUS_ROOT, fixture.file)));
  }
  manifest.maps = [...recorded.entries()]
    .filter(([, contents]) => contents !== null)
    .map(([key]) => {
      const [producer, fixture] = key.split("/");
      const file = `maps/${producer}/${fixture}.json`;
      return { producer, fixture, file, sha256: sha256(readFileSync(join(CORPUS_ROOT, file))) };
    });

  // A map file no manifest row references is a map nothing checks. That can
  // only come from a row a maintainer retired by hand, so removing the file is
  // the second half of their edit rather than a decision this tool makes.
  const referenced = new Set(manifest.maps.map((entry) => entry.file));
  for (const directory of readdirSync(join(CORPUS_ROOT, "maps"), { withFileTypes: true })) {
    if (!directory.isDirectory()) {
      continue;
    }
    for (const name of readdirSync(join(CORPUS_ROOT, "maps", directory.name))) {
      const file = `maps/${directory.name}/${name}`;
      if (!referenced.has(file)) {
        rmSync(join(CORPUS_ROOT, file));
        write(`removed ${file}: no manifest row references it\n`);
      }
    }
  }

  const rows = [];
  const deltas = [];
  const seeded = [];
  const binaryPresent = existsSync(options.fallowBin);
  if (binaryPresent) {
    const existing = new Map(
      (manifest.census ?? []).map((row) => [`${row.producer}/${row.fixture}`, row]),
    );
    const census = [];
    for (const entry of manifest.maps) {
      const key = `${entry.producer}/${entry.fixture}`;
      const fixture = manifest.fixtures.find((candidate) => candidate.id === entry.fixture);
      const observed = buildCensus(
        invokeFallow(
          options.fallowBin,
          resolve(CORPUS_ROOT, dirname(fixture.file)),
          join(CORPUS_ROOT, entry.file),
          manifest.recorded.coverage_root,
        ),
      );
      rows.push({
        fixture: entry.fixture,
        id: key,
        producer: entry.producer,
        resolved: observed.filter((unit) => unit.coverage_source === "istanbul").length,
        units: observed,
      });
      const previous = existing.get(key);
      if (previous === undefined) {
        census.push({
          producer: entry.producer,
          fixture: entry.fixture,
          units: observed.map(seedCensusUnit),
        });
        seeded.push(`${key}: seeded a new census row`);
        continue;
      }
      census.push(previous);
      for (const delta of compareCensus(previous.units, observed)) {
        deltas.push(`${key}: ${delta}`);
      }
    }
    manifest.census = census;
  }

  writeFileSync(MANIFEST, `${JSON.stringify(manifest, null, 2)}\n`);

  write(`re-recorded ${manifest.maps.length} producer maps\n`);
  if (!binaryPresent) {
    write(
      `no fallow binary at ${options.fallowBin}; skipped the census delta report. ` +
        "Build it and run npm run check:coverage-producers.\n",
    );
    return 0;
  }
  for (const entry of equivalenceClasses(rows)) {
    write(
      `${entry.resolved}/${entry.units} units resolved across ${entry.fixtures} probes: ` +
        `${entry.members.join(", ")}\n`,
    );
  }
  for (const line of seeded) {
    write(`${line}\n`);
  }
  if (deltas.length === 0) {
    write(
      drift.length === 0
        ? "no drift: the pinned producers still emit the committed geometry\n"
        : "census unchanged: geometry moved, the matcher absorbed it, the map diff IS the change\n",
    );
    return 0;
  }
  for (const delta of deltas) {
    write(`${delta}\n`);
  }
  write(
    "A census regression is a matcher bug. Fix crates/engine/src/health/scoring.rs and refresh " +
      "again; edit the census by hand only when a producer genuinely stopped emitting a record.\n",
  );
  return 1;
};

const isMain =
  process.argv[1] !== undefined && resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (isMain) {
  try {
    process.exitCode = await main();
  } catch (error) {
    process.stderr.write(`${error.message}\n`);
    process.exitCode = 1;
  }
}
