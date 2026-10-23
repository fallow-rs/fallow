import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  chmodSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { commandArgs, normalize, parseArgs, parseUserCpu, verdict } from "./compare-binaries.mjs";

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const SCRIPT_PATH = resolve(SCRIPT_DIR, "compare-binaries.mjs");

const runScript = (args) =>
  spawnSync(process.execPath, [SCRIPT_PATH, ...args], {
    encoding: "utf8",
    timeout: 30_000,
  });

// A fake fallow binary that prints JSON with a volatile timing field. A
// `security` run prints `findings` when `extra` is set, and the exit code is
// `exitCode` for that run.
const fakeBin = (dir, name, { extra = false, exitCode = 0 } = {}) => {
  const path = join(dir, name);
  writeFileSync(
    path,
    `#!${process.execPath}
const args = process.argv.slice(2);
const security = args[0] === "security";
const out = { b: 1, a: { elapsed_ms: Math.random(), run_id: String(Math.random()), keep: args[0] } };
if (security && ${extra}) out.findings = ["x"];
if (!process.env.SKIP_FALLOW_AGENT_GUARD) out.guard = "missing";
process.stdout.write(JSON.stringify(out));
process.exitCode = security ? ${exitCode} : 0;
`,
  );
  chmodSync(path, 0o755);
  return path;
};

const fixture = () => {
  const dir = mkdtempSync(join(tmpdir(), "compare-binaries-"));
  const project = join(dir, "proj");
  mkdirSync(project);
  return { dir, project, outDir: join(dir, "out") };
};

test("normalize removes volatile keys at every depth and sorts keys", () => {
  const input = {
    z: 1,
    elapsed_ms: 5,
    nested: [{ timing: {}, duration_s: 2, b: 2, a: 1, generated_at: "now" }],
    meta: { run_id: "r", analysis_ms: 3, keep: true },
  };
  assert.equal(
    JSON.stringify(normalize(input)),
    JSON.stringify({ meta: { keep: true }, nested: [{ a: 1, b: 2 }], z: 1 }),
  );
});

test("commandArgs maps bare to no arguments and splits the rest", () => {
  assert.deepEqual(commandArgs("bare"), []);
  assert.deepEqual(commandArgs("security"), ["security"]);
  assert.deepEqual(commandArgs(" dupes  --mode semantic "), ["dupes", "--mode", "semantic"]);
});

test("parseUserCpu reads the last user line of time -p output", () => {
  assert.equal(parseUserCpu("noise\nreal 1.50\nuser 0.42\nsys 0.10\n"), 0.42);
  assert.equal(parseUserCpu("no timing here"), null);
});

test("verdict reports ERROR when a run has no JSON", () => {
  const ok = { normalized: "{}\n", error: null };
  assert.equal(verdict(ok, { ...ok }), "IDENTICAL");
  assert.equal(verdict(ok, { normalized: "[]\n", error: null }), "DIFF");
  assert.equal(verdict(ok, { normalized: null, error: "output is not JSON" }), "ERROR");
});

test("parseArgs applies the default command set, which includes security", () => {
  const opts = parseArgs(["--base", "a", "--head", "b", "--project", "p"]);
  assert.deepEqual(opts.commands, ["bare", "security", "health", "dupes"]);
  assert.deepEqual(opts.projects, [resolve("p")]);
});

test("compare-binaries prints help", () => {
  const result = runScript(["--help"]);
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /Usage: node scripts\/compare-binaries\.mjs/);
  assert.match(result.stdout, /--command CMD/);
});

test("compare-binaries rejects missing binaries and unknown options", () => {
  assert.match(runScript(["--base", "a"]).stderr, /--base and --head are required/);
  assert.match(runScript(["--bogus"]).stderr, /unknown option: --bogus/);
});

test("compare-binaries reports IDENTICAL when only volatile keys differ", () => {
  const { dir, project, outDir } = fixture();
  const base = fakeBin(dir, "base");
  const head = fakeBin(dir, "head");
  const result = runScript([
    "--base",
    base,
    "--head",
    head,
    "--project",
    project,
    "--out-dir",
    outDir,
  ]);

  assert.equal(result.status, 0, result.stdout + result.stderr);
  const lines = result.stdout.split("\n").filter((line) => line.startsWith("IDENTICAL"));
  assert.equal(lines.length, 4);
  assert.match(lines[0], /exit 0\/0/);
  assert.match(lines[0], /user /);
  const normalized = JSON.parse(readFileSync(join(outDir, "proj", "bare.base.json"), "utf8"));
  assert.deepEqual(normalized, { a: { keep: "--root" }, b: 1 });
});

test("compare-binaries fails and writes a diff when only an opt-in command differs", () => {
  const { dir, project, outDir } = fixture();
  const base = fakeBin(dir, "base");
  const head = fakeBin(dir, "head", { extra: true });
  const result = runScript([
    "--base",
    base,
    "--head",
    head,
    "--project",
    project,
    "--out-dir",
    outDir,
  ]);

  assert.equal(result.status, 1);
  assert.match(result.stdout, /^IDENTICAL\s+proj\s+bare/m);
  assert.match(result.stdout, /^DIFF\s+proj\s+security/m);
  const diff = readFileSync(join(outDir, "proj", "security.diff"), "utf8");
  assert.match(diff, /^\+\s+"findings"/m);
});

test("compare-binaries fails on an exit-code mismatch with identical output", () => {
  const { dir, project, outDir } = fixture();
  const base = fakeBin(dir, "base");
  const head = fakeBin(dir, "head", { exitCode: 1 });
  const result = runScript([
    "--base",
    base,
    "--head",
    head,
    "--project",
    project,
    "--command",
    "security",
    "--out-dir",
    outDir,
  ]);

  assert.equal(result.status, 1);
  assert.match(result.stdout, /^IDENTICAL\s+proj\s+security\s+exit 0\/1 EXIT-MISMATCH/m);
  assert.equal(existsSync(join(outDir, "proj", "security.diff")), false);
});
