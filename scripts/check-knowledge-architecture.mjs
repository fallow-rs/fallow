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

export const validateKnowledgeArchitecture = ({
  root = ROOT,
  visibleFiles = trackedFiles(root),
} = {}) => {
  const errors = [];
  const currentVisibleFiles = visibleFiles
    .map(toPosix)
    .filter((path) => existsSync(join(root, path)));
  const visible = new Set(currentVisibleFiles);
  const manifest = readJson(root, MANIFEST_PATH);

  if (manifest.schemaVersion !== 2) {
    errors.push(`unsupported knowledge manifest schema: ${manifest.schemaVersion}`);
  }
  if (manifest.trustBoundary?.automatedPrivateToPublic !== false) {
    errors.push("trust boundary must forbid automated private-to-public publication");
  }

  addPathChecks({
    errors,
    root,
    visible,
    path: MANIFEST_PATH,
    kind: "knowledge manifest",
  });

  for (const entryPoint of manifest.entryPoints ?? []) {
    addPathChecks({
      errors,
      root,
      visible,
      path: entryPoint.path,
      kind: `${entryPoint.client} entry point`,
    });
    if (!existsSync(join(root, entryPoint.path))) {
      continue;
    }
    const content = readFileSync(join(root, entryPoint.path), "utf8");
    addDocumentChecks({ content, errors, manifest, path: entryPoint.path, root, visible });
    const discoveredRoutes = entryPointRoutes(entryPoint.path, content);
    for (const route of entryPoint.routes ?? []) {
      if (isAbsolute(route) || route.startsWith("../")) {
        errors.push(`${entryPoint.path} has a non-repository route: ${route}`);
        continue;
      }
      if (
        (manifest.trustBoundary?.forbiddenRoutePrefixes ?? []).some((prefix) =>
          route.startsWith(prefix),
        )
      ) {
        errors.push(`${entryPoint.path} routes across the private boundary: ${route}`);
      }
      if (!discoveredRoutes.has(route)) {
        errors.push(`${entryPoint.path} does not route to ${route}`);
      }
    }
  }

  const docs = manifest.maintainerDocs?.documents ?? [];
  const docPaths = docs.map(({ path }) => path);
  if (unique(docPaths).length !== docPaths.length) {
    errors.push("knowledge manifest contains duplicate maintainer document paths");
  }

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
  const uncoveredDocs = trackedDocs.filter((path) => !docPaths.includes(path));
  const phantomDocs = docPaths.filter((path) => !trackedDocs.includes(path));
  for (const path of uncoveredDocs) {
    errors.push(`maintainer document is not classified: ${path}`);
  }
  for (const path of phantomDocs) {
    errors.push(`classified maintainer document is not tracked: ${path}`);
  }

  const entryPointPaths = new Set((manifest.entryPoints ?? []).map(({ path }) => path));
  const rootDocs = manifest.maintainerDocs?.rootDocuments ?? [];
  const rootDocPaths = rootDocs.map(({ path }) => path);
  if (unique(rootDocPaths).length !== rootDocPaths.length) {
    errors.push("knowledge manifest contains duplicate root document paths");
  }
  const trackedRootDocs = currentVisibleFiles
    .filter((path) => path.endsWith(".md") && !path.includes("/") && !entryPointPaths.has(path))
    .toSorted();
  for (const rootDoc of trackedRootDocs.filter((candidate) => !rootDocPaths.includes(candidate))) {
    errors.push(`root document is not classified: ${rootDoc}`);
  }
  for (const rootDoc of rootDocPaths.filter((candidate) => !trackedRootDocs.includes(candidate))) {
    errors.push(`classified root document is not tracked: ${rootDoc}`);
  }

  for (const { path, section } of rootDocs) {
    addPathChecks({ errors, root, visible, path, kind: "root document" });
    if (existsSync(join(root, path)) && section !== "release-history") {
      addDocumentChecks({
        content: readFileSync(join(root, path), "utf8"),
        errors,
        manifest,
        path,
        root,
        visible,
      });
    }
  }

  for (const { path } of docs) {
    addPathChecks({ errors, root, visible, path, kind: "maintainer document" });
    if (!existsSync(join(root, path))) {
      continue;
    }
    const content = readFileSync(join(root, path), "utf8");
    addDocumentChecks({
      content,
      errors,
      manifest,
      path,
      root,
      visible,
      checkSourcePaths: true,
    });
  }

  const indexPath = manifest.maintainerDocs?.index;
  if (!docPaths.includes(indexPath)) {
    errors.push(`maintainer docs index is not classified: ${indexPath}`);
  } else if (existsSync(join(root, indexPath))) {
    const linked = new Set(
      localMarkdownTargets(indexPath, readFileSync(join(root, indexPath), "utf8")),
    );
    for (const path of docPaths) {
      if (path !== indexPath && !linked.has(path)) {
        errors.push(`${indexPath} does not index ${path}`);
      }
    }
  }

  for (const ignored of manifest.maintainerDocs?.ignoredWorktreePrefixes ?? []) {
    const leaked = currentVisibleFiles.find((path) => path.startsWith(ignored.path));
    if (leaked !== undefined) {
      errors.push(`${ignored.path} must remain ignored, but Git can see ${leaked}`);
    }
  }

  const nestedGuides = currentVisibleFiles.filter((path) => path.endsWith("/AGENTS.md"));
  for (const path of nestedGuides) {
    addDocumentChecks({
      content: readFileSync(join(root, path), "utf8"),
      errors,
      manifest,
      path,
      root,
      visible,
      checkSourcePaths: true,
    });
  }

  const hostRules = manifest.hostRules ?? [];
  if (hostRules.length === 0) {
    errors.push("knowledge manifest must classify host rule trees");
  }
  const classifiedHostRules = new Set();
  for (const hostRule of hostRules) {
    if (
      typeof hostRule.root !== "string" ||
      hostRule.root.trim() === "" ||
      hostRule.mode !== "scoped-router"
    ) {
      errors.push(`invalid ${hostRule.client ?? "<unknown>"} host rule contract`);
      continue;
    }
    const prefix = `${hostRule.root.replace(/\/+$/u, "")}/`;
    const rules = currentVisibleFiles
      .filter((path) => path.startsWith(prefix) && path.endsWith(".md"))
      .toSorted();
    if (rules.length === 0) {
      errors.push(`${hostRule.client} host rule tree is empty: ${hostRule.root}`);
    }
    for (const path of rules) {
      classifiedHostRules.add(path);
      addDocumentChecks({
        content: readFileSync(join(root, path), "utf8"),
        errors,
        manifest,
        path,
        root,
        visible,
        checkSourcePaths: true,
      });
    }
  }
  for (const path of currentVisibleFiles.filter(
    (candidate) => candidate.startsWith(".claude/rules/") && candidate.endsWith(".md"),
  )) {
    if (!classifiedHostRules.has(path)) {
      errors.push(`host rule is not classified: ${path}`);
    }
  }

  const canonicalRoot = manifest.skills?.canonicalRoot;
  const canonical = skillNames(currentVisibleFiles, canonicalRoot);
  if (canonical.length === 0) {
    errors.push(`canonical skill tree is empty: ${canonicalRoot}`);
  }
  for (const name of canonical) {
    const path = `${canonicalRoot}/${name}/SKILL.md`;
    addPathChecks({
      errors,
      root,
      visible,
      path,
      kind: "canonical skill",
    });
    addDocumentChecks({
      content: readFileSync(join(root, path), "utf8"),
      errors,
      manifest,
      path,
      root,
      visible,
      checkSourcePaths: true,
    });
  }

  for (const adapter of manifest.skills?.adapterRoots ?? []) {
    const adapters = skillNames(currentVisibleFiles, adapter.path);
    if (adapter.mode === "generated" && adapters.join("\0") !== canonical.join("\0")) {
      errors.push(
        `${adapter.client} generated adapters do not match canonical skills: ` +
          `canonical=[${canonical.join(", ")}], adapters=[${adapters.join(", ")}]`,
      );
    }
    for (const name of adapters) {
      const path = `${adapter.path}/${name}/SKILL.md`;
      addPathChecks({
        errors,
        root,
        visible,
        path,
        kind: `${adapter.client} skill adapter`,
      });
      if (
        adapter.mode === "generated" &&
        !readFileSync(join(root, path), "utf8").includes(GENERATED_MARKER)
      ) {
        errors.push(`${path} is missing the generated adapter marker`);
      }
      addDocumentChecks({
        content: readFileSync(join(root, path), "utf8"),
        errors,
        manifest,
        path,
        root,
        visible,
        checkSourcePaths: true,
      });
    }
  }

  addPathChecks({
    errors,
    root,
    visible,
    path: manifest.skills?.generator,
    kind: "agent adapter generator",
  });

  const generatedContracts = manifest.generatedPathContracts ?? [];
  for (const contract of generatedContracts) {
    if (!isSafeRelativePath(contract.path)) {
      errors.push(`generated path contract is unsafe: ${contract.path ?? "<unknown>"}`);
    }
    if (!isSafeRelativePath(contract.setupDocument)) {
      errors.push(
        `generated path setup document is unsafe: ${contract.setupDocument ?? "<unknown>"}`,
      );
      continue;
    }
    addPathChecks({
      errors,
      root,
      visible,
      path: contract.setupDocument,
      kind: "generated path setup document",
    });
    if (
      !Array.isArray(contract.setupCommand) ||
      contract.setupCommand.length === 0 ||
      contract.setupCommand.some((part) => typeof part !== "string" || part.trim() === "")
    ) {
      errors.push(`generated path contract has no executable setupCommand: ${contract.path}`);
      continue;
    }
    if (existsSync(join(root, contract.setupDocument))) {
      const setupContent = readFileSync(join(root, contract.setupDocument), "utf8");
      const command = contract.setupCommand.join(" ");
      if (!setupContent.includes(command)) {
        errors.push(
          `${contract.setupDocument} does not document generated path command: ${command}`,
        );
      }
      if (!setupContent.includes(contract.path)) {
        errors.push(`${contract.setupDocument} does not document generated path: ${contract.path}`);
      }
    }
  }

  const requiredExternalFields = [
    "repository",
    "role",
    "direction",
    "revisionSource",
    "allowedRoot",
  ];
  const validDirections = new Set(["canonical-contract-to-public-consumer", "public-to-consumer"]);
  const validProcessorKinds = new Set(["generator", "validator", "none"]);
  const externalSources = manifest.externalSources ?? [];
  if (externalSources.length === 0) {
    errors.push("knowledge manifest must declare external source contracts");
  }
  for (const source of externalSources) {
    for (const field of requiredExternalFields) {
      if (typeof source[field] !== "string" || source[field].trim() === "") {
        errors.push(`external source ${source.repository ?? "<unknown>"} is missing ${field}`);
      }
    }
    if (!validDirections.has(source.direction)) {
      errors.push(
        `external source ${source.repository ?? "<unknown>"} has invalid direction: ${source.direction}`,
      );
    }
    if (typeof source.allowedRoot === "string" && !isSafeRelativePath(source.allowedRoot)) {
      errors.push(
        `external source ${source.repository ?? "<unknown>"} has unsafe allowedRoot: ${source.allowedRoot}`,
      );
    }
    if (
      typeof source.processor !== "object" ||
      source.processor === null ||
      !validProcessorKinds.has(source.processor.kind)
    ) {
      errors.push(`external source ${source.repository ?? "<unknown>"} has invalid processor`);
    } else if (
      source.processor.kind !== "none" &&
      (typeof source.processor.entrypoint !== "string" || source.processor.entrypoint.trim() === "")
    ) {
      errors.push(
        `external source ${source.repository ?? "<unknown>"} is missing processor entrypoint`,
      );
    } else if (
      source.processor.kind !== "none" &&
      !isSafeRelativePath(source.processor.entrypoint)
    ) {
      errors.push(
        `external source ${source.repository ?? "<unknown>"} has unsafe processor entrypoint: ${source.processor.entrypoint}`,
      );
    }
    if (
      !Array.isArray(source.checkCommand) ||
      source.checkCommand.length === 0 ||
      source.checkCommand.some((part) => typeof part !== "string" || part.trim() === "")
    ) {
      errors.push(
        `external source ${source.repository ?? "<unknown>"} has no executable checkCommand`,
      );
    } else if (source.checkCommand.some((part) => /^(?:&&|\|\||[;|])$/u.test(part))) {
      errors.push(
        `external source ${source.repository ?? "<unknown>"} checkCommand contains shell control`,
      );
    }
  }

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
