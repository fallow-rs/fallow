import { ast } from "./backend.mjs";
import { canonicalFileIdentity } from "./file-identity.mjs";

const {
  isExportDeclaration,
  isImportDeclaration,
  isImportEqualsDeclaration,
  isModuleBlock,
  isModuleDeclaration,
  isNamedExports,
  isNamespaceExportDeclaration,
  isNamespaceImport,
} = ast;

const JAVASCRIPT_FILE = /\.[cm]?jsx?$/iu;

// In JavaScript, require bindings and export assignments also create aliases.
// The scan keeps every identifier of these files.
export const isJavaScriptFile = (sourceFile) => JAVASCRIPT_FILE.test(sourceFile.fileName);

// An alias edge says that the local name of an import or export specifier
// refers to the imported name. A `from` of null means that the local name is
// always kept: a default import, a namespace import or export, or an
// `import x =` declaration.
const ALWAYS = null;
const edge = (from, to) => ({ from: from?.text ?? ALWAYS, to: to?.text });
const specifierEdge = (element) => edge(element.propertyName ?? element.name, element.name);

const importEdges = ({ importClause }) => {
  const bindings = importClause?.namedBindings;
  const defaultEdge = importClause?.name ? [{ from: ALWAYS, to: importClause.name.text }] : [];
  if (!bindings) return defaultEdge;
  if (isNamespaceImport(bindings))
    return [...defaultEdge, { from: ALWAYS, to: bindings.name.text }];
  return [...defaultEdge, ...bindings.elements.map(specifierEdge)];
};

const exportEdges = ({ exportClause }) => {
  if (!exportClause) return [];
  if (isNamedExports(exportClause)) return exportClause.elements.map(specifierEdge);
  return [{ from: ALWAYS, to: exportClause.name.text }];
};

/** Returns the statements of a `namespace` or `declare module` body, also when nested. */
export const moduleBodyStatements = (body) => {
  if (body && isModuleDeclaration(body)) return moduleBodyStatements(body.body);
  return body && isModuleBlock(body) ? body.statements : [];
};

// A JavaScript file can make an alias with `exports.run =`, `exports["run"] =`
// or `module.exports.run =`, also inside a comma sequence, a condition or a
// wrapper function. The text match finds each such name, and the scan keeps
// it, because a TypeScript importer of `run` can reach a target through it.
const COMMON_JS_EXPORT =
  /\b(?:module\s*\.\s*)?exports\s*(?:\.\s*([A-Za-z_$][\w$]*)|\[\s*(["'`])([^"'`]+)\2\s*\])/gu;

const commonJsExportEdges = (sourceFile) =>
  isJavaScriptFile(sourceFile)
    ? [...(sourceFile.text ?? "").matchAll(COMMON_JS_EXPORT)].map((match) => ({
        from: ALWAYS,
        to: match[1] ?? match[3],
      }))
    : [];

const statementAliasEdges = (statement) => {
  if (isImportDeclaration(statement)) return importEdges(statement);
  if (isImportEqualsDeclaration(statement) || isNamespaceExportDeclaration(statement)) {
    return [{ from: ALWAYS, to: statement.name?.text }];
  }
  if (isExportDeclaration(statement)) return exportEdges(statement);
  if (isModuleDeclaration(statement)) {
    return moduleBodyStatements(statement.body).flatMap(statementAliasEdges);
  }
  return [];
};

// Default-library and external-library files sit below node_modules. The
// path check skips them before the source file and its metadata, which each
// cost a checker round trip.
export const NODE_MODULES_SEGMENT = /[\\/]node_modules[\\/]/u;

const isOwnSourceFile = (project, sourceFile) =>
  Boolean(sourceFile) &&
  !project.program.isSourceFileDefaultLibrary(sourceFile) &&
  !project.program.isSourceFileFromExternalLibrary(sourceFile);

/** Returns the program files outside node_modules and the default libraries. */
export const ownSourceFiles = (project) =>
  project.program
    .getSourceFileNames()
    .filter((fileName) => !NODE_MODULES_SEGMENT.test(fileName))
    .map((fileName) => project.program.getSourceFile(fileName))
    .filter((sourceFile) => isOwnSourceFile(project, sourceFile));

// Alias edges come from every program file whose real path is outside
// node_modules, also a linked workspace package that the program marks as an
// external library or reaches through a node_modules path with
// `preserveSymlinks`.
const isAliasSourceFile = (fileName) =>
  !NODE_MODULES_SEGMENT.test(fileName) ||
  !NODE_MODULES_SEGMENT.test(canonicalFileIdentity(fileName));

const aliasSourceFiles = (project) =>
  project.program
    .getSourceFileNames()
    .filter(isAliasSourceFile)
    .map((fileName) => project.program.getSourceFile(fileName))
    .filter((sourceFile) => sourceFile && !project.program.isSourceFileDefaultLibrary(sourceFile));

const programAliasEdges = (project) =>
  aliasSourceFiles(project)
    .flatMap((sourceFile) => [
      ...sourceFile.statements.flatMap(statementAliasEdges),
      ...commonJsExportEdges(sourceFile),
    ])
    .filter(({ to }) => typeof to === "string");

/** Adds the local name of each edge whose imported name is in `names`, until no name is added. */
const closeOverAliases = (names, edges) => {
  let pending = edges.filter(({ from }) => from !== ALWAYS);
  let added = true;
  while (added) {
    added = false;
    const next = [];
    for (const aliasEdge of pending) {
      if (!names.has(aliasEdge.from)) {
        next.push(aliasEdge);
      } else if (!names.has(aliasEdge.to)) {
        names.add(aliasEdge.to);
        added = true;
      }
    }
    pending = next;
  }
  return names;
};

/**
 * Returns the identifier names that can reach a symbol-use target.
 *
 * An identifier resolves to a declaration with its own name, or to an alias
 * that an import or export declares under that name. An alias can reach a
 * target only when the name that it imports or re-exports can. So the scan
 * needs the target names, `default`, and the local name of each alias whose
 * imported name is one of these names, followed through re-export chains. The
 * local names of default imports, namespace imports and exports, and
 * `import x =` declarations are always kept.
 */
export const referenceNames = (projects, targetNames) =>
  referenceNamesFromEdges(projects.flatMap(programAliasEdges), targetNames);

/** The reference names for alias edges `{ from, to }`, where `from: null` always keeps `to`. */
export const referenceNamesFromEdges = (edges, targetNames) => {
  const names = new Set([...targetNames, "default"]);
  edges.filter(({ from }) => from === ALWAYS).forEach(({ to }) => names.add(to));
  return closeOverAliases(names, edges);
};
