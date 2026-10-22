import { ast } from "./backend.mjs";

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

const importNames = ({ importClause }) => {
  const bindings = importClause?.namedBindings;
  if (!bindings) return [importClause?.name];
  if (isNamespaceImport(bindings)) return [importClause.name, bindings.name];
  return [importClause.name, ...bindings.elements.map((element) => element.name)];
};

const exportNames = ({ exportClause }) => {
  if (!exportClause) return [];
  if (isNamedExports(exportClause)) return exportClause.elements.map((element) => element.name);
  return [exportClause.name];
};

/** Returns the statements of a `namespace` or `declare module` body, also when nested. */
export const moduleBodyStatements = (body) => {
  if (body && isModuleDeclaration(body)) return moduleBodyStatements(body.body);
  return body && isModuleBlock(body) ? body.statements : [];
};

const statementAliasNames = (statement) => {
  if (isImportDeclaration(statement)) return importNames(statement);
  if (isImportEqualsDeclaration(statement) || isNamespaceExportDeclaration(statement)) {
    return [statement.name];
  }
  if (isExportDeclaration(statement)) return exportNames(statement);
  if (isModuleDeclaration(statement)) {
    return moduleBodyStatements(statement.body).flatMap(statementAliasNames);
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

const programAliasNames = (project) =>
  ownSourceFiles(project)
    .flatMap((sourceFile) => sourceFile.statements.flatMap(statementAliasNames))
    .map((name) => name?.text)
    .filter((text) => typeof text === "string");

/**
 * Returns the identifier names that can reach a symbol-use target.
 *
 * An identifier resolves to a declaration with its own name, or to an alias
 * that an import or export declares under that name. So the scan needs only
 * the target names and the names that the alias declarations of the project
 * files declare, project declaration files included.
 */
export const referenceNames = (projects, targetNames) =>
  new Set([...targetNames, "default", ...projects.flatMap(programAliasNames)]);
