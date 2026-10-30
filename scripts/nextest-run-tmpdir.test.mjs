import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  symlinkSync,
  utimesSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { test } from "node:test";

const script = resolve(import.meta.dirname, "nextest-run-tmpdir.sh");
const skip = process.platform === "win32";
const THREE_HOURS_S = 3 * 60 * 60;

const runScript = (base, envFile) => {
  execFileSync("sh", [script], { env: { ...process.env, TMPDIR: base, NEXTEST_ENV: envFile } });
  const line = readFileSync(envFile, "utf8").trim().split("\n").at(-1);
  return line.replace(/^TMPDIR=/, "");
};

const withFixture = (body) => {
  const root = mkdtempSync(join(tmpdir(), "nextest-run-tmpdir-test-"));
  try {
    const real = join(root, "real");
    mkdirSync(real);
    const old = join(real, "fallow-nextest.OLD");
    const fresh = join(real, "fallow-nextest.FRESH");
    mkdirSync(old);
    mkdirSync(fresh);
    const past = Date.now() / 1000 - THREE_HOURS_S;
    utimesSync(old, past, past);
    symlinkSync("real", join(root, "link"));
    body({ root, real, old, fresh });
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
};

test("removes idle run directories and keeps recent ones", { skip }, () => {
  withFixture(({ root, real, old, fresh }) => {
    const dir = runScript(real, join(root, "env"));
    assert.equal(existsSync(old), false);
    assert.equal(existsSync(fresh), true);
    assert.equal(existsSync(dir), true);
    assert.match(dir, /\/real\/fallow-nextest\.[A-Za-z0-9]{6}$/);
  });
});

test("removes idle run directories when TMPDIR is a symlink", { skip }, () => {
  for (const suffix of ["", "/"]) {
    withFixture(({ root, old, fresh }) => {
      runScript(join(root, "link") + suffix, join(root, "env"));
      assert.equal(existsSync(old), false, `TMPDIR=link${suffix}`);
      assert.equal(existsSync(fresh), true, `TMPDIR=link${suffix}`);
      assert.equal(existsSync(join(root, "link")), true);
    });
  }
});
