import { existsSync, readFileSync } from "node:fs";
import path from "node:path";

import { projectCompilerOptions, projectConfigFileName } from "./backend.mjs";

const pathWithin = (root, candidate) =>
  candidate === root || candidate.startsWith(`${root}${path.sep}`);

const packageAt = (directory) => {
  const packageFile = path.join(directory, "package.json");
  if (!existsSync(packageFile)) return undefined;
  try {
    return { root: directory, json: JSON.parse(readFileSync(packageFile, "utf8")) };
  } catch {
    return { root: directory, json: {} };
  }
};

/** The nearest package.json at or above the project config, inside `root`. */
export const projectPackage = (root, project) => {
  const normalizedRoot = path.resolve(root);
  let current = path.dirname(projectConfigFileName(project));
  while (pathWithin(normalizedRoot, current)) {
    const found = packageAt(current);
    if (found) return found;
    if (current === normalizedRoot) break;
    current = path.dirname(current);
  }
  return { root: normalizedRoot, json: {} };
};

/** The `compilerOptions.paths` targets that map the package name. */
export const packagePathTargets = (project, packageJson) => {
  const packageName = packageJson.name;
  if (typeof packageName !== "string") return [];
  return projectCompilerOptions(project).paths?.[packageName] ?? [];
};
