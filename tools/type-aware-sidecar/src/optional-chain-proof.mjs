// Proves that an optional-chaining mutant is equivalent to the original code.
//
// The mutants `a?.b` -> `a.b`, `a?.()` -> `a()` and `a?.[k]` -> `a[k]` change
// nothing when the receiver `a` is never `null` or `undefined` at run time.
// The proof trusts declarations and never trusts flow analysis or assertions:
//
// - It reads the declared type of the root binding or property, not the type
//   that control flow narrows at the use. A call can invalidate a narrowing
//   that the checker keeps.
// - It rejects an assertion (`as`, `<T>x`, `x!`) anywhere in the receiver, and
//   in the initializer of each local binding that the receiver reads.
// - It rejects bindings and properties that the language does not force to
//   hold a value: `declare` and `.d.ts` variables, bindings without an
//   initializer, `!` and `declare` properties, and class properties that no
//   initializer or constructor assignment sets.
// - It needs `strictNullChecks` (a type excludes nullish values only then) and
//   `noUncheckedIndexedAccess` (otherwise `xs[0]` has a non-nullable type and
//   is `undefined` for an empty array).
//
// When the provenance of the type is unclear, the proof says nothing and the
// mutant keeps its normal check. Assignments to a binding after its
// initializer are not tracked.

import path from "node:path";

import { ast, projectCompilerOptions, sync, SyntaxKind } from "./backend.mjs";
import { declarationsForSymbol } from "./semantic-identity.mjs";

const { TypeFlags, SymbolFlags } = sync;
const {
  isArrayLiteralExpression,
  isArrowFunction,
  isAsExpression,
  isBinaryExpression,
  isCallExpression,
  isClassDeclaration,
  isClassExpression,
  isConstructorDeclaration,
  isElementAccessExpression,
  isExpressionStatement,
  isFunctionExpression,
  isIdentifier,
  isLiteralExpression,
  isNewExpression,
  isNonNullExpression,
  isObjectLiteralExpression,
  isParenthesizedExpression,
  isPropertyAccessExpression,
  isPropertyDeclaration,
  isSatisfiesExpression,
  isTemplateExpression,
  isThisExpression,
  isTypeAssertion,
  isVariableDeclaration,
} = ast;

/** The reason that the response gives for a proven mutant. */
export const TYPE_PROVEN_REASON = "type-proven-never-nullish";

const OPTIONAL_TOKEN = "?.";
const TYPESCRIPT_EXTENSIONS = new Set([".ts", ".tsx", ".mts", ".cts"]);
const MAX_UNION_DEPTH = 8;
const MAX_BINDING_DEPTH = 8;
const NON_NULLABLE_FLAGS = TypeFlags.DefinitelyNonNullable;
const UNION_OR_INTERSECTION = TypeFlags.Union | TypeFlags.Intersection;

const strictNullChecks = (options) =>
  options.strictNullChecks === true ||
  (options.strictNullChecks === undefined && options.strict === true);

const allowsTypeProof = (project, absolutePath) => {
  const options = projectCompilerOptions(project);
  return (
    TYPESCRIPT_EXTENSIONS.has(path.extname(absolutePath).toLowerCase()) &&
    strictNullChecks(options) &&
    options.noUncheckedIndexedAccess === true
  );
};

const neverNullish = (type, depth) => {
  if (depth > MAX_UNION_DEPTH) return false;
  if ((type.flags & UNION_OR_INTERSECTION) !== 0) {
    const members = type.getTypes?.() ?? [];
    return members.length > 0 && members.every((member) => neverNullish(member, depth + 1));
  }
  return type.flags !== 0 && (type.flags & ~NON_NULLABLE_FLAGS) === 0;
};

const isParameter = (node) => node.kind === SyntaxKind.Parameter;
const isPropertySignature = (node) => node.kind === SyntaxKind.PropertySignature;

const isAssertion = (node) =>
  isAsExpression(node) || isTypeAssertion(node) || isNonNullExpression(node);

const containsAssertion = (node) => {
  if (isAssertion(node)) return true;
  let found = false;
  node.forEachChild((child) => {
    found = found || containsAssertion(child);
    return found ? true : undefined;
  });
  return found;
};

const hasModifier = (node, kind) => node.modifiers?.some((modifier) => modifier.kind === kind);

const hasToken = (node, kind) => {
  let found = false;
  node.forEachChild((child) => {
    found = found || child.kind === kind;
    return found ? true : undefined;
  });
  return found;
};

// True for a `declare` declaration, a member of one, or anything in a
// declaration file: their types are claims that no code in the project checks.
const isAmbient = (node) => {
  for (let current = node; current; current = current.parent) {
    if (current.isDeclarationFile === true) return true;
    if (hasModifier(current, SyntaxKind.DeclareKeyword)) return true;
  }
  return false;
};

const typeAt = (project, declaration, symbol) =>
  project.checker.getTypeOfSymbolAtLocation(symbol, declaration);

// A constructor statement `this.name = value;` at the top level of the body.
const constructorAssigns = (classNode, name) =>
  classNode.members.some(
    (member) =>
      isConstructorDeclaration(member) &&
      member.body !== undefined &&
      member.body.statements.some((statement) => {
        if (!isExpressionStatement(statement)) return false;
        const { expression } = statement;
        return (
          isBinaryExpression(expression) &&
          expression.operatorToken.kind === SyntaxKind.EqualsToken &&
          isPropertyAccessExpression(expression.left) &&
          isThisExpression(expression.left.expression) &&
          expression.left.name.text === name &&
          !containsAssertion(expression.right)
        );
      }),
  );

const isParameterProperty = (declaration) =>
  isParameter(declaration) && isConstructorDeclaration(declaration.parent);

const classPropertyIsSet = (project, declaration, context) => {
  if (
    hasModifier(declaration, SyntaxKind.DeclareKeyword) ||
    hasToken(declaration, SyntaxKind.ExclamationToken)
  ) {
    return false;
  }
  if (declaration.initializer !== undefined) {
    return initializerIsSafe(project, declaration.initializer, context);
  }
  if (hasModifier(declaration, SyntaxKind.StaticKeyword)) return false;
  const owner = declaration.parent;
  return (
    (isClassDeclaration(owner) || isClassExpression(owner)) &&
    constructorAssigns(owner, declaration.name.text)
  );
};

const propertyIsSafe = (project, declaration, context) => {
  if (isPropertySignature(declaration)) return true;
  if (isParameterProperty(declaration)) return !isAmbient(declaration);
  if (!isPropertyDeclaration(declaration) || isAmbient(declaration)) return false;
  return classPropertyIsSet(project, declaration, context);
};

// The initializer must itself be non-nullish by declaration. Otherwise a
// stale narrowing in the initializer would give the binding a type that the
// value does not have.
const initializerIsSafe = (project, initializer, context) => {
  if (containsAssertion(initializer)) return false;
  const type = declaredType(project, initializer, context);
  return type !== null && neverNullish(type, 0);
};

const uniqueDeclaration = (project, symbol) => {
  const declarations = declarationsForSymbol(project, symbol);
  return declarations.length === 1 ? declarations[0] : null;
};

const bindingType = (project, identifier, context) => {
  if (context.depth > MAX_BINDING_DEPTH) return null;
  let symbol = project.checker.getSymbolAtLocation(identifier);
  if (!symbol) return null;
  if ((symbol.flags & SymbolFlags.Alias) !== 0) {
    symbol = project.checker.getAliasedSymbol(symbol);
    if (project.checker.isUnknownSymbol(symbol)) return null;
  }
  const declaration = uniqueDeclaration(project, symbol);
  if (declaration === null || context.visited.has(declaration) || isAmbient(declaration)) {
    return null;
  }
  context.visited.add(declaration);
  const inner = { ...context, depth: context.depth + 1 };
  if (isParameter(declaration) && isIdentifier(declaration.name)) {
    return typeAt(project, declaration, symbol);
  }
  if (
    isVariableDeclaration(declaration) &&
    isIdentifier(declaration.name) &&
    declaration.initializer !== undefined &&
    initializerIsSafe(project, declaration.initializer, inner)
  ) {
    return typeAt(project, declaration, symbol);
  }
  return null;
};

const propertyType = (project, access, context) => {
  const objectType = declaredType(project, access.expression, context);
  if (objectType === null) return null;
  const property = project.checker.getPropertyOfType(objectType, access.name.text);
  if (!property) return null;
  const declaration = uniqueDeclaration(project, property);
  if (declaration === null || !propertyIsSafe(project, declaration, context)) return null;
  return typeAt(project, declaration, property);
};

const callType = (project, call) => {
  const signature = project.checker.getResolvedSignature(call);
  const declaration = signature?.declaration?.resolve(project);
  if (!declaration?.type) return null;
  return project.checker.getTypeAtLocation(declaration.type) ?? null;
};

const isFreshValue = (node) =>
  isObjectLiteralExpression(node) ||
  isArrayLiteralExpression(node) ||
  isArrowFunction(node) ||
  isFunctionExpression(node) ||
  isClassExpression(node) ||
  isNewExpression(node) ||
  isTemplateExpression(node) ||
  isLiteralExpression(node);

// The type that declarations give to `expression`, or null when the
// expression is not a form whose type the proof trusts. The checker's own
// type at the use is never read for a reference, because it reflects control
// flow narrowing.
const declaredType = (project, expression, context) => {
  if (isParenthesizedExpression(expression) || isSatisfiesExpression(expression)) {
    return declaredType(project, expression.expression, context);
  }
  if (isIdentifier(expression)) return bindingType(project, expression, context);
  if (isThisExpression(expression)) {
    const type = project.checker.getTypeAtLocation(expression);
    return type ? (project.checker.getBaseConstraintOfType(type) ?? type) : null;
  }
  if (isPropertyAccessExpression(expression)) return propertyType(project, expression, context);
  if (isCallExpression(expression) && expression.questionDotToken === undefined) {
    return callType(project, expression);
  }
  if (isFreshValue(expression)) return project.checker.getTypeAtLocation(expression) ?? null;
  return null;
};

const optionalLinkReceiver = (node) => {
  if (node.questionDotToken === undefined) return null;
  const hasReceiver =
    isPropertyAccessExpression(node) || isElementAccessExpression(node) || isCallExpression(node);
  return hasReceiver ? node.expression : null;
};

// Maps the UTF-16 end offset of each `?.` token to the receiver it follows.
const receiversByLinkEnd = (sourceFile) => {
  const receivers = new Map();
  const visit = (node) => {
    const receiver = optionalLinkReceiver(node);
    if (receiver !== null) receivers.set(node.questionDotToken.end, receiver);
    node.forEachChild((child) => {
      visit(child);
      return undefined;
    });
  };
  visit(sourceFile);
  return receivers;
};

const isOptionalLink = (original, mutant) =>
  original.subarray(mutant.start, mutant.end).toString("utf8") === OPTIONAL_TOKEN;

const utf16Offset = (original, byteOffset) =>
  original.subarray(0, byteOffset).toString("utf8").length;

const receiverIsNeverNullish = (project, receiver) => {
  if (containsAssertion(receiver)) return false;
  try {
    const type = declaredType(project, receiver, { visited: new Set(), depth: 0 });
    return type !== null && neverNullish(type, 0);
  } catch {
    return false;
  }
};

/**
 * Returns the ids of the mutants of `file` that remove an optional link from
 * a receiver whose declarations prove it never nullish. `project` is the
 * project of the original file, before any mutant is applied.
 */
export const typeProvenEquivalents = (project, file, original) => {
  const links = file.mutants.filter((mutant) => isOptionalLink(original, mutant));
  if (links.length === 0 || !allowsTypeProof(project, file.absolutePath)) return new Set();
  const sourceFile = project.program.getSourceFile(file.absolutePath);
  if (!sourceFile) return new Set();
  try {
    const receivers = receiversByLinkEnd(sourceFile);
    const proven = links.filter((mutant) => {
      const receiver = receivers.get(utf16Offset(original, mutant.end));
      return receiver !== undefined && receiverIsNeverNullish(project, receiver);
    });
    return new Set(proven.map((mutant) => mutant.id));
  } catch {
    return new Set();
  }
};
