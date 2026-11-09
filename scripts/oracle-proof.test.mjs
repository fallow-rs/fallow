import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import { formatProof, main, ORACLES, proveFix } from "./oracle-proof.mjs";
import { traceResolverSource } from "./oracle-proof/cache-model.mjs";

const git = (cwd, args) => {
  const result = spawnSync("git", args, { cwd, encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  return result.stdout.trim();
};

/** A repository whose second commit removes the file `bug`. Returns its path and the fix commit. */
const repoWithFix = () => {
  const repo = mkdtempSync(join(tmpdir(), "oracle-proof-test-"));
  git(repo, ["init", "--quiet"]);
  git(repo, ["config", "user.email", "test@example.com"]);
  git(repo, ["config", "user.name", "Test"]);
  git(repo, ["config", "commit.gpgsign", "false"]);
  writeFileSync(join(repo, "bug"), "");
  git(repo, ["add", "bug"]);
  git(repo, ["commit", "--quiet", "-m", "add the bug"]);
  git(repo, ["rm", "--quiet", "bug"]);
  git(repo, ["commit", "--quiet", "-m", "fix the bug"]);
  return { repo, fix: git(repo, ["rev-parse", "HEAD"]) };
};

const oracle = (overrides = {}) => ({
  name: "fixture",
  description: "fails while the file bug exists",
  fixes: [],
  port: () => [],
  build: ["sh", ["-c", "exit 0"]],
  test: [
    "sh",
    [
      "-c",
      "test ! -f bug || { echo 'test bug_is_gone ... FAILED'; exit 1; }; echo 'test result: ok. 1 passed'",
    ],
  ],
  ...overrides,
});

const withRepo = (body) => {
  const { repo, fix } = repoWithFix();
  const logs = mkdtempSync(join(tmpdir(), "oracle-proof-logs-test-"));
  try {
    body({ repo, fix, logs });
  } finally {
    rmSync(repo, { recursive: true, force: true });
    rmSync(logs, { recursive: true, force: true });
  }
};

test("the proof holds when the oracle fails on the parent and passes on the fix", () => {
  withRepo(({ repo, fix, logs }) => {
    const proof = proveFix(oracle(), fix, { source: repo, logs });

    assert.equal(proof.holds, true);
    assert.equal(proof.toolError, false);
    assert.deepEqual(
      proof.runs.map((run) => run.result),
      ["fail", "pass"],
    );
    assert.match(formatProof(oracle(), proof), /proof holds/);
    assert.match(formatProof(oracle(), proof), /\| test bug_is_gone \.\.\. FAILED/);
  });
});

test("the worktrees are removed after the run", () => {
  withRepo(({ repo, fix, logs }) => {
    const proof = proveFix(oracle(), fix, { source: repo, logs });

    for (const run of proof.runs) assert.equal(existsSync(run.tree), false, run.tree);
    assert.equal(git(repo, ["worktree", "list", "--porcelain"]).match(/^worktree /gm).length, 1);
  });
});

test("--keep leaves the worktrees for inspection", () => {
  withRepo(({ repo, fix, logs }) => {
    const proof = proveFix(oracle(), fix, { source: repo, logs, keep: true });

    for (const run of proof.runs) assert.equal(existsSync(run.tree), true, run.tree);
    for (const run of proof.runs) git(repo, ["worktree", "remove", "--force", run.tree]);
  });
});

test("the proof does not hold when the oracle passes on the parent", () => {
  withRepo(({ repo, fix, logs }) => {
    const weak = oracle({ test: ["sh", ["-c", "echo 'test result: ok. 1 passed'"]] });
    const proof = proveFix(weak, fix, { source: repo, logs });

    assert.equal(proof.holds, false);
    assert.match(formatProof(weak, proof), /proof does not hold/);
  });
});

test("a pass without a test run is a tool error, not a proof", () => {
  withRepo(({ repo, fix, logs }) => {
    // A test filter that matches no test exits 0, as cargo does.
    const empty = oracle({
      test: ["sh", ["-c", "test ! -f bug || exit 1; echo 'test result: ok. 0 passed; 0 failed'"]],
    });
    const proof = proveFix(empty, fix, { source: repo, logs });

    assert.equal(proof.toolError, true);
    assert.equal(proof.holds, false);
    assert.deepEqual(
      proof.runs.map((run) => run.result),
      ["fail", "no-tests"],
    );
  });
});

test("a build failure is a tool error, not a failing oracle", () => {
  withRepo(({ repo, fix, logs }) => {
    const broken = oracle({ build: ["sh", ["-c", "exit 101"]] });
    const proof = proveFix(broken, fix, { source: repo, logs });

    assert.equal(proof.toolError, true);
    assert.equal(proof.holds, false);
    assert.deepEqual(
      proof.runs.map((run) => run.result),
      ["build-error", "build-error"],
    );
  });
});

test("a backport applies to each commit where it is missing and shows in the report", () => {
  withRepo(({ repo, fix, logs }) => {
    const backported = oracle({
      backports: [
        {
          name: "the later fix",
          applies: (tree) => existsSync(join(tree, "bug")),
          apply: (tree) => rmSync(join(tree, "bug")),
        },
      ],
    });
    const proof = proveFix(backported, fix, { source: repo, logs });

    assert.deepEqual(
      proof.runs.map((run) => [run.result, run.notes]),
      [
        ["pass", ["backported the later fix"]],
        ["pass", []],
      ],
    );
  });
});

test("the command line rejects an unknown oracle and a fix without a parent", () => {
  withRepo(({ repo, logs }) => {
    const root = git(repo, ["rev-list", "--max-parents=0", "HEAD"]);
    const errors = [];
    const original = console.error;
    console.error = (message) => errors.push(message);
    try {
      assert.equal(main(["no-such-oracle"]), 2);
      assert.equal(main(["cache-model", root, "--source", repo, "--logs", logs]), 2);
      assert.equal(main(["cache-model", "--source"]), 2);
    } finally {
      console.error = original;
    }
    assert.match(errors.join("\n"), /unknown oracle no-such-oracle/);
    assert.match(errors.join("\n"), /tool error/);
  });
});

test("each registered oracle names at least one fix commit", () => {
  for (const registered of Object.values(ORACLES)) {
    assert.notEqual(registered.fixes.length, 0, registered.name);
    for (const fix of registered.fixes)
      assert.match(fix.commit, /^[0-9a-f]{7,40}$/, registered.name);
  }
});

test("the read-trace port routes resolver calls through the trace and leaves tests alone", () => {
  const source = [
    "use std::path::Path;",
    "use oxc_resolver::{ResolveOptions, Resolver, ResolverGeneric};",
    "",
    "fn probe(path: &Path, resolver: oxc_resolver::Resolver) -> bool {",
    "    let _ = std::fs::read_to_string(path);",
    "    let _ = dunce::canonicalize(path);",
    "    path.is_file() || path.is_dir() || path.exists()",
    "}",
    "#[cfg(test)]",
    "mod tests {",
    "    fn helper(path: &Path) -> bool { path.is_file() }",
    "}",
  ].join("\n");

  const traced = traceResolverSource(source);

  assert.match(traced, /^use oxc_resolver::\{ResolveOptions, ResolverGeneric\};$/m);
  assert.match(traced, /^use crate::read_trace::Resolver;$/m);
  assert.equal(traced.match(/use crate::read_trace::TracedPath;/g).length, 1);
  assert.match(traced, /crate::read_trace::read_to_string\(path\)/);
  assert.match(traced, /crate::read_trace::canonicalize\(path\)/);
  assert.match(
    traced,
    /path\.traced_is_file\(\) \|\| path\.traced_is_dir\(\) \|\| path\.traced_exists\(\)/,
  );
  assert.match(traced, /fn helper\(path: &Path\) -> bool \{ path\.is_file\(\) \}/);
  assert.equal(traceResolverSource(traced), traced, "the port is idempotent");
});

test("a scoped fix runs its own test command and env, and the report names the scope", () => {
  withRepo(({ repo, fix, logs }) => {
    const scoped = {
      commit: fix,
      scope: "the bug test",
      reason: "the other tests need a later fix",
      test: [
        "sh",
        ["-c", 'test "$ORACLE_SCOPE" = bug && test ! -f bug && echo "test result: ok. 1 passed"'],
      ],
      env: { ORACLE_SCOPE: "bug" },
    };
    const proof = proveFix(oracle({ test: ["sh", ["-c", "exit 1"]] }), scoped, {
      source: repo,
      logs,
    });

    assert.equal(proof.holds, true);
    assert.match(
      formatProof(oracle(), proof),
      /scope: the bug test \(the other tests need a later fix\)/,
    );
  });
});

test("a run that a signal stops is a tool error, not a failing oracle", () => {
  withRepo(({ repo, fix, logs }) => {
    const killed = oracle({
      test: ["sh", ["-c", "test ! -f bug || kill -9 $$; echo 'test result: ok. 1 passed'"]],
    });
    const proof = proveFix(killed, fix, { source: repo, logs });

    assert.deepEqual(
      proof.runs.map((run) => run.result),
      ["killed", "pass"],
    );
    assert.equal(proof.toolError, true);
    assert.match(formatProof(killed, proof), /tool error/);
  });
});

test("a build failure shows the compiler errors in the report", () => {
  withRepo(({ repo, fix, logs }) => {
    const broken = oracle({
      build: ["sh", ["-c", "echo 'error[E0425]: cannot find value `x`'; exit 101"]],
    });
    const proof = proveFix(broken, fix, { source: repo, logs });

    assert.match(formatProof(broken, proof), /\| error\[E0425\]: cannot find value `x`/);
  });
});

test("the worktree is removed when the port step throws", () => {
  withRepo(({ repo, fix, logs }) => {
    const failing = oracle({
      port: () => {
        throw new Error("the port anchor is missing");
      },
    });

    assert.throws(() => proveFix(failing, fix, { source: repo, logs }), /port anchor is missing/);
    assert.equal(git(repo, ["worktree", "list", "--porcelain"]).match(/^worktree /gm).length, 1);
  });
});
