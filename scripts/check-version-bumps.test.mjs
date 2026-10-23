import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { createShipRepo } from "./ship-test-repo.mjs";

const SCRIPT = fileURLToPath(new URL("./check-version-bumps.mjs", import.meta.url));
const CACHE_FILE = "crates/extract/src/cache/types.rs";

const cacheSource = (version) => `pub const CACHE_VERSION: u32 = ${version};\n`;
const schemaSource = (version) =>
  `pub const RUNTIME_COVERAGE_SCHEMA_VERSION: &str = "${version}";\n`;

// `main` and `feat` fork from one commit. `onMain` and `onBranch` each
// add the commits of that side. The clone stays on `main`.
const forkRepo = (t, { base, onMain, onBranch }) => {
  const repo = createShipRepo("check-version-bumps-");
  t.after(repo.cleanup);
  repo.commit("chore: start", base);
  repo.git("switch", "--quiet", "-c", "feat");
  onBranch(repo);
  repo.git("switch", "--quiet", "main");
  onMain(repo);
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

const check = (repo) => runCheck(repo, ["--base", "main", "--head", "feat"]);

test("a cache bump that the base already made fails the check", (t) => {
  const repo = forkRepo(t, {
    base: { [CACHE_FILE]: cacheSource(372) },
    onMain: (r) => r.commit("perf: main cache change", { [CACHE_FILE]: cacheSource(373) }),
    onBranch: (r) => r.commit("perf: branch cache change", { [CACHE_FILE]: cacheSource(373) }),
  });

  const result = check(repo);

  assert.equal(result.status, 1, result.output);
  assert.match(
    result.output,
    /Version bumps of the branch that no longer change the value on main:\n\s+crates\/extract\/src\/cache\/types\.rs: CACHE_VERSION 372 -> 373/u,
  );
});

test("a merge of the base into the branch keeps the equal bump visible", (t) => {
  const repo = forkRepo(t, {
    base: { [CACHE_FILE]: cacheSource(372) },
    onMain: (r) => r.commit("perf: main cache change", { [CACHE_FILE]: cacheSource(373) }),
    onBranch: (r) => r.commit("perf: branch cache change", { [CACHE_FILE]: cacheSource(373) }),
  });
  repo.git("switch", "--quiet", "feat");
  repo.git("merge", "--quiet", "--no-edit", "main");
  repo.git("switch", "--quiet", "main");

  const result = check(repo);

  assert.equal(result.status, 1, result.output);
  assert.match(result.output, /types\.rs: CACHE_VERSION 372 -> 373/u);
});

test("the same removal on both sides passes the check", (t) => {
  const repo = forkRepo(t, {
    base: { "src/check.rs": `${schemaSource(1)}fn main() {}\n` },
    onMain: (r) => r.commit("refactor: main removal", { "src/check.rs": "fn main() {}\n" }),
    onBranch: (r) => r.commit("refactor: branch removal", { "src/check.rs": "fn main() {}\n" }),
  });

  assert.equal(check(repo).status, 0);
});

test("a bump that the branch reverts again passes the check", (t) => {
  const repo = forkRepo(t, {
    base: { [CACHE_FILE]: cacheSource(372) },
    onMain: (r) => r.commit("fix: main change", { "src/other.rs": "fn main() {}\n" }),
    onBranch: (r) => {
      r.commit("perf: branch cache change", { [CACHE_FILE]: cacheSource(373) });
      r.commit("revert: branch cache change", { [CACHE_FILE]: cacheSource(372) });
    },
  });

  assert.equal(check(repo).status, 0);
});

test("a string schema bump that the base already made fails the check", (t) => {
  const repo = forkRepo(t, {
    base: { "src/analyze.rs": schemaSource(1) },
    onMain: (r) => r.commit("feat: main schema change", { "src/analyze.rs": schemaSource(2) }),
    onBranch: (r) => r.commit("feat: branch schema change", { "src/analyze.rs": schemaSource(2) }),
  });

  const result = check(repo);

  assert.equal(result.status, 1, result.output);
  assert.match(result.output, /src\/analyze\.rs: RUNTIME_COVERAGE_SCHEMA_VERSION "1" -> "2"/u);
});

test("a bump above the base value passes the check", (t) => {
  const repo = forkRepo(t, {
    base: { [CACHE_FILE]: cacheSource(372) },
    onMain: (r) => r.commit("perf: main cache change", { [CACHE_FILE]: cacheSource(373) }),
    onBranch: (r) => r.commit("perf: branch cache change", { [CACHE_FILE]: cacheSource(374) }),
  });

  const result = check(repo);

  assert.equal(result.status, 0, result.output);
  assert.match(result.output, /each bump still changes the value on main/u);
});

test("a bump that the base did not make passes the check", (t) => {
  const repo = forkRepo(t, {
    base: { [CACHE_FILE]: cacheSource(372), "src/other.rs": "fn main() {}\n" },
    onMain: (r) => r.commit("fix: main change", { "src/other.rs": "fn main() { }\n" }),
    onBranch: (r) => r.commit("perf: branch cache change", { [CACHE_FILE]: cacheSource(373) }),
  });

  assert.equal(check(repo).status, 0);
});

test("a branch with no version bump passes when the base bumps", (t) => {
  const repo = forkRepo(t, {
    base: { [CACHE_FILE]: cacheSource(372), "src/other.rs": "fn main() {}\n" },
    onMain: (r) => r.commit("perf: main cache change", { [CACHE_FILE]: cacheSource(373) }),
    onBranch: (r) => r.commit("fix: branch change", { "src/other.rs": "fn main() { }\n" }),
  });

  assert.equal(check(repo).status, 0);
});

test("the check needs --base", (t) => {
  const repo = createShipRepo("check-version-bumps-");
  t.after(repo.cleanup);

  const result = runCheck(repo, []);

  assert.equal(result.status, 2);
  assert.match(result.output, /Usage: node scripts\/check-version-bumps\.mjs/u);
});
