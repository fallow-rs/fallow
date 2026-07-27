import path from "node:path";

import {
  isCallExpression,
  isExportDeclaration,
  isImportDeclaration,
  isStringLiteralLikeNode,
} from "typescript/unstable/ast/is";

import { canonicalFileIdentity } from "./file-identity.mjs";
import {
  projectSourceFiles,
  relativePath,
  semanticQueryIdentity,
  sourceFileIdentity,
} from "./semantic-identity.mjs";

const TEST_FILE_PATTERN = /(?:^|[/_.-])(?:test|spec)\.[cm]?[jt]sx?$/u;
const compareText = (left, right) => Buffer.compare(Buffer.from(left), Buffer.from(right));
const slash = (value) => value.split(path.sep).join("/");

const visit = (node, callback) => {
  callback(node);
  node.forEachChild((child) => {
    visit(child, callback);
    return undefined;
  });
};

const isModuleEdgeDeclaration = (node) => isImportDeclaration(node) || isExportDeclaration(node);

const moduleSpecifier = (node) => {
  if (!isModuleEdgeDeclaration(node)) return undefined;
  if (!node.moduleSpecifier) return undefined;
  return isStringLiteralLikeNode(node.moduleSpecifier) ? node.moduleSpecifier.text : null;
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

const addReverseModuleEdge = (reverse, provider, consumer) => {
  const consumers = reverse.get(provider) ?? new Set();
  consumers.add(consumer);
  reverse.set(provider, consumers);
};

const recordModuleEdge = (reverse, sourceFile, consumer, files, specifier) => {
  if (specifier === undefined) return;
  const provider = resolveLocalModule(sourceFile, specifier, files);
  if (provider) addReverseModuleEdge(reverse, provider, consumer);
};

const isDynamicImport = (node, sourceFile) =>
  isCallExpression(node) && node.expression?.getText(sourceFile) === "import";

const hasUnresolvedDynamicImport = (node, sourceFile) => {
  if (!isDynamicImport(node, sourceFile)) return false;
  const argument = node.arguments?.[0];
  return !argument || !isStringLiteralLikeNode(argument);
};

const scanModuleSource = (sourceFile, files, reverse) => {
  const consumer = sourceFileIdentity(sourceFile);
  let hasDynamicBehavior = false;
  visit(sourceFile, (node) => {
    recordModuleEdge(reverse, sourceFile, consumer, files, moduleSpecifier(node));
    hasDynamicBehavior ||= hasUnresolvedDynamicImport(node, sourceFile);
  });
  return hasDynamicBehavior;
};

const moduleGraph = (project) => {
  const sources = projectSourceFiles(project);
  const files = new Set(sources.map(sourceFileIdentity));
  const reverse = new Map();
  let hasDynamicBehavior = false;
  for (const sourceFile of sources) {
    const sourceHasDynamicBehavior = scanModuleSource(sourceFile, files, reverse);
    hasDynamicBehavior ||= sourceHasDynamicBehavior;
  }
  return { reverse, hasDynamicBehavior };
};

const mergeReverseGraph = (target, source) => {
  for (const [provider, consumers] of source) {
    const combinedConsumers = target.get(provider) ?? new Set();
    consumers.forEach((consumer) => combinedConsumers.add(consumer));
    target.set(provider, combinedConsumers);
  }
};

const combinedModuleGraph = (projects) => {
  const reverse = new Map();
  let hasDynamicBehavior = false;
  for (const project of projects) {
    const graph = moduleGraph(project);
    hasDynamicBehavior ||= graph.hasDynamicBehavior;
    mergeReverseGraph(reverse, graph.reverse);
  }
  return { reverse, hasDynamicBehavior };
};

const isTestFile = (fileName) =>
  TEST_FILE_PATTERN.test(slash(fileName)) || slash(fileName).includes("/__tests__/");

const affectedConsumer = (root, consumer, current, evidenceLimit) => {
  const provenance = [consumer, ...current.path];
  const relativeProvenance = provenance.map((file) => relativePath(root, file));
  return {
    queueEntry: { file: consumer, path: provenance },
    result: {
      path: relativePath(root, consumer),
      provenance: relativeProvenance.slice(0, evidenceLimit),
    },
    omitted: Math.max(0, relativeProvenance.length - evidenceLimit),
  };
};

const unvisitedConsumers = (graph, file, visited) =>
  [...(graph.reverse.get(file) ?? [])].filter((consumer) => !visited.has(consumer));

const impactClosure = (root, graph, directFiles, evidenceLimit) => {
  const queue = [...directFiles].map((file) => ({ file, path: [file] }));
  const visited = new Set(directFiles);
  const affected = [];
  const tests = [];
  let omittedProvenanceCount = 0;
  while (queue.length > 0) {
    const current = queue.shift();
    for (const consumer of unvisitedConsumers(graph, current.file, visited)) {
      visited.add(consumer);
      const entry = affectedConsumer(root, consumer, current, evidenceLimit);
      queue.push(entry.queueEntry);
      omittedProvenanceCount += entry.omitted;
      affected.push(entry.result);
      if (isTestFile(consumer)) tests.push(entry.result);
    }
  }
  return {
    affected: affected.toSorted((left, right) => compareText(left.path, right.path)),
    tests: tests.toSorted((left, right) => compareText(left.path, right.path)),
    omittedProvenanceCount,
  };
};

export const analyzeSymbolImpact = (
  { root, query, resolved, evidenceLimit, scanProjects, directReferenceFiles },
  { boundedResult },
) => {
  const declarationFile = sourceFileIdentity(resolved.declaration.getSourceFile());
  const directFiles = new Set([...directReferenceFiles].filter((file) => file !== declarationFile));
  const graph = combinedModuleGraph(scanProjects);
  const virtualDispatchCount = resolved.contractRelations.length;
  const hasBoundedDispatch = virtualDispatchCount > 0;
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
      ...(hasBoundedDispatch
        ? [{ reason_code: "virtual-dispatch", count: virtualDispatchCount }]
        : []),
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
      symbol: semanticQueryIdentity(root, query, resolved),
      selected_project: resolved.state.config,
      direct_consumers: directConsumers.slice(0, evidenceLimit),
      total_direct_consumer_count: directConsumers.length,
      transitive_affected_files: closure.affected.slice(0, evidenceLimit),
      total_transitive_affected_file_count: closure.affected.length,
      targeted_tests: targetedTests.slice(0, evidenceLimit),
      total_targeted_test_count: targetedTests.length,
      confidence: graph.hasDynamicBehavior || hasBoundedDispatch ? "bounded" : "high",
    },
  });
};
