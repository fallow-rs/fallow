#!/usr/bin/env node

// Show that an oracle catches a known bug. An oracle is a test that compares
// two results that must agree, for example a warm cache run and a cache-free
// run. It must fail on the parent of a named fix commit and pass on the fix
// commit, or it does not catch that bug class. The script runs each commit
// in a temporary worktree, so the checkout stays as it is.

import { spawnSync } from "node:child_process";
import { closeSync, mkdirSync, mkdtempSync, openSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { cargoTargetDir } from "./cargo-target.mjs";
import { cacheModel } from "./oracle-proof/cache-model.mjs";
import { mutationCache } from "./oracle-proof/mutation-cache.mjs";
import { permutation } from "./oracle-proof/permutation.mjs";
import { unicodeSpans } from "./oracle-proof/unicode-spans.mjs";

/** The registered oracles, by name. */
export const ORACLES = Object.freeze({
  [cacheModel.name]: cacheModel,
  [mutationCache.name]: mutationCache,
  [permutation.name]: permutation,
  [unicodeSpans.name]: unicodeSpans,
});

const EXIT_HOLDS = 0;
const EXIT_DOES_NOT_HOLD = 1;
const EXIT_TOOL_ERROR = 2;

/** The lines of a build or test log that name an error or a failed test. */
const FAILURE_LINE =
  /^(error(\[\w+\])?: |test .* FAILED$|thread '.*' panicked at |failures:$|test result: )/;
const FAILURE_LINE_LIMIT = 40;
/** A cargo test summary with at least one passed test. */
const TESTS_RAN = /^test result: ok\. [1-9]\d* passed/m;

const USAGE = `Usage: node scripts/oracle-proof.mjs <oracle> [<fix commit>...] [options]
       node scripts/oracle-proof.mjs --list

Run an oracle on the parent of each fix commit and on the fix commit, each
in a temporary worktree. The proof holds when the oracle fails on the parent
and passes on the fix. Without a fix commit, the script uses the fix
commits that the oracle names, each with its own scope. A fix commit on the
command line uses the full oracle.

Options:
  --source <dir>   The checkout that holds the current oracle (default: this repository).
  --logs <dir>     Write the build and test logs here (default: a new temporary directory).
  --keep           Keep the worktrees after the run.
  --list           List the oracles and their fix commits.

Exit codes: 0 every proof holds, 1 a proof does not hold, 2 tool error
(bad input, a git error, a build failure, a run that a signal stopped, or a
run in which no test ran).

An interrupt (Ctrl-C) during a build can leave a worktree in the temporary
directory. Remove it with \`git worktree remove --force <path>\`, or with
\`git worktree prune\` after you delete the directory.`;

const REPO_ROOT = resolve(fileURLToPath(new URL("..", import.meta.url)));

const git = (cwd, args) => {
  const result = spawnSync("git", args, { cwd, encoding: "utf8" });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(
      `git ${args.join(" ")} failed (exit ${result.status})\n${result.stderr.trim()}`,
    );
  }
  return result.stdout.trim();
};

/**
 * Run `[command, args]` in `cwd` and write its output to `logPath`. Returns
 * the exit status, or `null` when a signal stopped the process.
 */
const runLogged = ([command, args], { cwd, env, logPath }) => {
  const fd = openSync(logPath, "w");
  try {
    const result = spawnSync(command, args, { cwd, env, stdio: ["ignore", fd, fd] });
    if (result.error) throw result.error;
    return result.status;
  } finally {
    closeSync(fd);
  }
};

/** The lines of `logPath` that name the failed tests, at most `FAILURE_LINE_LIMIT`. */
const failureLines = (logPath) =>
  readFileSync(logPath, "utf8")
    .split(/\r?\n/)
    .filter((line) => FAILURE_LINE.test(line))
    .slice(0, FAILURE_LINE_LIMIT);

/**
 * Port the oracle onto `commit` in a new worktree, build it, and run it.
 * The result is `pass`, `fail`, `build-error`, `killed`, or `no-tests`. A
 * test that a signal stopped (for example out of memory) did not show the
 * bug, so `killed` is not `fail`. A test filter that matches no test exits
 * 0, so a run without a passed test is `no-tests`, not `pass`.
 */
const runOracleAt = (oracle, { source, commit, label, logs, env, keep, test }) => {
  const tree = mkdtempSync(join(tmpdir(), `oracle-proof-${label}-`));
  rmSync(tree, { recursive: true, force: true });
  git(source, ["worktree", "add", "--quiet", "--detach", tree, commit]);
  try {
    const notes = oracle.port(tree, source);
    for (const backport of oracle.backports ?? []) {
      if (!backport.applies(tree)) continue;
      backport.apply(tree);
      notes.push(`backported ${backport.name}`);
    }
    const buildLog = join(logs, `${label}-build.log`);
    const testLog = join(logs, `${label}-test.log`);
    if (runLogged(oracle.build, { cwd: tree, env, logPath: buildLog }) !== 0) {
      return { label, commit, tree, notes, result: "build-error", log: buildLog };
    }
    const status = runLogged(test, { cwd: tree, env, logPath: testLog });
    const result =
      status === null
        ? "killed"
        : status !== 0
          ? "fail"
          : TESTS_RAN.test(readFileSync(testLog, "utf8"))
            ? "pass"
            : "no-tests";
    return { label, commit, tree, notes, result, log: testLog };
  } finally {
    if (!keep) removeWorktree(source, tree);
  }
};

/** Remove `tree`, and warn instead of throw, so a failed removal does not hide the result. */
const removeWorktree = (source, tree) => {
  try {
    git(source, ["worktree", "remove", "--force", tree]);
  } catch (error) {
    console.error(`warning: could not remove the worktree ${tree}\n${error.message}`);
  }
};

const TOOL_ERRORS = new Set(["build-error", "killed", "no-tests"]);

/** A fix commit as `{ commit, scope?, reason?, test?, env? }`. A string is a commit with the full oracle. */
const toFix = (fix) => (typeof fix === "string" ? { commit: fix } : fix);

/**
 * Run `oracle` on `fix^` and on `fix`. The proof holds when the parent fails
 * and the fix passes. A fix with a `test` or an `env` narrows the oracle to
 * the tests of that fix, for example when the fix commit does not have a
 * later fix that other tests of the oracle need.
 */
export const proveFix = (oracle, fixSpec, { source, logs, env = process.env, keep = false }) => {
  const fix = toFix(fixSpec);
  const commit = git(source, ["rev-parse", "--verify", `${fix.commit}^{commit}`]);
  const parent = git(source, ["rev-parse", "--verify", `${commit}^`]);
  const short = commit.slice(0, 9);
  const shared = { source, logs, env: { ...env, ...fix.env }, keep, test: fix.test ?? oracle.test };
  const runs = [
    runOracleAt(oracle, { ...shared, commit: parent, label: `${short}-parent` }),
    runOracleAt(oracle, { ...shared, commit, label: `${short}-fix` }),
  ];
  const [before, after] = runs;
  const toolError = runs.some((run) => TOOL_ERRORS.has(run.result));
  const holds = before.result === "fail" && after.result === "pass";
  return {
    fix: short,
    scope: fix.scope ?? null,
    reason: fix.reason ?? null,
    runs,
    holds,
    toolError,
  };
};

const formatRun = (run, keep) => {
  const lines = [`  ${run.label} (${run.commit.slice(0, 9)}): ${run.result}`];
  lines.push(...run.notes.map((note) => `    ${note}`));
  lines.push(`    log: ${run.log}`);
  if (keep) lines.push(`    worktree: ${run.tree}`);
  if (run.result !== "pass") {
    lines.push(...failureLines(run.log).map((line) => `    | ${line}`));
  }
  return lines;
};

/** The human report of one proof. */
export const formatProof = (oracle, proof, keep = false) => {
  const verdict = proof.toolError
    ? "tool error: a build failed, a signal stopped a run, or no test ran"
    : proof.holds
      ? "proof holds: fails on the parent, passes on the fix"
      : "proof does not hold: the oracle must fail on the parent and pass on the fix";
  const scope = proof.scope
    ? [`  scope: ${proof.scope}${proof.reason ? ` (${proof.reason})` : ""}`]
    : [];
  return [
    `${oracle.name} on ${proof.fix}: ${verdict}`,
    ...scope,
    ...proof.runs.flatMap((run) => formatRun(run, keep)),
  ].join("\n");
};

const parseArgs = (argv) => {
  const options = { source: REPO_ROOT, logs: null, keep: false, list: false, positional: [] };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--keep") options.keep = true;
    else if (arg === "--list") options.list = true;
    else if (arg === "--source" || arg === "--logs") {
      const value = argv[index + 1];
      if (value === undefined || value.startsWith("--")) throw new Error(`${arg} needs a value`);
      options[arg.slice(2)] = resolve(value);
      index += 1;
    } else if (arg.startsWith("--")) throw new Error(`unknown option ${arg}`);
    else options.positional.push(arg);
  }
  return options;
};

const listOracles = () =>
  Object.values(ORACLES)
    .map((oracle) => {
      const fixes = oracle.fixes
        .map(toFix)
        .map((fix) => `  ${fix.commit}: ${fix.scope ?? "the full oracle"}`);
      return [`${oracle.name}: ${oracle.description}`, ...fixes].join("\n");
    })
    .join("\n");

/** Run the command line and return the exit code. */
export const main = (argv = process.argv.slice(2)) => {
  let options;
  try {
    options = parseArgs(argv);
  } catch (error) {
    console.error(`${error.message}\n\n${USAGE}`);
    return EXIT_TOOL_ERROR;
  }
  if (options.list) {
    console.log(listOracles());
    return EXIT_HOLDS;
  }
  const [name, ...fixArgs] = options.positional;
  const oracle = ORACLES[name];
  if (oracle === undefined) {
    console.error(
      `${name ? `unknown oracle ${name}` : "name an oracle"}\n\n${USAGE}\n\n${listOracles()}`,
    );
    return EXIT_TOOL_ERROR;
  }
  const fixes = fixArgs.length > 0 ? fixArgs : oracle.fixes;
  const logs = options.logs ?? mkdtempSync(join(tmpdir(), "oracle-proof-logs-"));
  mkdirSync(logs, { recursive: true });
  // One target directory for every worktree, so each build after the first
  // reuses the compiled dependencies.
  const env = {
    ...process.env,
    CARGO_TARGET_DIR: join(cargoTargetDir(options.source), "oracle-proof"),
  };
  let exit = EXIT_HOLDS;
  for (const fix of fixes) {
    try {
      const proof = proveFix(oracle, fix, {
        source: options.source,
        logs,
        env,
        keep: options.keep,
      });
      console.log(formatProof(oracle, proof, options.keep));
      if (proof.toolError) exit = EXIT_TOOL_ERROR;
      else if (!proof.holds && exit === EXIT_HOLDS) exit = EXIT_DOES_NOT_HOLD;
    } catch (error) {
      console.error(`${oracle.name} on ${toFix(fix).commit}: tool error\n${error.message}`);
      exit = EXIT_TOOL_ERROR;
    }
  }
  return exit;
};

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = main();
}
