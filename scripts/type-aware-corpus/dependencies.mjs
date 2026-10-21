import { existsSync, realpathSync } from "node:fs";
import { dirname, resolve, sep } from "node:path";

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

export const fixtureDependencyEnvironment = (root, projectId, approvedDependencyRoot) => {
  const approved = resolve(approvedDependencyRoot);
  // Fallow and TypeScript resolve dependencies from the real fixture path. A
  // symlinked artifact directory can move that path away from the workspace,
  // and every semantic query then abstains on missing type declarations.
  const realRoot = realpathSync(root);
  const realApproved = existsSync(approved) ? realpathSync(approved) : approved;
  const dependencyRoots = ancestorDirectories(realRoot).filter(containsInstalledDependencies);
  if (dependencyRoots.some((dependencyRoot) => dependencyRoot !== realApproved)) {
    throw new Error(
      `${projectId} resolves dependencies from an unapproved ancestor: ${dependencyRoots.join(", ")}`,
    );
  }
  if (!realRoot.startsWith(`${realApproved}${sep}`)) {
    throw new Error(
      `${projectId} fixture real path is outside the approved dependency root: ${realRoot}; use a real artifact directory inside ${realApproved}`,
    );
  }
  if (dependencyRoots.length === 0) {
    return { dependency_installation: "none", dependency_root: null };
  }
  return {
    dependency_installation: "ancestor-workspace",
    dependency_root: approved,
  };
};
