import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";

import { canonicalFileIdentity } from "./file-identity.mjs";
import { relativePath } from "./semantic-identity.mjs";

const INFERRED_PROJECT = "<inferred>";
const compareText = (left, right) => Buffer.compare(Buffer.from(left), Buffer.from(right));
const slash = (value) => value.split(path.sep).join("/");

const blockingDiagnosticCount = (project) =>
  project.program.getConfigFileParsingDiagnostics().length +
  project.program.getProgramDiagnostics().length +
  project.program.getSyntacticDiagnostics().length +
  project.program.getBindDiagnostics().length;

const configPath = (root, project) => {
  const normalized = slash(project.configFileName);
  if (normalized.endsWith("/dev/null/inferred")) return INFERRED_PROJECT;
  return relativePath(root, project.configFileName) || path.basename(project.configFileName);
};

const normalizedConfigValue = (root, value) => {
  if (Array.isArray(value)) return value.map((item) => normalizedConfigValue(root, item));
  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value)
        .filter(([, item]) => item !== undefined)
        .toSorted(([left], [right]) => compareText(left, right))
        .map(([key, item]) => [key, normalizedConfigValue(root, item)]),
    );
  }
  if (typeof value !== "string" || !path.isAbsolute(value)) return value;
  const relative = path.relative(root, value);
  return relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative)
    ? slash(value)
    : slash(relative || ".");
};

const stripJsonComments = (text) => {
  let result = "";
  let inString = false;
  let escaped = false;
  for (let index = 0; index < text.length; index += 1) {
    const current = text[index];
    const next = text[index + 1];
    if (inString) {
      result += current;
      if (escaped) escaped = false;
      else if (current === "\\") escaped = true;
      else if (current === '"') inString = false;
      continue;
    }
    if (current === '"') {
      inString = true;
      result += current;
      continue;
    }
    if (current === "/" && next === "/") {
      result += "  ";
      index += 2;
      while (index < text.length && text[index] !== "\n") {
        result += " ";
        index += 1;
      }
      if (index < text.length) result += "\n";
      continue;
    }
    if (current === "/" && next === "*") {
      result += "  ";
      index += 2;
      while (index < text.length && !(text[index] === "*" && text[index + 1] === "/")) {
        result += text[index] === "\n" ? "\n" : " ";
        index += 1;
      }
      if (index < text.length) {
        result += "  ";
        index += 1;
      }
      continue;
    }
    result += current;
  }
  return result;
};

const stripTrailingCommas = (text) => {
  let result = "";
  let inString = false;
  let escaped = false;
  for (let index = 0; index < text.length; index += 1) {
    const current = text[index];
    if (inString) {
      result += current;
      if (escaped) escaped = false;
      else if (current === "\\") escaped = true;
      else if (current === '"') inString = false;
      continue;
    }
    if (current === '"') {
      inString = true;
      result += current;
      continue;
    }
    if (current === ",") {
      let lookahead = index + 1;
      while (/\s/u.test(text[lookahead] ?? "")) lookahead += 1;
      if (text[lookahead] === "}" || text[lookahead] === "]") continue;
    }
    result += current;
  }
  return result;
};

const readConfigDocument = (fileName) => {
  try {
    const text = readFileSync(fileName, "utf8");
    return JSON.parse(stripTrailingCommas(stripJsonComments(text)));
  } catch {
    return undefined;
  }
};

const resolveConfigDocument = (configFileName, specifier) => {
  if (typeof specifier !== "string") return undefined;
  if (!path.isAbsolute(specifier) && !specifier.startsWith(".")) return undefined;
  const target = path.resolve(path.dirname(configFileName), specifier);
  const candidates = path.extname(target)
    ? [target]
    : [path.join(target, "tsconfig.json"), `${target}.json`, target];
  return candidates.find((candidate) => existsSync(candidate) && readConfigDocument(candidate));
};

const configDocumentClosure = (root, configFileName, seen = new Set()) => {
  const document = readConfigDocument(configFileName);
  if (!document) return null;
  const canonical = canonicalFileIdentity(configFileName);
  if (seen.has(canonical)) {
    return { config: relativePath(root, canonical), cycle: true };
  }
  const nextSeen = new Set(seen).add(canonical);
  const extensions = Array.isArray(document.extends) ? document.extends : [document.extends];
  const referenced = Array.isArray(document.references) ? document.references : [];
  const closure = (specifiers) =>
    specifiers
      .map((specifier) => resolveConfigDocument(configFileName, specifier))
      .filter(Boolean)
      .map((resolved) => configDocumentClosure(root, resolved, nextSeen))
      .filter(Boolean)
      .toSorted((left, right) => compareText(left.config, right.config));
  return {
    config: relativePath(root, canonical),
    document: normalizedConfigValue(root, document),
    extends: closure(extensions),
    references: closure(referenced.map((reference) => reference?.path)),
  };
};

const effectiveProjectConfigHash = (root, project) => {
  const effective = {
    config: configPath(root, project),
    compiler_options: normalizedConfigValue(root, project.compilerOptions),
    root_files: project.rootFiles
      .map((fileName) => normalizedConfigValue(root, fileName))
      .toSorted(compareText),
    config_document: configDocumentClosure(root, project.configFileName),
  };
  return `sha256:${createHash("sha256").update(JSON.stringify(effective)).digest("hex")}`;
};

export const projectState = (root, project, source) => {
  const diagnosticCount = blockingDiagnosticCount(project);
  return {
    project,
    config: configPath(root, project),
    effective_config_hash: effectiveProjectConfigHash(root, project),
    source,
    status: diagnosticCount === 0 ? "complete" : "unavailable",
    reason_code: diagnosticCount === 0 ? null : "blocking-diagnostics",
    blocking_diagnostic_count: diagnosticCount,
    source_file_count: project.program.getSourceFileNames().length,
    program_reused: false,
    candidate_count: 0,
    confirmed_used_count: 0,
    contract_preserved_count: 0,
    no_static_references_count: 0,
    fix_eligible_count: 0,
    unresolved_count: 0,
    abstained_count: 0,
  };
};

export const projectResult = ({ project: _project, ...state }) => state;
