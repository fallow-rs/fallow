import path from "node:path";

import { version as typescriptVersion } from "typescript";

const PROTOCOL_VERSION = 1;
const OPERATION = "class-member-uses";
const BACKEND = "typescript-go";
const BACKEND_VERSION = typescriptVersion;

const CANDIDATE_KINDS = new Set(["class_method", "class_property"]);
const REQUEST_KEYS = new Set(["protocol_version", "operation", "root", "candidates"]);
const CANDIDATE_KEYS = new Set(["id", "path", "parent_name", "member_name", "kind", "line", "col"]);
const MAX_WARNINGS = 20;
const MAX_WARNING_CHARS = 512;

const isObject = (value) => typeof value === "object" && value !== null && !Array.isArray(value);

const requireString = (value, field) => {
  if (typeof value !== "string" || value.length === 0) {
    throw new Error(`${field} must be a non-empty string`);
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

export const parseRequest = (value) => {
  requireObject(value, "request");
  requireExactKeys(value, REQUEST_KEYS, "request");
  requireLiteral(value.protocol_version, PROTOCOL_VERSION, "protocol_version");
  requireLiteral(value.operation, OPERATION, "operation");
  const root = parseRoot(value.root);
  const candidates = requireArray(value.candidates, "candidates").map((candidate, index) =>
    parseCandidate(candidate, index, root),
  );
  requireUniqueCandidateIds(candidates);

  return { root, candidates };
};

export const createResponse = ({
  selectedTsconfigs,
  confirmedIds,
  unresolvedIds,
  warnings,
  elapsedMs,
}) => ({
  protocol_version: PROTOCOL_VERSION,
  backend: BACKEND,
  backend_version: BACKEND_VERSION,
  selected_tsconfigs: [...selectedTsconfigs].toSorted(),
  confirmed_used_candidate_ids: [...confirmedIds].toSorted((left, right) => left - right),
  unresolved_candidate_ids: [...unresolvedIds].toSorted((left, right) => left - right),
  warnings: [...warnings]
    .map((warning) => [...warning.replace(/\s+/g, " ").trim()].slice(0, MAX_WARNING_CHARS).join(""))
    .filter(Boolean)
    .toSorted()
    .slice(0, MAX_WARNINGS),
  elapsed_ms: Math.max(0, Math.round(elapsedMs)),
});
