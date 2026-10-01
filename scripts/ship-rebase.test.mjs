import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { chmodSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { delimiter, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import {
  changelogProblems,
  duplicateSubsections,
  keepBothSides,
  movedEntries,
} from "./ship-rebase.mjs";
import { pushSshEnv, shellCommand } from "./ship-git.mjs";
import { createShipRepo } from "./ship-test-repo.mjs";

const SCRIPT = fileURLToPath(new URL("./ship-rebase.mjs", import.meta.url));

const changelog = (...entries) =>
  [
    "# Changelog",
    "",
    "## [Unreleased]",
    "",
    "### Added",
    "",
    ...entries,
    "",
    "## [1.0.0]",
    "",
    "- First release.",
    "",
  ].join("\n");

const cacheSource = (version) => `pub const GRAPH_CACHE_VERSION: u32 = ${version};\n`;

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

const fileAtHead = (repo, path) => repo.git("show", `HEAD:${path}`);

test("a CHANGELOG conflict keeps the entries of both sides", (t) => {
  const repo = forkRepo(t, {
    base: { "CHANGELOG.md": changelog(), "src/lib.rs": cacheSource(1) },
    onMain: (r) => r.commit("feat: main change", { "CHANGELOG.md": changelog("- Main entry.") }),
    onBranch: (r) =>
      r.commit("feat: branch change", {
        "CHANGELOG.md": changelog("- Branch entry."),
        "src/lib.rs": cacheSource(2),
      }),
  });

  const result = shipRebase(repo);

  assert.equal(result.status, 0, result.output);
  assert.equal(
    fileAtHead(repo, "CHANGELOG.md"),
    changelog("- Main entry.", "- Branch entry.").trimEnd(),
  );
  assert.match(result.output, /Resolved conflicts at 1 stops of the rebase\./u);
  assert.match(result.output, /CHANGELOG check: the rebase kept the lines of the branch/u);
  assert.match(result.output, /the new entries of the branch are in the first release section/u);
  assert.match(result.output, /src\/lib\.rs: GRAPH_CACHE_VERSION 1 -> 2/u);
  assert.equal(repo.git("rev-parse", "HEAD^"), repo.git("rev-parse", "origin/main"));
});

test("a conflict outside CHANGELOG.md stops the rebase", (t) => {
  const repo = forkRepo(t, {
    base: { "CHANGELOG.md": changelog(), "src/value.txt": "base\n" },
    onMain: (r) => r.commit("fix: main value", { "src/value.txt": "main\n" }),
    onBranch: (r) => r.commit("fix: branch value", { "src/value.txt": "branch\n" }),
  });

  const result = shipRebase(repo);

  assert.equal(result.status, 1, result.output);
  assert.match(result.output, /STOP: feat: Conflicts outside CHANGELOG\.md: src\/value\.txt/u);
  assert.match(readFileSync(join(repo.work, "src/value.txt"), "utf8"), /^<{7} /mu);
  assert.equal(existsSync(join(repo.work, ".git", "rebase-merge")), true);
});

test("the CHANGELOG check finds a superseded entry that a keep-both resolution kept", (t) => {
  const repo = forkRepo(t, {
    base: { "CHANGELOG.md": changelog() },
    onMain: (r) => r.commit("feat: main change", { "CHANGELOG.md": changelog("- Main entry.") }),
    onBranch: (r) => {
      r.commit("feat: first draft", { "CHANGELOG.md": changelog("- Draft entry.") });
      r.commit("docs: final entry", { "CHANGELOG.md": changelog("- Final entry.") });
    },
  });

  const result = shipRebase(repo, "--push");

  assert.equal(result.status, 1, result.output);
  assert.match(
    result.output,
    /Lines that the rebase added and the branch did not add:\n\s+- Draft entry\./u,
  );
  assert.doesNotMatch(result.output, /Pushed/u);
  assert.notEqual(repo.git("rev-parse", "origin/feat"), repo.git("rev-parse", "HEAD"));
});

test("the CHANGELOG check finds a subsection that the resolution added two times", (t) => {
  const withSections = (sections) => changelog().replace("## [1.0.0]", `${sections}\n\n## [1.0.0]`);
  const repo = forkRepo(t, {
    base: { "CHANGELOG.md": changelog() },
    onMain: (r) =>
      r.commit("fix: main fix", { "CHANGELOG.md": withSections("### Fixed\n\n- Main fix.") }),
    onBranch: (r) =>
      r.commit("fix: branch fix", {
        "CHANGELOG.md": withSections(
          "### Changed\n\n- Branch change.\n\n### Fixed\n\n- Branch fix.",
        ),
      }),
  });

  const result = shipRebase(repo);

  assert.equal(result.status, 1, result.output);
  assert.match(
    result.output,
    /Subsections that occur two times in the first release section:\n\s+### Fixed/u,
  );
});

test("the cache version check finds a bump that the base already made", (t) => {
  const repo = forkRepo(t, {
    base: { "CHANGELOG.md": changelog(), "src/lib.rs": cacheSource(1) },
    onMain: (r) => r.commit("fix: main cache change", { "src/lib.rs": cacheSource(2) }),
    onBranch: (r) => r.commit("fix: branch cache change", { "src/lib.rs": cacheSource(2) }),
  });

  const result = shipRebase(repo);

  assert.equal(result.status, 1, result.output);
  assert.match(
    result.output,
    /Version changes that the rebase lost:\n\s+src\/lib\.rs: GRAPH_CACHE_VERSION 1 -> 2/u,
  );
});

test("the cache version check reads constants of every unsigned integer type", (t) => {
  const source = (version) =>
    `pub(super) const AUDIT_BASE_SNAPSHOT_CACHE_VERSION: u8 = ${version};\n`;
  const repo = forkRepo(t, {
    base: { "CHANGELOG.md": changelog(), "src/audit.rs": source(1) },
    onMain: (r) => r.commit("fix: main cache change", { "src/audit.rs": source(2) }),
    onBranch: (r) => r.commit("fix: branch cache change", { "src/audit.rs": source(2) }),
  });

  const result = shipRebase(repo);

  assert.equal(result.status, 1, result.output);
  assert.match(
    result.output,
    /Version changes that the rebase lost:\n\s+src\/audit\.rs: AUDIT_BASE_SNAPSHOT_CACHE_VERSION 1 -> 2/u,
  );
});

// `feat` adds an entry, `main` adds another entry, and then `feat` merges
// `main`. `inMerge` changes the work tree before the merge commit.
const mergedRepo = (t, inMerge) => {
  const repo = forkRepo(t, {
    base: { "CHANGELOG.md": changelog(), "src/b.txt": "b\n" },
    onMain: (r) => r.commit("feat: main change", { "CHANGELOG.md": changelog("- Main entry.") }),
    onBranch: (r) =>
      r.commit("feat: branch change", { "CHANGELOG.md": changelog("- Branch entry.") }),
  });
  repo.git("switch", "--quiet", "feat");
  assert.throws(() => repo.git("merge", "--quiet", "--no-edit", "main"));
  repo.write("CHANGELOG.md", changelog("- Main entry.", "- Branch entry."));
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
  assert.equal(fileAtHead(repo, "src/b.txt"), "b");
  assert.doesNotMatch(result.output, /Pushed/u);
});

// `main` got the first commit of the branch as a separate commit, so the
// rebase skips it. The merge sees two different values for one line.
// `onBranch` adds more commits after the two value commits.
const skippedCommitRepo = (t, onBranch) =>
  forkRepo(t, {
    base: { "CHANGELOG.md": changelog(), "src/value.txt": "1\n" },
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
  assert.equal(fileAtHead(repo, "src/value.txt"), "3");
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
  assert.equal(
    fileAtHead(repo, "CHANGELOG.md"),
    changelog("- Main entry.", "- Branch entry.").trimEnd(),
  );
});

test("the CHANGELOG check finds an entry that the branch changed in a merge commit", (t) => {
  const repo = mergedRepo(t, (r) =>
    r.write("CHANGELOG.md", changelog("- Main entry.", "- Branch entry, reworded.")),
  );

  const result = shipRebase(repo);

  assert.equal(result.status, 1, result.output);
  assert.match(
    result.output,
    /Lines that the branch added and the rebase lost:\n\s+- Branch entry, reworded\./u,
  );
  assert.match(
    result.output,
    /Lines that the rebase added and the branch did not add:\n\s+- Branch entry\./u,
  );
});

test("the CHANGELOG check finds an entry that the branch replaced and the resolution kept", (t) => {
  const repo = forkRepo(t, {
    base: { "CHANGELOG.md": changelog("- Old entry.") },
    onMain: (r) =>
      r.commit("feat: main change", {
        "CHANGELOG.md": changelog("- Main entry.", "- Old entry."),
      }),
    onBranch: (r) => r.commit("docs: new entry", { "CHANGELOG.md": changelog("- New entry.") }),
  });

  const result = shipRebase(repo);

  assert.equal(result.status, 1, result.output);
  assert.match(
    result.output,
    /Lines that the branch removed and the rebase kept:\n\s+- Old entry\./u,
  );
});

// A release on `main` puts a version heading under `## [Unreleased]`, so
// the entries above it move into the released version.
const release = (text) => text.replace("## [Unreleased]\n\n", "## [Unreleased]\n\n## [1.1.0]\n\n");

const MOVED =
  /Entries of the branch that moved out of the first release section:\n\s+- Branch entry\./u;
const MOVE_BY_HAND =
  /HEAD holds the rebased branch\. To fix it:\n\s+1\. Move these entries to the first release section of CHANGELOG\.md and commit the change\./u;

test("the script stops when a clean rebase moves a branch entry into a release", (t) => {
  const repo = forkRepo(t, {
    base: { "CHANGELOG.md": changelog("- Old entry.") },
    onMain: (r) =>
      r.commit("chore: release 1.1.0", { "CHANGELOG.md": release(changelog("- Old entry.")) }),
    onBranch: (r) =>
      r.commit("feat: branch change", {
        "CHANGELOG.md": changelog("- Old entry.", "- Branch entry."),
      }),
  });

  const oldTip = repo.git("rev-parse", "origin/feat");

  const result = shipRebase(repo, "--push");

  assert.equal(result.status, 1, result.output);
  assert.match(result.output, /Resolved conflicts at 0 stops of the rebase\./u);
  assert.match(result.output, MOVED);
  assert.match(result.output, MOVE_BY_HAND);
  assert.equal(repo.git("log", "-1", "--format=%s"), "feat: branch change");
  repo.git("fetch", "--quiet", "origin");
  assert.equal(repo.git("rev-parse", "origin/feat"), oldTip);
});

test("the printed recovery steps for a moved entry lead to a passing run", (t) => {
  const repo = forkRepo(t, {
    base: { "CHANGELOG.md": changelog("- Old entry.") },
    onMain: (r) =>
      r.commit("chore: release 1.1.0", { "CHANGELOG.md": release(changelog("- Old entry.")) }),
    onBranch: (r) =>
      r.commit("feat: branch change", {
        "CHANGELOG.md": changelog("- Old entry.", "- Branch entry."),
      }),
  });
  const oldTip = repo.git("rev-parse", "origin/feat");

  const failed = shipRebase(repo, "--push");

  assert.equal(failed.status, 1, failed.output);
  const rebasedTip = repo.git("rev-parse", "HEAD");
  assert.match(failed.output, new RegExp(`git diff ${rebasedTip} HEAD`, "u"));
  const push = failed.output.match(/^\s*git (push \S+ \S+ \S+)$/mu);
  assert.notEqual(push, null, failed.output);
  assert.equal(
    push[1],
    `push --force-with-lease=refs/heads/feat:${oldTip} origin HEAD:refs/heads/feat`,
  );
  const fixed = fileAtHead(repo, "CHANGELOG.md")
    .replace("- Branch entry.\n", "")
    .replace("## [Unreleased]\n\n", "## [Unreleased]\n\n### Added\n\n- Branch entry.\n\n");
  repo.commit("docs: move the branch entry to the unreleased section", {
    "CHANGELOG.md": `${fixed}\n`,
  });
  repo.git(...push[1].split(" "));

  const rerun = shipRebase(repo, "--push");

  assert.equal(rerun.status, 0, rerun.output);
  assert.match(rerun.output, /The branch is already on origin\/main\. Nothing to push\./u);
  repo.git("fetch", "--quiet", "origin");
  assert.equal(repo.git("rev-parse", "origin/feat"), repo.git("rev-parse", "HEAD"));
});

const OTHER_PROBLEMS_FIRST =
  /Fix the other problems first\. After a push of HEAD, the next run cannot find them, because it compares the pushed result with itself\./u;

// A moved entry is not the only problem, so a push of HEAD would hide the
// other problem from the next run. The script must not print the push.
const assertNoRecoverySteps = (result) => {
  assert.equal(result.status, 1, result.output);
  assert.match(result.output, MOVED);
  assert.match(result.output, OTHER_PROBLEMS_FIRST);
  assert.doesNotMatch(result.output, MOVE_BY_HAND);
  assert.doesNotMatch(result.output, /git push/u);
  assert.doesNotMatch(result.output, /Run the script again/u);
};

test("a moved entry with a lost version bump prints no push command", (t) => {
  const repo = forkRepo(t, {
    base: { "CHANGELOG.md": changelog("- Old entry."), "src/cache.rs": cacheSource(1) },
    onMain: (r) =>
      r.commit("chore: release 1.1.0", {
        "CHANGELOG.md": release(changelog("- Old entry.")),
        "src/cache.rs": cacheSource(2),
      }),
    onBranch: (r) =>
      r.commit("feat: branch change", {
        "CHANGELOG.md": changelog("- Old entry.", "- Branch entry."),
        "src/cache.rs": cacheSource(2),
      }),
  });

  const result = shipRebase(repo, "--push");

  assertNoRecoverySteps(result);
  assert.match(
    result.output,
    /Version changes that the rebase lost:\n\s+src\/cache\.rs: GRAPH_CACHE_VERSION 1 -> 2/u,
  );
});

test("a moved entry with a tree check difference prints no push command", (t) => {
  const repo = forkRepo(t, {
    base: { "CHANGELOG.md": changelog("- Old entry."), "src/b.txt": "b\n" },
    onMain: (r) =>
      r.commit("chore: release 1.1.0", { "CHANGELOG.md": release(changelog("- Old entry.")) }),
    onBranch: (r) => {
      r.commit("feat: branch change", {
        "CHANGELOG.md": changelog("- Old entry.", "- Branch entry."),
      });
      r.git("switch", "--quiet", "-c", "side", "HEAD^");
      r.commit("feat: side change", { "src/side.txt": "side\n" });
      r.git("switch", "--quiet", "feat");
      r.git("merge", "--quiet", "--no-ff", "--no-commit", "side");
      r.write("src/b.txt", "b edited in the merge\n");
      r.git("add", "--all");
      r.git("commit", "--quiet", "--no-edit");
    },
  });

  const result = shipRebase(repo, "--push");

  assertNoRecoverySteps(result);
  assert.match(
    result.output,
    /Paths where the rebase result differs from a merge of the branch into origin\/main:\n\s+src\/b\.txt/u,
  );
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

test("the script stops when a keep-both resolution puts a branch entry in a release", (t) => {
  const withTop = (top) => changelog().replace("## [Unreleased]\n\n", `## [Unreleased]\n\n${top}`);
  const repo = forkRepo(t, {
    base: { "CHANGELOG.md": changelog() },
    onMain: (r) => r.commit("chore: release 1.1.0", { "CHANGELOG.md": release(changelog()) }),
    onBranch: (r) =>
      r.commit("fix: branch fix", { "CHANGELOG.md": withTop("### Fixed\n\n- Branch entry.\n\n") }),
  });

  const result = shipRebase(repo);

  assert.equal(result.status, 1, result.output);
  assert.match(result.output, /Resolved conflicts at 1 stops of the rebase\./u);
  assert.match(result.output, MOVED);
  assert.match(result.output, MOVE_BY_HAND);
});

test("movedEntries compares the entries that the branch adds to the first section", () => {
  const text = (unreleased, released = []) =>
    [
      "# Changelog",
      "",
      "## [Unreleased]",
      "",
      ...unreleased,
      "",
      "## [1.1.0]",
      "",
      ...released,
      "",
    ].join("\n");
  const oldTipText = text(["- Same.", "- Branch."], ["- Old release note."]);
  const added = ["- Branch.", "- Old release note.", "- Same."];

  assert.deepEqual(
    movedEntries({
      added,
      oldTipText,
      baseText: text(["- Same."]),
      newTipText: text(["- Same.", "- Same.", "- Branch."], ["- Old release note."]),
    }),
    [],
  );
  assert.deepEqual(
    movedEntries({
      added,
      oldTipText,
      baseText: text(["- Same."]),
      newTipText: text(["- Same.", "- Branch."], ["- Same.", "- Old release note."]),
    }),
    ["- Same."],
  );
  assert.deepEqual(
    movedEntries({
      added,
      oldTipText,
      baseText: text([]),
      newTipText: text([], ["- Same.", "- Branch.", "- Old release note."]),
    }),
    ["- Same.", "- Branch."],
  );
});

test("changelogProblems reports each kind of line difference", () => {
  const before = { added: ["- Added.", "- Lost."], removed: ["- Replaced."] };
  const after = { added: ["- Added.", "- Extra."], removed: ["- Base line."] };

  assert.deepEqual(changelogProblems(before, after), [
    "Lines that the branch added and the rebase lost:\n    - Lost.",
    "Lines that the rebase added and the branch did not add:\n    - Extra.",
    "Base lines that the rebase removed:\n    - Base line.",
    "Lines that the branch removed and the rebase kept:\n    - Replaced.",
  ]);
  assert.deepEqual(changelogProblems(before, before), []);
});

test("the rebase ignores a recorded rerere resolution", (t) => {
  const repo = forkRepo(t, {
    base: { "CHANGELOG.md": changelog(), "src/lib.rs": cacheSource(1) },
    onMain: (r) => r.commit("fix: main cache change", { "src/lib.rs": cacheSource(2) }),
    onBranch: (r) =>
      r.commit("fix: branch cache change", {
        "src/lib.rs": cacheSource(3),
        "src/other.rs": "pub const OTHER: u32 = 1;\n",
      }),
  });
  repo.git("config", "rerere.enabled", "true");
  repo.git("config", "rerere.autoUpdate", "true");
  // Record a resolution that keeps the value of main, as a resolution of
  // another pull request can do.
  repo.git("switch", "--quiet", "--detach", "origin/feat");
  assert.throws(() => repo.git("rebase", "origin/main"));
  repo.write("src/lib.rs", cacheSource(2));
  repo.git("add", "src/lib.rs");
  repo.git("-c", "core.editor=true", "rebase", "--continue");
  // Plain git now reuses the resolution without a conflict marker.
  repo.git("switch", "--quiet", "--detach", "origin/feat");
  assert.throws(() => repo.git("rebase", "origin/main"));
  assert.equal(readFileSync(join(repo.work, "src/lib.rs"), "utf8"), cacheSource(2));
  repo.git("rebase", "--abort");
  repo.git("switch", "--quiet", "main");

  const result = shipRebase(repo);

  assert.equal(result.status, 1, result.output);
  assert.match(result.output, /Conflicts outside CHANGELOG\.md: src\/lib\.rs/u);
  assert.match(readFileSync(join(repo.work, "src/lib.rs"), "utf8"), /^<{7} /mu);
});

test("--push updates the branch with a lease on the old tip", (t) => {
  const repo = forkRepo(t, {
    base: { "CHANGELOG.md": changelog() },
    onMain: (r) => r.commit("feat: main change", { "CHANGELOG.md": changelog("- Main entry.") }),
    onBranch: (r) =>
      r.commit("feat: branch change", { "CHANGELOG.md": changelog("- Branch entry.") }),
  });

  const result = shipRebase(repo, "--push");

  assert.equal(result.status, 0, result.output);
  assert.match(
    result.output,
    /Pushing feat\. The pre-push hook of the checkout runs first\.\nPushed feat with a lease/u,
  );
  repo.git("fetch", "--quiet", "origin");
  assert.equal(repo.git("rev-parse", "origin/feat"), repo.git("rev-parse", "HEAD"));
});

test("--push fails when the remote branch moved after the fetch", (t) => {
  const repo = forkRepo(t, {
    base: { "CHANGELOG.md": changelog() },
    onMain: (r) => r.commit("feat: main change", { "CHANGELOG.md": changelog("- Main entry.") }),
    onBranch: (r) =>
      r.commit("feat: branch change", { "CHANGELOG.md": changelog("- Branch entry.") }),
  });
  // Someone else pushes to `feat` between the fetch and the push of the
  // script. The script switches to the fetched tip after the fetch, so a
  // post-checkout hook that runs one time moves the remote branch.
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
});

test("the script refuses a work tree with tracked changes", (t) => {
  const repo = forkRepo(t, {
    base: { "CHANGELOG.md": changelog() },
    onMain: () => {},
    onBranch: (r) => r.commit("feat: branch change", { "CHANGELOG.md": changelog("- Entry.") }),
  });
  repo.write("CHANGELOG.md", "local edit\n");

  const result = shipRebase(repo);

  assert.equal(result.status, 2, result.output);
  assert.match(result.output, /tracked changes/u);
});

test("the rebase is linear when rebase.rebaseMerges is set", (t) => {
  const repo = mergedRepo(t, () => {});
  repo.git("config", "rebase.rebaseMerges", "true");

  const result = shipRebase(repo);

  assert.equal(result.status, 0, result.output);
  assert.equal(repo.git("rev-list", "--merges", "origin/main..HEAD"), "");
});

test("the rebase moves no local branch when rebase.updateRefs is set", (t) => {
  const repo = forkRepo(t, {
    base: { "CHANGELOG.md": changelog() },
    onMain: (r) => r.commit("feat: main change", { "CHANGELOG.md": changelog("- Main entry.") }),
    onBranch: (r) => {
      r.commit("feat: first change", { "src/a.txt": "a\n" });
      r.git("branch", "part");
      r.commit("feat: branch change", { "CHANGELOG.md": changelog("- Branch entry.") });
    },
  });
  const part = repo.git("rev-parse", "part");
  repo.git("config", "rebase.updateRefs", "true");

  const result = shipRebase(repo);

  assert.equal(result.status, 0, result.output);
  assert.equal(repo.git("rev-parse", "part"), part);
});

test("the script ignores an inherited GIT_DIR and GIT_WORK_TREE", (t) => {
  const repo = forkRepo(t, {
    base: { "CHANGELOG.md": changelog() },
    onMain: (r) => r.commit("feat: main change", { "CHANGELOG.md": changelog("- Main entry.") }),
    onBranch: (r) =>
      r.commit("feat: branch change", { "CHANGELOG.md": changelog("- Branch entry.") }),
  });
  const other = createShipRepo("ship-rebase-other-");
  t.after(other.cleanup);
  other.commit("chore: other", { "README.md": "other\n" });

  const result = repo.runNode(SCRIPT, ["--branch", "feat"], {
    GIT_DIR: join(other.work, ".git"),
    GIT_WORK_TREE: other.work,
  });

  assert.equal(result.status, 0, result.output);
  assert.equal(repo.git("rev-parse", "HEAD^"), repo.git("rev-parse", "origin/main"));
  assert.equal(other.git("log", "--format=%s"), "chore: other");
});

// A `gh` stand-in that prints the view of a pull request from a fork.
const forkGh = (repo) => {
  const bin = join(repo.root, "bin");
  mkdirSync(bin);
  writeFileSync(
    join(bin, "gh"),
    `#!/bin/sh\necho '{"headRefName":"feat","isCrossRepository":true}'\n`,
  );
  chmodSync(join(bin, "gh"), 0o755);
  return { PATH: `${bin}${delimiter}${process.env.PATH}` };
};

test("the script refuses a pull request from a fork", (t) => {
  const repo = forkRepo(t, {
    base: { "CHANGELOG.md": changelog() },
    onMain: () => {},
    onBranch: (r) => r.commit("feat: branch change", { "CHANGELOG.md": changelog("- Entry.") }),
  });

  const result = repo.runNode(SCRIPT, ["--pr", "12"], forkGh(repo));

  assert.equal(result.status, 2, result.output);
  assert.match(result.output, /Pull request 12 comes from a fork/u);
  assert.equal(repo.git("branch", "--show-current"), "main");
});

test("the script refuses a checkout with a stopped rebase", (t) => {
  const repo = forkRepo(t, {
    base: { "CHANGELOG.md": changelog() },
    onMain: (r) => r.commit("fix: main change", { "src/a.txt": "a\n" }),
    onBranch: (r) => r.commit("feat: branch change", { "CHANGELOG.md": changelog("- Entry.") }),
  });
  // A failed `--exec` step stops the rebase with a clean work tree.
  repo.git("switch", "--quiet", "--detach", "origin/feat");
  assert.throws(() => repo.git("rebase", "--exec", "false", "origin/main"));

  const result = shipRebase(repo);

  assert.equal(result.status, 2, result.output);
  assert.match(result.output, /A rebase is in progress/u);
});

test("the version report names the file of a deleted constant", (t) => {
  const repo = forkRepo(t, {
    base: { "CHANGELOG.md": changelog(), "src/old.rs": cacheSource(1) },
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
    base: { "CHANGELOG.md": changelog(), "src/check.rs": source(9) },
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
    base: { "CHANGELOG.md": changelog(), "src/analyze.rs": source("1") },
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

test("keepBothSides drops the base section of a diff3 conflict", () => {
  const text = [
    "before",
    "<<<<<<< HEAD",
    "ours",
    "||||||| base",
    "original",
    "=======",
    "theirs",
    ">>>>>>> commit",
    "after",
    "",
  ].join("\n");

  assert.deepEqual(keepBothSides(text), { text: "before\nours\ntheirs\nafter\n", conflicts: 1 });
});

test("keepBothSides keeps a setext underline outside a conflict", () => {
  const text = "Title\n=======\n\n<<<<<<< HEAD\na\n=======\nb\n>>>>>>> x\n";

  assert.deepEqual(keepBothSides(text), { text: "Title\n=======\n\na\nb\n", conflicts: 1 });
});

test("keepBothSides rejects a conflict without an end marker", () => {
  assert.throws(() => keepBothSides("<<<<<<< HEAD\na\n=======\nb\n"), /no end marker/u);
});

test("duplicateSubsections looks only at the first release section", () => {
  const text =
    "# Changelog\n\n## [Unreleased]\n\n### Added\n\n### Added\n\n## [1.0.0]\n\n### Fixed\n\n### Fixed\n";

  assert.deepEqual(duplicateSubsections(text, "# Changelog\n"), ["### Added"]);
});

test("duplicateSubsections reports only a duplicate that the base does not have", () => {
  const section = (...headings) => `# Changelog\n\n## [Unreleased]\n\n${headings.join("\n\n")}\n`;
  const base = section("### Fixed", "### Added", "### Fixed");

  assert.deepEqual(duplicateSubsections(base, base), []);
  assert.deepEqual(
    duplicateSubsections(section("### Fixed", "### Added", "### Fixed", "### Added"), base),
    ["### Added"],
  );
  assert.deepEqual(duplicateSubsections(section("### Fixed", "### Fixed", "### Fixed"), base), [
    "### Fixed",
  ]);
});

test("the subsection check accepts a duplicate that the base already has", (t) => {
  const withSections = (sections) => changelog().replace("## [1.0.0]", `${sections}\n\n## [1.0.0]`);
  const baseSections =
    "### Fixed\n\n- Old fix.\n\n### Changed\n\n- Old change.\n\n### Fixed\n\n- Other fix.";
  const repo = forkRepo(t, {
    base: { "CHANGELOG.md": withSections(baseSections) },
    onMain: (r) => r.commit("chore: main change", { "src/a.txt": "a\n" }),
    onBranch: (r) =>
      r.commit("fix: branch fix", {
        "CHANGELOG.md": withSections(`${baseSections}\n- Branch fix.`),
      }),
  });

  const result = shipRebase(repo);

  assert.equal(result.status, 0, result.output);
});

test("pushSshEnv keeps the push connection open unless the user set an SSH command", () => {
  const keepalive = pushSshEnv({}, "");
  assert.match(keepalive.GIT_SSH_COMMAND, /ServerAliveInterval=\d+/u);
  assert.deepEqual(pushSshEnv({ GIT_SSH_COMMAND: "ssh -i key" }, ""), {});
  assert.deepEqual(pushSshEnv({}, "ssh -o ProxyCommand=x"), {});
});
