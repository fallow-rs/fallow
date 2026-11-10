import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { BENCHMARKED_CACHES } from "./check-perf-label.mjs";
import { createShipRepo } from "./ship-test-repo.mjs";

const SCRIPT = fileURLToPath(new URL("./check-perf-label.mjs", import.meta.url));
const EXTRACT_FILE = "crates/extract/src/cache/types.rs";
const GRAPH_FILE = "crates/graph/src/cache/mod.rs";
const CHURN_FILE = "crates/engine/src/churn.rs";

const extractSource = (version) =>
  `pub(super) const CACHE_VERSION: u32 = ${version};\npub const DUPES_CACHE_VERSION: u32 = 21;\n`;
const graphSource = (version) => `pub const GRAPH_CACHE_VERSION: u32 = ${version};\n`;
const churnSource = (version) => `const CHURN_CACHE_VERSION: u8 = ${version};\n`;

const BASE = {
  [EXTRACT_FILE]: extractSource(432),
  [GRAPH_FILE]: graphSource(110),
  [CHURN_FILE]: churnSource(8),
};

// `feat` forks from `main` and adds the commits of `onBranch`. The clone
// stays on `main`.
const branchRepo = (t, onBranch) => {
  const repo = createShipRepo("check-perf-label-");
  t.after(repo.cleanup);
  repo.commit("chore: start", BASE);
  repo.git("switch", "--quiet", "-c", "feat");
  onBranch(repo);
  repo.git("switch", "--quiet", "main");
  return repo;
};

/** Start the script in the clone and return `{ status, output }`. */
const runCheck = (repo, args) => {
  const result = spawnSync(process.execPath, [SCRIPT, ...args], {
    cwd: repo.work,
    env: repo.env,
    encoding: "utf8",
  });
  return { status: result.status, output: `${result.stdout}${result.stderr}` };
};

const check = (repo, labels) =>
  runCheck(repo, ["--base", "main", "--head", "feat", "--labels-json", JSON.stringify(labels)]);

test("an extraction cache bump without the label fails", (t) => {
  const repo = branchRepo(t, (r) =>
    r.commit("feat: new facts", { [EXTRACT_FILE]: extractSource(433) }),
  );

  const result = check(repo, ["enhancement"]);

  assert.equal(result.status, 1, result.output);
  assert.match(result.output, /crates\/extract\/src\/cache\/types\.rs: CACHE_VERSION 432 -> 433/u);
  assert.match(result.output, /Add the "ci:perf" label/u);
});

test("an extraction cache bump with the label passes", (t) => {
  const repo = branchRepo(t, (r) =>
    r.commit("feat: new facts", { [EXTRACT_FILE]: extractSource(433) }),
  );

  const result = check(repo, ["ci:perf"]);

  assert.equal(result.status, 0, result.output);
});

test("a graph cache bump without the label fails", (t) => {
  const repo = branchRepo(t, (r) =>
    r.commit("perf: graph format", { [GRAPH_FILE]: graphSource(111) }),
  );

  const result = check(repo, []);

  assert.equal(result.status, 1, result.output);
  assert.match(result.output, /GRAPH_CACHE_VERSION 110 -> 111/u);
});

test("a bump of a cache with no benchmark passes without the label", (t) => {
  const repo = branchRepo(t, (r) => r.commit("fix: churn cache", { [CHURN_FILE]: churnSource(9) }));

  const result = check(repo, []);

  assert.equal(result.status, 0, result.output);
});

test("a bump that the branch reverts passes without the label", (t) => {
  const repo = branchRepo(t, (r) => {
    r.commit("feat: new facts", { [EXTRACT_FILE]: extractSource(433) });
    r.commit("revert: new facts", { [EXTRACT_FILE]: extractSource(432) });
  });

  const result = check(repo, []);

  assert.equal(result.status, 0, result.output);
});

test("a branch with no bump passes without the label", (t) => {
  const repo = branchRepo(t, (r) => r.commit("docs: note", { "README.md": "text\n" }));

  const result = check(repo, []);

  assert.equal(result.status, 0, result.output);
});

test("a missing or invalid label list is a usage error", (t) => {
  const repo = branchRepo(t, (r) => r.commit("docs: note", { "README.md": "text\n" }));

  assert.equal(runCheck(repo, ["--base", "main", "--head", "feat"]).status, 2);
  assert.equal(
    runCheck(repo, ["--base", "main", "--head", "feat", "--labels-json", "ci:perf"]).status,
    2,
  );
});

test("a dupes cache bump without the label fails", (t) => {
  const repo = branchRepo(t, (r) =>
    r.commit("perf: dupes format", {
      [EXTRACT_FILE]: extractSource(432).replace(
        "DUPES_CACHE_VERSION: u32 = 21",
        "DUPES_CACHE_VERSION: u32 = 22",
      ),
    }),
  );

  const result = check(repo, []);

  assert.equal(result.status, 1, result.output);
  assert.match(result.output, /DUPES_CACHE_VERSION 21 -> 22/u);
});

test("the label passes next to other labels", (t) => {
  const repo = branchRepo(t, (r) =>
    r.commit("feat: new facts", { [EXTRACT_FILE]: extractSource(433) }),
  );

  const result = check(repo, ["enhancement", "ci:perf", "area: extract"]);

  assert.equal(result.status, 0, result.output);
});

// The check knows each cache by its file and constant name. A move of the
// constant must update the list, or the check stops seeing its bumps.
test("each benchmarked cache constant exists in the repository", () => {
  const root = fileURLToPath(new URL("..", import.meta.url));
  for (const key of BENCHMARKED_CACHES) {
    const [file, name] = key.split("\0");
    const source = readFileSync(join(root, file), "utf8");
    assert.match(source, new RegExp(`const ${name}\\s*:`, "u"), `${name} in ${file}`);
  }
});
