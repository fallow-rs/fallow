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

/**
 * A regular expression for the source files of a wildcard package target.
 * Every `*` of a Node subpath pattern stands for the same substring, and that
 * substring can contain `/`: `./src/*.ts` matches `src/nested/deep.ts`. A
 * target with its first `*` at the package root, such as `./*.js`, keeps a
 * capture of one path segment, so it does not make every test and config file
 * of the package an entry.
 */
export const wildcardPattern = (target) => {
  const normalized = target.replace(/^\.\//u, "").replace(/^dist\//u, "src/");
  const sourceTarget = normalized.replace(/(?:\.d)?\.[cm]?[jt]sx?$/u, ".ts");
  const capture = sourceTarget.startsWith("*") ? "([^/]+)" : "(.+)";
  const escaped = sourceTarget
    .split("*")
    .map((part) => part.replace(/[\\^$.*+?()[\]{}|]/gu, "\\$&"))
    .map((part, index) => (index === 0 ? part : `${index === 1 ? capture : "\\1"}${part}`))
    .join("");
  return new RegExp(`^${escaped}$`, "u");
};
