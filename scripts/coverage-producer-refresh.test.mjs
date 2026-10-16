import assert from "node:assert/strict";
import test from "node:test";

import {
  main,
  normalizeMap,
  parseArgs,
  seedCensusUnit,
  serializeMap,
} from "./coverage-producer-refresh.mjs";

const withFallowBin = (value, run) => {
  const previous = process.env.FALLOW_BIN;
  if (value === undefined) {
    delete process.env.FALLOW_BIN;
  } else {
    process.env.FALLOW_BIN = value;
  }
  try {
    return run();
  } finally {
    if (previous === undefined) {
      delete process.env.FALLOW_BIN;
    } else {
      process.env.FALLOW_BIN = previous;
    }
  }
};

test("parseArgs defaults to the re-record mode", () => {
  const options = withFallowBin(undefined, () => parseArgs([]));

  assert.equal(options.check, false);
  assert.equal(typeof options.fallowBin, "string");
  assert.match(options.fallowBin, /fallow(\.exe)?$/);
});

test("parseArgs selects the check mode with --check", () => {
  assert.equal(parseArgs(["--check"]).check, true);
  assert.equal(parseArgs(["--check", "--check"]).check, true);
});

test("parseArgs reads the binary path from FALLOW_BIN", () => {
  const options = withFallowBin("/opt/custom/fallow", () => parseArgs(["--check"]));

  assert.equal(options.fallowBin, "/opt/custom/fallow");
});

test("parseArgs rejects an unknown argument", () => {
  assert.throws(() => parseArgs(["--write"]), /unknown argument: --write/);
  assert.throws(() => parseArgs(["--check", "extra"]), /unknown argument: extra/);
  assert.throws(() => parseArgs([""]), /unknown argument: $/);
});

test("serializeMap writes two-space JSON with one trailing newline", () => {
  assert.equal(serializeMap({ a: { b: 1 } }), '{\n  "a": {\n    "b": 1\n  }\n}\n');
  assert.equal(serializeMap({}), "{}\n");
});

test("normalizeMap keys the coverage by the virtual path and rewrites path", () => {
  const fileCoverage = { path: "/home/user/checkout/probe.js", s: { 0: 1 }, statementMap: {} };

  const map = normalizeMap(fileCoverage, "/fallow/corpus/probe.js");

  assert.deepEqual(Object.keys(map), ["/fallow/corpus/probe.js"]);
  assert.deepEqual(map["/fallow/corpus/probe.js"], {
    path: "/fallow/corpus/probe.js",
    s: { 0: 1 },
    statementMap: {},
  });
  assert.equal(fileCoverage.path, "/home/user/checkout/probe.js", "the input stays unchanged");
});

test("normalizeMap adds a path when the producer emits none", () => {
  const map = normalizeMap({ s: {} }, "/fallow/corpus/empty.ts");

  assert.deepEqual(map, { "/fallow/corpus/empty.ts": { s: {}, path: "/fallow/corpus/empty.ts" } });
});

test("seedCensusUnit keeps the measured percentage of an istanbul unit", () => {
  const unit = {
    name: "probe",
    line: 3,
    col: 7,
    coverage_source: "istanbul",
    coverage_pct: 50,
    cyclomatic: 4,
  };

  assert.deepEqual(seedCensusUnit(unit), {
    name: "probe",
    line: 3,
    col: 7,
    coverage_source: "istanbul",
    coverage_pct: 50,
  });
});

test("seedCensusUnit seeds an unattributed unit with a null rationale", () => {
  for (const coverageSource of ["estimated", "none", undefined]) {
    const unit = { name: "<arrow>", line: 9, col: 2, coverage_source: coverageSource };

    assert.deepEqual(seedCensusUnit(unit), {
      name: "<arrow>",
      line: 9,
      col: 2,
      coverage_source: "estimated",
      rationale: null,
    });
  }
});

test("main rejects an unknown argument before it reads the corpus", async () => {
  const output = [];

  await assert.rejects(
    main(["--nope"], (text) => output.push(text)),
    /unknown argument: --nope/,
  );
  assert.deepEqual(output, []);
});
