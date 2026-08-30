import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";

import {
  ALIAS_FUZZ_MAX_LINE_DRIFT,
  CORPUS_CONFIG_FILE,
  MIN_PROBE_CYCLOMATIC,
  buildCensus,
  censusSignature,
  compareCensus,
  equivalenceClasses,
  loadManifest,
  parseArgs,
  perturbCoverageColumns,
  perturbCoverageMap,
  runConformance,
  runSelfTest,
} from "./coverage-producer-conformance.mjs";

const REPO_ROOT = resolve(import.meta.dirname, "..");
const MANIFEST = resolve(REPO_ROOT, "tests/coverage-producer-corpus/manifest.json");

/**
 * Replay the recorded census as if the binary had produced it. Every unit is
 * given a cyclomatic complexity that clears the probe floor, so a test that
 * wants to exercise the floor has to lower it explicitly.
 */
const replayInvoke =
  (loaded, transform = (units) => units) =>
  (_binary, request) => {
    const row = loaded.census.get(request.label.replace(" (perturbed)", ""));
    return {
      findings: transform(row.units, request).map((unit) => ({
        col: unit.col,
        coverage_pct: unit.coverage_pct,
        coverage_source: unit.coverage_source,
        cyclomatic: unit.cyclomatic ?? MIN_PROBE_CYCLOMATIC,
        line: unit.line,
        name: unit.name,
      })),
    };
  };

test("manifest pins the producer lockfile, every fixture and every recorded map", () => {
  const loaded = loadManifest(MANIFEST);

  assert.match(loaded.manifest.recorded.producers_lock_sha256, /^[0-9a-f]{64}$/);
  assert.match(loaded.manifest.recorded.node_pin, /^v\d+\.\d+\.\d+/);
  assert.equal(loaded.manifest.recorded.coverage_root, "/fallow/corpus");
  assert.ok(loaded.producers.size >= 4, "the matrix needs at least four producer rows");
  assert.ok(loaded.maps.size >= loaded.fixtures.size);
  assert.equal(loaded.census.size, loaded.maps.size);
});

test("every probe carries a named invariant that points at a matcher mechanism", () => {
  const loaded = loadManifest(MANIFEST);

  for (const fixture of loaded.fixtures.values()) {
    assert.ok(fixture.invariants.length > 0, `${fixture.id} names no invariant`);
    for (const id of fixture.invariants) {
      assert.match(loaded.invariants.get(id).rust_fn, /^crates\/.+\.rs::/);
    }
  }
});

test("at least one row is an independent producer, and the oxc rows are flagged", () => {
  const loaded = loadManifest(MANIFEST);
  const producers = [...loaded.producers.values()];

  assert.ok(
    producers.filter((producer) => !producer.self_conformance).length >= 2,
    "agreement between this project's own instrumenter profiles is weaker evidence",
  );
  for (const producer of producers.filter((entry) => entry.package.startsWith("oxc-"))) {
    assert.equal(producer.self_conformance, true);
  }
});

test("the matrix is not degenerate: producers disagree about the same probes", () => {
  const loaded = loadManifest(MANIFEST);
  const rows = [...loaded.census.entries()].map(([id, row]) => ({
    fixture: row.fixture,
    id,
    producer: row.producer,
    resolved: row.units.filter((unit) => unit.coverage_source === "istanbul").length,
    units: row.units,
  }));

  assert.ok(
    equivalenceClasses(rows).length >= 2,
    "collapsing the matrix to one behavior would let a single producer stand for all of them",
  );

  const disagreements = [...loaded.fixtures.keys()].filter((fixture) => {
    const signatures = new Set(
      rows.filter((row) => row.fixture === fixture).map((row) => censusSignature(row.units)),
    );
    return signatures.size > 1;
  });
  assert.ok(
    disagreements.length >= 3,
    "at least three probes must separate the producers, or the corpus proves little",
  );
});

test("unattributed census units carry a hand-written rationale", () => {
  const loaded = loadManifest(MANIFEST);
  const misses = [...loaded.census.values()].flatMap((row) =>
    row.units.filter((unit) => unit.coverage_source === "estimated"),
  );

  assert.ok(misses.length > 0, "a matrix where every producer resolves everything proves nothing");
  for (const miss of misses) {
    assert.ok(miss.rationale.length > 40, "a rationale must name the producer behavior");
  }
});

test("census comparison reports source flips, fingerprint moves and vanished units", () => {
  const recorded = [
    { col: 0, coverage_pct: 50, coverage_source: "istanbul", line: 1, name: "alpha" },
    { col: 0, coverage_source: "estimated", line: 8, name: "beta" },
  ];

  assert.deepEqual(compareCensus(recorded, recorded), []);
  assert.deepEqual(
    compareCensus(recorded, [
      { col: 0, coverage_pct: 75, coverage_source: "istanbul", line: 1, name: "alpha" },
      recorded[1],
    ]),
    ["000001:0:alpha: coverage_pct 50 -> 75"],
  );
  assert.deepEqual(
    compareCensus(recorded, [
      { col: 0, coverage_source: "estimated", line: 1, name: "alpha" },
      recorded[1],
    ]),
    ["000001:0:alpha: coverage_source istanbul -> estimated"],
  );
  assert.deepEqual(compareCensus(recorded, [recorded[0]]), [
    "missing unit 000008:0:beta (recorded estimated)",
  ]);
});

test("the recorded census replays clean against the manifest", () => {
  const loaded = loadManifest(MANIFEST);
  const result = runConformance(loaded, replayInvoke(loaded), "unused");

  assert.deepEqual(result.failures, []);
  assert.equal(result.rows.length, loaded.maps.size);
});

test("a shallow probe fails the CRAP floor precondition", () => {
  const loaded = loadManifest(MANIFEST);
  const result = runConformance(
    loaded,
    replayInvoke(loaded, (units) => units.map((unit) => ({ ...unit, cyclomatic: 1 }))),
    "unused",
  );

  assert.ok(
    result.failures.some((failure) => failure.includes("has cyclomatic 1")),
    "a cc=1 probe sits on the --max-crap 1 equality boundary and must be rejected",
  );
});

test("a census delta names the matcher mechanism that just lost its evidence", () => {
  const loaded = loadManifest(MANIFEST);
  const result = runConformance(
    loaded,
    replayInvoke(loaded, (units) =>
      units.map((unit, index) =>
        index === 0 ? { ...unit, coverage_pct: undefined, coverage_source: "estimated" } : unit,
      ),
    ),
    "unused",
  );

  assert.ok(result.failures.length > 0);
  assert.ok(result.failures.every((failure) => failure.includes("evidence lost for:")));
  assert.ok(
    result.failures.some((failure) => failure.includes("crates/engine/src/health/scoring.rs::")),
  );
});

test("perturbation moves a record clear of the matcher's fuzz window", () => {
  const map = {
    "/fallow/corpus/probe.js": {
      fnMap: {
        0: {
          decl: { end: { column: 3, line: 4 }, start: { column: 2, line: 4 } },
          line: 5,
          loc: { end: { column: 3, line: 10 }, start: { column: 49, line: 5 } },
          name: "(anonymous_0)",
        },
      },
      path: "/fallow/corpus/probe.js",
    },
  };

  const perturbed = perturbCoverageMap(map, "/fallow/corpus/probe.js", 5);
  const moved = perturbed.map["/fallow/corpus/probe.js"].fnMap[0];

  assert.ok(
    moved.decl.start.line - 5 > ALIAS_FUZZ_MAX_LINE_DRIFT,
    "a fixed three-line shift leaves a decorator declaration inside the fuzz window",
  );
  assert.equal(map["/fallow/corpus/probe.js"].fnMap[0].decl.start.line, 4, "input is not mutated");
});

test("the column perturbation moves every column past the end of every line", () => {
  const map = {
    "/fallow/corpus/probe.js": {
      fnMap: {
        0: {
          decl: { end: { column: 16, line: 1 }, start: { column: 9, line: 1 } },
          line: 1,
          loc: { end: { column: -1, line: 4 }, start: { column: 19, line: 1 } },
          name: "alpha",
        },
      },
      path: "/fallow/corpus/probe.js",
    },
  };
  const source = "function alpha() {\n  return 1;\n}\n";

  const perturbed = perturbCoverageColumns(map, "/fallow/corpus/probe.js", source);
  const moved = perturbed.map["/fallow/corpus/probe.js"].fnMap[0];
  const longest = Math.max(...source.split("\n").map((line) => line.length));

  assert.ok(
    moved.decl.start.column > longest,
    "a moved column that still lands on a real line proves nothing about the census",
  );
  assert.equal(
    perturbed.columns,
    3,
    "the unplaceable end column is left where the producer put it",
  );
  assert.equal(moved.loc.end.column, -1);
  assert.equal(
    map["/fallow/corpus/probe.js"].fnMap[0].decl.start.column,
    9,
    "input is not mutated",
  );
});

test("the self-test fails a row whose census stopped responding to geometry", () => {
  const loaded = loadManifest(MANIFEST);
  const result = runSelfTest(loaded, replayInvoke(loaded), "unused");

  assert.ok(
    result.failures.length > 0,
    "an invoke that ignores the perturbed map must be reported as lost sensitivity",
  );
  assert.ok(result.failures.some((failure) => failure.includes("lost its line sensitivity")));
  assert.ok(
    result.failures.some((failure) => failure.includes("reads only the line half of an anchor")),
    "line sensitivity alone would leave every recorded column unproven",
  );
  for (const dimension of ["line", "column"]) {
    assert.ok(
      result.failures.some((failure) => failure.endsWith(`can fail on ${dimension} drift`)),
      `a producer or probe with no ${dimension}-sensitive row must be named`,
    );
  }
});

test("census derived from a report keeps identity and provenance only", () => {
  const census = buildCensus({
    findings: [
      {
        col: 0,
        coverage_pct: 40,
        coverage_source: "istanbul",
        cyclomatic: 2,
        line: 8,
        name: "beta",
      },
      { col: 0, cyclomatic: 2, line: 1, name: "alpha" },
    ],
  });

  assert.deepEqual(
    census.map((unit) => [unit.name, unit.coverage_source, unit.coverage_pct]),
    [
      ["alpha", "estimated", undefined],
      ["beta", "istanbul", 40],
    ],
  );
});

test("no argument can turn a census delta into a report", () => {
  assert.equal(parseArgs(["--pretty"]).pretty, true);
  assert.equal(parseArgs(["--fallow-bin", "/tmp/fallow"]).fallowBin, "/tmp/fallow");
  for (const argument of ["--check", "--report-only", "--allow-regressions", "--update"]) {
    assert.throws(
      () => parseArgs([argument]),
      /unknown argument/,
      `${argument} must not exist: a gate whose verdict rests on a flag is one token from off`,
    );
  }
});

test("the gate npm script runs the harness with no verdict-changing flag", () => {
  const rootPackage = JSON.parse(readFileSync(resolve(REPO_ROOT, "package.json"), "utf8"));

  assert.equal(
    rootPackage.scripts["check:coverage-producers"],
    "node scripts/coverage-producer-conformance.mjs",
  );
  assert.equal(
    rootPackage.scripts["conformance:coverage-producers"],
    undefined,
    "a second entry point for the same command is a second thing to keep in step",
  );
});

test("the census runs under a corpus-owned config, not the repository's", () => {
  const loaded = loadManifest(MANIFEST);

  assert.equal(loaded.configPath, resolve(loaded.root, CORPUS_CONFIG_FILE));
  assert.ok(existsSync(loaded.configPath), "the corpus config is committed, not generated");
  for (const script of ["coverage-producer-conformance.mjs", "coverage-producer-refresh.mjs"]) {
    assert.match(
      readFileSync(resolve(REPO_ROOT, "scripts", script), "utf8"),
      /"--config",/,
      `${script} must pin the config, or fallow walks up into the repository's own .fallowrc.json`,
    );
  }
});

test("the corpus lives outside every formatter and linter directory list", () => {
  const rootPackage = JSON.parse(readFileSync(resolve(REPO_ROOT, "package.json"), "utf8"));

  for (const script of ["lint:js", "fmt:js", "fmt:js:check"]) {
    assert.ok(
      !/tests\/coverage-producer-corpus\/(?!producers\/)/.test(rootPackage.scripts[script]),
      `${script} must not reach the probe sources; reformatting them moves the anchors under test`,
    );
    assert.ok(
      rootPackage.scripts[script].includes("tests/coverage-producer-corpus/producers/drivers"),
      `${script} must still cover the driver modules, which are committed JavaScript`,
    );
  }
  assert.equal(
    rootPackage.devDependencies["istanbul-lib-instrument"],
    undefined,
    "the producers are installed under their own prefix, never by the root npm ci",
  );
});
