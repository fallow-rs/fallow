#!/usr/bin/env node
/**
 * Fail fast when Node module resolution escapes this checkout.
 *
 * Node resolves a bare specifier by walking ancestor directories until it
 * finds a matching `node_modules` entry, and `npm run` extends `PATH` the same
 * way. A checkout nested inside another checkout (for example a git worktree
 * created inside the clone) therefore borrows the outer install when it has
 * none of its own, silently running tool versions this checkout does not pin.
 * This guard turns that into a named failure. It also compares the installed
 * version with an exact pin, so a stale local install fails too.
 *
 * Run: `node scripts/assert-local-resolution.mjs <dependency>...`
 */

import { existsSync, readFileSync, realpathSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

import { runCliMain } from "./cli-main.mjs";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const DEFAULT_INSTALL_COMMAND = "npm ci";
const DEPENDENCY_FIELDS = ["dependencies", "devDependencies", "optionalDependencies"];
const EXACT_VERSION = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/u;

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

const readJson = (path) => JSON.parse(readFileSync(path, "utf8"));

/**
 * Find the nearest `package.json` at or above `startDirectory`, and inside
 * `repoRoot`, that declares `dependency`.
 *
 * @returns {{ manifest: string, spec: string } | null}
 */
const declaredSpec = (dependency, startDirectory, repoRoot) => {
  let directory = startDirectory;
  for (;;) {
    const manifest = join(directory, "package.json");
    if (existsSync(manifest)) {
      const json = readJson(manifest);
      for (const field of DEPENDENCY_FIELDS) {
        const spec = json[field]?.[dependency];
        if (typeof spec === "string") {
          return { manifest, spec };
        }
      }
    }
    const parent = dirname(directory);
    if (parent === directory || !isInside(repoRoot, parent)) {
      return null;
    }
    directory = parent;
  }
};

const assertPinnedVersion = ({ dependency, directory, resolveFrom, repoRoot, installCommand }) => {
  const declared = declaredSpec(dependency, dirname(resolveFrom), repoRoot);
  // A range leaves the exact version to the lockfile, which this guard does not read.
  if (declared === null || !EXACT_VERSION.test(declared.spec)) {
    return;
  }
  const installed = readJson(join(directory, "package.json")).version;
  if (installed === declared.spec) {
    return;
  }
  throw new Error(
    `${dependency} ${installed} is installed at ${directory}, but ${declared.manifest} ` +
      `pins ${declared.spec}. The local install is stale. Run \`${installCommand}\` in ${repoRoot}.`,
  );
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
 * @throws {Error} When the dependency is missing, resolves outside the
 *   checkout, or has a version other than the exact pin in the nearest
 *   `package.json` that declares it. The message names the path, both
 *   versions where they apply, and the install command.
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
    assertPinnedVersion({ dependency, directory, resolveFrom, repoRoot, installCommand });
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
