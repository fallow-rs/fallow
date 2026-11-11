// Syntactic facts about a declaration that the type proofs share.

import { SyntaxKind } from "./backend.mjs";

const hasModifier = (node, kind) => node.modifiers?.some((modifier) => modifier.kind === kind);

/** True when a direct child of `node` is a token of `kind`, such as `!`. */
export const hasToken = (node, kind) => {
  let found = false;
  node.forEachChild((child) => {
    found = found || child.kind === kind;
    return found ? true : undefined;
  });
  return found;
};

/**
 * True for a `declare` declaration, a member of one, or anything in a
 * declaration file: their types are claims that no code in the project checks.
 */
export const isAmbient = (node) => {
  for (let current = node; current; current = current.parent) {
    if (current.isDeclarationFile === true) return true;
    if (hasModifier(current, SyntaxKind.DeclareKeyword)) return true;
  }
  return false;
};
