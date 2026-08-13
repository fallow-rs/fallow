#!/usr/bin/env node
/**
 * Fail fast when Node module resolution escapes this checkout.
 *
 * Node resolves a bare specifier by walking ancestor directories until it
 * finds a matching `node_modules` entry, and `npm run` extends `PATH` the same
 * way. A checkout nested inside another checkout (for example a git worktree
 * created inside the clone) therefore borrows the outer install when it has
 * none of its own, silently running tool versions this checkout does not pin.
 * This guard turns that into a named failure.
 *
 * Run: `node scripts/assert-local-resolution.mjs <dependency>...`
 */

import { existsSync, realpathSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

import { runCliMain } from "./cli-main.mjs";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const DEFAULT_INSTALL_COMMAND = "npm ci";

const realPath = (path) => {
  try {
    return realpathSync(path);
  } catch {
    return path;
  }
};

const isInside = (root, path) =>
  path === root || path.startsWith(root.endsWith(sep) ? root : root + sep);

/**
 * Walk ancestor `node_modules` directories the way Node does.
 *
 * @param {string} dependency Package name, optionally scoped.
 * @param {string} startDirectory Directory to start the walk from.
 * @returns {string | null} Installed package directory, or `null` when absent.
 */
const ancestorPackageDirectory = (dependency, startDirectory) => {
  let directory = startDirectory;
  for (;;) {
    const candidate = join(directory, "node_modules", dependency);
    if (existsSync(join(candidate, "package.json"))) {
      return candidate;
    }
    const parent = dirname(directory);
    if (parent === directory) {
      return null;
    }
    directory = parent;
  }
};

/**
 * Locate the package directory Node would load for `dependency`.
 *
 * @param {string} dependency Package name, optionally scoped.
 * @param {string} resolveFrom Absolute path of the file that imports it.
 * @returns {string | null} Installed package directory, or `null` when absent.
 */
export const resolveDependencyDirectory = (dependency, resolveFrom) => {
  try {
    return dirname(createRequire(resolveFrom).resolve(`${dependency}/package.json`));
  } catch {
    // An `exports` map may hide `package.json`, so fall back to the walk.
  }
  return ancestorPackageDirectory(dependency, dirname(resolveFrom));
};

/**
 * Assert that `dependency` resolves to an install inside this checkout.
 *
 * @param {object} options
 * @param {string} options.dependency Package name, optionally scoped.
 * @param {string} [options.resolveFrom] Absolute path of the importing file;
 *   defaults to the repository manifest, which matches how `npm run` resolves
 *   binaries declared in the root `package.json`.
 * @param {string} [options.repoRoot] Checkout that must own the install.
 * @param {string} [options.installCommand] Command that creates that install.
 * @throws {Error} When the dependency is missing or resolves outside the
 *   checkout, naming the foreign path and the install command.
 */
export const assertLocalResolution = ({
  dependency,
  resolveFrom = join(REPO_ROOT, "package.json"),
  repoRoot = REPO_ROOT,
  installCommand = DEFAULT_INSTALL_COMMAND,
}) => {
  const directory = resolveDependencyDirectory(dependency, resolveFrom);
  if (directory === null) {
    throw new Error(
      `${dependency} is not installed anywhere Node can reach from ${resolveFrom}; ` +
        `run \`${installCommand}\` in ${repoRoot}`,
    );
  }
  if (isInside(repoRoot, directory) || isInside(realPath(repoRoot), realPath(directory))) {
    return;
  }
  throw new Error(
    `${dependency} resolves to ${directory}, which is outside this checkout (${repoRoot}). ` +
      "Node walks ancestor node_modules directories, so a checkout nested inside another " +
      "checkout runs the outer install and its versions instead of the pinned ones. " +
      `Run \`${installCommand}\` in ${repoRoot}.`,
  );
};

export const main = (args = process.argv.slice(2)) => {
  if (args.length === 0) {
    throw new Error("usage: node scripts/assert-local-resolution.mjs <dependency>...");
  }
  for (const dependency of args) {
    assertLocalResolution({ dependency });
  }
  return 0;
};

if (process.argv[1] !== undefined && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  runCliMain(main);
}
