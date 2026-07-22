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

const matchesDeclaration = (candidate, declaration) => {
  const checks = [
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

const countDiagnostics = (project) =>
  project.program.getConfigFileParsingDiagnostics().length +
  project.program.getProgramDiagnostics().length +
  project.program.getGlobalDiagnostics().length +
  project.program.getSyntacticDiagnostics().length +
  project.program.getBindDiagnostics().length +
  project.program.getSemanticDiagnostics().length;

const addWarning = (warnings, warning) => {
  if (warnings.length < MAX_WARNINGS) {
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

const selectCandidateProjects = (snapshot, candidates, warnings) => {
  const candidatesByProject = new Map();
  for (const candidate of candidates) {
    const project = snapshot.getDefaultProjectForFile(candidate.absolutePath);
    if (!project) {
      addWarning(warnings, `No TypeScript project selected for ${candidate.path}; finding kept`);
      continue;
    }
    const projectCandidates = candidatesByProject.get(project) ?? [];
    projectCandidates.push(candidate);
    candidatesByProject.set(project, projectCandidates);
  }
  return candidatesByProject;
};

const warnForProjectDiagnostics = (root, project, warnings) => {
  const diagnosticCount = countDiagnostics(project);
  if (diagnosticCount > 0) {
    const diagnosticLabel = diagnosticCount === 1 ? "diagnostic" : "diagnostics";
    addWarning(
      warnings,
      `${relativeConfigPath(root, project.configFileName)} has ${diagnosticCount} TypeScript ${diagnosticLabel}; unresolved findings were kept`,
    );
  }
};

const scanSelectedProjects = (root, candidatesByProject, warnings) => {
  const selectedTsconfigs = [];
  const confirmedIds = new Set();
  for (const [project, projectCandidates] of candidatesByProject) {
    selectedTsconfigs.push(relativeConfigPath(root, project.configFileName));
    warnForProjectDiagnostics(root, project, warnings);
    scanProject({ project, candidates: projectCandidates, confirmedIds });
  }
  return { selectedTsconfigs: new Set(selectedTsconfigs), confirmedIds };
};

const emptyAnalysis = () => ({
  selectedTsconfigs: [],
  confirmedIds: [],
  unresolvedIds: [],
  warnings: [],
});

export const analyzeClassMemberUses = ({ root, candidates }) => {
  if (candidates.length === 0) {
    return emptyAnalysis();
  }

  const api = new API({ cwd: root });
  try {
    const snapshot = api.updateSnapshot({
      openFiles: candidates.map((candidate) => candidate.absolutePath),
    });
    const warnings = [];
    const candidatesByProject = selectCandidateProjects(snapshot, candidates, warnings);

    if (candidatesByProject.size === 0) {
      throw new Error("TypeScript could not construct a project for any candidate");
    }
    const { selectedTsconfigs, confirmedIds } = scanSelectedProjects(
      root,
      candidatesByProject,
      warnings,
    );

    const unresolvedIds = candidates
      .filter((candidate) => !confirmedIds.has(candidate.id))
      .map((candidate) => candidate.id);

    return {
      selectedTsconfigs,
      confirmedIds,
      unresolvedIds,
      warnings,
    };
  } finally {
    api.close();
  }
};
