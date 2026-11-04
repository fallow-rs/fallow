import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import {
  chmodSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  realpathSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const SCRIPT = resolve(dirname(fileURLToPath(import.meta.url)), "hook-file-length.sh");
const ARGS_FILE = "fallow-args.txt";

// A repository with a private git config and a stub fallow binary at
// target/debug/fallow. The stub writes its arguments to a file, so a test can
// read the base that the script gives to `fallow audit`.
const createRepo = (t) => {
  const root = realpathSync(mkdtempSync(join(tmpdir(), "hook-file-length-")));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const globalConfig = join(root, "gitconfig");
  writeFileSync(
    globalConfig,
    "[user]\n\tname = Test\n\temail = test@example.invalid\n[commit]\n\tgpgsign = false\n[init]\n\tdefaultBranch = main\n",
  );
  const work = join(root, "work");
  mkdirSync(join(work, "target/debug"), { recursive: true });
  const stub = join(work, "target/debug/fallow");
  writeFileSync(stub, `#!/bin/sh\nprintf '%s\\n' "$@" > ${ARGS_FILE}\n`);
  chmodSync(stub, 0o755);
  const env = { ...process.env, GIT_CONFIG_GLOBAL: globalConfig, GIT_CONFIG_NOSYSTEM: "1" };
  for (const name of [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_COMMON_DIR",
    "CARGO_TARGET_DIR",
  ]) {
    delete env[name];
  }
  const git = (...args) =>
    execFileSync("git", args, { cwd: work, env, encoding: "utf8", stdio: "pipe" }).trim();
  const commit = (path, text, message) => {
    writeFileSync(join(work, path), text);
    git("add", path);
    git("commit", "--quiet", "-m", message);
    return git("rev-parse", "HEAD");
  };
  const run = (...args) => spawnSync("sh", [SCRIPT, ...args], { cwd: work, env, encoding: "utf8" });
  const auditArgs = () => readFileSync(join(work, ARGS_FILE), "utf8").trim().split("\n");
  git("init", "--quiet");
  return { git, commit, run, auditArgs };
};

const baseOf = (args) => args[args.indexOf("--base") + 1];

// The base branch can remove lines from a long file after the branch starts.
// A compare with the tip of the base branch then shows growth on the branch.
test("the script compares with the merge base, not with the tip of the base ref", (t) => {
  const repo = createRepo(t);
  const start = repo.commit("a.mjs", "one\n", "start");
  repo.git("switch", "--quiet", "-c", "feature");
  repo.commit("a.mjs", "one\ntwo\n", "feature");
  repo.git("switch", "--quiet", "main");
  const mainTip = repo.commit("b.mjs", "b\n", "main");
  repo.git("switch", "--quiet", "feature");

  const result = repo.run("missing/ref", "main");

  assert.equal(result.status, 0, result.stderr);
  const args = repo.auditArgs();
  assert.equal(args[0], "audit");
  assert.equal(baseOf(args), start);
  assert.notEqual(baseOf(args), mainTip);
});

test("the pre-commit base HEAD stays HEAD", (t) => {
  const repo = createRepo(t);
  const head = repo.commit("a.mjs", "one\n", "start");

  const result = repo.run("HEAD");

  assert.equal(result.status, 0, result.stderr);
  assert.equal(baseOf(repo.auditArgs()), head);
});

test("the script passes with a note when no ref exists", (t) => {
  const repo = createRepo(t);
  repo.commit("a.mjs", "one\n", "start");

  const result = repo.run("origin/main", "upstream/main");

  assert.equal(result.status, 0);
  assert.match(result.stderr, /no ref of 'origin\/main upstream\/main' exists/u);
});
