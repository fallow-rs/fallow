#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { parseOptions } from "./lib/options.mjs";

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(SCRIPT_DIR, "..");
const DEFAULT_OUT_DIR = join(REPO_ROOT, "target", "compare-binaries");
const FIXTURES_RELATIVE = join("benchmarks", "fixtures", "real-world");
const DEFAULT_PROJECTS = ["fastify", "vue-core", "svelte", "next.js"];
// Bare `fallow` does not run the opt-in `security` analysis, so it is listed
// on its own.
const DEFAULT_COMMANDS = ["bare", "security", "health", "dupes"];
const BARE_COMMAND = "bare";
const FIXED_ARGS = ["--format", "json", "--quiet", "--no-cache"];
const VOLATILE_KEY_PARTS = ["elapsed", "_ms", "timing", "duration", "run_id", "generated_at"];
const DEFAULT_TIMEOUT_MS = 600_000;
const MAX_BUFFER = 512 * 1024 * 1024;
const TIME_BIN = "/usr/bin/time";

const usage = () => `Usage: node scripts/compare-binaries.mjs --base BIN --head BIN [options]

Runs two fallow binaries on the same projects and compares the JSON output.
Volatile keys (${VOLATILE_KEY_PARTS.join(", ")}) are removed before the compare.

Options:
  --base BIN        Baseline fallow binary. Required.
  --head BIN        Candidate fallow binary. Required.
  --project DIR     Project root. Repeatable. Default: ${DEFAULT_PROJECTS.join(", ")}
                    under ${FIXTURES_RELATIVE}
  --command CMD     fallow subcommand with optional arguments, for example
                    "dupes --mode semantic". Use "${BARE_COMMAND}" for bare fallow.
                    Repeatable. Default: ${DEFAULT_COMMANDS.join(", ")}
  --out-dir DIR     Normalized output and diffs. Default: target/compare-binaries
  --timeout-ms N    Timeout for one run. Default: ${DEFAULT_TIMEOUT_MS}

Each run uses: --root <project> ${FIXED_ARGS.join(" ")}
and SKIP_FALLOW_AGENT_GUARD=1. The exit code is 1 on any DIFF, exit-code
mismatch or ERROR (output that is not JSON).
`;

const OPTION_SPEC = {
  "--base": { key: "base", type: "string" },
  "--head": { key: "head", type: "string" },
  "--project": { key: "projects", type: "string", repeat: true },
  "--command": { key: "commands", type: "string", repeat: true },
  "--out-dir": { key: "outDir", type: "string" },
  "--timeout-ms": { key: "timeoutMs", type: "positiveInt" },
  "--help": { key: "help", type: "flag" },
  "-h": { key: "help", type: "flag" },
};

const applyDefaults = (opts) => {
  if (!opts.base || !opts.head) {
    throw new Error("--base and --head are required");
  }
  opts.base = resolve(opts.base);
  opts.head = resolve(opts.head);
  opts.outDir = resolve(opts.outDir);
  opts.projects =
    opts.projects.length > 0
      ? opts.projects.map((project) => resolve(project))
      : DEFAULT_PROJECTS.map((project) => join(defaultFixturesDir(), project));
  if (opts.commands.length === 0) {
    opts.commands = [...DEFAULT_COMMANDS];
  }
  return opts;
};

export const parseArgs = (argv) => {
  const defaults = {
    base: "",
    head: "",
    projects: [],
    commands: [],
    outDir: DEFAULT_OUT_DIR,
    timeoutMs: DEFAULT_TIMEOUT_MS,
    help: false,
  };
  const opts = parseOptions(argv, OPTION_SPEC, defaults);
  return opts.help ? opts : applyDefaults(opts);
};

// A worktree has no downloaded fixtures, so fall back to the main checkout.
const defaultFixturesDir = () => {
  const local = join(REPO_ROOT, FIXTURES_RELATIVE);
  if (existsSync(local)) {
    return local;
  }
  const proc = spawnSync("git", ["rev-parse", "--path-format=absolute", "--git-common-dir"], {
    cwd: REPO_ROOT,
    encoding: "utf8",
  });
  if (proc.status === 0) {
    const shared = join(dirname(proc.stdout.trim()), FIXTURES_RELATIVE);
    if (existsSync(shared)) {
      return shared;
    }
  }
  return local;
};

/** Split a `--command` value into fallow arguments. `bare` gives no arguments. */
export const commandArgs = (command) => {
  const parts = command.trim().split(/\s+/).filter(Boolean);
  return parts.length === 1 && parts[0] === BARE_COMMAND ? [] : parts;
};

const isVolatileKey = (key) => VOLATILE_KEY_PARTS.some((part) => key.includes(part));

/** Remove volatile keys at every depth and sort object keys. */
export const normalize = (value) => {
  if (Array.isArray(value)) {
    return value.map(normalize);
  }
  if (value === null || typeof value !== "object") {
    return value;
  }
  const out = {};
  for (const key of Object.keys(value).toSorted()) {
    if (!isVolatileKey(key)) {
      out[key] = normalize(value[key]);
    }
  }
  return out;
};

/** Read the user CPU seconds from the stderr of `/usr/bin/time -p`. */
export const parseUserCpu = (stderr) => {
  const matches = [...stderr.matchAll(/^user\s+([\d.]+)\s*$/gm)];
  return matches.length > 0 ? Number(matches.at(-1)[1]) : null;
};

const runOnce = (bin, project, args, timeoutMs) => {
  const fallowArgs = [...args, "--root", project, ...FIXED_ARGS];
  const timed = existsSync(TIME_BIN);
  const started = process.hrtime.bigint();
  const proc = spawnSync(timed ? TIME_BIN : bin, timed ? ["-p", bin, ...fallowArgs] : fallowArgs, {
    encoding: "utf8",
    timeout: timeoutMs,
    maxBuffer: MAX_BUFFER,
    env: { ...process.env, SKIP_FALLOW_AGENT_GUARD: "1" },
  });
  const wallMs = Number(process.hrtime.bigint() - started) / 1e6;
  const stderr = proc.stderr ?? "";
  let normalized = null;
  let error = proc.error ? proc.error.message : null;
  if (!error) {
    try {
      normalized = `${JSON.stringify(normalize(JSON.parse(proc.stdout)), null, 2)}\n`;
    } catch (parseError) {
      error = `output is not JSON: ${parseError.message}`;
    }
  }
  return {
    status: proc.status,
    userCpu: timed ? parseUserCpu(stderr) : null,
    wallMs,
    normalized,
    raw: proc.stdout ?? "",
    stderr,
    error,
  };
};

/** Classify one base/head pair. */
export const verdict = (base, head) => {
  if (base.error || head.error) {
    return "ERROR";
  }
  return base.normalized === head.normalized ? "IDENTICAL" : "DIFF";
};

const formatCpu = (seconds) => (seconds === null ? "n/a" : `${seconds.toFixed(2)}s`);

const formatLine = (project, command, result) => {
  const exits = `exit ${result.base.status}/${result.head.status}`;
  const cpu = `user ${formatCpu(result.base.userCpu)} -> ${formatCpu(result.head.userCpu)}`;
  const mismatch = result.exitMismatch ? " EXIT-MISMATCH" : "";
  return `${result.verdict.padEnd(9)} ${project.padEnd(12)} ${command.padEnd(10)} ${exits}${mismatch}  ${cpu}`;
};

const slug = (command) => command.trim().replace(/[^A-Za-z0-9._-]+/g, "_");

const writeArtifacts = (dir, name, result) => {
  mkdirSync(dir, { recursive: true });
  const files = {};
  for (const side of ["base", "head"]) {
    const run = result[side];
    files[side] = join(dir, `${name}.${side}.json`);
    writeFileSync(files[side], run.normalized ?? run.raw);
    if (run.error) {
      writeFileSync(join(dir, `${name}.${side}.stderr.txt`), run.stderr);
    }
  }
  if (result.verdict !== "DIFF") {
    return null;
  }
  const diff = spawnSync("diff", ["-u", files.base, files.head], {
    encoding: "utf8",
    maxBuffer: MAX_BUFFER,
  });
  const diffPath = join(dir, `${name}.diff`);
  writeFileSync(diffPath, diff.stdout ?? "");
  return diffPath;
};

const comparePair = (opts, project, command) => {
  const args = commandArgs(command);
  const base = runOnce(opts.base, project, args, opts.timeoutMs);
  const head = runOnce(opts.head, project, args, opts.timeoutMs);
  const result = {
    base,
    head,
    verdict: verdict(base, head),
    exitMismatch: base.status !== head.status,
  };
  const diffPath = writeArtifacts(join(opts.outDir, basename(project)), slug(command), result);
  return { ...result, diffPath };
};

const assertInputsExist = (opts) => {
  const missing = [opts.base, opts.head].filter((bin) => !existsSync(bin));
  if (missing.length > 0) {
    throw new Error(`binary not found: ${missing.join(", ")}`);
  }
  const missingProjects = opts.projects.filter((project) => !existsSync(project));
  if (missingProjects.length > 0) {
    throw new Error(
      `project not found: ${missingProjects.join(", ")}\nRun node benchmarks/download-fixtures.mjs or pass --project.`,
    );
  }
};

/** Compare every project and command pair. Returns true when all pairs match. */
const compareAll = (opts) => {
  let ok = true;
  for (const project of opts.projects) {
    for (const command of opts.commands) {
      const result = comparePair(opts, project, command);
      console.log(formatLine(basename(project), command, result));
      if (result.diffPath) {
        console.log(`  diff: ${result.diffPath}`);
      }
      ok &&= result.verdict === "IDENTICAL" && !result.exitMismatch;
    }
  }
  console.log(`output: ${opts.outDir}`);
  return ok;
};

const main = () => {
  const opts = parseArgs(process.argv.slice(2));
  if (opts.help) {
    console.log(usage());
    return;
  }
  assertInputsExist(opts);
  if (!compareAll(opts)) {
    process.exitCode = 1;
  }
};

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    main();
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  }
}
