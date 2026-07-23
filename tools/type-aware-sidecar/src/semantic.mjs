import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";

import { API } from "typescript/unstable/sync";
import {
  isCallExpression,
  isClassDeclaration,
  isClassExpression,
  isEnumDeclaration,
  isElementAccessExpression,
  isExportDeclaration,
  isExportSpecifier,
  isFunctionBody,
  isFunctionDeclaration,
  isIdentifier,
  isImportDeclaration,
  isImportClause,
  isImportSpecifier,
  isInterfaceDeclaration,
  isMethodDeclaration,
  isGetAccessorDeclaration,
  isPropertyAccessExpression,
  isPropertyDeclaration,
  isPropertySignatureDeclaration,
  isPrivateIdentifier,
  isSetAccessorDeclaration,
  isStringLiteralLikeNode,
  isTypeAliasDeclaration,
  isTypeNode,
  isTypeQueryNode,
  isVariableDeclaration,
} from "typescript/unstable/ast/is";

import { canonicalFileIdentity } from "./typescript-go.mjs";

const INFERRED_PROJECT = "<inferred>";
const TEST_FILE_PATTERN = /(?:^|[/_.-])(?:test|spec)\.[cm]?[jt]sx?$/u;
const MAX_SYMBOL_USE_EVIDENCE = 1;
const DECLARATION_KINDS = new Set([
  "export",
  "class",
  "interface",
  "type_alias",
  "enum",
  "function",
  "variable",
  "class_method",
  "class_property",
]);

const compareText = (left, right) => Buffer.compare(Buffer.from(left), Buffer.from(right));
const slash = (value) => value.split(path.sep).join("/");
const relativePath = (root, fileName) =>
  slash(path.relative(canonicalFileIdentity(root), canonicalFileIdentity(fileName)));

const sourceFileIdentity = (sourceFile) => canonicalFileIdentity(sourceFile.fileName);
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

const positions = (node) => {
  const sourceFile = node.getSourceFile();
  const nodes = node.name ? [node, node.name] : [node];
  return nodes.map((candidate) => {
    const start = candidate.getStart(sourceFile);
    const { line } = sourceFile.getLineAndCharacterOfPosition(start);
    const lineStart = sourceFile.getPositionOfLineAndCharacter(line, 0);
    return {
      line: line + 1,
      col: Buffer.byteLength(sourceFile.text.slice(lineStart, start), "utf8"),
    };
  });
};

const declarationKind = (node) => {
  if (isClassDeclaration(node) || isClassExpression(node)) return "class";
  if (isInterfaceDeclaration(node)) return "interface";
  if (isTypeAliasDeclaration(node)) return "type_alias";
  if (isEnumDeclaration(node)) return "enum";
  if (isFunctionDeclaration(node)) return "function";
  if (isVariableDeclaration(node)) return "variable";
  if (
    isMethodDeclaration(node) ||
    isGetAccessorDeclaration(node) ||
    isSetAccessorDeclaration(node)
  ) {
    return "class_method";
  }
  if (isPropertyDeclaration(node) || isPropertySignatureDeclaration(node)) return "class_property";
  return undefined;
};

const declarationNamespaces = (node) => {
  if (
    isInterfaceDeclaration(node) ||
    isTypeAliasDeclaration(node) ||
    isPropertySignatureDeclaration(node)
  ) {
    return new Set(["type"]);
  }
  if (isClassDeclaration(node) || isClassExpression(node) || isEnumDeclaration(node)) {
    return new Set(["type", "value"]);
  }
  return new Set(["value"]);
};

const declarationName = (node) => node.name?.text;

const declarationOwner = (node) => {
  let current = node.parent;
  while (current) {
    if (
      isClassDeclaration(current) ||
      isClassExpression(current) ||
      isInterfaceDeclaration(current)
    ) {
      return current.name?.text ?? null;
    }
    current = current.parent;
  }
  return null;
};

const isDeclaration = (node) => declarationKind(node) !== undefined;

const visit = (node, callback) => {
  callback(node);
  node.forEachChild((child) => {
    visit(child, callback);
    return undefined;
  });
};

const declarationIndexes = new WeakMap();

const anchorKey = ({ declarationKind: kind, localName, exportedName, owner, line, col }) =>
  [kind, localName, exportedName, owner ?? "", line, col].join("\0");

const declarationIndex = (sourceFile) => {
  const cached = declarationIndexes.get(sourceFile);
  if (cached) return cached;
  const index = new Map();
  visit(sourceFile, (node) => {
    if (isExportSpecifier(node)) {
      const exportedName = node.name?.text;
      const localName = node.propertyName?.text ?? exportedName;
      const exportPositions = [node, node.name, node.propertyName]
        .filter(Boolean)
        .flatMap((candidate) => positions(candidate));
      for (const position of exportPositions) {
        for (const identityLocalName of new Set([localName, exportedName])) {
          index.set(
            anchorKey({
              declarationKind: "export",
              localName: identityLocalName,
              exportedName,
              owner: null,
              ...position,
            }),
            node,
          );
        }
      }
      return;
    }
    if (!isDeclaration(node)) return;
    const defaultModifier = node.modifiers?.find(
      (modifier) => modifier.getText(node.getSourceFile()) === "default",
    );
    const localName = declarationName(node) ?? (defaultModifier ? "default" : undefined);
    if (!localName) return;
    for (const position of positions(node)) {
      const common = {
        localName,
        exportedName: localName,
        owner: declarationOwner(node),
        ...position,
      };
      index.set(anchorKey({ declarationKind: declarationKind(node), ...common }), node);
      index.set(anchorKey({ declarationKind: "export", ...common }), node);
    }
    if (defaultModifier) {
      for (const position of [...positions(node), ...positions(defaultModifier)]) {
        for (const identityLocalName of new Set([localName, "default"])) {
          index.set(
            anchorKey({
              declarationKind: "export",
              localName: identityLocalName,
              exportedName: "default",
              owner: declarationOwner(node),
              ...position,
            }),
            node,
          );
        }
      }
    }
  });
  declarationIndexes.set(sourceFile, index);
  return index;
};

const findDeclaration = (sourceFile, identity) => {
  const anchor = declarationIndex(sourceFile).get(anchorKey(identity));
  if (!anchor || isExportSpecifier(anchor)) return anchor;
  return declarationNamespaces(anchor).has(identity.namespace) ? anchor : undefined;
};

const symbolForDeclaration = (project, declaration) =>
  project.checker.getSymbolAtLocation(declaration.name ?? declaration);

const resolveAlias = (checker, symbol) => {
  if (!symbol) return undefined;
  try {
    const aliased = checker.getAliasedSymbol(symbol);
    return checker.isUnknownSymbol(aliased) ? symbol : aliased;
  } catch {
    return symbol;
  }
};

const declarationsForSymbol = (project, symbol) =>
  (symbol?.declarations ?? []).map((handle) => handle.resolve(project)).filter(Boolean);

const stableDeclarationKey = (node, namespace = "value") => {
  const first = positions(node)[0];
  return [
    sourceFileIdentity(node.getSourceFile()),
    namespace,
    declarationKind(node) ?? "unknown",
    declarationName(node) ?? "default",
    first.line,
    first.col,
    declarationOwner(node) ?? "",
  ].join("\0");
};

const stableSymbolIdentity = (root, declaration, namespace, exportedName) => {
  const position = positions(declaration)[0];
  return {
    path: relativePath(root, declaration.getSourceFile().fileName),
    namespace,
    declaration_kind: declarationKind(declaration) ?? "unknown",
    exported_name: exportedName,
    local_name: declarationName(declaration) ?? exportedName,
    line: position.line,
    col: position.col,
    owner: declarationOwner(declaration),
  };
};

const isProjectSource = (project, sourceFile) =>
  Boolean(sourceFile) &&
  !sourceFile.isDeclarationFile &&
  !project.program.isSourceFileDefaultLibrary(sourceFile) &&
  !project.program.isSourceFileFromExternalLibrary(sourceFile);

const projectSourceFiles = (project) =>
  project.program
    .getSourceFileNames()
    .map((fileName) => project.program.getSourceFile(fileName))
    .filter((sourceFile) => isProjectSource(project, sourceFile));

const projectExportIndexes = new WeakMap();

const projectExportIndex = (project) => {
  const cached = projectExportIndexes.get(project);
  if (cached) return cached;
  const index = new Set();
  for (const sourceFile of projectSourceFiles(project)) {
    const moduleSymbol = project.checker.getSymbolAtLocation(sourceFile);
    if (!moduleSymbol) continue;
    for (const exported of project.checker.getExportsOfModule(moduleSymbol)) {
      const target = resolveAlias(project.checker, exported);
      for (const declaration of declarationsForSymbol(project, target)) {
        for (const namespace of declarationNamespaces(declaration)) {
          index.add(`${exported.name}\0${stableDeclarationKey(declaration, namespace)}`);
        }
      }
    }
  }
  projectExportIndexes.set(project, index);
  return index;
};

const blockingDiagnosticCount = (project) =>
  project.program.getConfigFileParsingDiagnostics().length +
  project.program.getProgramDiagnostics().length +
  project.program.getSyntacticDiagnostics().length +
  project.program.getBindDiagnostics().length;

const configPath = (root, project) => {
  const normalized = slash(project.configFileName);
  if (normalized.endsWith("/dev/null/inferred")) return INFERRED_PROJECT;
  return relativePath(root, project.configFileName) || path.basename(project.configFileName);
};

const projectState = (root, project, source) => {
  const diagnosticCount = blockingDiagnosticCount(project);
  return {
    project,
    config: configPath(root, project),
    source,
    status: diagnosticCount === 0 ? "complete" : "unavailable",
    reason_code: diagnosticCount === 0 ? null : "blocking-diagnostics",
    blocking_diagnostic_count: diagnosticCount,
    source_file_count: project.program.getSourceFileNames().length,
    program_reused: true,
  };
};

const projectResult = ({ project: _project, ...state }) => state;

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

const actionForReason = (reasonCode) => {
  if (reasonCode === "dynamic-behavior") {
    return "Review dynamic imports and registrations before changing this symbol.";
  }
  if (reasonCode === "blocking-diagnostics") {
    return "Repair structural TypeScript diagnostics in every selected project and retry.";
  }
  if (reasonCode === "unknown-entry-point") {
    return "Refresh the package entry points or pass project-relative source entry points.";
  }
  return "Narrow the query or raise the caller-side evidence budget within the protocol limit.";
};

const boundedResult = ({
  query,
  assertion,
  evidence,
  data,
  evidenceLimit,
  totalEvidenceCount,
  omissions: extraOmissions = [],
}) => {
  const orderedEvidence = [...evidence].toSorted((left, right) =>
    compareText(JSON.stringify(left), JSON.stringify(right)),
  );
  const totalEvidence = totalEvidenceCount ?? orderedEvidence.length;
  const returnedEvidenceCount = Math.min(orderedEvidence.length, evidenceLimit);
  const evidenceTruncated = totalEvidence > returnedEvidenceCount;
  const rawOmissions = [];
  if (evidenceTruncated) {
    rawOmissions.push({
      reason_code: "evidence-limit",
      count: totalEvidence - returnedEvidenceCount,
    });
  }
  rawOmissions.push(...extraOmissions.filter((omission) => omission.count > 0));
  const omissionCounts = new Map();
  for (const omission of rawOmissions) {
    omissionCounts.set(
      omission.reason_code,
      (omissionCounts.get(omission.reason_code) ?? 0) + omission.count,
    );
  }
  const reasonPriority = new Map([
    ["blocking-diagnostics", 0],
    ["unknown-entry-point", 1],
    ["dynamic-behavior", 2],
    ["evidence-limit", 3],
  ]);
  const omissions = [...omissionCounts]
    .map(([reason_code, count]) => ({ reason_code, count }))
    .toSorted(
      (left, right) =>
        (reasonPriority.get(left.reason_code) ?? 10) -
          (reasonPriority.get(right.reason_code) ?? 10) ||
        compareText(left.reason_code, right.reason_code),
    );
  const partial = omissions.length > 0;
  const reasonCode = partial ? omissions[0].reason_code : null;
  const truncated = omissions.some((omission) => omission.reason_code === "evidence-limit");
  return {
    queryId: query.id,
    operation: query.operation,
    assertion,
    status: partial ? "partial" : "complete",
    reasonCode,
    actions: partial ? omissions.map((omission) => actionForReason(omission.reason_code)) : [],
    evidence: orderedEvidence.slice(0, evidenceLimit),
    totalEvidenceCount: totalEvidence,
    truncated,
    omissions,
    data,
  };
};

const isTypePosition = (node) => {
  let current = node.parent;
  while (current) {
    if (isTypeQueryNode(current)) return false;
    if (isImportSpecifier(current) || isExportSpecifier(current)) {
      return Boolean(current.isTypeOnly || current.parent?.parent?.isTypeOnly);
    }
    if (isTypeNode(current)) return true;
    if (isDeclaration(current) || isImportDeclaration(current) || isExportDeclaration(current)) {
      return false;
    }
    current = current.parent;
  }
  return false;
};

const referenceRole = (node) => {
  let current = node;
  while (current) {
    if (isImportSpecifier(current)) return current.isTypeOnly ? "type-import" : "import";
    if (isExportSpecifier(current)) return current.isTypeOnly ? "type-re-export" : "re-export";
    if (isCallExpression(current)) return "call";
    if (isPropertyAccessExpression(current)) return "property-access";
    if (isTypePosition(node)) return "type-reference";
    if (isDeclaration(current)) break;
    current = current.parent;
  }
  return isTypePosition(node) ? "type-reference" : "value-reference";
};

const aliasHop = (root, node) => {
  let current = node;
  while (current) {
    if (isImportSpecifier(current) || isExportSpecifier(current)) {
      const fromName = current.propertyName?.text ?? current.name?.text;
      const toName = current.name?.text ?? fromName;
      return {
        ...location(root, current),
        from_name: fromName,
        to_name: toName,
        relation: isImportSpecifier(current) ? "import-alias" : "re-export",
      };
    }
    if (isDeclaration(current)) break;
    current = current.parent;
  }
  return undefined;
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

const referenceEvidence = (root, ownerProject, declaration, identity, scanProjects) => {
  const targetSymbol = resolveAlias(
    ownerProject.checker,
    symbolForDeclaration(ownerProject, declaration),
  );
  if (!targetSymbol) return [];
  const targetKeys = new Set(
    declarationsForSymbol(ownerProject, targetSymbol)
      .filter((node) => declarationNamespaces(node).has(identity.namespace))
      .map((node) => stableDeclarationKey(node, identity.namespace)),
  );
  const declarationLocation = location(root, declaration.name ?? declaration);
  const evidence = [];
  const seen = new Set();
  for (const project of scanProjects) {
    for (const sourceFile of projectSourceFiles(project)) {
      const nodes = collectReferenceNodes(sourceFile);
      const symbols = project.checker.getSymbolAtLocation(nodes);
      for (let index = 0; index < nodes.length; index += 1) {
        const node = nodes[index];
        const nodeLocation = location(root, node);
        if (
          nodeLocation.path === declarationLocation.path &&
          nodeLocation.line === declarationLocation.line &&
          nodeLocation.col === declarationLocation.col
        ) {
          continue;
        }
        if (identity.namespace === "type" && !isTypePosition(node)) continue;
        if (identity.namespace === "value" && isTypePosition(node)) continue;
        const symbol = resolveAlias(project.checker, symbols[index]);
        const matches = declarationsForSymbol(project, symbol)
          .filter((candidate) => declarationNamespaces(candidate).has(identity.namespace))
          .some((candidate) => targetKeys.has(stableDeclarationKey(candidate, identity.namespace)));
        if (!matches) continue;
        const use = semanticUse(root, node, identity.namespace);
        const key = JSON.stringify(use);
        if (!seen.has(key)) {
          seen.add(key);
          evidence.push(use);
        }
      }
    }
  }
  return evidence;
};

const collectReferenceNodes = (
  sourceFile,
  candidateNames = null,
  includeDefaultImports = false,
) => {
  const nodes = [];
  visit(sourceFile, (node) => {
    if (
      isIdentifier(node) &&
      (!candidateNames ||
        candidateNames.has(node.text) ||
        (includeDefaultImports && isImportClause(node.parent) && node.parent.name === node))
    ) {
      nodes.push(node);
      return;
    }
    if (
      isStringLiteralLikeNode(node) &&
      isElementAccessExpression(node.parent) &&
      node.parent.argumentExpression === node &&
      (!candidateNames || candidateNames.has(node.text))
    ) {
      nodes.push(node);
    }
  });
  return nodes;
};

const batchSymbolUseEvidence = (root, resolvedQueries, scanProjects) => {
  const evidenceByQuery = new Map(resolvedQueries.map(({ query }) => [query.id, []]));
  const totalByQuery = new Map(resolvedQueries.map(({ query }) => [query.id, 0]));
  const declarationLocationsByQuery = new Map(
    resolvedQueries.map(({ query }) => [query.id, new Set()]),
  );
  const targetsByKey = new Map();
  const candidateNames = new Set();
  const includeDefaultImports = resolvedQueries.some(
    ({ query }) => query.symbol.exportedName === "default",
  );
  let sourceScanCount = 0;
  for (const entry of resolvedQueries) {
    candidateNames.add(entry.query.symbol.localName);
    candidateNames.add(entry.query.symbol.exportedName);
    const ownerProject = entry.resolved.project;
    const symbol = resolveAlias(
      ownerProject.checker,
      symbolForDeclaration(ownerProject, entry.resolved.declaration),
    );
    for (const declaration of declarationsForSymbol(ownerProject, symbol)) {
      if (!declarationNamespaces(declaration).has(entry.query.symbol.namespace)) continue;
      declarationLocationsByQuery
        .get(entry.query.id)
        .add(locationKey(location(root, declaration.name ?? declaration)));
      const key = stableDeclarationKey(declaration, entry.query.symbol.namespace);
      const targets = targetsByKey.get(key) ?? [];
      targets.push(entry);
      targetsByKey.set(key, targets);
    }
  }

  const scannedFiles = new Set();
  for (const project of scanProjects) {
    for (const sourceFile of projectSourceFiles(project)) {
      const fileIdentity = sourceFileIdentity(sourceFile);
      if (scannedFiles.has(fileIdentity)) continue;
      scannedFiles.add(fileIdentity);
      sourceScanCount += 1;
      const nodes = collectReferenceNodes(sourceFile, candidateNames, includeDefaultImports);
      const symbols = project.checker.getSymbolAtLocation(nodes);
      for (let index = 0; index < nodes.length; index += 1) {
        const node = nodes[index];
        const namespace = isTypePosition(node) ? "type" : "value";
        const symbol = resolveAlias(project.checker, symbols[index]);
        const matchingEntries = new Set();
        for (const declaration of declarationsForSymbol(project, symbol)) {
          if (!declarationNamespaces(declaration).has(namespace)) continue;
          const key = stableDeclarationKey(declaration, namespace);
          for (const entry of targetsByKey.get(key) ?? []) matchingEntries.add(entry);
        }
        for (const entry of matchingEntries) {
          if (
            declarationLocationsByQuery.get(entry.query.id).has(locationKey(location(root, node)))
          ) {
            continue;
          }
          if (
            node === entry.resolved.declaration.name ||
            node.parent === entry.resolved.declaration
          ) {
            continue;
          }
          const evidence = evidenceByQuery.get(entry.query.id);
          const use = semanticUse(root, node, namespace);
          totalByQuery.set(entry.query.id, totalByQuery.get(entry.query.id) + 1);
          if (evidence.length < MAX_SYMBOL_USE_EVIDENCE) {
            evidence.push(use);
          }
        }
      }
    }
  }
  return { evidenceByQuery, totalByQuery, sourceScanCount };
};

const selectProjectForSymbol = (snapshot, explicitProjects, absolutePath, allowDefaultFallback) =>
  explicitProjects.find((project) => project.program.getSourceFile(absolutePath)) ??
  (allowDefaultFallback ? snapshot.getDefaultProjectForFile(absolutePath) : undefined);

const resolveSymbolQuery = (
  root,
  snapshot,
  explicitProjects,
  statesByProject,
  query,
  allowDefaultFallback,
) => {
  if (!DECLARATION_KINDS.has(query.symbol.declarationKind)) {
    return {
      error: unavailable(
        query,
        "unsupported-syntax",
        "Use a supported declaration kind or retain the syntactic finding.",
      ),
    };
  }
  const project = selectProjectForSymbol(
    snapshot,
    explicitProjects,
    query.symbol.absolutePath,
    allowDefaultFallback,
  );
  if (!project) {
    return {
      error: unavailable(
        query,
        "no-project",
        "Pass a tsconfig containing the declaration with --type-aware-project.",
      ),
    };
  }
  const state = statesByProject.get(project);
  if (state?.status !== "complete") {
    return {
      error: unavailable(
        query,
        "blocking-diagnostics",
        `Repair structural diagnostics in ${state?.config ?? "the selected project"} and retry.`,
      ),
    };
  }
  const sourceFile = project.program.getSourceFile(query.symbol.absolutePath);
  const anchor = sourceFile ? findDeclaration(sourceFile, query.symbol) : undefined;
  const declaration =
    anchor && isExportSpecifier(anchor)
      ? declarationsForSymbol(
          project,
          resolveAlias(project.checker, project.checker.getSymbolAtLocation(anchor.name)),
        ).find((node) => declarationNamespaces(node).has(query.symbol.namespace))
      : anchor;
  const exportAliasMatches = (candidate) => {
    if (!candidate) return false;
    if (
      query.symbol.declarationKind !== "export" &&
      query.symbol.exportedName === query.symbol.localName
    ) {
      return true;
    }
    const candidateKey = stableDeclarationKey(candidate, query.symbol.namespace);
    return projectExportIndex(project).has(`${query.symbol.exportedName}\0${candidateKey}`);
  };
  if (!anchor || !declaration || !exportAliasMatches(declaration)) {
    return {
      error: unavailable(
        query,
        "unknown-symbol",
        "Refresh the syntactic result and retry with its exact declaration identity.",
      ),
    };
  }
  return { project, declaration, state };
};

const analyzeSymbolUse = (root, query, resolved, evidenceLimit, evidence, totalEvidenceCount) => {
  return boundedResult({
    query,
    assertion: totalEvidenceCount > 0 ? "confirmed-used" : "no-confirmed-use",
    evidence,
    evidenceLimit,
    totalEvidenceCount: evidence.length,
    data: {
      symbol: stableSymbolIdentity(
        root,
        resolved.declaration,
        query.symbol.namespace,
        query.symbol.exportedName,
      ),
      selected_project: resolved.state.config,
      total_reference_count: totalEvidenceCount,
    },
  });
};

const analyzeSymbolTrace = (root, query, resolved, evidenceLimit, scanProjects) => {
  const references = referenceEvidence(
    root,
    resolved.project,
    resolved.declaration,
    query.symbol,
    scanProjects,
  );
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
    assertion: references.length > 0 ? "references-found" : "no-references-found",
    evidence: [declaration, ...references],
    evidenceLimit,
    data: {
      symbol: stableSymbolIdentity(
        root,
        resolved.declaration,
        query.symbol.namespace,
        query.symbol.exportedName,
      ),
      selected_project: resolved.state.config,
      alias_hops: aliasHops.slice(0, evidenceLimit),
      total_alias_hop_count: aliasHops.length,
      checker_evidence_count: references.length,
      graph_evidence_count: aliasHops.length,
    },
    omissions: [
      { reason_code: "evidence-limit", count: Math.max(0, aliasHops.length - evidenceLimit) },
    ],
  });
};

const packageTargets = (value, targets) => {
  if (typeof value === "string") {
    targets.add(value);
    return;
  }
  if (Array.isArray(value)) {
    for (const entry of value) packageTargets(entry, targets);
    return;
  }
  if (value && typeof value === "object") {
    for (const entry of Object.values(value)) packageTargets(entry, targets);
  }
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

const projectPackage = (root, project) => {
  const normalizedRoot = path.resolve(root);
  let current = path.dirname(project.configFileName);
  while (current === normalizedRoot || current.startsWith(`${normalizedRoot}${path.sep}`)) {
    const packageFile = path.join(current, "package.json");
    if (existsSync(packageFile)) {
      try {
        return { root: current, json: JSON.parse(readFileSync(packageFile, "utf8")) };
      } catch {
        return { root: current, json: {} };
      }
    }
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

const discoverEntryPoints = (root, project, requested) => {
  const sourceFiles = new Map(
    projectSourceFiles(project).map((sourceFile) => [sourceFileIdentity(sourceFile), sourceFile]),
  );
  if (requested.length > 0) {
    return requested
      .map((entry) => sourceFiles.get(canonicalFileIdentity(entry.absolutePath)))
      .filter(Boolean);
  }
  const packageInfo = projectPackage(root, project);
  const targets = new Set();
  packageTargets(packageInfo.json.exports, targets);
  packageTargets(packageInfo.json.types, targets);
  packageTargets(packageInfo.json.typings, targets);
  packageTargets(packageInfo.json.module, targets);
  packageTargets(packageInfo.json.main, targets);
  if (targets.size === 0) targets.add("src/index.ts");
  const entries = [];
  for (const target of targets) {
    if (target.includes("*")) {
      const pattern = wildcardPattern(target);
      for (const sourceFile of sourceFiles.values()) {
        if (
          pattern.test(relativePath(packageInfo.root, sourceFile.fileName)) &&
          !entries.includes(sourceFile)
        ) {
          entries.push(sourceFile);
        }
      }
      continue;
    }
    for (const candidate of sourceCandidatesForTarget(packageInfo.root, target)) {
      const sourceFile = sourceFiles.get(canonicalFileIdentity(candidate));
      if (sourceFile && !entries.includes(sourceFile)) {
        entries.push(sourceFile);
        break;
      }
    }
  }
  const packageName = packageInfo.json.name;
  const pathTargets =
    typeof packageName === "string" ? (project.compilerOptions.paths?.[packageName] ?? []) : [];
  for (const target of pathTargets) {
    for (const candidateRoot of ancestorRoots(root, packageInfo.root)) {
      for (const candidate of sourceCandidatesForTarget(candidateRoot, target)) {
        const sourceFile = sourceFiles.get(canonicalFileIdentity(candidate));
        if (sourceFile && !entries.includes(sourceFile)) {
          entries.push(sourceFile);
        }
      }
    }
  }
  return entries;
};

const typeReferenceNodes = (declaration) => {
  const nodes = [];
  const isPrivateMember = (node) =>
    (node.name && isPrivateIdentifier(node.name)) ||
    node.modifiers?.some((modifier) => modifier.getText(node.getSourceFile()) === "private");
  const scan = (node) => {
    if (isFunctionBody(node)) return;
    if (node !== declaration && isDeclaration(node) && isPrivateMember(node)) return;
    if (isIdentifier(node) && isTypePosition(node)) nodes.push(node);
    node.forEachChild((child) => {
      scan(child);
      return undefined;
    });
  };
  scan(declaration);
  return nodes;
};

const publicApiGraph = (root, project, entryPoints, includeEntries = false) => {
  const exports = [];
  const publicKeys = new Set();
  for (const sourceFile of entryPoints) {
    const moduleSymbol = project.checker.getSymbolAtLocation(sourceFile);
    if (!moduleSymbol) continue;
    for (const exportedSymbol of project.checker.getExportsOfModule(moduleSymbol)) {
      const target = resolveAlias(project.checker, exportedSymbol);
      for (const declaration of declarationsForSymbol(project, target)) {
        if (!isProjectSource(project, declaration.getSourceFile())) continue;
        const namespace = declarationNamespaces(declaration).has("type") ? "type" : "value";
        const key = stableDeclarationKey(declaration, namespace);
        publicKeys.add(key);
        exports.push({ exportedName: exportedSymbol.name, declaration, namespace, key });
      }
    }
  }

  const edges = [];
  const leaks = [];
  const seenEdges = new Set();
  for (const exported of exports) {
    const nodes = typeReferenceNodes(exported.declaration);
    const symbols = project.checker.getSymbolAtLocation(nodes);
    for (let index = 0; index < nodes.length; index += 1) {
      const target = resolveAlias(project.checker, symbols[index]);
      for (const declaration of declarationsForSymbol(project, target)) {
        if (!isProjectSource(project, declaration.getSourceFile())) continue;
        const targetKey = stableDeclarationKey(declaration, "type");
        if (targetKey === exported.key) continue;
        const edgeKey = `${exported.key}\0${targetKey}`;
        if (seenEdges.has(edgeKey)) continue;
        seenEdges.add(edgeKey);
        const evidence = location(root, nodes[index]);
        const edge = {
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
        };
        edges.push(edge);
        if (!publicKeys.has(targetKey)) {
          leaks.push({
            exposed_symbol: edge.source,
            private_declaration: edge.target,
            relation: "public-signature-private-type",
            evidence,
          });
        }
      }
    }
  }
  const signatureFingerprint = (entry) => {
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
  const entries = includeEntries
    ? exports.map((entry) => {
        const exposed = stableSymbolIdentity(
          root,
          entry.declaration,
          entry.namespace,
          entry.exportedName,
        );
        const referencedTypes = edges
          .filter(
            (edge) =>
              edge.source.path === exposed.path &&
              edge.source.namespace === exposed.namespace &&
              edge.source.local_name === exposed.local_name &&
              edge.source.line === exposed.line &&
              edge.source.col === exposed.col,
          )
          .map((edge) => ({ declaration: edge.target, relation: edge.relation }))
          .toSorted((left, right) => compareText(JSON.stringify(left), JSON.stringify(right)));
        return {
          exposed,
          origin: exposed,
          signature_fingerprint: signatureFingerprint(entry),
          referenced_types: referencedTypes,
        };
      })
    : [];
  return { exports, entries, edges, leaks };
};

const graphProjects = (snapshot, explicitProjects) =>
  (explicitProjects.length > 0 ? explicitProjects : snapshot.getProjects()).filter(
    (project) => project.program.getSourceFileNames().length > 0,
  );

const analyzeApiSurface = (root, query, states, evidenceLimit) => {
  const ready = states.filter((state) => state.status === "complete");
  if (ready.length === 0) {
    return unavailable(
      query,
      states.length === 0 ? "no-project" : "blocking-diagnostics",
      "Pass a healthy tsconfig containing the package entry points and retry.",
    );
  }
  const exports = [];
  const entries = [];
  const leaks = [];
  const edges = [];
  let missingEntryPointCount = 0;
  for (const state of ready) {
    const entryPoints = discoverEntryPoints(root, state.project, query.entryPoints);
    if (query.entryPoints.length > 0 && entryPoints.length === 0) {
      missingEntryPointCount += query.entryPoints.length;
    }
    const graph = publicApiGraph(root, state.project, entryPoints, true);
    exports.push(
      ...graph.exports.map((entry) =>
        stableSymbolIdentity(root, entry.declaration, entry.namespace, entry.exportedName),
      ),
    );
    entries.push(...graph.entries);
    leaks.push(...graph.leaks);
    edges.push(...graph.edges);
  }
  const evidence = leaks.length > 0 ? leaks : edges;
  const orderedExports = exports.toSorted((left, right) =>
    compareText(JSON.stringify(left), JSON.stringify(right)),
  );
  const orderedEntries = entries.toSorted((left, right) =>
    compareText(JSON.stringify(left.exposed), JSON.stringify(right.exposed)),
  );
  const orderedLeaks = leaks.toSorted((left, right) =>
    compareText(JSON.stringify(left), JSON.stringify(right)),
  );
  const orderedEdges = edges.toSorted((left, right) =>
    compareText(JSON.stringify(left), JSON.stringify(right)),
  );
  const unavailableProjectCount = states.length - ready.length;
  let nestedReferenceOmissionCount = 0;
  const boundedEntries = orderedEntries.slice(0, evidenceLimit).map((entry) => {
    nestedReferenceOmissionCount += Math.max(0, entry.referenced_types.length - evidenceLimit);
    return {
      ...entry,
      referenced_types: entry.referenced_types.slice(0, evidenceLimit),
      total_referenced_type_count: entry.referenced_types.length,
    };
  });
  return boundedResult({
    query,
    assertion: leaks.length > 0 ? "leak-confirmed" : "no-leak-confirmed",
    evidence,
    evidenceLimit,
    data: {
      exports: orderedExports.slice(0, evidenceLimit),
      total_export_count: orderedExports.length,
      entries: boundedEntries,
      total_entry_count: orderedEntries.length,
      leaks: orderedLeaks.slice(0, evidenceLimit),
      total_leak_count: orderedLeaks.length,
      public_signature_edges: orderedEdges.slice(0, evidenceLimit),
      total_public_signature_edge_count: orderedEdges.length,
    },
    omissions: [
      {
        reason_code: "evidence-limit",
        count:
          Math.max(0, orderedExports.length - evidenceLimit) +
          Math.max(0, orderedEntries.length - evidenceLimit) +
          Math.max(0, orderedLeaks.length - evidenceLimit) +
          Math.max(0, orderedEdges.length - evidenceLimit) +
          nestedReferenceOmissionCount,
      },
      { reason_code: "blocking-diagnostics", count: unavailableProjectCount },
      { reason_code: "unknown-entry-point", count: missingEntryPointCount },
    ],
  });
};

const moduleSpecifier = (node) => {
  if ((isImportDeclaration(node) || isExportDeclaration(node)) && node.moduleSpecifier) {
    return isStringLiteralLikeNode(node.moduleSpecifier) ? node.moduleSpecifier.text : null;
  }
  return undefined;
};

const resolveLocalModule = (sourceFile, specifier, files) => {
  if (!specifier?.startsWith(".")) return undefined;
  const base = path.resolve(path.dirname(sourceFile.fileName), specifier);
  const candidates = [
    base,
    `${base}.ts`,
    `${base}.tsx`,
    `${base}.js`,
    `${base}.jsx`,
    path.join(base, "index.ts"),
    path.join(base, "index.tsx"),
  ];
  return candidates.map(canonicalFileIdentity).find((candidate) => files.has(candidate));
};

const moduleGraph = (project) => {
  const sources = projectSourceFiles(project);
  const files = new Set(sources.map(sourceFileIdentity));
  const reverse = new Map();
  let hasDynamicBehavior = false;
  for (const sourceFile of sources) {
    const consumer = sourceFileIdentity(sourceFile);
    visit(sourceFile, (node) => {
      const specifier = moduleSpecifier(node);
      if (specifier !== undefined) {
        const provider = resolveLocalModule(sourceFile, specifier, files);
        if (provider) {
          const consumers = reverse.get(provider) ?? new Set();
          consumers.add(consumer);
          reverse.set(provider, consumers);
        }
      }
      if (isCallExpression(node) && node.expression?.getText(sourceFile) === "import") {
        const argument = node.arguments?.[0];
        if (!argument || !isStringLiteralLikeNode(argument)) hasDynamicBehavior = true;
      }
    });
  }
  return { reverse, hasDynamicBehavior };
};

const combinedModuleGraph = (projects) => {
  const reverse = new Map();
  let hasDynamicBehavior = false;
  for (const project of projects) {
    const graph = moduleGraph(project);
    hasDynamicBehavior ||= graph.hasDynamicBehavior;
    for (const [provider, consumers] of graph.reverse) {
      const combinedConsumers = reverse.get(provider) ?? new Set();
      for (const consumer of consumers) combinedConsumers.add(consumer);
      reverse.set(provider, combinedConsumers);
    }
  }
  return { reverse, hasDynamicBehavior };
};

const isTestFile = (fileName) =>
  TEST_FILE_PATTERN.test(slash(fileName)) || slash(fileName).includes("/__tests__/");

const impactClosure = (root, graph, directFiles, evidenceLimit) => {
  const queue = [...directFiles].map((file) => ({ file, path: [file] }));
  const visited = new Set(directFiles);
  const affected = [];
  const tests = [];
  let omittedProvenanceCount = 0;
  while (queue.length > 0) {
    const current = queue.shift();
    for (const consumer of graph.reverse.get(current.file) ?? []) {
      if (visited.has(consumer)) continue;
      visited.add(consumer);
      const provenance = [consumer, ...current.path];
      queue.push({ file: consumer, path: provenance });
      const relativeProvenance = provenance.map((file) => relativePath(root, file));
      omittedProvenanceCount += Math.max(0, relativeProvenance.length - evidenceLimit);
      const entry = {
        path: relativePath(root, consumer),
        provenance: relativeProvenance.slice(0, evidenceLimit),
      };
      affected.push(entry);
      if (isTestFile(consumer)) tests.push(entry);
    }
  }
  return {
    affected: affected.toSorted((left, right) => compareText(left.path, right.path)),
    tests: tests.toSorted((left, right) => compareText(left.path, right.path)),
    omittedProvenanceCount,
  };
};

const analyzeSymbolImpact = (root, query, resolved, evidenceLimit, scanProjects) => {
  const references = referenceEvidence(
    root,
    resolved.project,
    resolved.declaration,
    query.symbol,
    scanProjects,
  );
  const declarationFile = sourceFileIdentity(resolved.declaration.getSourceFile());
  const directFiles = new Set(
    references
      .map((entry) => canonicalFileIdentity(path.resolve(root, entry.path)))
      .filter((file) => file !== declarationFile),
  );
  const graph = combinedModuleGraph(scanProjects);
  const closure = impactClosure(root, graph, directFiles, evidenceLimit);
  const directTests = [...directFiles]
    .filter(isTestFile)
    .map((file) => {
      const relative = relativePath(root, file);
      return { path: relative, provenance: [relative] };
    })
    .toSorted((left, right) => compareText(left.path, right.path));
  const targetedTests = [...directTests, ...closure.tests].toSorted((left, right) =>
    compareText(left.path, right.path),
  );
  const directConsumers = [...directFiles]
    .map((file) => ({
      path: relativePath(root, file),
      namespace: query.symbol.namespace,
    }))
    .toSorted((left, right) => compareText(left.path, right.path));
  const evidence = [
    ...directConsumers.map((consumer) => ({ ...consumer, role: "direct-consumer" })),
    ...closure.affected.map((consumer) => ({ ...consumer, role: "transitive-consumer" })),
    ...targetedTests.map((test) => ({ ...test, role: "targeted-test" })),
  ];
  return boundedResult({
    query,
    assertion: directConsumers.length > 0 ? "consumers-found" : "no-consumers-found",
    evidence,
    evidenceLimit,
    omissions: [
      ...(graph.hasDynamicBehavior ? [{ reason_code: "dynamic-behavior", count: 1 }] : []),
      {
        reason_code: "evidence-limit",
        count:
          Math.max(0, directConsumers.length - evidenceLimit) +
          Math.max(0, closure.affected.length - evidenceLimit) +
          Math.max(0, targetedTests.length - evidenceLimit) +
          closure.omittedProvenanceCount,
      },
    ],
    data: {
      symbol: stableSymbolIdentity(
        root,
        resolved.declaration,
        query.symbol.namespace,
        query.symbol.exportedName,
      ),
      selected_project: resolved.state.config,
      direct_consumers: directConsumers.slice(0, evidenceLimit),
      total_direct_consumer_count: directConsumers.length,
      transitive_affected_files: closure.affected.slice(0, evidenceLimit),
      total_transitive_affected_file_count: closure.affected.length,
      targeted_tests: targetedTests.slice(0, evidenceLimit),
      total_targeted_test_count: targetedTests.length,
      confidence: graph.hasDynamicBehavior ? "bounded" : "high",
    },
  });
};

const findCycles = (edges) => {
  const adjacency = new Map();
  for (const edge of edges) {
    const source = edge.source.path;
    const target = edge.target.path;
    const targets = adjacency.get(source) ?? new Set();
    targets.add(target);
    adjacency.set(source, targets);
  }
  const cycles = [];
  for (const [source, targets] of adjacency) {
    for (const target of targets) {
      if (adjacency.get(target)?.has(source) && compareText(source, target) < 0) {
        cycles.push([source, target, source]);
      }
    }
  }
  return cycles.toSorted((left, right) => compareText(left.join("\0"), right.join("\0")));
};

const percentile = (values, ratio) => {
  if (values.length === 0) return 0;
  const ordered = [...values].toSorted((left, right) => left - right);
  return ordered[Math.min(ordered.length - 1, Math.floor((ordered.length - 1) * ratio))];
};

const analyzeTypeCoupling = (root, query, states, evidenceLimit) => {
  const ready = states.filter((state) => state.status === "complete");
  if (ready.length === 0) {
    return unavailable(
      query,
      states.length === 0 ? "no-project" : "blocking-diagnostics",
      "Pass a healthy tsconfig containing the package entry points and retry.",
    );
  }
  const edges = [];
  const projectFiles = new Set();
  let missingEntryPointCount = 0;
  for (const state of ready) {
    for (const sourceFile of projectSourceFiles(state.project)) {
      projectFiles.add(sourceFileIdentity(sourceFile));
    }
    const entries = discoverEntryPoints(root, state.project, query.entryPoints);
    if (query.entryPoints.length > 0 && entries.length === 0) {
      missingEntryPointCount += query.entryPoints.length;
    }
    edges.push(
      ...publicApiGraph(root, state.project, entries).edges.filter(
        (edge) => edge.source.path !== edge.target.path,
      ),
    );
  }
  edges.sort((left, right) => compareText(JSON.stringify(left), JSON.stringify(right)));
  const seenEdgeKeys = new Set();
  const uniqueEdges = edges.filter((edge) => {
    const key = JSON.stringify([edge.source, edge.target]);
    if (seenEdgeKeys.has(key)) return false;
    seenEdgeKeys.add(key);
    return true;
  });
  edges.splice(0, edges.length, ...uniqueEdges);
  const outgoing = new Map();
  const incoming = new Map();
  for (const edge of edges) {
    const source = edge.source.path;
    const target = edge.target.path;
    outgoing.set(source, (outgoing.get(source) ?? new Set()).add(target));
    incoming.set(target, (incoming.get(target) ?? new Set()).add(source));
  }
  const files = new Set([...outgoing.keys(), ...incoming.keys()]);
  const perFile = [...files]
    .map((file) => ({
      path: file,
      outgoing_label: "public API depends on",
      outgoing_files: [...(outgoing.get(file) ?? [])].toSorted(compareText),
      incoming_label: "public types used by",
      incoming_files: [...(incoming.get(file) ?? [])].toSorted(compareText),
    }))
    .toSorted((left, right) => compareText(left.path, right.path));
  const degrees = perFile.map((entry) => entry.outgoing_files.length + entry.incoming_files.length);
  const outgoingDegrees = perFile.map((entry) => entry.outgoing_files.length);
  const incomingDegrees = perFile.map((entry) => entry.incoming_files.length);
  const highCouplingThreshold = percentile(degrees, 0.9);
  const topContributors = [...perFile]
    .toSorted(
      (left, right) =>
        right.outgoing_files.length +
          right.incoming_files.length -
          (left.outgoing_files.length + left.incoming_files.length) ||
        compareText(left.path, right.path),
    )
    .slice(0, 10);
  const boundCoupledFile = (entry) => ({
    ...entry,
    outgoing_files: entry.outgoing_files.slice(0, evidenceLimit),
    total_outgoing_file_count: entry.outgoing_files.length,
    incoming_files: entry.incoming_files.slice(0, evidenceLimit),
    total_incoming_file_count: entry.incoming_files.length,
  });
  const nestedFileOmissionCount = perFile.reduce(
    (count, entry) =>
      count +
      Math.max(0, entry.outgoing_files.length - evidenceLimit) +
      Math.max(0, entry.incoming_files.length - evidenceLimit),
    0,
  );
  const evidence = edges.map((edge) => ({
    source: edge.source.path,
    target: edge.target.path,
    relation: edge.relation,
    evidence: edge.evidence,
  }));
  const cycles = query.includeCycles ? findCycles(edges) : [];
  const unavailableProjectCount = states.length - ready.length;
  return boundedResult({
    query,
    assertion: edges.length > 0 ? "coupling-found" : "no-coupling-found",
    evidence,
    evidenceLimit,
    data: {
      scope: "project-local-public-signatures",
      direction: "directed",
      project_size: projectFiles.size,
      files_analyzed: projectFiles.size,
      distinct_coupled_files: files.size,
      edge_count: edges.length,
      coupled_file_percentage:
        projectFiles.size === 0 ? null : (files.size / projectFiles.size) * 100,
      p50_distinct_connections: percentile(degrees, 0.5),
      p90_distinct_connections: percentile(degrees, 0.9),
      p95_public_api_depends_on: percentile(outgoingDegrees, 0.95),
      p95_public_types_used_by: percentile(incomingDegrees, 0.95),
      high_coupling_percentage:
        projectFiles.size === 0
          ? null
          : (degrees.filter((degree) => degree > highCouplingThreshold).length /
              projectFiles.size) *
            100,
      concentration:
        edges.length === 0
          ? 0
          : topContributors.reduce(
              (sum, entry) => sum + entry.outgoing_files.length + entry.incoming_files.length,
              0,
            ) /
            (edges.length * 2),
      files: perFile.slice(0, evidenceLimit).map(boundCoupledFile),
      total_file_count: perFile.length,
      top_contributors: topContributors.map(boundCoupledFile),
      edges: edges.slice(0, evidenceLimit),
      cycles: cycles.slice(0, evidenceLimit),
      total_cycle_count: cycles.length,
    },
    omissions: [
      {
        reason_code: "evidence-limit",
        count:
          Math.max(0, perFile.length - evidenceLimit) +
          Math.max(0, cycles.length - evidenceLimit) +
          nestedFileOmissionCount,
      },
      { reason_code: "blocking-diagnostics", count: unavailableProjectCount },
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

export const analyzeSemanticQueries = ({ root, projects, queries, evidenceLimit }) => {
  if (queries.length === 0) {
    return {
      selectedTsconfigs: [],
      projectResults: [],
      results: [],
      phaseTimings: { project_setup: 0, diagnostics: 0, semantic_queries: 0 },
      warnings: [],
    };
  }
  const setupStartedAt = performance.now();
  const openFiles = queries
    .filter((query) => query.symbol)
    .map((query) => query.symbol.absolutePath);
  const api = new API({ cwd: root });
  try {
    const conventionalProject = path.join(root, "tsconfig.json");
    const openProjects =
      projects.length === 0 && existsSync(conventionalProject)
        ? [conventionalProject]
        : projects.map((project) => project.absolutePath);
    const snapshot = api.updateSnapshot({
      openFiles: [...new Set(openFiles)],
      openProjects,
    });
    const explicitProjects = openProjects
      .map((project) => snapshot.getProject(project))
      .filter(Boolean);
    const allowDefaultFallback = projects.length === 0;
    const symbolProjects = openFiles
      .map((file) => selectProjectForSymbol(snapshot, explicitProjects, file, allowDefaultFallback))
      .filter(Boolean);
    const selectedProjects = [
      ...new Set([...graphProjects(snapshot, explicitProjects), ...symbolProjects]),
    ];
    if (selectedProjects.length === 0) return emptySemanticAnalysis(queries);
    const projectSetupMs = performance.now() - setupStartedAt;
    const diagnosticsStartedAt = performance.now();
    const states = selectedProjects.map((project) =>
      projectState(root, project, projects.length > 0 ? "explicit" : "auto"),
    );
    const diagnosticsMs = performance.now() - diagnosticsStartedAt;
    const statesByProject = new Map(states.map((state) => [state.project, state]));
    const explicitProjectSet = new Set(explicitProjects);
    const graphStates =
      explicitProjectSet.size === 0
        ? states
        : states.filter((state) => explicitProjectSet.has(state.project));
    const queryStartedAt = performance.now();
    const resolvedByQuery = new Map();
    for (const query of queries) {
      if (!query.symbol) continue;
      try {
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
      } catch {
        resolvedByQuery.set(query.id, {
          error: unavailable(
            query,
            "unsupported-syntax",
            "Retain the syntactic finding and narrow the query to supported syntax.",
          ),
        });
      }
    }
    const resolvedSymbolUses = queries
      .filter((query) => query.operation === "symbol-use")
      .flatMap((query) => {
        const resolved = resolvedByQuery.get(query.id);
        return resolved?.error ? [] : [{ query, resolved }];
      });
    const semanticProjects = states
      .filter((state) => state.status === "complete")
      .map((state) => state.project);
    let symbolUseBatch;
    try {
      symbolUseBatch = batchSymbolUseEvidence(root, resolvedSymbolUses, semanticProjects);
    } catch {
      symbolUseBatch = {
        evidenceByQuery: new Map(),
        totalByQuery: new Map(),
        sourceScanCount: 0,
        failed: true,
      };
    }
    const results = queries.map((query) => {
      try {
        if (query.operation === "api-surface") {
          return analyzeApiSurface(root, query, graphStates, evidenceLimit);
        }
        if (query.operation === "type-coupling") {
          return analyzeTypeCoupling(root, query, graphStates, evidenceLimit);
        }
        const resolved = resolvedByQuery.get(query.id);
        if (resolved.error) return resolved.error;
        if (query.operation === "symbol-use") {
          if (symbolUseBatch.failed) {
            return unavailable(
              query,
              "unsupported-syntax",
              "Retain the syntactic finding and narrow the query to supported syntax.",
            );
          }
          return analyzeSymbolUse(
            root,
            query,
            resolved,
            evidenceLimit,
            symbolUseBatch.evidenceByQuery.get(query.id) ?? [],
            symbolUseBatch.totalByQuery.get(query.id) ?? 0,
          );
        }
        if (query.operation === "symbol-trace") {
          return analyzeSymbolTrace(root, query, resolved, evidenceLimit, semanticProjects);
        }
        return analyzeSymbolImpact(root, query, resolved, evidenceLimit, semanticProjects);
      } catch {
        return unavailable(
          query,
          "unsupported-syntax",
          "Retain the syntactic finding and narrow the query to supported syntax.",
        );
      }
    });
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
    };
  } finally {
    api.close();
  }
};
