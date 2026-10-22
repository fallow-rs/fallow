import { SyntaxKind } from "typescript/unstable/ast";
import {
  isArrayLiteralExpression,
  isBinaryExpression,
  isBindingElement,
  isElementAccessExpression,
  isForInStatement,
  isForOfStatement,
  isIdentifier,
  isImportClause,
  isImportEqualsDeclaration,
  isImportSpecifier,
  isObjectBindingPattern,
  isObjectLiteralExpression,
  isParenthesizedExpression,
  isPropertyAccessExpression,
  isPropertyAssignment,
  isQualifiedName,
  isShorthandPropertyAssignment,
  isSpreadAssignment,
  isSpreadElement,
  isStringLiteralLikeNode,
  isVariableDeclaration,
} from "typescript/unstable/ast/is";

// A shorthand name is one identifier with two meanings. The checker resolves
// it to the local binding, so the property it reads, or the import it
// forwards, needs a separate lookup.

const isShorthandBindingName = (node) =>
  isBindingElement(node.parent) &&
  node.parent.name === node &&
  !node.parent.propertyName &&
  !node.parent.dotDotDotToken &&
  isObjectBindingPattern(node.parent.parent);

const isShorthandPropertyName = (node) =>
  isShorthandPropertyAssignment(node.parent) && node.parent.name === node;

/** Returns the symbol of the value that a name reads, also for a shorthand property. */
export const referenceValueSymbol = (project, node) =>
  isShorthandPropertyName(node)
    ? project.checker.getShorthandAssignmentValueSymbol(node.parent)
    : project.checker.getSymbolAtLocation(node);

// Both `const { name } = ns` and `const { name: local } = ns` read `name`
// from `ns`.
const destructuredExpression = (node) => {
  const element = node.parent;
  const readName = element.propertyName ?? element.name;
  const declaration = element.parent?.parent;
  return readName === node && isVariableDeclaration(declaration)
    ? declaration.initializer
    : undefined;
};

const QUALIFIER_RULES = [
  [isBindingElement, (parent, node) => destructuredExpression(node)],
  [
    isPropertyAccessExpression,
    (parent, node) => (parent.name === node ? parent.expression : undefined),
  ],
  [
    isElementAccessExpression,
    (parent, node) => (parent.argumentExpression === node ? parent.expression : undefined),
  ],
  [isQualifiedName, (parent, node) => (parent.right === node ? parent.left : undefined)],
];

/** Returns the expression that qualifies a name, such as `ns` in `ns.name`. */
export const qualifierExpression = (node) => {
  const parent = node.parent;
  const rule = parent && QUALIFIER_RULES.find(([matches]) => matches(parent));
  return rule?.[1](parent, node);
};

const isImportBinding = (node) =>
  isImportSpecifier(node) || isImportClause(node) || isImportEqualsDeclaration(node);

const importBindingNames = (nodes) =>
  new Set(
    nodes
      .filter((node) => isImportBinding(node.parent) && node.parent.name === node)
      .map((node) => node.text),
  );

const isEqualsAssignmentOf = (parent, node) =>
  parent.operatorToken.kind === SyntaxKind.EqualsToken && parent.left === node;
const isInitializerOf = (parent, node) => parent.initializer === node;

const ASSIGNMENT_TARGET_RULES = [
  [isBinaryExpression, isEqualsAssignmentOf],
  [isForOfStatement, isInitializerOf],
  [isForInStatement, isInitializerOf],
  [
    isPropertyAssignment,
    (parent, node) => isInitializerOf(parent, node) && isAssignmentTarget(parent.parent),
  ],
  [isArrayLiteralExpression, (parent) => isAssignmentTarget(parent)],
  [isParenthesizedExpression, (parent) => isAssignmentTarget(parent)],
  [isSpreadElement, (parent) => isAssignmentTarget(parent)],
  [isSpreadAssignment, (parent) => isAssignmentTarget(parent)],
];

const isAssignmentTarget = (node) => {
  const parent = node.parent;
  const rule = parent && ASSIGNMENT_TARGET_RULES.find(([matches]) => matches(parent));
  return rule ? rule[1](parent, node) : false;
};

const propertyNameText = (name) =>
  isIdentifier(name) || isStringLiteralLikeNode(name) ? name.text : undefined;

// Only a direct `=` source and nested object patterns resolve. Array and loop
// patterns give undefined, and the caller records an uncertainty for them.
const assignmentPatternType = (project, literal) => {
  const parent = literal.parent;
  if (isBinaryExpression(parent)) {
    // A binary that is itself a pattern target is a default value, and the
    // pattern also reads the outer source.
    return isAssignmentTarget(parent) ? undefined : project.checker.getTypeAtLocation(parent.right);
  }
  if (!isPropertyAssignment(parent) || !isObjectLiteralExpression(parent.parent)) return undefined;
  const outer = assignmentPatternType(project, parent.parent);
  const name = propertyNameText(parent.name);
  if (!outer || name === undefined) return undefined;
  const property = project.checker.getPropertyOfType(outer, name);
  return property ? project.checker.getTypeOfSymbolAtLocation(property, parent) : undefined;
};

const bindingPropertySymbols = (project, nodes, symbolsByNode) => {
  if (nodes.length === 0) return;
  const types = project.checker.getTypeAtLocation(nodes.map((node) => node.parent.parent));
  nodes.forEach((node, index) => {
    const property = types[index] && project.checker.getPropertyOfType(types[index], node.text);
    if (property) symbolsByNode.set(node, [property]);
  });
};

const assignmentPropertySymbol = (project, node) => {
  const type = assignmentPatternType(project, node.parent.parent);
  return type ? project.checker.getPropertyOfType(type, node.text) : undefined;
};

const isPropertyName = (node) =>
  (isShorthandPropertyAssignment(node.parent) || isPropertyAssignment(node.parent)) &&
  node.parent.name === node;

const shorthandPropertySymbols = (project, node, names, unresolved) => {
  if (isAssignmentTarget(node.parent.parent)) {
    if (!names.candidates.has(node.text)) return [];
    const property = assignmentPropertySymbol(project, node);
    if (!property) unresolved.push(node);
    return property ? [property] : [];
  }
  if (!isShorthandPropertyName(node)) return [];
  if (!names.readsExports || !names.imports.has(node.text)) return [];
  const value = project.checker.getShorthandAssignmentValueSymbol(node.parent);
  return value ? [value] : [];
};

/**
 * Returns the symbols that each name in `nodes` reads. `symbols` holds the
 * checker symbol of each name. A shorthand binding element also reads a
 * property of the destructured type. A shorthand or renamed property in an
 * assignment pattern does the same. A shorthand property in an object literal also reads
 * the binding it names, which can be an import.
 *
 * The candidate names of `state` limit the property lookups, and its export
 * names enable the import lookups. `unresolved` lists the assignment pattern
 * names whose source type is unknown.
 */
export const shorthandReferenceSymbols = (project, nodes, symbols, state) => {
  const symbolsByNode = new Map();
  const unresolved = [];
  bindingPropertySymbols(
    project,
    nodes.filter((node) => isShorthandBindingName(node) && state.candidateNames.has(node.text)),
    symbolsByNode,
  );
  const shorthandProperties = nodes.filter(isPropertyName);
  const readsExports = state.exportedNames.size > 0;
  const names = {
    candidates: state.candidateNames,
    readsExports,
    imports: readsExports && shorthandProperties.length > 0 ? importBindingNames(nodes) : new Set(),
  };
  shorthandProperties.forEach((node) => {
    const extra = shorthandPropertySymbols(project, node, names, unresolved);
    if (extra.length > 0) symbolsByNode.set(node, extra);
  });
  return {
    symbols: nodes.map((node, index) => [symbols[index], ...(symbolsByNode.get(node) ?? [])]),
    unresolved,
  };
};
