import { lstatSync, realpathSync, statSync } from "node:fs";
import path from "node:path";

/**
 * The size limit for one file that the checker reads. Generated declaration
 * files of large packages stay well below it.
 */
const MAX_CHECKER_READ_BYTES = 64 * 1024 * 1024;

/**
 * The environment variable that names the read scope base. fallow sets it to
 * the Git work tree of the analysis root (the root itself when the work tree
 * is the home directory).
 */
const SCOPE_ROOT_ENV = "FALLOW_TYPE_AWARE_SCOPE_ROOT";

const withSeparator = (dir) => (dir.endsWith(path.sep) ? dir : `${dir}${path.sep}`);

const isWithin = (dir, file) => file === dir || file.startsWith(withSeparator(dir));

const lstatOrNull = (file) => {
  try {
    return lstatSync(file);
  } catch {
    return null;
  }
};

/**
 * Resolve paths with one `realpath` call per directory. A file is resolved
 * on its own only when `lstat` shows that it is a link.
 */
const createResolver = () => {
  const directories = new Map();
  const realDirectory = (dir) => {
    const cached = directories.get(dir);
    if (cached !== undefined) return cached;
    let real;
    try {
      real = realpathSync(dir);
    } catch {
      const parent = path.dirname(dir);
      real = parent === dir ? dir : path.join(realDirectory(parent), path.basename(dir));
    }
    directories.set(dir, real);
    return real;
  };
  return (file) => {
    const absolute = path.resolve(file);
    const parent = path.dirname(absolute);
    if (parent === absolute) return { real: absolute, stats: lstatOrNull(absolute) };
    const located = path.join(realDirectory(parent), path.basename(absolute));
    const stats = lstatOrNull(located);
    if (!stats?.isSymbolicLink()) return { located, real: located, stats };
    try {
      const real = realpathSync(located);
      return { located, real, stats: statSync(real) };
    } catch {
      // A dangling link has no content to give.
      return { located, real: located, stats: null };
    }
  };
};

const realOrResolved = (file) => {
  try {
    return realpathSync(file);
  } catch {
    return path.resolve(file);
  }
};

/**
 * Each real `node_modules` directory at or above `dir`. A `node_modules`
 * entry that is a link is skipped, because a committed link can point at any
 * directory, also the file system root.
 */
const nodeModulesAbove = (dir) => {
  const found = [];
  for (let current = dir; ; current = path.dirname(current)) {
    const candidate = path.join(current, "node_modules");
    if (lstatOrNull(candidate)?.isDirectory()) found.push(candidate);
    if (path.dirname(current) === current) return found;
  }
};

/**
 * The read scope base of `root`: `scopeRoot` (or the scope root that fallow
 * sets in the environment) when it is the root or a directory above it, and
 * the resolved root otherwise.
 *
 * @param {string} root
 * @param {string | undefined} scopeRoot
 */
const readScopeBase = (root, scopeRoot = process.env[SCOPE_ROOT_ENV]) => {
  const realRoot = realOrResolved(root);
  if (!scopeRoot || !path.isAbsolute(scopeRoot)) return realRoot;
  const base = realOrResolved(scopeRoot);
  return isWithin(base, realRoot) ? base : realRoot;
};

const hasNodeModulesSegment = (file) => file.split(path.sep).includes("node_modules");

/**
 * A test of whether the checker may touch a path for the project at `root`.
 *
 * A path is in scope when its resolved path is inside the read scope base
 * (see {@link readScopeBase}), inside a real `node_modules` directory at or
 * above the root, or inside one of `compilerDirs`. A link below an allowed
 * `node_modules` directory is also in scope when its target is inside the
 * base or has a `node_modules` segment, such as a workspace link or the
 * pnpm virtual store. A path that is not absolute, such as a bundled library
 * file of the compiler, is left to the compiler.
 *
 * The result names whether the path is in scope and, for a file that may be
 * read, whether it is a regular file of at most
 * {@link MAX_CHECKER_READ_BYTES} bytes.
 *
 * @param {string} root
 * @param {string[]} [compilerDirs] directories of the compiler and its library files
 * @param {string} [scopeRoot]
 * @returns {{ inScope: (fileName: string) => boolean, readable: (fileName: string) => boolean }}
 */
export const createReadScope = (root, compilerDirs = [], scopeRoot = undefined) => {
  const base = readScopeBase(root, scopeRoot);
  const nodeModules = nodeModulesAbove(realOrResolved(root));
  const allowed = [base, ...nodeModules, ...compilerDirs.map(realOrResolved)];
  const resolve = createResolver();
  const check = (fileName) => {
    if (!path.isAbsolute(fileName)) return { inScope: true, stats: undefined };
    const { located, real, stats } = resolve(fileName);
    if (allowed.some((dir) => isWithin(dir, real))) return { inScope: true, stats };
    const linkedFromNodeModules =
      located !== undefined &&
      located !== real &&
      nodeModules.some((dir) => isWithin(dir, located)) &&
      (isWithin(base, real) || hasNodeModulesSegment(real));
    return { inScope: linkedFromNodeModules, stats };
  };
  return {
    inScope: (fileName) => check(fileName).inScope,
    readable: (fileName) => {
      const { inScope, stats } = check(fileName);
      if (!inScope) return false;
      // A missing file has no content to give, and the compiler reports it.
      if (stats === undefined || stats === null) return true;
      return stats.isFile() && stats.size <= MAX_CHECKER_READ_BYTES;
    },
  };
};

/**
 * True when `file` resolves inside the read scope base of `root` (see
 * {@link readScopeBase}), also through links. A missing file is compared by
 * its path.
 */
export const resolvesInside = (root, file) =>
  isWithin(readScopeBase(root), createResolver()(file).real);
