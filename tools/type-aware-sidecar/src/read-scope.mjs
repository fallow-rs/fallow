import { realpathSync, statSync } from "node:fs";
import path from "node:path";

/**
 * The size limit for one file that the checker reads. Generated declaration
 * files of large packages stay well below it.
 */
const MAX_CHECKER_READ_BYTES = 64 * 1024 * 1024;

const isWithin = (dir, file) => {
  const relative = path.relative(dir, file);
  return (
    relative === "" ||
    (relative !== ".." && !relative.startsWith(`..${path.sep}`) && !path.isAbsolute(relative))
  );
};

/**
 * The resolved path of `file`. For a missing file, the nearest existing
 * directory above it is resolved and the rest of the path is kept.
 */
const realOrResolved = (file) => {
  const absolute = path.resolve(file);
  try {
    return realpathSync(absolute);
  } catch {
    const parent = path.dirname(absolute);
    return parent === absolute
      ? absolute
      : path.join(realOrResolved(parent), path.basename(absolute));
  }
};

/** Each `node_modules` directory at `dir` or above it. */
const nodeModulesAbove = (dir) => {
  const found = [];
  for (let current = dir; ; current = path.dirname(current)) {
    found.push(realOrResolved(path.join(current, "node_modules")));
    if (path.dirname(current) === current) return found;
  }
};

/**
 * The directory that the read scope of `root` starts from: the resolved
 * analysis root that fallow sends. The scope never widens to a parent
 * directory, also when that directory is a Git work tree.
 *
 * @param {string} root
 */
const readScopeBase = (root) => realOrResolved(root);

/**
 * A test of whether the checker may read a file for the project at `root`.
 *
 * A file may be read when its resolved path is inside {@link readScopeBase},
 * inside a `node_modules` directory at or above it, or inside one of
 * `compilerDirs`, and when it is a regular file of at most
 * {@link MAX_CHECKER_READ_BYTES} bytes. A path that is not absolute, such as
 * a bundled library file of the compiler, is left to the compiler.
 *
 * @param {string} root
 * @param {string[]} [compilerDirs] directories of the compiler and its library files
 * @returns {(fileName: string) => boolean}
 */
export const createReadScope = (root, compilerDirs = []) => {
  const base = readScopeBase(root);
  const allowed = [base, ...nodeModulesAbove(base), ...compilerDirs.map(realOrResolved)];
  return (fileName) => {
    if (!path.isAbsolute(fileName)) return true;
    const resolved = realOrResolved(fileName);
    if (!allowed.some((dir) => isWithin(dir, resolved))) return false;
    try {
      const stats = statSync(resolved);
      return stats.isFile() && stats.size <= MAX_CHECKER_READ_BYTES;
    } catch {
      // A missing file has no content to give.
      return true;
    }
  };
};

/**
 * True when `file` resolves inside the read scope base of `root` (see
 * {@link readScopeBase}), also through links. A missing file is compared by
 * its path.
 */
export const resolvesInside = (root, file) => isWithin(readScopeBase(root), realOrResolved(file));
