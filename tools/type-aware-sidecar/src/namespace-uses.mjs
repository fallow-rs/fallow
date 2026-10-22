import { SyntaxKind, ast, sync } from "./backend.mjs";
import {
  NODE_MODULES_SEGMENT,
  isJavaScriptFile,
  moduleBodyStatements,
  ownSourceFiles,
} from "./reference-names.mjs";
import { declarationsForSymbol, resolveAlias, sourceFileIdentity } from "./semantic-identity.mjs";
import {
  isAssignmentTarget,
  patternOfKey,
  patternSourceType,
  qualifierExpression,
  referenceValueSymbol,
} from "./shorthand-references.mjs";
import { matchingAliasEntries, recordSymbolUncertainty } from "./symbol-targets.mjs";

const { SymbolFlags } = sync;
const {
  isAsExpression,
  isBinaryExpression,
  isBindingElement,
  isCallExpression,
  isComputedPropertyName,
  isElementAccessExpression,
  isExportDeclaration,
  isIdentifier,
  isImportClause,
  isImportDeclaration,
  isImportEqualsDeclaration,
  isImportSpecifier,
  isModuleDeclaration,
  isNamedImports,
  isNamespaceExport,
  isNamespaceImport,
  isNonNullExpression,
  isNumericLiteral,
  isObjectBindingPattern,
  isObjectLiteralExpression,
  isParameterDeclaration,
  isParenthesizedExpression,
  isPropertyAccessExpression,
  isPropertyAssignment,
  isQualifiedName,
  isSatisfiesExpression,
  isShorthandPropertyAssignment,
  isStringLiteralLikeNode,
  isTypeAssertion,
  isTypeQueryNode,
  isVariableDeclaration,
  isVariableStatement,
} = ast;

// A namespace object holds every export of a module. A static member read of
// the object reads one export. Any other use, such as a call argument, an
// alias, a rest element, or a computed key, can read every export, so the
// exports of that module get an uncertainty instead of a closed-world claim.

const WRAPPERS = [
  isParenthesizedExpression,
  isAsExpression,
  isNonNullExpression,
  isSatisfiesExpression,
  isTypeAssertion,
];

const isWrapper = (node) => WRAPPERS.some((matches) => matches(node));

const outermostWrapper = (node) => {
  let current = node;
  while (current.parent && isWrapper(current.parent) && current.parent.expression === current) {
    current = current.parent;
  }
  return current;
};

const innermostExpression = (node) => {
  let current = node;
  while (current && isWrapper(current)) current = current.expression;
  return current;
};

const isLiteralKeyText = (node) => isStringLiteralLikeNode(node) || isNumericLiteral(node);

const isLiteralKey = (name) =>
  isIdentifier(name) ||
  isLiteralKeyText(name) ||
  (isComputedPropertyName(name) && isLiteralKeyText(name.expression));

const isStaticBindingPattern = (pattern) =>
  isObjectBindingPattern(pattern) &&
  pattern.elements.every(
    (element) =>
      !element.dotDotDotToken && (!element.propertyName || isLiteralKey(element.propertyName)),
  );

const isStaticAssignmentPattern = (literal) =>
  isObjectLiteralExpression(literal) &&
  literal.properties.every(
    (property) =>
      isShorthandPropertyAssignment(property) ||
      (isPropertyAssignment(property) && isLiteralKey(property.name)),
  );

// The source is the initializer of a variable, or the default value of a
// parameter. A declared type replaces the type of the source, and then a key
// no longer resolves to the export.
const isStaticDeclarationSource = (parent, node) =>
  parent.initializer === node && !parent.type && isStaticBindingPattern(parent.name);

const keyText = (name) => (isComputedPropertyName(name) ? name.expression.text : name.text);

const bindingKeys = (pattern) =>
  pattern.elements.map((element) => keyText(element.propertyName ?? element.name));

const assignmentKeys = (literal) => literal.properties.map((property) => keyText(property.name));

const isStaticPatternType = (query) =>
  (isParameterDeclaration(query.parent) || isVariableDeclaration(query.parent)) &&
  query.parent.type === query &&
  isStaticBindingPattern(query.parent.name);

// A member read names one export, and the scan tracks a namespace that this
// export holds by its name. A type position cannot read a value: a value of
// type `typeof ns` comes from a value use of `ns`, which the scan classifies on
// its own. Only a type that a pattern destructures reads keys.
const MEMBER_USE_RULES = [
  [isPropertyAccessExpression, (parent, node) => parent.expression === node],
  [isQualifiedName, (parent, node) => parent.left === node],
  [isTypeQueryNode, (parent, node) => parent.exprName === node && !isStaticPatternType(parent)],
];

const isStaticAssignmentSource = (parent, node) =>
  parent.operatorToken.kind === SyntaxKind.EqualsToken &&
  parent.right === node &&
  !isAssignmentTarget(parent) &&
  isStaticAssignmentPattern(parent.left);

const declarationKeys = (parent, node) =>
  isStaticDeclarationSource(parent, node) ? bindingKeys(parent.name) : undefined;

// A keyed read, such as a literal element access or a destructure with literal
// keys, gives the export names that it reads. Such a read can pass on a nested
// namespace under another name.
const KEYED_USE_RULES = [
  [
    isElementAccessExpression,
    (parent, node) =>
      parent.expression === node && isLiteralKeyText(parent.argumentExpression)
        ? [parent.argumentExpression.text]
        : undefined,
  ],
  [isVariableDeclaration, declarationKeys],
  [isParameterDeclaration, declarationKeys],
  [
    isBinaryExpression,
    (parent, node) =>
      isStaticAssignmentSource(parent, node) ? assignmentKeys(parent.left) : undefined,
  ],
  [
    isTypeQueryNode,
    (parent, node) =>
      parent.exprName === node && isStaticPatternType(parent)
        ? bindingKeys(parent.parent.name)
        : undefined,
  ],
];

const ruleResult = (rules, expression) => {
  const parent = expression.parent;
  const rule = parent && rules.find(([matches]) => matches(parent));
  return rule ? rule[1](parent, expression) : undefined;
};

const IMPORT_DECLARATION_PARENTS = [
  isNamespaceImport,
  isNamespaceExport,
  isImportEqualsDeclaration,
  isImportClause,
  isImportSpecifier,
];

const NAMED_DECLARATION_PARENTS = [isVariableDeclaration, isBindingElement, isParameterDeclaration];

// The scan classifies each use of a binding, so its declaration is no use.
const isBindingDeclarationName = (node) =>
  IMPORT_DECLARATION_PARENTS.some((matches) => matches(node.parent)) ||
  NAMED_DECLARATION_PARENTS.some((matches) => matches(node.parent) && node.parent.name === node);

// `outer.inner` reads the namespace `inner` that a barrel re-exports.
const namespaceExpression = (node) => {
  const parent = node.parent;
  const isMemberName =
    (isPropertyAccessExpression(parent) && parent.name === node) ||
    (isQualifiedName(parent) && parent.right === node);
  return outermostWrapper(isMemberName ? parent : node);
};

const isRequireCall = (node) =>
  node !== undefined &&
  isCallExpression(node) &&
  isIdentifier(node.expression) &&
  node.expression.text === "require";

const namespaceImportNames = (statement) => {
  const bindings = statement.importClause?.namedBindings;
  return bindings && isNamespaceImport(bindings) ? [bindings.name] : [];
};

const namespaceExportNames = (statement) =>
  statement.exportClause && isNamespaceExport(statement.exportClause)
    ? [statement.exportClause.name]
    : [];

const requireBindingNames = (statement) =>
  statement.declarationList.declarations
    .filter((declaration) => isRequireCall(declaration.initializer))
    .map((declaration) => declaration.name);

const NAMESPACE_NAME_RULES = [
  [isImportDeclaration, namespaceImportNames],
  [isImportEqualsDeclaration, (statement) => [statement.name]],
  [isExportDeclaration, namespaceExportNames],
  [isVariableStatement, requireBindingNames],
  [
    isModuleDeclaration,
    (statement) => moduleBodyStatements(statement.body).flatMap(statementNamespaceNames),
  ],
];

const statementNamespaceNames = (statement) =>
  NAMESPACE_NAME_RULES.find(([matches]) => matches(statement))?.[1](statement) ?? [];

// Each binding is a pair of the imported name and the local name. A default
// import reads `default`, which `export * as default` can make a namespace.
const importBindings = (statement) => {
  const clause = isImportDeclaration(statement) ? statement.importClause : undefined;
  const bindings = clause?.namedBindings;
  const named =
    bindings && isNamedImports(bindings)
      ? bindings.elements.map((element) => [
          (element.propertyName ?? element.name).text,
          element.name.text,
        ])
      : [];
  return clause?.name ? [["default", clause.name.text], ...named] : named;
};

// `import { inner as local }` binds a namespace that a barrel re-exports under
// a new name. Each pass adds one level of renames.
const addRenamedImports = (names, bindings) => {
  let size = -1;
  while (size !== names.size) {
    size = names.size;
    bindings.filter(([imported]) => names.has(imported)).forEach(([, local]) => names.add(local));
  }
  return names;
};

// Only these declarations bind a name to a namespace object. The name check
// skips the alias lookup, which costs checker round trips, for other names.
const programNamespaceNames = (project) => {
  const statements = ownSourceFiles(project).flatMap((sourceFile) => sourceFile.statements);
  const names = statements
    .flatMap(statementNamespaceNames)
    .filter((name) => name && isIdentifier(name))
    .map((name) => name.text);
  return addRenamedImports(new Set(names), statements.flatMap(importBindings));
};

const caches = new WeakMap();

const batchCache = (state, project) => {
  const byProject = caches.get(state) ?? new Map();
  caches.set(state, byProject);
  if (!byProject.has(project)) {
    byProject.set(project, {
      names: programNamespaceNames(project),
      modulesBySymbol: new Map(),
      nestedModules: new Map(),
      escapedModules: new Set(),
    });
  }
  return byProject.get(project);
};

const moduleSourceFile = (project, symbol) =>
  symbol && (symbol.flags & SymbolFlags.ValueModule) !== 0
    ? declarationsForSymbol(project, symbol).find(
        (declaration) => declaration.kind === SyntaxKind.SourceFile,
      )
    : undefined;

// A module below node_modules holds no query targets, so its namespace object
// needs no uncertainty.
const isOwnModule = (sourceFile) =>
  sourceFile !== undefined && !NODE_MODULES_SEGMENT.test(sourceFile.fileName);

const namespaceModule = (project, cache, symbol) => {
  if (!symbol || (symbol.flags & SymbolFlags.Alias) === 0) return undefined;
  if (!cache.modulesBySymbol.has(symbol)) {
    const target = resolveAlias(project.checker, symbol);
    cache.modulesBySymbol.set(
      symbol,
      isOwnModule(moduleSourceFile(project, target)) ? target : undefined,
    );
  }
  return cache.modulesBySymbol.get(symbol);
};

// Only an alias or a symbol with a queried export name can match an entry.
const canMatchEntry = (state, symbol) =>
  (symbol.flags & SymbolFlags.Alias) !== 0 || state.exportedNames.has(symbol.name);

const recordModuleEscape = (state, project, cache, moduleSymbol) => {
  if (cache.escapedModules.has(moduleSymbol)) return;
  cache.escapedModules.add(moduleSymbol);
  const identity = sourceFileIdentity(moduleSourceFile(project, moduleSymbol));
  const entries = new Set(state.exportEntriesByModule.get(identity) ?? []);
  const exports = project.checker.getExportsOfModule(moduleSymbol);
  for (const exported of exports.filter((symbol) => canMatchEntry(state, symbol))) {
    matchingAliasEntries(project, exported, "value", state).forEach((entry) => entries.add(entry));
    const reexported = namespaceModule(project, cache, exported);
    if (reexported) recordModuleEscape(state, project, cache, reexported);
  }
  entries.forEach((entry) => recordSymbolUncertainty(state, entry, "dynamic-behavior"));
};

const nestedNamespaceModules = (project, cache, moduleSymbol) => {
  if (!cache.nestedModules.has(moduleSymbol)) {
    const nested = project.checker
      .getExportsOfModule(moduleSymbol)
      .map((exported) => ({
        name: exported.name,
        module: namespaceModule(project, cache, exported),
      }))
      .filter(({ module }) => module);
    cache.nestedModules.set(moduleSymbol, nested);
  }
  return cache.nestedModules.get(moduleSymbol);
};

const recordExpressionUse = (state, project, cache, expression, moduleSymbol) => {
  if (ruleResult(MEMBER_USE_RULES, expression)) return;
  const keys = ruleResult(KEYED_USE_RULES, expression);
  const escaped = keys
    ? nestedNamespaceModules(project, cache, moduleSymbol)
        .filter(({ name }) => keys.includes(name))
        .map(({ module }) => module)
    : [moduleSymbol];
  escaped.forEach((escapedModule) => recordModuleEscape(state, project, cache, escapedModule));
};

// The checker gives no symbol for the specifier of `module.exports =
// require(...)`, but the call has the module type.
const requiredModule = (project, call) => {
  const symbol = project.checker.getSymbolAtLocation(call.arguments[0]);
  return moduleSourceFile(project, symbol)
    ? symbol
    : project.checker.getTypeAtLocation(call)?.getSymbol();
};

const requireCallOf = (node) => {
  const call = node.parent;
  return isRequireCall(call) && call.expression === node ? call : undefined;
};

const hasLiteralSpecifier = (call) =>
  call.arguments.length === 1 && isStringLiteralLikeNode(call.arguments[0]);

const initializesTrackedName = (cache, expression) => {
  const declaration = expression.parent;
  return (
    isVariableDeclaration(declaration) &&
    declaration.initializer === expression &&
    isIdentifier(declaration.name) &&
    cache.names.has(declaration.name.text)
  );
};

// A `require` call that does not initialize a tracked name, such as
// `module.exports = require("./values")`, is a namespace object use.
const recordRequireUse = (state, project, cache, node) => {
  const call = requireCallOf(node);
  if (!call || !hasLiteralSpecifier(call)) return;
  const expression = outermostWrapper(call);
  if (initializesTrackedName(cache, expression)) return;
  const moduleSymbol = requiredModule(project, call);
  if (isOwnModule(moduleSourceFile(project, moduleSymbol))) {
    recordExpressionUse(state, project, cache, expression, moduleSymbol);
  }
};

/**
 * Records a `dynamic-behavior` uncertainty for the exports that a namespace
 * object use can read, unless the use is a static member read.
 */
export const recordNamespaceUse = (state, project, node, symbol) => {
  if (state.exportedNames.size === 0 || !isIdentifier(node)) return;
  const cache = batchCache(state, project);
  if (node.text === "require" && isJavaScriptFile(node.getSourceFile())) {
    recordRequireUse(state, project, cache, node);
    return;
  }
  if (!cache.names.has(node.text) || isBindingDeclarationName(node)) return;
  const valueSymbol = isShorthandPropertyAssignment(node.parent)
    ? referenceValueSymbol(project, node)
    : symbol;
  const moduleSymbol = namespaceModule(project, cache, valueSymbol);
  if (moduleSymbol) {
    recordExpressionUse(state, project, cache, namespaceExpression(node), moduleSymbol);
  }
};

const recordImportedModuleEscape = (state, project, specifier) => {
  const moduleSymbol = project.checker.getSymbolAtLocation(specifier);
  if (isOwnModule(moduleSourceFile(project, moduleSymbol))) {
    recordModuleEscape(state, project, batchCache(state, project), moduleSymbol);
  }
};

const isDynamicImportCall = (node) =>
  isCallExpression(node) && node.expression.getText(node.getSourceFile()) === "import";

const recordModuleEntriesUncertainty = (state, entries) =>
  (entries ?? []).forEach((entry) => recordSymbolUncertainty(state, entry, "dynamic-behavior"));

/**
 * Records a `dynamic-behavior` uncertainty for the exports that a dynamic
 * import can read. A non-literal specifier can load any module of the project.
 * A literal specifier loads one namespace object, which also holds the exports
 * that the module re-exports.
 */
export const recordDynamicImportUncertainty = (state, project, node, moduleIdentityOf) => {
  if (!isDynamicImportCall(node)) return;
  const specifier = node.arguments[0];
  if (!specifier || !isStringLiteralLikeNode(specifier)) {
    recordModuleEntriesUncertainty(state, state.exportEntriesByProject.get(project));
    return;
  }
  if (state.exportedNames.size > 0) recordImportedModuleEscape(state, project, specifier);
  const moduleIdentity = moduleIdentityOf(project, specifier);
  if (moduleIdentity) {
    recordModuleEntriesUncertainty(state, state.exportEntriesByModule.get(moduleIdentity));
  }
};

const moduleIdentityOfSymbol = (project, symbol) => {
  const sourceFile = moduleSourceFile(project, symbol);
  return sourceFile ? sourceFileIdentity(sourceFile) : undefined;
};

const moduleIdentityOfType = (project, type) =>
  type ? moduleIdentityOfSymbol(project, type.getSymbol()) : undefined;

// Many reads share one qualifier symbol, such as `ns` in `(ns as T).name`.
const symbolModuleIdentities = new WeakMap();

const aliasModuleIdentity = (project, symbol) => {
  if (!symbol) return undefined;
  const bySymbol = symbolModuleIdentities.get(project) ?? new Map();
  symbolModuleIdentities.set(project, bySymbol);
  if (!bySymbol.has(symbol)) {
    bySymbol.set(symbol, moduleIdentityOfSymbol(project, resolveAlias(project.checker, symbol)));
  }
  return bySymbol.get(symbol);
};

const qualifierModuleIdentity = (project, qualifier) => {
  const symbol = project.checker.getSymbolAtLocation(innermostExpression(qualifier));
  return (
    aliasModuleIdentity(project, symbol) ??
    (isQualifiedName(qualifier)
      ? undefined
      : moduleIdentityOfType(project, project.checker.getTypeAtLocation(qualifier)))
  );
};

const sourceIdentityOf = (project, node) => {
  const pattern = patternOfKey(node);
  if (pattern) return moduleIdentityOfType(project, patternSourceType(project, pattern));
  const qualifier = qualifierExpression(node);
  return qualifier ? qualifierModuleIdentity(project, qualifier) : undefined;
};

// The scan asks once for each entry that a name matches. One answer for each
// name keeps the checker round trips per reference, not per entry.
const sourceIdentities = new WeakMap();

/**
 * Returns the module whose namespace object a member name reads, such as the
 * module of `ns` in `(ns as typeof ns).name` or in `({ name } = ns)`.
 */
export const namespaceSourceIdentity = (project, node) => {
  const byNode = sourceIdentities.get(project) ?? new WeakMap();
  sourceIdentities.set(project, byNode);
  if (!byNode.has(node)) byNode.set(node, sourceIdentityOf(project, node));
  return byNode.get(node);
};
