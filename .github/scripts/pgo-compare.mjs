// Compare a base fallow-multicall build with a PGO build on held-out fixtures.
//
// Usage:
//   node pgo-compare.mjs --base <bin> --pgo <bin> --fixtures <dir>
//     [--projects query,vite,astro] [--commands check,dupes,health]
//     [--rounds 7] [--perf] [--min-gain 0.05] [--summary <file>]
//
// Each round runs base and PGO once per case, and the order alternates per
// round, so slow drift on the runner hits both builds the same way. The script
// reports the median wall time per case and the geometric mean of the
// PGO/base ratios. With --perf, it also records one `perf stat` instruction
// count per build and case. With --min-gain, it exits 1 when the geomean wall
// time is not at least that fraction lower than base. It always exits 1 when
// the PGO binary is larger than the base binary.

import { appendFileSync, mkdtempSync, readFileSync, realpathSync, rmSync, statSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { parseArgs } from "node:util";

const DEFAULT_PROJECTS = "query,vite,astro";
const DEFAULT_COMMANDS = "check,dupes,health";
const DEFAULT_ROUNDS = "7";
// Fallow exits 0 without error findings and 1 with error findings.
const ACCEPTED_EXIT_CODES = new Set([0, 1]);

export const median = (values) => {
  const sorted = values.toSorted((a, b) => a - b);
  const middle = sorted.length >> 1;
  return sorted.length % 2 === 1 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2;
};

/** Geometric mean of ratios, returned as a relative change (-0.07 is 7% less). */
export const geomeanChange = (ratios) =>
  Math.exp(ratios.reduce((sum, ratio) => sum + Math.log(ratio), 0) / ratios.length) - 1;

/** Returns the list of gate failures. An empty list means the gate passes. */
export const gateFailures = ({ wallChange, minGain, baseBytes, pgoBytes }) => {
  const failures = [];
  if (minGain !== null && !(wallChange <= -minGain)) {
    failures.push(
      `PGO geomean wall time changed by ${formatPercent(wallChange)}; the gate needs ${formatPercent(-minGain)} or lower`,
    );
  }
  if (pgoBytes > baseBytes) {
    failures.push(
      `PGO binary is ${pgoBytes} bytes, larger than the base binary (${baseBytes} bytes)`,
    );
  }
  return failures;
};

export const formatPercent = (value) => `${value >= 0 ? "+" : ""}${(value * 100).toFixed(1)}%`;

/** Parses the instruction count from `perf stat -x,` output. */
export const parsePerfInstructions = (text) => {
  for (const line of text.split(/\r?\n/)) {
    const fields = line.split(",");
    if (fields.length > 2 && fields[2].startsWith("instructions")) {
      const value = Number(fields[0]);
      return Number.isFinite(value) && fields[0] !== "" ? value : null;
    }
  }
  return null;
};

const runOnce = (bin, command, cwd, env) => {
  const args = [command, "--format", "json", "--quiet", "--no-cache"];
  const start = process.hrtime.bigint();
  const result = spawnSync(bin, args, { cwd, env, stdio: ["ignore", "ignore", "pipe"] });
  const elapsed = Number(process.hrtime.bigint() - start) / 1e6;
  if (result.error || !ACCEPTED_EXIT_CODES.has(result.status)) {
    const reason = result.error?.message ?? `exit ${result.status}, signal ${result.signal}`;
    throw new Error(`${bin} ${command} in ${cwd} failed: ${reason}\n${result.stderr}`);
  }
  return elapsed;
};

const perfInstructions = (bin, command, cwd, env, workDir) => {
  const output = join(workDir, "perf.csv");
  const args = ["stat", "-x,", "-e", "instructions:u", "-o", output, "--"];
  args.push(bin, command, "--format", "json", "--quiet", "--no-cache");
  const result = spawnSync("perf", args, { cwd, env, stdio: "ignore" });
  if (result.error || !ACCEPTED_EXIT_CODES.has(result.status)) return null;
  return parsePerfInstructions(readFileSync(output, "utf8"));
};

const measureCase = ({ builds, project, command, cwd, env, rounds, perf, workDir }) => {
  const wall = { base: [], pgo: [] };
  for (let round = 0; round < rounds; round += 1) {
    const order = round % 2 === 0 ? ["base", "pgo"] : ["pgo", "base"];
    for (const name of order) wall[name].push(runOnce(builds[name], command, cwd, env));
  }
  const row = {
    case: `${project} ${command}`,
    baseMs: median(wall.base),
    pgoMs: median(wall.pgo),
    baseInstructions: null,
    pgoInstructions: null,
  };
  if (perf) {
    row.baseInstructions = perfInstructions(builds.base, command, cwd, env, workDir);
    row.pgoInstructions = perfInstructions(builds.pgo, command, cwd, env, workDir);
  }
  return row;
};

const instructionChange = (row) =>
  row.baseInstructions && row.pgoInstructions
    ? formatPercent(row.pgoInstructions / row.baseInstructions - 1)
    : "n/a";

export const renderSummary = ({ title, rows, wallChange, baseBytes, pgoBytes, failures }) => {
  const lines = [`## ${title}`, ""];
  lines.push("| Case | Base median | PGO median | Wall change | Instructions change |");
  lines.push("| --- | ---: | ---: | ---: | ---: |");
  for (const row of rows) {
    lines.push(
      `| ${row.case} | ${row.baseMs.toFixed(0)} ms | ${row.pgoMs.toFixed(0)} ms | ${formatPercent(row.pgoMs / row.baseMs - 1)} | ${instructionChange(row)} |`,
    );
  }
  lines.push("");
  lines.push(`Geomean wall time change: ${formatPercent(wallChange)}`);
  lines.push("");
  lines.push(
    `Binary size: base ${baseBytes} bytes, PGO ${pgoBytes} bytes (${formatPercent(pgoBytes / baseBytes - 1)})`,
  );
  lines.push("");
  lines.push(failures.length === 0 ? "Gate: pass" : `Gate: fail\n\n- ${failures.join("\n- ")}`);
  return `${lines.join("\n")}\n`;
};

export const main = (argv = process.argv.slice(2)) => {
  const { values } = parseArgs({
    args: argv,
    options: {
      base: { type: "string" },
      pgo: { type: "string" },
      fixtures: { type: "string" },
      projects: { type: "string", default: DEFAULT_PROJECTS },
      commands: { type: "string", default: DEFAULT_COMMANDS },
      rounds: { type: "string", default: DEFAULT_ROUNDS },
      perf: { type: "boolean", default: false },
      "min-gain": { type: "string" },
      summary: { type: "string" },
      title: { type: "string", default: "PGO comparison" },
    },
  });
  if (!values.base || !values.pgo || !values.fixtures) {
    console.error("usage: pgo-compare.mjs --base <bin> --pgo <bin> --fixtures <dir> [options]");
    return 2;
  }
  const rounds = Number(values.rounds);
  const minGain = values["min-gain"] === undefined ? null : Number(values["min-gain"]);
  if (!Number.isInteger(rounds) || rounds < 1 || (minGain !== null && !Number.isFinite(minGain))) {
    console.error("--rounds must be a positive integer and --min-gain a number");
    return 2;
  }

  const workDir = mkdtempSync(join(tmpdir(), "pgo-compare-"));
  // An absolute cache directory keeps every run out of the fixture tree.
  const env = { ...process.env, FALLOW_CACHE_DIR: join(workDir, "cache") };
  // Each run starts in a fixture directory, so relative binary paths must resolve first.
  const builds = { base: resolve(values.base), pgo: resolve(values.pgo) };
  const rows = [];
  try {
    for (const project of values.projects.split(",")) {
      for (const command of values.commands.split(",")) {
        const cwd = join(values.fixtures, project);
        rows.push(
          measureCase({ builds, project, command, cwd, env, rounds, perf: values.perf, workDir }),
        );
        console.error(`pgo-compare: ${project} ${command} done`);
      }
    }
  } finally {
    rmSync(workDir, { recursive: true, force: true });
  }

  const wallChange = geomeanChange(rows.map((row) => row.pgoMs / row.baseMs));
  const baseBytes = statSync(values.base).size;
  const pgoBytes = statSync(values.pgo).size;
  const failures = gateFailures({ wallChange, minGain, baseBytes, pgoBytes });
  const summary = renderSummary({
    title: values.title,
    rows,
    wallChange,
    baseBytes,
    pgoBytes,
    failures,
  });
  process.stdout.write(summary);
  if (values.summary) appendFileSync(values.summary, summary);
  return failures.length === 0 ? 0 : 1;
};

// import.meta.url holds the real path, encoded as a URL. Compare it with the
// same form of argv[1], so a symlink, a space or a Windows path still runs main.
const isEntryPoint = () =>
  process.argv[1] !== undefined &&
  import.meta.url === pathToFileURL(realpathSync(process.argv[1])).href;

if (isEntryPoint()) {
  process.exitCode = main();
}
