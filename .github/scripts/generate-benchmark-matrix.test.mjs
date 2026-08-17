import assert from "node:assert/strict";
import { test } from "node:test";

import {
  allFastTargets,
  changedFilesFromEnvironment,
  selectFastTargets,
} from "./generate-benchmark-matrix.mjs";

const names = (targets) => targets.map((target) => target.bench).toSorted();

test("manual and merge queue runs select every fast benchmark", () => {
  assert.deepEqual(names(selectFastTargets(null)), names(allFastTargets()));
});

test("global benchmark files select every fast benchmark", () => {
  assert.deepEqual(
    names(selectFastTargets([".github/workflows/bench.yml"])),
    names(allFastTargets()),
  );
  assert.deepEqual(
    names(selectFastTargets(["crates/benchmarks/Cargo.toml"])),
    names(allFastTargets()),
  );
});

test("output-only changes select the output component and API stable surface", () => {
  assert.deepEqual(names(selectFastTargets(["crates/output/src/health.rs"])), [
    "component_output",
    "programmatic_stable",
  ]);
  assert.deepEqual(names(selectFastTargets(["crates/cli/src/cli_report.rs"])), [
    "component_output",
  ]);
  assert.deepEqual(names(selectFastTargets(["crates/cli/src/report/github_summary.rs"])), [
    "component_output",
  ]);
});

test("graph changes select graph-sensitive targets", () => {
  assert.deepEqual(names(selectFastTargets(["crates/graph/src/project.rs"])), [
    "analysis",
    "component_engine",
    "component_graph",
    "programmatic_stable",
  ]);
});

test("component bench file changes select the matching shard", () => {
  assert.deepEqual(names(selectFastTargets(["crates/benchmarks/benches/component_config.rs"])), [
    "component_config",
  ]);
});

test("unrelated files select no benchmark shards", () => {
  assert.deepEqual(names(selectFastTargets(["README.md"])), []);
});

test("changed files can be injected for local tests", () => {
  const files = changedFilesFromEnvironment({
    BENCH_CHANGED_FILES: "crates/output/src/lib.rs\nREADME.md\n",
  });

  assert.deepEqual(files, ["crates/output/src/lib.rs", "README.md"]);
});
