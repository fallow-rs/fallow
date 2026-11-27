import path from "node:path";

import { ast, heritageTypeName, sync } from "./backend.mjs";
import { declarationsForSymbol } from "./semantic-identity.mjs";

const { isImportClause, isImportSpecifier, isNamespaceImport } = ast;
const { SymbolFlags } = sync;

const isImportAlias = (node) =>
  isImportSpecifier(node) || isNamespaceImport(node) || isImportClause(node);

/**
 * Whether a heritage type that a framework contract names resolves to no
 * declaration, or only to the import that names it. Such a type can come
 * from a file that the checker did not read, so its provenance is unknown.
 *
 * @param {unknown} heritageType
 * @param {unknown} project
 * @param {unknown} symbol
 * @param {{ contracts: Array<{ relation: string, heritageSymbol: string, heritageNames: string[] }>, relation: string }} match
 */
export const isUnresolvedContractHeritage = (
  heritageType,
  project,
  symbol,
  { contracts, relation },
) => {
  if (!declarationsForSymbol(project, symbol).every(isImportAlias)) return false;
  const name = heritageTypeName(heritageType)?.getText?.();
  return contracts.some(
    (contract) =>
      contract.relation === relation &&
      (contract.heritageSymbol === name || contract.heritageNames.includes(name)),
  );
};

/** Whether a declaration is in a file inside `root`. */
export const declarationIsProjectLocal = (root, declaration) => {
  const relative = path.relative(root, declaration.getSourceFile().fileName);
  return relative === "" || (!path.isAbsolute(relative) && !relative.startsWith(`..${path.sep}`));
};

/** The npm package name of a declaration file below `node_modules`. */
export const packageNameForDeclaration = (declaration) => {
  const normalized = declaration.getSourceFile().fileName.replaceAll("\\", "/");
  const marker = "/node_modules/";
  const index = normalized.lastIndexOf(marker);
  if (index < 0) return undefined;
  const segments = normalized.slice(index + marker.length).split("/");
  if (segments[0]?.startsWith("@")) {
    return segments.length >= 2 ? `${segments[0]}/${segments[1]}` : undefined;
  }
  return segments[0] || undefined;
};

/** Whether a contract member is optional (`name?` or an optional symbol). */
export const optionalContractDeclaration = (symbol, declaration) =>
  (symbol.flags & SymbolFlags.Optional) !== 0 || declaration?.questionToken !== undefined;

// A dependency or the platform can call an optional member of its own
// interface as a hook, and that call is outside the program. An optional
// member of a project contract has its callers in the program. A workspace
// package outside the analysis root is project code, so only a file below
// `node_modules` or a default library file is a dependency contract.
export const optionalContractRelation = (project, contractDeclaration) =>
  packageNameForDeclaration(contractDeclaration) !== undefined ||
  project.program.isSourceFileDefaultLibrary(contractDeclaration.getSourceFile())
    ? "dependency-hook"
    : "optional-contract";
