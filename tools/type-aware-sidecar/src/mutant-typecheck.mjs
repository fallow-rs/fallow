// Type-checks mutated source files for `fallow mutation --typecheck`.
//
// The check applies one mutant at a time to an in-memory copy of its file and
// compares the error diagnostics of that file with the diagnostics of the
// original file. A new error makes the mutant `compile-error`. The check reads
// only the mutated file, so a mutant that breaks only another file stays
// `compiles`. The project files on disk never change.

import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import path from "node:path";

import { API, DiagnosticCategory } from "typescript/unstable/sync";

import { canonicalFileIdentity } from "./file-identity.mjs";

const JAVASCRIPT_EXTENSIONS = new Set([".js", ".jsx", ".mjs", ".cjs"]);
const LEADING_COMMENTS = /^(?:\s|\/\/[^\n]*|\/\*[\s\S]*?\*\/)*/u;
const NO_CHECK_PRAGMA = /@ts-nocheck\b/u;
const MAX_CODES = 8;

const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");

const isInferredProject = (project) =>
  project.configFileName.split(path.sep).join("/").endsWith("/dev/null/inferred");

const hasNoCheckPragma = (text) => NO_CHECK_PRAGMA.test(LEADING_COMMENTS.exec(text)?.[0] ?? "");

const isChecked = (project, absolutePath, text) => {
  if (hasNoCheckPragma(text)) return false;
  if (!JAVASCRIPT_EXTENSIONS.has(path.extname(absolutePath).toLowerCase())) return true;
  return project.compilerOptions?.checkJs === true;
};

const errorKey = (diagnostic) => `${diagnostic.code}\u0000${diagnostic.text}`;

const fileErrors = (project, absolutePath) =>
  [
    ...project.program.getSyntacticDiagnostics(absolutePath),
    ...project.program.getSemanticDiagnostics(absolutePath),
  ].filter((diagnostic) => diagnostic.category === DiagnosticCategory.Error);

const countKeys = (diagnostics) => {
  const counts = new Map();
  for (const diagnostic of diagnostics) {
    const key = errorKey(diagnostic);
    counts.set(key, (counts.get(key) ?? 0) + 1);
  }
  return counts;
};

// The codes of the errors that the mutated file has and the original file
// does not have. Positions move with the mutation, so the comparison uses the
// code and the message text.
const newErrorCodes = (baseline, diagnostics) => {
  const remaining = new Map(baseline);
  const codes = new Set();
  for (const diagnostic of diagnostics) {
    const key = errorKey(diagnostic);
    const count = remaining.get(key) ?? 0;
    if (count > 0) remaining.set(key, count - 1);
    else codes.add(diagnostic.code);
  }
  return [...codes].toSorted((left, right) => left - right).slice(0, MAX_CODES);
};

const mutatedText = (original, mutant) =>
  Buffer.concat([
    original.subarray(0, mutant.start),
    Buffer.from(mutant.replacement, "utf8"),
    original.subarray(mutant.end),
  ]).toString("utf8");

const unchecked = (file, reason) =>
  file.mutants.map((mutant) => ({ id: mutant.id, outcome: "unchecked", reason }));

const readOriginal = (file) => {
  try {
    return readFileSync(file.absolutePath);
  } catch {
    return null;
  }
};

const defaultProjectConfig = (api, absolutePath) => {
  const snapshot = api.updateSnapshot({ openFiles: [absolutePath] });
  try {
    const project = snapshot.getDefaultProjectForFile(absolutePath);
    return project && !isInferredProject(project) ? project.configFileName : null;
  } finally {
    api.updateSnapshot({ closeFiles: [absolutePath] }).dispose();
    snapshot.dispose();
  }
};

const createOverlayApi = (root, createApi) => {
  const overlay = new Map();
  const api = createApi({
    cwd: root,
    fs: {
      readFile: (fileName) =>
        overlay.size === 0 ? undefined : overlay.get(canonicalFileIdentity(fileName)),
    },
  });
  return { api, overlay };
};

const sourceSkipReason = (file, original) => {
  if (original === null) return "file-missing";
  const changed = file.sourceSha256 !== null && sha256(original) !== file.sourceSha256;
  return changed ? "source-changed" : null;
};

const projectSkipReason = (project, file, original) => {
  if (!project?.program.getSourceFile(file.absolutePath)) return "no-project";
  return isChecked(project, file.absolutePath, original.toString("utf8"))
    ? null
    : "not-type-checked";
};

const checkMutant = (context, check, mutant) => {
  context.overlay.set(check.identity, mutatedText(check.original, mutant));
  const next = context.api.updateSnapshot(check.changes);
  check.snapshot.dispose();
  check.snapshot = next;
  const errors = fileErrors(next.getProject(check.config), check.file.absolutePath);
  const codes = newErrorCodes(check.baseline, errors);
  return codes.length > 0
    ? { id: mutant.id, outcome: "compile-error", codes }
    : { id: mutant.id, outcome: "compiles" };
};

const checkInProject = (context, file, original, config) => {
  const check = {
    file,
    original,
    config,
    identity: canonicalFileIdentity(file.absolutePath),
    changes: { fileChanges: { changed: [file.absolutePath] } },
    snapshot: context.api.updateSnapshot({ openProjects: [config] }),
    baseline: null,
  };
  try {
    const project = check.snapshot.getProject(config);
    const reason = projectSkipReason(project, file, original);
    if (reason !== null) return unchecked(file, reason);
    check.baseline = countKeys(fileErrors(project, file.absolutePath));
    return file.mutants.map((mutant) => checkMutant(context, check, mutant));
  } finally {
    context.overlay.delete(check.identity);
    context.api.updateSnapshot({ ...check.changes, closeProjects: [config] }).dispose();
    check.snapshot.dispose();
  }
};

const checkFile = (context, file) => {
  const original = readOriginal(file);
  const reason = sourceSkipReason(file, original);
  if (reason !== null) return unchecked(file, reason);
  const config = defaultProjectConfig(context.api, file.absolutePath);
  if (config === null) return unchecked(file, "no-project");
  return checkInProject(context, file, original, config);
};

/** Type-checks each mutant of a parsed `mutant-typecheck` request. */
export const checkMutants = (
  { root, files },
  { createApi = (options) => new API(options) } = {},
) => {
  if (files.length === 0) return [];
  const context = createOverlayApi(root, createApi);
  try {
    return files.flatMap((file) => checkFile(context, file));
  } finally {
    context.api.close();
  }
};
