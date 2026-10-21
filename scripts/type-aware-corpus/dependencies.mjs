import { existsSync, realpathSync } from "node:fs";
import { dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");

export const DEPENDENCY_DIRECTORY_NAMES = new Set(["node_modules", ".pnpm-store"]);

const ancestorDirectories = (root) => {
  const directories = [];
  let ancestor = dirname(resolve(root));
  while (true) {
    directories.push(ancestor);
    const parent = dirname(ancestor);
    if (parent === ancestor) return directories;
    ancestor = parent;
  }
};

const containsInstalledDependencies = (root) =>
  [...DEPENDENCY_DIRECTORY_NAMES].some((name) => existsSync(resolve(root, name)));

// Fallow and the sidecar canonicalize the analysis root, so the corpus must
// judge dependency resolution from the same real path.
const realPath = (path) => {
  const lexical = resolve(path);
  let existing = lexical;
  while (!existsSync(existing)) existing = dirname(existing);
  return join(realpathSync.native(existing), relative(existing, lexical));
};

const realApprovedRoot = (approvedDependencyRoot) => realPath(approvedDependencyRoot);

const assertRealPathInside = (label, path, realApproved) => {
  const real = realPath(path);
  if (real.startsWith(`${realApproved}${sep}`)) return real;
  throw new Error(
    `${label} ${resolve(path)} resolves to ${real}, outside the approved dependency root ${realApproved}. TypeScript then cannot find the workspace type declarations. Use an --out-dir inside the workspace. Replace a symlinked target directory with a real directory, or disable the build cache wrapper that creates the symlink.`,
  );
};

/** Fail before any fixture is created when the artifact directory leaves the workspace. */
export const assertArtifactDirectoryInWorkspace = (outDir, approvedDependencyRoot = REPO_ROOT) => {
  assertRealPathInside("artifact directory", outDir, realApprovedRoot(approvedDependencyRoot));
};

export const fixtureDependencyEnvironment = (
  root,
  projectId,
  approvedDependencyRoot = REPO_ROOT,
) => {
  if (!existsSync(root)) {
    throw new Error(`${projectId} fixture is missing: ${root}; run prepare or supplemental first`);
  }
  const realApproved = realApprovedRoot(approvedDependencyRoot);
  const realRoot = assertRealPathInside(`${projectId} fixture path`, root, realApproved);
  const dependencyRoots = ancestorDirectories(realRoot).filter(containsInstalledDependencies);
  if (dependencyRoots.some((dependencyRoot) => dependencyRoot !== realApproved)) {
    throw new Error(
      `${projectId} resolves dependencies from an unapproved ancestor: ${dependencyRoots.join(", ")}`,
    );
  }
  if (dependencyRoots.length === 0) {
    throw new Error(
      `${projectId} finds no installed dependencies above ${realRoot}; run npm ci in ${realApproved} first`,
    );
  }
  return {
    dependency_installation: "ancestor-workspace",
    dependency_root: resolve(approvedDependencyRoot),
  };
};
