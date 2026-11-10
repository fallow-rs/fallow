#!/usr/bin/env node

// Fail a pull request that changes the format of a benchmarked cache without
// the "ci:perf" label. The Benchmarks workflow runs on a pull request only
// with that label, so without it a cost on the cache load and save paths
// shows only after the merge.

import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

import { branchVersionBumps } from "./check-version-bumps.mjs";
import { runCliMain } from "./cli-main.mjs";
import { formatVersionChange } from "./version-constants.mjs";

export const PERF_LABEL = "ci:perf";

// The cache version constants whose cache the CodSpeed benchmarks load and
// save, as `file\0name`.
export const BENCHMARKED_CACHES = new Set([
  "crates/extract/src/cache/types.rs\0CACHE_VERSION",
  "crates/extract/src/cache/types.rs\0DUPES_CACHE_VERSION",
  "crates/graph/src/cache/mod.rs\0GRAPH_CACHE_VERSION",
]);

const USAGE = `Usage: node scripts/check-perf-label.mjs --base <ref> [--head <ref>] --labels-json <json>

Fail when the commits of <head> that <base> does not contain bump the
version of a benchmarked cache format, and the JSON array <json> of
pull request label names does not hold "${PERF_LABEL}". <head> defaults
to HEAD.`;

/** The bumps of `base..head` that change a benchmarked cache format. */
export const benchmarkedCacheBumps = (cwd, base, head) =>
  branchVersionBumps(cwd, base, head).filter(({ file, name }) =>
    BENCHMARKED_CACHES.has(`${file}\0${name}`),
  );

/** The label names of a JSON array, or `null` when the text is not one. */
const parseLabels = (text) => {
  try {
    const labels = JSON.parse(text);
    return Array.isArray(labels) && labels.every((label) => typeof label === "string")
      ? labels
      : null;
  } catch {
    return null;
  }
};

const main = () => {
  const { values } = parseArgs({
    options: {
      base: { type: "string" },
      head: { type: "string", default: "HEAD" },
      "labels-json": { type: "string" },
    },
  });
  const labels = values["labels-json"] === undefined ? null : parseLabels(values["labels-json"]);
  if (values.base === undefined || labels === null) {
    console.error(USAGE);
    return 2;
  }
  const bumps = benchmarkedCacheBumps(process.cwd(), values.base, values.head);
  if (bumps.length === 0) {
    console.log("Cache formats: the branch changes no benchmarked cache format.");
    return 0;
  }
  if (labels.includes(PERF_LABEL)) {
    console.log(
      `Cache formats: the branch changes a benchmarked cache format and has "${PERF_LABEL}".`,
    );
    return 0;
  }
  console.error(
    [
      "The branch changes the format of a benchmarked cache:",
      ...bumps.map(formatVersionChange),
      `Add the "${PERF_LABEL}" label, so that CodSpeed measures the cache load and save paths before the merge.`,
    ].join("\n"),
  );
  return 1;
};

if (process.argv[1] !== undefined && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  runCliMain(main);
}
