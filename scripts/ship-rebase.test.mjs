import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { chmodSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { delimiter, join, resolve } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { takeBaseCacheVersions } from "./ship-rebase.mjs";
import {
  diffHunks,
  ownHunkProblems,
  pullRequestHead,
  pushSshEnv,
  shellCommand,
} from "./ship-git.mjs";
import { createShipRepo } from "./ship-test-repo.mjs";

const SCRIPT = fileURLToPath(new URL("./ship-rebase.mjs", import.meta.url));

const cacheSource = (version) => `pub const GRAPH_CACHE_VERSION: u32 = ${version};\n`;
const schemaSource = (version) => `pub const CHECK_SCHEMA_VERSION: u32 = ${version};\n`;

// `main` and `feat` fork from one commit. `onMain` and `onBranch` each
// return the commits of that side.
const forkRepo = (t, { base, onMain, onBranch }) => {
  const repo = createShipRepo("ship-rebase-");
  t.after(repo.cleanup);
  repo.commit("chore: start", base);
  repo.git("push", "--quiet", "origin", "main");
  repo.git("switch", "--quiet", "-c", "feat");
  onBranch(repo);
  repo.git("push", "--quiet", "origin", "feat");
  repo.git("switch", "--quiet", "main");
  onMain(repo);
  repo.git("push", "--quiet", "origin", "main");
  return repo;
};

const shipRebase = (repo, ...args) => repo.runNode(SCRIPT, ["--branch", "feat", ...args]);

const fileAt = (repo, commit, path) => repo.git("show", `${commit}:${path}`);

/** The full hash of the rebase result that the script printed. */
const newTipOf = (result) => {
  const match = result.output.match(/new tip ([0-9a-f]{40})\./u);
  assert.notEqual(match, null, result.output);
  return match[1];
};

/** The path of the temporary worktree that the script kept. */
const keptWorktreeOf = (result) => {
  const match = result.output.match(/The temporary worktree (\S+) holds/u);
  assert.notEqual(match, null, result.output);
  return match[1];
};

/** The paths of all worktrees of the fixture repository. */
const worktreePaths = (repo) =>
  repo
    .git("worktree", "list", "--porcelain")
    .split("\n")
    .filter((line) => line.startsWith("worktree "))
    .map((line) => line.slice("worktree ".length));

const rebaseInProgressIn = (repo, dir) =>
  ["rebase-merge", "rebase-apply"].some((name) =>
    existsSync(resolve(dir, repo.git("-C", dir, "rev-parse", "--git-path", name))),
  );

/** The branch, the HEAD commit, the status and the rebase state of the clone. */
const callerState = (repo) => ({
  branch: repo.git("branch", "--show-current"),
  head: repo.git("rev-parse", "HEAD"),
  status: repo.git("status", "--porcelain=v1", "--untracked-files=all"),
  rebase: rebaseInProgressIn(repo, repo.work),
});

test("a CHANGELOG.md conflict stops the rebase", (t) => {
  const repo = forkRepo(t, {
    base: { "CHANGELOG.md": "# Changelog\n" },
    onMain: (r) =>
      r.commit("chore: release v1.1.0", { "CHANGELOG.md": "# Changelog\n\n## 1.1.0\n" }),
    onBranch: (r) =>
      r.commit("docs: fix a release note", { "CHANGELOG.md": "# Changelog\n\nNote.\n" }),
  });

  const result = shipRebase(repo);

  assert.equal(result.status, 1, result.output);
  assert.match(
    result.output,
    /STOP: feat: Conflicts outside generated files and cache versions: CHANGELOG\.md/u,
  );
  assert.match(readFileSync(join(keptWorktreeOf(result), "CHANGELOG.md"), "utf8"), /^<{7} /mu);
});

test("a clean rebase reports the tree check without excluded paths", (t) => {
  const repo = divergedRepo(t);

  const result = shipRebase(repo);

  assert.equal(result.status, 0, result.output);
  assert.match(result.output, /\nTree check: the result matches a merge into origin\/main\.\n/u);
});

test("a conflict in an ordinary file stops the rebase", (t) => {
  const repo = forkRepo(t, {
    base: { "src/value.txt": "base\n" },
    onMain: (r) => r.commit("fix: main value", { "src/value.txt": "main\n" }),
    onBranch: (r) => r.commit("fix: branch value", { "src/value.txt": "branch\n" }),
  });

  const result = shipRebase(repo);

  assert.equal(result.status, 1, result.output);
  assert.match(
    result.output,
    /STOP: feat: Conflicts outside generated files and cache versions: src\/value\.txt/u,
  );
  const kept = keptWorktreeOf(result);
  assert.match(readFileSync(join(kept, "src/value.txt"), "utf8"), /^<{7} /mu);
  assert.equal(rebaseInProgressIn(repo, kept), true);
});

test("a cache bump that the base already made gets the base value + 1", (t) => {
  const repo = forkRepo(t, {
    base: { "src/lib.rs": cacheSource(1) },
    onMain: (r) => r.commit("fix: main cache change", { "src/lib.rs": cacheSource(2) }),
    onBranch: (r) =>
      r.commit("fix: branch cache change", {
        "src/lib.rs": cacheSource(2),
        "src/other.txt": "other\n",
      }),
  });

  const result = shipRebase(repo);

  assert.equal(result.status, 0, result.output);
  const tip = newTipOf(result);
  assert.equal(fileAt(repo, tip, "src/lib.rs"), cacheSource(3).trimEnd());
  assert.equal(
    repo.git("log", "-1", "--format=%s", tip),
    "chore: bump cache versions above the base branch",
  );
  // The Commit messages workflow and the commit-msg hook need a DCO sign-off.
  assert.match(
    repo.git("log", "-1", "--format=%(trailers:key=Signed-off-by)", tip),
    /^Signed-off-by: .+ <.+>/mu,
  );
  assert.match(result.output, /src\/lib\.rs: GRAPH_CACHE_VERSION set to the base value \+ 1/u);
});

test("a conflict between two cache bumps takes the base value + 1", (t) => {
  const repo = forkRepo(t, {
    base: { "src/lib.rs": cacheSource(1) },
    onMain: (r) => {
      r.commit("fix: first main cache change", { "src/lib.rs": cacheSource(2) });
      r.commit("fix: second main cache change", { "src/lib.rs": cacheSource(3) });
    },
    onBranch: (r) =>
      r.commit("fix: branch cache change", {
        "src/lib.rs": cacheSource(2),
        "src/other.txt": "other\n",
      }),
  });

  const result = shipRebase(repo);

  assert.equal(result.status, 0, result.output);
  assert.equal(fileAt(repo, newTipOf(result), "src/lib.rs"), cacheSource(4).trimEnd());
  assert.match(result.output, /GRAPH_CACHE_VERSION 3 -> 4/u);
  assert.match(result.output, /Resolved conflicts at 1 stops of the rebase\./u);
});

test("takeBaseCacheVersions keeps the base side of pure cache version blocks only", () => {
  const block = (ours, theirs) => `<<<<<<< HEAD\n${ours}=======\n${theirs}>>>>>>> abc (feat)\n`;
  assert.equal(
    takeBaseCacheVersions(`// top\n${block(cacheSource(3), cacheSource(2))}// end\n`),
    `// top\n${cacheSource(3)}// end\n`,
  );
  assert.equal(takeBaseCacheVersions(block(cacheSource(3), schemaSource(2))), null);
  assert.equal(takeBaseCacheVersions(block("let a = 1;\n", "let a = 2;\n")), null);
  assert.equal(takeBaseCacheVersions(cacheSource(1)), null, "no conflict block");
});

test("a conflict in a generated file takes the base side and asks for a regeneration", (t) => {
  const repo = forkRepo(t, {
    base: { "docs/output-schema.json": '{"v":1}\n' },
    onMain: (r) => r.commit("feat: main schema", { "docs/output-schema.json": '{"v":2}\n' }),
    onBranch: (r) =>
      r.commit("feat: branch schema", {
        "docs/output-schema.json": '{"v":3}\n',
        "src/other.txt": "other\n",
      }),
  });

  const result = shipRebase(repo, "--push");

  assert.equal(result.status, 1, result.output);
  assert.match(
    result.output,
    /Generated files that took the base side and need a regeneration:\n\s+docs\/output-schema\.json/u,
  );
  assert.match(result.output, /npm run generate:contracts/u);
  const kept = keptWorktreeOf(result);
  assert.equal(readFileSync(join(kept, "docs/output-schema.json"), "utf8"), '{"v":2}\n');
  assert.equal(readFileSync(join(kept, "src/other.txt"), "utf8"), "other\n");
  assert.notEqual(repo.git("rev-parse", "origin/feat"), repo.git("-C", kept, "rev-parse", "HEAD"));
  assert.doesNotMatch(result.output, /Pushed/u);
});

test("a generated file conflict with a lost version bump prints no push command", (t) => {
  const repo = forkRepo(t, {
    base: { "docs/output-schema.json": '{"v":1}\n', "src/check.rs": schemaSource(1) },
    onMain: (r) =>
      r.commit("feat: main schema", {
        "docs/output-schema.json": '{"v":2}\n',
        "src/check.rs": schemaSource(2),
      }),
    onBranch: (r) =>
      r.commit("feat: branch schema", {
        "docs/output-schema.json": '{"v":3}\n',
        "src/check.rs": schemaSource(2),
      }),
  });

  const result = shipRebase(repo, "--push");

  assert.equal(result.status, 1, result.output);
  assert.match(result.output, /Generated files that took the base side/u);
  assert.match(result.output, /Version changes that the rebase lost/u);
  assert.match(result.output, /Fix the other problems first\./u);
  assert.doesNotMatch(result.output, /npm run generate:contracts/u);
  assert.doesNotMatch(result.output, /Pushed/u);
});

test("a generated file conflict with a tree check difference prints no push command", (t) => {
  const repo = mergedRepo(t, (r) => r.write("src/b.txt", "b edited in the merge\n"));
  repo.git("switch", "--quiet", "main");
  repo.commit("feat: main schema", { "docs/output-schema.json": '{"v":2}\n' });
  repo.git("push", "--quiet", "origin", "main");
  repo.git("switch", "--quiet", "feat");
  repo.commit("feat: branch schema", { "docs/output-schema.json": '{"v":3}\n' });
  repo.git("push", "--quiet", "origin", "feat");
  repo.git("switch", "--quiet", "main");

  const result = shipRebase(repo, "--push");

  assert.equal(result.status, 1, result.output);
  assert.match(result.output, /Generated files that took the base side/u);
  assert.match(result.output, /Paths where the rebase result differs from a merge/u);
  assert.match(result.output, /Fix the other problems first\./u);
  assert.doesNotMatch(result.output, /npm run generate:contracts/u);
  assert.doesNotMatch(result.output, /Pushed/u);
});

test("a conflict with other lines next to a cache version stops the rebase", (t) => {
  const repo = forkRepo(t, {
    base: { "src/lib.rs": cacheSource(1) },
    onMain: (r) =>
      r.commit("fix: main cache change", { "src/lib.rs": `${cacheSource(2)}const A: u8 = 1;\n` }),
    onBranch: (r) =>
      r.commit("fix: branch cache change", { "src/lib.rs": `${cacheSource(2)}const A: u8 = 2;\n` }),
  });

  const result = shipRebase(repo);

  assert.equal(result.status, 1, result.output);
  assert.match(result.output, /cache versions: src\/lib\.rs/u);
});

test("the cache version check reads constants of every unsigned integer type", (t) => {
  const source = (version) =>
    `pub(super) const AUDIT_BASE_SNAPSHOT_CACHE_VERSION: u8 = ${version};\n`;
  const repo = forkRepo(t, {
    base: { "src/audit.rs": source(1) },
    onMain: (r) => r.commit("fix: main cache change", { "src/audit.rs": source(2) }),
    onBranch: (r) =>
      r.commit("fix: branch cache change", {
        "src/audit.rs": source(2),
        "src/other.txt": "other\n",
      }),
  });

  const result = shipRebase(repo);

  assert.equal(result.status, 0, result.output);
  assert.equal(fileAt(repo, newTipOf(result), "src/audit.rs"), source(3).trimEnd());
});

// `feat` and `main` each change a file, and then `feat` merges `main`.
// `inMerge` changes the work tree before the merge commit.
const mergedRepo = (t, inMerge) => {
  const repo = forkRepo(t, {
    base: { "src/b.txt": "b\n" },
    onMain: (r) => r.commit("feat: main change", { "src/main.txt": "main\n" }),
    onBranch: (r) => r.commit("feat: branch change", { "src/branch.txt": "branch\n" }),
  });
  repo.git("switch", "--quiet", "feat");
  repo.git("merge", "--quiet", "--no-ff", "--no-commit", "main");
  inMerge(repo);
  repo.git("add", "--all");
  repo.git("commit", "--quiet", "--no-edit");
  repo.git("push", "--quiet", "origin", "feat");
  repo.git("switch", "--quiet", "main");
  repo.commit("fix: later main change", { "src/c.txt": "c\n" });
  repo.git("push", "--quiet", "origin", "main");
  return repo;
};

test("the tree check finds a change that the branch made in a merge commit", (t) => {
  const repo = mergedRepo(t, (r) => r.write("src/b.txt", "b edited in the merge\n"));

  const result = shipRebase(repo, "--push");

  assert.equal(result.status, 1, result.output);
  assert.match(
    result.output,
    /Paths where the rebase result differs from a merge of the branch into origin\/main:\n\s+src\/b\.txt/u,
  );
  assert.match(result.output, /The branch has 1 merge commits\./u);
  assert.equal(fileAt(repo, newTipOf(result), "src/b.txt"), "b");
  assert.doesNotMatch(result.output, /Pushed/u);
});

// `main` got the first commit of the branch as a separate commit, so the
// rebase skips it. The merge sees two different values for one line.
// `onBranch` adds more commits after the two value commits.
const skippedCommitRepo = (t, onBranch) =>
  forkRepo(t, {
    base: { "src/value.txt": "1\n" },
    onMain: (r) => r.commit("fix: value 2 on main", { "src/value.txt": "2\n" }),
    onBranch: (r) => {
      r.commit("fix: value 2", { "src/value.txt": "2\n" });
      r.commit("fix: value 3", { "src/value.txt": "3\n" });
      onBranch(r);
    },
  });

test("the tree check passes a path where only the merge conflicts", (t) => {
  const repo = skippedCommitRepo(t, () => {});

  const result = shipRebase(repo);

  assert.equal(result.status, 0, result.output);
  assert.equal(fileAt(repo, newTipOf(result), "src/value.txt"), "3");
  assert.match(
    result.output,
    /the merge conflicts at these paths, so the check does not compare them\. The branch has no merge commits:\n\s+src\/value\.txt\n/u,
  );
});

test("the tree check fails a path where the merge conflicts on a branch with a merge commit", (t) => {
  const repo = skippedCommitRepo(t, (r) => {
    r.git("switch", "--quiet", "-c", "side", "main");
    r.commit("feat: side change", { "src/side.txt": "side\n" });
    r.git("switch", "--quiet", "feat");
    r.git("merge", "--quiet", "--no-ff", "--no-edit", "side");
  });

  const result = shipRebase(repo);

  assert.equal(result.status, 1, result.output);
  assert.match(
    result.output,
    /\n\s+src\/value\.txt \(the merge conflicts here, so compare it by hand\)\n\s+The branch has 1 merge commits\./u,
  );
});

test("a branch with a clean merge commit passes the tree check", (t) => {
  const repo = mergedRepo(t, () => {});

  const result = shipRebase(repo);

  assert.equal(result.status, 0, result.output);
  assert.equal(fileAt(repo, newTipOf(result), "src/main.txt"), "main");
  assert.equal(fileAt(repo, newTipOf(result), "src/branch.txt"), "branch");
});

test("shellCommand quotes each argument that the shell would read", () => {
  const args = ["push", "--force-with-lease=refs/heads/a$b;c:1f", "origin", "HEAD:refs/heads/it's"];
  const command = shellCommand(args);

  assert.equal(
    command,
    "push '--force-with-lease=refs/heads/a$b;c:1f' origin 'HEAD:refs/heads/it'\\''s'",
  );
  const echoed = execFileSync("sh", ["-c", `printf '%s\\n' ${command}`], { encoding: "utf8" });
  assert.deepEqual(echoed.trimEnd().split("\n"), args);
});

test("the rebase ignores a recorded rerere resolution", (t) => {
  const repo = forkRepo(t, {
    base: { "src/value.txt": "base\n" },
    onMain: (r) => r.commit("fix: main value", { "src/value.txt": "main\n" }),
    onBranch: (r) =>
      r.commit("fix: branch value", {
        "src/value.txt": "branch\n",
        "src/other.rs": "pub const OTHER: u32 = 1;\n",
      }),
  });
  repo.git("config", "rerere.enabled", "true");
  repo.git("config", "rerere.autoUpdate", "true");
  // Record a resolution that keeps the value of main, as a resolution of
  // another pull request can do.
  repo.git("switch", "--quiet", "--detach", "origin/feat");
  assert.throws(() => repo.git("rebase", "origin/main"));
  repo.write("src/value.txt", "main\n");
  repo.git("add", "src/value.txt");
  repo.git("-c", "core.editor=true", "rebase", "--continue");
  // Plain git now reuses the resolution without a conflict marker.
  repo.git("switch", "--quiet", "--detach", "origin/feat");
  assert.throws(() => repo.git("rebase", "origin/main"));
  assert.equal(readFileSync(join(repo.work, "src/value.txt"), "utf8"), "main\n");
  repo.git("rebase", "--abort");
  repo.git("switch", "--quiet", "main");

  const result = shipRebase(repo);

  assert.equal(result.status, 1, result.output);
  assert.match(result.output, /cache versions: src\/value\.txt/u);
  assert.match(readFileSync(join(keptWorktreeOf(result), "src/value.txt"), "utf8"), /^<{7} /mu);
});

test("--push updates the branch with a lease on the old tip", (t) => {
  const repo = divergedRepo(t);

  const result = shipRebase(repo, "--push");

  assert.equal(result.status, 0, result.output);
  assert.match(
    result.output,
    /Pushing feat\. The pre-push hook of the checkout runs first\.\nPushed feat with a lease/u,
  );
  repo.git("fetch", "--quiet", "origin");
  assert.equal(repo.git("rev-parse", "origin/feat"), newTipOf(result));
  assert.match(
    result.output,
    new RegExp(
      `\\n  node scripts/ship-wait-checks\\.mjs --pr <number> --head ${newTipOf(result)}\\n`,
      "u",
    ),
  );
});

test("--pr --push prints a wait command with the pull request number", (t) => {
  const repo = divergedRepo(t);

  const result = repo.runNode(SCRIPT, ["--pr", "12", "--push"], sameRepoGh(repo));

  assert.equal(result.status, 0, result.output);
  assert.match(
    result.output,
    new RegExp(
      `\\n  node scripts/ship-wait-checks\\.mjs --pr 12 --head ${newTipOf(result)}\\n`,
      "u",
    ),
  );
});

test("--push fails when the remote branch moved after the fetch", (t) => {
  const repo = divergedRepo(t);
  // Someone else pushes to `feat` between the fetch and the push of the
  // script. The script adds a worktree at the fetched tip after the fetch,
  // so a post-checkout hook that runs one time moves the remote branch.
  repo.git("switch", "--quiet", "--detach", "origin/feat");
  const other = repo.commit("feat: other change", { "src/other.txt": "other\n" });
  repo.git("push", "--quiet", "origin", "HEAD:refs/heads/other");
  repo.git("switch", "--quiet", "main");
  const origin = join(repo.root, "origin.git");
  const hook = join(repo.work, ".git", "hooks", "post-checkout");
  writeFileSync(
    hook,
    [
      "#!/bin/sh",
      'marker="$(git rev-parse --git-dir)/moved"',
      '[ -e "$marker" ] && exit 0',
      'touch "$marker"',
      "unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE",
      `git --git-dir="${origin}" update-ref refs/heads/feat ${other}`,
      "",
    ].join("\n"),
  );
  chmodSync(hook, 0o755);

  const result = shipRebase(repo, "--push");

  assert.equal(result.status, 2, result.output);
  assert.match(result.output, /stale info/u);
  assert.equal(repo.git("--git-dir", origin, "rev-parse", "refs/heads/feat"), other);
  const kept = keptWorktreeOf(result);
  assert.equal(repo.git("-C", kept, "rev-parse", "HEAD"), newTipOf(result));
  assert.match(result.output, new RegExp(`git -C ${kept} push --force-with-lease=`, "u"));
});

// `main` and `feat` each change another file, so the rebase has no conflict.
const divergedRepo = (t) =>
  forkRepo(t, {
    base: { "src/base.txt": "base\n" },
    onMain: (r) => r.commit("feat: main change", { "src/main.txt": "main\n" }),
    onBranch: (r) => r.commit("feat: branch change", { "src/branch.txt": "branch\n" }),
  });

test("a passing run keeps the caller checkout and removes its worktree", (t) => {
  const repo = divergedRepo(t);
  const before = callerState(repo);

  const result = shipRebase(repo);

  assert.equal(result.status, 0, result.output);
  assert.deepEqual(callerState(repo), before);
  assert.equal(before.branch, "main");
  assert.deepEqual(worktreePaths(repo), [repo.work]);
  assert.doesNotMatch(result.output, /HEAD is detached/u);
});

test("a conflict stop keeps the caller checkout and leaves the rebase in a kept worktree", (t) => {
  const repo = forkRepo(t, {
    base: { "src/value.txt": "base\n" },
    onMain: (r) => r.commit("fix: main value", { "src/value.txt": "main\n" }),
    onBranch: (r) => r.commit("fix: branch value", { "src/value.txt": "branch\n" }),
  });
  const before = callerState(repo);

  const result = shipRebase(repo);

  assert.equal(result.status, 1, result.output);
  assert.deepEqual(callerState(repo), before);
  const kept = keptWorktreeOf(result);
  assert.deepEqual(worktreePaths(repo), [repo.work, kept]);
  assert.equal(rebaseInProgressIn(repo, kept), true);
  assert.match(result.output, new RegExp(`git -C ${kept} rebase --continue`, "u"));
  assert.match(result.output, new RegExp(`git -C ${kept} push --force-with-lease=`, "u"));
  assert.match(result.output, new RegExp(`git -C ${kept} rebase --abort`, "u"));
  assert.match(result.output, new RegExp(`worktree remove --force ${kept}`, "u"));
});

test("a failed check keeps the caller checkout and the rebased HEAD in a kept worktree", (t) => {
  const repo = forkRepo(t, {
    base: { "src/check.rs": schemaSource(1) },
    onMain: (r) => r.commit("feat: main schema change", { "src/check.rs": schemaSource(2) }),
    onBranch: (r) =>
      r.commit("feat: branch schema change", {
        "src/check.rs": schemaSource(2),
        "src/other.txt": "other\n",
      }),
  });
  const before = callerState(repo);

  const result = shipRebase(repo, "--push");

  assert.equal(result.status, 1, result.output);
  assert.match(result.output, /Version changes that the rebase lost/u);
  assert.deepEqual(callerState(repo), before);
  const kept = keptWorktreeOf(result);
  assert.equal(repo.git("-C", kept, "rev-parse", "HEAD"), newTipOf(result));
  assert.equal(repo.git("-C", kept, "rev-parse", "HEAD^"), repo.git("rev-parse", "origin/main"));
  assert.equal(rebaseInProgressIn(repo, kept), false);
  assert.match(result.output, new RegExp(`worktree remove --force ${kept}`, "u"));
});

test("the run does not touch an unrelated branch with uncommitted changes", (t) => {
  const repo = divergedRepo(t);
  repo.git("switch", "--quiet", "-c", "unrelated");
  repo.commit("chore: unrelated work", { "src/notes.txt": "notes\n" });
  repo.write("src/notes.txt", "edited notes\n");
  repo.write("src/staged.txt", "staged edit\n");
  repo.git("add", "src/staged.txt");
  repo.write("src/draft.txt", "untracked\n");
  const before = callerState(repo);

  const result = shipRebase(repo, "--push");

  assert.equal(result.status, 0, result.output);
  assert.deepEqual(callerState(repo), before);
  assert.equal(readFileSync(join(repo.work, "src/notes.txt"), "utf8"), "edited notes\n");
  assert.equal(repo.git("diff", "--cached", "--name-only"), "src/staged.txt");
  repo.git("fetch", "--quiet", "origin");
  assert.equal(repo.git("rev-parse", "origin/feat"), newTipOf(result));
  assert.deepEqual(worktreePaths(repo), [repo.work]);
});

test("--push runs the pre-push hook of a relative core.hooksPath", (t) => {
  const repo = divergedRepo(t);
  const log = join(repo.root, "pre-push.log");
  mkdirSync(join(repo.work, "hooks"));
  mkdirSync(join(repo.work, "target"));
  writeFileSync(
    join(repo.work, "hooks", "pre-push"),
    `#!/bin/sh\nprintf '%s\\n' "$CARGO_TARGET_DIR" > "${log}"\n`,
  );
  chmodSync(join(repo.work, "hooks", "pre-push"), 0o755);
  repo.git("config", "core.hooksPath", "hooks");

  const result = shipRebase(repo, "--push");

  assert.equal(result.status, 0, result.output);
  assert.equal(readFileSync(log, "utf8"), `${join(repo.work, "target")}\n`);
});

test("the rebase is linear when rebase.rebaseMerges is set", (t) => {
  const repo = mergedRepo(t, () => {});
  repo.git("config", "rebase.rebaseMerges", "true");

  const result = shipRebase(repo);

  assert.equal(result.status, 0, result.output);
  assert.equal(repo.git("rev-list", "--merges", `origin/main..${newTipOf(result)}`), "");
});

test("the rebase moves no local branch when rebase.updateRefs is set", (t) => {
  const repo = forkRepo(t, {
    base: { "src/base.txt": "base\n" },
    onMain: (r) => r.commit("feat: main change", { "src/main.txt": "main\n" }),
    onBranch: (r) => {
      r.commit("feat: first change", { "src/a.txt": "a\n" });
      r.git("branch", "part");
      r.commit("feat: branch change", { "src/branch.txt": "branch\n" });
    },
  });
  const part = repo.git("rev-parse", "part");
  repo.git("config", "rebase.updateRefs", "true");

  const result = shipRebase(repo);

  assert.equal(result.status, 0, result.output);
  assert.equal(repo.git("rev-parse", "part"), part);
});

test("the script ignores an inherited GIT_DIR and GIT_WORK_TREE", (t) => {
  const repo = divergedRepo(t);
  const other = createShipRepo("ship-rebase-other-");
  t.after(other.cleanup);
  other.commit("chore: other", { "README.md": "other\n" });

  const result = repo.runNode(SCRIPT, ["--branch", "feat"], {
    GIT_DIR: join(other.work, ".git"),
    GIT_WORK_TREE: other.work,
  });

  assert.equal(result.status, 0, result.output);
  assert.equal(repo.git("rev-parse", `${newTipOf(result)}^`), repo.git("rev-parse", "origin/main"));
  assert.equal(other.git("log", "--format=%s"), "chore: other");
});

// A `gh` stand-in that prints the view of a pull request from a fork.
/** A `gh` stub for a pull request whose branch `feat` is in `origin`. */
const sameRepoGh = (repo) => {
  const bin = join(repo.root, "bin");
  mkdirSync(bin);
  writeFileSync(
    join(bin, "gh"),
    `#!/bin/sh\necho '{"headRefName":"feat","isCrossRepository":false,"maintainerCanModify":false,"headRepository":{"name":"fallow"},"headRepositoryOwner":{"login":"fallow-rs"}}'\n`,
  );
  chmodSync(join(bin, "gh"), 0o755);
  return { PATH: `${bin}${delimiter}${process.env.PATH}` };
};

const forkGh = (repo) => {
  const bin = join(repo.root, "bin");
  mkdirSync(bin);
  writeFileSync(
    join(bin, "gh"),
    `#!/bin/sh\necho '{"headRefName":"feat","isCrossRepository":true,"maintainerCanModify":false,"headRepository":{"name":"fallow"},"headRepositoryOwner":{"login":"someone"}}'\n`,
  );
  chmodSync(join(bin, "gh"), 0o755);
  return { PATH: `${bin}${delimiter}${process.env.PATH}` };
};

test("the script refuses a fork pull request without maintainer edits", (t) => {
  const repo = forkRepo(t, {
    base: { "src/base.txt": "base\n" },
    onMain: () => {},
    onBranch: (r) => r.commit("feat: branch change", { "src/branch.txt": "branch\n" }),
  });

  const result = repo.runNode(SCRIPT, ["--pr", "12"], forkGh(repo));

  assert.equal(result.status, 2, result.output);
  assert.match(
    result.output,
    /Pull request 12 comes from a fork that does not allow maintainer edits/u,
  );
  assert.equal(repo.git("branch", "--show-current"), "main");
});

/**
 * Move `feat` from `origin` to a second bare repository, as a pull request
 * from a fork has it. Returns the path of the fork.
 */
const moveBranchToFork = (repo) => {
  const fork = join(repo.root, "fork.git");
  repo.git("init", "--quiet", "--bare", fork);
  repo.git("fetch", "--quiet", "origin");
  repo.git("push", "--quiet", fork, "origin/feat:refs/heads/feat");
  repo.git("push", "--quiet", "origin", "--delete", "feat");
  return fork;
};

test("--head-remote rebases a fork branch and pushes it back to the fork", (t) => {
  const repo = divergedRepo(t);
  const fork = moveBranchToFork(repo);

  const result = shipRebase(repo, "--head-remote", fork, "--push");

  assert.equal(result.status, 0, result.output);
  const tip = newTipOf(result);
  assert.equal(repo.git("--git-dir", fork, "rev-parse", "refs/heads/feat"), tip);
  assert.equal(repo.git("rev-parse", `${tip}^`), repo.git("rev-parse", "origin/main"));
  assert.equal(repo.git("ls-remote", "--heads", "origin", "feat"), "");
});

test("the version check finds a lost bump on a fork branch", (t) => {
  const repo = forkRepo(t, {
    base: { "src/check.rs": schemaSource(1) },
    onMain: (r) => r.commit("feat: main schema change", { "src/check.rs": schemaSource(2) }),
    onBranch: (r) => r.commit("feat: branch schema change", { "src/check.rs": schemaSource(2) }),
  });
  const fork = moveBranchToFork(repo);
  const before = repo.git("--git-dir", fork, "rev-parse", "refs/heads/feat");

  const result = shipRebase(repo, "--head-remote", fork, "--push");

  assert.equal(result.status, 1, result.output);
  assert.match(
    result.output,
    /Version changes that the rebase lost:\n\s+src\/check\.rs: CHECK_SCHEMA_VERSION 1 -> 2/u,
  );
  assert.equal(repo.git("--git-dir", fork, "rev-parse", "refs/heads/feat"), before);
  assert.doesNotMatch(result.output, /push --force-with-lease/u);
});

test("pullRequestHead reads the remote of a fork that allows maintainer edits", () => {
  assert.deepEqual(pullRequestHead("7", { headRefName: "fix/a", isCrossRepository: false }), {
    branch: "fix/a",
    headRemote: null,
  });
  assert.deepEqual(
    pullRequestHead("7", {
      headRefName: "perf/b",
      isCrossRepository: true,
      maintainerCanModify: true,
      headRepository: { name: "fallow" },
      headRepositoryOwner: { login: "contributor" },
    }),
    { branch: "perf/b", headRemote: "https://github.com/contributor/fallow.git" },
  );
  assert.throws(
    () =>
      pullRequestHead("7", {
        headRefName: "perf/b",
        isCrossRepository: true,
        maintainerCanModify: false,
        headRepository: { name: "fallow" },
        headRepositoryOwner: { login: "contributor" },
      }),
    /does not allow maintainer edits/u,
  );
});

test("a stopped rebase in the caller checkout stays as it is", (t) => {
  const repo = forkRepo(t, {
    base: { "src/base.txt": "base\n" },
    onMain: (r) => r.commit("fix: main change", { "src/a.txt": "a\n" }),
    onBranch: (r) => r.commit("feat: branch change", { "src/branch.txt": "branch\n" }),
  });
  // A failed `--exec` step stops the rebase with a clean work tree.
  repo.git("switch", "--quiet", "--detach", "origin/feat");
  assert.throws(() => repo.git("rebase", "--exec", "false", "origin/main"));
  const before = callerState(repo);

  const result = shipRebase(repo);

  assert.equal(result.status, 0, result.output);
  assert.deepEqual(callerState(repo), before);
  assert.equal(before.rebase, true);
});

test("the version report names the file of a deleted constant", (t) => {
  const repo = forkRepo(t, {
    base: { "src/old.rs": cacheSource(1) },
    onMain: (r) => r.commit("fix: main change", { "src/a.txt": "a\n" }),
    onBranch: (r) => {
      r.git("rm", "--quiet", "src/old.rs");
      r.git("commit", "--quiet", "-m", "refactor: remove the old cache");
    },
  });

  const result = shipRebase(repo);

  assert.equal(result.status, 0, result.output);
  assert.match(result.output, /\n\s+src\/old\.rs: GRAPH_CACHE_VERSION 1 -> \(removed\)/u);
});

test("the version check finds a schema version bump that the base already made", (t) => {
  const source = (version) => `pub const CHECK_SCHEMA_VERSION: u32 = ${version};\n`;
  const repo = forkRepo(t, {
    base: { "src/check.rs": source(9) },
    onMain: (r) => r.commit("feat: main schema change", { "src/check.rs": source(10) }),
    onBranch: (r) => r.commit("feat: branch schema change", { "src/check.rs": source(10) }),
  });

  const result = shipRebase(repo);

  assert.equal(result.status, 1, result.output);
  assert.match(
    result.output,
    /Version changes that the rebase lost:\n\s+src\/check\.rs: CHECK_SCHEMA_VERSION 9 -> 10/u,
  );
});

test("the version check reads a string schema version", (t) => {
  const source = (version) => `const RUNTIME_COVERAGE_SCHEMA_VERSION: &str = "${version}";\n`;
  const repo = forkRepo(t, {
    base: { "src/analyze.rs": source("1") },
    onMain: (r) => r.commit("feat: main schema change", { "src/analyze.rs": source("2") }),
    onBranch: (r) => r.commit("feat: branch schema change", { "src/analyze.rs": source("2") }),
  });

  const result = shipRebase(repo);

  assert.equal(result.status, 1, result.output);
  assert.match(
    result.output,
    /Version changes that the rebase lost:\n\s+src\/analyze\.rs: RUNTIME_COVERAGE_SCHEMA_VERSION "1" -> "2"/u,
  );
});

test("the script refuses both --pr and --branch", (t) => {
  const repo = createShipRepo("ship-rebase-args-");
  t.after(repo.cleanup);

  const result = repo.runNode(SCRIPT, ["--pr", "1", "--branch", "feat"]);

  assert.equal(result.status, 2);
  assert.match(result.output, /Give exactly one of --pr or --branch/u);
});

test("pushSshEnv keeps the push connection open unless the user set an SSH command", () => {
  const keepalive = pushSshEnv({}, "");
  assert.match(keepalive.GIT_SSH_COMMAND, /ServerAliveInterval=\d+/u);
  assert.deepEqual(pushSshEnv({ GIT_SSH_COMMAND: "ssh -i key" }, ""), {});
  assert.deepEqual(pushSshEnv({}, "ssh -o ProxyCommand=x"), {});
});

const lines = (...values) => `${values.join("\n")}\n`;
const NINE = ["1", "2", "3", "4", "5", "6", "7", "8", "9"];
const nineWith = (changes) => lines(...NINE.map((line, index) => changes[index + 1] ?? line));

// `parent` forks from `main`, and `feat` stacks on `parent`. Then `main`
// gets the squash of `parent`, and `onMain` adds more commits. Returns the
// repository and the old tip of `parent`.
const stackedRepo = (t, { base, onParent, onChild, onMain = () => {} }) => {
  const repo = createShipRepo("ship-rebase-stack-");
  t.after(repo.cleanup);
  repo.commit("chore: start", base);
  repo.git("push", "--quiet", "origin", "main");
  repo.git("switch", "--quiet", "-c", "parent");
  onParent(repo);
  const parentTip = repo.git("rev-parse", "HEAD");
  repo.git("switch", "--quiet", "-c", "feat");
  onChild(repo);
  repo.git("push", "--quiet", "origin", "feat");
  repo.git("switch", "--quiet", "main");
  repo.git("merge", "--quiet", "--squash", "parent");
  repo.git("commit", "--quiet", "-m", "feat: parent (#1)");
  onMain(repo);
  repo.git("push", "--quiet", "origin", "main");
  return { repo, parentTip };
};

test("--upstream rebases only the own commits of a stacked branch after a squash merge", (t) => {
  const { repo, parentTip } = stackedRepo(t, {
    base: { "src/a.txt": "a\n", "src/b.txt": nineWith({}) },
    onParent: (r) => {
      r.commit("feat: parent one", { "src/a.txt": "a1\n" });
      r.commit("feat: parent two", {
        "src/a.txt": "a2\n",
        "src/notes.txt": "parent\n",
      });
    },
    onChild: (r) =>
      r.commit("feat: child", {
        "src/b.txt": nineWith({ 3: "three" }),
        "src/notes.txt": "parent\nchild\n",
      }),
    // A change next to the hunk of the branch changes only its context.
    onMain: (r) => r.commit("fix: main", { "src/b.txt": nineWith({ 5: "five" }) }),
  });

  const plain = shipRebase(repo);
  assert.equal(plain.status, 1, plain.output);
  assert.match(plain.output, /STOP: feat: Conflicts outside .*src\/a\.txt/u);

  const result = shipRebase(repo, "--upstream", parentTip);

  assert.equal(result.status, 0, result.output);
  const tip = newTipOf(result);
  assert.equal(repo.git("rev-parse", `${tip}^`), repo.git("rev-parse", "origin/main"));
  assert.equal(fileAt(repo, tip, "src/b.txt"), nineWith({ 3: "three", 5: "five" }).trimEnd());
  assert.equal(fileAt(repo, tip, "src/notes.txt"), "parent\nchild");
  assert.match(result.output, /: 1 commits, new tip/u);
  assert.match(result.output, new RegExp(`a merge of the commits after ${parentTip} into`, "u"));
  assert.match(result.output, /Hunk check: the result has the same hunks/u);
});

test("--upstream sets a cache bump of a stacked branch to the base value + 1", (t) => {
  const source = (version, extra) => `${cacheSource(version)}\nfn a() {}\n\nfn b() {}\n${extra}`;
  const { repo, parentTip } = stackedRepo(t, {
    base: { "src/lib.rs": source(1, "") },
    onParent: (r) => r.commit("feat: parent", { "src/lib.rs": source(2, "") }),
    onChild: (r) => r.commit("feat: child", { "src/lib.rs": source(3, "\nfn child_test() {}\n") }),
    onMain: (r) => r.commit("feat: other", { "src/lib.rs": source(5, "") }),
  });

  const result = shipRebase(repo, "--upstream", parentTip);

  assert.equal(result.status, 0, result.output);
  assert.equal(
    fileAt(repo, newTipOf(result), "src/lib.rs"),
    source(6, "\nfn child_test() {}\n").trimEnd(),
  );
  assert.match(result.output, /GRAPH_CACHE_VERSION set to the base value \+ 1/u);
  assert.match(result.output, /Hunk check: the result has the same hunks/u);
});

test("--upstream finds a hunk that the rebase of a stacked branch lost", (t) => {
  const { repo, parentTip } = stackedRepo(t, {
    base: { "src/a.txt": "a\n", "src/c.txt": "c\n" },
    onParent: (r) => r.commit("feat: parent", { "src/a.txt": "a1\n" }),
    onChild: (r) => {
      r.commit("feat: child", { "src/a.txt": "a2\n" });
      r.git("switch", "--quiet", "-c", "side");
      r.commit("feat: side", { "src/side.txt": "side\n" });
      r.git("switch", "--quiet", "feat");
      // A change in the merge commit itself. A rebase drops it.
      r.git("merge", "--quiet", "--no-ff", "--no-commit", "side");
      r.write("src/c.txt", "c in the merge\n");
      r.git("add", "--all");
      r.git("commit", "--quiet", "-m", "merge side");
    },
  });

  const result = shipRebase(repo, "--upstream", parentTip, "--push");

  assert.equal(result.status, 1, result.output);
  assert.match(
    result.output,
    /Hunks of the branch that the rebase lost or changed:\n\s+src\/c\.txt:\n\s+-c\n\s+\+c in the merge/u,
  );
  assert.match(result.output, new RegExp(`git range-diff ${parentTip}\\.\\.`, "u"));
  assert.doesNotMatch(result.output, /Pushed/u);
});

test("ownHunkProblems finds a hunk that a whole-file resolution dropped", (t) => {
  const { repo, parentTip } = stackedRepo(t, {
    base: { "src/a.txt": nineWith({}) },
    onParent: (r) => r.commit("feat: parent", { "src/a.txt": nineWith({ 1: "p" }) }),
    onChild: (r) =>
      r.commit("feat: child", { "src/a.txt": nineWith({ 1: "child", 9: "child test" }) }),
    onMain: (r) => r.commit("fix: main", { "src/a.txt": nineWith({ 1: "main" }) }),
  });
  const oldTip = repo.git("rev-parse", "feat");
  const baseTip = repo.git("rev-parse", "main");
  const rebaseByHand = (resolveConflict) => {
    repo.git("switch", "--quiet", "--detach", oldTip);
    assert.throws(() => repo.git("rebase", "--quiet", "--onto", baseTip, parentTip));
    resolveConflict();
    repo.git("add", "--all");
    repo.git("-c", "core.editor=true", "rebase", "--continue");
    return repo.git("rev-parse", "HEAD");
  };
  const range = { upstream: parentTip, oldTip, baseTip };

  // The base side of the whole file also drops the hunk at line 9, which
  // did not conflict.
  const dropped = rebaseByHand(() => repo.git("checkout", "--ours", "src/a.txt"));
  const [problem, ...others] = ownHunkProblems(repo.work, { ...range, newTip: dropped });
  assert.deepEqual(others, []);
  assert.match(problem, /lost or changed:\n\s+src\/a\.txt:\n\s+-p\n\s+\+child\n/u);
  assert.match(problem, /\n\s+src\/a\.txt:\n\s+-9\n\s+\+child test\n/u);

  // A resolution of the conflict changes only the hunk at line 1.
  const resolved = rebaseByHand(() =>
    repo.write("src/a.txt", nineWith({ 1: "child", 9: "child test" })),
  );
  const [changed] = ownHunkProblems(repo.work, { ...range, newTip: resolved });
  assert.match(changed, /lost or changed:\n\s+src\/a\.txt:\n\s+-p\n\s+\+child\n/u);
  assert.match(changed, /does not have:\n\s+src\/a\.txt:\n\s+-main\n\s+\+child\n/u);
  assert.doesNotMatch(changed, /child test/u);
});

test("diffHunks keys hunks by path and ignores their line numbers", () => {
  const diff = [
    "diff --git a/src/a b.txt b/src/a b.txt",
    "index 1..2 100644",
    "--- a/src/a b.txt",
    "+++ b/src/a b.txt",
    "@@ -1 +1 @@",
    "-one",
    "+two",
    "@@ -9 +9 @@",
    "--- dashes",
    "+++ more",
    "diff --git a/run.sh b/run.sh",
    "old mode 100644",
    "new mode 100755",
  ].join("\n");

  assert.deepEqual(
    [...diffHunks(diff)],
    [
      ["src/a b.txt", ["-one\n+two", "--- dashes\n+++ more"]],
      ["run.sh", ["old mode 100644", "new mode 100755"]],
    ],
  );
});

test("--upstream must be an ancestor of the branch", (t) => {
  const repo = forkRepo(t, {
    base: { "src/base.txt": "base\n" },
    onMain: (r) => r.commit("fix: main", { "src/main.txt": "main\n" }),
    onBranch: (r) => r.commit("fix: branch", { "src/branch.txt": "branch\n" }),
  });

  const result = shipRebase(repo, "--upstream", "origin/main");

  assert.equal(result.status, 2, result.output);
  assert.match(result.output, /--upstream origin\/main is not an ancestor of feat\./u);
});

test("--if-needed skips the rebase of a branch that merges without a conflict", (t) => {
  const repo = forkRepo(t, {
    base: { "src/lib.rs": cacheSource(1) },
    onMain: (r) => r.commit("fix: main change", { "src/main.txt": "main\n" }),
    onBranch: (r) =>
      r.commit("fix: branch change", {
        "src/branch.txt": "branch\n",
        "src/lib.rs": cacheSource(2),
      }),
  });
  const oldTip = repo.git("rev-parse", "origin/feat");

  const result = shipRebase(repo, "--if-needed", "--push");

  assert.equal(result.status, 0, result.output);
  assert.match(result.output, /No rebase needed: feat merges into origin\/main/u);
  assert.match(result.output, new RegExp(`--auto --squash --match-head-commit ${oldTip}`, "u"));
  repo.git("fetch", "--quiet", "origin");
  assert.equal(repo.git("rev-parse", "origin/feat"), oldTip);
  assert.deepEqual(worktreePaths(repo), [repo.work]);
});

test("--if-needed rebases when the merge loses a cache bump of the branch", (t) => {
  const repo = forkRepo(t, {
    base: { "src/lib.rs": cacheSource(1) },
    onMain: (r) => r.commit("fix: main cache change", { "src/lib.rs": cacheSource(2) }),
    onBranch: (r) =>
      r.commit("fix: branch cache change", {
        "src/lib.rs": cacheSource(2),
        "src/other.txt": "other\n",
      }),
  });

  const result = shipRebase(repo, "--if-needed");

  assert.equal(result.status, 0, result.output);
  assert.match(result.output, /Rebase needed for feat:/u);
  assert.match(result.output, /GRAPH_CACHE_VERSION 1 -> 2/u);
  assert.equal(fileAt(repo, newTipOf(result), "src/lib.rs"), cacheSource(3).trimEnd());
});

test("--if-needed rebases a branch that conflicts", (t) => {
  const repo = forkRepo(t, {
    base: { "src/value.txt": "base\n" },
    onMain: (r) => r.commit("fix: main value", { "src/value.txt": "main\n" }),
    onBranch: (r) => r.commit("fix: branch value", { "src/value.txt": "branch\n" }),
  });

  const result = shipRebase(repo, "--if-needed");

  assert.equal(result.status, 1, result.output);
  assert.match(result.output, /The merge into the base has a conflict/u);
  assert.match(result.output, /STOP: feat: Conflicts outside generated files/u);
});

test("--if-needed skips the rebase of a branch that changes a released CHANGELOG.md section", (t) => {
  const repo = forkRepo(t, {
    base: { "CHANGELOG.md": "# Changelog\n\n## 1.0.0\n\n- Old note.\n" },
    onMain: (r) => r.commit("fix: main change", { "src/main.txt": "main\n" }),
    onBranch: (r) =>
      r.commit("docs: fix a release note", {
        "CHANGELOG.md": "# Changelog\n\n## 1.0.0\n\n- Fixed note.\n",
      }),
  });

  const result = shipRebase(repo, "--if-needed");

  assert.equal(result.status, 0, result.output);
  assert.match(result.output, /No rebase needed: feat merges into origin\/main/u);
  assert.doesNotMatch(result.output, /CHANGELOG/u);
});

test("--if-needed finds an equal cache bump that a merge of main into the branch hides", (t) => {
  const repo = forkRepo(t, {
    base: { "src/lib.rs": cacheSource(1) },
    onMain: (r) => r.commit("fix: main cache change", { "src/lib.rs": cacheSource(2) }),
    onBranch: (r) =>
      r.commit("fix: branch cache change", {
        "src/lib.rs": cacheSource(2),
        "src/other.txt": "other\n",
      }),
  });
  repo.git("switch", "--quiet", "feat");
  repo.git("merge", "--quiet", "--no-edit", "origin/main");
  repo.git("push", "--quiet", "origin", "feat");
  repo.git("switch", "--quiet", "main");

  const result = shipRebase(repo, "--if-needed");

  assert.match(result.output, /Rebase needed for feat:/u);
  assert.match(result.output, /GRAPH_CACHE_VERSION 1 -> 2/u);
  assert.doesNotMatch(result.output, /No rebase needed/u);
});
