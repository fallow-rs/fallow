import path from "node:path";

import { ast, heritageTypeName } from "./backend.mjs";
import { declarationsForSymbol } from "./semantic-identity.mjs";

const { isImportClause, isImportSpecifier, isNamespaceImport } = ast;

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
