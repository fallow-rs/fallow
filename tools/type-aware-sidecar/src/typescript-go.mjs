import path from "node:path";

import { API } from "typescript/unstable/sync";
import {
  isClassDeclaration,
  isClassExpression,
  isElementAccessExpression,
  isGetAccessorDeclaration,
  isMethodDeclaration,
  isPrivateIdentifier,
  isPropertyAccessExpression,
  isPropertyDeclaration,
  isSetAccessorDeclaration,
  isStringLiteralLikeNode,
} from "typescript/unstable/ast/is";

import { canonicalFileIdentity } from "./file-identity.mjs";

const MAX_WARNINGS = 20;
const MAX_USE_EVIDENCE_PER_CANDIDATE = 20;
const INFERRED_PROJECT = "<inferred>";

const sourceFileIdentities = new WeakMap();

const sourceFileIdentity = (sourceFile) => {
  const cached = sourceFileIdentities.get(sourceFile);
  if (cached) {
    return cached;
  }
  const identity = canonicalFileIdentity(sourceFile.fileName);
  sourceFileIdentities.set(sourceFile, identity);
  return identity;
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
  sourceFileIdentity(declaration.getSourceFile()) === candidate.fileIdentity;

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

const useLocation = (root, node) => {
  const sourceFile = node.getSourceFile();
  const start = node.getStart(sourceFile);
  const { line } = sourceFile.getLineAndCharacterOfPosition(start);
  return {
    path: path.relative(root, sourceFile.fileName).split(path.sep).join("/"),
    line: line + 1,
    col: Buffer.byteLength(
      sourceFile.text.slice(sourceFile.getPositionOfLineAndCharacter(line, 0), start),
      "utf8",
    ),
  };
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

const propertyAccessName = (node, candidateNames) => {
  if (!isPropertyAccessExpression(node)) return undefined;
  if (isPrivateIdentifier(node.name)) return undefined;
  return candidateNames.has(node.name.text) ? node.name : undefined;
};

const elementAccessName = (node, candidateNames) => {
  if (!isElementAccessExpression(node)) return undefined;
  const argument = node.argumentExpression;
  if (!isStringLiteralLikeNode(argument)) return undefined;
  return candidateNames.has(argument.text) ? argument : undefined;
};

const candidatePropertyName = (node, candidateNames) =>
  propertyAccessName(node, candidateNames) ?? elementAccessName(node, candidateNames);

const collectPropertyNames = (sourceFile, candidateNames) => {
  const nodes = [];
  const visit = (node) => {
    const candidate = candidatePropertyName(node, candidateNames);
    if (candidate) nodes.push(candidate);
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

const sameLocation = (left, right) =>
  left.path === right.path && left.line === right.line && left.col === right.col;

const recordConfirmedUse = (candidateId, location, confirmedUses) => {
  const locations = confirmedUses.get(candidateId) ?? [];
  if (locations.length >= MAX_USE_EVIDENCE_PER_CANDIDATE) return;
  if (locations.some((existing) => sameLocation(existing, location))) return;
  locations.push(location);
  confirmedUses.set(candidateId, locations);
};

const confirmSymbolUses = ({
  symbol,
  memberName,
  useNode,
  root,
  candidatesByMember,
  project,
  confirmedIds,
  confirmedUses,
}) => {
  const possibleCandidates = candidatesByMember.get(memberName) ?? [];
  for (const declarationHandle of symbolDeclarations(symbol)) {
    for (const candidateId of matchingCandidateIds(
      declarationHandle,
      project,
      possibleCandidates,
    )) {
      confirmedIds.add(candidateId);
      recordConfirmedUse(candidateId, useLocation(root, useNode), confirmedUses);
    }
  }
};

const scanSourceFile = ({
  sourceFile,
  candidateNames,
  candidatesByMember,
  project,
  confirmedIds,
  confirmedUses,
  root,
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
      useNode: propertyNames[index],
      root,
      candidatesByMember,
      project,
      confirmedIds,
      confirmedUses,
    });
  }
};

const scanProject = ({ root, project, candidates, confirmedIds, confirmedUses }) => {
  const candidatesByMember = groupCandidatesByMember(candidates);
  const candidateNames = new Set(candidatesByMember.keys());

  for (const fileName of project.program.getSourceFileNames()) {
    const sourceFile = project.program.getSourceFile(fileName);
    if (shouldSkipSourceFile(project, sourceFile)) {
      continue;
    }
    scanSourceFile({
      sourceFile,
      candidateNames,
      candidatesByMember,
      project,
      confirmedIds,
      confirmedUses,
      root,
    });
  }
};

const candidateProject = (snapshot, candidate, explicitProjects) => {
  if (explicitProjects.length === 0) {
    return snapshot.getDefaultProjectForFile(candidate.absolutePath);
  }
  return explicitProjects.find((project) => project.program.getSourceFile(candidate.absolutePath));
};

const recordMissingProject = (candidate, abstentions, warnings) => {
  abstentions.push({ candidate_id: candidate.id, reason: "no-project" });
  addWarning(warnings, `No TypeScript project selected for ${candidate.path}; finding kept`);
};

const addProjectCandidate = (candidatesByProject, project, candidate) => {
  const projectCandidates = candidatesByProject.get(project) ?? [];
  projectCandidates.push(candidate);
  candidatesByProject.set(project, projectCandidates);
};

const selectCandidateProjects = (snapshot, candidates, explicitProjects, warnings) => {
  const candidatesByProject = new Map();
  const abstentions = [];
  for (const candidate of candidates) {
    const project = candidateProject(snapshot, candidate, explicitProjects);
    if (!project) {
      recordMissingProject(candidate, abstentions, warnings);
      continue;
    }
    addProjectCandidate(candidatesByProject, project, candidate);
  }
  return { candidatesByProject, abstentions };
};

const diagnosticWarning = (config, count) => {
  const label = count === 1 ? "diagnostic" : "diagnostics";
  const recovery =
    config === INFERRED_PROJECT
      ? "pass an explicit tsconfig with --type-aware-project"
      : `run \`tsc -p ${config} --noEmit\``;
  return `${config} has ${count} blocking TypeScript ${label}; semantic refinement abstained; ${recovery}`;
};

const inspectProject = (root, project, projectCandidates, explicit, warnings) => {
  const config = relativeConfigPath(root, project.configFileName);
  const startedAt = performance.now();
  const diagnosticCount = countBlockingDiagnostics(project);
  const diagnosticsMs = performance.now() - startedAt;
  const state = {
    project,
    projectCandidates,
    config,
    source: explicit ? "explicit" : "auto",
    status: diagnosticCount > 0 ? "abstained" : "refined",
    blocking_diagnostic_count: diagnosticCount,
    source_file_count: project.program.getSourceFileNames().length,
  };
  if (diagnosticCount > 0) {
    state.abstain_reason = "blocking-diagnostics";
    addWarning(warnings, diagnosticWarning(config, diagnosticCount));
  }
  return { state, diagnosticsMs };
};

const inspectProjects = (root, candidatesByProject, scanProjects, explicit, warnings) => {
  const states = [];
  let diagnosticsMs = 0;
  for (const project of scanProjects) {
    const inspection = inspectProject(
      root,
      project,
      candidatesByProject.get(project) ?? [],
      explicit,
      warnings,
    );
    states.push(inspection.state);
    diagnosticsMs += inspection.diagnosticsMs;
  }
  return { states, diagnosticsMs };
};

const collectAbstentions = (states) => {
  const abstentions = [];
  const ids = new Set();
  for (const state of states.filter((entry) => entry.status === "abstained")) {
    for (const candidate of state.projectCandidates) {
      abstentions.push({ candidate_id: candidate.id, reason: "blocking-diagnostics" });
      ids.add(candidate.id);
    }
  }
  return { abstentions, ids };
};

const scanRefinedProjects = (root, states, candidates, confirmedIds, confirmedUses) => {
  if (candidates.length === 0) return 0;
  let elapsedMs = 0;
  for (const state of states.filter((entry) => entry.status === "refined")) {
    const startedAt = performance.now();
    scanProject({
      root,
      project: state.project,
      candidates,
      confirmedIds,
      confirmedUses,
    });
    elapsedMs += performance.now() - startedAt;
  }
  return elapsedMs;
};

const projectResult = (state, confirmedIds) => {
  const { project: _project, projectCandidates, ...result } = state;
  const confirmedCount = projectCandidates.filter((candidate) =>
    confirmedIds.has(candidate.id),
  ).length;
  const refined = result.status === "refined";
  return {
    ...result,
    candidate_count: projectCandidates.length,
    confirmed_used_count: confirmedCount,
    unresolved_count: refined ? projectCandidates.length - confirmedCount : 0,
    abstained_count: refined ? 0 : projectCandidates.length,
  };
};

const scanSelectedProjects = (root, candidatesByProject, scanProjects, explicit, warnings) => {
  const confirmedIds = new Set();
  const confirmedUses = new Map();
  const inspected = inspectProjects(root, candidatesByProject, scanProjects, explicit, warnings);
  const abstained = collectAbstentions(inspected.states);
  const activeCandidates = [...candidatesByProject.values()]
    .flat()
    .filter((candidate) => !abstained.ids.has(candidate.id));
  const symbolScanMs = scanRefinedProjects(
    root,
    inspected.states,
    activeCandidates,
    confirmedIds,
    confirmedUses,
  );
  return {
    selectedTsconfigs: new Set(inspected.states.map((state) => state.config)),
    confirmedIds,
    confirmedUses,
    abstentions: abstained.abstentions,
    projectResults: inspected.states.map((state) => projectResult(state, confirmedIds)),
    diagnosticsMs: inspected.diagnosticsMs,
    symbolScanMs,
  };
};

const emptyAnalysis = () => ({
  selectedTsconfigs: [],
  confirmedIds: [],
  confirmedUses: new Map(),
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
  const candidatesWithIdentity = candidates.map((candidate) => ({
    ...candidate,
    fileIdentity: canonicalFileIdentity(candidate.absolutePath),
  }));
  const api = new API({ cwd: root });
  try {
    const snapshot = api.updateSnapshot({
      openFiles: [...new Set(candidatesWithIdentity.map((candidate) => candidate.absolutePath))],
      openProjects: projects.map((project) => project.absolutePath),
    });
    const warnings = [];
    const explicitProjects = projects
      .map((project) => snapshot.getProject(project.absolutePath))
      .filter(Boolean);
    const projectSetupMs = performance.now() - setupStartedAt;
    const selection = selectCandidateProjects(
      snapshot,
      candidatesWithIdentity,
      explicitProjects,
      warnings,
    );
    const scanProjects = (
      explicitProjects.length > 0 ? explicitProjects : [...selection.candidatesByProject.keys()]
    ).filter((project) => project.program.getSourceFileNames().length > 0);
    const analysis = scanSelectedProjects(
      root,
      selection.candidatesByProject,
      scanProjects,
      projects.length > 0,
      warnings,
    );
    const abstentions = [...selection.abstentions, ...analysis.abstentions];
    const abstainedIds = new Set(abstentions.map((abstention) => abstention.candidate_id));

    const unresolvedIds = candidatesWithIdentity
      .filter(
        (candidate) => !analysis.confirmedIds.has(candidate.id) && !abstainedIds.has(candidate.id),
      )
      .map((candidate) => candidate.id);

    return {
      selectedTsconfigs: analysis.selectedTsconfigs,
      confirmedIds: analysis.confirmedIds,
      confirmedUses: analysis.confirmedUses,
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
