// Shared git steps for the serial merge helpers of the ship workflow:
// `ship-rebase.mjs` for this repository and `ship-docs-rebase.mjs` for the
// public documentation repository.

import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, realpathSync, rmSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { isAbsolute, join, resolve } from "node:path";
import { parseArgs } from "node:util";

const OUTPUT_LIMIT = 64 * 1024 * 1024;

// An inherited GIT_DIR or GIT_WORK_TREE, for example from a hook, would point
// the commands at another repository than the working directory.
const GIT_LOCATION_VARIABLES = ["GIT_DIR", "GIT_WORK_TREE", "GIT_COMMON_DIR", "GIT_INDEX_FILE"];

// Settings for every rebase command. A recorded rerere resolution from another
// branch can resolve a conflict with the wrong value and let the rebase go on
// without a stop. Git enables rerere when `rr-cache` exists, so turn it off
// explicitly. The plain conflict style keeps the marker format known.
const REBASE_CONFIG = [
  "-c",
  "rerere.enabled=false",
  "-c",
  "rerere.autoUpdate=false",
  "-c",
  "merge.conflictStyle=merge",
];

// Keep the rebase to the detached HEAD and keep it linear, whatever the
// config of the checkout says. `--update-refs` would move other local
// branches. `--rebase-merges` would recreate merge commits, and the conflict
// resolution would then run on merge conflicts. `--no-autosquash` and
// `--no-autostash` are only a guard for other Git versions: Git 2.55 reads
// `rebase.autoSquash` only for an interactive rebase, and the temporary
// worktree has no changes, so autostash has nothing to stash.
const REBASE_FLAGS = [
  "--no-update-refs",
  "--no-rebase-merges",
  "--no-autosquash",
  "--no-autostash",
];

const gitEnv = (extra = {}) => {
  const env = { ...process.env, ...extra };
  for (const name of GIT_LOCATION_VARIABLES) {
    delete env[name];
  }
  return env;
};

/**
 * Run a command and return `{ status, stdout, stderr }`. Throw when the
 * command fails and `allowFailure` is not set.
 */
export const run = (command, args, { cwd, env, allowFailure = false } = {}) => {
  const result = spawnSync(command, args, {
    cwd,
    env: gitEnv(env),
    encoding: "utf8",
    maxBuffer: OUTPUT_LIMIT,
  });
  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0 && !allowFailure) {
    const output = `${result.stderr}${result.stdout}`.trim();
    throw new Error(`${command} ${args.join(" ")} failed (exit ${result.status})\n${output}`);
  }
  return { status: result.status, stdout: result.stdout, stderr: result.stderr };
};

/** Run git in `cwd` and return its trimmed standard output. */
export const git = (cwd, args, options = {}) => run("git", args, { ...options, cwd }).stdout.trim();

/** The paths with an unmerged index entry, in git order. */
const unmergedPaths = (cwd) =>
  git(cwd, ["diff", "--name-only", "--diff-filter=U", "-z"])
    .split("\0")
    .filter((path) => path !== "");

/** True when a rebase has stopped in `cwd` and waits for `--continue`. */
const rebaseInProgress = (cwd) =>
  ["rebase-merge", "rebase-apply"].some((name) =>
    existsSync(resolve(cwd, git(cwd, ["rev-parse", "--git-path", name]))),
  );

/**
 * The head branch of pull request `pr`, read with `gh` in `cwd`. A pull
 * request from a fork has no branch on `origin`, so it is refused.
 */
const pullRequestBranch = (cwd, pr) => {
  const view = run("gh", ["pr", "view", pr, "--json", "headRefName,isCrossRepository"], { cwd });
  const { headRefName, isCrossRepository } = JSON.parse(view.stdout);
  if (isCrossRepository) {
    throw new Error(
      `Pull request ${pr} comes from a fork. Rebase it in a checkout of the fork instead.`,
    );
  }
  return headRefName;
};

/**
 * Fetch `remote` in `cwd` and resolve the refs that the rebase and its
 * checks need. The fetch changes only remote-tracking refs, so the checkout
 * of `cwd` stays as it is.
 */
const fetchBranch = (cwd, { remote, branch, base }) => {
  git(cwd, ["fetch", "--quiet", remote]);
  const oldTip = git(cwd, ["rev-parse", "--verify", `refs/remotes/${remote}/${branch}^{commit}`]);
  const baseTip = git(cwd, ["rev-parse", "--verify", `${base}^{commit}`]);
  const oldBase = git(cwd, ["merge-base", oldTip, baseTip]);
  return { oldTip, baseTip, oldBase };
};

/**
 * The `git -c` arguments that keep the hooks of the checkout in `top` for
 * the commands in the temporary worktree. Git reads a relative
 * `core.hooksPath` from the top of the worktree where the hook runs, so the
 * temporary worktree would not find the hooks of the checkout. An absolute
 * path and the default hooks directory are the same for every worktree.
 */
const hookConfig = (top) => {
  const hooksPath = run("git", ["config", "--type=path", "--get", "core.hooksPath"], {
    cwd: top,
    allowFailure: true,
  }).stdout.trim();
  if (hooksPath === "" || isAbsolute(hooksPath)) {
    return [];
  }
  return ["-c", `core.hooksPath=${resolve(top, hooksPath)}`];
};

/**
 * Add a temporary worktree with a detached HEAD at `commit` and return its
 * path. The rebase, the checks and the push run there, so the checkout in
 * `top` keeps its branch, its HEAD and its work tree.
 */
const addWorktree = (top, commit, { branch, hooks }) => {
  const name = branch.replace(/[^\w.-]+/gu, "-");
  const path = realpathSync(mkdtempSync(join(tmpdir(), `ship-${name}-`)));
  try {
    git(top, [...hooks, "worktree", "add", "--quiet", "--detach", path, commit]);
  } catch (error) {
    // Without a worktree, the empty directory has no use and no report names it.
    rmSync(path, { recursive: true, force: true });
    throw error;
  }
  return path;
};

/** Remove the temporary worktree. A failure only prints a warning. */
const removeWorktree = (top, log, path) => {
  const removed = run("git", ["worktree", "remove", "--force", path], {
    cwd: top,
    allowFailure: true,
  });
  run("git", ["worktree", "prune"], { cwd: top, allowFailure: true });
  if (removed.status !== 0) {
    log(`Could not remove the temporary worktree ${path}:\n${removed.stderr.trim()}`);
  }
};

const stopped = (reason, paths = []) => ({ ok: false, reason, paths });

const continueRebase = (cwd, hooks) => {
  // A resolution can leave nothing to commit when the base already holds the
  // whole change. `--continue` refuses that commit, so skip it instead.
  const nothingStaged = run("git", ["diff", "--cached", "--quiet"], { cwd, allowFailure: true });
  const step = nothingStaged.status === 0 ? "--skip" : "--continue";
  return {
    skipped: step === "--skip",
    result: run("git", [...hooks, ...REBASE_CONFIG, "rebase", step], {
      cwd,
      env: { GIT_EDITOR: "true" },
      allowFailure: true,
    }),
  };
};

/**
 * Rebase the detached HEAD in `cwd` onto `onto` with rerere turned off.
 *
 * At each stop, `resolveConflicts(paths)` gets the unmerged paths. It returns
 * `{ ok: true }` after it staged a resolution, or `{ ok: false, reason }` to
 * stop. A stop leaves the rebase in progress, so the person can inspect it.
 *
 * Returns `{ ok: true, resolved, skipped }` with the count of resolved
 * stops and the count of commits that became empty after a resolution, or
 * `{ ok: false, reason, paths }`.
 */
const rebaseOnto = (cwd, onto, resolveConflicts, hooks) => {
  const commitCount = Number(git(cwd, ["rev-list", "--count", `${onto}..HEAD`]));
  let result = run("git", [...hooks, ...REBASE_CONFIG, "rebase", ...REBASE_FLAGS, onto], {
    cwd,
    allowFailure: true,
  });
  let skipped = 0;
  for (let stop = 0; stop <= commitCount; stop += 1) {
    if (!rebaseInProgress(cwd)) {
      if (result.status !== 0) {
        throw new Error(`git rebase failed (exit ${result.status})\n${result.stderr.trim()}`);
      }
      return { ok: true, resolved: stop, skipped };
    }
    const paths = unmergedPaths(cwd);
    if (paths.length === 0) {
      return stopped(`The rebase stopped without a conflict:\n${result.stderr.trim()}`);
    }
    const resolution = resolveConflicts(paths);
    if (!resolution.ok) {
      return stopped(resolution.reason, paths);
    }
    const next = continueRebase(cwd, hooks);
    skipped += next.skipped ? 1 : 0;
    result = next.result;
  }
  return stopped("The rebase stopped more often than it has commits.");
};

// `git merge-tree` exits with 1 when the merge has conflicts. The tree it
// writes then holds the conflict markers.
const MERGE_TREE_CONFLICT_EXIT = 1;

const nulSeparated = (text) => text.split("\0").filter((path) => path !== "");

/**
 * Compare the rebase result with a three-way merge of the old branch tip
 * into the base. A rebase drops the merge commits of the branch, and with
 * them each change that a merge commit made. Such a change then shows as a
 * difference here. The paths in `ignored` are left out, because the caller
 * checks them in a different way.
 *
 * Where the merge itself conflicts, its tree holds conflict markers, so it
 * is no reference. When the branch has no merge commits, the rebase dropped
 * no change, and it applied each commit at such a path without a conflict.
 * These paths are then only logged. When the branch has merge commits, they
 * stay problems that a person must compare by hand.
 *
 * Returns `[]` when the trees are the same, or a list with one problem.
 */
const checkRebaseTree = (cwd, log, { oldBase, oldTip, baseTip, newTip, base, ignored }) => {
  const merge = run(
    "git",
    ["merge-tree", "--write-tree", "--name-only", "--no-messages", "-z", baseTip, oldTip],
    { cwd, allowFailure: true },
  );
  if (merge.status !== 0 && merge.status !== MERGE_TREE_CONFLICT_EXIT) {
    throw new Error(`git merge-tree failed (exit ${merge.status})\n${merge.stderr.trim()}`);
  }
  const [mergeTree, ...conflicted] = nulSeparated(merge.stdout);
  const exclusions = ignored.map((path) => `:(exclude,top)${path}`);
  const differences = nulSeparated(
    git(cwd, ["diff", "--name-only", "--no-renames", "-z", mergeTree, newTip, "--", ...exclusions]),
  );
  const conflicts = new Set(conflicted);
  const merges = git(cwd, ["rev-list", "--count", "--merges", `${oldBase}..${oldTip}`]);
  const unchecked = merges === "0" ? differences.filter((path) => conflicts.has(path)) : [];
  if (unchecked.length > 0) {
    log(
      `Tree check: the merge conflicts at these paths, so the check does not compare them. The branch has no merge commits:\n${unchecked.map((path) => `    ${path}`).join("\n")}`,
    );
  }
  const compared = differences.filter((path) => !unchecked.includes(path));
  if (compared.length === 0) {
    return [];
  }
  const lines = compared.map((path) =>
    conflicts.has(path)
      ? `    ${path} (the merge conflicts here, so compare it by hand)`
      : `    ${path}`,
  );
  if (merges !== "0") {
    lines.push(
      `  The branch has ${merges} merge commits. The rebase drops the changes of a merge commit.`,
    );
  }
  return [
    `Paths where the rebase result differs from a merge of the branch into ${base}:\n${lines.join("\n")}`,
  ];
};

/** Print the result of a complete rebase and return the new tip. */
const logRebased = (cwd, log, { branch, base, baseTip, resolved, skipped }) => {
  const newTip = git(cwd, ["rev-parse", "HEAD"]);
  const commits = git(cwd, ["rev-list", "--count", `${baseTip}..${newTip}`]);
  log(`Rebased ${branch} onto ${base}: ${commits} commits, new tip ${newTip}.`);
  log(`Resolved conflicts at ${resolved} stops of the rebase.`);
  if (skipped > 0) {
    log(`Skipped ${skipped} commits that became empty after the conflict resolution.`);
  }
  return newTip;
};

const SHELL_SAFE = /^[\w@%+=:,./-]+$/u;

/**
 * `args` as one shell command line. An argument with a character that the
 * shell reads, for example `$`, `;` or `'`, gets single quotes. A Git ref
 * name can contain such characters.
 */
export const shellCommand = (args) =>
  args.map((arg) => (SHELL_SAFE.test(arg) ? arg : `'${arg.replaceAll("'", "'\\''")}'`)).join(" ");

/**
 * The arguments of `git push` that push HEAD to `remote/branch`, but only
 * when the remote branch is still at `oldTip`. A push from someone else
 * since the fetch makes the push fail.
 */
export const leasePushArgs = ({ remote, branch, oldTip }) => {
  const ref = `refs/heads/${branch}`;
  return ["push", `--force-with-lease=${ref}:${oldTip}`, remote, `HEAD:${ref}`];
};

/** The shell command that runs `git args` in the worktree at `path`. */
export const worktreeCommand = (path, args) => shellCommand(["git", "-C", path, ...args]);

/**
 * The report for a temporary worktree that the script keeps after a stop.
 * It names the path and the next commands. `push` adds the lease push
 * command: a stop after a failed check leaves it out, because the result
 * must not go to the remote.
 */
const keptWorktreeReport = ({ top, worktree, remote, branch, oldTip }, { push }) => {
  let inRebase = false;
  try {
    inRebase = rebaseInProgress(worktree);
  } catch {
    // A broken worktree must not hide the original error or the kept path.
    return `The temporary worktree ${worktree} is kept. Your checkout did not change.`;
  }
  const lines = [
    `The temporary worktree ${worktree} holds the result. Your checkout did not change.`,
  ];
  if (inRebase) {
    lines.push(
      "The rebase is still in progress. Resolve the conflicts in that worktree, then run:",
      `  ${worktreeCommand(worktree, ["rebase", "--continue"])}`,
    );
  }
  if (push) {
    lines.push(
      "A push of a result that you changed by hand skips the checks of this script. Check the result first.",
      "Push the result from that worktree with a lease on the old branch tip:",
      `  ${worktreeCommand(worktree, leasePushArgs({ remote, branch, oldTip }))}`,
    );
  }
  lines.push("To discard the worktree:");
  if (inRebase) {
    lines.push(`  ${worktreeCommand(worktree, ["rebase", "--abort"])}`);
  }
  lines.push(`  ${worktreeCommand(top, ["worktree", "remove", "--force", worktree])}`);
  return lines.join("\n");
};

/**
 * Extra environment for the pre-push hook in the temporary worktree. A
 * Cargo hook would otherwise build into a new, empty `target` directory of
 * the worktree. The `target` directory of the checkout keeps the build
 * cache. A `CARGO_TARGET_DIR` that the user set stays as it is.
 */
const pushCargoEnv = (env, top) => {
  const target = join(top, "target");
  if (env.CARGO_TARGET_DIR || !existsSync(target) || !statSync(target).isDirectory()) {
    return {};
  }
  return { CARGO_TARGET_DIR: target };
};

/** The SSH command that keeps an idle push connection open. */
const KEEPALIVE_SSH_COMMAND = "ssh -o ServerAliveInterval=30 -o ServerAliveCountMax=40";

/**
 * Extra environment for `git push`. Git opens the SSH connection before the
 * pre-push hook runs, and a slow hook leaves the connection idle until the
 * remote closes it. SSH keepalives hold it open. An SSH command that the user
 * set in `GIT_SSH_COMMAND` or `core.sshCommand` stays as it is.
 */
export const pushSshEnv = (env, configuredSshCommand) =>
  env.GIT_SSH_COMMAND || configuredSshCommand ? {} : { GIT_SSH_COMMAND: KEEPALIVE_SSH_COMMAND };

/** With `push`, push HEAD with a lease on `oldTip` (see `leasePushArgs`). */
const pushIfAsked = (cwd, log, { push, remote, branch, base, oldTip, newTip, top, hooks }) => {
  if (!push) {
    return;
  }
  if (newTip === oldTip) {
    log(`The branch is already on ${base}. Nothing to push.`);
    return;
  }
  // The pre-push hook of the checkout runs now, and its output shows only
  // when the push ends.
  log(`Pushing ${branch}. The pre-push hook of the checkout runs first.`);
  const configured = run("git", ["config", "--get", "core.sshCommand"], {
    cwd,
    allowFailure: true,
  }).stdout.trim();
  git(cwd, [...hooks, ...leasePushArgs({ remote, branch, oldTip }), "--quiet"], {
    env: { ...pushSshEnv(process.env, configured), ...pushCargoEnv(process.env, top) },
  });
  log(`Pushed ${branch} with a lease on ${oldTip.slice(0, 12)}.`);
};

/**
 * Run the tree check in the worktree `cwd` and then
 * `checks(rebased, treeProblems)`, which returns a list of problems.
 * `treeProblems` is the result of the tree check. Print each problem and
 * return 1 when there is one. Otherwise print the `passed` lines and
 * return 0. The tree check leaves out the paths in `ignored`.
 */
export const checkRebase = (cwd, log, options, rebased, { ignored, checks, passed }) => {
  const treeProblems = checkRebaseTree(cwd, log, { ...rebased, base: options.base, ignored });
  const problems = [...treeProblems, ...checks(rebased, treeProblems)];
  if (problems.length > 0) {
    log(`CHECK FAILED: ${rebased.branch}`);
    for (const problem of problems) {
      log(`  ${problem}`);
    }
    return 1;
  }
  log(
    `Tree check: outside ${ignored.join(", ")}, the result matches a merge into ${options.base}.`,
  );
  for (const line of passed) {
    log(line);
  }
  return 0;
};

/**
 * Rebase the branch of `options` on its base in a temporary worktree, with
 * rerere turned off. The checkout in `cwd` does not change.
 *
 * - `resolveConflicts(worktree)` returns the resolver for the stops of the
 *   rebase (see `rebaseOnto`).
 * - `check(worktree, rebased)` runs the checks of the result and returns
 *   the exit code, for example with `checkRebase`. `rebased` holds
 *   `{ branch, oldBase, oldTip, baseTip, newTip, worktree }`.
 *
 * When the checks pass, push when `options.push` is set and remove the
 * worktree. After a stop, a failed check or a failed command, keep the
 * worktree and print its path and the next commands. Returns the exit
 * code, or throws for a failed command.
 */
export const shipBranch = (cwd, log, options, { resolveConflicts, check }) => {
  const branch = options.branch ?? pullRequestBranch(cwd, options.pr);
  const refs = fetchBranch(cwd, { ...options, branch });
  const hooks = hookConfig(cwd);
  const worktree = addWorktree(cwd, refs.oldTip, { branch, hooks });
  const target = { ...options, ...refs, branch, top: cwd, worktree, hooks };
  let pushing = false;
  try {
    const rebase = rebaseOnto(worktree, refs.baseTip, resolveConflicts(worktree), hooks);
    if (!rebase.ok) {
      log(`STOP: ${branch}: ${rebase.reason}`);
      log(keptWorktreeReport(target, { push: true }));
      return 1;
    }
    const newTip = logRebased(worktree, log, { ...options, ...rebase, ...refs, branch });
    const rebased = { ...refs, branch, newTip, worktree };
    const code = check(worktree, rebased);
    if (code !== 0) {
      log(keptWorktreeReport(target, { push: false }));
      return code;
    }
    pushing = true;
    pushIfAsked(worktree, log, { ...target, newTip });
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    throw new Error(`${message}\n${keptWorktreeReport(target, { push: pushing })}`, {
      cause: error,
    });
  }
  removeWorktree(cwd, log, worktree);
  return 0;
};

const DEFAULT_REMOTE = "origin";
const DEFAULT_BASE_BRANCH = "main";

/**
 * Parse the options that both rebase helpers share. Returns `{ help: true }`
 * for `--help`. Throws on invalid input.
 */
const parseShipOptions = (argv) => {
  const { values } = parseArgs({
    args: argv,
    options: {
      pr: { type: "string" },
      branch: { type: "string" },
      base: { type: "string" },
      remote: { type: "string", default: DEFAULT_REMOTE },
      push: { type: "boolean", default: false },
      help: { type: "boolean", short: "h", default: false },
    },
    strict: true,
  });
  if (values.help) {
    return { help: true };
  }
  if ((values.pr === undefined) === (values.branch === undefined)) {
    throw new Error("Give exactly one of --pr or --branch.");
  }
  if (values.pr !== undefined && !/^[1-9]\d*$/u.test(values.pr)) {
    throw new Error(`--pr must be a pull request number, not "${values.pr}".`);
  }
  return {
    help: false,
    pr: values.pr ?? null,
    branch: values.branch ?? null,
    remote: values.remote,
    base: values.base ?? `${values.remote}/${DEFAULT_BASE_BRANCH}`,
    push: values.push,
  };
};

/**
 * The CLI entry of a rebase helper. `ship(root, options)` gets the top
 * level of the checkout that holds the working directory.
 */
export const shipMain = (usage, ship) => () => {
  let options;
  try {
    options = parseShipOptions(process.argv.slice(2));
  } catch (error) {
    console.error(`${error.message}\n\n${usage}`);
    return 2;
  }
  if (options.help) {
    console.log(usage);
    return 0;
  }
  try {
    return ship(git(process.cwd(), ["rev-parse", "--show-toplevel"]), options);
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    return 2;
  }
};
