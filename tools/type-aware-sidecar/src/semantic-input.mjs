import { existsSync } from "node:fs";
import path from "node:path";

/** Source extensions accepted by the semantic backend, including declaration-file suffixes. */
export const MODULE_SOURCE_EXTENSIONS = [
  ".ts",
  ".tsx",
  ".mts",
  ".cts",
  ".js",
  ".jsx",
  ".mjs",
  ".cjs",
];

/** Whether the backend can parse the query's source without a component transform. */
export const isSupportedSemanticQuery = (query) =>
  !query.symbol ||
  MODULE_SOURCE_EXTENSIONS.includes(path.extname(query.symbol.absolutePath).toLowerCase());

/** Component files must stay out of openFiles because the backend cannot infer their script kind. */
export const semanticOpenFiles = (queries) =>
  queries
    .filter((query) => query.symbol && isSupportedSemanticQuery(query))
    .map((query) => query.symbol.absolutePath);

/**
 * True when the request named projects. Such a request gets only the named
 * projects that are left, and no fallback to another project.
 */
export const projectsNamed = (projects) => projects.named ?? projects.length > 0;

/** Resolve explicit projects or the conventional root project. */
export const semanticOpenProjects = (root, projects) => {
  if (projectsNamed(projects)) return projects.map((project) => project.absolutePath);
  const conventionalProject = path.join(root, "tsconfig.json");
  return existsSync(conventionalProject) ? [conventionalProject] : [];
};

/** The projects of the graph queries: the explicit ones, else each project with files. */
export const graphProjects = (snapshot, explicitProjects) =>
  (explicitProjects.length > 0 ? explicitProjects : snapshot.getProjects()).filter(
    (project) => project.program.getSourceFileNames().length > 0,
  );
