import assert from "node:assert/strict";
import { test } from "node:test";

import {
  benchArgError,
  inlineOption,
  parseNonNegativeInt,
  parseOptions,
  parsePositiveInt,
  readBenchCounts,
} from "./lib/options.mjs";

const spec = {
  "--list": { key: "list", type: "flag" },
  "--out-dir": { key: "outDir", type: "string" },
  "--project": { key: "projects", type: "string", repeat: true },
  "--max": { key: "max", type: "positiveInt" },
};

test("parseOptions reads flags, separate values, inline values and repeats", () => {
  const values = parseOptions(
    ["--list", "--out-dir", "a", "--max=3", "--project", "x", "--project=y"],
    spec,
    { projects: [] },
  );
  assert.deepEqual(values, { projects: ["x", "y"], list: true, outDir: "a", max: 3 });
});

test("parseOptions keeps an equals sign inside an inline value", () => {
  assert.equal(parseOptions(["--out-dir=a=b"], spec).outDir, "a=b");
});

test("parseOptions rejects inherited object keys", () => {
  assert.throws(() => parseOptions(["constructor", "x"], spec), /Unknown argument/);
  assert.throws(() => parseOptions(["toString"], spec), /Unknown argument/);
  assert.throws(() => parseOptions(["__proto__"], spec), /Unknown argument/);
});

test("parseOptions requires a value for a value option", () => {
  assert.throws(() => parseOptions(["--out-dir", "--list"], spec), /Missing value for --out-dir/);
  assert.throws(() => parseOptions(["--out-dir"], spec), /Missing value for --out-dir/);
});

test("parsePositiveInt accepts only whole numbers of 1 or more", () => {
  for (const raw of ["0", "1.5", "abc", "3abc", "", "-1"]) {
    assert.throws(() => parsePositiveInt("--runs", raw), /--runs must be a positive integer/);
  }
  assert.equal(parsePositiveInt("--runs", "3"), 3);
  assert.equal(parsePositiveInt("--runs", 3), 3);
});

test("parseNonNegativeInt accepts zero and rejects fractions", () => {
  assert.equal(parseNonNegativeInt("--warmup", "0"), 0);
  assert.throws(
    () => parseNonNegativeInt("--warmup", "1.5"),
    /--warmup must be a non-negative integer/,
  );
});

test("parseOptions leaves the defaults unchanged and checks numeric values", () => {
  const defaults = { projects: ["a"] };
  parseOptions(["--project", "b"], spec, defaults);
  assert.deepEqual(defaults.projects, ["a"]);
  assert.throws(() => parseOptions(["--max", "1.5"], spec), /--max must be a positive integer/);
});

test("inlineOption reads the first name=value argument", () => {
  assert.equal(inlineOption(["--a=1", "--runs=3", "--runs=4"], "--runs"), "3");
  assert.equal(inlineOption(["--runs"], "--runs"), undefined);
});

test("readBenchCounts uses the defaults and the given counts", () => {
  assert.deepEqual(readBenchCounts(["--runs=3"], { runs: "5", warmup: "2" }), {
    runs: 3,
    warmup: 2,
  });
  assert.deepEqual(readBenchCounts(["--samples=0"], { runs: "1", samples: "4" }), {
    runs: 1,
    samples: 0,
  });
});

test("benchArgError accepts known flags and inline values", () => {
  const known = { flags: ["--synthetic"], values: ["--runs"] };
  assert.equal(benchArgError(["--synthetic", "--runs=3"], known), null);
  assert.equal(
    benchArgError(["--runs", "3"], known),
    "Missing value for --runs: write --runs=VALUE",
  );
  assert.equal(benchArgError(["--synthetic=1"], known), "Unknown argument: --synthetic=1");
  assert.equal(benchArgError(["constructor"], known), "Unknown argument: constructor");
});
