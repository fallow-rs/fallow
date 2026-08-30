#!/usr/bin/env node
/**
 * Assert that Fallow still resolves real coverage-producer geometry.
 *
 * `crates/engine/src/health/scoring.rs` maps every function Fallow extracts
 * onto a record in an Istanbul coverage map. The matcher rests on WHERE each
 * producer anchors a function record, and every geometry expectation in the
 * repository is otherwise a hand-written JSON literal inside a Rust test, so
 * nothing notices when a producer moves an anchor in a minor release.
 *
 * This harness closes that gap with committed, machine-recorded maps:
 *
 *   Layer 1  Behavioral census. Per (producer, fixture) it runs the real
 *            binary against the committed map and asserts the sorted
 *            `{name, line, col, coverage_source, coverage_pct}` list equals the
 *            manifest census. Coverage percentages are per-function
 *            fingerprints, so a match proves WHICH record resolved.
 *   Layer 2  Geometry. Not asserted. The committed map is the review artifact:
 *            a producer that moves an anchor the matcher absorbs shows up as a
 *            map diff with an unchanged census, which reviews at a glance. The
 *            map bytes are pinned by sha256 in the manifest, so a map cannot be
 *            edited without either the digest check firing or the edit landing
 *            in the diff.
 *   Layer 3  Self-test. After the census passes, every row that resolves at
 *            least one unit is perturbed in memory and the census must FAIL:
 *            once with a record moved past `ALIAS_FUZZ_MAX_LINE_DRIFT`, once
 *            with every column in the file moved past the end of its own line.
 *            A row that resolves nothing is skipped, because moving a record
 *            no census reads proves nothing. A third perturbation collapses
 *            every column to the line start and is RECORDED rather than
 *            asserted: the matcher tolerates bounded column drift on purpose,
 *            so most rows survive a collapse, and demanding otherwise would
 *            fail the harness for the tolerance working as designed.
 *
 * Failure always exits non-zero. There is no flag that downgrades a census
 * delta to a report, because the gate is one npm script and a flag is one
 * token to drop.
 *
 * Run: `npm run check:coverage-producers`
 */

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const DEFAULT_MANIFEST = resolve(REPO_ROOT, "tests/coverage-producer-corpus/manifest.json");
const DEFAULT_FALLOW_BIN = resolve(REPO_ROOT, "target/debug/fallow");

/**
 * Config the census runs under, committed next to the manifest.
 *
 * Fallow walks up from `--root` looking for a config, and every fixture
 * directory sits inside this repository, so without an explicit config the
 * census would inherit the repository's own `.fallowrc.json`. That file has
 * nothing to do with producers or the matcher, and a PR that only edits it
 * would move a census this harness then blames on
 * `crates/engine/src/health/scoring.rs`.
 */
export const CORPUS_CONFIG_FILE = ".fallowrc.jsonc";

export const EXPECTED_SCHEMA = "fallow-coverage-producer-conformance/v1";

/**
 * Mirror of `ALIAS_FUZZ_MAX_LINE_DRIFT` in
 * `crates/engine/src/health/scoring.rs`. The self-test has to move a record
 * strictly further than the matcher's fuzz window, or a passing perturbed run
 * would prove nothing.
 */
export const ALIAS_FUZZ_MAX_LINE_DRIFT = 2;

/** Lines the self-test shifts a record by, one past the matcher's window. */
export const PERTURBATION_LINE_SHIFT = ALIAS_FUZZ_MAX_LINE_DRIFT + 1;

const COVERAGE_SOURCES = new Set(["istanbul", "estimated"]);

/**
 * Lowest cyclomatic complexity a probe function may have.
 *
 * A cc=1 function that is fully covered scores CRAP exactly 1.0, which is the
 * `--max-crap 1` threshold itself, so whether it reaches `findings[]` rests on
 * a floating-point equality. Every probe stays strictly above that boundary.
 */
export const MIN_PROBE_CYCLOMATIC = 2;

const fail = (message) => {
  throw new Error(message);
};

export const sha256 = (contents) => createHash("sha256").update(contents).digest("hex");

/**
 * Parse the harness arguments.
 *
 * No argument changes the verdict. `--pretty` indents the report, the other
 * two point the run at a different binary or manifest, and a census delta
 * exits non-zero under every combination of them.
 */
export const parseArgs = (argv) => {
  const options = {
    fallowBin: process.env.FALLOW_BIN ?? DEFAULT_FALLOW_BIN,
    manifest: DEFAULT_MANIFEST,
    pretty: false,
  };

  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === "--pretty") {
      options.pretty = true;
    } else if (argument === "--fallow-bin") {
      options.fallowBin = argv[index + 1] ?? fail("--fallow-bin requires a path");
      index += 1;
    } else if (argument === "--manifest") {
      options.manifest = argv[index + 1] ?? fail("--manifest requires a path");
      index += 1;
    } else {
      fail(`unknown argument: ${argument}`);
    }
  }

  return options;
};

const assertNonEmptyString = (value, path) => {
  if (typeof value !== "string" || value.trim().length === 0) {
    fail(`${path} must be a non-empty string`);
  }
};

const assertBoolean = (value, path) => {
  if (typeof value !== "boolean") {
    fail(`${path} must be a boolean`);
  }
};

const loadInvariants = (manifest) => {
  if (!Array.isArray(manifest.invariants) || manifest.invariants.length === 0) {
    fail("manifest.invariants must name the matcher mechanisms under test");
  }
  const invariants = new Map();
  for (const [index, invariant] of manifest.invariants.entries()) {
    const path = `invariants[${index}]`;
    assertNonEmptyString(invariant.id, `${path}.id`);
    assertNonEmptyString(invariant.rust_fn, `${path}.rust_fn`);
    assertNonEmptyString(invariant.mechanism, `${path}.mechanism`);
    if (invariants.has(invariant.id)) {
      fail(`duplicate invariant id: ${invariant.id}`);
    }
    invariants.set(invariant.id, invariant);
  }
  return invariants;
};

const loadProducers = (manifest, root) => {
  if (!Array.isArray(manifest.producers) || manifest.producers.length < 2) {
    fail("manifest.producers must describe at least two rows");
  }
  const producers = new Map();
  for (const [index, producer] of manifest.producers.entries()) {
    const path = `producers[${index}]`;
    assertNonEmptyString(producer.id, `${path}.id`);
    assertNonEmptyString(producer.package, `${path}.package`);
    assertNonEmptyString(producer.version, `${path}.version`);
    assertNonEmptyString(producer.profile, `${path}.profile`);
    assertNonEmptyString(producer.driver, `${path}.driver`);
    assertNonEmptyString(producer.consumers, `${path}.consumers`);
    assertBoolean(producer.self_conformance, `${path}.self_conformance`);
    assertBoolean(producer.derives_from_v8, `${path}.derives_from_v8`);
    if (producers.has(producer.id)) {
      fail(`duplicate producer id: ${producer.id}`);
    }
    if (!existsSync(resolve(root, producer.driver))) {
      fail(`${path}.driver does not exist: ${producer.driver}`);
    }
    producers.set(producer.id, producer);
  }
  return producers;
};

const loadFixtures = (manifest, root, invariants) => {
  if (!Array.isArray(manifest.fixtures) || manifest.fixtures.length === 0) {
    fail("manifest.fixtures must describe at least one probe");
  }
  const fixtures = new Map();
  for (const [index, fixture] of manifest.fixtures.entries()) {
    const path = `fixtures[${index}]`;
    assertNonEmptyString(fixture.id, `${path}.id`);
    assertNonEmptyString(fixture.file, `${path}.file`);
    assertNonEmptyString(fixture.virtual_path, `${path}.virtual_path`);
    assertNonEmptyString(fixture.divergence, `${path}.divergence`);
    if (fixtures.has(fixture.id)) {
      fail(`duplicate fixture id: ${fixture.id}`);
    }
    if (!fixture.virtual_path.startsWith(`${manifest.recorded.coverage_root}/`)) {
      fail(`${path}.virtual_path must sit under ${manifest.recorded.coverage_root}`);
    }
    const contents = readFileSync(resolve(root, fixture.file));
    if (sha256(contents) !== fixture.sha256) {
      fail(`${fixture.file} digest mismatch; re-run npm run refresh:coverage-producers`);
    }
    if (contents.includes(0x0d)) {
      fail(`${fixture.file} contains CR; a CRLF checkout shifts every producer column`);
    }
    if (!Array.isArray(fixture.invariants) || fixture.invariants.length === 0) {
      fail(`${path}.invariants must name the mechanisms this probe defends`);
    }
    for (const invariant of fixture.invariants) {
      if (!invariants.has(invariant)) {
        fail(`${path} names an unknown invariant: ${invariant}`);
      }
    }
    // The self-test's column perturbation is measured against the probe: it
    // moves every column past the end of the longest line in this source.
    fixtures.set(fixture.id, { ...fixture, source: contents.toString("utf8") });
  }
  return fixtures;
};

const loadCensusUnits = (row, path) => {
  if (!Array.isArray(row.units) || row.units.length === 0) {
    fail(`${path}.units must list every unit Fallow reports`);
  }
  return row.units.map((unit, index) => {
    const unitPath = `${path}.units[${index}]`;
    assertNonEmptyString(unit.name, `${unitPath}.name`);
    if (!Number.isSafeInteger(unit.line) || unit.line < 1) {
      fail(`${unitPath}.line must be a 1-based line`);
    }
    if (!Number.isSafeInteger(unit.col) || unit.col < 0) {
      fail(`${unitPath}.col must be a 0-based column`);
    }
    if (!COVERAGE_SOURCES.has(unit.coverage_source)) {
      fail(`${unitPath}.coverage_source must be istanbul or estimated`);
    }
    if (unit.coverage_source === "istanbul") {
      if (!Number.isFinite(unit.coverage_pct)) {
        fail(`${unitPath}.coverage_pct must be the fingerprint the record carries`);
      }
      if (unit.rationale !== undefined) {
        fail(`${unitPath}.rationale belongs on unattributed units only`);
      }
    } else {
      if (unit.coverage_pct !== undefined) {
        fail(`${unitPath}.coverage_pct must be absent for an unattributed unit`);
      }
      if (typeof unit.rationale !== "string" || unit.rationale.trim().length === 0) {
        fail(
          `${unitPath}.rationale must be hand-written: name the producer version and why it ` +
            "emits no record Fallow can resolve for this unit",
        );
      }
    }
    return unit;
  });
};

/**
 * Sortable identity of one census unit, so a recorded census and an observed
 * one compare independently of report ordering.
 */
export const censusKey = (unit) => `${String(unit.line).padStart(6, "0")}:${unit.col}:${unit.name}`;

export const sortCensus = (units) =>
  units.toSorted((left, right) => censusKey(left).localeCompare(censusKey(right)));

/** Canonical string form of one census, used for equivalence-class grouping. */
export const censusSignature = (units) =>
  sortCensus(units)
    .map(
      (unit) =>
        `${censusKey(unit)}|${unit.coverage_source}|${unit.coverage_pct === undefined ? "-" : unit.coverage_pct}`,
    )
    .join("\n");

const loadFingerprintCollisions = (fixture, path) => {
  const declared = fixture.fingerprint_collisions ?? [];
  if (!Array.isArray(declared)) {
    fail(`${path}.fingerprint_collisions must be an array`);
  }
  for (const [index, collision] of declared.entries()) {
    const collisionPath = `${path}.fingerprint_collisions[${index}]`;
    assertNonEmptyString(collision.rationale, `${collisionPath}.rationale`);
    if (!Array.isArray(collision.producers) || collision.producers.length === 0) {
      fail(`${collisionPath}.producers must name the rows where the fingerprints collide`);
    }
  }
  return declared;
};

export const loadManifest = (manifestPath) => {
  const absolutePath = resolve(manifestPath);
  const manifest = JSON.parse(readFileSync(absolutePath, "utf8"));
  if (manifest.$schema !== EXPECTED_SCHEMA) {
    fail(`unsupported coverage producer manifest schema: ${manifest.$schema}`);
  }
  const root = dirname(absolutePath);

  const recorded = manifest.recorded ?? fail("manifest.recorded must record provenance");
  assertNonEmptyString(recorded.node_pin, "recorded.node_pin");
  assertNonEmptyString(recorded.platform, "recorded.platform");
  assertNonEmptyString(recorded.coverage_root, "recorded.coverage_root");
  assertNonEmptyString(recorded.producers_lock_file, "recorded.producers_lock_file");
  if (!/^[0-9a-f]{64}$/.test(recorded.producers_lock_sha256 ?? "")) {
    fail("recorded.producers_lock_sha256 must be a sha256 digest");
  }
  const lockContents = readFileSync(resolve(root, recorded.producers_lock_file));
  if (sha256(lockContents) !== recorded.producers_lock_sha256) {
    fail(
      "producer lockfile digest mismatch: the pinned dependency graph moved. " +
        "The lockfile is the pin, not the version string; istanbul-lib-instrument 6.0.3 " +
        "floats @babel/parser on ^7.23.9 and emits different geometry on different days. " +
        "Re-record with npm run refresh:coverage-producers.",
    );
  }

  const configPath = join(root, CORPUS_CONFIG_FILE);
  if (!existsSync(configPath)) {
    fail(
      `the corpus config ${CORPUS_CONFIG_FILE} is missing; without it the census inherits ` +
        "whatever config the surrounding checkout happens to carry",
    );
  }

  const invariants = loadInvariants(manifest);
  const producers = loadProducers(manifest, root);
  const fixtures = loadFixtures(manifest, root, invariants);

  const maps = new Map();
  for (const [index, entry] of (manifest.maps ?? []).entries()) {
    const path = `maps[${index}]`;
    if (!producers.has(entry.producer)) {
      fail(`${path}.producer is not a manifest row: ${entry.producer}`);
    }
    if (!fixtures.has(entry.fixture)) {
      fail(`${path}.fixture is not a manifest probe: ${entry.fixture}`);
    }
    const key = `${entry.producer}/${entry.fixture}`;
    if (maps.has(key)) {
      fail(`duplicate recorded map: ${key}`);
    }
    const contents = readFileSync(resolve(root, entry.file));
    if (sha256(contents) !== entry.sha256) {
      fail(`${entry.file} digest mismatch; re-run npm run refresh:coverage-producers`);
    }
    maps.set(key, { ...entry, map: JSON.parse(contents.toString("utf8")) });
  }
  if (maps.size === 0) {
    fail("manifest.maps must reference at least one recorded producer map");
  }

  const census = new Map();
  for (const [index, row] of (manifest.census ?? []).entries()) {
    const path = `census[${index}]`;
    const key = `${row.producer}/${row.fixture}`;
    if (!maps.has(key)) {
      fail(`${path} has no recorded map: ${key}`);
    }
    if (census.has(key)) {
      fail(`duplicate census row: ${key}`);
    }
    census.set(key, { ...row, units: loadCensusUnits(row, path) });
  }
  for (const key of maps.keys()) {
    if (!census.has(key)) {
      fail(`recorded map ${key} has no census row; seed it with refresh:coverage-producers`);
    }
  }

  return { census, configPath, fixtures, invariants, manifest, maps, producers, root };
};

/**
 * Reduce a `fallow health` report to the census: the sorted per-unit list of
 * identity plus coverage provenance. Nothing else is asserted, because
 * `summary.coverage_model`, the health score and the matched/total scalars all
 * stay constant across the geometry moves this corpus exists to detect.
 */
export const buildCensus = (report) => {
  if (report === null || typeof report !== "object" || !Array.isArray(report.findings)) {
    fail("fallow health report must contain findings[]");
  }
  return sortCensus(
    report.findings.map((finding) => {
      const unit = {
        col: finding.col,
        coverage_source: finding.coverage_source ?? "estimated",
        cyclomatic: finding.cyclomatic,
        line: finding.line,
        name: finding.name,
      };
      if (finding.coverage_pct !== undefined) {
        unit.coverage_pct = finding.coverage_pct;
      }
      return unit;
    }),
  );
};

const comparableUnit = (unit) => ({
  col: unit.col,
  coverage_pct: unit.coverage_pct,
  coverage_source: unit.coverage_source,
  line: unit.line,
  name: unit.name,
});

/**
 * Census delta, as human-readable lines. Empty means the row still holds.
 */
export const compareCensus = (expected, observed) => {
  const deltas = [];
  const expectedByKey = new Map(sortCensus(expected).map((unit) => [censusKey(unit), unit]));
  const observedByKey = new Map(sortCensus(observed).map((unit) => [censusKey(unit), unit]));

  for (const [key, unit] of expectedByKey) {
    if (!observedByKey.has(key)) {
      deltas.push(`missing unit ${key} (recorded ${unit.coverage_source})`);
    }
  }
  for (const [key, unit] of observedByKey) {
    if (!expectedByKey.has(key)) {
      deltas.push(`unexpected unit ${key} (observed ${unit.coverage_source})`);
      continue;
    }
    const before = comparableUnit(expectedByKey.get(key));
    const after = comparableUnit(unit);
    if (before.coverage_source !== after.coverage_source) {
      deltas.push(`${key}: coverage_source ${before.coverage_source} -> ${after.coverage_source}`);
      continue;
    }
    if (!Object.is(before.coverage_pct, after.coverage_pct)) {
      deltas.push(`${key}: coverage_pct ${before.coverage_pct} -> ${after.coverage_pct}`);
    }
  }
  return deltas;
};

/**
 * Group producers whose census is identical across every probe, so an
 * unchanged matrix reviews as a handful of lines rather than one per row.
 */
export const equivalenceClasses = (rows) => {
  const byProducer = new Map();
  for (const row of rows) {
    const list = byProducer.get(row.producer) ?? [];
    list.push(row);
    byProducer.set(row.producer, list);
  }

  const classes = new Map();
  for (const [producer, list] of byProducer) {
    const ordered = list.toSorted((left, right) => left.fixture.localeCompare(right.fixture));
    const signature = ordered
      .map((row) => `${row.fixture}\n${censusSignature(row.units)}`)
      .join("\n--\n");
    const existing = classes.get(signature);
    if (existing === undefined) {
      classes.set(signature, {
        fixtures: ordered.length,
        members: [producer],
        resolved: ordered.reduce((total, row) => total + row.resolved, 0),
        units: ordered.reduce((total, row) => total + row.units.length, 0),
      });
    } else {
      existing.members.push(producer);
    }
  }
  return [...classes.values()];
};

const recordAliasLines = (record) =>
  [record.line, record.decl?.start?.line, record.loc?.start?.line].filter((line) =>
    Number.isInteger(line),
  );

/**
 * Shift `fnMap` records so no alias of the moved record can still reach
 * `targetLine` within the matcher's fuzz window.
 *
 * Shifting by a fixed three lines is not enough on its own: a record whose
 * declaration opens a line above the unit (a decorated member, for instance)
 * lands two lines away, which is still inside `ALIAS_FUZZ_MAX_LINE_DRIFT`. The
 * shift is therefore computed from the record's own lowest alias line.
 *
 * @param {object} map Recorded coverage map, not mutated.
 * @param {string} virtualPath Key the map records the probe under.
 * @param {number} targetLine Line of the resolved unit whose record to move.
 * @param {boolean} everyRecord Move all records, not just the nearest one.
 *   Reserved for rows whose declared fingerprint collision makes a single
 *   record's move invisible in the census.
 * @returns {{ map: object, record: string, shift: number } | null}
 */
export const perturbCoverageMap = (map, virtualPath, targetLine, everyRecord = false) => {
  const copy = JSON.parse(JSON.stringify(map));
  const file = copy[virtualPath];
  if (file === undefined) {
    return null;
  }
  const entries = Object.entries(file.fnMap ?? {});
  if (entries.length === 0) {
    return null;
  }
  const effectiveLine = ([, record]) =>
    record.line > 0 ? record.line : (record.decl?.start?.line ?? 0);
  const [id, nearest] = entries.reduce((best, candidate) =>
    Math.abs(effectiveLine(candidate) - targetLine) < Math.abs(effectiveLine(best) - targetLine)
      ? candidate
      : best,
  );

  const moving = everyRecord ? entries.map(([, record]) => record) : [nearest];
  const lowest = Math.min(...moving.flatMap((record) => recordAliasLines(record)));
  const shift = Math.max(
    PERTURBATION_LINE_SHIFT,
    targetLine + ALIAS_FUZZ_MAX_LINE_DRIFT + 1 - lowest,
  );

  const move = (position) => {
    if (position !== null && typeof position === "object" && Number.isInteger(position.line)) {
      position.line += shift;
    }
  };
  for (const record of moving) {
    if (Number.isInteger(record.line)) {
      record.line += shift;
    }
    for (const span of [record.decl, record.loc]) {
      move(span?.start);
      move(span?.end);
    }
  }
  return { map: copy, record: everyRecord ? "all" : id, shift };
};

/**
 * Move every placed column in the file past the end of every line in the
 * probe, which is the column analogue of moving a line past the matcher's
 * fuzz window.
 *
 * The matcher normalizes small column drift on purpose: a named record that
 * moves a few columns still resolves through the name-fuzzy alias, which is
 * why the census absorbs a producer that re-anchors `decl` from the identifier
 * to the `function` keyword. What must stay true is that the census reads the
 * column at all, and the way to prove that without asserting geometry is to
 * push every column somewhere no line reaches. Every record moves, because a
 * producer that changes where it anchors changes every record in the file, not
 * one.
 *
 * @param {object} map Recorded coverage map, not mutated.
 * @param {string} virtualPath Key the map records the probe under.
 * @param {string} source Probe source, which bounds the shift.
 * @returns {{ map: object, columns: number, shift: number } | null}
 */
export const perturbCoverageColumns = (map, virtualPath, source, direction = "right") => {
  const copy = JSON.parse(JSON.stringify(map));
  const file = copy[virtualPath];
  if (file === undefined) {
    return null;
  }
  const records = Object.values(file.fnMap ?? {});
  if (records.length === 0) {
    return null;
  }
  const shift = Math.max(...source.split("\n").map((line) => line.length)) + 1;

  let columns = 0;
  for (const record of records) {
    for (const span of [record.decl, record.loc]) {
      for (const position of [span?.start, span?.end]) {
        if (Number.isInteger(position?.column) && position.column >= 0) {
          // Right: the anchor moves past the end of every line in the probe.
          // Collapse: the producer stops placing columns and reports the line
          // start, which is the direction a re-anchor actually takes (an
          // identifier anchor moving to the keyword, or column precision being
          // dropped altogether).
          position.column = direction === "collapse" ? 0 : position.column + shift;
          columns += 1;
        }
      }
    }
  }
  return columns === 0 ? null : { columns, direction, map: copy, shift };
};

const invokeFallow = (binary, request) => {
  const result = spawnSync(
    binary,
    [
      "health",
      "--root",
      request.root,
      // The census must answer for the matcher, not for whatever config the
      // surrounding checkout carries. Fallow walks up from `--root` otherwise,
      // and every fixture sits inside this repository.
      "--config",
      request.configPath,
      "--complexity",
      "--coverage",
      request.coveragePath,
      "--coverage-root",
      request.coverageRoot,
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
  if (result.error) {
    fail(`${request.label}: failed to start fallow: ${result.error.message}`);
  }
  if (result.signal) {
    fail(`${request.label}: fallow terminated by signal ${result.signal}`);
  }
  // `health` exits 1 whenever it reports findings, which is every run here.
  if (result.status !== 0 && result.status !== 1) {
    fail(`${request.label}: fallow exited ${result.status}: ${(result.stderr ?? "").trim()}`);
  }
  try {
    return JSON.parse(result.stdout);
  } catch (error) {
    fail(`${request.label}: invalid fallow JSON: ${error.message}`);
  }
};

const withTemporaryMap = (map, use) => {
  const directory = mkdtempSync(join(tmpdir(), "fallow-coverage-producer-"));
  const path = join(directory, "coverage-final.json");
  writeFileSync(path, JSON.stringify(map));
  try {
    return use(path);
  } finally {
    rmSync(directory, { force: true, recursive: true });
  }
};

const fingerprintCollisions = (units) => {
  const byPct = new Map();
  for (const unit of units) {
    if (unit.coverage_source !== "istanbul") {
      continue;
    }
    const bucket = byPct.get(unit.coverage_pct) ?? [];
    bucket.push(censusKey(unit));
    byPct.set(unit.coverage_pct, bucket);
  }
  return [...byPct.entries()]
    .filter(([, keys]) => keys.length > 1)
    .map(([pct, keys]) => `${pct}% shared by ${keys.join(", ")}`);
};

/**
 * Run every recorded row against the real binary.
 *
 * @param {ReturnType<typeof loadManifest>} loaded
 * @param {(binary: string, request: object) => object} invoke Injectable so the
 *   unit test can exercise the comparison without a binary.
 * @param {string} fallowBin
 */
export const runConformance = (loaded, invoke = invokeFallow, fallowBin = DEFAULT_FALLOW_BIN) => {
  const rows = [];
  const failures = [];

  for (const [key, entry] of loaded.maps) {
    const producer = loaded.producers.get(entry.producer);
    const fixture = loaded.fixtures.get(entry.fixture);
    const expected = loaded.census.get(key);
    const fixtureRoot = resolve(loaded.root, dirname(fixture.file));
    const request = {
      configPath: loaded.configPath,
      coverageRoot: loaded.manifest.recorded.coverage_root,
      label: key,
      root: fixtureRoot,
    };

    const observed = withTemporaryMap(entry.map, (coveragePath) =>
      buildCensus(invoke(fallowBin, { ...request, coveragePath })),
    );

    const deltas = compareCensus(expected.units, observed);
    const invariantNames = fixture.invariants
      .map((id) => `${id} (${loaded.invariants.get(id).rust_fn})`)
      .join(", ");
    for (const delta of deltas) {
      failures.push(`${key}: ${delta}\n    evidence lost for: ${invariantNames}`);
    }

    const shallow = observed.filter((unit) => unit.cyclomatic < MIN_PROBE_CYCLOMATIC);
    for (const unit of shallow) {
      failures.push(
        `${key}: probe ${censusKey(unit)} has cyclomatic ${unit.cyclomatic}; ` +
          `every probe must reach ${MIN_PROBE_CYCLOMATIC} so its CRAP score clears ` +
          "the --max-crap 1 floor without resting on a floating-point equality",
      );
    }

    const collisions = fingerprintCollisions(expected.units);
    const declared = loadFingerprintCollisions(fixture, `fixtures[${fixture.id}]`).filter(
      (collision) => collision.producers.includes(entry.producer),
    );
    if (collisions.length > 0 && declared.length === 0) {
      failures.push(
        `${key}: coverage_pct fingerprints collide (${collisions.join("; ")}) without a ` +
          "declared fingerprint_collisions entry, so the census cannot prove WHICH record " +
          "resolved. Give each probe function a distinct executed-over-total statement ratio.",
      );
    }
    if (collisions.length === 0 && declared.length > 0) {
      failures.push(
        `${key}: fixtures[${fixture.id}].fingerprint_collisions declares a collision this row ` +
          "no longer has; drop the stale declaration",
      );
    }

    rows.push({
      collisions,
      fixture: entry.fixture,
      id: key,
      producer: entry.producer,
      resolved: observed.filter((unit) => unit.coverage_source === "istanbul").length,
      self_conformance: producer.self_conformance,
      units: observed.map(comparableUnit),
    });
  }

  return { failures, rows };
};

/**
 * Layer 3. Perturb every row twice and require the census to fail both times:
 * once with a record moved past the matcher's fuzz window, once with every
 * column moved past the end of its own line. Offline: no producer package is
 * loaded, and a perturbed map is written to a temporary directory, never into
 * the working tree.
 */
export const runSelfTest = (loaded, invoke = invokeFallow, fallowBin = DEFAULT_FALLOW_BIN) => {
  const checks = [];
  const failures = [];

  const censusOf = (key, fixture, map) =>
    withTemporaryMap(map, (coveragePath) =>
      buildCensus(
        invoke(fallowBin, {
          configPath: loaded.configPath,
          coveragePath,
          coverageRoot: loaded.manifest.recorded.coverage_root,
          label: `${key} (perturbed)`,
          root: resolve(loaded.root, dirname(fixture.file)),
        }),
      ),
    );

  const runPerturbed = (key, fixture, expected, anchorLine, everyRecord) => {
    const perturbed = perturbCoverageMap(
      loaded.maps.get(key).map,
      fixture.virtual_path,
      anchorLine,
      everyRecord,
    );
    if (perturbed === null) {
      return null;
    }
    return {
      deltas: compareCensus(expected.units, censusOf(key, fixture, perturbed.map)),
      perturbed,
    };
  };

  const runColumnPerturbed = (key, fixture, expected, direction) => {
    const perturbed = perturbCoverageColumns(
      loaded.maps.get(key).map,
      fixture.virtual_path,
      fixture.source,
      direction,
    );
    if (perturbed === null) {
      return null;
    }
    return {
      deltas: compareCensus(expected.units, censusOf(key, fixture, perturbed.map)),
      perturbed,
    };
  };

  for (const [key, entry] of loaded.maps) {
    const fixture = loaded.fixtures.get(entry.fixture);
    const expected = loaded.census.get(key);
    const anchor = expected.units.find((unit) => unit.coverage_source === "istanbul");
    if (anchor === undefined) {
      // The row's whole content is that this producer's records do not
      // resolve. Moving one of them proves nothing, and the census already
      // fails the moment any of them starts resolving.
      checks.push({ id: key, sensitive: null, skipped: "row resolves no unit" });
      continue;
    }
    const check = { id: key };
    checks.push(check);

    const single = runPerturbed(key, fixture, expected, anchor.line, false);
    if (single === null) {
      failures.push(`${key}: recorded map holds no fnMap record to perturb`);
      continue;
    }
    if (single.deltas.length > 0) {
      check.record = single.perturbed.record;
      check.sensitive = true;
      check.shift = single.perturbed.shift;
    } else {
      const collisions = (fixture.fingerprint_collisions ?? []).filter((collision) =>
        collision.producers.includes(entry.producer),
      );
      if (collisions.length === 0) {
        failures.push(
          `${key}: shifting record ${single.perturbed.record} by ${single.perturbed.shift} ` +
            "lines left the census unchanged, so this row has lost its line sensitivity and " +
            "can no longer fail",
        );
        check.record = single.perturbed.record;
        check.sensitive = false;
      } else {
        // Every record in this row carries the same fingerprint by
        // construction, so moving one is invisible: the census can only notice
        // the geometry moving as a whole. The manifest already documents why.
        const all = runPerturbed(key, fixture, expected, anchor.line, true);
        check.escalated = true;
        check.record = "all";
        check.sensitive = all !== null && all.deltas.length > 0;
        if (check.sensitive) {
          check.shift = all.perturbed.shift;
        } else {
          failures.push(
            `${key}: shifting every record left the census unchanged, so this row has lost ` +
              "its line sensitivity and can no longer fail",
          );
        }
      }
    }

    // A rightward move lands every anchor past the end of every line in the
    // probe, which no producer would emit, so a census that survives it is
    // reading the line half of an anchor alone. That one is a failure.
    const columns = runColumnPerturbed(key, fixture, expected, "right");
    if (columns === null) {
      failures.push(`${key}: recorded map places no column to perturb`);
      continue;
    }
    check.column_sensitive = columns.deltas.length > 0;
    check.column_shift = columns.perturbed.shift;
    if (!check.column_sensitive) {
      failures.push(
        `${key}: moving all ${columns.perturbed.columns} recorded columns right by ` +
          `${columns.perturbed.shift}, past the end of every line in the probe, left the census ` +
          "unchanged, so this row reads only the line half of an anchor and a producer that " +
          "re-columns its records would land silently",
      );
    }

    // Collapsing every column to the line start is the other direction a
    // producer re-anchors in, and the matcher absorbs it by design: the
    // anonymous fallback accepts a bounded column drift, so a census that
    // holds through a collapse is the tolerance working rather than a blind
    // spot. Recorded, not asserted, so the count moving is visible in a diff.
    const collapsed = runColumnPerturbed(key, fixture, expected, "collapse");
    check.column_collapse_sensitive = collapsed !== null && collapsed.deltas.length > 0;
  }

  for (const [dimension, sensitiveIn] of [
    ["line", (check) => check.sensitive === true],
    ["column", (check) => check.column_sensitive === true],
  ]) {
    const sensitive = checks.filter(sensitiveIn).map((check) => check.id);
    for (const producer of loaded.producers.keys()) {
      if (!sensitive.some((id) => id.startsWith(`${producer}/`))) {
        failures.push(`producer row ${producer} has no census that can fail on ${dimension} drift`);
      }
    }
    for (const fixture of loaded.fixtures.keys()) {
      if (!sensitive.some((id) => id.endsWith(`/${fixture}`))) {
        failures.push(`fixture ${fixture} has no census that can fail on ${dimension} drift`);
      }
    }
  }

  return { checks, failures };
};

const summarize = (loaded, conformance, selfTest) => ({
  schema: EXPECTED_SCHEMA,
  recorded: loaded.manifest.recorded,
  producers: [...loaded.producers.values()].map((producer) => ({
    id: producer.id,
    package: producer.package,
    profile: producer.profile,
    self_conformance: producer.self_conformance,
    version: producer.version,
  })),
  equivalence_classes: equivalenceClasses(conformance.rows),
  rows: conformance.rows,
  self_test: selfTest.checks,
  summary: {
    census_deltas: conformance.failures.length,
    independent_rows: conformance.rows.filter((row) => !row.self_conformance).length,
    lost_sensitivity: selfTest.failures.length,
    rows: conformance.rows.length,
  },
});

export const main = (
  argv = process.argv.slice(2),
  write = process.stdout.write.bind(process.stdout),
) => {
  const options = parseArgs(argv);
  const loaded = loadManifest(options.manifest);
  if (!existsSync(options.fallowBin)) {
    fail(
      `fallow binary not found at ${options.fallowBin}; run cargo build -p fallow-cli --bin fallow`,
    );
  }
  const conformance = runConformance(loaded, invokeFallow, resolve(options.fallowBin));
  const selfTest =
    conformance.failures.length === 0
      ? runSelfTest(loaded, invokeFallow, resolve(options.fallowBin))
      : { checks: [], failures: [] };

  const result = summarize(loaded, conformance, selfTest);
  write(`${JSON.stringify(result, null, options.pretty ? 2 : 0)}\n`);

  const problems = [...conformance.failures, ...selfTest.failures];

  for (const entry of result.equivalence_classes) {
    process.stderr.write(
      `[coverage-producers] ${entry.resolved}/${entry.units} units resolved across ` +
        `${entry.fixtures} probes: ${entry.members.join(", ")}\n`,
    );
  }
  for (const problem of problems) {
    process.stderr.write(`[coverage-producers] ${problem}\n`);
  }

  if (problems.length > 0) {
    process.stderr.write(
      "[coverage-producers] A census delta is a matcher change, not a baseline to accept. " +
        "Fix crates/engine/src/health/scoring.rs, or hand-edit the census only when a producer " +
        "genuinely stopped emitting a record.\n",
    );
    return 1;
  }
  return 0;
};

const isMain =
  process.argv[1] !== undefined && resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (isMain) {
  try {
    process.exitCode = main();
  } catch (error) {
    process.stderr.write(`${error.message}\n`);
    process.exitCode = 1;
  }
}
