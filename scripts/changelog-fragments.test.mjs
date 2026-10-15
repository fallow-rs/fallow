import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  realpathSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";

import {
  changelogFragments,
  fragmentNames,
  fragmentProblems,
  mergeUnreleased,
  parseArgs,
  splitSections,
} from "./changelog-fragments.mjs";

const changelog = (...unreleased) =>
  [
    "# Changelog",
    "",
    "## [Unreleased]",
    "",
    ...unreleased,
    "## [1.0.0] - 2026-01-01",
    "",
    "### Added",
    "",
    "- First release.",
    "",
  ].join("\n");

// A repository with a private git config, so the signing settings of the
// person who runs the tests stay out of the fixture.
const createRepo = (t) => {
  const root = realpathSync(mkdtempSync(join(tmpdir(), "changelog-fragments-")));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const globalConfig = join(root, "gitconfig");
  writeFileSync(
    globalConfig,
    "[user]\n\tname = Test\n\temail = test@example.invalid\n[commit]\n\tgpgsign = false\n[init]\n\tdefaultBranch = main\n",
  );
  const work = join(root, "work");
  mkdirSync(work);
  const env = { ...process.env, GIT_CONFIG_GLOBAL: globalConfig, GIT_CONFIG_NOSYSTEM: "1" };
  for (const name of ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_COMMON_DIR"]) {
    delete env[name];
  }
  let clock = 1_700_000_000;
  const git = (...args) =>
    execFileSync("git", args, { cwd: work, env, encoding: "utf8", stdio: "pipe" }).trim();
  const write = (path, text) => {
    mkdirSync(dirname(join(work, path)), { recursive: true });
    writeFileSync(join(work, path), text);
  };
  const commit = (message, files) => {
    for (const [path, text] of Object.entries(files)) {
      write(path, text);
    }
    git("add", "--all");
    clock += 60;
    const date = `@${clock} +0000`;
    execFileSync("git", ["commit", "--quiet", "-m", message], {
      cwd: work,
      env: { ...env, GIT_AUTHOR_DATE: date, GIT_COMMITTER_DATE: date },
      stdio: "pipe",
    });
  };
  const read = (path) => readFileSync(join(work, path), "utf8");
  git("init", "--quiet");
  return { work, git, write, commit, read };
};

const run = (repo, ...argv) => {
  const lines = [];
  const code = changelogFragments(argv, repo.work, (line) => lines.push(line));
  return { code, output: lines.join("\n") };
};

test("a valid fragment has no problems", () => {
  assert.deepEqual(
    fragmentProblems("changelog.d/a.md", "### Added\n\n- **New.**\n\n### Fixed\n\n- **Fix.**\n"),
    [],
  );
});

test("fragment problems name the path and the rule", () => {
  const dash = String.fromCodePoint(0x2014);
  assert.deepEqual(
    fragmentProblems(
      "changelog.d/a.md",
      `Intro.\n\n### Added\n\n- One ${dash} two.\n\n### Notes\n\n- X.\n\n### Added\n\n- Again.\n\n### Fixed\n`,
    ),
    [
      "changelog.d/a.md: has text before its first ### heading",
      'changelog.d/a.md: has the unknown heading "### Notes" (use one of: Breaking, Added, Changed, Deprecated, Removed, Fixed, Performance, Security)',
      'changelog.d/a.md: repeats "### Added"',
      'changelog.d/a.md: has an empty "### Fixed" section',
      "changelog.d/a.md: contains an em-dash, which the release notes inherit",
    ],
  );
  assert.deepEqual(fragmentProblems("changelog.d/b.md", "- No heading.\n"), [
    "changelog.d/b.md: has text before its first ### heading",
    "changelog.d/b.md: has no ### heading (use one of: Breaking, Added, Changed, Deprecated, Removed, Fixed, Performance, Security)",
  ]);
});

test("a ### line in a fenced code block is entry text", () => {
  const { sections } = splitSections("### Added\n\n- Example:\n\n  ```md\n  ### Fixed\n  ```\n");
  assert.deepEqual(
    sections.map(({ name }) => name),
    ["Added"],
  );
  assert.match(sections[0].body, /### Fixed/u);
});

test("the merge keeps existing entries first and gives each heading once", () => {
  const existing =
    "\n### Fixed\n\n- Old fix.\n\n### Internal\n\n- Old note.\n\n### Fixed\n\n- Second old fix.\n\n";
  const merged = mergeUnreleased(existing, [
    "### Fixed\n\n- New fix.\n",
    "### Added\n\n- New feature.\n",
  ]);
  assert.equal(
    merged,
    [
      "### Added",
      "",
      "- New feature.",
      "",
      "### Fixed",
      "",
      "- Old fix.",
      "",
      "- Second old fix.",
      "",
      "- New fix.",
      "",
      "### Internal",
      "",
      "- Old note.",
      "",
    ].join("\n"),
  );
  assert.equal(mergeUnreleased("\n\n", []), "");
});

test("collect moves fragments into [Unreleased] in merge order and deletes them", (t) => {
  const repo = createRepo(t);
  repo.commit("chore: base", {
    "CHANGELOG.md": changelog("### Fixed", "", "- Existing fix.", ""),
    "changelog.d/README.md": "# Fragments\n",
  });
  repo.commit("feat: later name, merged first", {
    "changelog.d/zeta.md": "### Added\n\n- **Zeta.**\n",
  });
  repo.commit("fix: earlier name, merged second", {
    "changelog.d/alpha.md": "### Fixed\n\n- **Alpha fix.**\n\n### Added\n\n- **Alpha.**\n",
  });
  repo.write("changelog.d/untracked.md", "### Added\n\n- **Untracked.**\n");

  const preview = run(repo, "collect", "--dry-run");
  assert.equal(preview.code, 0);
  assert.equal(fragmentNames(join(repo.work, "changelog.d")).length, 3, "dry run keeps files");

  assert.equal(run(repo, "collect").code, 0);
  const expected = changelog(
    "### Added",
    "",
    "- **Zeta.**",
    "",
    "- **Alpha.**",
    "",
    "- **Untracked.**",
    "",
    "### Fixed",
    "",
    "- Existing fix.",
    "",
    "- **Alpha fix.**",
    "",
  );
  assert.equal(repo.read("CHANGELOG.md"), expected);
  assert.equal(
    `${preview.output}\n`,
    expected.split("## [Unreleased]\n\n")[1].split("## [1.0.0]")[0],
  );
  assert.deepEqual(fragmentNames(join(repo.work, "changelog.d")), []);
  assert.ok(existsSync(join(repo.work, "changelog.d/README.md")), "collect keeps the README");
});

test("collect refuses invalid fragments and changes nothing", (t) => {
  const repo = createRepo(t);
  repo.commit("chore: base", {
    "CHANGELOG.md": changelog(),
    "changelog.d/bad.md": "### Notes\n\n- X.\n",
  });
  assert.throws(() => run(repo, "collect"), /changelog\.d\/bad\.md: has the unknown heading/u);
  assert.equal(repo.read("CHANGELOG.md"), changelog());
  assert.ok(existsSync(join(repo.work, "changelog.d/bad.md")));
});

test("check --base fails a branch that edits [Unreleased]", (t) => {
  const repo = createRepo(t);
  repo.commit("chore: base", { "CHANGELOG.md": changelog() });
  repo.git("checkout", "--quiet", "-b", "feat");
  repo.commit("feat: entry in the changelog", {
    "CHANGELOG.md": changelog("### Added", "", "- **Direct entry.**", ""),
  });
  assert.throws(
    () => run(repo, "check", "--base", "main"),
    /this branch changes the \[Unreleased\] section\. Move the new entries to a file in changelog\.d\//u,
  );
});

test("check --base reads a changelog larger than the default process buffer", (t) => {
  const repo = createRepo(t);
  const history = `${"- Old entry.\n".repeat(120_000)}\n`;
  repo.commit("chore: base", { "CHANGELOG.md": `${changelog()}${history}` });
  repo.git("checkout", "--quiet", "-b", "feat");
  repo.commit("feat: fragment", { "changelog.d/feat.md": "### Added\n\n- **New.**\n" });

  assert.ok(repo.read("CHANGELOG.md").length > 1024 * 1024);
  assert.equal(run(repo, "check", "--base", "main").code, 0);
});

test("check --base passes a fragment branch after a release on the base branch", (t) => {
  const repo = createRepo(t);
  repo.commit("chore: base", {
    "CHANGELOG.md": changelog("### Fixed", "", "- Old fix.", ""),
  });
  repo.git("checkout", "--quiet", "-b", "feat");
  repo.commit("feat: fragment", { "changelog.d/feat.md": "### Added\n\n- **New.**\n" });
  repo.git("checkout", "--quiet", "main");
  repo.commit("chore: release", { "CHANGELOG.md": changelog() });
  repo.git("checkout", "--quiet", "feat");

  const result = run(repo, "check", "--base", "main");
  assert.equal(result.code, 0);
  assert.equal(result.output, "changelog.d: 1 fragment(s) are valid.");
});

test("arguments belong to their command", () => {
  assert.deepEqual(parseArgs(["check", "--base", "origin/main"]), {
    command: "check",
    base: "origin/main",
    dryRun: false,
  });
  assert.throws(() => parseArgs(["collect", "--base", "main"]), /Unknown option '--base'/u);
  assert.throws(() => parseArgs(["check", "--dry-run"]), /Unknown option '--dry-run'/u);
  assert.throws(() => parseArgs(["check", "--base"]), /argument missing/u);
  assert.throws(() => parseArgs(["toString"]), /Usage:/u);
  assert.throws(() => parseArgs([]), /Usage:/u);
});
