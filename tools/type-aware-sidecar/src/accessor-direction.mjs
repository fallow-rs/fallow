import { ast, SyntaxKind } from "./backend.mjs";
import { declarationName, ownerDeclaration } from "./semantic-identity.mjs";

const { isGetAccessorDeclaration, isSetAccessorDeclaration } = ast;

// Node kinds that a write target passes through on the way to its assignment:
// `[a] = xs`, `({ a } = o)`, `({ k: a } = o)`, `(a as T) = v`.
const TARGET_WRAPPERS = new Set([
  SyntaxKind.ParenthesizedExpression,
  SyntaxKind.ArrayLiteralExpression,
  SyntaxKind.ObjectLiteralExpression,
  SyntaxKind.ShorthandPropertyAssignment,
  SyntaxKind.SpreadElement,
  SyntaxKind.SpreadAssignment,
  SyntaxKind.AsExpression,
  SyntaxKind.TypeAssertionExpression,
  SyntaxKind.NonNullExpression,
  SyntaxKind.SatisfiesExpression,
]);

const isAssignmentOperator = (kind) =>
  kind >= SyntaxKind.FirstAssignment && kind <= SyntaxKind.LastAssignment;

const isIncrementOperator = (kind) =>
  kind === SyntaxKind.PlusPlusToken || kind === SyntaxKind.MinusMinusToken;

const memberAccess = (reference) => {
  const parent = reference.parent;
  if (parent?.kind === SyntaxKind.PropertyAccessExpression && parent.name === reference) {
    return parent;
  }
  if (
    parent?.kind === SyntaxKind.ElementAccessExpression &&
    parent.argumentExpression === reference
  ) {
    return parent;
  }
  return reference;
};

const binaryDirection = (parent, node) => {
  if (parent.left !== node || !isAssignmentOperator(parent.operatorToken.kind)) return "read";
  return parent.operatorToken.kind === SyntaxKind.EqualsToken ? "write" : "read-write";
};

/**
 * How an expression is accessed: `read`, `write` (the target of a plain `=`, a
 * destructuring assignment or a `for...in`/`for...of` head), or `read-write`
 * (a compound assignment, `++` or `--`).
 */
export const targetDirection = (target) => {
  let node = target;
  for (let parent = node.parent; parent; parent = node.parent) {
    if (TARGET_WRAPPERS.has(parent.kind)) {
      node = parent;
      continue;
    }
    switch (parent.kind) {
      case SyntaxKind.PropertyAssignment:
        if (parent.initializer !== node) return "read";
        node = parent;
        continue;
      case SyntaxKind.BinaryExpression:
        return binaryDirection(parent, node);
      case SyntaxKind.PrefixUnaryExpression:
      case SyntaxKind.PostfixUnaryExpression:
        return isIncrementOperator(parent.operator) ? "read-write" : "read";
      case SyntaxKind.ForInStatement:
      case SyntaxKind.ForOfStatement:
        return parent.initializer === node ? "write" : "read";
      default:
        return "read";
    }
  }
  return "read";
};

/** Whether a getter or setter has the other half of its pair in the same owner. */
export const accessorPairExists = (declaration) => {
  if (!isGetAccessorDeclaration(declaration) && !isSetAccessorDeclaration(declaration)) {
    return false;
  }
  const owner = ownerDeclaration(declaration);
  const name = declarationName(declaration);
  return Boolean(
    owner?.members?.some(
      (member) =>
        member !== declaration &&
        declarationName(member) === name &&
        (isGetAccessorDeclaration(member) || isSetAccessorDeclaration(member)),
    ),
  );
};

/**
 * Whether a reference counts as a use of this declaration. The checker gives
 * both halves of a get and set accessor pair one symbol, so a read would
 * otherwise confirm the setter and a write would confirm the getter. A lone
 * accessor counts every reference.
 */
const isShorthandKey = (reference) =>
  reference.parent?.kind === SyntaxKind.ShorthandPropertyAssignment &&
  reference.parent.name === reference;

const memberDirection = (reference) =>
  // The key of `({ value } = inst)` reads `inst.value` and writes only the local binding.
  isShorthandKey(reference) ? "read" : targetDirection(memberAccess(reference));

export const countsAsAccessorUse = (declaration, reference) => {
  if (!accessorPairExists(declaration)) return true;
  const direction = memberDirection(reference);
  return isSetAccessorDeclaration(declaration) ? direction !== "read" : direction !== "write";
};
