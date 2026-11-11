// Call evidence for the `constant-parameter` rule.
//
// The static rule selects the candidates. This module answers two questions
// for them and returns evidence only, never a finding:
//
// - Whether a call argument that the static facts do not know can be
//   `undefined`. When no argument of a parameter with a default can be
//   `undefined`, the default never applies.
// - The calls of one exact class method, with the same fact for each
//   argument, or the reason that the calls are not all visible.
//
// The checker resolves the calls of a method with `getSignatureUsage`, which
// follows the class and interface hierarchy. The list can hold a call that
// dispatches to an override or to another implementer. That is safe for each
// claim of the rule: a value that each call of a larger set passes is also the
// value of each call of the real set. A missing call is not safe, so the
// module abstains when a reference of the method name does not resolve, when a
// file outside the program names the method, and when a computed member
// access can read it.

import { readFileSync } from "node:fs";
import path from "node:path";

import {
  createApi as createBackendApi,
  isInferredProject,
  projectCompilerOptions,
  ast,
  NodeFlags,
  SyntaxKind,
  sync,
} from "./backend.mjs";
import { targetDirection } from "./accessor-direction.mjs";
import { hasToken, isAmbient } from "./declaration-facts.mjs";
import { declarationsForSymbol } from "./semantic-identity.mjs";

const { SymbolFlags, TypeFlags } = sync;
const {
  isArrowFunction,
  isAsExpression,
  isBinaryExpression,
  isCallExpression,
  isClassDeclaration,
  isConditionalExpression,
  isDoStatement,
  isElementAccessExpression,
  isForStatement,
  isFunctionDeclaration,
  isFunctionExpression,
  isIdentifier,
  isIfStatement,
  isMethodDeclaration,
  isNonNullExpression,
  isParenthesizedExpression,
  isPrefixUnaryExpression,
  isPropertyAccessExpression,
  isSatisfiesExpression,
  isSpreadElement,
  isStringLiteralLikeNode,
  isTypeAssertion,
  isVariableDeclaration,
  isWhileStatement,
} = ast;

const SCRIPT_EXTENSIONS = new Set([".ts", ".tsx", ".mts", ".cts", ".js", ".jsx", ".mjs", ".cjs"]);
const IDENTIFIER_TOKENS = /[A-Za-z_$][\w$]*/gu;
const MAX_UNION_MEMBERS = 64;
const MAYBE_UNDEFINED_FLAGS =
  TypeFlags.Undefined |
  TypeFlags.Void |
  TypeFlags.Any |
  TypeFlags.Unknown |
  TypeFlags.TypeParameter |
  TypeFlags.Index |
  TypeFlags.IndexedAccess |
  TypeFlags.Conditional |
  TypeFlags.Substitution;

const utf16Offset = (bytes, byteOffset) => bytes.subarray(0, byteOffset).toString("utf8").length;

const strictNullChecks = (project) => {
  const options = projectCompilerOptions(project);
  return (
    options.strictNullChecks === true ||
    (options.strictNullChecks === undefined && options.strict === true)
  );
};

const withoutWrappers = (node) => {
  let current = node;
  while (isParenthesizedExpression(current) || isSatisfiesExpression(current))
    current = current.expression;
  return current;
};

const isConstAssertion = (node) =>
  isAsExpression(node) &&
  node.type?.kind === SyntaxKind.TypeReference &&
  node.type.typeName?.text === "const";

// An assertion tells the checker a type that no code proves, so a value under
// one has no type evidence. `as const` only narrows a literal.
const containsAssertion = (node) => {
  if (
    isNonNullExpression(node) ||
    isTypeAssertion(node) ||
    (isAsExpression(node) && !isConstAssertion(node))
  ) {
    return true;
  }
  let found = false;
  node.forEachChild((child) => {
    found = found || containsAssertion(child);
    return found ? true : undefined;
  });
  return found;
};

// The checker can fail on the members of some generic types. No members means
// no proof.
const membersOf = (type) => {
  try {
    return type.getTypes?.() ?? [];
  } catch {
    return [];
  }
};

const maybeUndefined = (type, depth = 0) => {
  if (depth > 4) return true;
  if (type.flags & TypeFlags.Union) {
    const members = membersOf(type);
    if (members.length === 0 || members.length > MAX_UNION_MEMBERS) return true;
    return members.some((member) => maybeUndefined(member, depth + 1));
  }
  if (type.flags & TypeFlags.Intersection) {
    const members = membersOf(type);
    // An intersection with no known member proves nothing.
    if (members.length === 0) return true;
    return members.every((member) => maybeUndefined(member, depth + 1));
  }
  return type.flags === 0 || (type.flags & MAYBE_UNDEFINED_FLAGS) !== 0;
};

const JAVASCRIPT_EXTENSIONS = new Set([".js", ".jsx", ".mjs", ".cjs"]);

// A JavaScript file that the project does not check has inferred types that
// no code proves, so only the form of the argument counts there.
const isTypeChecked = (project, sourceFile) =>
  !JAVASCRIPT_EXTENSIONS.has(path.extname(sourceFile.fileName).toLowerCase()) ||
  projectCompilerOptions(project).checkJs === true;

// Expressions whose value is never `undefined`, whatever the types say.
const NEVER_UNDEFINED_KINDS = new Set([
  SyntaxKind.StringLiteral,
  SyntaxKind.NoSubstitutionTemplateLiteral,
  SyntaxKind.TemplateExpression,
  SyntaxKind.NumericLiteral,
  SyntaxKind.BigIntLiteral,
  SyntaxKind.RegularExpressionLiteral,
  SyntaxKind.TrueKeyword,
  SyntaxKind.FalseKeyword,
  SyntaxKind.NullKeyword,
  SyntaxKind.ObjectLiteralExpression,
  SyntaxKind.ArrayLiteralExpression,
  SyntaxKind.ArrowFunction,
  SyntaxKind.FunctionExpression,
  SyntaxKind.ClassExpression,
  SyntaxKind.NewExpression,
]);

// The evidence for one argument that needs no type, or `null` when the type
// decides. The second value is the expression whose type decides.
const syntacticArgument = (project, argument) => {
  const expression = withoutWrappers(argument);
  const unknown = { maybe_undefined: true };
  if (NEVER_UNDEFINED_KINDS.has(expression.kind)) return [{ maybe_undefined: false }, null];
  if (!isTypeChecked(project, expression.getSourceFile())) {
    return [{ ...unknown, reason: "unchecked-javascript" }, null];
  }
  if (containsAssertion(expression)) return [{ ...unknown, reason: "assertion" }, null];
  return [null, expression];
};

const typedArgument = (project, type) => {
  if (!type) return { maybe_undefined: true, reason: "no-type" };
  // Without `strictNullChecks`, a type drops `undefined`, so `T | undefined`
  // reads as `T`. Such a type proves nothing.
  if (!strictNullChecks(project)) return { maybe_undefined: true, reason: "not-strict" };
  return { maybe_undefined: maybeUndefined(type) };
};

const isConstDeclaration = (declaration) =>
  isVariableDeclaration(declaration) && (declaration.parent.flags & NodeFlags.Const) !== 0;

const MUTABLE_KINDS = new Set([
  SyntaxKind.VariableDeclaration,
  SyntaxKind.Parameter,
  SyntaxKind.BindingElement,
]);

// The ids of the symbols that code of a file writes, from one batched checker
// request. `unresolved` is true when a write target has no symbol.
const writesOf = (project, sourceFile, cache) => {
  if (cache.has(sourceFile.fileName)) return cache.get(sourceFile.fileName);
  const targets = [];
  const visit = (node) => {
    if (isIdentifier(node) && targetDirection(node) !== "read") targets.push(node);
    node.forEachChild(visit);
  };
  visit(sourceFile);
  const writes = { ids: new Set(), unresolved: false };
  if (targets.length > 0) {
    const symbols = project.checker.getSymbolAtLocation(targets);
    symbols.forEach((symbol) => {
      if (symbol) writes.ids.add(symbol.id);
      else writes.unresolved = true;
    });
  }
  cache.set(sourceFile.fileName, writes);
  return writes;
};

// True when a binding can hold a value that the narrowed type at a read does
// not show. The checker keeps a narrowing across a call, also when the call
// writes the binding in a closure, so a `let`, `var` or parameter that any
// code of the module writes has only its declared type as evidence.
const isWrittenBinding = (project, symbol, cache) => {
  const declarations = declarationsForSymbol(project, symbol);
  if (declarations.length !== 1) return declarations.length > 1;
  const [declaration] = declarations;
  if (!MUTABLE_KINDS.has(declaration.kind) || isConstDeclaration(declaration)) return false;
  const sourceFile = declaration.getSourceFile();
  if (sourceFile.externalModuleIndicator === undefined && sourceFile.isDeclarationFile !== true)
    return true;
  const writes = writesOf(project, sourceFile, cache);
  return writes.unresolved || writes.ids.has(symbol.id);
};

// The identifiers that an expression reads, whether it reads a member, and
// whether it reads an index.
const readsOf = (expression) => {
  const identifiers = [];
  let members = false;
  let indexes = false;
  let calls = false;
  let optional = false;
  const visit = (node) => {
    if (isFunctionLike(node)) return;
    if (isPropertyAccessExpression(node) || isElementAccessExpression(node)) members = true;
    if (isElementAccessExpression(node)) indexes = true;
    if (isCallExpression(node)) calls = true;
    if (node.questionDotToken !== undefined) optional = true;
    if (isIdentifier(node)) {
      const parent = node.parent;
      const isName =
        (isPropertyAccessExpression(parent) && parent.name === node) ||
        (parent?.kind === SyntaxKind.PropertyAssignment && parent.name === node);
      if (!isName) identifiers.push(node);
    }
    node.forEachChild(visit);
  };
  visit(expression);
  return { identifiers, members, indexes, calls, optional };
};

const uncheckedIndexes = (project) =>
  projectCompilerOptions(project).noUncheckedIndexedAccess !== true;

const strictPropertyInitialization = (project) => {
  const options = projectCompilerOptions(project);
  return (
    strictNullChecks(project) &&
    (options.strictPropertyInitialization === true ||
      (options.strictPropertyInitialization === undefined && options.strict === true))
  );
};

const MAX_VALUE_DEPTH = 4;
const TS_DIRECTIVE = /@ts-(?:ignore|expect-error|nocheck)\b/u;

// A `@ts-ignore`, `@ts-expect-error` or `@ts-nocheck` comment lets a file
// declare a type that its code does not satisfy.
const hasTsDirective = (sourceFile, context) => {
  const { fileName } = sourceFile;
  if (!context.directives.has(fileName))
    context.directives.set(fileName, TS_DIRECTIVE.test(sourceFile.text));
  return context.directives.get(fileName);
};

/**
 * Why the value of an expression can differ from its type, or null. The
 * check follows the bindings that the expression reads, a few levels deep:
 * an assertion, an `any` value, a `@ts-*` comment, a definite assignment, an
 * ambient declaration, and, without `noUncheckedIndexedAccess`, an index read
 * or a call result, whose type can drop `undefined`.
 */
const valueRisk = (project, expression, context, depth = 0) => {
  if (depth > MAX_VALUE_DEPTH) return "deep-value";
  if (containsAssertion(expression)) return "assertion";
  if (hasTsDirective(expression.getSourceFile(), context)) return "ts-directive";
  const reads = readsOf(expression);
  if (uncheckedIndexes(project) && reads.indexes) return "index-read";
  if (uncheckedIndexes(project) && reads.calls) return "call-result";
  for (const identifier of reads.identifiers) {
    const risk = identifierRisk(project, identifier, context, depth);
    if (risk) return risk;
  }
  return null;
};

const symbolOfIdentifier = (project, identifier, context) => {
  if (!context.symbols.has(identifier)) {
    context.symbols.set(identifier, project.checker.getSymbolAtLocation(identifier) ?? null);
  }
  return context.symbols.get(identifier);
};

// The symbol that an import alias names, so the check reads the declaration
// in the module that declares it.
const resolvedSymbol = (project, symbol) => {
  if ((symbol.flags & SymbolFlags.Alias) === 0) return symbol;
  const target = project.checker.getAliasedSymbol(symbol);
  return project.checker.isUnknownSymbol(target) ? null : target;
};

const identifierRisk = (project, identifier, context, depth) => {
  const found = symbolOfIdentifier(project, identifier, context);
  const symbol = found && resolvedSymbol(project, found);
  if (!symbol) return identifier.text === "undefined" ? "undefined" : "no-symbol";
  if (context.visited.has(symbol.id)) return null;
  context.visited.add(symbol.id);
  const declarations = declarationsForSymbol(project, symbol);
  for (const declaration of declarations) {
    const risk = declarationRisk(project, declaration, context, depth + 1);
    if (risk) return risk;
  }
  return null;
};

// Why the declared type of a declaration is not evidence of its value, or
// null. A destructured binding can read an index.
const declarationRisk = (project, declaration, context, depth) => {
  if (isAmbient(declaration)) return "ambient";
  if (hasTsDirective(declaration.getSourceFile(), context)) return "ts-directive";
  if (declaration.kind === SyntaxKind.BindingElement) {
    return uncheckedIndexes(project) ? "index-read" : null;
  }
  if (!isVariableDeclaration(declaration)) return null;
  if (declaration.exclamationToken !== undefined) return "definite-assignment";
  if (!declaration.initializer) return null;
  const type = project.checker.getTypeAtLocation(declaration.initializer);
  if (!type || (type.flags & (TypeFlags.Any | TypeFlags.Unknown)) !== 0) return "any-value";
  return valueRisk(project, declaration.initializer, context, depth);
};

const SET_MEMBER_KINDS = new Set([
  SyntaxKind.PropertySignature,
  SyntaxKind.GetAccessor,
  SyntaxKind.MethodDeclaration,
  SyntaxKind.MethodSignature,
  SyntaxKind.Parameter,
]);

// Why a member declaration does not hold a value of its declared type, or
// null. A class field without an initializer is `undefined` until code sets
// it, unless the checker enforces the set, and an object literal property
// holds the value of its initializer.
const memberRisk = (project, declaration, context) => {
  if (isAmbient(declaration)) return "ambient";
  if (hasTsDirective(declaration.getSourceFile(), context)) return "ts-directive";
  if (SET_MEMBER_KINDS.has(declaration.kind)) return null;
  if (declaration.kind === SyntaxKind.PropertyAssignment) {
    return valueRisk(project, declaration.initializer, context, 1);
  }
  if (declaration.kind === SyntaxKind.ShorthandPropertyAssignment) {
    return valueRisk(project, declaration.name, context, 1);
  }
  if (declaration.kind !== SyntaxKind.PropertyDeclaration) return "member-not-set";
  if (hasToken(declaration, SyntaxKind.ExclamationToken)) return "member-not-set";
  if (declaration.initializer) return valueRisk(project, declaration.initializer, context, 1);
  return strictPropertyInitialization(project) ? null : "member-not-set";
};

// True for `this.x` in a constructor or a field initializer, where a field
// can still be unset.
const readsUnsetThis = (expression) => {
  if (expression.expression?.kind !== SyntaxKind.ThisKeyword) return false;
  for (let node = expression.parent; node; node = node.parent) {
    if (node.kind === SyntaxKind.Constructor || node.kind === SyntaxKind.PropertyDeclaration)
      return true;
    if (isFunctionLike(node) || node.kind === SyntaxKind.ClassDeclaration) return false;
  }
  return false;
};

const noEvidence = (reason) => ({ maybe_undefined: true, reason });

// Why a binding gives no evidence at a call, `written` when only its declared
// type is evidence, or null when the type at the call is evidence.
const bindingReason = (project, identifier, context) => {
  const risk = identifierRisk(project, identifier, context, 0);
  if (risk) return risk;
  const symbol = symbolOfIdentifier(project, identifier, context);
  return isWrittenBinding(project, symbol, context.writes) ? "written" : null;
};

const blockingRead = (project, plan, context) => {
  for (const identifier of plan.identifiers) {
    context.visited = new Set();
    const reason = bindingReason(project, identifier, context);
    if (reason === "written")
      return plan.expression === identifier ? "declared" : "written-binding";
    if (reason) return reason;
  }
  return null;
};

const memberEvidence = (project, expression, context) => {
  if (readsOf(expression).optional) return { reason: "optional-chain" };
  if (readsUnsetThis(expression)) return { reason: "member-not-set" };
  const member = project.checker.getSymbolAtLocation(expression.name);
  if (!member) return { reason: "no-symbol" };
  context.visited = new Set();
  const declarations = declarationsForSymbol(project, member);
  if (declarations.length === 0) return { reason: "member-not-set" };
  for (const declaration of declarations) {
    const risk = memberRisk(project, declaration, context);
    if (risk) return { reason: risk };
  }
  context.visited = new Set();
  const receiver = valueRisk(project, expression.expression, context, 1);
  return receiver ? { reason: receiver } : { declared: member };
};

// Where the evidence of one argument comes from: the type at the call, the
// declared type of a symbol, or nowhere, with a reason.
const planEvidence = (project, plan, context) => {
  const { expression } = plan;
  if (hasTsDirective(expression.getSourceFile(), context)) return { reason: "ts-directive" };
  if (isPropertyAccessExpression(expression)) return memberEvidence(project, expression, context);
  if (plan.members) return { reason: "member-read" };
  if (uncheckedIndexes(project) && plan.calls) return { reason: "call-result" };
  const blocked = blockingRead(project, plan, context);
  if (blocked === "declared") return { declared: symbolOfIdentifier(project, expression, context) };
  return blocked ? { reason: blocked } : { atCall: true };
};

// A user-defined type guard (`x is T`, `asserts x is T`) is a claim that no
// code checks. True when a call in the function around `identifier` passes
// the same name to such a guard.
const guardedByPredicate = (project, identifier) => {
  let scope = identifier.parent;
  while (scope && !isFunctionLike(scope) && scope.kind !== SyntaxKind.SourceFile)
    scope = scope.parent;
  if (!scope) return false;
  let guarded = false;
  const visit = (node) => {
    if (guarded) return;
    if (
      isCallExpression(node) &&
      (node.arguments ?? []).some(
        (argument) => isIdentifier(argument) && argument.text === identifier.text,
      )
    ) {
      const signature = project.checker.getResolvedSignature(node);
      if (signature && project.checker.getTypePredicateOfSignature(signature)) guarded = true;
    }
    node.forEachChild(visit);
  };
  visit(scope);
  return guarded;
};

// The evidence for each argument of a list. A bare identifier of a binding
// that the module never writes, and an expression whose reads are all such
// bindings, use the type at the call. A written binding uses its declared
// type. A bare member read uses the declared type of the member, because
// code can change a member through another reference. Each other expression
// with a member read has no evidence.
const typeArguments = (project, argumentNodes) => {
  const results = Array.from({ length: argumentNodes.length });
  const plans = [];
  argumentNodes.forEach((argument, index) => {
    const [result, expression] = syntacticArgument(project, argument);
    if (result) results[index] = result;
    else plans.push({ index, expression, ...readsOf(expression) });
  });
  const identifiers = plans.flatMap((plan) => plan.identifiers);
  const symbols = identifiers.length === 0 ? [] : project.checker.getSymbolAtLocation(identifiers);
  const context = {
    symbols: new Map(identifiers.map((identifier, at) => [identifier, symbols[at] ?? null])),
    writes: new Map(),
    directives: new Map(),
    visited: new Set(),
  };
  const atCall = [];
  for (const plan of plans) {
    const source = planEvidence(project, plan, context);
    if (source.atCall) atCall.push(plan);
    else if (source.declared) {
      results[plan.index] = typedArgument(
        project,
        project.checker.getTypeOfSymbol(source.declared),
      );
    } else results[plan.index] = noEvidence(source.reason ?? "no-symbol");
  }
  if (atCall.length > 0) {
    const types = project.checker.getTypeAtLocation(atCall.map((plan) => plan.expression));
    atCall.forEach((plan, at) => {
      const evidence = typedArgument(project, types[at]);
      const narrowed = !evidence.maybe_undefined && isIdentifier(plan.expression);
      results[plan.index] =
        narrowed && guardedByPredicate(project, plan.expression)
          ? noEvidence("type-predicate")
          : evidence;
    });
  }
  return results;
};

// The call expression at each start. Chained calls such as `f(x).g()` share a
// start, so the deepest call at a start is the call of its callee.
const callsByStart = (sourceFile, starts) => {
  const found = new Map();
  const visit = (node) => {
    if (isCallExpression(node)) {
      const start = node.getStart(sourceFile);
      if (starts.has(start)) found.set(start, node);
    }
    node.forEachChild(visit);
  };
  visit(sourceFile);
  return found;
};

// The programs of the request. A project object belongs to the snapshot that
// made it, so each snapshot stays open until the request ends.
class Programs {
  constructor(root, createApi) {
    this.api = createApi({ cwd: root });
    this.snapshots = [];
    this.projects = new Map();
    this.byFile = new Map();
  }

  track(snapshot) {
    this.snapshots.push(snapshot);
    return snapshot;
  }

  defaultConfig(absolutePath) {
    this.probes = (this.probes ?? 0) + 1;
    const probe = this.track(this.api.updateSnapshot({ openFiles: [absolutePath] }));
    const project = probe.getDefaultProjectForFile(absolutePath);
    const config = project && !isInferredProject(project) ? project.configFileName : null;
    this.track(this.api.updateSnapshot({ closeFiles: [absolutePath] }));
    return config;
  }

  open(config) {
    if (!this.projects.has(config)) {
      const started = performance.now();
      const snapshot = this.track(this.api.updateSnapshot({ openProjects: [config] }));
      const project = snapshot.getProject(config) ?? null;
      this.projects.set(
        config,
        project && { project, files: new Set(project.program.getSourceFileNames()) },
      );
      this.openMs = (this.openMs ?? 0) + (performance.now() - started);
    }
    return this.projects.get(config);
  }

  /** The open program that holds a file, with its file names. */
  entry(absolutePath) {
    if (this.byFile.has(absolutePath)) return this.byFile.get(absolutePath);
    let found = null;
    // The checker parses only script files. It fails on a component file.
    if (SCRIPT_EXTENSIONS.has(path.extname(absolutePath).toLowerCase())) {
      for (const entry of this.projects.values()) {
        if (entry?.files.has(absolutePath)) {
          found = entry;
          break;
        }
      }
      if (found === null) {
        const config = this.defaultConfig(absolutePath);
        const entry = config === null ? null : this.open(config);
        found = entry?.files.has(absolutePath) ? entry : null;
      }
    }
    this.byFile.set(absolutePath, found);
    return found;
  }

  project(absolutePath) {
    return this.entry(absolutePath)?.project ?? null;
  }

  close() {
    for (const snapshot of this.snapshots.toReversed()) snapshot.dispose();
    this.api.close();
  }
}

const readBytes = (absolutePath) => {
  try {
    return readFileSync(absolutePath);
  } catch {
    return null;
  }
};

const unchecked = (request, reason) => ({ id: request.id, outcome: "unchecked", reason });

// The evidence of each requested argument of one file.
const fileArgumentEvidence = (programs, absolutePath, requests) => {
  const project = programs.project(absolutePath);
  if (!project) return requests.map((request) => unchecked(request, "no-project"));
  const bytes = readBytes(absolutePath);
  if (!bytes) return requests.map((request) => unchecked(request, "file-missing"));
  const sourceFile = project.program.getSourceFile(absolutePath);
  const starts = new Map(
    requests.map((request) => [request, utf16Offset(bytes, request.call_start)]),
  );
  const calls = callsByStart(sourceFile, new Set(starts.values()));
  const results = new Map();
  const nodes = [];
  for (const request of requests) {
    const call = calls.get(starts.get(request));
    const args = call?.arguments ?? [];
    if (!call) results.set(request, unchecked(request, "call-not-found"));
    else if (args.slice(0, request.index + 1).some(isSpreadElement))
      results.set(request, unchecked(request, "spread"));
    else if (!args[request.index]) results.set(request, unchecked(request, "argument-not-found"));
    else nodes.push([request, args[request.index]]);
  }
  const typed = typeArguments(
    project,
    nodes.map(([, node]) => node),
  );
  nodes.forEach(([request], index) =>
    results.set(request, { id: request.id, outcome: "typed", ...typed[index] }),
  );
  return requests.map((request) => results.get(request));
};

const typeRound = (programs, root, args) => {
  const byFile = new Map();
  for (const request of args) {
    const absolutePath = path.resolve(root, request.path);
    if (!byFile.has(absolutePath)) byFile.set(absolutePath, []);
    byFile.get(absolutePath).push(request);
  }
  const results = new Map();
  for (const [absolutePath, requests] of byFile) {
    let evidence;
    try {
      evidence = fileArgumentEvidence(programs, absolutePath, requests);
    } catch {
      evidence = requests.map((request) => unchecked(request, "checker-error"));
    }
    evidence.forEach((result) => results.set(result.id, result));
  }
  return results;
};

/**
 * The evidence of each default candidate: `defined` when no argument can be
 * `undefined`. The first round types one argument of each candidate, the
 * second round the rest of the candidates that the first round keeps, so a
 * candidate that one argument rules out costs one checker request.
 */
const defaultsEvidence = (programs, root, defaults) => {
  const state = new Map(defaults.map((candidate) => [candidate.id, { outcome: "defined" }]));
  const args = defaults.flatMap((candidate) =>
    candidate.arguments.map((argument, at) => ({
      ...argument,
      id: `${candidate.id}:${at}`,
      group: candidate.id,
    })),
  );
  const first = new Set();
  const rounds = [[], []];
  for (const request of args) {
    if (first.has(request.group)) rounds[1].push(request);
    else {
      first.add(request.group);
      rounds[0].push(request);
    }
  }
  for (const round of rounds) {
    const pending = round.filter((request) => state.get(request.group).outcome === "defined");
    const byId = new Map(pending.map((request) => [request.id, request]));
    for (const [id, result] of typeRound(programs, root, pending)) {
      const group = state.get(byId.get(id).group);
      if (group.outcome !== "defined") continue;
      if (result.outcome !== "typed")
        state.set(byId.get(id).group, { outcome: "unchecked", reason: result.reason });
      else if (result.maybe_undefined)
        state.set(byId.get(id).group, { outcome: "maybe-undefined" });
    }
  }
  return defaults.map((candidate) => ({ id: candidate.id, ...state.get(candidate.id) }));
};

const lineCol = (sourceFile, position) => {
  const text = sourceFile.text;
  const lineStart = text.lastIndexOf("\n", position - 1) + 1;
  let line = 1;
  for (let at = text.indexOf("\n"); at !== -1 && at < position; at = text.indexOf("\n", at + 1))
    line += 1;
  return { line, col: Buffer.byteLength(text.slice(lineStart, position), "utf8") };
};

const isFunctionLike = (node) =>
  isFunctionDeclaration(node) ||
  isFunctionExpression(node) ||
  isArrowFunction(node) ||
  isMethodDeclaration(node);

const conditionTest = (node) => {
  if (
    isIfStatement(node) ||
    isWhileStatement(node) ||
    isDoStatement(node) ||
    isConditionalExpression(node)
  ) {
    return isConditionalExpression(node) ? node.condition : node.expression;
  }
  if (isForStatement(node)) return node.condition;
  return undefined;
};

const testsName = (test, name) => {
  let current = test && withoutWrappers(test);
  while (
    current &&
    isPrefixUnaryExpression(current) &&
    current.operator === SyntaxKind.ExclamationToken
  ) {
    current = withoutWrappers(current.operand);
  }
  return Boolean(current && isIdentifier(current) && current.text === name);
};

const ASSIGNMENT_OPERATORS = new Set([
  SyntaxKind.EqualsToken,
  SyntaxKind.PlusEqualsToken,
  SyntaxKind.MinusEqualsToken,
  SyntaxKind.AsteriskEqualsToken,
  SyntaxKind.SlashEqualsToken,
  SyntaxKind.QuestionQuestionEqualsToken,
  SyntaxKind.BarBarEqualsToken,
  SyntaxKind.AmpersandAmpersandEqualsToken,
  SyntaxKind.PercentEqualsToken,
]);

const isIncrement = (node) =>
  (node.kind === SyntaxKind.PrefixUnaryExpression ||
    node.kind === SyntaxKind.PostfixUnaryExpression) &&
  (node.operator === SyntaxKind.PlusPlusToken || node.operator === SyntaxKind.MinusMinusToken);

// The parameter names that one node writes. A destructuring target can
// write each name.
const writtenNames = (node, names) => {
  if (isBinaryExpression(node) && ASSIGNMENT_OPERATORS.has(node.operatorToken.kind)) {
    const target = withoutWrappers(node.left);
    if (isIdentifier(target)) return names.includes(target.text) ? [target.text] : [];
    return isPropertyAccessExpression(target) || isElementAccessExpression(target) ? [] : names;
  }
  if (isIncrement(node)) {
    const operand = withoutWrappers(node.operand);
    return isIdentifier(operand) && names.includes(operand.text) ? [operand.text] : [];
  }
  return [];
};

// Syntactic facts of the body: condition reads and writes of each parameter
// name, and a read of `arguments`. A nested function with the same name
// counts too, so the facts can only be too careful.
const bodyFacts = (method, names) => {
  const facts = {
    arguments: false,
    reads: new Map(names.map((name) => [name, 0])),
    written: new Set(),
  };
  const visit = (node) => {
    if (isIdentifier(node) && node.text === "arguments") facts.arguments = true;
    const test = conditionTest(node);
    for (const name of names)
      if (testsName(test, name)) facts.reads.set(name, facts.reads.get(name) + 1);
    for (const name of writtenNames(node, names)) facts.written.add(name);
    node.forEachChild(visit);
  };
  if (method.body) visit(method.body);
  for (const parameter of method.parameters)
    if (parameter.initializer) visit(parameter.initializer);
  return facts;
};

const hasDecorators = (node) =>
  (node.modifiers ?? []).some((modifier) => modifier.kind === SyntaxKind.Decorator) ||
  (node.decorators?.length ?? 0) > 0;

const methodParams = (sourceFile, method) => {
  const params = method.parameters.filter(
    (parameter) => !(isIdentifier(parameter.name) && parameter.name.text === "this"),
  );
  const names = params
    .filter((parameter) => isIdentifier(parameter.name))
    .map((parameter) => parameter.name.text);
  const facts = bodyFacts(method, names);
  return {
    readsArguments: facts.arguments,
    params: params.map((parameter, index) => {
      const identifier = isIdentifier(parameter.name);
      const name = identifier ? parameter.name.text : null;
      return {
        index,
        name,
        pattern: !identifier,
        rest: parameter.dotDotDotToken !== undefined,
        optional: parameter.questionToken !== undefined,
        has_default: parameter.initializer !== undefined,
        written: name !== null && facts.written.has(name),
        condition_reads: name === null ? 0 : facts.reads.get(name),
        decorated: hasDecorators(parameter),
        ...lineCol(sourceFile, parameter.name.getStart(sourceFile)),
      };
    }),
  };
};

// The callee of a call, without parentheses, must be the reference itself.
const calleeIsReference = (call, name) => {
  const callee = withoutWrappers(call.expression);
  if (isPropertyAccessExpression(callee))
    return callee.name === name || callee.name.pos === name.pos;
  if (isElementAccessExpression(callee)) return callee.argumentExpression?.pos === name.pos;
  return callee.pos === name.pos;
};

const isPassThrough = (project, argument, method) => {
  const expression = withoutWrappers(argument);
  if (!isIdentifier(expression)) return null;
  if (expression.pos < method.pos || expression.end > method.end) return null;
  const symbol = project.checker.getSymbolAtLocation(expression);
  const declaration = declarationsForSymbol(project, symbol)[0];
  if (!declaration || declaration.parent !== method) return null;
  return method.parameters.indexOf(declaration);
};

// The evidence of one call. An argument that needs a type is `null`, and
// its node goes to `pending` for one batched checker request.
const callEvidence = (project, method, call, name, pending) => {
  const sourceFile = call.getSourceFile();
  const args = call.arguments ?? [];
  const spread = args.findIndex(isSpreadElement);
  const evidence = {
    path: sourceFile.fileName,
    ...lineCol(sourceFile, name.getStart(sourceFile)),
    arg_count: args.length,
    spread_from: spread === -1 ? null : spread,
    args: [],
  };
  evidence.args = args.map((argument, index) => {
    if (spread !== -1 && index >= spread) return { maybe_undefined: true };
    const through = isPassThrough(project, argument, method);
    if (through !== null) return { maybe_undefined: true, pass_through: through };
    pending.push(argument);
    return null;
  });
  return evidence;
};

// The module-scope class of the name. An exported class is a statement of
// the module, and a local class of the same name in a function is another
// class.
const declarationOfMethod = (sourceFile, className, methodName, start) => {
  const classes = sourceFile.statements.filter(
    (statement) => isClassDeclaration(statement) && statement.name?.text === className,
  );
  if (classes.length !== 1) return { classNode: null, methods: [] };
  const [classNode] = classes;
  const methods = classNode.members.filter(
    (member) =>
      isMethodDeclaration(member) &&
      member.name &&
      (member.name.text ?? member.name.escapedText) === methodName,
  );
  return { classNode, methods, start };
};

// A member of a base type or an interface outside the program, such as a
// framework base class, is called by code that the program does not hold.
const inheritsExternalMember = (project, classNode, methodName) => {
  for (const clause of classNode.heritageClauses ?? []) {
    for (const heritage of clause.types) {
      const type = project.checker.getTypeAtLocation(heritage);
      const property = type && project.checker.getPropertyOfType(type, methodName);
      if (!property) continue;
      const external = (property.declarations ?? []).some(
        (handle) => /\.d\.[cm]?ts$/u.test(handle.path) || handle.path.includes("/node_modules/"),
      );
      if (external || (property.declarations ?? []).length === 0) return true;
    }
  }
  return false;
};

/** Each identifier token of a text, for a cheap first filter by name. */
const tokensOf = (text) => new Set(text.match(IDENTIFIER_TOKENS) ?? []);

const COMPUTED_CALL = /\]\s*(?:\?\.\s*)?\(/u;
// A computed access with a key that is not a string or a number, or a
// reflective read. Each can read a method that no reference names.
const COMPUTED_ACCESS = /[\w$)\]!]\s*(?:\?\.\s*)?\[\s*[A-Za-z_$(]|\bReflect\./u;
const REFLECT_CALL = /\bReflect\.(?:get|apply|construct)\s*\(/u;
const MEMBER_NAME = /(?:\?\.|\.)\s*([A-Za-z_$][\w$]*)/gu;
const STRING_KEY = /\[\s*(['"`])([A-Za-z_$][\w$]*)\1\s*\]/gu;
const STRING_NAME = /(['"`])([A-Za-z_$][\w$]*)\1/gu;
const OPEN_BRACKET = /([A-Za-z_$][\w$]*|[)\]!])\s*(?:\?\.\s*)?\[/gu;
const NOT_A_RECEIVER = new Set([
  "return",
  "case",
  "in",
  "of",
  "typeof",
  "void",
  "yield",
  "await",
  "new",
  "delete",
  "throw",
  "else",
  "do",
  "instanceof",
  "extends",
  "export",
  "default",
  "const",
  "let",
  "var",
]);
const CALL_AFTER = /^\s*(?:\?\.\s*)?(?:\(|\.\s*(?:call|apply|bind)\b)/u;

// The index after the `]` that closes the `[` at `open`, or -1.
const closingBracket = (text, open) => {
  let depth = 0;
  for (let at = open; at < text.length; at += 1) {
    const char = text[at];
    if (char === "[") depth += 1;
    else if (char === "]") {
      depth -= 1;
      if (depth === 0) return at + 1;
    } else if (char === "\n" && at - open > 400) return -1;
  }
  return -1;
};

/**
 * The computed member accesses of a text, found without a parse: `x[key]`
 * where `key` is not a string or a number. A match in a comment or a string
 * only makes the scan more careful.
 */
const computedAccesses = (text) => {
  const accesses = [];
  for (const match of text.matchAll(OPEN_BRACKET)) {
    const receiver = match[1];
    if (NOT_A_RECEIVER.has(receiver)) continue;
    const open = match.index + match[0].length - 1;
    const next = text.slice(open + 1).match(/^\s*(.)/su)?.[1];
    if (next === undefined || next === "]" || /['"`\d]/u.test(next)) continue;
    const close = closingBracket(text, open);
    const called = close !== -1 && CALL_AFTER.test(text.slice(close, close + 40));
    accesses.push({ position: /[)\]!]/u.test(receiver) ? null : match.index, called });
  }
  return accesses;
};

const memberPositions = (text, wanted) => {
  const positions = [];
  for (const match of text.matchAll(MEMBER_NAME)) {
    if (wanted.has(match[1]))
      positions.push([match[1], match.index + match[0].length - match[1].length]);
  }
  for (const match of text.matchAll(STRING_KEY)) {
    if (wanted.has(match[2])) positions.push([match[2], match.index + match[0].indexOf(match[1])]);
  }
  return positions;
};

const isUntypedType = (type) =>
  !type || type.flags === 0 || (type.flags & (TypeFlags.Any | TypeFlags.Unknown)) !== 0;

/** The scan state of one program. */
const programRisks = () => ({
  hard: new Map(),
  externalMembers: new Map(),
  prototypes: new Set(),
  strings: new Set(),
  untypedComputedCalls: 0,
  properties: new Map(),
  references: new Map(),
});

const addRisk = (risks, name, reason) => {
  if (!risks.hard.has(name)) risks.hard.set(name, reason);
};

// The checks of one file inside the program of the declarations.
const scanProgramFile = (entry, risks, absolutePath, text, named, names) => {
  const { checker } = entry.project;
  const positions = memberPositions(text, new Set(named));
  if (positions.length > 0) {
    const symbols = checker.getSymbolAtPosition(
      absolutePath,
      positions.map(([, position]) => position),
    );
    positions.forEach(([name, position], index) => {
      const symbol = symbols[index];
      if (!symbol) {
        addRisk(risks, name, "unresolved-reference");
        return;
      }
      if (!risks.references.has(name)) risks.references.set(name, []);
      risks.references.get(name).push({ file: absolutePath, position, symbol });
    });
  }
  const accesses = computedAccesses(text);
  const typed = accesses.filter((access) => access.position !== null);
  const types =
    typed.length === 0
      ? []
      : checker.getTypeAtPosition(
          absolutePath,
          typed.map((access) => access.position),
        );
  typed.forEach((access, index) =>
    recordComputedAccess(checker, risks, names, types[index], access.called),
  );
  if (accesses.some((access) => access.position === null)) {
    recordComplexReceivers(entry.project, risks, names, absolutePath);
  }
  if (REFLECT_CALL.test(text)) risks.untypedComputedCalls += 1;
};

// A computed access on a receiver that is not an identifier, such as
// `get()[key]` or `(x as T)[key]`, needs the parse to find the receiver.
const recordComplexReceivers = (project, risks, names, absolutePath) => {
  const sourceFile = project.program.getSourceFile(absolutePath);
  if (!sourceFile) return;
  const accesses = [];
  const visit = (node) => {
    if (
      isElementAccessExpression(node) &&
      node.argumentExpression &&
      !isStringLiteralLikeNode(node.argumentExpression) &&
      node.argumentExpression.kind !== SyntaxKind.NumericLiteral &&
      !isIdentifier(node.expression)
    ) {
      accesses.push(node);
    }
    node.forEachChild(visit);
  };
  visit(sourceFile);
  if (accesses.length === 0) return;
  const types = project.checker.getTypeAtLocation(accesses.map((access) => access.expression));
  accesses.forEach((access, index) => {
    const called = isCalledAccess(access);
    recordComputedAccess(project.checker, risks, names, types[index], called);
  });
};

const isCalledAccess = (access) => {
  let current = access;
  while (isParenthesizedExpression(current.parent)) current = current.parent;
  const { parent } = current;
  if (isCallExpression(parent) && parent.expression === current) return true;
  return (
    isPropertyAccessExpression(parent) &&
    ["call", "apply", "bind"].includes(parent.name.text) &&
    parent.expression === current
  );
};

// A computed access on a typed receiver blocks each method of the type that
// the access can read. A called access on an untyped receiver counts for the
// string policy.
const recordComputedAccess = (checker, risks, names, type, called) => {
  if (isUntypedType(type)) {
    if (called) risks.untypedComputedCalls += 1;
    return;
  }
  if (!risks.properties.has(type.id)) {
    let properties = [];
    try {
      properties = checker.getPropertiesOfType(type).map((property) => property.name);
    } catch {
      properties = null;
    }
    risks.properties.set(type.id, properties);
  }
  const properties = risks.properties.get(type.id);
  if (properties === null || properties.length === 0) {
    if (called) risks.untypedComputedCalls += 1;
    return;
  }
  for (const property of properties) {
    if (names.has(property)) addRisk(risks, property, "dynamic-member-access");
  }
};

/**
 * The names that each program cannot attribute, keyed by program. A name is
 * blocked when a file outside the program names it, when a reference by name
 * does not resolve, or when a computed access on a typed receiver can read it.
 * `strings` holds each name that a string literal spells, and
 * `untypedComputedCalls` counts the computed calls on a receiver with no
 * known members. Such a call can reach each method whose name a string holds.
 */
const namesByProgram = (programs, root, classes) => {
  const namesByEntry = new Map();
  const classNames = new Set();
  for (const request of classes) {
    const entry = programs.entry(path.resolve(root, request.path));
    if (!entry) continue;
    if (!namesByEntry.has(entry)) namesByEntry.set(entry, new Set());
    request.methods.forEach((method) => namesByEntry.get(entry).add(method.name));
    classNames.add(request.class);
  }
  return { namesByEntry, classNames };
};

const PROTOTYPE_READ = /\b([A-Za-z_$][\w$]*)\s*\.\s*prototype\b/gu;

// A read of `Class.prototype` can reach each method by reflection.
const recordPrototypes = (risks, classNames, text) => {
  for (const match of text.matchAll(PROTOTYPE_READ)) {
    if (!classNames.has(match[1])) continue;
    for (const state of risks.values()) state.prototypes.add(match[1]);
  }
};

// The checks of one file for the program of one class candidate.
const scanEntryFile = (entry, state, names, absolutePath, text, tokens) => {
  const named = [...names].filter((name) => tokens.has(name));
  if (!entry.files.has(absolutePath)) {
    named.forEach((name) => addRisk(state, name, "outside-program"));
    if (COMPUTED_CALL.test(text) || REFLECT_CALL.test(text)) state.untypedComputedCalls += 1;
    return;
  }
  try {
    scanProgramFile(entry, state, absolutePath, text, named, names);
  } catch {
    named.forEach((name) => addRisk(state, name, "checker-error"));
  }
};

const recordStrings = (risks, allNames, text) => {
  for (const match of text.matchAll(STRING_NAME)) {
    if (!allNames.has(match[2])) continue;
    for (const state of risks.values()) state.strings.add(match[2]);
  }
};

// The checks of one project file for each program of a class candidate.
const scanRiskFile = (scan, risks, allNames, absolutePath, text) => {
  const { namesByEntry, classNames } = scan;
  const tokens = tokensOf(text);
  if (![...allNames].some((name) => tokens.has(name)) && !COMPUTED_ACCESS.test(text)) return;
  recordStrings(risks, allNames, text);
  recordPrototypes(risks, classNames, text);
  for (const [entry, names] of namesByEntry) {
    scanEntryFile(entry, risks.get(entry), names, absolutePath, text, tokens);
  }
};

const escapeRegExp = (text) => text.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&");

const DECLARATION_FILE = /\.d\.[cm]?ts$/u;
const MAX_EXTERNAL_MEMBERS = 200;

// The member declarations of each method name in the declaration files of
// a program, outside the TypeScript lib. Code of a dependency can call such a
// member on a value of a class that matches the owner type.
const recordExternalMembers = (entry, state, names) => {
  const pattern = new RegExp(
    `(?:^|[\\s;{,(])(${[...names].map(escapeRegExp).join("|")})\\s*\\??\\s*[(<:]`,
    "gu",
  );
  for (const fileName of entry.files) {
    // The TypeScript lib files hold the platform types, whose callers the
    // protocol names cover.
    if (!DECLARATION_FILE.test(fileName) || /^lib\.[\w.]*\.d\.ts$/u.test(path.basename(fileName)))
      continue;
    const bytes = readBytes(fileName);
    if (!bytes) continue;
    const text = bytes.toString("utf8");
    const positions = [...text.matchAll(pattern)].map((match) => [
      match[1],
      match.index + match[0].lastIndexOf(match[1]),
    ]);
    if (positions.length === 0) continue;
    let symbols;
    try {
      symbols = entry.project.checker.getSymbolAtPosition(
        fileName,
        positions.map(([, position]) => position),
      );
    } catch {
      positions.forEach(([name]) => addRisk(state, name, "external-structural"));
      continue;
    }
    positions.forEach(([name], index) => {
      if (!state.externalMembers.has(name)) state.externalMembers.set(name, new Map());
      const members = state.externalMembers.get(name);
      const symbol = symbols[index];
      if (symbol) members.set(symbol.id, symbol);
      if (members.size > MAX_EXTERNAL_MEMBERS) addRisk(state, name, "external-structural");
    });
  }
};

const nameRisks = (programs, root, files, classes) => {
  const scan = namesByProgram(programs, root, classes);
  const { namesByEntry } = scan;
  const risks = new Map([...namesByEntry.keys()].map((entry) => [entry, programRisks()]));
  if (risks.size === 0) return risks;
  for (const [entry, names] of namesByEntry) recordExternalMembers(entry, risks.get(entry), names);
  const allNames = new Set([...namesByEntry.values()].flatMap((names) => [...names]));
  for (const file of files) {
    const absolutePath = path.resolve(root, file);
    if (/\.d\.[cm]?ts$/u.test(absolutePath)) continue;
    const bytes = readBytes(absolutePath);
    if (bytes) scanRiskFile(scan, risks, allNames, absolutePath, bytes.toString("utf8"));
  }
  return risks;
};

const OWNER_KINDS = new Set([
  SyntaxKind.ClassDeclaration,
  SyntaxKind.ClassExpression,
  SyntaxKind.InterfaceDeclaration,
  SyntaxKind.TypeLiteral,
  SyntaxKind.ObjectLiteralExpression,
]);

const ownerType = (project, member) => {
  const owner = member?.parent;
  if (!owner || !OWNER_KINDS.has(owner.kind)) return null;
  if (owner.name && owner.kind !== SyntaxKind.ObjectLiteralExpression) {
    const symbol = project.checker.getSymbolAtLocation(owner.name);
    return symbol ? project.checker.getDeclaredTypeOfSymbol(symbol) : null;
  }
  return project.checker.getTypeAtLocation(owner) ?? null;
};

const usagePositions = (project, usages) => {
  const known = new Set();
  for (const usage of usages) {
    const name = usage.name.resolve(project);
    if (name) known.add(`${name.getSourceFile().fileName}\u0000${name.getStart()}`);
  }
  return known;
};

const classTypeOf = (project, classNode, declaration) => {
  const classSymbol = project.checker.getSymbolAtLocation(classNode.name);
  if (!classSymbol) return null;
  const isStatic = (declaration.modifiers ?? []).some(
    (modifier) => modifier.kind === SyntaxKind.StaticKeyword,
  );
  return isStatic
    ? project.checker.getTypeOfSymbol(classSymbol)
    : project.checker.getDeclaredTypeOfSymbol(classSymbol);
};

// The reason for one member symbol of the same name, or null when a value of
// the class cannot stand behind it.
const memberAlias = (project, symbol, declaration, classType) => {
  const members = declarationsForSymbol(project, symbol);
  if (members.length === 0) return "unresolved-member";
  for (const member of members) {
    if (member === declaration) continue;
    const type = ownerType(project, member);
    if (!type || project.checker.isTypeAssignableTo(classType, type)) return "structural-alias";
  }
  return null;
};

// A member of the same name in a dependency type that the class matches. A
// dependency can receive an instance and call the method through that type.
const externalStructural = (project, classNode, declaration, members) => {
  if (!members || members.size === 0) return null;
  const classType = classTypeOf(project, classNode, declaration);
  if (!classType) return "unresolved-class";
  for (const symbol of members.values()) {
    if (memberAlias(project, symbol, declaration, classType)) return "external-structural";
  }
  return null;
};

/**
 * A reason when a reference by the method name names another member that a
 * value of the class can stand behind. TypeScript types are structural, so
 * code can pass an instance as a type that the class never `implements`, and
 * call the method through that type. The checker does not link such a call to
 * the method.
 */
const structuralAlias = (project, classNode, declaration, references, usages) => {
  const known = usagePositions(project, usages);
  const classType = classTypeOf(project, classNode, declaration);
  if (!classType) return "unresolved-class";
  const checked = new Map();
  for (const reference of references) {
    if (known.has(`${reference.file}\u0000${reference.position}`)) continue;
    const { id } = reference.symbol;
    if (!checked.has(id))
      checked.set(id, memberAlias(project, reference.symbol, declaration, classType));
    if (checked.get(id)) return checked.get(id);
  }
  return null;
};

const abstained = (reason) => ({ outcome: "abstained", reason });

// The declaration of a method, or the reason that the method has no
// closed set of calls by its form.
// Methods that the language or the platform calls by name: a promise
// `then`, a string or JSON conversion, an iterator step, an event listener.
const PROTOCOL_METHODS = new Set([
  "then",
  "catch",
  "finally",
  "toString",
  "toLocaleString",
  "valueOf",
  "toJSON",
  "handleEvent",
  "next",
  "return",
  "throw",
  "constructor",
]);

const methodDeclaration = (project, sourceFile, classRequest, name) => {
  if (PROTOCOL_METHODS.has(name)) return { reason: "protocol-method" };
  const { classNode, methods } = declarationOfMethod(sourceFile, classRequest.class, name);
  if (!classNode) return { reason: "class-not-found" };
  if (hasDecorators(classNode)) return { reason: "decorator" };
  if (methods.length !== 1 || !methods[0].body)
    return { reason: methods.length > 1 ? "overload" : "no-body" };
  const [declaration] = methods;
  if (hasDecorators(declaration)) return { reason: "decorator" };
  if (declaration.name.kind !== SyntaxKind.Identifier) return { reason: "computed-name" };
  if (declaration.asteriskToken) return { reason: "generator" };
  if (inheritsExternalMember(project, classNode, name)) return { reason: "external-contract" };
  return { classNode, declaration };
};

const isMemberDeclarationName = (name) =>
  isMethodDeclaration(name.parent) || name.parent?.kind === SyntaxKind.MethodSignature;

// The calls of a method, or the reason that a reference is not a call.
const methodCalls = (project, sourceFile, declaration, usages) => {
  const calls = [];
  const pending = [];
  for (const usage of usages) {
    const name = usage.name.resolve(project);
    if (!name) return { reason: "unresolved-usage" };
    if (name.pos === declaration.name.pos && name.getSourceFile() === sourceFile) continue;
    const call = usage.call?.resolve(project);
    const isCall = call && isCallExpression(call) && calleeIsReference(call, name);
    if (isCall) calls.push(callEvidence(project, declaration, call, name, pending));
    else if (!isMemberDeclarationName(name)) return { reason: "value-reference" };
  }
  // `pending` holds the arguments in the order of their placeholders.
  const typed = typeArguments(project, pending);
  let next = 0;
  for (const evidence of calls) {
    evidence.args = evidence.args.map((value) =>
      value === null ? { maybe_undefined: typed[next++].maybe_undefined } : value,
    );
  }
  return { calls };
};

const nameRisk = (state, name) => {
  const risk = state.hard.get(name);
  if (risk) return risk;
  return state.untypedComputedCalls > 0 && state.strings.has(name) ? "name-in-string" : null;
};

const methodEvidence = (programs, root, classRequest, method, risks) => {
  const absolutePath = path.resolve(root, classRequest.path);
  const entry = programs.entry(absolutePath);
  if (!entry) return abstained("no-project");
  const state = risks.get(entry);
  const risk = nameRisk(state, method.name);
  if (risk) return abstained(risk);
  if (state.prototypes.has(classRequest.class)) return abstained("prototype-access");
  const { project } = entry;
  const sourceFile = project.program.getSourceFile(absolutePath);
  const found = methodDeclaration(project, sourceFile, classRequest, method.name);
  if (found.reason) return abstained(found.reason);
  const { classNode, declaration } = found;
  const { readsArguments, params } = methodParams(sourceFile, declaration);
  if (readsArguments) return abstained("arguments-read");
  // The experimental tsrs backend does not serve signature usage, so it gives
  // no method evidence.
  if (typeof project.checker.getSignatureUsage !== "function")
    return abstained("backend-unsupported");
  const usages = project.checker.getSignatureUsage(declaration);
  const alias = structuralAlias(
    project,
    classNode,
    declaration,
    state.references.get(method.name) ?? [],
    usages,
  );
  if (alias) return abstained(alias);
  const external = externalStructural(
    project,
    classNode,
    declaration,
    state.externalMembers.get(method.name),
  );
  if (external) return abstained(external);
  const collected = methodCalls(project, sourceFile, declaration, usages);
  if (collected.reason) return abstained(collected.reason);
  return {
    outcome: "complete",
    ...lineCol(sourceFile, declaration.name.getStart(sourceFile)),
    params,
    calls: collected.calls,
  };
};

const relativePath = (root, fileName) => path.relative(root, fileName).split(path.sep).join("/");

/**
 * Collects the call evidence of a parsed `call-evidence` request: whether
 * the arguments of each default candidate can be `undefined`, and the calls
 * of each method of each class candidate.
 */
export const collectCallEvidence = (
  { root, defaults = [], classes = [], files = [] },
  { createApi = createBackendApi } = {},
) => {
  if (defaults.length === 0 && classes.length === 0) return { defaults: [], methods: [] };
  const programs = new Programs(root, createApi);
  try {
    const defaultResults = defaultsEvidence(programs, root, defaults);
    const named = classes.map((request) => ({
      ...request,
      methods: request.methods.map((name) => ({ id: request.id, name })),
    }));
    const risks = nameRisks(programs, root, files, named);
    const methods = named.flatMap((request) =>
      request.methods.map((method) => {
        let evidence;
        try {
          evidence = methodEvidence(programs, root, request, method, risks);
        } catch {
          evidence = { outcome: "abstained", reason: "checker-error" };
        }
        const calls = evidence.calls?.map((call) => ({
          ...call,
          path: relativePath(root, call.path),
        }));
        return {
          class_id: request.id,
          method: method.name,
          ...evidence,
          ...(calls ? { calls } : {}),
        };
      }),
    );
    return { defaults: defaultResults, methods };
  } finally {
    programs.close();
  }
};
