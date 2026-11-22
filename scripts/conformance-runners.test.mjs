import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { chmodSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { makeTempDir } from "./lib/temp-dir.mjs";

const CONFORMANCE_DIR = resolve(
  dirname(fileURLToPath(import.meta.url)),
  "..",
  "tests",
  "conformance",
);
const REAL_GIT = execFileSync("sh", ["-c", "command -v git"], { encoding: "utf8" }).trim();

const writeExecutable = (path, body) => {
  writeFileSync(path, `#!/bin/sh\n${body}\n`);
  chmodSync(path, 0o755);
};

// Stub tools on PATH. `fallow` logs the directory it analyzes and whether
// node_modules exists there. The package managers create node_modules. With
// `installFails`, they leave a partial node_modules and exit 1. `git clone`
// always fails, so a test can only reuse a checkout that it prepared.
const makeStubs = (dir, { installFails = false } = {}) => {
  const bin = join(dir, "bin");
  const log = join(dir, "calls.log");
  mkdirSync(bin, { recursive: true });
  writeExecutable(
    join(bin, "fallow"),
    `if [ "$1" = "--version" ]; then echo "fallow 0.0.0"; exit 0; fi
root="$PWD"
while [ $# -gt 0 ]; do [ "$1" = "--root" ] && root="$2"; shift; done
if [ -d "$root/node_modules" ]; then deps=yes; else deps=no; fi
echo "fallow $(basename "$root") node_modules=$deps" >> "${log}"
echo '{}'`,
  );
  const install = `echo "install $(basename "$PWD")" >> "${log}"; mkdir -p node_modules; exit ${
    installFails ? 1 : 0
  }`;
  writeExecutable(join(bin, "npm"), install);
  writeExecutable(join(bin, "pnpm"), install);
  writeExecutable(join(bin, "npx"), "echo '{}'");
  writeExecutable(
    join(bin, "git"),
    `if [ "$1" = "clone" ]; then exit 128; fi
exec "${REAL_GIT}" "$@"`,
  );
  return { env: { ...process.env, PATH: `${bin}:${process.env.PATH}` }, log };
};

const readLog = (log) => (existsSync(log) ? readFileSync(log, "utf8") : "");

const makeProject = (dir) => {
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, "package.json"), '{"name":"fixture"}\n');
  return dir;
};

// A checkout of preact, the first project of run-all.sh, with `tag` on HEAD.
const makeCachedCheckout = (cloneDir, tag) => {
  const dest = makeProject(join(cloneDir, "preact"));
  const git = (...args) =>
    execFileSync(REAL_GIT, ["-C", dest, ...args], {
      env: { ...process.env, GIT_CONFIG_GLOBAL: "/dev/null", GIT_CONFIG_SYSTEM: "/dev/null" },
      stdio: "ignore",
    });
  git("init", "-q");
  git("add", ".");
  git("-c", "user.name=t", "-c", "user.email=t@example.com", "commit", "-q", "-m", "init");
  git("tag", tag);
  return dest;
};

const runAll = (dir, stubs) =>
  spawnSync(
    "bash",
    [
      join(CONFORMANCE_DIR, "run-all.sh"),
      "--fallow-bin",
      join(dir, "bin", "fallow"),
      "--clone-dir",
      join(dir, "clones"),
    ],
    { encoding: "utf8", env: stubs.env, timeout: 60_000 },
  );

test("run.sh installs dependencies before fallow analyzes the project", () => {
  const dir = makeTempDir("fallow-conformance-run-");
  const stubs = makeStubs(dir);
  const project = makeProject(join(dir, "project"));

  const result = spawnSync("bash", [join(CONFORMANCE_DIR, "run.sh"), project], {
    encoding: "utf8",
    env: stubs.env,
    timeout: 60_000,
  });

  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(readLog(stubs.log).trim().split("\n"), [
    "install project",
    "fallow project node_modules=yes",
  ]);
});

test("run.sh runs both tools without a partial tree when the install fails", () => {
  const dir = makeTempDir("fallow-conformance-run-");
  const stubs = makeStubs(dir, { installFails: true });
  const project = makeProject(join(dir, "project"));

  const result = spawnSync("bash", [join(CONFORMANCE_DIR, "run.sh"), project], {
    encoding: "utf8",
    env: stubs.env,
    timeout: 60_000,
  });

  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stderr, /dependency install failed/);
  assert.deepEqual(readLog(stubs.log).trim().split("\n"), [
    "install project",
    "fallow project node_modules=no",
  ]);
});

test("run-all.sh skips a project whose dependency install fails", () => {
  const dir = makeTempDir("fallow-conformance-run-all-");
  const stubs = makeStubs(dir, { installFails: true });
  makeCachedCheckout(join(dir, "clones"), "10.25.4");

  const result = runAll(dir, stubs);

  assert.equal(result.status, 1);
  assert.match(result.stderr, /SKIP: dependency install failed/);
  assert.deepEqual(readLog(stubs.log).trim().split("\n"), ["install preact"]);
});

test("run-all.sh refuses a cached checkout at another revision", () => {
  const dir = makeTempDir("fallow-conformance-run-all-");
  const stubs = makeStubs(dir);
  makeCachedCheckout(join(dir, "clones"), "10.0.0");

  const result = runAll(dir, stubs);

  assert.equal(result.status, 1);
  assert.match(result.stderr, /is not at 10\.25\.4/);
  assert.equal(readLog(stubs.log), "");
});

test("run-all.sh reuses a cached checkout at the pinned tag", () => {
  const dir = makeTempDir("fallow-conformance-run-all-");
  const stubs = makeStubs(dir);
  makeCachedCheckout(join(dir, "clones"), "10.25.4");

  const result = runAll(dir, stubs);

  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(readLog(stubs.log).trim().split("\n"), [
    "install preact",
    "fallow preact node_modules=yes",
  ]);
});

test("run-all.sh skips the project again after an install left a partial tree", () => {
  const dir = makeTempDir("fallow-conformance-run-all-");
  const failing = makeStubs(dir, { installFails: true });
  makeCachedCheckout(join(dir, "clones"), "10.25.4");

  runAll(dir, failing);
  const second = runAll(dir, failing);

  assert.equal(second.status, 1);
  assert.match(second.stderr, /SKIP: dependency install failed/);
  assert.doesNotMatch(readLog(failing.log), /^fallow /m);
});
