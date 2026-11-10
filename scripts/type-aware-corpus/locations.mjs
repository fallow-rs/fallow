import { isAbsolute, relative, resolve, sep } from "node:path";

const DEPENDENCY_PREFIX = "node_modules/";

const isInside = (base, path) => {
  const fromBase = relative(base, path);
  return fromBase !== ".." && !fromBase.startsWith(`..${sep}`) && !isAbsolute(fromBase);
};

/**
 * The recorded path of a contract declaration. A declaration in the prepared
 * fixture keeps its fixture-relative path. A class can also implement or
 * override a declaration from a type package, for example `@types/node` or
 * the TypeScript `lib.dom.d.ts`. The corpus resolves those from the pinned
 * workspace dependency root, so such a path is recorded as `node_modules/...`
 * relative to that root. Prepared fixtures never contain `node_modules`, so
 * the prefix cannot name a fixture file. Returns null for any other path.
 */
export const contractDeclarationPath = (fixtureRoot, dependencyRoot, path) => {
  const resolved = resolve(fixtureRoot, path);
  if (isInside(fixtureRoot, resolved)) return path;
  if (!isInside(resolve(dependencyRoot, "node_modules"), resolved)) return null;
  return relative(dependencyRoot, resolved).split(sep).join("/");
};

/**
 * The absolute source path of an evidence location, or null when the path
 * escapes the fixture. With a dependency root, a `node_modules/...` path
 * resolves inside the `node_modules` directory of that root.
 */
export const resolveEvidencePath = (fixtureRoot, path, dependencyRoot = null) => {
  if (dependencyRoot !== null && path.replaceAll("\\", "/").startsWith(DEPENDENCY_PREFIX)) {
    const resolved = resolve(dependencyRoot, path);
    return isInside(resolve(dependencyRoot, "node_modules"), resolved) ? resolved : null;
  }
  const resolved = resolve(fixtureRoot, path);
  return isInside(fixtureRoot, resolved) ? resolved : null;
};
