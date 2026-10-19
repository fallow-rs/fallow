#!/usr/bin/env node

import { execFileSync } from "node:child_process";
import { existsSync, lstatSync, readFileSync, realpathSync } from "node:fs";
import { dirname, isAbsolute, join, normalize, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const MANIFEST_PATH = "scripts/knowledge-surfaces.json";
const GENERATED_MARKER = "<!-- Generated from .agents/skills. Do not edit. -->";
const SOURCE_PATH_PATTERN =
  /`((?:\.agents|\.claude|\.github|action|benchmarks|ci|crates|docs|editors|npm|scripts|tests|viz-frontend)\/[A-Za-z0-9._*/<>-]+)`/gu;

const toPosix = (path) => path.split(sep).join("/");

const unique = (values) => [...new Set(values)];

const readJson = (root, path) => JSON.parse(readFileSync(join(root, path), "utf8"));

const trackedFiles = (root) =>
  execFileSync("git", ["ls-files", "-z", "--cached"], {
    cwd: root,
    encoding: "utf8",
  })
    .split("\0")
    .filter(Boolean)
    .map(toPosix);

const localMarkdownTargets = (sourcePath, content) => {
  const targets = [];
  const pattern = /(?<!!)\[[^\]]*]\(([^)]+)\)/g;
  for (const match of content.matchAll(pattern)) {
    let target = match[1].trim();
    if (target.startsWith("<") && target.endsWith(">")) {
      target = target.slice(1, -1);
    }
    target = target.split(/\s+(?=["'])/u)[0];
    if (
      target === "" ||
      target.startsWith("#") ||
      /^(?:https?:|mailto:|tel:|data:)/u.test(target)
    ) {
      continue;
    }
    const withoutFragment = target.split("#", 1)[0].split("?", 1)[0];
    if (withoutFragment === "") {
      continue;
    }
    const decoded = decodeURIComponent(withoutFragment);
    const resolved = toPosix(normalize(join(dirname(sourcePath), decoded)));
    targets.push(resolved);
  }
  return targets;
};

const isInsideRoot = (root, path) => {
  const rootPrefix = `${realpathSync(root)}${sep}`;
  const real = realpathSync(path);
  return real === realpathSync(root) || real.startsWith(rootPrefix);
};

const isLexicallyInsideRoot = (root, path) => {
  const relativePath = relative(resolve(root), resolve(path));
  return relativePath === "" || (!relativePath.startsWith(`..${sep}`) && relativePath !== "..");
};

const isSafeRelativePath = (path) =>
  typeof path === "string" &&
  path.trim() !== "" &&
  !isAbsolute(path) &&
  !path.includes("\\") &&
  !path.split("/").includes("..") &&
  toPosix(normalize(path)) === path;

const skillNames = (files, root) => {
  const escaped = root.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&");
  const pattern = new RegExp(`^${escaped}/([^/]+)/SKILL\\.md$`, "u");
  return files
    .map((path) => path.match(pattern)?.[1])
    .filter((name) => name !== undefined)
    .toSorted();
};

const addPathChecks = ({ errors, root, visible, path, kind, rejectSymlink = true }) => {
  const absolute = join(root, path);
  if (!existsSync(absolute)) {
    errors.push(`${kind} is missing: ${path}`);
    return;
  }
  if (!visible.has(path)) {
    errors.push(`${kind} is not tracked: ${path}`);
  }
  if (rejectSymlink && lstatSync(absolute).isSymbolicLink()) {
    errors.push(`${kind} must not be a symlink: ${path}`);
  }
  if (!isInsideRoot(root, absolute)) {
    errors.push(`${kind} escapes the repository root: ${path}`);
  }
};

const addTrustBoundaryChecks = ({ content, errors, manifest, path }) => {
  for (const prefix of manifest.trustBoundary?.forbiddenReferencePrefixes ?? []) {
    const escaped = prefix.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&");
    const leadingBoundary =
      prefix === "internal/" ? "(?:^|[\\s([{\"'=`])" : String.raw`(?:^|[^\w@-])`;
    if (new RegExp(`${leadingBoundary}${escaped}`, "mu").test(content)) {
      errors.push(`${path} references private path prefix: ${prefix}`);
    }
  }
  for (const forbidden of manifest.trustBoundary?.forbiddenReferencePatterns ?? []) {
    if (content.includes(forbidden.pattern)) {
      errors.push(`${path} references non-portable knowledge: ${forbidden.pattern}`);
    }
  }
  for (const repository of manifest.trustBoundary?.forbiddenRepositories ?? []) {
    const escaped = repository.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&");
    const pattern = new RegExp(
      `(?:(?:(?:https?|git):)?//(?:www\\.)?github\\.com/|ssh://git@github\\.com/|git@github\\.com:)${escaped}(?:\\.git)?(?=$|[/?#:\\s\\x60),.])`,
      "iu",
    );
    if (pattern.test(content)) {
      errors.push(`${path} references private repository: ${repository}`);
    }
  }
};

const addSourcePathChecks = ({ content, errors, manifest, root, path, visible }) => {
  for (const match of content.matchAll(SOURCE_PATH_PATTERN)) {
    const target = match[1];
    if (/[*<>[\]?{}]/u.test(target)) {
      continue;
    }
    const generated = (manifest.generatedPathContracts ?? []).some(
      ({ path: generatedPath }) =>
        target === generatedPath || target.startsWith(`${generatedPath}/`),
    );
    if (generated) {
      continue;
    }
    const absolute = resolve(root, target);
    if (!isLexicallyInsideRoot(root, absolute)) {
      errors.push(`${path} references path outside the repository: ${target}`);
    } else if (!existsSync(absolute)) {
      errors.push(`${path} references missing repository path: ${target}`);
    } else if (!isInsideRoot(root, absolute)) {
      errors.push(`${path} references symlink outside the repository: ${target}`);
    } else if (
      !visible.has(target) &&
      ![...visible].some((visiblePath) => visiblePath.startsWith(`${target.replace(/\/+$/u, "")}/`))
    ) {
      errors.push(`${path} references untracked repository path: ${target}`);
    }
  }
};

const addDocumentChecks = ({
  content,
  errors,
  manifest,
  path,
  root,
  visible,
  checkSourcePaths = false,
}) => {
  addTrustBoundaryChecks({ content, errors, manifest, path });
  if (checkSourcePaths) {
    addSourcePathChecks({ content, errors, manifest, path, root, visible });
  }
  for (const target of localMarkdownTargets(path, content)) {
    const absolute = resolve(root, target);
    if (!isLexicallyInsideRoot(root, absolute)) {
      errors.push(`${path} links outside the repository root: ${target}`);
    } else if (!existsSync(absolute)) {
      errors.push(`${path} links to missing local path: ${target}`);
    } else if (!isInsideRoot(root, absolute)) {
      errors.push(`${path} links through a path outside the repository root: ${target}`);
    } else {
      const repositoryPath = toPosix(relative(root, absolute));
      if (
        !visible.has(repositoryPath) &&
        ![...visible].some((visiblePath) => visiblePath.startsWith(`${repositoryPath}/`))
      ) {
        errors.push(`${path} links to untracked local path: ${target}`);
      }
    }
  }
};

const entryPointRoutes = (sourcePath, content) => {
  const withoutComments = content.replace(/<!--[\s\S]*?-->/gu, "");
  const normalizeRoute = (route) =>
    toPosix(normalize(route.replace(/^@/u, ""))).replace(/\/+$/u, "");
  const routes = new Set(localMarkdownTargets(sourcePath, withoutComments).map(normalizeRoute));
  for (const match of withoutComments.matchAll(/`([^`\r\n]+)`/gu)) {
    routes.add(normalizeRoute(match[1]));
  }
  for (const match of withoutComments.matchAll(/^@([^\s\r\n]+)$/gmu)) {
    routes.add(normalizeRoute(match[1]));
  }
  return routes;
};

const REQUIRED_EXTERNAL_FIELDS = [
  "repository",
  "role",
  "direction",
  "revisionSource",
  "allowedRoot",
];
const VALID_DIRECTIONS = new Set(["canonical-contract-to-public-consumer", "public-to-consumer"]);
const VALID_PROCESSOR_KINDS = new Set(["generator", "validator", "none"]);
const SHELL_CONTROL_PATTERN = /^(?:&&|\|\||[;|])$/u;

const readText = (root, path) => readFileSync(join(root, path), "utf8");

const isExecutableCommand = (command) =>
  Array.isArray(command) &&
  command.length > 0 &&
  !command.some((part) => typeof part !== "string" || part.trim() === "");

const checkDocument = (context, path, checkSourcePaths = true) => {
  addDocumentChecks({
    ...context,
    content: readText(context.root, path),
    path,
    checkSourcePaths,
  });
};

const addClassificationChecks = ({ errors, label, classified, tracked }) => {
  if (unique(classified).length !== classified.length) {
    errors.push(`knowledge manifest contains duplicate ${label} paths`);
  }
  for (const path of tracked.filter((candidate) => !classified.includes(candidate))) {
    errors.push(`${label} is not classified: ${path}`);
  }
  for (const path of classified.filter((candidate) => !tracked.includes(candidate))) {
    errors.push(`classified ${label} is not tracked: ${path}`);
  }
};

const checkManifestHeader = (context) => {
  const { errors, manifest } = context;
  if (manifest.schemaVersion !== 2) {
    errors.push(`unsupported knowledge manifest schema: ${manifest.schemaVersion}`);
  }
  if (manifest.trustBoundary?.automatedPrivateToPublic !== false) {
    errors.push("trust boundary must forbid automated private-to-public publication");
  }
  addPathChecks({ ...context, path: MANIFEST_PATH, kind: "knowledge manifest" });
};

const checkEntryPointRoutes = ({ errors, manifest }, entryPoint, discoveredRoutes) => {
  const forbiddenPrefixes = manifest.trustBoundary?.forbiddenRoutePrefixes ?? [];
  for (const route of entryPoint.routes ?? []) {
    if (isAbsolute(route) || route.startsWith("../")) {
      errors.push(`${entryPoint.path} has a non-repository route: ${route}`);
      continue;
    }
    if (forbiddenPrefixes.some((prefix) => route.startsWith(prefix))) {
      errors.push(`${entryPoint.path} routes across the private boundary: ${route}`);
    }
    if (!discoveredRoutes.has(route)) {
      errors.push(`${entryPoint.path} does not route to ${route}`);
    }
  }
};

const checkEntryPoints = (context) => {
  for (const entryPoint of context.manifest.entryPoints ?? []) {
    addPathChecks({
      ...context,
      path: entryPoint.path,
      kind: `${entryPoint.client} entry point`,
    });
    if (!existsSync(join(context.root, entryPoint.path))) {
      continue;
    }
    const content = readText(context.root, entryPoint.path);
    addDocumentChecks({ ...context, content, path: entryPoint.path });
    checkEntryPointRoutes(context, entryPoint, entryPointRoutes(entryPoint.path, content));
  }
};

const checkMaintainerDocClassification = (context, docPaths) => {
  const { currentVisibleFiles, errors, manifest } = context;
  const ignoredPrefixes =
    manifest.maintainerDocs?.ignoredWorktreePrefixes?.map(({ path }) => path) ?? [];
  const trackedDocs = currentVisibleFiles
    .filter(
      (path) =>
        path.startsWith("docs/") &&
        path.endsWith(".md") &&
        !ignoredPrefixes.some((prefix) => path.startsWith(prefix)),
    )
    .toSorted();
  addClassificationChecks({
    errors,
    label: "maintainer document",
    classified: docPaths,
    tracked: trackedDocs,
  });
};

const checkRootDocuments = (context) => {
  const { currentVisibleFiles, errors, manifest, root } = context;
  const entryPointPaths = new Set((manifest.entryPoints ?? []).map(({ path }) => path));
  const rootDocs = manifest.maintainerDocs?.rootDocuments ?? [];
  const trackedRootDocs = currentVisibleFiles
    .filter((path) => path.endsWith(".md") && !path.includes("/") && !entryPointPaths.has(path))
    .toSorted();
  addClassificationChecks({
    errors,
    label: "root document",
    classified: rootDocs.map(({ path }) => path),
    tracked: trackedRootDocs,
  });
  for (const { path, section } of rootDocs) {
    addPathChecks({ ...context, path, kind: "root document" });
    if (existsSync(join(root, path)) && section !== "release-history") {
      checkDocument(context, path, false);
    }
  }
};

const checkMaintainerDocuments = (context, docs) => {
  for (const { path } of docs) {
    addPathChecks({ ...context, path, kind: "maintainer document" });
    if (existsSync(join(context.root, path))) {
      checkDocument(context, path);
    }
  }
};

const checkDocsIndex = ({ errors, manifest, root }, docPaths) => {
  const indexPath = manifest.maintainerDocs?.index;
  if (!docPaths.includes(indexPath)) {
    errors.push(`maintainer docs index is not classified: ${indexPath}`);
    return;
  }
  if (!existsSync(join(root, indexPath))) {
    return;
  }
  const linked = new Set(localMarkdownTargets(indexPath, readText(root, indexPath)));
  for (const path of docPaths) {
    if (path !== indexPath && !linked.has(path)) {
      errors.push(`${indexPath} does not index ${path}`);
    }
  }
};

const checkIgnoredWorktreePrefixes = ({ currentVisibleFiles, errors, manifest }) => {
  for (const ignored of manifest.maintainerDocs?.ignoredWorktreePrefixes ?? []) {
    const leaked = currentVisibleFiles.find((path) => path.startsWith(ignored.path));
    if (leaked !== undefined) {
      errors.push(`${ignored.path} must remain ignored, but Git can see ${leaked}`);
    }
  }
};

const checkNestedGuides = (context) => {
  for (const path of context.currentVisibleFiles.filter((candidate) =>
    candidate.endsWith("/AGENTS.md"),
  )) {
    checkDocument(context, path);
  }
};

const isValidHostRule = (hostRule) =>
  typeof hostRule.root === "string" &&
  hostRule.root.trim() !== "" &&
  hostRule.mode === "scoped-router";

const isHostRulePath = (path) =>
  (path.startsWith(".claude/rules/") || path.startsWith(".agents/rules/")) && path.endsWith(".md");

const checkHostRuleTree = (context, hostRule, classifiedHostRules) => {
  const prefix = `${hostRule.root.replace(/\/+$/u, "")}/`;
  const rules = context.currentVisibleFiles
    .filter((path) => path.startsWith(prefix) && path.endsWith(".md"))
    .toSorted();
  if (rules.length === 0) {
    context.errors.push(`${hostRule.client} host rule tree is empty: ${hostRule.root}`);
  }
  for (const path of rules) {
    classifiedHostRules.add(path);
    checkDocument(context, path);
  }
};

const checkHostRules = (context) => {
  const { currentVisibleFiles, errors, manifest } = context;
  const hostRules = manifest.hostRules ?? [];
  if (hostRules.length === 0) {
    errors.push("knowledge manifest must classify host rule trees");
  }
  const classifiedHostRules = new Set();
  for (const hostRule of hostRules) {
    if (isValidHostRule(hostRule)) {
      checkHostRuleTree(context, hostRule, classifiedHostRules);
    } else {
      errors.push(`invalid ${hostRule.client ?? "<unknown>"} host rule contract`);
    }
  }
  for (const path of currentVisibleFiles.filter(isHostRulePath)) {
    if (!classifiedHostRules.has(path)) {
      errors.push(`host rule is not classified: ${path}`);
    }
  }
};

const checkCanonicalSkills = (context) => {
  const canonicalRoot = context.manifest.skills?.canonicalRoot;
  const canonical = skillNames(context.currentVisibleFiles, canonicalRoot);
  if (canonical.length === 0) {
    context.errors.push(`canonical skill tree is empty: ${canonicalRoot}`);
  }
  for (const name of canonical) {
    const path = `${canonicalRoot}/${name}/SKILL.md`;
    addPathChecks({ ...context, path, kind: "canonical skill" });
    checkDocument(context, path);
  }
  return canonical;
};

const checkSkillAdapter = (context, adapter, canonical) => {
  const { errors, root } = context;
  const generated = adapter.mode === "generated";
  const adapters = skillNames(context.currentVisibleFiles, adapter.path);
  if (generated && adapters.join("\0") !== canonical.join("\0")) {
    errors.push(
      `${adapter.client} generated adapters do not match canonical skills: ` +
        `canonical=[${canonical.join(", ")}], adapters=[${adapters.join(", ")}]`,
    );
  }
  for (const name of adapters) {
    const path = `${adapter.path}/${name}/SKILL.md`;
    addPathChecks({ ...context, path, kind: `${adapter.client} skill adapter` });
    if (generated && !readText(root, path).includes(GENERATED_MARKER)) {
      errors.push(`${path} is missing the generated adapter marker`);
    }
    checkDocument(context, path);
  }
};

const checkSkills = (context) => {
  const canonical = checkCanonicalSkills(context);
  for (const adapter of context.manifest.skills?.adapterRoots ?? []) {
    checkSkillAdapter(context, adapter, canonical);
  }
  addPathChecks({
    ...context,
    path: context.manifest.skills?.generator,
    kind: "agent adapter generator",
  });
};

const checkGeneratedSetupDocument = ({ errors, root }, contract) => {
  if (!existsSync(join(root, contract.setupDocument))) {
    return;
  }
  const setupContent = readText(root, contract.setupDocument);
  const command = contract.setupCommand.join(" ");
  if (!setupContent.includes(command)) {
    errors.push(`${contract.setupDocument} does not document generated path command: ${command}`);
  }
  if (!setupContent.includes(contract.path)) {
    errors.push(`${contract.setupDocument} does not document generated path: ${contract.path}`);
  }
};

const checkGeneratedContract = (context, contract) => {
  const { errors } = context;
  if (!isSafeRelativePath(contract.path)) {
    errors.push(`generated path contract is unsafe: ${contract.path ?? "<unknown>"}`);
  }
  if (!isSafeRelativePath(contract.setupDocument)) {
    errors.push(
      `generated path setup document is unsafe: ${contract.setupDocument ?? "<unknown>"}`,
    );
    return;
  }
  addPathChecks({
    ...context,
    path: contract.setupDocument,
    kind: "generated path setup document",
  });
  if (!isExecutableCommand(contract.setupCommand)) {
    errors.push(`generated path contract has no executable setupCommand: ${contract.path}`);
    return;
  }
  checkGeneratedSetupDocument(context, contract);
};

const checkGeneratedContracts = (context) => {
  for (const contract of context.manifest.generatedPathContracts ?? []) {
    checkGeneratedContract(context, contract);
  }
};

const processorProblem = (processor) => {
  if (
    typeof processor !== "object" ||
    processor === null ||
    !VALID_PROCESSOR_KINDS.has(processor.kind)
  ) {
    return "has invalid processor";
  }
  if (processor.kind === "none") {
    return null;
  }
  if (typeof processor.entrypoint !== "string" || processor.entrypoint.trim() === "") {
    return "is missing processor entrypoint";
  }
  if (!isSafeRelativePath(processor.entrypoint)) {
    return `has unsafe processor entrypoint: ${processor.entrypoint}`;
  }
  return null;
};

const checkCommandProblem = (checkCommand) => {
  if (!isExecutableCommand(checkCommand)) {
    return "has no executable checkCommand";
  }
  if (checkCommand.some((part) => SHELL_CONTROL_PATTERN.test(part))) {
    return "checkCommand contains shell control";
  }
  return null;
};

const externalSourceProblems = (source) => {
  const problems = REQUIRED_EXTERNAL_FIELDS.filter(
    (field) => typeof source[field] !== "string" || source[field].trim() === "",
  ).map((field) => `is missing ${field}`);
  if (!VALID_DIRECTIONS.has(source.direction)) {
    problems.push(`has invalid direction: ${source.direction}`);
  }
  if (typeof source.allowedRoot === "string" && !isSafeRelativePath(source.allowedRoot)) {
    problems.push(`has unsafe allowedRoot: ${source.allowedRoot}`);
  }
  problems.push(processorProblem(source.processor), checkCommandProblem(source.checkCommand));
  return problems.filter((problem) => problem !== null);
};

const checkExternalSources = ({ errors, manifest }) => {
  const externalSources = manifest.externalSources ?? [];
  if (externalSources.length === 0) {
    errors.push("knowledge manifest must declare external source contracts");
  }
  for (const source of externalSources) {
    const name = source.repository ?? "<unknown>";
    for (const problem of externalSourceProblems(source)) {
      errors.push(`external source ${name} ${problem}`);
    }
  }
};

export const validateKnowledgeArchitecture = ({
  root = ROOT,
  visibleFiles = trackedFiles(root),
} = {}) => {
  const errors = [];
  const currentVisibleFiles = visibleFiles
    .map(toPosix)
    .filter((path) => existsSync(join(root, path)));
  const context = {
    currentVisibleFiles,
    errors,
    manifest: readJson(root, MANIFEST_PATH),
    root,
    visible: new Set(currentVisibleFiles),
  };
  const docs = context.manifest.maintainerDocs?.documents ?? [];
  const docPaths = docs.map(({ path }) => path);

  checkManifestHeader(context);
  checkEntryPoints(context);
  checkMaintainerDocClassification(context, docPaths);
  checkRootDocuments(context);
  checkMaintainerDocuments(context, docs);
  checkDocsIndex(context, docPaths);
  checkIgnoredWorktreePrefixes(context);
  checkNestedGuides(context);
  checkHostRules(context);
  checkSkills(context);
  checkGeneratedContracts(context);
  checkExternalSources(context);

  return errors;
};

export const main = () => {
  try {
    const errors = validateKnowledgeArchitecture();
    if (errors.length > 0) {
      for (const error of errors) {
        process.stderr.write(`knowledge architecture: ${error}\n`);
      }
      return 1;
    }
    process.stdout.write("Knowledge architecture is consistent.\n");
    return 0;
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    process.stderr.write(`knowledge architecture: ${message}\n`);
    return 2;
  }
};

if (process.argv[1] !== undefined && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = main();
}
