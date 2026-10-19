// Tests for check-dco.mjs, the sign-off check in commitlint.yml.
//
// Run: node --test .github/scripts/check-dco.test.mjs

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { checkDco, parseLog } from "./check-dco.mjs";

const SCRIPT_PATH = fileURLToPath(new URL("./check-dco.mjs", import.meta.url));

const commit = (message, authorEmail = "dev@example.com") => ({
  sha: "abc123",
  authorEmail,
  message,
});

test("a commit signed off by its author passes", () => {
  const message = "fix: x\n\nBody.\n\nSigned-off-by: Dev One <dev@example.com>\n";
  assert.deepEqual(checkDco([commit(message)]), { ok: true });
});

test("a commit without a sign-off fails", () => {
  assert.deepEqual(checkDco([commit("fix: x\n")]), { ok: false, missing: ["abc123"] });
});

test("a sign-off by another person fails", () => {
  const message = "fix: x\n\nSigned-off-by: Other <other@example.com>\n";
  assert.equal(checkDco([commit(message)]).ok, false);
});

test("the email comparison ignores case", () => {
  const message = "fix: x\n\nSigned-off-by: Dev One <Dev@Example.com>\n";
  assert.deepEqual(checkDco([commit(message)]), { ok: true });
});

test("a co-author sign-off does not replace the author sign-off", () => {
  const message =
    "fix: x\n\nSigned-off-by: Other <other@example.com>\nSigned-off-by: Dev One <dev@example.com>\n";
  assert.deepEqual(checkDco([commit(message)]), { ok: true });
});

test("parseLog splits records into commits", () => {
  const log =
    "aaa\u001fa@example.com\u001ffix: a\n\nSigned-off-by: A <a@example.com>\n\u001e\nbbb\u001fb@example.com\u001ffix: b\n\u001e";
  const commits = parseLog(log);
  assert.equal(commits.length, 2);
  assert.equal(commits[1].sha, "bbb");
  assert.deepEqual(checkDco(commits), { ok: false, missing: ["bbb"] });
});

// A git variable from the caller (for example GIT_AUTHOR_EMAIL, or GIT_DIR
// from a hook) would change the author that the script reads.
const cleanEnv = (extra = {}) => {
  const env = { ...process.env, ...extra };
  for (const key of Object.keys(env)) {
    if (/^GIT_(AUTHOR|COMMITTER)_/u.test(key) || key === "GIT_DIR" || key === "GIT_INDEX_FILE") {
      delete env[key];
    }
  }
  return env;
};

const git = (cwd, args) => {
  const run = spawnSync("git", args, { cwd, env: cleanEnv(), encoding: "utf8" });
  assert.equal(run.status, 0, run.stderr);
  return run.stdout.trim();
};

const withRepo = (callback) => {
  const dir = mkdtempSync(join(tmpdir(), "check-dco-"));
  try {
    git(dir, ["init", "-q", "-b", "main"]);
    git(dir, ["config", "user.name", "Dev One"]);
    git(dir, ["config", "user.email", "dev@example.com"]);
    git(dir, ["config", "commit.gpgsign", "false"]);
    git(dir, ["commit", "-q", "--allow-empty", "-m", "chore: base"]);
    callback(dir, git(dir, ["rev-parse", "HEAD"]));
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
};

const runScript = (cwd, base) =>
  spawnSync(process.execPath, [SCRIPT_PATH], {
    cwd,
    env: cleanEnv({ BASE_SHA: base }),
    encoding: "utf8",
  });

test("the script exits 1 and names a commit without a sign-off", () => {
  withRepo((dir, base) => {
    git(dir, ["commit", "-q", "--allow-empty", "-m", "fix: unsigned"]);
    const run = runScript(dir, base);
    assert.equal(run.status, 1);
    assert.match(run.stderr, new RegExp(git(dir, ["rev-parse", "HEAD"]), "u"));
    assert.match(run.stderr, /git commit -s --amend/u);
  });
});

test("the script exits 0 when every commit has a sign-off", () => {
  withRepo((dir, base) => {
    git(dir, ["commit", "-q", "--allow-empty", "-s", "-m", "fix: signed"]);
    const run = runScript(dir, base);
    assert.equal(run.status, 0, run.stderr);
  });
});

test("the script exits 2 without BASE_SHA", () => {
  const env = { ...process.env };
  delete env.BASE_SHA;
  const run = spawnSync(process.execPath, [SCRIPT_PATH], { env, encoding: "utf8" });
  assert.equal(run.status, 2);
});

const runOnMessage = (cwd, message) => {
  const file = join(cwd, "COMMIT_MSG");
  writeFileSync(file, message);
  return spawnSync(process.execPath, [SCRIPT_PATH, "--message-file", file], {
    cwd,
    env: cleanEnv(),
    encoding: "utf8",
  });
};

test("the commit-msg mode exits 1 for a message without the author sign-off", () => {
  withRepo((dir) => {
    const run = runOnMessage(dir, "fix: unsigned\n");
    assert.equal(run.status, 1);
    assert.match(run.stderr, /git commit -s/u);
  });
});

test("the commit-msg mode exits 0 for a message signed off by the author", () => {
  withRepo((dir) => {
    const run = runOnMessage(dir, "fix: signed\n\nSigned-off-by: Dev One <dev@example.com>\n");
    assert.equal(run.status, 0, run.stderr);
  });
});

test("the commit-msg mode skips a merge commit", () => {
  withRepo((dir, base) => {
    writeFileSync(join(dir, ".git", "MERGE_HEAD"), `${base}\n`);
    const run = runOnMessage(dir, "Merge branch 'main'\n");
    assert.equal(run.status, 0, run.stderr);
  });
});
