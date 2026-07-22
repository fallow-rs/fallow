import path from "node:path";

import { version as typescriptVersion } from "typescript";

const PROTOCOL_VERSION = 2;
const OPERATION = "class-member-uses";
const SIDECAR_VERSION = "0.1.0";
const BACKEND = "typescript-go";
const BACKEND_VERSION = typescriptVersion;

const CANDIDATE_KINDS = new Set(["class_method", "class_property"]);
const REQUEST_KEYS = new Set(["protocol_version", "operation", "root", "projects", "candidates"]);
const CANDIDATE_KEYS = new Set(["id", "path", "parent_name", "member_name", "kind", "line", "col"]);
const MAX_WARNINGS = 20;
const MAX_WARNING_CHARS = 512;
const MAX_PROJECTS = 256;
const MAX_CANDIDATES = 25_000;
const MAX_STRING_CHARS = 4_096;

const isObject = (value) => typeof value === "object" && value !== null && !Array.isArray(value);
const compareText = (left, right) => Buffer.compare(Buffer.from(left), Buffer.from(right));

const requireString = (value, field) => {
  if (typeof value !== "string" || value.length === 0) {
    throw new Error(`${field} must be a non-empty string`);
  }
  if ([...value].length > MAX_STRING_CHARS) {
    throw new Error(`${field} exceeds the ${MAX_STRING_CHARS} character limit`);
  }
  return value;
};

const requireInteger = (value, field, minimum) => {
  if (!Number.isSafeInteger(value) || value < minimum) {
    throw new Error(`${field} must be an integer greater than or equal to ${minimum}`);
  }
  return value;
};

const requireExactKeys = (value, allowed, field) => {
  const unexpected = Object.keys(value).filter((key) => !allowed.has(key));
  if (unexpected.length > 0) {
    throw new Error(`${field} contains unknown field ${unexpected.toSorted().join(", ")}`);
  }
};

const requireObject = (value, field) => {
  if (!isObject(value)) {
    throw new Error(`${field} must be a JSON object`);
  }
  return value;
};

const requireLiteral = (value, expected, field) => {
  if (value !== expected) {
    throw new Error(`unsupported ${field} ${String(value)}`);
  }
};

const requireArray = (value, field) => {
  if (!Array.isArray(value)) {
    throw new Error(`${field} must be an array`);
  }
  return value;
};

const requireBoundedArray = (value, field, maximum) => {
  const array = requireArray(value, field);
  if (array.length > maximum) {
    throw new Error(`${field} exceeds the ${maximum} item limit`);
  }
  return array;
};

const isWithinRoot = (root, file) => {
  const relative = path.relative(root, file);
  return relative === "" || (!relative.startsWith(`..${path.sep}`) && relative !== "..");
};

const parseCandidatePath = (value, field, root) => {
  const candidatePath = requireString(value, field);
  if (path.isAbsolute(candidatePath)) {
    throw new Error(`${field} must be project-relative`);
  }
  const absolutePath = path.resolve(root, candidatePath);
  if (!isWithinRoot(root, absolutePath)) {
    throw new Error(`${field} must resolve within root`);
  }
  return { candidatePath, absolutePath };
};

const parseCandidateKind = (value, field) => {
  const kind = requireString(value, field);
  if (!CANDIDATE_KINDS.has(kind)) {
    throw new Error(`${field} must be class_method or class_property`);
  }
  return kind;
};

const parseCandidate = (value, index, root) => {
  const field = `candidates[${index}]`;
  requireObject(value, field);
  requireExactKeys(value, CANDIDATE_KEYS, field);
  const { candidatePath, absolutePath } = parseCandidatePath(value.path, `${field}.path`, root);
  const kind = parseCandidateKind(value.kind, `${field}.kind`);

  return {
    id: requireInteger(value.id, `${field}.id`, 0),
    path: candidatePath,
    absolutePath,
    parentName: requireString(value.parent_name, `${field}.parent_name`),
    memberName: requireString(value.member_name, `${field}.member_name`),
    kind,
    line: requireInteger(value.line, `${field}.line`, 1),
    col: requireInteger(value.col, `${field}.col`, 0),
  };
};

const parseRoot = (value) => {
  const root = requireString(value, "root");
  if (!path.isAbsolute(root)) {
    throw new Error("root must be an absolute path");
  }
  return path.resolve(root);
};

const requireUniqueCandidateIds = (candidates) => {
  const candidateIds = new Set();
  for (const candidate of candidates) {
    if (candidateIds.has(candidate.id)) {
      throw new Error(`duplicate candidate id ${candidate.id}`);
    }
    candidateIds.add(candidate.id);
  }
};

const requireUniqueProjects = (projects) => {
  const projectPaths = new Set();
  for (const project of projects) {
    if (projectPaths.has(project.absolutePath)) {
      throw new Error(`duplicate project path ${project.path}`);
    }
    projectPaths.add(project.absolutePath);
  }
};

export const parseRequest = (value) => {
  requireObject(value, "request");
  requireExactKeys(value, REQUEST_KEYS, "request");
  requireLiteral(value.protocol_version, PROTOCOL_VERSION, "protocol_version");
  requireLiteral(value.operation, OPERATION, "operation");
  const root = parseRoot(value.root);
  const projects = requireBoundedArray(value.projects, "projects", MAX_PROJECTS).map(
    (project, index) => {
      const projectPath = requireString(project, `projects[${index}]`);
      return {
        path: projectPath,
        absolutePath: path.resolve(root, projectPath),
      };
    },
  );
  requireUniqueProjects(projects);
  const candidates = requireBoundedArray(value.candidates, "candidates", MAX_CANDIDATES).map(
    (candidate, index) => parseCandidate(candidate, index, root),
  );
  requireUniqueCandidateIds(candidates);

  return { root, projects, candidates };
};

export const createResponse = ({
  selectedTsconfigs,
  confirmedIds,
  unresolvedIds,
  abstentions = [],
  projectResults = [],
  phaseTimings = { project_setup: 0, diagnostics: 0, symbol_scan: 0 },
  warnings,
  elapsedMs,
}) => ({
  protocol_version: PROTOCOL_VERSION,
  sidecar_version: SIDECAR_VERSION,
  backend: BACKEND,
  backend_version: BACKEND_VERSION,
  selected_tsconfigs: [...selectedTsconfigs].toSorted(compareText),
  confirmed_used_candidate_ids: [...confirmedIds].toSorted((left, right) => left - right),
  unresolved_candidate_ids: [...unresolvedIds].toSorted((left, right) => left - right),
  abstentions: [...abstentions].toSorted((left, right) => left.candidate_id - right.candidate_id),
  projects: [...projectResults].toSorted(
    (left, right) =>
      compareText(left.config, right.config) || compareText(left.source, right.source),
  ),
  phase_timings_ms: Object.fromEntries(
    Object.entries(phaseTimings).map(([name, duration]) => [
      name,
      Math.max(0, Math.round(duration)),
    ]),
  ),
  warnings: [
    ...new Set(
      [...warnings]
        .map((warning) =>
          [...warning.replace(/\s+/g, " ").trim()].slice(0, MAX_WARNING_CHARS).join(""),
        )
        .filter(Boolean),
    ),
  ]
    .toSorted(compareText)
    .slice(0, MAX_WARNINGS),
  elapsed_ms: Math.max(0, Math.round(elapsedMs)),
});
