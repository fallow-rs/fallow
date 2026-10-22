import { SyntaxKind, ast } from "./backend.mjs";

const {
  isArrayLiteralExpression,
  isBinaryExpression,
  isBindingElement,
  isComputedPropertyName,
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
} = ast;

// A shorthand name is one identifier with two meanings. The checker resolves
// it to the local binding, so the property it reads, or the import it
// forwards, needs a separate lookup.

// A literal key can sit in a computed name, such as `{ ["name"]: local }`.
const keyNode = (node) => (isComputedPropertyName(node.parent) ? node.parent : node);

const bindingKeyPattern = (node) => {
  const key = keyNode(node);
  const element = key.parent;
  if (!isBindingElement(element) || !isObjectBindingPattern(element.parent)) return undefined;
  const isKey = element.propertyName
    ? element.propertyName === key
    : element.name === key && !element.dotDotDotToken;
  return isKey ? element.parent : undefined;
};

// The checker already resolves an identifier key that renames, such as `name`
// in `{ name: local }`, to the property.
const isRenamingKey = (node) =>
  isIdentifier(node) && isBindingElement(node.parent) && node.parent.propertyName === node;

const assignmentKeyProperty = (node) => {
  const key = keyNode(node);
  const property = key.parent;
  return (isShorthandPropertyAssignment(property) || isPropertyAssignment(property)) &&
    property.name === key
    ? property
    : undefined;
};

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

/** Returns true when a node is the target of an assignment, such as a pattern in `({ a } = b)`. */
export const isAssignmentTarget = (node) => {
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
  const types = project.checker.getTypeAtLocation(nodes.map(bindingKeyPattern));
  nodes.forEach((node, index) => {
    const property = types[index] && project.checker.getPropertyOfType(types[index], node.text);
    if (property) symbolsByNode.set(node, [property]);
  });
};

const assignmentPropertySymbol = (project, node) => {
  const type = assignmentPatternType(project, assignmentKeyProperty(node).parent);
  return type ? project.checker.getPropertyOfType(type, node.text) : undefined;
};

const isPropertyName = (node) => assignmentKeyProperty(node) !== undefined;

/**
 * Returns the object binding pattern or the assignment pattern whose source a
 * key reads, such as `{ name }` in `const { name } = source`.
 */
export const patternOfKey = (node) => {
  const pattern = bindingKeyPattern(node);
  if (pattern) return pattern;
  const literal = assignmentKeyProperty(node)?.parent;
  return literal && isAssignmentTarget(literal) ? literal : undefined;
};

/** Returns the type of the source that a binding or an assignment pattern reads. */
export const patternSourceType = (project, pattern) =>
  isObjectBindingPattern(pattern)
    ? project.checker.getTypeAtLocation(pattern)
    : assignmentPatternType(project, pattern);

/** Returns true when a string literal names the member that an access or a pattern reads. */
export const isLiteralMemberName = (node) =>
  isStringLiteralLikeNode(node) &&
  ((isElementAccessExpression(node.parent) && node.parent.argumentExpression === node) ||
    patternOfKey(node) !== undefined);

const shorthandPropertySymbols = (project, node, names, unresolved) => {
  if (isAssignmentTarget(assignmentKeyProperty(node).parent)) {
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
 * checker symbol of each name. The key of a binding element, also a string or
 * computed string key, reads a property of the destructured type. A key in an
 * assignment pattern does the same. A shorthand property in an object literal
 * also reads the binding it names, which can be an import.
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
    nodes.filter(
      (node) =>
        state.candidateNames.has(node.text) && bindingKeyPattern(node) && !isRenamingKey(node),
    ),
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
