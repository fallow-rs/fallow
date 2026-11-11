import assert from "node:assert/strict";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { execFileSync, spawnSync } from "node:child_process";
import { realpathSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";

import {
  GIT_LOCATION_VARIABLES,
  companionSkillsRoot,
  decide,
  diffTrees,
  listFiles,
  main,
  pinLaunchers,
  releasedSkillPairs,
  runCheck,
  runVendor,
  stripUnsupportedMetadata,
} from "./vendor-skills.mjs";

const makeTree = (files) => {
  const dir = mkdtempSync(join(tmpdir(), "vendor-skills-"));
  for (const [path, content] of Object.entries(files)) {
    const destination = join(dir, path);
    mkdirSync(dirname(destination), { recursive: true });
    writeFileSync(destination, content);
  }
  return dir;
};

test("listFiles is recursive, sorted, and skips dotfiles", () => {
  const dir = makeTree({
    "SKILL.md": "a",
    "references/mcp.md": "b",
    "references/cli.md": "c",
    ".DS_Store": "junk",
  });
  assert.deepEqual(listFiles(dir), ["SKILL.md", "references/cli.md", "references/mcp.md"]);
  rmSync(dir, { recursive: true });
});

test("listFiles rejects symlinks", () => {
  const dir = makeTree({ "SKILL.md": "a" });
  symlinkSync(join(dir, "SKILL.md"), join(dir, "linked.md"));
  assert.throws(() => listFiles(dir), /symlinks/u);
  rmSync(dir, { recursive: true });
});

test("metadata transform preserves the rest of SKILL.md", () => {
  const source = [
    "---",
    "name: fallow",
    "metadata:",
    "  version: 1.0.0",
    "license: MIT",
    "---",
    "# Fallow",
  ].join("\n");
  assert.equal(
    stripUnsupportedMetadata(source),
    ["---", "name: fallow", "license: MIT", "---", "# Fallow"].join("\n"),
  );
});

test("launcher transform pins each download-and-run form of the fallow package", () => {
  const source = [
    "npx fallow dead-code",
    "Run `npx fallow recommend` or `pnpm dlx fallow recommend`.",
    "bunx fallow audit; yarn dlx fallow audit; npx --yes fallow health",
    "npx --no-install fallow --version",
    "codex mcp add fallow -- npx --no fallow-mcp",
    "npx fallow@3.1.0 dead-code",
  ].join("\n");
  assert.equal(
    pinLaunchers(source, "3.42.0"),
    [
      "npx fallow@3.42.0 dead-code",
      "Run `npx fallow@3.42.0 recommend` or `pnpm dlx fallow@3.42.0 recommend`.",
      "bunx fallow@3.42.0 audit; yarn dlx fallow@3.42.0 audit; npx --yes fallow@3.42.0 health",
      "npx --no-install fallow --version",
      "codex mcp add fallow -- npx --no fallow-mcp",
      "npx fallow@3.1.0 dead-code",
    ].join("\n"),
  );
  assert.throws(() => pinLaunchers(source, "3"), /exact/u);
});

test("the vendored skill pins launchers to the version of its npm package", () => {
  const repo = makeTree({
    "npm/fallow/package.json": JSON.stringify({ name: "fallow", version: "3.42.0" }),
    "npm/fallow/skills/fallow/SKILL.md": "---\nname: fallow\n---\nRun `npx fallow dead-code`.\n",
    "npm/fallow/skills/fallow/references/ci.md": "run: npx fallow audit\n",
  });
  const published = makeTree({});
  const canonical = join(repo, "npm", "fallow", "skills", "fallow");
  assert.equal(runVendor(canonical, published), 0);
  assert.match(readFileSync(join(published, "SKILL.md"), "utf8"), /`npx fallow@3\.42\.0 dead-code`/u);
  assert.equal(readFileSync(join(published, "references/ci.md"), "utf8"), "run: npx fallow@3.42.0 audit\n");
  assert.equal(runCheck(canonical, published, { renderDiffs: false }), 0);
  rmSync(repo, { recursive: true });
  rmSync(published, { recursive: true });
});

test("a release does not make a pinned copy drift, but an unpinned copy always drifts", () => {
  const repo = makeTree({
    "npm/fallow/package.json": JSON.stringify({ name: "fallow", version: "3.43.0" }),
    "npm/fallow/skills/fallow/SKILL.md": "---\nname: fallow\n---\nRun `npx fallow dead-code` or `node fallow.js`.\n",
  });
  const canonical = join(repo, "npm", "fallow", "skills", "fallow");
  const pinned = makeTree({ "SKILL.md": "---\nname: fallow\n---\nRun `npx fallow@3.42.0 dead-code` or `node fallow.js`.\n" });
  const unpinned = makeTree({ "SKILL.md": "---\nname: fallow\n---\nRun `npx fallow dead-code` or `node fallow.js`.\n" });
  assert.equal(runCheck(canonical, pinned, { renderDiffs: false }), 0);
  assert.equal(runCheck(canonical, unpinned, { renderDiffs: false }), 1);
  assert.equal(pinLaunchers("npx fallow.js and npx fallow/x", "3.42.0"), "npx fallow.js and npx fallow/x");
  for (const dir of [repo, pinned, unpinned]) {
    rmSync(dir, { recursive: true });
  }
});

test("diffTrees ignores host adapters but reports contract drift", () => {
  const canonical = makeTree({
    "SKILL.md": "---\nname: fallow\nmetadata:\n  version: 1.0.0\n---\n# Fallow\n",
    "references/mcp.md": "canonical",
  });
  const published = makeTree({
    "SKILL.md": "---\nname: fallow\n---\n# Fallow\n",
    "references/mcp.md": "published",
    "agents/openai.yaml": "interface: {}",
  });
  assert.deepEqual(diffTrees(canonical, published), {
    missing: [],
    extra: [],
    changed: ["references/mcp.md"],
  });
  rmSync(canonical, { recursive: true });
  rmSync(published, { recursive: true });
});

test("runCheck returns success only for the transformed exact contract", () => {
  const canonical = makeTree({
    "SKILL.md": "---\nname: fallow\nmetadata:\n  version: 1.0.0\n---\n",
  });
  const matching = makeTree({ "SKILL.md": "---\nname: fallow\n---\n" });
  const drifted = makeTree({ "SKILL.md": "---\nname: other\n---\n" });
  assert.equal(runCheck(canonical, matching, { renderDiffs: false }), 0);
  assert.equal(runCheck(canonical, drifted, { renderDiffs: false }), 1);
  for (const dir of [canonical, matching, drifted]) {
    rmSync(dir, { recursive: true });
  }
});

test("runVendor updates contract files without deleting host adapters", () => {
  const canonical = makeTree({
    "SKILL.md": "---\nname: fallow\nmetadata:\n  version: 1.0.0\n---\n",
    "references/mcp.md": "new",
  });
  const published = makeTree({
    "SKILL.md": "old",
    "references/stale.md": "remove",
    "agents/openai.yaml": "interface: {}",
  });
  assert.equal(runVendor(canonical, published), 0);
  assert.deepEqual(diffTrees(canonical, published), {
    missing: [],
    extra: [],
    changed: [],
  });
  assert.equal(existsSync(join(published, "agents/openai.yaml")), true);
  assert.equal(readFileSync(join(published, "references/mcp.md"), "utf8"), "new");
  assert.equal(existsSync(join(published, "references/stale.md")), false);
  rmSync(canonical, { recursive: true });
  rmSync(published, { recursive: true });
});

test("missing companion checkout always fails closed", () => {
  assert.equal(decide({ present: false, check: true }).action, "error");
  assert.equal(decide({ present: false, check: false }).action, "error");
  assert.equal(decide({ present: true, check: true }).action, "check");
  assert.equal(decide({ present: true, check: false }).action, "vendor");
});

test("main throws when the explicit public consumer is missing", () => {
  const previous = process.env.FALLOW_SKILLS_DIR;
  process.env.FALLOW_SKILLS_DIR = join(tmpdir(), "vendor-skills-missing-consumer");
  try {
    assert.throws(() => main(["--check"]), /not found/u);
    assert.throws(() => main([]), /not found/u);
  } finally {
    if (previous === undefined) {
      delete process.env.FALLOW_SKILLS_DIR;
    } else {
      process.env.FALLOW_SKILLS_DIR = previous;
    }
  }
});

// An inherited GIT_DIR or GIT_WORK_TREE, for example from a hook, would point
// these commands at the real repository instead of the temporary one.
const gitEnv = () =>
  Object.fromEntries(
    Object.entries(process.env).filter(([name]) => !GIT_LOCATION_VARIABLES.includes(name)),
  );

const git = (cwd, ...args) => execFileSync("git", args, { cwd, env: gitEnv(), stdio: "pipe" });

test("a linked worktree resolves the companion next to the main working tree", () => {
  const base = realpathSync(mkdtempSync(join(tmpdir(), "vendor-skills-worktree-")));
  const mainTree = join(base, "fallow");
  const linked = join(base, "fallow", ".worktrees", "linked");
  try {
    mkdirSync(mainTree);
    git(mainTree, "init", "-q");
    git(
      mainTree,
      "-c",
      "commit.gpgsign=false",
      "-c",
      "user.name=t",
      "-c",
      "user.email=t@example.com",
      "commit",
      "-q",
      "--allow-empty",
      "-m",
      "init",
    );
    git(mainTree, "worktree", "add", "-q", linked);

    assert.equal(companionSkillsRoot({ env: {}, repoRoot: linked }), join(base, "fallow-skills"));
    assert.equal(companionSkillsRoot({ env: {}, repoRoot: mainTree }), join(base, "fallow-skills"));
  } finally {
    rmSync(base, { recursive: true, force: true });
  }
});

test("the git helper of this file ignores an inherited GIT_DIR", () => {
  const base = realpathSync(mkdtempSync(join(tmpdir(), "vendor-skills-gitdir-")));
  const inherited = join(base, "inherited.git");
  const previous = process.env.GIT_DIR;
  process.env.GIT_DIR = inherited;
  try {
    const checkout = join(base, "fallow");
    mkdirSync(checkout);
    git(checkout, "init", "-q");
    assert.equal(existsSync(inherited), false, "git init must not write to the inherited GIT_DIR");
    assert.equal(existsSync(join(checkout, ".git")), true);
  } finally {
    if (previous === undefined) {
      delete process.env.GIT_DIR;
    } else {
      process.env.GIT_DIR = previous;
    }
    rmSync(base, { recursive: true, force: true });
  }
});

test("FALLOW_SKILLS_DIR overrides the companion location", () => {
  assert.equal(
    companionSkillsRoot({ env: { FALLOW_SKILLS_DIR: "/elsewhere/skills" }, repoRoot: "/unused" }),
    "/elsewhere/skills",
  );
});

test("a checkout outside git falls back to the sibling directory", () => {
  const base = realpathSync(mkdtempSync(join(tmpdir(), "vendor-skills-nogit-")));
  const checkout = join(base, "fallow");
  try {
    mkdirSync(checkout);
    assert.equal(
      companionSkillsRoot({ env: { GIT_CEILING_DIRECTORIES: base }, repoRoot: checkout }),
      join(base, "fallow-skills"),
    );
  } finally {
    rmSync(base, { recursive: true, force: true });
  }
});

test("main refuses an unknown argument before it touches the companion repo", () => {
  const previous = process.env.FALLOW_SKILLS_DIR;
  process.env.FALLOW_SKILLS_DIR = join(tmpdir(), "vendor-skills-missing-consumer");
  try {
    assert.throws(() => main(["--frobnicate"]), /unknown argument: --frobnicate/u);
    assert.throws(() => main(["--check", "--force"]), /unknown argument: --force/u);
  } finally {
    if (previous === undefined) {
      delete process.env.FALLOW_SKILLS_DIR;
    } else {
      process.env.FALLOW_SKILLS_DIR = previous;
    }
  }
});

test("main prints the usage for --help and writes nothing", () => {
  const previous = process.env.FALLOW_SKILLS_DIR;
  process.env.FALLOW_SKILLS_DIR = join(tmpdir(), "vendor-skills-missing-consumer");
  const lines = [];
  const original = console.log;
  console.log = (line) => lines.push(line);
  try {
    assert.equal(main(["--help"]), 0);
    assert.equal(main(["-h"]), 0);
  } finally {
    console.log = original;
    if (previous === undefined) {
      delete process.env.FALLOW_SKILLS_DIR;
    } else {
      process.env.FALLOW_SKILLS_DIR = previous;
    }
  }
  assert.match(lines.join("\n"), /Usage:/u);
});

test("every released skill maps to its own published tree", () => {
  const repoRoot = makeTree({
    "npm/fallow/skills/fallow/SKILL.md": "---\nname: fallow\n---\n",
    "npm/fallow/skills/fallow-setup/SKILL.md": "---\nname: fallow-setup\n---\n",
    "npm/fallow/skills/_artifacts/notes.txt": "build output\n",
  });
  assert.deepEqual(releasedSkillPairs(repoRoot, "/companion"), [
    {
      name: "fallow",
      canonical: join(repoRoot, "npm/fallow/skills/fallow"),
      published: join("/companion", "fallow/skills/fallow"),
    },
    {
      name: "fallow-setup",
      canonical: join(repoRoot, "npm/fallow/skills/fallow-setup"),
      published: join("/companion", "fallow/skills/fallow-setup"),
    },
  ]);
});

test("a released skill without a published tree is drift", () => {
  const canonical = makeTree({ "SKILL.md": "---\nname: fallow-setup\n---\n" });
  const missing = join(tmpdir(), "vendor-skills-absent-published-tree");
  assert.deepEqual(diffTrees(canonical, missing).missing, ["SKILL.md"]);
  assert.equal(runCheck(canonical, missing, { renderDiffs: false }), 1);
});

test("base parity permits candidate changes but rejects published drift and strict current drift", () => {
  const sourceSkill = "---\nname: fallow\nmetadata:\n  version: 1.0.0\n---\n# Fallow\n";
  const publishedSkill = stripUnsupportedMetadata(sourceSkill);
  const baseReference = "The released command contract.\n";
  const candidateReference = "The candidate command contract adds a documented fix.\n";
  const scriptFiles = Object.fromEntries(
    ["vendor-skills.mjs", "released-skills.mjs"].map((name) => [
      `scripts/${name}`,
      readFileSync(new URL(name, import.meta.url), "utf8"),
    ]),
  );
  const sourceFiles = {
    ...scriptFiles,
    "npm/fallow/skills/fallow/SKILL.md": sourceSkill,
  };
  const fixture = makeTree({
    ...Object.fromEntries(
      Object.entries(sourceFiles).map(([path, text]) => [`base/${path}`, text]),
    ),
    ...Object.fromEntries(
      Object.entries(sourceFiles).map(([path, text]) => [`candidate/${path}`, text]),
    ),
    "base/npm/fallow/skills/fallow/references/cli.md": baseReference,
    "candidate/npm/fallow/skills/fallow/references/cli.md": candidateReference,
    "companion/fallow/skills/fallow/SKILL.md": publishedSkill,
    "companion/fallow/skills/fallow/references/cli.md": baseReference,
  });
  const check = (source, args = ["--check"]) =>
    spawnSync(process.execPath, ["scripts/vendor-skills.mjs", ...args], {
      cwd: join(fixture, source),
      env: { ...gitEnv(), FALLOW_SKILLS_DIR: join(fixture, "companion") },
      encoding: "utf8",
    });
  try {
    const base = check("base");
    assert.equal(base.status, 0, base.stderr);
    const current = check("candidate");
    assert.equal(current.status, 1, current.stderr);
    assert.match(current.stderr, /differs: references\/cli\.md/u);
    assert.equal(
      readFileSync(join(fixture, "companion/fallow/skills/fallow/references/cli.md"), "utf8"),
      baseReference,
      "checks must not publish candidate content",
    );

    writeFileSync(
      join(fixture, "companion/fallow/skills/fallow/references/cli.md"),
      "stale content\n",
    );
    const publishedDrift = check("base");
    assert.equal(publishedDrift.status, 1, publishedDrift.stderr);
    assert.match(publishedDrift.stderr, /differs: references\/cli\.md/u);

    const synchronized = check("candidate", []);
    assert.equal(synchronized.status, 0, synchronized.stderr);
    const currentAfterSync = check("candidate");
    assert.equal(currentAfterSync.status, 0, currentAfterSync.stderr);
  } finally {
    rmSync(fixture, { recursive: true, force: true });
  }
});
