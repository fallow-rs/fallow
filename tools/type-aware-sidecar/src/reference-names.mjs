import {
  isExportDeclaration,
  isImportDeclaration,
  isImportEqualsDeclaration,
  isModuleBlock,
  isModuleDeclaration,
  isNamedExports,
  isNamespaceExportDeclaration,
  isNamespaceImport,
} from "typescript/unstable/ast/is";

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

const moduleBodyNames = (body) => {
  if (body && isModuleDeclaration(body)) return moduleBodyNames(body.body);
  return body && isModuleBlock(body) ? body.statements.flatMap(statementAliasNames) : [];
};

const statementAliasNames = (statement) => {
  if (isImportDeclaration(statement)) return importNames(statement);
  if (isImportEqualsDeclaration(statement) || isNamespaceExportDeclaration(statement)) {
    return [statement.name];
  }
  if (isExportDeclaration(statement)) return exportNames(statement);
  if (isModuleDeclaration(statement)) return moduleBodyNames(statement.body);
  return [];
};

const isOwnSourceFile = (project, sourceFile) =>
  Boolean(sourceFile) &&
  !project.program.isSourceFileDefaultLibrary(sourceFile) &&
  !project.program.isSourceFileFromExternalLibrary(sourceFile);

/**
 * Returns the identifier names that can reach a symbol-use target.
 *
 * An identifier resolves to a declaration with its own name, or to an alias
 * that an import or export declares under that name. So the scan needs only
 * the target names and the names that the alias declarations of the project
 * files declare, project declaration files included.
 */
export const referenceNames = (projects, targetNames) => {
  const names = new Set([...targetNames, "default"]);
  const visited = new Set();
  const ownSourceFiles = projects.flatMap((project) =>
    project.program
      .getSourceFileNames()
      .filter((fileName) => !visited.has(fileName) && visited.add(fileName))
      .map((fileName) => project.program.getSourceFile(fileName))
      .filter((sourceFile) => isOwnSourceFile(project, sourceFile)),
  );
  ownSourceFiles
    .flatMap((sourceFile) => sourceFile.statements.flatMap(statementAliasNames))
    .forEach((name) => typeof name?.text === "string" && names.add(name.text));
  return names;
};
