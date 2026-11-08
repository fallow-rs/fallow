#!/usr/bin/env node
// Measures `fallow mutation` on public Vitest projects at pinned commits and
// checks the speed targets of issue #4341.
//
// Usage:
//   node scripts/mutation-bench.mjs --bin <fallow> [--base <fallow>] [--runs 3]
//     [--projects defu,cookie-es,ufo] [--workdir <dir>] [--check] [--json]
//
// Each run is `fallow mutation --all --concurrency 2 --no-cache --format json`
// in a clean clone. With `--base`, the base binary and `--bin` run as
// interleaved pairs, so that other work on the host changes both sides. The
// CPU time is user plus system time of the run and its child processes, from
// `/usr/bin/time -p`. Each run must give the verdicts of the first run of the
// first binary; a difference makes the exit code 1. `--check` also makes a
// missed target an exit code 1.

import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, rmSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

/** The projects of the benchmark, at pinned commits, with their targets. */
export const PROJECTS = [
  {
    name: "defu",
    repo: "https://github.com/unjs/defu",
    commit: "82632b6",
    targets: { mutantsPerCpuHour: 38_000 },
  },
  {
    name: "cookie-es",
    repo: "https://github.com/unjs/cookie-es",
    commit: "f89ede8",
    targets: { wallMs: 30_000, mutantsPerCpuHour: 40_000 },
  },
  {
    name: "ufo",
    repo: "https://github.com/unjs/ufo",
    commit: "f06c800",
    targets: { wallMs: 45_000, mutantsPerCpuHour: 32_000 },
  },
];

/** The statuses of a mutant that ran its tests. */
const EXECUTED = new Set(["killed", "timeout", "survived", "flaky", "runtime_error"]);

const MS_PER_HOUR = 3_600_000;

/** Reads `real`, `user` and `sys` seconds from the output of `time -p`. */
export const parseTime = (text) => {
  const value = (name) => {
    const match = new RegExp(`^${name}\\s+([\\d.]+)`, "mu").exec(text);
    return match === null ? null : Number(match[1]) * 1000;
  };
  const wallMs = value("real");
  const user = value("user");
  const sys = value("sys");
  if (wallMs === null || user === null || sys === null) return null;
  return { wallMs, cpuMs: user + sys };
};

/** Gives the measures of one run from its report and its times. */
export const summarize = (report, time) => {
  const executed = report.mutants.filter((mutant) => EXECUTED.has(mutant.status)).length;
  return {
    wallMs: time.wallMs,
    cpuMs: time.cpuMs,
    executed,
    mutantsPerCpuHour: time.cpuMs > 0 ? Math.round((executed * MS_PER_HOUR) / time.cpuMs) : 0,
    score: report.summary?.score ?? null,
    verdicts: new Map(report.mutants.map((mutant) => [mutant.mutant_id, mutant.status])),
  };
};

/** Lists the mutants whose status differs between two runs, sorted by id. */
export const verdictDifferences = (reference, candidate) => {
  const ids = new Set([...reference.keys(), ...candidate.keys()]);
  return [...ids]
    .filter((id) => reference.get(id) !== candidate.get(id))
    .toSorted()
    .map((id) => ({ id, from: reference.get(id) ?? null, to: candidate.get(id) ?? null }));
};

/** Lists the targets that a run misses. */
export const missedTargets = (targets, run) => {
  const missed = [];
  if (targets.wallMs !== undefined && run.wallMs > targets.wallMs) {
    missed.push(`wall ${Math.round(run.wallMs)} ms > ${targets.wallMs} ms`);
  }
  if (
    targets.mutantsPerCpuHour !== undefined &&
    run.mutantsPerCpuHour < targets.mutantsPerCpuHour
  ) {
    missed.push(`${run.mutantsPerCpuHour} mutants per CPU hour < ${targets.mutantsPerCpuHour}`);
  }
  return missed;
};

// The flags that take a value, and how each one sets the options.
const VALUE_FLAGS = {
  "--bin": (options, value) => (options.bin = resolve(value)),
  "--base": (options, value) => (options.base = resolve(value)),
  "--runs": (options, value) => (options.runs = Number(value)),
  "--projects": (options, value) => (options.projects = value.split(",")),
  "--workdir": (options, value) => (options.workdir = resolve(value)),
};

const SWITCHES = {
  "--check": (options) => (options.check = true),
  "--json": (options) => (options.json = true),
};

export const parseArgs = (argv) => {
  const options = { runs: 3, check: false, json: false, projects: null, base: null, bin: null };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (Object.hasOwn(VALUE_FLAGS, arg)) VALUE_FLAGS[arg](options, argv[(index += 1)]);
    else if (Object.hasOwn(SWITCHES, arg)) SWITCHES[arg](options);
    else throw new Error(`unknown argument: ${arg}`);
  }
  if (options.bin === null) throw new Error("--bin <fallow binary> is required");
  if (!Number.isInteger(options.runs) || options.runs < 1) throw new Error("--runs must be >= 1");
  return options;
};

const run = (command, args, cwd) =>
  execFileSync(command, args, { cwd, stdio: ["ignore", "ignore", "inherit"] });

/** Clones a project at its commit and installs its dependencies once. */
const prepare = (project, workdir) => {
  const dir = join(workdir, project.name);
  if (!existsSync(join(dir, "node_modules"))) {
    rmSync(dir, { recursive: true, force: true });
    mkdirSync(workdir, { recursive: true });
    run("git", ["clone", "--quiet", project.repo, dir], workdir);
    run("git", ["checkout", "--quiet", project.commit], dir);
    run("pnpm", ["install", "--frozen-lockfile", "--silent"], dir);
  }
  return dir;
};

const measure = (bin, dir) => {
  rmSync(join(dir, ".fallow", "mutation"), { recursive: true, force: true });
  const result = spawnSync(
    "/usr/bin/time",
    [
      "-p",
      bin,
      "mutation",
      "--all",
      "--concurrency",
      "2",
      "--no-cache",
      "--format",
      "json",
      "--quiet",
    ],
    { cwd: dir, encoding: "utf8", maxBuffer: 256 * 1024 * 1024 },
  );
  const time = parseTime(result.stderr);
  if (time === null)
    throw new Error(`no timing from /usr/bin/time:\n${result.stderr.slice(-2000)}`);
  const report = JSON.parse(result.stdout);
  if (!Array.isArray(report.mutants)) {
    throw new Error(`fallow mutation failed in ${dir}: ${result.stdout.slice(0, 2000)}`);
  }
  return summarize(report, time);
};

const format = (label, project, measured) =>
  [
    label.padEnd(5),
    project.padEnd(10),
    `wall ${(measured.wallMs / 1000).toFixed(1)} s`,
    `cpu ${(measured.cpuMs / 1000).toFixed(1)} s`,
    `${measured.mutantsPerCpuHour} mutants/cpu-h`,
    `executed ${measured.executed}`,
  ].join("  ");

const printRun = (result) => {
  console.log(format(result.side, result.project, result));
  for (const difference of result.differences) {
    console.log(`  verdict ${difference.id}: ${difference.from} -> ${difference.to}`);
  }
  for (const miss of result.missed) console.log(`  target missed: ${miss}`);
};

/** Runs one project with each binary, `runs` times, as interleaved pairs. */
const benchProject = (project, dir, sides, options) => {
  const results = [];
  let reference = null;
  for (let index = 0; index < options.runs; index += 1) {
    for (const [side, bin] of sides) {
      const measured = measure(bin, dir);
      reference ??= measured.verdicts;
      const result = {
        project: project.name,
        side,
        run: index + 1,
        ...measured,
        differences: verdictDifferences(reference, measured.verdicts),
        missed: side === "bin" ? missedTargets(project.targets, measured) : [],
      };
      if (!options.json) printRun(result);
      results.push(result);
    }
  }
  return results;
};

/** True when a run changed a verdict, or missed a target under `--check`. */
export const failedRun = (result, check) =>
  result.differences.length > 0 || (check && result.missed.length > 0);

/** The binaries of a run: the base first, so each pair starts with it. */
const sidesOf = (options) =>
  options.base
    ? [
        ["base", options.base],
        ["bin", options.bin],
      ]
    : [["bin", options.bin]];

const main = () => {
  const options = parseArgs(process.argv.slice(2));
  const workdir = options.workdir ?? resolve("target", "mutation-bench");
  const selected = options.projects ?? PROJECTS.map((project) => project.name);
  const projects = PROJECTS.filter((project) => selected.includes(project.name));
  const results = projects.flatMap((project) =>
    benchProject(project, prepare(project, workdir), sidesOf(options), options),
  );
  if (options.json) {
    console.log(
      JSON.stringify(
        results.map(({ verdicts: _verdicts, ...rest }) => rest),
        null,
        2,
      ),
    );
  }
  return results.some((result) => failedRun(result, options.check)) ? 1 : 0;
};

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    process.exitCode = main();
  } catch (error) {
    console.error(`mutation-bench: ${error.message}`);
    process.exitCode = 2;
  }
}
