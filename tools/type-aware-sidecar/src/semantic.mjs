import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";

import { API, SignatureKind, SymbolFlags } from "typescript/unstable/sync";
import {
  isCallExpression,
  isClassDeclaration,
  isClassExpression,
  isDecorator,
  isElementAccessExpression,
  isExportDeclaration,
  isExportSpecifier,
  isFunctionBody,
  isGetAccessorDeclaration,
  isIdentifier,
  isImportDeclaration,
  isImportClause,
  isImportSpecifier,
  isImportTypeNode,
  isNamespaceImport,
  isPropertyAccessExpression,
  isPrivateIdentifier,
  isSetAccessorDeclaration,
  isStringLiteralLikeNode,
  isTypeNode,
  isTypeQueryNode,
} from "typescript/unstable/ast/is";

import { canonicalFileIdentity } from "./file-identity.mjs";
import { projectResult, projectState } from "./project-state.mjs";
import {
  declarationName,
  declarationNamespaces,
  declarationsForSymbol,
  findDeclaration,
  isDeclaration,
  isExportAnchor,
  isProjectSource,
  nodeText,
  ownerDeclaration,
  projectExportIndex,
  projectSourceFiles,
  relativePath,
  resolveAlias,
  semanticQueryIdentity,
  sourceFileIdentity,
  stableDeclarationKey,
  stableSymbolIdentity,
  symbolForDeclaration,
} from "./semantic-identity.mjs";
import { analyzeSymbolImpact } from "./symbol-impact.mjs";
import { analyzeTypeCoupling } from "./type-coupling.mjs";

const ATTACHED_COMMENT_PATTERN = /\/\/|\/\*/u;
const MAX_SYMBOL_USE_EVIDENCE = 1;
const DECLARATION_KINDS = new Set([
  "export",
  "class",
  "interface",
  "type_alias",
  "enum",
  "function",
  "namespace",
  "variable",
  "class_method",
  "class_property",
]);

const compareText = (left, right) => Buffer.compare(Buffer.from(left), Buffer.from(right));
const locationKey = ({ path: filePath, line, col }) => `${filePath}\0${line}\0${col}`;

const location = (root, node) => {
  const sourceFile = node.getSourceFile();
  const start = node.getStart(sourceFile);
  const { line } = sourceFile.getLineAndCharacterOfPosition(start);
  const lineStart = sourceFile.getPositionOfLineAndCharacter(line, 0);
  return {
    path: relativePath(root, sourceFile.fileName),
    line: line + 1,
    col: Buffer.byteLength(sourceFile.text.slice(lineStart, start), "utf8"),
  };
};

const visit = (node, callback) => {
  callback(node);
  node.forEachChild((child) => {
    visit(child, callback);
    return undefined;
  });
};

const conservativeAssertion = (operation) =>
  ({
    "symbol-use": "no-confirmed-use",
    "symbol-trace": "no-references-found",
    "api-surface": "no-leak-confirmed",
    "symbol-impact": "no-consumers-found",
    "type-coupling": "no-coupling-found",
  })[operation];

const unavailable = (query, reasonCode, action) => ({
  queryId: query.id,
  operation: query.operation,
  assertion: conservativeAssertion(query.operation),
  status: "unavailable",
  reasonCode,
  actions: [action],
  evidence: [],
  totalEvidenceCount: 0,
  truncated: false,
  omissions: [{ reason_code: reasonCode, count: 1 }],
  data: {},
});

const REASON_ACTIONS = new Map([
  ["dynamic-behavior", "Review dynamic imports and registrations before changing this symbol."],
  [
    "virtual-dispatch",
    "Inspect interface, inherited, and virtual call sites before changing this implementation.",
  ],
  [
    "dynamic-member-access",
    "Replace or review computed, reflective, and string-dispatched member access before retrying.",
  ],
  [
    "decorated-declaration",
    "Review the class or member decorator contract before removing this declaration.",
  ],
  [
    "optional-contract",
    "Review the optional interface or inherited contract before removing this declaration.",
  ],
  [
    "accessor-pair",
    "Retain paired accessors unless the getter and setter are analyzed and removed together.",
  ],
  [
    "overload-set",
    "Retain overloaded members unless every declaration in the overload set is changed together.",
  ],
  ["attached-comment", "Review the attached declaration comment before removing this member."],
  ["abstract-declaration", "Retain abstract declarations because they define a class contract."],
  [
    "incomplete-project-coverage",
    "Repair every owning TypeScript project or select the complete project set and retry.",
  ],
  [
    "framework-contract-provenance",
    "Use a project layout that exposes the framework declaration's exact package provenance.",
  ],
  [
    "ambiguous-project",
    "Select projects that resolve this declaration to one consistent symbol identity.",
  ],
  [
    "blocking-diagnostics",
    "Repair structural TypeScript diagnostics in every selected project and retry.",
  ],
  [
    "svelte-virtual-module-exports",
    "Run svelte-check for framework diagnostics. See https://docs.fallow.tools/analysis/type-aware#svelte-virtual-module-exports for supported Svelte project setup.",
  ],
  [
    "unknown-entry-point",
    "Refresh the package entry points or pass project-relative source entry points.",
  ],
]);
const DEFAULT_REASON_ACTION =
  "Narrow the query to a specific symbol, entry point, or healthy TypeScript project and retry.";
const REASON_PRIORITY = new Map([
  ["blocking-diagnostics", 0],
  ["svelte-virtual-module-exports", 1],
  ["incomplete-project-coverage", 2],
  ["framework-contract-provenance", 3],
  ["ambiguous-project", 4],
  ["unknown-symbol", 5],
  ["unknown-entry-point", 6],
  ["decorated-declaration", 7],
  ["optional-contract", 8],
  ["accessor-pair", 9],
  ["overload-set", 10],
  ["attached-comment", 11],
  ["abstract-declaration", 12],
  ["dynamic-member-access", 13],
  ["virtual-dispatch", 14],
  ["dynamic-behavior", 15],
  ["evidence-limit", 16],
]);

const actionForReason = (reasonCode) => REASON_ACTIONS.get(reasonCode) ?? DEFAULT_REASON_ACTION;

const orderedEvidence = (evidence) =>
  [...evidence].toSorted((left, right) => compareText(JSON.stringify(left), JSON.stringify(right)));

const evidenceLimitOmission = (totalEvidence, returnedEvidenceCount) =>
  totalEvidence > returnedEvidenceCount
    ? [{ reason_code: "evidence-limit", count: totalEvidence - returnedEvidenceCount }]
    : [];

const combineOmissions = (omissions) => {
  const counts = new Map();
  for (const omission of omissions.filter((entry) => entry.count > 0)) {
    counts.set(omission.reason_code, (counts.get(omission.reason_code) ?? 0) + omission.count);
  }
  return [...counts].map(([reason_code, count]) => ({ reason_code, count }));
};

const projectStateReason = (state) => state.reason_code ?? "blocking-diagnostics";

const unavailableProjectOmissions = (states) =>
  combineOmissions(
    states
      .filter((state) => state.status !== "complete")
      .map((state) => ({ reason_code: projectStateReason(state), count: 1 })),
  );

const unavailableProjectReason = (states) => {
  if (states.length === 0) return "no-project";
  return (
    unavailableProjectOmissions(states).toSorted(compareOmissions)[0]?.reason_code ?? "no-project"
  );
};

const resultStatus = (partial) => (partial ? "partial" : "complete");
const resultReason = (omissions) => (omissions.length > 0 ? omissions[0].reason_code : null);
const resultActions = (omissions) =>
  omissions.map((omission) => actionForReason(omission.reason_code));

const compareOmissions = (left, right) =>
  (REASON_PRIORITY.get(left.reason_code) ?? 10) - (REASON_PRIORITY.get(right.reason_code) ?? 10) ||
  compareText(left.reason_code, right.reason_code);

const boundedResult = ({
  query,
  assertion,
  evidence,
  data,
  evidenceLimit,
  totalEvidenceCount,
  omissions: extraOmissions = [],
}) => {
  const ordered = orderedEvidence(evidence);
  const totalEvidence = totalEvidenceCount === undefined ? ordered.length : totalEvidenceCount;
  const returnedEvidenceCount = Math.min(ordered.length, evidenceLimit);
  const omissions = combineOmissions([
    ...evidenceLimitOmission(totalEvidence, returnedEvidenceCount),
    ...extraOmissions,
  ]).toSorted(compareOmissions);
  const partial = omissions.length > 0;
  const truncated = omissions.some((omission) => omission.reason_code === "evidence-limit");
  return {
    queryId: query.id,
    operation: query.operation,
    assertion,
    status: resultStatus(partial),
    reasonCode: resultReason(omissions),
    actions: resultActions(omissions),
    evidence: ordered.slice(0, evidenceLimit),
    totalEvidenceCount: totalEvidence,
    truncated,
    omissions,
    data,
  };
};

const TYPE_POSITION_RULES = [
  [isTypeQueryNode, () => false],
  [isImportTypeNode, (node) => !node.isTypeOf],
  [
    (node) => isImportSpecifier(node) || isExportSpecifier(node),
    (node) => Boolean(node.isTypeOnly || node.parent?.parent?.isTypeOnly),
  ],
  [isTypeNode, () => true],
  [
    (node) => isDeclaration(node) || isImportDeclaration(node) || isExportDeclaration(node),
    () => false,
  ],
];

const typePositionAt = (node) => {
  const rule = TYPE_POSITION_RULES.find(([matches]) => matches(node));
  return rule?.[1](node);
};

const isTypePosition = (node) => {
  let current = node.parent;
  while (current) {
    const result = typePositionAt(current);
    if (result !== undefined) return result;
    current = current.parent;
  }
  return false;
};

const referenceRoleAt = (node) => {
  const rules = [
    [isImportSpecifier, node.isTypeOnly ? "type-import" : "import"],
    [isExportSpecifier, node.isTypeOnly ? "type-re-export" : "re-export"],
    [isCallExpression, "call"],
    [isPropertyAccessExpression, "property-access"],
  ];
  return rules.find(([matches]) => matches(node))?.[1];
};

const referenceRole = (node) => {
  if (isTypePosition(node)) return "type-reference";
  const role = referenceRoleAt(node);
  if (role) return role;
  if (isReferenceBoundary(node)) return "value-reference";
  return referenceRole(node.parent);
};

const isReferenceBoundary = (node) => !node.parent || isDeclaration(node);
const isAliasSpecifier = (node) => isImportSpecifier(node) || isExportSpecifier(node);

const aliasNode = (node) => {
  if (!node) return undefined;
  if (isDeclaration(node)) return undefined;
  if (isAliasSpecifier(node)) return node;
  return aliasNode(node.parent);
};

const aliasRelation = (alias) => (isImportSpecifier(alias) ? "import-alias" : "re-export");
const aliasNames = (alias) => {
  const aliasName = nodeText(alias.name);
  const fromName = nodeText(alias.propertyName) ?? aliasName;
  return { fromName, toName: aliasName ?? fromName };
};

const aliasHop = (root, node) => {
  const alias = aliasNode(node);
  if (!alias) return undefined;
  const names = aliasNames(alias);
  return {
    ...location(root, alias),
    from_name: names.fromName,
    to_name: names.toName,
    relation: aliasRelation(alias),
  };
};

const semanticUse = (root, node, namespace) => {
  const hop = aliasHop(root, node);
  return {
    ...location(root, node),
    role: referenceRole(node),
    source: "checker",
    namespace,
    via: hop ? [hop] : [],
  };
};

const isDefaultImportReference = (node) => isImportClause(node.parent) && node.parent.name === node;

const candidateNameMatches = (candidateNames, name) => !candidateNames || candidateNames.has(name);

const isIdentifierReference = (node, candidateNames, includeDefaultImports) => {
  if (!isIdentifier(node)) return false;
  if (candidateNameMatches(candidateNames, node.text)) return true;
  return includeDefaultImports && isDefaultImportReference(node);
};

const isElementNameNode = (node) =>
  isStringLiteralLikeNode(node) &&
  isElementAccessExpression(node.parent) &&
  node.parent.argumentExpression === node;

const isElementReference = (node, candidateNames) =>
  isElementNameNode(node) && candidateNameMatches(candidateNames, node.text);

const entriesByName = (entries) => {
  const byName = new Map();
  for (const entry of entries) {
    const names = new Set([entry.query.symbol.localName, entry.query.symbol.exportedName]);
    for (const name of names) {
      const named = byName.get(name) ?? [];
      named.push(entry);
      byName.set(name, named);
    }
  }
  return byName;
};

const symbolUseBatchState = (resolvedQueries, evidenceLimit) => {
  const classMemberEntries = resolvedQueries.filter(({ query }) =>
    ["class_method", "class_property"].includes(query.symbol.declarationKind),
  );
  return {
    evidenceByQuery: new Map(resolvedQueries.map(({ query }) => [query.id, []])),
    totalByQuery: new Map(resolvedQueries.map(({ query }) => [query.id, 0])),
    directFilesByQuery: new Map(resolvedQueries.map(({ query }) => [query.id, new Set()])),
    aliasHopTotalByQuery: new Map(resolvedQueries.map(({ query }) => [query.id, 0])),
    contractRelationsByQuery: new Map(resolvedQueries.map(({ query }) => [query.id, []])),
    uncertaintiesByQuery: new Map(resolvedQueries.map(({ query }) => [query.id, new Set()])),
    declarationLocationsByQuery: new Map(resolvedQueries.map(({ query }) => [query.id, new Set()])),
    targetsByKey: new Map(),
    exportEntriesByModule: new Map(),
    exportEntriesByProject: new Map(),
    candidateNames: new Set(),
    classMemberEntries,
    classMemberEntriesByName: entriesByName(classMemberEntries),
    classMemberEntrySetsByProject: new Map(),
    projectsWithUnknownDynamicAccess: new Set(),
    evidenceLimit,
  };
};

const addSymbolTarget = (root, state, entry, declaration, namespace) => {
  if (!declarationNamespaces(declaration).has(namespace)) return;
  state.declarationLocationsByQuery
    .get(entry.query.id)
    .add(locationKey(location(root, declaration.name ?? declaration)));
  const key = stableDeclarationKey(declaration, namespace);
  const targets = state.targetsByKey.get(key) ?? [];
  targets.push(entry);
  state.targetsByKey.set(key, targets);
};

const registerSymbolTargets = (root, state, entry) => {
  state.candidateNames.add(entry.query.symbol.localName);
  state.candidateNames.add(entry.query.symbol.exportedName);
  if (entry.query.symbol.declarationKind === "export") {
    const moduleIdentity = canonicalFileIdentity(entry.query.symbol.absolutePath);
    const entries = state.exportEntriesByModule.get(moduleIdentity) ?? new Set();
    entries.add(entry);
    state.exportEntriesByModule.set(moduleIdentity, entries);
    entry.resolved.ownerContexts.forEach(({ project }) => {
      const projectEntries = state.exportEntriesByProject.get(project) ?? new Set();
      projectEntries.add(entry);
      state.exportEntriesByProject.set(project, projectEntries);
    });
  }
  entry.resolved.ownerContexts.forEach(({ project, declaration }) => {
    const symbol = resolveAlias(project.checker, symbolForDeclaration(project, declaration));
    declarationsForSymbol(project, symbol).forEach((target) => {
      const namespaces =
        entry.query.symbol.declarationKind === "export"
          ? declarationNamespaces(target)
          : new Set([entry.query.symbol.namespace]);
      namespaces.forEach((namespace) => addSymbolTarget(root, state, entry, target, namespace));
    });
  });
  state.contractRelationsByQuery.set(entry.query.id, entry.resolved.contractRelations);
};

const matchingSymbolEntries = (project, symbol, namespace, targetsByKey) => {
  return new Set(
    declarationsForSymbol(project, symbol)
      .filter((declaration) => declarationNamespaces(declaration).has(namespace))
      .flatMap((declaration) => {
        const key = stableDeclarationKey(declaration, namespace);
        return targetsByKey.get(key) ?? [];
      }),
  );
};

const isDeclarationReference = (root, state, entry, node) => {
  const locations = state.declarationLocationsByQuery.get(entry.query.id);
  if (locations.has(locationKey(location(root, node)))) return true;
  return node === entry.resolved.declaration.name || node.parent === entry.resolved.declaration;
};

const moduleEdgeDeclaration = (node) => {
  let current = node;
  while (current) {
    if (isImportDeclaration(current) || isExportDeclaration(current)) return current;
    current = current.parent;
  }
  return undefined;
};

const ancestorImportType = (node) => {
  let current = node;
  while (current) {
    if (isImportTypeNode(current)) return current;
    current = current.parent;
  }
  return undefined;
};

const importTypeSpecifier = (node) => {
  const argument = ancestorImportType(node)?.argument;
  const literal = argument?.literal;
  return literal && isStringLiteralLikeNode(literal) ? literal : undefined;
};

const isDynamicImportCall = (node) =>
  isCallExpression(node) && node.expression.getText(node.getSourceFile()) === "import";

const moduleIdentityForSpecifier = (project, specifier) => {
  if (!specifier) return undefined;
  const moduleSymbol = project.checker.getSymbolAtLocation(specifier);
  const moduleDeclaration = declarationsForSymbol(
    project,
    resolveAlias(project.checker, moduleSymbol),
  )[0];
  return moduleDeclaration ? sourceFileIdentity(moduleDeclaration.getSourceFile()) : undefined;
};

const namespaceImportDeclaration = (project, node) => {
  const parent = node.parent;
  if (!parent) return undefined;
  const namespace = isPropertyAccessExpression(parent)
    ? parent.name === node
      ? parent.expression
      : undefined
    : isElementAccessExpression(parent) && parent.argumentExpression === node
      ? parent.expression
      : undefined;
  if (!namespace) return undefined;
  if (!isIdentifier(namespace)) return undefined;
  const symbol = project.checker.getSymbolAtLocation(namespace);
  return symbol?.declarations
    ?.map((declaration) => declaration.resolve(project))
    .find(isNamespaceImport);
};

const referencedModuleIdentity = (project, node) => {
  const declaration =
    moduleEdgeDeclaration(node) ?? moduleEdgeDeclaration(namespaceImportDeclaration(project, node));
  const declarationSpecifier = declaration?.moduleSpecifier;
  const specifier =
    declarationSpecifier && isStringLiteralLikeNode(declarationSpecifier)
      ? declarationSpecifier
      : importTypeSpecifier(node);
  return moduleIdentityForSpecifier(project, specifier);
};

const isExactExportReference = (project, entry, node) =>
  entry.query.symbol.declarationKind !== "export" ||
  referencedModuleIdentity(project, node) ===
    canonicalFileIdentity(entry.query.symbol.absolutePath);

const recordSymbolUse = (root, state, project, entry, node, namespace) => {
  if (isDeclarationReference(root, state, entry, node)) return;
  if (!isExactExportReference(project, entry, node)) return;
  const evidence = state.evidenceByQuery.get(entry.query.id);
  state.totalByQuery.set(entry.query.id, state.totalByQuery.get(entry.query.id) + 1);
  state.directFilesByQuery.get(entry.query.id).add(sourceFileIdentity(node.getSourceFile()));
  const use = semanticUse(root, node, namespace);
  if (use.via.length > 0) {
    state.aliasHopTotalByQuery.set(
      entry.query.id,
      state.aliasHopTotalByQuery.get(entry.query.id) + use.via.length,
    );
  }
  const limit =
    entry.query.operation === "symbol-use" ? MAX_SYMBOL_USE_EVIDENCE : state.evidenceLimit;
  if (evidence.length < limit) {
    evidence.push(use);
  }
};

const projectOwnsEntry = (project, entry) =>
  entry.resolved.ownerContexts.some((context) => context.project === project);

const classMemberEntrySetForProject = (state, project) => {
  const cached = state.classMemberEntrySetsByProject.get(project);
  if (cached) return cached;
  const entries = new Set(
    state.classMemberEntries.filter((entry) => projectOwnsEntry(project, entry)),
  );
  state.classMemberEntrySetsByProject.set(project, entries);
  return entries;
};

const recordSymbolUncertainty = (state, entry, reasonCode) => {
  state.uncertaintiesByQuery.get(entry.query.id).add(reasonCode);
};

const isExactElementReference = (node) => isElementNameNode(node) && typeof node.text === "string";

const recordStringDispatchedMemberAccess = (state, projectEntries, node) => {
  if (!isStringLiteralLikeNode(node) || isExactElementReference(node)) return;
  (state.classMemberEntriesByName.get(node.text) ?? [])
    .filter((entry) => projectEntries.has(entry))
    .forEach((entry) => recordSymbolUncertainty(state, entry, "dynamic-member-access"));
};

const recordComputedMemberAccess = (state, project, entries, node) => {
  if (
    entries.size === 0 ||
    state.projectsWithUnknownDynamicAccess.has(project) ||
    !isElementAccessExpression(node) ||
    node.argumentExpression === undefined ||
    isStringLiteralLikeNode(node.argumentExpression)
  ) {
    return;
  }
  const receiverType = project.checker.getTypeAtLocation(node.expression);
  const receiverProperties = project.checker.getPropertiesOfType(receiverType);
  if (receiverProperties.length === 0) {
    entries.forEach((entry) => recordSymbolUncertainty(state, entry, "dynamic-member-access"));
    state.projectsWithUnknownDynamicAccess.add(project);
    return;
  }
  const matched = new Set(
    receiverProperties
      .filter((property) => state.candidateNames.has(property.name))
      .flatMap((property) => [
        ...matchingSymbolEntries(project, property, "value", state.targetsByKey),
      ]),
  );
  matched.forEach((entry) => recordSymbolUncertainty(state, entry, "dynamic-member-access"));
};

const recordDynamicImportUncertainty = (state, project, node) => {
  if (!isDynamicImportCall(node)) return;
  const specifier = node.arguments[0];
  if (!specifier || !isStringLiteralLikeNode(specifier)) {
    (state.exportEntriesByProject.get(project) ?? []).forEach((entry) =>
      recordSymbolUncertainty(state, entry, "dynamic-behavior"),
    );
    return;
  }
  const moduleIdentity = moduleIdentityForSpecifier(project, specifier);
  if (!moduleIdentity) return;
  (state.exportEntriesByModule.get(moduleIdentity) ?? []).forEach((entry) =>
    recordSymbolUncertainty(state, entry, "dynamic-behavior"),
  );
};

const referenceNamespaces = (project, node, symbol) => {
  const alias = aliasNode(node);
  if (!alias || !isExportSpecifier(alias)) {
    return [isTypePosition(node) ? "type" : "value"];
  }
  if (alias.isTypeOnly || alias.parent?.parent?.isTypeOnly) return ["type"];
  const namespaces = new Set(
    declarationsForSymbol(project, symbol).flatMap((declaration) => [
      ...declarationNamespaces(declaration),
    ]),
  );
  return namespaces.size > 0 ? [...namespaces] : [isTypePosition(node) ? "type" : "value"];
};

const scanSymbolUseFile = (root, state, project, sourceFile, includeDefaultImports) => {
  const projectEntries = classMemberEntrySetForProject(state, project);
  const nodes = [];
  visit(sourceFile, (node) => {
    recordDynamicImportUncertainty(state, project, node);
    if (projectEntries.size > 0) {
      recordStringDispatchedMemberAccess(state, projectEntries, node);
      recordComputedMemberAccess(state, project, projectEntries, node);
    }
    if (isIdentifierReference(node, state.candidateNames, includeDefaultImports)) {
      nodes.push(node);
      return;
    }
    if (isElementReference(node, state.candidateNames)) nodes.push(node);
  });
  const symbols = project.checker.getSymbolAtLocation(nodes);
  nodes.forEach((node, index) => {
    const symbol = resolveAlias(project.checker, symbols[index]);
    referenceNamespaces(project, node, symbol).forEach((namespace) => {
      const entries = matchingSymbolEntries(project, symbol, namespace, state.targetsByKey);
      entries.forEach((entry) => recordSymbolUse(root, state, project, entry, node, namespace));
    });
  });
};

const scanSymbolUseProjects = (root, state, projects, includeDefaultImports) => {
  const scannedFiles = new Set();
  let sourceScanCount = 0;
  for (const project of projects) {
    for (const sourceFile of projectSourceFiles(project)) {
      const fileIdentity = sourceFileIdentity(sourceFile);
      if (scannedFiles.has(fileIdentity)) continue;
      scannedFiles.add(fileIdentity);
      sourceScanCount += 1;
      scanSymbolUseFile(root, state, project, sourceFile, includeDefaultImports);
    }
  }
  return sourceScanCount;
};

const batchSymbolUseEvidence = (root, resolvedQueries, scanProjects, evidenceLimit) => {
  const state = symbolUseBatchState(resolvedQueries, evidenceLimit);
  resolvedQueries.forEach((entry) => registerSymbolTargets(root, state, entry));
  const includeDefaultImports = resolvedQueries.some(
    ({ query }) => query.symbol.exportedName === "default",
  );
  const sourceScanCount = scanSymbolUseProjects(root, state, scanProjects, includeDefaultImports);
  return {
    evidenceByQuery: state.evidenceByQuery,
    totalByQuery: state.totalByQuery,
    directFilesByQuery: state.directFilesByQuery,
    aliasHopTotalByQuery: state.aliasHopTotalByQuery,
    contractRelationsByQuery: state.contractRelationsByQuery,
    uncertaintiesByQuery: state.uncertaintiesByQuery,
    sourceScanCount,
  };
};

const selectProjectForSymbol = (snapshot, explicitProjects, absolutePath, allowDefaultFallback) =>
  explicitProjects.find((project) => project.program.getSourceFile(absolutePath)) ??
  (allowDefaultFallback ? snapshot.getDefaultProjectForFile(absolutePath) : undefined);

const symbolResolutionError = (query, reasonCode, action) => ({
  error: unavailable(query, reasonCode, action),
});

const isCompleteProjectState = (state) => state?.status === "complete";
const owningProjectContexts = (statesByProject, absolutePath) =>
  [...statesByProject.entries()]
    .filter(([project]) => project.program.getSourceFile(absolutePath))
    .map(([project, state]) => ({ project, state }));

const selectSymbolContext = (
  snapshot,
  explicitProjects,
  statesByProject,
  query,
  allowDefaultFallback,
) => {
  if (!DECLARATION_KINDS.has(query.symbol.declarationKind)) {
    return symbolResolutionError(
      query,
      "unsupported-syntax",
      "Use a supported declaration kind or retain the syntactic finding.",
    );
  }
  let owners = owningProjectContexts(statesByProject, query.symbol.absolutePath);
  if (owners.length === 0) {
    const fallback = selectProjectForSymbol(
      snapshot,
      explicitProjects,
      query.symbol.absolutePath,
      allowDefaultFallback,
    );
    const fallbackState = statesByProject.get(fallback);
    if (fallback && fallbackState) owners = [{ project: fallback, state: fallbackState }];
  }
  if (owners.length === 0) {
    return symbolResolutionError(
      query,
      "no-project",
      "Pass a tsconfig containing the declaration with --type-aware-project.",
    );
  }
  const completeOwners = owners.filter(({ state }) => isCompleteProjectState(state));
  if (completeOwners.length === 0) {
    const selectedOwner = [...owners].toSorted((left, right) =>
      compareOmissions(
        { reason_code: projectStateReason(left.state) },
        { reason_code: projectStateReason(right.state) },
      ),
    )[0];
    const reasonCode = projectStateReason(selectedOwner.state);
    return {
      ...selectedOwner,
      ...symbolResolutionError(query, reasonCode, actionForReason(reasonCode)),
    };
  }
  return { owners, completeOwners };
};

const resolvedAnchorTarget = (project, anchor, requestedSymbol) => {
  if (!anchor) return undefined;
  if (!isExportAnchor(anchor)) {
    return declarationNamespaces(anchor).has(requestedSymbol.namespace)
      ? { declaration: anchor, namespace: requestedSymbol.namespace }
      : undefined;
  }
  const checkerSymbol = project.checker.getSymbolAtLocation(anchor.name);
  const declarations = declarationsForSymbol(project, resolveAlias(project.checker, checkerSymbol));
  const exact = declarations.find((node) =>
    declarationNamespaces(node).has(requestedSymbol.namespace),
  );
  if (exact) return { declaration: exact, namespace: requestedSymbol.namespace };
  if (requestedSymbol.declarationKind !== "export") return undefined;
  const fallback = declarations
    .flatMap((declaration) =>
      [...declarationNamespaces(declaration)].map((namespace) => ({ declaration, namespace })),
    )
    .toSorted((left, right) =>
      compareText(
        stableDeclarationKey(left.declaration, left.namespace),
        stableDeclarationKey(right.declaration, right.namespace),
      ),
    )[0];
  return fallback;
};

const requiresExportIndex = (symbol) =>
  symbol.declarationKind === "export" || symbol.exportedName !== symbol.localName;

const exportAliasMatches = (project, symbol, target) => {
  if (!target) return false;
  if (!requiresExportIndex(symbol)) return true;
  const candidateKey = stableDeclarationKey(target.declaration, target.namespace);
  return projectExportIndex(project).has(`${symbol.exportedName}\0${candidateKey}`);
};

const symbolQueryAnchor = (project, query) => {
  const sourceFile = project.program.getSourceFile(query.symbol.absolutePath);
  return sourceFile ? findDeclaration(sourceFile, query.symbol) : undefined;
};

const symbolQueryResolved = (project, symbol, anchor, target) =>
  Boolean(anchor) && exportAliasMatches(project, symbol, target);

const modifierNamed = (node, name) =>
  Boolean(node?.modifiers?.some((modifier) => modifier.getText(node.getSourceFile()) === name));

const hasDecorator = (node) =>
  Boolean(node?.decorators?.length) ||
  Boolean(node?.modifiers?.some((modifier) => isDecorator(modifier)));

const accessorPairExists = (declaration) => {
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

const sameNamedMemberCount = (declaration) => {
  const owner = ownerDeclaration(declaration);
  const name = declarationName(declaration);
  return owner?.members?.filter((member) => declarationName(member) === name).length ?? 0;
};

const hasAttachedComment = (declaration) => {
  const sourceFile = declaration.getSourceFile();
  const trivia = sourceFile.text.slice(
    declaration.getFullStart(),
    declaration.getStart(sourceFile),
  );
  return ATTACHED_COMMENT_PATTERN.test(trivia);
};

const optionalContractDeclaration = (symbol, declaration) =>
  (symbol.flags & SymbolFlags.Optional) !== 0 || declaration?.questionToken !== undefined;

const heritageRelation = (clause, declaration) => {
  const text = clause.getText(clause.getSourceFile());
  if (text.startsWith("implements")) return "interface-implementation";
  if (modifierNamed(declaration, "abstract")) return "abstract-implementation";
  return "override";
};

const contractRelationsFor = (root, project, declaration, memberName) => {
  const owner = ownerDeclaration(declaration);
  if (!owner || (!isClassDeclaration(owner) && !isClassExpression(owner))) return [];
  const relations = [];
  for (const clause of owner.heritageClauses ?? []) {
    for (const heritageType of clause.types ?? []) {
      const type = project.checker.getTypeAtLocation(heritageType);
      const symbol = project.checker
        .getPropertiesOfType(type)
        .find((property) => property.name === memberName);
      if (!symbol) continue;
      for (const contractDeclaration of declarationsForSymbol(project, symbol)) {
        const optional = optionalContractDeclaration(symbol, contractDeclaration);
        relations.push({
          relation: optional ? "optional-contract" : heritageRelation(clause, contractDeclaration),
          declaration: stableSymbolIdentity(root, contractDeclaration, "value", memberName),
          optional,
        });
      }
    }
  }
  return uniqueSorted(relations);
};

const packageNameForDeclaration = (declaration) => {
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

const declarationIsProjectLocal = (root, declaration) => {
  const relative = path.relative(root, declaration.getSourceFile().fileName);
  return relative === "" || (!path.isAbsolute(relative) && !relative.startsWith(`..${path.sep}`));
};

const frameworkContractRelationsFor = (root, project, declaration, contracts, memberName) => {
  const owner = ownerDeclaration(declaration);
  if (!owner || (!isClassDeclaration(owner) && !isClassExpression(owner))) {
    return { relations: [], provenanceUnknown: false };
  }
  const matchingContracts = contracts.filter((contract) => contract.members.includes(memberName));
  const relations = [];
  let provenanceUnknown = false;
  for (const clause of owner.heritageClauses ?? []) {
    const clauseRelation = clause.getText(clause.getSourceFile()).startsWith("implements")
      ? "implements"
      : "extends";
    for (const heritageType of clause.types ?? []) {
      const rawSymbol =
        project.checker.getSymbolAtLocation(heritageType.expression) ??
        project.checker.getTypeAtLocation(heritageType).aliasSymbol ??
        project.checker.getTypeAtLocation(heritageType).symbol;
      const symbol = resolveAlias(project.checker, rawSymbol);
      for (const heritageDeclaration of declarationsForSymbol(project, symbol)) {
        const declarationPackage = packageNameForDeclaration(heritageDeclaration);
        for (const contract of matchingContracts) {
          if (
            contract.relation !== clauseRelation ||
            contract.heritageSymbol !== declarationName(heritageDeclaration)
          ) {
            continue;
          }
          if (!declarationPackage) {
            provenanceUnknown ||= !declarationIsProjectLocal(root, heritageDeclaration);
            continue;
          }
          if (contract.package !== declarationPackage) continue;
          relations.push({
            framework: contract.framework,
            package: contract.package,
            relation: contract.relation,
            declaration: stableSymbolIdentity(
              root,
              heritageDeclaration,
              "value",
              contract.heritageSymbol,
            ),
          });
        }
      }
    }
  }
  return { relations: uniqueSorted(relations), provenanceUnknown };
};

const declarationEditGuard = (declaration) => {
  const sourceFile = declaration.getSourceFile();
  const start = declaration.getStart(sourceFile);
  const end = declaration.end;
  const text = sourceFile.text.slice(start, end);
  return {
    start: Buffer.byteLength(sourceFile.text.slice(0, start), "utf8"),
    end: Buffer.byteLength(sourceFile.text.slice(0, end), "utf8"),
    declaration_sha256: createHash("sha256").update(text).digest("hex"),
  };
};

const resolvedOwnerContext = (query, owner) => {
  const anchor = symbolQueryAnchor(owner.project, query);
  const target = resolvedAnchorTarget(owner.project, anchor, query.symbol);
  return symbolQueryResolved(owner.project, query.symbol, anchor, target)
    ? { ...owner, ...target }
    : undefined;
};

const resolvedIdentityKey = (context) =>
  stableDeclarationKey(context.declaration, context.namespace);

const resolutionOmissions = (owners, completeOwners, resolvedOwners, identityCount) => [
  {
    reason_code: "incomplete-project-coverage",
    count: owners.length - completeOwners.length,
  },
  {
    reason_code: "unknown-symbol",
    count: completeOwners.length - resolvedOwners.length,
  },
  {
    reason_code: "ambiguous-project",
    count: Math.max(0, identityCount - 1),
  },
];

const resolveSymbolQuery = (
  root,
  snapshot,
  explicitProjects,
  statesByProject,
  query,
  allowDefaultFallback,
) => {
  const context = selectSymbolContext(
    snapshot,
    explicitProjects,
    statesByProject,
    query,
    allowDefaultFallback,
  );
  if (context.error) return context;
  const ownerContexts = context.completeOwners
    .map((owner) => resolvedOwnerContext(query, owner))
    .filter(Boolean);
  if (ownerContexts.length === 0) {
    return {
      ...context.completeOwners[0],
      ...symbolResolutionError(
        query,
        "unknown-symbol",
        "Refresh the syntactic result and retry with its exact declaration identity.",
      ),
    };
  }
  const identityCount = new Set(ownerContexts.map(resolvedIdentityKey)).size;
  const primary = ownerContexts[0];
  const contractRelations = uniqueSorted(
    ownerContexts.flatMap(({ project, declaration }) =>
      contractRelationsFor(root, project, declaration, query.symbol.localName),
    ),
  );
  const frameworkContractEvidence = ownerContexts.map(({ project, declaration }) =>
    frameworkContractRelationsFor(
      root,
      project,
      declaration,
      query.frameworkContracts ?? [],
      query.symbol.localName,
    ),
  );
  const frameworkContractRelations = uniqueSorted(
    frameworkContractEvidence.flatMap(({ relations }) => relations),
  );
  const declarationOwnerNode = ownerDeclaration(primary.declaration);
  const declarationUncertainties = new Set();
  if (frameworkContractEvidence.some(({ provenanceUnknown }) => provenanceUnknown)) {
    declarationUncertainties.add("framework-contract-provenance");
  }
  if (hasDecorator(primary.declaration) || hasDecorator(declarationOwnerNode)) {
    declarationUncertainties.add("decorated-declaration");
  }
  if (primary.declaration.name && !isIdentifier(primary.declaration.name)) {
    declarationUncertainties.add("dynamic-member-access");
  }
  if (contractRelations.some((relation) => relation.optional)) {
    declarationUncertainties.add("optional-contract");
  }
  if (accessorPairExists(primary.declaration)) {
    declarationUncertainties.add("accessor-pair");
  }
  if (sameNamedMemberCount(primary.declaration) > 1 && !accessorPairExists(primary.declaration)) {
    declarationUncertainties.add("overload-set");
  }
  if (hasAttachedComment(primary.declaration)) {
    declarationUncertainties.add("attached-comment");
  }
  if (modifierNamed(primary.declaration, "abstract")) {
    declarationUncertainties.add("abstract-declaration");
  }
  return {
    ...primary,
    ownerContexts,
    owningProjects: context.owners.map(({ state }) => state.config).toSorted(compareText),
    contractRelations,
    frameworkContractRelations,
    declarationUncertainties,
    editGuard: declarationEditGuard(primary.declaration),
    omissions: resolutionOmissions(
      context.owners,
      context.completeOwners,
      ownerContexts,
      identityCount,
    ),
  };
};

const analyzeSymbolUse = (
  root,
  query,
  resolved,
  evidenceLimit,
  evidence,
  totalEvidenceCount,
  batchUncertainties,
) => {
  const uncertainties = new Set([...resolved.declarationUncertainties, ...batchUncertainties]);
  const requiredContracts = resolved.contractRelations.filter((relation) => !relation.optional);
  const frameworkContracts = resolved.frameworkContractRelations;
  const omissions = [
    ...resolved.omissions,
    ...[...uncertainties].map((reason_code) => ({ reason_code, count: 1 })),
  ];
  const closedWorldEligible =
    totalEvidenceCount === 0 &&
    requiredContracts.length === 0 &&
    frameworkContracts.length === 0 &&
    omissions.every((omission) => omission.count === 0);
  const assertion =
    totalEvidenceCount > 0
      ? "confirmed-used"
      : requiredContracts.length > 0 || frameworkContracts.length > 0
        ? "contract-preserved"
        : closedWorldEligible
          ? "confirmed-no-static-references"
          : "no-confirmed-use";
  return boundedResult({
    query,
    assertion,
    evidence,
    evidenceLimit,
    totalEvidenceCount,
    omissions,
    data: {
      symbol: semanticQueryIdentity(root, query, resolved),
      selected_project: resolved.state.config,
      owning_projects: resolved.owningProjects,
      total_reference_count: totalEvidenceCount,
      contract_relations: resolved.contractRelations,
      framework_contract_relations: frameworkContracts,
      closed_world_eligible: closedWorldEligible,
      edit_guard: resolved.editGuard,
    },
  });
};

const analyzeSymbolTrace = (
  root,
  query,
  resolved,
  evidenceLimit,
  references,
  totalReferenceCount,
  totalAliasHopCount,
) => {
  const declaration = {
    ...location(root, resolved.declaration),
    role: "declaration",
    source: "checker",
  };
  const aliasHops = references
    .flatMap((entry) => entry.via)
    .toSorted((left, right) => compareText(JSON.stringify(left), JSON.stringify(right)));
  return boundedResult({
    query,
    assertion: totalReferenceCount > 0 ? "references-found" : "no-references-found",
    evidence: [declaration, ...references],
    evidenceLimit,
    totalEvidenceCount: totalReferenceCount + 1,
    data: {
      symbol: semanticQueryIdentity(root, query, resolved),
      selected_project: resolved.state.config,
      alias_hops: aliasHops.slice(0, evidenceLimit),
      total_alias_hop_count: totalAliasHopCount,
      checker_evidence_count: totalReferenceCount,
      graph_evidence_count: totalAliasHopCount,
    },
  });
};

const packageTargets = (value, targets) => {
  if (typeof value === "string") {
    targets.add(value);
    return;
  }
  packageTargetChildren(value).forEach((entry) => packageTargets(entry, targets));
};

const packageTargetChildren = (value) => {
  if (Array.isArray(value)) return value;
  if (value === null || typeof value !== "object") return [];
  return Object.values(value);
};

const sourceCandidatesForTarget = (root, target) => {
  const normalized = target.replace(/^\.\//u, "");
  const extensionless = normalized.replace(/(?:\.d)?\.[cm]?[jt]sx?$/u, "");
  const withoutDist = extensionless.replace(/^dist\//u, "src/");
  return [normalized, extensionless, withoutDist]
    .flatMap((entry) => [entry, `${entry}.ts`, `${entry}.tsx`, `${entry}/index.ts`])
    .map((entry) => path.resolve(root, entry));
};

const wildcardPattern = (target) => {
  const normalized = target.replace(/^\.\//u, "").replace(/^dist\//u, "src/");
  const sourceTarget = normalized.replace(/(?:\.d)?\.[cm]?[jt]sx?$/u, ".ts");
  const escaped = sourceTarget
    .split("*")
    .map((part) => part.replace(/[\\^$.*+?()[\]{}|]/gu, "\\$&"))
    .join("[^/]+");
  return new RegExp(`^${escaped}$`, "u");
};

const pathWithin = (root, candidate) =>
  candidate === root || candidate.startsWith(`${root}${path.sep}`);

const packageAt = (directory) => {
  const packageFile = path.join(directory, "package.json");
  if (!existsSync(packageFile)) return undefined;
  try {
    return { root: directory, json: JSON.parse(readFileSync(packageFile, "utf8")) };
  } catch {
    return { root: directory, json: {} };
  }
};

const projectPackage = (root, project) => {
  const normalizedRoot = path.resolve(root);
  let current = path.dirname(project.configFileName);
  while (pathWithin(normalizedRoot, current)) {
    const found = packageAt(current);
    if (found) return found;
    if (current === normalizedRoot) break;
    current = path.dirname(current);
  }
  return { root: normalizedRoot, json: {} };
};

const ancestorRoots = (root, start) => {
  const normalizedRoot = path.resolve(root);
  const roots = [];
  let current = path.resolve(start);
  while (current === normalizedRoot || current.startsWith(`${normalizedRoot}${path.sep}`)) {
    roots.push(current);
    if (current === normalizedRoot) break;
    current = path.dirname(current);
  }
  return roots;
};

const sourceFileIndex = (project) =>
  new Map(
    projectSourceFiles(project).map((sourceFile) => [sourceFileIdentity(sourceFile), sourceFile]),
  );

const requestedEntryPoints = (requested, sourceFiles) =>
  requested
    .map((entry) => sourceFiles.get(canonicalFileIdentity(entry.absolutePath)))
    .filter(Boolean);

const packageEntryTargets = (packageJson) => {
  const targets = new Set();
  ["exports", "types", "typings", "module", "main"].forEach((field) =>
    packageTargets(packageJson[field], targets),
  );
  if (targets.size === 0) targets.add("src/index.ts");
  return targets;
};

const addEntry = (entries, sourceFile) => {
  if (sourceFile && !entries.includes(sourceFile)) entries.push(sourceFile);
};

const addWildcardEntries = (entries, sourceFiles, packageRoot, target) => {
  const pattern = wildcardPattern(target);
  for (const sourceFile of sourceFiles.values()) {
    if (pattern.test(relativePath(packageRoot, sourceFile.fileName))) {
      addEntry(entries, sourceFile);
    }
  }
};

const addDirectEntry = (entries, sourceFiles, packageRoot, target) => {
  const sourceFile = sourceCandidatesForTarget(packageRoot, target)
    .map((candidate) => sourceFiles.get(canonicalFileIdentity(candidate)))
    .find(Boolean);
  addEntry(entries, sourceFile);
};

const addPackageEntry = (entries, sourceFiles, packageRoot, target) => {
  if (target.includes("*")) {
    addWildcardEntries(entries, sourceFiles, packageRoot, target);
    return;
  }
  addDirectEntry(entries, sourceFiles, packageRoot, target);
};

const addPathEntry = (entries, sourceFiles, roots, target) => {
  roots.forEach((candidateRoot) =>
    sourceCandidatesForTarget(candidateRoot, target).forEach((candidate) =>
      addEntry(entries, sourceFiles.get(canonicalFileIdentity(candidate))),
    ),
  );
};

const packagePathTargets = (project, packageJson) => {
  const packageName = packageJson.name;
  if (typeof packageName !== "string") return [];
  return project.compilerOptions.paths?.[packageName] ?? [];
};

const discoverEntryPoints = (root, project, requested) => {
  const sourceFiles = sourceFileIndex(project);
  if (requested.length > 0) return requestedEntryPoints(requested, sourceFiles);
  const packageInfo = projectPackage(root, project);
  const entries = [];
  packageEntryTargets(packageInfo.json).forEach((target) =>
    addPackageEntry(entries, sourceFiles, packageInfo.root, target),
  );
  const roots = ancestorRoots(root, packageInfo.root);
  packagePathTargets(project, packageInfo.json).forEach((target) =>
    addPathEntry(entries, sourceFiles, roots, target),
  );
  return entries;
};

const isPrivateMember = (node) =>
  Boolean(node.name && isPrivateIdentifier(node.name)) ||
  Boolean(node.modifiers?.some((modifier) => modifier.getText(node.getSourceFile()) === "private"));

const skipTypeReferenceNode = (declaration, node) => {
  if (isFunctionBody(node)) return true;
  return node !== declaration && isDeclaration(node) && isPrivateMember(node);
};

const scanTypeReferenceNode = (declaration, nodes, node) => {
  if (skipTypeReferenceNode(declaration, node)) return;
  if (isIdentifier(node) && isTypePosition(node)) nodes.push(node);
  node.forEachChild((child) => {
    scanTypeReferenceNode(declaration, nodes, child);
    return undefined;
  });
};

const typeReferenceNodes = (declaration) => {
  const nodes = [];
  scanTypeReferenceNode(declaration, nodes, declaration);
  return nodes;
};

const publicExportsFrom = (project, sourceFile) => {
  const moduleSymbol = project.checker.getSymbolAtLocation(sourceFile);
  if (!moduleSymbol) return [];
  return project.checker.getExportsOfModule(moduleSymbol).flatMap((exportedSymbol) => {
    const target = resolveAlias(project.checker, exportedSymbol);
    return declarationsForSymbol(project, target)
      .filter((declaration) => isProjectSource(project, declaration.getSourceFile()))
      .map((declaration) => {
        const namespace = declarationNamespaces(declaration).has("type") ? "type" : "value";
        return {
          exportedName: exportedSymbol.name,
          declaration,
          namespace,
          key: stableDeclarationKey(declaration, namespace),
        };
      });
  });
};

const collectPublicExports = (project, entryPoints) => {
  const exports = [];
  const publicKeys = new Set();
  entryPoints
    .flatMap((sourceFile) => publicExportsFrom(project, sourceFile))
    .forEach((entry) => {
      publicKeys.add(entry.key);
      exports.push(entry);
    });
  return { exports, publicKeys };
};

const publicTypeDeclaration = (project, declaration) =>
  isProjectSource(project, declaration.getSourceFile()) &&
  isDeclaration(declaration) &&
  declarationNamespaces(declaration).has("type");

const safeCheckerValue = (operation, fallback) => {
  try {
    return operation() ?? fallback;
  } catch {
    return fallback;
  }
};

const checkerNamedTypeDeclarations = (project, type) => {
  const symbols = [type.getAliasSymbol(), type.getSymbol()].filter(Boolean);
  const declarations = symbols.flatMap((symbol) =>
    declarationsForSymbol(project, resolveAlias(project.checker, symbol)),
  );
  return uniqueSorted(
    declarations.filter(
      (declaration) => isDeclaration(declaration) && declarationNamespaces(declaration).has("type"),
    ),
    (declaration) => stableDeclarationKey(declaration, "type"),
  );
};

const signatureTypes = (project, type, anchor) =>
  [SignatureKind.Call, SignatureKind.Construct].flatMap((kind) =>
    safeCheckerValue(() => project.checker.getSignaturesOfType(type, kind), []).flatMap(
      (signature) => [
        safeCheckerValue(() => project.checker.getReturnTypeOfSignature(signature), undefined),
        ...signature.getParameters().map((parameter) => {
          const declaration =
            declarationsForSymbol(project, parameter).find((candidate) =>
              isProjectSource(project, candidate.getSourceFile()),
            ) ?? anchor;
          return safeCheckerValue(
            () => project.checker.getTypeOfSymbolAtLocation(parameter, declaration),
            undefined,
          );
        }),
      ],
    ),
  );

const checkerTypeChildren = (project, type, anchor, hasNamedDeclaration) => {
  if (type.isTypeParameter()) {
    const constraint = safeCheckerValue(
      () => project.checker.getConstraintOfTypeParameter(type),
      undefined,
    );
    return constraint ? [constraint] : [];
  }
  const structural = [
    ...(type.getTypes() ?? []),
    ...safeCheckerValue(() => type.getAliasTypeArguments(), []),
    ...safeCheckerValue(() => project.checker.getTypeArguments(type), []),
  ].filter(Boolean);
  if (hasNamedDeclaration) return structural;
  const direct = [...structural, ...signatureTypes(project, type, anchor)];
  if (direct.length > 0) return direct;
  return safeCheckerValue(() => project.checker.getPropertiesOfType(type), []).flatMap(
    (property) => {
      const declaration =
        declarationsForSymbol(project, property).find((candidate) =>
          isProjectSource(project, candidate.getSourceFile()),
        ) ?? anchor;
      const propertyType = safeCheckerValue(
        () => project.checker.getTypeOfSymbolAtLocation(property, declaration),
        undefined,
      );
      return propertyType ? [propertyType] : [];
    },
  );
};

const publicApiEdge = (root, exported, declaration, evidence) => ({
  source: stableSymbolIdentity(
    root,
    exported.declaration,
    exported.namespace,
    exported.exportedName,
  ),
  target: stableSymbolIdentity(
    root,
    declaration,
    "type",
    declarationName(declaration) ?? "default",
  ),
  relation: "public API depends on",
  evidence,
});

const recordPublicApiEdge = (root, state, exported, declaration, evidence) => {
  const targetKey = stableDeclarationKey(declaration, "type");
  if (targetKey === exported.key) return;
  const edgeKey = `${exported.key}\0${targetKey}`;
  if (state.seenEdges.has(edgeKey)) return;
  state.seenEdges.add(edgeKey);
  const edge = publicApiEdge(root, exported, declaration, evidence);
  state.edges.push(edge);
  if (!state.publicKeys.has(targetKey)) {
    state.leaks.push({
      exposed_symbol: edge.source,
      private_declaration: edge.target,
      relation: "public-signature-private-type",
      evidence,
    });
  }
};

const scanCheckerType = (root, project, state, exported) => {
  const symbol = symbolForDeclaration(project, exported.declaration);
  if (!symbol) return;
  const initial = safeCheckerValue(
    () =>
      project.checker.getTypeOfSymbolAtLocation(
        resolveAlias(project.checker, symbol),
        exported.declaration,
      ),
    undefined,
  );
  if (!initial) return;
  const pending = [initial];
  const seen = new Set();
  const evidence = location(root, exported.declaration);
  while (pending.length > 0) {
    const type = pending.pop();
    if (!type || seen.has(type.id)) continue;
    seen.add(type.id);
    const namedDeclarations = checkerNamedTypeDeclarations(project, type);
    const declarations = namedDeclarations.filter((declaration) =>
      isProjectSource(project, declaration.getSourceFile()),
    );
    declarations.forEach((declaration) =>
      recordPublicApiEdge(root, state, exported, declaration, evidence),
    );
    pending.push(
      ...checkerTypeChildren(project, type, exported.declaration, namedDeclarations.length > 0),
    );
  }
};

const scanPublicExport = (root, project, state, exported) => {
  const nodes = typeReferenceNodes(exported.declaration);
  const symbols = project.checker.getSymbolAtLocation(nodes);
  nodes.forEach((node, index) => {
    const target = resolveAlias(project.checker, symbols[index]);
    declarationsForSymbol(project, target)
      .filter((declaration) => publicTypeDeclaration(project, declaration))
      .forEach((declaration) =>
        recordPublicApiEdge(root, state, exported, declaration, location(root, node)),
      );
  });
  scanCheckerType(root, project, state, exported);
};

const collectPublicApiEdges = (root, project, exports, publicKeys) => {
  const state = { edges: [], leaks: [], seenEdges: new Set(), publicKeys };
  exports.forEach((exported) => scanPublicExport(root, project, state, exported));
  return state;
};

const signatureFingerprint = (project, entry) => {
  let signature;
  try {
    const symbol = symbolForDeclaration(project, entry.declaration);
    const type = symbol
      ? project.checker.getTypeOfSymbolAtLocation(symbol, entry.declaration)
      : undefined;
    signature = type
      ? project.checker.typeToString(type, entry.declaration)
      : entry.declaration.getText(entry.declaration.getSourceFile());
  } catch {
    signature = entry.declaration.getText(entry.declaration.getSourceFile());
  }
  return `sha256:${createHash("sha256").update(signature.replace(/\s+/gu, " ")).digest("hex")}`;
};

const comparableSymbolIdentity = ({ path: filePath, namespace, local_name, line, col }) =>
  JSON.stringify([filePath, namespace, local_name, line, col]);

const sameSymbolIdentity = (left, right) =>
  comparableSymbolIdentity(left) === comparableSymbolIdentity(right);

const referencedTypes = (edges, exposed) =>
  edges
    .filter((edge) => sameSymbolIdentity(edge.source, exposed))
    .map((edge) => ({ declaration: edge.target, relation: edge.relation }))
    .toSorted((left, right) => compareText(JSON.stringify(left), JSON.stringify(right)));

const publicApiEntry = (root, project, edges, entry) => {
  const exposed = stableSymbolIdentity(
    root,
    entry.declaration,
    entry.namespace,
    entry.exportedName,
  );
  return {
    exposed,
    origin: exposed,
    signature_fingerprint: signatureFingerprint(project, entry),
    referenced_types: referencedTypes(edges, exposed),
  };
};

const publicApiGraph = (root, project, entryPoints, includeEntries = false) => {
  const { exports, publicKeys } = collectPublicExports(project, entryPoints);
  const { edges, leaks } = collectPublicApiEdges(root, project, exports, publicKeys);
  const entries = includeEntries
    ? exports.map((entry) => publicApiEntry(root, project, edges, entry))
    : [];
  return { exports, entries, edges, leaks };
};

const graphProjects = (snapshot, explicitProjects) =>
  (explicitProjects.length > 0 ? explicitProjects : snapshot.getProjects()).filter(
    (project) => project.program.getSourceFileNames().length > 0,
  );

const readyProjectStates = (query, states) => {
  const ready = states.filter((state) => state.status === "complete");
  if (ready.length > 0) return { ready };
  const reasonCode = unavailableProjectReason(states);
  return {
    error: unavailable(query, reasonCode, actionForReason(reasonCode)),
  };
};

const graphCollection = () => ({
  exports: [],
  entries: [],
  leaks: [],
  edges: [],
  resolvedEntryPoints: new Set(),
});

const collectApiSurfaceGraph = (root, query, ready) => {
  const collected = graphCollection();
  ready.forEach((state) => {
    const entryPoints = discoverEntryPoints(root, state.project, query.entryPoints);
    entryPoints.forEach((entryPoint) =>
      collected.resolvedEntryPoints.add(sourceFileIdentity(entryPoint)),
    );
    const graph = publicApiGraph(root, state.project, entryPoints, true);
    collected.exports.push(
      ...graph.exports.map((entry) =>
        stableSymbolIdentity(root, entry.declaration, entry.namespace, entry.exportedName),
      ),
    );
    collected.entries.push(...graph.entries);
    collected.leaks.push(...graph.leaks);
    collected.edges.push(...graph.edges);
  });
  return collected;
};

const uniqueSorted = (values, projection = (value) => value) =>
  [...new Map(values.map((value) => [JSON.stringify(projection(value)), value])).values()].toSorted(
    (left, right) =>
      compareText(JSON.stringify(projection(left)), JSON.stringify(projection(right))),
  );

const missingEntryPoints = (query, resolvedEntryPoints) =>
  query.entryPoints.filter(
    (entryPoint) => !resolvedEntryPoints.has(canonicalFileIdentity(entryPoint.absolutePath)),
  ).length;

const confirmedPrivateLeakIds = (query, leaks) => {
  const confirmedLeakKeys = new Set(
    leaks.map((leak) =>
      JSON.stringify([
        leak.evidence.path,
        leak.exposed_symbol.exported_name,
        leak.private_declaration.local_name,
      ]),
    ),
  );
  return query.privateLeakCandidates
    .filter((candidate) =>
      confirmedLeakKeys.has(
        JSON.stringify([candidate.path, candidate.exportName, candidate.typeName]),
      ),
    )
    .map((candidate) => candidate.id)
    .toSorted((left, right) => left - right);
};

const boundedApiEntries = (entries, evidenceLimit) => {
  let omissionCount = 0;
  const bounded = entries.slice(0, evidenceLimit).map((entry) => {
    omissionCount += Math.max(0, entry.referenced_types.length - evidenceLimit);
    return {
      ...entry,
      referenced_types: entry.referenced_types.slice(0, evidenceLimit),
      total_referenced_type_count: entry.referenced_types.length,
    };
  });
  return { bounded, omissionCount };
};

const apiEvidenceOmissionCount = (collections, evidenceLimit, nestedCount) =>
  collections.reduce(
    (count, entries) => count + Math.max(0, entries.length - evidenceLimit),
    nestedCount,
  );

const apiEvidence = (leaks, edges) => (leaks.length > 0 ? leaks : edges);
const apiAssertion = (leaks) => (leaks.length > 0 ? "leak-confirmed" : "no-leak-confirmed");
const apiConfirmationComplete = (unavailableCount, missingCount) =>
  unavailableCount === 0 && missingCount === 0;

const analyzeApiSurface = (root, query, states, evidenceLimit) => {
  const readiness = readyProjectStates(query, states);
  if (readiness.error) return readiness.error;
  const collected = collectApiSurfaceGraph(root, query, readiness.ready);
  const orderedExports = uniqueSorted(collected.exports);
  const orderedEntries = uniqueSorted(collected.entries);
  const orderedLeaks = uniqueSorted(collected.leaks);
  const orderedEdges = uniqueSorted(collected.edges);
  const missingEntryPointCount = missingEntryPoints(query, collected.resolvedEntryPoints);
  const confirmedCandidateIds = confirmedPrivateLeakIds(query, orderedLeaks);
  const unavailableProjectCount = states.length - readiness.ready.length;
  const boundedEntries = boundedApiEntries(orderedEntries, evidenceLimit);
  const omissionCount = apiEvidenceOmissionCount(
    [orderedExports, orderedEntries, orderedLeaks, orderedEdges],
    evidenceLimit,
    boundedEntries.omissionCount,
  );
  return boundedResult({
    query,
    assertion: apiAssertion(orderedLeaks),
    evidence: apiEvidence(orderedLeaks, orderedEdges),
    evidenceLimit,
    data: {
      exports: orderedExports.slice(0, evidenceLimit),
      total_export_count: orderedExports.length,
      entries: boundedEntries.bounded,
      total_entry_count: orderedEntries.length,
      leaks: orderedLeaks.slice(0, evidenceLimit),
      private_leak_confirmation: {
        requested_candidate_count: query.privateLeakCandidates.length,
        confirmation_complete: apiConfirmationComplete(
          unavailableProjectCount,
          missingEntryPointCount,
        ),
        confirmed_candidate_ids: confirmedCandidateIds,
      },
      total_leak_count: orderedLeaks.length,
      public_signature_edges: orderedEdges.slice(0, evidenceLimit),
      total_public_signature_edge_count: orderedEdges.length,
    },
    omissions: [
      {
        reason_code: "evidence-limit",
        count: omissionCount,
      },
      ...unavailableProjectOmissions(states),
      { reason_code: "unknown-entry-point", count: missingEntryPointCount },
    ],
  });
};

const emptySemanticAnalysis = (queries) => ({
  selectedTsconfigs: [],
  projectResults: [],
  results: queries.map((query) =>
    unavailable(
      query,
      "no-project",
      "Pass an explicit tsconfig with --type-aware-project and retry.",
    ),
  ),
  phaseTimings: { project_setup: 0, diagnostics: 0, semantic_queries: 0 },
  warnings: [],
});

const resolveSemanticQueries = ({
  root,
  snapshot,
  explicitProjects,
  statesByProject,
  queries,
  allowDefaultFallback,
}) => {
  const resolvedByQuery = new Map();
  for (const query of queries) {
    if (!query.symbol) continue;
    resolvedByQuery.set(
      query.id,
      resolveSymbolQuery(
        root,
        snapshot,
        explicitProjects,
        statesByProject,
        query,
        allowDefaultFallback,
      ),
    );
  }
  return resolvedByQuery;
};

const runSymbolUseBatch = (root, queries, resolvedByQuery, semanticProjects, evidenceLimit) => {
  const resolvedSymbolUses = queries
    .filter((query) => query.symbol)
    .flatMap((query) => {
      const resolved = resolvedByQuery.get(query.id);
      return resolved?.error ? [] : [{ query, resolved }];
    });
  if (resolvedSymbolUses.length === 0) {
    return {
      evidenceByQuery: new Map(),
      totalByQuery: new Map(),
      directFilesByQuery: new Map(),
      aliasHopTotalByQuery: new Map(),
      contractRelationsByQuery: new Map(),
      uncertaintiesByQuery: new Map(),
      sourceScanCount: 0,
    };
  }
  return batchSymbolUseEvidence(root, resolvedSymbolUses, semanticProjects, evidenceLimit);
};

const analyzeGraphSemanticQuery = (root, query, graphStates, evidenceLimit) => {
  if (query.operation === "api-surface") {
    return analyzeApiSurface(root, query, graphStates, evidenceLimit);
  }
  return analyzeTypeCoupling(
    { root, query, states: graphStates, evidenceLimit },
    {
      boundedResult,
      discoverEntryPoints,
      missingEntryPoints,
      publicApiGraph,
      readyProjectStates,
      unavailableProjectOmissions,
      uniqueSorted,
    },
  );
};

const analyzeSymbolUseQuery = (root, query, resolved, evidenceLimit, symbolUseBatch) => {
  const evidence = symbolUseBatch.evidenceByQuery.get(query.id);
  const total = symbolUseBatch.totalByQuery.get(query.id);
  return analyzeSymbolUse(
    root,
    query,
    resolved,
    evidenceLimit,
    evidence === undefined ? [] : evidence,
    total === undefined ? 0 : total,
    symbolUseBatch.uncertaintiesByQuery.get(query.id) ?? new Set(),
  );
};

const analyzeResolvedSemanticQuery = ({
  root,
  query,
  resolved,
  evidenceLimit,
  symbolUseBatch,
  semanticProjects,
}) => {
  if (resolved.error) return resolved.error;
  if (query.operation === "symbol-use") {
    return analyzeSymbolUseQuery(root, query, resolved, evidenceLimit, symbolUseBatch);
  }
  const references = symbolUseBatch.evidenceByQuery.get(query.id) ?? [];
  const totalReferenceCount = symbolUseBatch.totalByQuery.get(query.id) ?? 0;
  if (query.operation === "symbol-trace") {
    return analyzeSymbolTrace(
      root,
      query,
      resolved,
      evidenceLimit,
      references,
      totalReferenceCount,
      symbolUseBatch.aliasHopTotalByQuery.get(query.id) ?? 0,
    );
  }
  return analyzeSymbolImpact(
    {
      root,
      query,
      resolved,
      evidenceLimit,
      scanProjects: semanticProjects,
      directReferenceFiles: symbolUseBatch.directFilesByQuery.get(query.id) ?? new Set(),
    },
    { boundedResult },
  );
};

const analyzeSemanticQuery = (context, query) => {
  if (query.operation === "api-surface" || query.operation === "type-coupling") {
    return analyzeGraphSemanticQuery(
      context.root,
      query,
      context.graphStates,
      context.evidenceLimit,
    );
  }
  return analyzeResolvedSemanticQuery({
    ...context,
    query,
    resolved: context.resolvedByQuery.get(query.id),
  });
};

const recordAbstainedProjectOutcome = (state, result) => {
  state.abstained_count += 1;
  const projectFailure = new Set([
    "no-project",
    "ambiguous-project",
    "blocking-diagnostics",
    "svelte-virtual-module-exports",
    "unknown-symbol",
    "incomplete-project-coverage",
  ]);
  if (!projectFailure.has(result?.reasonCode)) return;
  if (state.reason_code !== null) return;
  state.reason_code =
    result === undefined || result.reasonCode === null ? "unsupported-syntax" : result.reasonCode;
};

const recordProjectOutcome = (state, query, result) => {
  state.candidate_count += 1;
  if (result.assertion === "confirmed-used") {
    state.confirmed_used_count += 1;
    return;
  }
  if (result.assertion === "contract-preserved") {
    state.contract_preserved_count += 1;
    return;
  }
  if (result.assertion === "confirmed-no-static-references") {
    state.no_static_references_count += 1;
    if (result.data.closed_world_eligible && query.symbol.declarationKind === "class_method") {
      state.fix_eligible_count += 1;
    }
    return;
  }
  if (result.status === "complete") {
    state.unresolved_count += 1;
    return;
  }
  recordAbstainedProjectOutcome(state, result);
};

const markUnavailableProject = (state) => {
  state.status = "unavailable";
};

const hasAbstainedSemanticQuery = (state) =>
  state.status === "complete" && state.abstained_count > 0;

const recordQueryProjectOutcome = (query, resolvedByQuery, resultsByQuery) => {
  if (query.operation !== "symbol-use") return;
  const resolved = resolvedByQuery.get(query.id);
  if (resolved === undefined) return;
  const states =
    resolved.ownerContexts?.map(({ state }) => state) ?? [resolved.state].filter(Boolean);
  [...new Set(states)].forEach((state) =>
    recordProjectOutcome(state, query, resultsByQuery.get(query.id)),
  );
};

const recordProjectOutcomes = (states, queries, resolvedByQuery, results) => {
  const resultsByQuery = new Map(results.map((result) => [result.queryId, result]));
  queries.forEach((query) => recordQueryProjectOutcome(query, resolvedByQuery, resultsByQuery));
  states.filter(hasAbstainedSemanticQuery).forEach(markUnavailableProject);
};

const recordProgramReuse = (states, graphStates, queries, resolvedByQuery) => {
  const graphQueryCount = queries.filter(
    ({ operation }) => operation === "api-surface" || operation === "type-coupling",
  ).length;
  const graphProjectSet = new Set(graphStates.map(({ project }) => project));
  for (const state of states) {
    let queryCount = graphProjectSet.has(state.project) ? graphQueryCount : 0;
    for (const query of queries) {
      if (!query.symbol) continue;
      const resolved = resolvedByQuery.get(query.id);
      if (resolved?.ownerContexts?.some(({ project }) => project === state.project)) {
        queryCount += 1;
      }
    }
    state.program_reused = queryCount > 1;
  }
};

const emptyRequestedAnalysis = () => ({
  selectedTsconfigs: [],
  projectResults: [],
  results: [],
  phaseTimings: { project_setup: 0, diagnostics: 0, semantic_queries: 0 },
  warnings: [],
});

const semanticOpenFiles = (queries) =>
  queries.filter((query) => query.symbol).map((query) => query.symbol.absolutePath);

const semanticOpenProjects = (root, projects) => {
  if (projects.length > 0) return projects.map((project) => project.absolutePath);
  const conventionalProject = path.join(root, "tsconfig.json");
  return existsSync(conventionalProject) ? [conventionalProject] : [];
};

const semanticProjectSelection = (snapshot, openProjects, openFiles, allowDefaultFallback) => {
  const explicitProjects = openProjects
    .map((project) => snapshot.getProject(project))
    .filter(Boolean);
  const symbolProjects = openFiles
    .map((file) => selectProjectForSymbol(snapshot, explicitProjects, file, allowDefaultFallback))
    .filter(Boolean);
  const selectedProjects = [
    ...new Set([...graphProjects(snapshot, explicitProjects), ...symbolProjects]),
  ];
  return { explicitProjects, selectedProjects };
};

const semanticSetup = (root, projects, queries, api, sessionState) => {
  const openFiles = semanticOpenFiles(queries);
  const openProjects = semanticOpenProjects(root, projects);
  const openFileSet = new Set(openFiles);
  const openProjectSet = new Set(openProjects);
  const newlyOpenedFiles = sessionState
    ? openFiles.filter((file) => !sessionState.openFiles.has(file))
    : openFiles;
  const newlyOpenedProjects = sessionState
    ? openProjects.filter((project) => !sessionState.openProjects.has(project))
    : openProjects;
  const closeFiles = sessionState
    ? [...sessionState.openFiles].filter((file) => !openFileSet.has(file))
    : [];
  const closeProjects = sessionState
    ? [...sessionState.openProjects].filter((project) => !openProjectSet.has(project))
    : [];
  const snapshot = api.updateSnapshot({
    openFiles: [...new Set(newlyOpenedFiles)],
    openProjects: newlyOpenedProjects,
    closeFiles,
    closeProjects,
    ...(sessionState?.fileChanges ? { fileChanges: sessionState.fileChanges } : {}),
  });
  if (sessionState) {
    const previousSnapshot = sessionState.snapshot;
    sessionState.snapshot = snapshot;
    sessionState.openFiles = openFileSet;
    sessionState.openProjects = openProjectSet;
    previousSnapshot?.dispose();
  }
  const allowDefaultFallback = projects.length === 0;
  return {
    snapshot,
    openFiles,
    allowDefaultFallback,
    ...semanticProjectSelection(snapshot, openProjects, openFiles, allowDefaultFallback),
  };
};

const semanticStates = (root, projects, selectedProjects) => {
  const source = projects.length > 0 ? "explicit" : "auto";
  return selectedProjects.map((project) => projectState(root, project, source));
};

const graphProjectStates = (states, explicitProjects) => {
  const explicitProjectSet = new Set(explicitProjects);
  if (explicitProjectSet.size === 0) return states;
  return states.filter((state) => explicitProjectSet.has(state.project));
};

const executeSemanticAnalysis = ({
  root,
  projects,
  queries,
  evidenceLimit,
  setupStartedAt,
  setup,
}) => {
  if (setup.selectedProjects.length === 0) return emptySemanticAnalysis(queries);
  const projectSetupMs = performance.now() - setupStartedAt;
  const diagnosticsStartedAt = performance.now();
  const states = semanticStates(root, projects, setup.selectedProjects);
  const diagnosticsMs = performance.now() - diagnosticsStartedAt;
  const statesByProject = new Map(states.map((state) => [state.project, state]));
  const graphStates = graphProjectStates(states, setup.explicitProjects);
  const queryStartedAt = performance.now();
  const resolvedByQuery = resolveSemanticQueries({
    root,
    snapshot: setup.snapshot,
    explicitProjects: setup.explicitProjects,
    statesByProject,
    queries,
    allowDefaultFallback: setup.allowDefaultFallback,
  });
  const semanticProjects = states
    .filter((state) => state.status === "complete")
    .map((state) => state.project);
  const symbolUseBatch = runSymbolUseBatch(
    root,
    queries,
    resolvedByQuery,
    semanticProjects,
    evidenceLimit,
  );
  const context = {
    root,
    graphStates,
    evidenceLimit,
    resolvedByQuery,
    symbolUseBatch,
    semanticProjects,
  };
  const results = queries.map((query) => analyzeSemanticQuery(context, query));
  recordProjectOutcomes(states, queries, resolvedByQuery, results);
  recordProgramReuse(states, graphStates, queries, resolvedByQuery);
  return {
    selectedTsconfigs: states.map((state) => state.config),
    projectResults: states.map(projectResult),
    results,
    phaseTimings: {
      project_setup: projectSetupMs,
      diagnostics: diagnosticsMs,
      semantic_queries: performance.now() - queryStartedAt,
    },
    warnings: [],
    sourceScanCount: symbolUseBatch.sourceScanCount,
    referenceScanCount: 0,
  };
};

const executeDisposableSemanticAnalysis = (input) => {
  try {
    return executeSemanticAnalysis(input);
  } finally {
    input.setup.snapshot.dispose();
  }
};

export const analyzeSemanticQueries = (
  { root, projects, queries, evidenceLimit },
  { createApi = (cwd) => new API({ cwd }) } = {},
) => {
  if (queries.length === 0) return emptyRequestedAnalysis();
  const setupStartedAt = performance.now();
  const api = createApi(root);
  try {
    return executeDisposableSemanticAnalysis({
      root,
      projects,
      queries,
      evidenceLimit,
      setupStartedAt,
      setup: semanticSetup(root, projects, queries, api),
    });
  } finally {
    api.close();
  }
};

export const createSemanticSession = (root, { createApi = (cwd) => new API({ cwd }) } = {}) => {
  const api = createApi(root);
  const state = {
    openFiles: new Set(),
    openProjects: new Set(),
    fileChanges: undefined,
    revision: 0,
    analyzed: false,
    closed: false,
    snapshot: undefined,
  };
  return {
    analyze(request, { revision, fileChanges } = {}) {
      if (state.closed) throw new Error("semantic session is closed");
      if (request.root !== root) throw new Error("semantic session root mismatch");
      if (!Number.isSafeInteger(revision) || revision <= state.revision) {
        throw new Error("semantic session revision must increase");
      }
      state.fileChanges = fileChanges;
      const setupStartedAt = performance.now();
      const result = executeSemanticAnalysis({
        root,
        projects: request.projects,
        queries: request.queries,
        evidenceLimit: request.evidenceLimit,
        setupStartedAt,
        setup: semanticSetup(root, request.projects, request.queries, api, state),
      });
      const invalidationKind = fileChanges?.invalidateAll
        ? "full"
        : fileChanges
          ? "incremental"
          : state.analyzed
            ? "none"
            : "full";
      result.projectResults = result.projectResults.map((project) => ({
        ...project,
        program_reused_from_previous_snapshot: state.analyzed && invalidationKind !== "full",
        snapshot_revision: revision,
        invalidation_kind: invalidationKind,
      }));
      state.revision = revision;
      state.analyzed = true;
      state.fileChanges = undefined;
      return result;
    },
    close() {
      if (state.closed) return;
      state.closed = true;
      state.snapshot?.dispose();
      api.close();
    },
  };
};
