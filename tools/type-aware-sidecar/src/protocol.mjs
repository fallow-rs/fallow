import { createRequire } from "node:module";
import path from "node:path";

import { version as typescriptVersion } from "typescript";

import {
  ANALYSIS_OPERATION,
  BACKEND_FAMILY,
  BACKEND_VERSION,
  QUERY_OPERATIONS as MANIFEST_QUERY_OPERATIONS,
  WIRE_PROTOCOL_VERSION,
} from "./generated-protocol.mjs";
import { normalizePhaseTimings, normalizeSemanticResult } from "./response-normalization.mjs";

const require = createRequire(import.meta.url);
const { version: SIDECAR_VERSION } = require("../package.json");
const QUERY_OPERATIONS = new Set(MANIFEST_QUERY_OPERATIONS);

if (typescriptVersion !== BACKEND_VERSION) {
  throw new Error(
    `typescript backend version ${typescriptVersion} does not match protocol ${BACKEND_VERSION}`,
  );
}

export const createStatusResponse = () => ({
  package_version: SIDECAR_VERSION,
  protocol_version: WIRE_PROTOCOL_VERSION,
  backend_family: BACKEND_FAMILY,
  backend_version: BACKEND_VERSION,
});

const BATCH_REQUEST_KEYS = new Set([
  "protocol_version",
  "operation",
  "root",
  "projects",
  "queries",
  "evidence_limit",
]);
const SYMBOL_QUERY_KEYS = new Set(["id", "operation", "symbol"]);
const SYMBOL_USE_QUERY_KEYS = new Set(["id", "operation", "symbol", "framework_contracts"]);
const FRAMEWORK_CONTRACT_KEYS = new Set([
  "framework",
  "package",
  "heritage_symbol",
  "heritage_names",
  "relation",
  "members",
]);
const API_SURFACE_QUERY_KEYS = new Set([
  "id",
  "operation",
  "entry_points",
  "include_cycles",
  "private_leak_candidates",
]);
const TYPE_COUPLING_QUERY_KEYS = new Set(["id", "operation", "entry_points", "include_cycles"]);
const PRIVATE_LEAK_CANDIDATE_KEYS = new Set(["id", "path", "export_name", "type_name"]);
const SYMBOL_KEYS = new Set([
  "path",
  "namespace",
  "declaration_kind",
  "exported_name",
  "local_name",
  "line",
  "col",
  "owner",
]);
const SYMBOL_OPERATIONS = new Set(["symbol-use", "symbol-trace", "symbol-impact"]);
const SYMBOL_NAMESPACES = new Set(["value", "type"]);
const MAX_WARNINGS = 20;
const MAX_WARNING_CHARS = 512;
const MAX_PROJECTS = 256;
const MAX_QUERIES = 25_000;
const MAX_GRAPH_QUERIES = 256;
const MAX_PRIVATE_LEAK_CANDIDATES = 25_000;
const MAX_EVIDENCE_PER_RESULT = 40;
const MAX_STRING_CHARS = 4_096;

const isObject = (value) => typeof value === "object" && value !== null && !Array.isArray(value);
const compareText = (left, right) => Buffer.compare(Buffer.from(left), Buffer.from(right));
const normalizeWarnings = (warnings) =>
  [
    ...new Set(
      warnings
        .map((warning) =>
          [...warning.replace(/\s+/g, " ").trim()].slice(0, MAX_WARNING_CHARS).join(""),
        )
        .filter(Boolean),
    ),
  ]
    .toSorted(compareText)
    .slice(0, MAX_WARNINGS);

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

const requireBoolean = (value, field) => {
  if (typeof value !== "boolean") {
    throw new Error(`${field} must be a boolean`);
  }
  return value;
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

const parseSymbolIdentity = (value, field, root) => {
  requireObject(value, field);
  requireExactKeys(value, SYMBOL_KEYS, field);
  const { candidatePath, absolutePath } = parseCandidatePath(value.path, `${field}.path`, root);
  const namespace = requireString(value.namespace, `${field}.namespace`);
  if (!SYMBOL_NAMESPACES.has(namespace)) {
    throw new Error(`${field}.namespace must be value or type`);
  }
  const owner = value.owner === undefined ? null : requireString(value.owner, `${field}.owner`);
  return {
    path: candidatePath,
    absolutePath,
    namespace,
    declarationKind: requireString(value.declaration_kind, `${field}.declaration_kind`),
    exportedName: requireString(value.exported_name, `${field}.exported_name`),
    localName: requireString(value.local_name, `${field}.local_name`),
    line: requireInteger(value.line, `${field}.line`, 1),
    col: requireInteger(value.col, `${field}.col`, 0),
    owner,
  };
};

const parseEntryPoints = (value, field, root) =>
  requireBoundedArray(value ?? [], field, MAX_PROJECTS).map((entryPoint, index) =>
    parseCandidatePath(entryPoint, `${field}[${index}]`, root),
  );

const parsePrivateLeakCandidates = (value, field, root) => {
  const candidates = requireBoundedArray(value ?? [], field, MAX_PRIVATE_LEAK_CANDIDATES).map(
    (candidate, index) => {
      const candidateField = `${field}[${index}]`;
      requireObject(candidate, candidateField);
      requireExactKeys(candidate, PRIVATE_LEAK_CANDIDATE_KEYS, candidateField);
      const { candidatePath, absolutePath } = parseCandidatePath(
        candidate.path,
        `${candidateField}.path`,
        root,
      );
      return {
        id: requireInteger(candidate.id, `${candidateField}.id`, 0),
        path: candidatePath,
        absolutePath,
        exportName: requireString(candidate.export_name, `${candidateField}.export_name`),
        typeName: requireString(candidate.type_name, `${candidateField}.type_name`),
      };
    },
  );
  requireUniqueCandidateIds(candidates);
  return candidates;
};

const queryKeys = (operation) => {
  if (operation === "symbol-use") return SYMBOL_USE_QUERY_KEYS;
  if (SYMBOL_OPERATIONS.has(operation)) return SYMBOL_QUERY_KEYS;
  return operation === "api-surface" ? API_SURFACE_QUERY_KEYS : TYPE_COUPLING_QUERY_KEYS;
};

const parseFrameworkContracts = (value, field) =>
  requireBoundedArray(value ?? [], field, MAX_PROJECTS).map((contract, index) => {
    const contractField = `${field}[${index}]`;
    requireObject(contract, contractField);
    requireExactKeys(contract, FRAMEWORK_CONTRACT_KEYS, contractField);
    const relation = requireString(contract.relation, `${contractField}.relation`);
    if (relation !== "extends" && relation !== "implements") {
      throw new Error(`${contractField}.relation must be extends or implements`);
    }
    return {
      framework: requireString(contract.framework, `${contractField}.framework`),
      package: requireString(contract.package, `${contractField}.package`),
      heritageSymbol: requireString(contract.heritage_symbol, `${contractField}.heritage_symbol`),
      heritageNames: requireBoundedArray(
        contract.heritage_names,
        `${contractField}.heritage_names`,
        MAX_PROJECTS,
      ).map((name, nameIndex) =>
        requireString(name, `${contractField}.heritage_names[${nameIndex}]`),
      ),
      relation,
      members: requireBoundedArray(contract.members, `${contractField}.members`, MAX_PROJECTS).map(
        (member, memberIndex) => requireString(member, `${contractField}.members[${memberIndex}]`),
      ),
    };
  });

const parseIncludeCycles = (value, field) =>
  value === undefined ? false : requireBoolean(value, field);

const parseGraphQuery = (value, field, root, query) => {
  const graphQuery = {
    ...query,
    entryPoints: parseEntryPoints(value.entry_points, `${field}.entry_points`, root),
    includeCycles: parseIncludeCycles(value.include_cycles, `${field}.include_cycles`),
  };
  if (query.operation !== "api-surface") return graphQuery;
  return {
    ...graphQuery,
    privateLeakCandidates: parsePrivateLeakCandidates(
      value.private_leak_candidates,
      `${field}.private_leak_candidates`,
      root,
    ),
  };
};

const parseQuery = (value, index, root) => {
  const field = `queries[${index}]`;
  requireObject(value, field);
  const operation = requireString(value.operation, `${field}.operation`);
  if (!QUERY_OPERATIONS.has(operation)) {
    throw new Error(`unsupported ${field}.operation ${operation}`);
  }
  requireExactKeys(value, queryKeys(operation), field);
  const query = {
    id: requireInteger(value.id, `${field}.id`, 0),
    operation,
  };
  if (SYMBOL_OPERATIONS.has(operation)) {
    return {
      ...query,
      symbol: parseSymbolIdentity(value.symbol, `${field}.symbol`, root),
      ...(operation === "symbol-use"
        ? {
            frameworkContracts: parseFrameworkContracts(
              value.framework_contracts,
              `${field}.framework_contracts`,
            ),
          }
        : {}),
    };
  }
  return parseGraphQuery(value, field, root, query);
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

const parseProjects = (value, root) => {
  const projects = requireBoundedArray(value, "projects", MAX_PROJECTS).map((project, index) => {
    const projectPath = requireString(project, `projects[${index}]`);
    return {
      path: projectPath,
      absolutePath: path.resolve(root, projectPath),
    };
  });
  requireUniqueProjects(projects);
  return projects;
};

const validateGraphQueryCount = (queries) => {
  const count = queries.filter(
    (query) => query.operation === "api-surface" || query.operation === "type-coupling",
  ).length;
  if (count > MAX_GRAPH_QUERIES) {
    throw new Error(`graph queries exceed the ${MAX_GRAPH_QUERIES} item limit`);
  }
};

const parseEvidenceLimit = (value) => {
  const limit =
    value === undefined ? MAX_EVIDENCE_PER_RESULT : requireInteger(value, "evidence_limit", 1);
  if (limit > MAX_EVIDENCE_PER_RESULT) {
    throw new Error(`evidence_limit exceeds the ${MAX_EVIDENCE_PER_RESULT} item limit`);
  }
  return limit;
};

const parseBatchRequest = (value) => {
  requireObject(value, "request");
  requireExactKeys(value, BATCH_REQUEST_KEYS, "request");
  requireLiteral(value.protocol_version, WIRE_PROTOCOL_VERSION, "protocol_version");
  requireLiteral(value.operation, ANALYSIS_OPERATION, "operation");
  const root = parseRoot(value.root);
  const projects = parseProjects(value.projects, root);
  const queries = requireBoundedArray(value.queries, "queries", MAX_QUERIES).map((query, index) =>
    parseQuery(query, index, root),
  );
  validateGraphQueryCount(queries);
  requireUniqueCandidateIds(queries);
  const evidenceLimit = parseEvidenceLimit(value.evidence_limit);
  return { protocolVersion: WIRE_PROTOCOL_VERSION, root, projects, queries, evidenceLimit };
};

export const parseRequest = (value) => {
  requireObject(value, "request");
  if (value.protocol_version === WIRE_PROTOCOL_VERSION) {
    return parseBatchRequest(value);
  }
  throw new Error(`unsupported protocol_version ${String(value.protocol_version)}`);
};

export const createSemanticResponse = ({
  selectedTsconfigs,
  projectResults,
  results,
  phaseTimings,
  warnings,
  elapsedMs,
}) => ({
  protocol_version: WIRE_PROTOCOL_VERSION,
  operation: ANALYSIS_OPERATION,
  sidecar_version: SIDECAR_VERSION,
  backend: BACKEND_FAMILY,
  backend_version: BACKEND_VERSION,
  selected_tsconfigs: [...selectedTsconfigs].toSorted(compareText),
  projects: [...projectResults].toSorted((left, right) => compareText(left.config, right.config)),
  results: [...results]
    .map(normalizeSemanticResult)
    .toSorted((left, right) => left.query_id - right.query_id),
  phase_timings_ms: normalizePhaseTimings(phaseTimings),
  warnings: normalizeWarnings(warnings),
  elapsed_ms: Math.max(0, Math.round(elapsedMs)),
});
