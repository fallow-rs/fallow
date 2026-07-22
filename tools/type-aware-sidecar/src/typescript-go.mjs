import path from "node:path";

import { API } from "typescript/unstable/sync";
import {
  isClassDeclaration,
  isClassExpression,
  isGetAccessorDeclaration,
  isMethodDeclaration,
  isPrivateIdentifier,
  isPropertyAccessExpression,
  isPropertyDeclaration,
  isSetAccessorDeclaration,
} from "typescript/unstable/ast/is";

const MAX_WARNINGS = 20;
const INFERRED_PROJECT = "<inferred>";

const normalizeFileName = (fileName) => {
  const normalized = path.normalize(path.resolve(fileName));
  return process.platform === "win32" || process.platform === "darwin"
    ? normalized.toLowerCase()
    : normalized;
};

const relativeConfigPath = (root, configFileName) => {
  const normalized = configFileName.split(path.sep).join("/");
  if (normalized.endsWith("/dev/null/inferred")) {
    return INFERRED_PROJECT;
  }
  const relative = path.relative(root, configFileName);
  return (relative || path.basename(configFileName)).split(path.sep).join("/");
};

const isClassNode = (node) => [isClassDeclaration, isClassExpression].some((check) => check(node));

const findClass = (node) => {
  let current = node.parent;
  while (current) {
    if (isClassNode(current)) {
      return current;
    }
    current = current.parent;
  }
  return undefined;
};

const isMethodLikeDeclaration = (node) =>
  [isMethodDeclaration, isGetAccessorDeclaration, isSetAccessorDeclaration].some((check) =>
    check(node),
  );

const declarationKind = (node) => {
  if (isMethodLikeDeclaration(node)) {
    return "class_method";
  }
  if (isPropertyDeclaration(node)) {
    return "class_property";
  }
  return undefined;
};

const declarationNameMatches = (candidate, declaration) => {
  const declarationName = declaration.name;
  return (
    Boolean(declarationName) &&
    !isPrivateIdentifier(declarationName) &&
    declarationName.getText(declaration.getSourceFile()) === candidate.memberName
  );
};

const declarationOwnerMatches = (candidate, declaration) => {
  const owner = findClass(declaration);
  return Boolean(owner?.name) && owner.name.text === candidate.parentName;
};

const declarationPathMatches = (candidate, declaration) =>
  normalizeFileName(declaration.getSourceFile().fileName) ===
  normalizeFileName(candidate.absolutePath);

const declarationPosition = (declaration) => {
  const sourceFile = declaration.getSourceFile();
  const start = declaration.getStart(sourceFile);
  const { line, character } = sourceFile.getLineAndCharacterOfPosition(start);
  const lineStart = sourceFile.getPositionOfLineAndCharacter(line, 0);

  // TypeScript-Go positions and JavaScript string indices use UTF-16 code units.
  // Protocol v2 carries Oxc coordinates: 1-based lines and 0-based UTF-8 byte columns.
  const col = Buffer.byteLength(sourceFile.text.slice(lineStart, lineStart + character), "utf8");
  return { line: line + 1, col };
};

const declarationPositionMatches = (candidate, declaration) => {
  const position = declarationPosition(declaration);
  return position.line === candidate.line && position.col === candidate.col;
};

const matchesDeclaration = (candidate, declaration) => {
  const checks = [
    declarationPositionMatches(candidate, declaration),
    declarationKind(declaration) === candidate.kind,
    declarationNameMatches(candidate, declaration),
    declarationOwnerMatches(candidate, declaration),
    declarationPathMatches(candidate, declaration),
  ];
  return checks.every(Boolean);
};

const collectPropertyNames = (sourceFile, candidateNames) => {
  const nodes = [];
  const visit = (node) => {
    if (
      isPropertyAccessExpression(node) &&
      !isPrivateIdentifier(node.name) &&
      candidateNames.has(node.name.text)
    ) {
      nodes.push(node.name);
    }
    node.forEachChild(visit);
    return undefined;
  };
  sourceFile.forEachChild(visit);
  return nodes;
};

const countBlockingDiagnostics = (project) => {
  const configFile = project.program.getConfigFileParsingDiagnostics().length;
  const program = project.program.getProgramDiagnostics().length;
  const syntactic = project.program.getSyntacticDiagnostics().length;
  const bind = project.program.getBindDiagnostics().length;
  return configFile + program + syntactic + bind;
};

const addWarning = (warnings, warning) => {
  if (warnings.length < MAX_WARNINGS && !warnings.includes(warning)) {
    warnings.push(warning);
  }
};

const groupCandidatesByMember = (candidates) => {
  const candidatesByMember = new Map();
  for (const candidate of candidates) {
    const matching = candidatesByMember.get(candidate.memberName) ?? [];
    matching.push(candidate);
    candidatesByMember.set(candidate.memberName, matching);
  }
  return candidatesByMember;
};

const shouldSkipSourceFile = (project, sourceFile) =>
  !sourceFile ||
  sourceFile.isDeclarationFile ||
  project.program.isSourceFileDefaultLibrary(sourceFile) ||
  project.program.isSourceFileFromExternalLibrary(sourceFile);

const matchingCandidateIds = (declarationHandle, project, possibleCandidates) => {
  const declaration = declarationHandle.resolve(project);
  if (!declaration) {
    return [];
  }
  return possibleCandidates
    .filter((candidate) => matchesDeclaration(candidate, declaration))
    .map((candidate) => candidate.id);
};

const symbolDeclarations = (symbol) => (symbol ? symbol.declarations : []);

const confirmSymbolUses = ({ symbol, memberName, candidatesByMember, project, confirmedIds }) => {
  const possibleCandidates = candidatesByMember.get(memberName) ?? [];
  for (const declarationHandle of symbolDeclarations(symbol)) {
    for (const candidateId of matchingCandidateIds(
      declarationHandle,
      project,
      possibleCandidates,
    )) {
      confirmedIds.add(candidateId);
    }
  }
};

const scanSourceFile = ({
  sourceFile,
  candidateNames,
  candidatesByMember,
  project,
  confirmedIds,
}) => {
  const propertyNames = collectPropertyNames(sourceFile, candidateNames);
  if (propertyNames.length === 0) {
    return;
  }
  const symbols = project.checker.getSymbolAtLocation(propertyNames);
  for (let index = 0; index < symbols.length; index += 1) {
    confirmSymbolUses({
      symbol: symbols[index],
      memberName: propertyNames[index].text,
      candidatesByMember,
      project,
      confirmedIds,
    });
  }
};

const scanProject = ({ project, candidates, confirmedIds }) => {
  const candidatesByMember = groupCandidatesByMember(candidates);
  const candidateNames = new Set(candidatesByMember.keys());

  for (const fileName of project.program.getSourceFileNames()) {
    const sourceFile = project.program.getSourceFile(fileName);
    if (shouldSkipSourceFile(project, sourceFile)) {
      continue;
    }
    scanSourceFile({ sourceFile, candidateNames, candidatesByMember, project, confirmedIds });
  }
};

const selectCandidateProjects = (snapshot, candidates, explicitProjects, warnings) => {
  const candidatesByProject = new Map();
  const abstentions = [];
  for (const candidate of candidates) {
    const matchingExplicitProjects = explicitProjects.filter((project) =>
      project.program.getSourceFile(candidate.absolutePath),
    );
    const project =
      explicitProjects.length === 0
        ? snapshot.getDefaultProjectForFile(candidate.absolutePath)
        : matchingExplicitProjects.length === 1
          ? matchingExplicitProjects[0]
          : undefined;
    if (!project) {
      const ambiguous = matchingExplicitProjects.length > 1;
      const label = ambiguous ? "Multiple TypeScript projects" : "No TypeScript project";
      const reason = ambiguous ? "ambiguous-project" : "no-project";
      abstentions.push({ candidate_id: candidate.id, reason });
      addWarning(warnings, `${label} selected for ${candidate.path}; finding kept`);
      continue;
    }
    const projectCandidates = candidatesByProject.get(project) ?? [];
    projectCandidates.push(candidate);
    candidatesByProject.set(project, projectCandidates);
  }
  return { candidatesByProject, abstentions };
};

const scanSelectedProjects = (root, candidatesByProject, explicit, warnings) => {
  const selectedTsconfigs = [];
  const confirmedIds = new Set();
  const abstentions = [];
  const projectResults = [];
  let diagnosticsMs = 0;
  let symbolScanMs = 0;
  for (const [project, projectCandidates] of candidatesByProject) {
    const config = relativeConfigPath(root, project.configFileName);
    const source = explicit ? "explicit" : "auto";
    const sourceFileCount = project.program.getSourceFileNames().length;
    selectedTsconfigs.push(config);
    const diagnosticsStartedAt = performance.now();
    const blockingDiagnosticCount = countBlockingDiagnostics(project);
    diagnosticsMs += performance.now() - diagnosticsStartedAt;
    if (blockingDiagnosticCount > 0) {
      const diagnosticLabel = blockingDiagnosticCount === 1 ? "diagnostic" : "diagnostics";
      const recovery =
        config === "<inferred>"
          ? "pass an explicit tsconfig with --type-aware-project"
          : `run \`tsc -p ${config} --noEmit\``;
      addWarning(
        warnings,
        `${config} has ${blockingDiagnosticCount} blocking TypeScript ${diagnosticLabel}; semantic refinement abstained; ${recovery}`,
      );
      abstentions.push(
        ...projectCandidates.map((candidate) => ({
          candidate_id: candidate.id,
          reason: "blocking-diagnostics",
        })),
      );
      projectResults.push({
        config,
        source,
        status: "abstained",
        candidate_count: projectCandidates.length,
        confirmed_used_count: 0,
        unresolved_count: 0,
        abstained_count: projectCandidates.length,
        blocking_diagnostic_count: blockingDiagnosticCount,
        source_file_count: sourceFileCount,
        abstain_reason: "blocking-diagnostics",
      });
      continue;
    }
    const scanStartedAt = performance.now();
    scanProject({ project, candidates: projectCandidates, confirmedIds });
    symbolScanMs += performance.now() - scanStartedAt;
    const confirmedCount = projectCandidates.filter((candidate) =>
      confirmedIds.has(candidate.id),
    ).length;
    projectResults.push({
      config,
      source,
      status: "refined",
      candidate_count: projectCandidates.length,
      confirmed_used_count: confirmedCount,
      unresolved_count: projectCandidates.length - confirmedCount,
      abstained_count: 0,
      blocking_diagnostic_count: 0,
      source_file_count: sourceFileCount,
    });
  }
  return {
    selectedTsconfigs: new Set(selectedTsconfigs),
    confirmedIds,
    abstentions,
    projectResults,
    diagnosticsMs,
    symbolScanMs,
  };
};

const emptyAnalysis = () => ({
  selectedTsconfigs: [],
  confirmedIds: [],
  unresolvedIds: [],
  abstentions: [],
  projectResults: [],
  phaseTimings: { project_setup: 0, diagnostics: 0, symbol_scan: 0 },
  warnings: [],
});

export const analyzeClassMemberUses = ({ root, projects, candidates }) => {
  if (candidates.length === 0) {
    return emptyAnalysis();
  }

  const setupStartedAt = performance.now();
  const api = new API({ cwd: root });
  try {
    const snapshot = api.updateSnapshot({
      openFiles: candidates.map((candidate) => candidate.absolutePath),
      openProjects: projects.map((project) => project.absolutePath),
    });
    const warnings = [];
    const explicitProjects = projects
      .map((project) => snapshot.getProject(project.absolutePath))
      .filter(Boolean);
    const projectSetupMs = performance.now() - setupStartedAt;
    const selection = selectCandidateProjects(snapshot, candidates, explicitProjects, warnings);
    const analysis = scanSelectedProjects(
      root,
      selection.candidatesByProject,
      projects.length > 0,
      warnings,
    );
    const abstentions = [...selection.abstentions, ...analysis.abstentions];
    const abstainedIds = new Set(abstentions.map((abstention) => abstention.candidate_id));

    const unresolvedIds = candidates
      .filter(
        (candidate) => !analysis.confirmedIds.has(candidate.id) && !abstainedIds.has(candidate.id),
      )
      .map((candidate) => candidate.id);

    return {
      selectedTsconfigs: analysis.selectedTsconfigs,
      confirmedIds: analysis.confirmedIds,
      unresolvedIds,
      abstentions,
      projectResults: analysis.projectResults,
      phaseTimings: {
        project_setup: projectSetupMs,
        diagnostics: analysis.diagnosticsMs,
        symbol_scan: analysis.symbolScanMs,
      },
      warnings,
    };
  } finally {
    api.close();
  }
};
