#!/usr/bin/env node

import { execFileSync } from "node:child_process";
import { existsSync, lstatSync, readFileSync, realpathSync } from "node:fs";
import { dirname, isAbsolute, join, normalize, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const MANIFEST_PATH = "scripts/knowledge-surfaces.json";
const GENERATED_MARKER = "<!-- Generated from .agents/skills. Do not edit. -->";

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
    if (
      content.includes(`\`${prefix}`) ||
      content.includes(`](${prefix}`) ||
      content.includes(`@${prefix}`)
    ) {
      errors.push(`${path} references private path prefix: ${prefix}`);
    }
  }
};

export const validateKnowledgeArchitecture = ({
  root = ROOT,
  visibleFiles = trackedFiles(root),
} = {}) => {
  const errors = [];
  const visible = new Set(visibleFiles.map(toPosix));
  const manifest = readJson(root, MANIFEST_PATH);

  if (manifest.schemaVersion !== 1) {
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
    addTrustBoundaryChecks({
      content,
      errors,
      manifest,
      path: entryPoint.path,
    });
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
      if (!content.includes(route)) {
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
  const trackedDocs = visibleFiles
    .map(toPosix)
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

  for (const path of docPaths) {
    addPathChecks({ errors, root, visible, path, kind: "maintainer document" });
    if (!existsSync(join(root, path))) {
      continue;
    }
    const content = readFileSync(join(root, path), "utf8");
    addTrustBoundaryChecks({ content, errors, manifest, path });
    for (const target of localMarkdownTargets(path, content)) {
      if (!existsSync(join(root, target))) {
        errors.push(`${path} links to missing local path: ${target}`);
      }
    }
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
    const leaked = visibleFiles.find((path) => toPosix(path).startsWith(ignored.path));
    if (leaked !== undefined) {
      errors.push(`${ignored.path} must remain ignored, but Git can see ${leaked}`);
    }
  }

  const canonicalRoot = manifest.skills?.canonicalRoot;
  const canonical = skillNames(visibleFiles, canonicalRoot);
  if (canonical.length === 0) {
    errors.push(`canonical skill tree is empty: ${canonicalRoot}`);
  }
  for (const name of canonical) {
    addPathChecks({
      errors,
      root,
      visible,
      path: `${canonicalRoot}/${name}/SKILL.md`,
      kind: "canonical skill",
    });
  }

  for (const adapter of manifest.skills?.adapterRoots ?? []) {
    const adapters = skillNames(visibleFiles, adapter.path);
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
    }
  }

  addPathChecks({
    errors,
    root,
    visible,
    path: manifest.skills?.generator,
    kind: "agent adapter generator",
  });

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
