import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { devNull, tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

import { GIT_LOCATION_VARIABLES } from "./vendor-skills.mjs";
import { commitMessage, prNumbers, run } from "./sync-skills-companion.mjs";

const SCRIPT = join(dirname(fileURLToPath(import.meta.url)), "sync-skills-companion.mjs");
const OLD_VERSION = "1.2.3";
const NEW_VERSION = "1.2.4";
const SOURCE_SKILL = "---\nname: fallow\nmetadata:\n  version: 1.0.0\n---\n# Fallow\n";
const PUBLISHED_SKILL = "---\nname: fallow\n---\n# Fallow\n";
const CLI_REFERENCE = "npm/fallow/skills/fallow/references/cli.md";
const PUBLISHED_CLI_REFERENCE = "fallow/skills/fallow/references/cli.md";
const MANIFESTS = [
  "fallow/.codex-plugin/plugin.json",
  "fallow/.claude-plugin/plugin.json",
  ".claude-plugin/marketplace.json",
];

// The temp repositories must not read the global or system git config of the
// person who runs the tests, and a git hook must not point git elsewhere.
const GIT_ENV = (() => {
  const env = {
    ...process.env,
    GIT_CONFIG_GLOBAL: devNull,
    GIT_CONFIG_NOSYSTEM: "1",
    GIT_AUTHOR_NAME: "Test",
    GIT_AUTHOR_EMAIL: "test@example.com",
    GIT_COMMITTER_NAME: "Test",
    GIT_COMMITTER_EMAIL: "test@example.com",
  };
  for (const name of [...GIT_LOCATION_VARIABLES, "FALLOW_SKILLS_DIR"]) {
    delete env[name];
  }
  return env;
})();

const git = (cwd, ...args) =>
  execFileSync("git", args, {
    cwd,
    env: GIT_ENV,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  }).trim();

const write = (root, files) => {
  for (const [path, content] of Object.entries(files)) {
    mkdirSync(dirname(join(root, path)), { recursive: true });
    writeFileSync(join(root, path), content);
  }
};

const commitAll = (cwd, message) => {
  git(cwd, "add", "-A");
  git(cwd, "commit", "-q", "-m", message);
  return git(cwd, "rev-parse", "HEAD");
};

const sourceLock = (commit) =>
  `${JSON.stringify(
    {
      schemaVersion: 2,
      repository: "https://github.com/fallow-rs/fallow",
      commit,
      skills: [{ name: "fallow", sourceRoot: "npm/fallow/skills/fallow" }],
    },
    null,
    2,
  )}\n`;

const manifestFiles = (versions) => ({
  [MANIFESTS[0]]: `{\n  "name": "fallow",\n  "version": "${versions[0]}",\n  "skills": "./skills/"\n}\n`,
  [MANIFESTS[1]]: `{\n  "name": "fallow",\n  "version": "${versions[1]}"\n}\n`,
  [MANIFESTS[2]]:
    `{\n  "name": "fallow",\n  "metadata": {\n    "version": "${versions[2]}"\n  },\n` +
    `  "plugins": [\n    {\n      "name": "fallow",\n      "version": "${versions[3]}"\n    }\n  ]\n}\n`,
});

const versionsOf = (skills) => {
  const codex = JSON.parse(readFileSync(join(skills, MANIFESTS[0]), "utf8"));
  const claude = JSON.parse(readFileSync(join(skills, MANIFESTS[1]), "utf8"));
  const marketplace = JSON.parse(readFileSync(join(skills, MANIFESTS[2]), "utf8"));
  return [
    codex.version,
    claude.version,
    marketplace.metadata.version,
    marketplace.plugins[0].version,
  ];
};

/**
 * Build a fallow repository and a companion repository, each with a bare
 * `origin`, under one temp directory. The companion signs with a throwaway
 * SSH key, so `git commit -S` works without the keys of the person who runs
 * the tests.
 */
const makeFixture = (versions = [OLD_VERSION, OLD_VERSION, OLD_VERSION, OLD_VERSION]) => {
  const root = realpathSync(mkdtempSync(join(tmpdir(), "sync-skills-companion-")));
  const fallow = join(root, "fallow");
  const skills = join(root, "fallow-skills");

  git(root, "init", "-q", "--bare", "-b", "main", "fallow-origin.git");
  git(root, "init", "-q", "-b", "main", "fallow");
  git(fallow, "remote", "add", "origin", join(root, "fallow-origin.git"));
  write(fallow, {
    "npm/fallow/skills/fallow/SKILL.md": SOURCE_SKILL,
    [CLI_REFERENCE]: "one\n",
    "README.md": "readme\n",
  });
  const firstPin = commitAll(fallow, "feat: add the skill (#10)");
  git(fallow, "push", "-q", "origin", "main");

  git(root, "init", "-q", "--bare", "-b", "main", "skills-origin.git");
  git(root, "init", "-q", "-b", "main", "fallow-skills");
  git(skills, "remote", "add", "origin", join(root, "skills-origin.git"));
  execFileSync("ssh-keygen", ["-q", "-t", "ed25519", "-N", "", "-f", join(root, "key")]);
  git(skills, "config", "gpg.format", "ssh");
  git(skills, "config", "gpg.ssh.program", "ssh-keygen");
  git(skills, "config", "user.signingkey", join(root, "key"));
  git(skills, "config", "commit.gpgsign", "false");
  write(skills, {
    "fallow/skills/fallow/SKILL.md": PUBLISHED_SKILL,
    [PUBLISHED_CLI_REFERENCE]: "one\n",
    "source-lock.json": sourceLock(firstPin),
    ...manifestFiles(versions),
  });
  commitAll(skills, "chore: start the companion");
  git(skills, "push", "-q", "origin", "main");

  return { root, fallow, skills, firstPin };
};

/** Run the script in process with the fixture paths and collect its output. */
const sync = (fixture, argv = [], { skipChecks = true } = {}) => {
  const lines = [];
  const code = run({
    argv,
    repoRoot: fixture.fallow,
    env: { ...GIT_ENV, FALLOW_SKILLS_DIR: fixture.skills },
    skipChecks,
    log: (line) => lines.push(line),
    error: (line) => lines.push(line),
  });
  return { code, output: lines.join("\n") };
};

const changeSkill = (fixture, content, message) => {
  write(fixture.fallow, { [CLI_REFERENCE]: content });
  const sha = commitAll(fixture.fallow, message);
  git(fixture.fallow, "push", "-q", "origin", "main");
  return sha;
};

const companionState = (skills) => ({
  head: git(skills, "rev-parse", "HEAD"),
  status: git(skills, "status", "--porcelain"),
});

const withFixture = (body, versions) => () => {
  const fixture = makeFixture(versions);
  try {
    body(fixture);
  } finally {
    rmSync(fixture.root, { recursive: true, force: true });
  }
};

test("prNumbers reads every pull request number from the subjects once", () => {
  assert.deepEqual(prNumbers(["fix: b (#43)", "chore: x", "feat: a (#42) (#7)", "docs: c (#43)"]), [
    "43",
    "42",
    "7",
  ]);
});

test("commitMessage names the pull requests or else the short pin", () => {
  assert.equal(
    commitMessage({ numbers: ["42", "43"], pin: "abcdef0123456789" }),
    "docs: sync the Fallow skill from fallow (#42, #43)",
  );
  assert.equal(
    commitMessage({ numbers: [], pin: "abcdef0123456789" }),
    "docs: sync the Fallow skill from fallow abcdef012345",
  );
});

test(
  "refuses a companion with uncommitted changes",
  withFixture((fixture) => {
    changeSkill(fixture, "two\n", "fix: update the reference (#42)");
    write(fixture.skills, { "notes.txt": "draft\n" });
    const { code, output } = sync(fixture);
    assert.equal(code, 1);
    assert.match(output, /uncommitted changes/u);
    assert.equal(readFileSync(join(fixture.skills, PUBLISHED_CLI_REFERENCE), "utf8"), "one\n");
  }),
);

test(
  "refuses a companion that is behind its origin",
  withFixture((fixture) => {
    changeSkill(fixture, "two\n", "fix: update the reference (#42)");
    const other = join(fixture.root, "other");
    git(fixture.root, "clone", "-q", join(fixture.root, "skills-origin.git"), "other");
    write(other, { "README.md": "newer\n" });
    commitAll(other, "docs: newer readme");
    git(other, "push", "-q", "origin", "main");
    const before = companionState(fixture.skills);
    const { code, output } = sync(fixture);
    assert.equal(code, 1);
    assert.match(output, /origin\/main/u);
    assert.deepEqual(companionState(fixture.skills), before);
  }),
);

test(
  "refuses a pin that is not on origin/main",
  withFixture((fixture) => {
    write(fixture.fallow, { [CLI_REFERENCE]: "local\n" });
    const local = commitAll(fixture.fallow, "fix: local only (#50)");
    const before = companionState(fixture.skills);

    const head = sync(fixture);
    assert.equal(head.code, 1);
    assert.match(head.output, /not on origin\/main/u);

    git(fixture.fallow, "reset", "-q", "--hard", "origin/main");
    const pinned = sync(fixture, ["--pin", local]);
    assert.equal(pinned.code, 1);
    assert.match(pinned.output, /not on origin\/main/u);
    assert.deepEqual(companionState(fixture.skills), before);
  }),
);

test(
  "refuses uncommitted changes under npm/fallow/skills",
  withFixture((fixture) => {
    write(fixture.fallow, { [CLI_REFERENCE]: "edited\n" });
    const { code, output } = sync(fixture);
    assert.equal(code, 1);
    assert.match(output, /npm\/fallow\/skills/u);
  }),
);

test(
  "does nothing when the trees already match",
  withFixture((fixture) => {
    const before = companionState(fixture.skills);
    const { code, output } = sync(fixture);
    assert.equal(code, 0);
    assert.match(output, /nothing to sync/u);
    assert.deepEqual(companionState(fixture.skills), before);
  }),
);

test(
  "a sync vendors the skill, raises the versions, repins, and commits once",
  withFixture((fixture) => {
    changeSkill(fixture, "two\n", "fix: update the reference (#42)");
    write(fixture.fallow, { "README.md": "elsewhere\n" });
    commitAll(fixture.fallow, "chore: unrelated (#99)");
    const pin = changeSkill(fixture, "three\n", "docs: reword the reference (#43)");
    const before = git(fixture.skills, "rev-parse", "HEAD");

    const { code, output } = sync(fixture);
    assert.equal(code, 0, output);

    const skills = fixture.skills;
    assert.equal(readFileSync(join(skills, PUBLISHED_CLI_REFERENCE), "utf8"), "three\n");
    assert.deepEqual(versionsOf(skills), [NEW_VERSION, NEW_VERSION, NEW_VERSION, NEW_VERSION]);
    const lock = JSON.parse(readFileSync(join(skills, "source-lock.json"), "utf8"));
    assert.equal(lock.commit, pin);
    assert.equal(lock.skills[0].sourceRoot, "npm/fallow/skills/fallow");

    assert.equal(git(skills, "rev-parse", "HEAD~1"), before);
    assert.equal(
      git(skills, "log", "-1", "--format=%s"),
      "docs: sync the Fallow skill from fallow (#42, #43)",
    );
    assert.match(git(skills, "cat-file", "commit", "HEAD"), /^gpgsig /mu);
    assert.equal(git(skills, "status", "--porcelain"), "");
    assert.equal(git(skills, "rev-parse", "origin/main"), before);
    assert.match(output, /push origin main/u);
    assert.match(output, /Public skills contract/u);
  }),
);

test(
  "--pin vendors the tree of that commit, not the working tree",
  withFixture((fixture) => {
    const pin = changeSkill(fixture, "two\n", "fix: update the reference (#42)");
    changeSkill(fixture, "three\n", "docs: reword the reference (#43)");
    const { code, output } = sync(fixture, ["--pin", pin, "--message", "docs: custom sync"]);
    assert.equal(code, 0, output);
    assert.equal(readFileSync(join(fixture.skills, PUBLISHED_CLI_REFERENCE), "utf8"), "two\n");
    const lock = JSON.parse(readFileSync(join(fixture.skills, "source-lock.json"), "utf8"));
    assert.equal(lock.commit, pin);
    assert.equal(git(fixture.skills, "log", "-1", "--format=%s"), "docs: custom sync");
  }),
);

test(
  "refuses manifest versions that differ",
  withFixture(
    (fixture) => {
      changeSkill(fixture, "two\n", "fix: update the reference (#42)");
      const before = companionState(fixture.skills);
      const { code, output } = sync(fixture);
      assert.equal(code, 1);
      assert.match(output, /versions differ/u);
      assert.deepEqual(companionState(fixture.skills), before);
    },
    [OLD_VERSION, OLD_VERSION, OLD_VERSION, "1.2.0"],
  ),
);

test(
  "--dry-run reports the change and writes nothing",
  withFixture((fixture) => {
    changeSkill(fixture, "two\n", "fix: update the reference (#42)");
    const before = companionState(fixture.skills);
    const { code, output } = sync(fixture, ["--dry-run"]);
    assert.equal(code, 0, output);
    assert.match(output, /references\/cli\.md/u);
    assert.match(output, /1\.2\.3 -> 1\.2\.4/u);
    assert.deepEqual(companionState(fixture.skills), before);
  }),
);

test("an unknown argument exits 2 with the usage", () => {
  const result = spawnSync(process.execPath, [SCRIPT, "--bogus"], {
    encoding: "utf8",
    env: { ...GIT_ENV, FALLOW_SKILLS_DIR: join(tmpdir(), "missing-companion") },
  });
  assert.equal(result.status, 2);
  assert.match(result.stderr, /unknown argument: --bogus/u);
  assert.match(result.stderr, /Usage:/u);
});

test("--pin without a value exits 2", () => {
  const result = spawnSync(process.execPath, [SCRIPT, "--pin"], {
    encoding: "utf8",
    env: { ...GIT_ENV, FALLOW_SKILLS_DIR: join(tmpdir(), "missing-companion") },
  });
  assert.equal(result.status, 2);
  assert.match(result.stderr, /--pin needs a value/u);
});

test(
  "refuses an unexpected source-lock shape before it writes any file",
  withFixture((fixture) => {
    const lockPath = join(fixture.skills, "source-lock.json");
    const lock = JSON.parse(readFileSync(lockPath, "utf8"));
    lock.previous = { commit: lock.commit };
    writeFileSync(lockPath, `${JSON.stringify(lock, null, 2)}\n`);
    commitAll(fixture.skills, "chore: keep the previous pin");
    git(fixture.skills, "push", "-q", "origin", "main");
    changeSkill(fixture, "two\n", "fix: update the reference (#42)");
    const before = companionState(fixture.skills);
    const { code, output } = sync(fixture);
    assert.equal(code, 1, output);
    assert.match(output, /expected 1 "commit" value/u);
    assert.deepEqual(companionState(fixture.skills), before);
  }),
);
