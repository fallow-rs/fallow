#!/usr/bin/env node
/**
 * Prepare a clone for contributor work: check the tools, install the git
 * hooks, and install the root and VS Code extension dependencies.
 *
 * This script runs before any dependency install, so it uses only Node
 * built-ins. You can run it again at any time. Each step leaves a finished
 * setup unchanged.
 *
 * Run: `pnpm bootstrap` or `node scripts/bootstrap.mjs`
 */

import { spawnSync } from "node:child_process";
import { realpathSync } from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

export const MIN_NODE = "22.13.0";

// Old clones pointed git at this folder before hk took over the hooks.
export const LEGACY_HOOKS_PATH = ".githooks";

export const REQUIRED_TOOLS = [
  {
    name: "pnpm",
    hint: "pnpm is missing. Run `mise install`, or install pnpm 11.25.0 from https://pnpm.io/installation",
  },
  {
    name: "hk",
    hint: "hk is missing. Run `mise install`, or install hk and pkl at the versions in mise.toml from https://hk.jdx.dev",
  },
];

export const DEPENDENCY_INSTALLS = [
  { label: "root tools", cwd: "." },
  { label: "VS Code extension", cwd: "editors/vscode" },
];

const parseVersion = (version) =>
  version
    .replace(/^v/u, "")
    .split("-")[0]
    .split(".")
    .map((part) => Number.parseInt(part, 10) || 0);

/** Return true when `version` is at least `minimum`. Both use `major.minor.patch`. */
export const isAtLeast = (version, minimum) => {
  const actual = parseVersion(version);
  const wanted = parseVersion(minimum);
  for (let index = 0; index < 3; index += 1) {
    const difference = (actual[index] ?? 0) - (wanted[index] ?? 0);
    if (difference !== 0) return difference > 0;
  }
  return true;
};

/**
 * Return one message per problem with the local tools, or an empty list.
 *
 * @param {{ nodeVersion: string, hasCommand: (name: string) => boolean }} probe
 * @returns {string[]}
 */
export const findToolProblems = ({ nodeVersion, hasCommand }) => {
  const problems = [];
  if (!isAtLeast(nodeVersion, MIN_NODE)) {
    problems.push(
      `Node.js ${nodeVersion} is too old. Install Node.js ${MIN_NODE} or later, for example with \`mise install\`.`,
    );
  }
  for (const { name, hint } of REQUIRED_TOOLS) {
    if (!hasCommand(name)) problems.push(hint);
  }
  return problems;
};

/**
 * Decide what to do with the `core.hooksPath` git setting.
 *
 * @param {string | null} value The current value, or `null` when it is not set.
 * @returns {"keep" | "unset" | "warn"}
 */
export const decideHooksPath = (value) => {
  if (value === null || value.trim() === "") return "keep";
  const normalized = value.trim().replace(/^\.\//u, "").replace(/\/+$/u, "");
  return normalized === LEGACY_HOOKS_PATH ? "unset" : "warn";
};

// From this version, hk installs hooks through git config, and git runs them
// next to the hooks in `core.hooksPath`. Older git runs only `core.hooksPath`.
export const CONFIG_HOOKS_GIT = "2.54.0";

/** Return the warning for a `core.hooksPath` value that bootstrap keeps. */
export const hooksPathWarning = (value, gitVersion) =>
  isAtLeast(gitVersion, CONFIG_HOOKS_GIT)
    ? `core.hooksPath is ${value}. Bootstrap keeps it. git runs the hooks in that folder in addition to the hk hooks.`
    : `core.hooksPath is ${value}. Bootstrap keeps it. git ${gitVersion} runs only the hooks in that folder, so the hk hooks do not run. Remove the setting to use the hk hooks.`;

/** Return the `hk` arguments that install the hooks. */
export const hkInstallArgs = (hasMise) => (hasMise ? ["install", "--mise"] : ["install"]);

/**
 * Run every bootstrap step and return the process exit code.
 *
 * @param {object} io Side effects, injectable for tests.
 * @param {string} io.nodeVersion The running Node.js version.
 * @param {(name: string) => boolean} io.hasCommand True when the command is on PATH.
 * @param {string} io.gitVersion The git version, for example `2.54.0`.
 * @param {() => { local: string | null, effective: string | null }} io.readHooksPath
 *   The `core.hooksPath` value in the repository config and in all scopes.
 * @param {() => boolean} io.unsetHooksPath Remove `core.hooksPath` from the repository config. True on success.
 * @param {(command: string, args: string[], cwd: string) => number} io.run Run a command, return its exit status.
 * @param {(message: string) => void} io.log Print a progress line.
 * @param {(message: string) => void} io.warn Print a warning or an error line.
 * @returns {number}
 */
export const bootstrap = ({
  nodeVersion,
  gitVersion,
  hasCommand,
  readHooksPath,
  unsetHooksPath,
  run,
  log,
  warn,
}) => {
  const problems = findToolProblems({ nodeVersion, hasCommand });
  if (problems.length > 0) {
    for (const problem of problems) warn(problem);
    return 1;
  }

  // Bootstrap removes only the repository value. A value from another scope
  // can belong to other projects, so bootstrap only warns about it.
  const { local } = readHooksPath();
  if (decideHooksPath(local) === "unset") {
    if (!unsetHooksPath()) {
      warn(
        `core.hooksPath is ${local}, but bootstrap cannot remove it. Run \`git config --local --unset-all core.hooksPath\`.`,
      );
      return 1;
    }
    log(
      `Removed core.hooksPath=${local}. hk now runs the hooks, and git no longer runs ${LEGACY_HOOKS_PATH}.`,
    );
  }
  const { effective } = readHooksPath();
  if (decideHooksPath(effective) !== "keep") warn(hooksPathWarning(effective, gitVersion));

  const steps = [
    {
      label: "Install the git hooks",
      command: "hk",
      args: hkInstallArgs(hasCommand("mise")),
      cwd: ".",
    },
    ...DEPENDENCY_INSTALLS.map(({ label, cwd }) => ({
      label: `Install the ${label} dependencies`,
      command: "pnpm",
      args: ["install", "--frozen-lockfile"],
      cwd,
    })),
  ];
  for (const { label, command, args, cwd } of steps) {
    log(`${label}: ${[command, ...args].join(" ")}${cwd === "." ? "" : ` (in ${cwd})`}`);
    const status = run(command, args, cwd);
    if (status !== 0) {
      warn(`${label} failed with exit code ${status}.`);
      return 1;
    }
  }

  log("Bootstrap is complete. Run `pnpm verify:fast` to check the setup.");
  return 0;
};

// Windows installs pnpm as a `.cmd` shim, which needs a shell to start.
const USE_SHELL = process.platform === "win32";

const spawn = (command, args, cwd, stdio) =>
  spawnSync(command, args, {
    cwd: path.join(ROOT, cwd),
    stdio,
    shell: USE_SHELL,
    encoding: "utf8",
  });

const readHooksPathValue = (scopeArgs) => {
  const result = spawn("git", ["config", ...scopeArgs, "--get", "core.hooksPath"], ".", "pipe");
  return result.status === 0 ? result.stdout.trim() : null;
};

const readGitVersion = () =>
  /\d+\.\d+\.\d+/u.exec(spawn("git", ["--version"], ".", "pipe").stdout ?? "")?.[0] ?? "0.0.0";

const defaultIo = () => ({
  nodeVersion: process.versions.node,
  gitVersion: readGitVersion(),
  hasCommand: (name) => {
    const result = spawn(name, ["--version"], ".", "ignore");
    return result.error === undefined && result.status === 0;
  },
  readHooksPath: () => ({
    local: readHooksPathValue(["--local"]),
    effective: readHooksPathValue([]),
  }),
  unsetHooksPath: () =>
    spawn("git", ["config", "--local", "--unset-all", "core.hooksPath"], ".", "ignore").status ===
    0,
  run: (command, args, cwd) => spawn(command, args, cwd, "inherit").status ?? 1,
  log: (message) => console.log(`bootstrap: ${message}`),
  warn: (message) => console.error(`bootstrap: ${message}`),
});

const isDirectInvocation =
  process.argv[1] !== undefined &&
  import.meta.url === pathToFileURL(realpathSync(path.resolve(process.argv[1]))).href;

if (isDirectInvocation) {
  process.exitCode = bootstrap(defaultIo());
}
