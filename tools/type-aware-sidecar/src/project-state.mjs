import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";

import { SymbolFlags } from "typescript/unstable/sync";
import {
  isExportDeclaration,
  isImportDeclaration,
  isNamedExports,
  isNamedImports,
} from "typescript/unstable/ast/is";

import { canonicalFileIdentity } from "./file-identity.mjs";
import { projectSourceFiles, relativePath } from "./semantic-identity.mjs";

const INFERRED_PROJECT = "<inferred>";
const compareText = (left, right) => Buffer.compare(Buffer.from(left), Buffer.from(right));
const slash = (value) => value.split(path.sep).join("/");

const structuralDiagnostics = (project) => [
  ...project.program.getConfigFileParsingDiagnostics(),
  ...project.program.getProgramDiagnostics(),
  ...project.program.getSyntacticDiagnostics(),
  ...project.program.getBindDiagnostics(),
];

const isProjectLocalDiagnostic = (project, diagnostic) => {
  if (!diagnostic.fileName) return true;
  const sourceFile = project.program.getSourceFile(diagnostic.fileName);
  if (!sourceFile) return true;
  return (
    !project.program.isSourceFileDefaultLibrary(sourceFile) &&
    !project.program.isSourceFileFromExternalLibrary(sourceFile)
  );
};

const isSvelteSpecifier = (node) =>
  typeof node.moduleSpecifier?.text === "string" && node.moduleSpecifier.text.endsWith(".svelte");

const isUnknownAlias = (checker, specifier) => {
  const symbol = checker.getSymbolAtLocation(specifier.name);
  if (!symbol) return true;
  return checker.isUnknownSymbol(checker.getAliasedSymbol(symbol));
};

const concreteExportSymbol = (checker, symbol) => {
  const target =
    (symbol.flags & SymbolFlags.Alias) === 0 ? symbol : checker.getAliasedSymbol(symbol);
  return !checker.isUnknownSymbol(target) && (target.declarations?.length ?? 0) > 0;
};

const hasProvableNamedExports = (project, declaration) => {
  const moduleSymbol = project.checker.getSymbolAtLocation(declaration.moduleSpecifier);
  if (!moduleSymbol) return false;
  const hasConcreteModuleDeclaration = moduleSymbol.declarations?.some((moduleDeclaration) => {
    const declarationPath =
      moduleDeclaration.path ??
      moduleDeclaration.fileName ??
      moduleDeclaration.getSourceFile?.().fileName ??
      "";
    return declarationPath.endsWith(".d.svelte") || declarationPath.endsWith(".d.svelte.ts");
  });
  if (!hasConcreteModuleDeclaration) return false;
  const namedExports = project.checker
    .getExportsOfModule(moduleSymbol)
    .filter((symbol) => symbol.name !== "default");
  return namedExports.every((symbol) => concreteExportSymbol(project.checker, symbol));
};

const svelteDeclarationHasGap = (project, node) => {
  if (!isSvelteSpecifier(node)) return false;
  if (isImportDeclaration(node)) {
    const bindings = node.importClause?.namedBindings;
    return (
      bindings !== undefined &&
      isNamedImports(bindings) &&
      bindings.elements.some((item) => isUnknownAlias(project.checker, item))
    );
  }
  if (!isExportDeclaration(node)) return false;
  if (!node.exportClause) return !hasProvableNamedExports(project, node);
  if (!isNamedExports(node.exportClause)) return !hasProvableNamedExports(project, node);
  return node.exportClause.elements.some((item) => isUnknownAlias(project.checker, item));
};

const sourceHasSvelteVirtualModuleGap = (project, sourceFile) => {
  let gap = false;
  const visit = (node) => {
    if (gap) return;
    gap = svelteDeclarationHasGap(project, node);
    if (gap) return;
    node.forEachChild((child) => {
      visit(child);
      return undefined;
    });
  };
  visit(sourceFile);
  return gap;
};

const hasSvelteVirtualModuleGap = (project) =>
  projectSourceFiles(project).some(
    (sourceFile) =>
      sourceFile.text.includes(".svelte") && sourceHasSvelteVirtualModuleGap(project, sourceFile),
  );

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

// The hash deliberately excludes the project's root file listing: base and
// head of a diff naturally differ in file membership (added or deleted
// files), and semantic evidence stays comparable as long as the
// configuration that produced it is the same. Configuration changes are
// covered by the compiler options and the config document closure
// (https://github.com/fallow-rs/fallow/issues/2102).
const effectiveProjectConfigHash = (root, project) => {
  const effective = {
    config: configPath(root, project),
    compiler_options: normalizedConfigValue(root, project.compilerOptions),
    config_document: configDocumentClosure(root, project.configFileName),
  };
  return `sha256:${createHash("sha256").update(JSON.stringify(effective)).digest("hex")}`;
};

export const projectState = (root, project, source) => {
  const diagnostics = structuralDiagnostics(project);
  const localDiagnosticCount = diagnostics.filter((diagnostic) =>
    isProjectLocalDiagnostic(project, diagnostic),
  ).length;
  const hasLocalDiagnostics = localDiagnosticCount > 0;
  const svelteVirtualModuleGap = !hasLocalDiagnostics && hasSvelteVirtualModuleGap(project);
  const reasonCode = hasLocalDiagnostics
    ? "blocking-diagnostics"
    : svelteVirtualModuleGap
      ? "svelte-virtual-module-exports"
      : diagnostics.length > 0
        ? "blocking-diagnostics"
        : null;
  return {
    project,
    config: configPath(root, project),
    effective_config_hash: effectiveProjectConfigHash(root, project),
    source,
    status: reasonCode === null ? "complete" : "unavailable",
    reason_code: reasonCode,
    blocking_diagnostic_count: reasonCode === "blocking-diagnostics" ? diagnostics.length : 0,
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
