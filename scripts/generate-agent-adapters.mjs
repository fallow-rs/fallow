#!/usr/bin/env node
/**
 * Generate Claude skill adapters from the client-neutral `.agents/skills`
 * source tree.
 *
 * Codex and other Agent Skills clients consume `.agents/skills` directly.
 * Claude receives byte-stable generated copies under `.claude/skills`.
 */

import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const GENERATED_MARKER = "<!-- Generated from .agents/skills. Do not edit. -->";

const parseFrontmatter = (text, sourcePath) => {
  const match = text.match(/^---\n([\s\S]*?)\n---\n([\s\S]*)$/);
  if (!match) {
    throw new Error(`${sourcePath}: missing YAML frontmatter`);
  }
  const nameMatch = match[1].match(/^name:\s*([a-z0-9-]+)\s*$/m);
  if (!nameMatch) {
    throw new Error(`${sourcePath}: missing valid name`);
  }
  return { body: match[2], frontmatter: match[1], name: nameMatch[1] };
};

const canonicalSkills = (repoRoot = REPO_ROOT) => {
  const sourceRoot = join(repoRoot, ".agents", "skills");
  if (!existsSync(sourceRoot)) {
    throw new Error(`missing canonical skill root: ${relative(repoRoot, sourceRoot)}`);
  }
  return readdirSync(sourceRoot, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => {
      const sourcePath = join(sourceRoot, entry.name, "SKILL.md");
      if (!existsSync(sourcePath)) {
        throw new Error(`missing canonical skill: ${relative(repoRoot, sourcePath)}`);
      }
      const source = readFileSync(sourcePath, "utf8");
      const parsed = parseFrontmatter(source, relative(repoRoot, sourcePath));
      if (parsed.name !== entry.name) {
        throw new Error(
          `${relative(repoRoot, sourcePath)}: name ${parsed.name} does not match directory ${entry.name}`,
        );
      }
      return { ...parsed, sourcePath };
    })
    .toSorted((left, right) => left.name.localeCompare(right.name));
};

const renderAdapter = ({ body, frontmatter }) =>
  `---\n${frontmatter}\n---\n${GENERATED_MARKER}\n${body}`;

const adapterPath = (repoRoot, name) => join(repoRoot, ".claude", "skills", name, "SKILL.md");

const staleGeneratedAdapters = (repoRoot, names) => {
  const root = join(repoRoot, ".claude", "skills");
  if (!existsSync(root)) {
    return [];
  }
  return readdirSync(root, { withFileTypes: true })
    .filter((entry) => entry.isDirectory() && !names.has(entry.name))
    .map((entry) => join(root, entry.name, "SKILL.md"))
    .filter((path) => existsSync(path) && readFileSync(path, "utf8").includes(GENERATED_MARKER));
};

/// Files a skill ships alongside its SKILL.md, relative to the skill directory.
/// A skill whose SKILL.md links to `references/x.md` is broken in the adapter
/// tree unless those files travel with it.
const companionFiles = (skillDir, prefix = "") =>
  readdirSync(join(skillDir, prefix), { withFileTypes: true }).flatMap((entry) => {
    const relativePath = prefix ? `${prefix}/${entry.name}` : entry.name;
    if (entry.isDirectory()) {
      return companionFiles(skillDir, relativePath);
    }
    return relativePath === "SKILL.md" ? [] : [relativePath];
  });

export const generateAgentAdapters = ({ check = false, repoRoot = REPO_ROOT } = {}) => {
  const skills = canonicalSkills(repoRoot);
  const drifted = [];
  for (const skill of skills) {
    const destination = adapterPath(repoRoot, skill.name);
    const expected = renderAdapter(skill);
    const current = existsSync(destination) ? readFileSync(destination, "utf8") : null;
    if (current !== expected) {
      drifted.push(relative(repoRoot, destination));
      if (!check) {
        mkdirSync(dirname(destination), { recursive: true });
        writeFileSync(destination, expected);
      }
    }

    const sourceDir = dirname(skill.sourcePath);
    for (const companion of companionFiles(sourceDir)) {
      const companionSource = readFileSync(join(sourceDir, companion), "utf8");
      const companionDestination = join(dirname(destination), companion);
      const companionCurrent = existsSync(companionDestination)
        ? readFileSync(companionDestination, "utf8")
        : null;
      if (companionCurrent === companionSource) {
        continue;
      }
      drifted.push(relative(repoRoot, companionDestination));
      if (!check) {
        mkdirSync(dirname(companionDestination), { recursive: true });
        writeFileSync(companionDestination, companionSource);
      }
    }
  }

  const names = new Set(skills.map(({ name }) => name));
  for (const stalePath of staleGeneratedAdapters(repoRoot, names)) {
    drifted.push(relative(repoRoot, stalePath));
    if (!check) {
      rmSync(dirname(stalePath), { recursive: true, force: true });
    }
  }
  return drifted.toSorted();
};

const main = (argv = process.argv.slice(2)) => {
  const unknown = argv.filter((arg) => arg !== "--check");
  if (unknown.length > 0) {
    throw new Error(`unknown argument: ${unknown[0]}`);
  }
  const check = argv.includes("--check");
  const drifted = generateAgentAdapters({ check });
  for (const path of drifted) {
    console.log(`${check ? "stale" : "generated"}: ${path}`);
  }
  return check && drifted.length > 0 ? 1 : 0;
};

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    process.exitCode = main();
  } catch (error) {
    console.error(`generate-agent-adapters: ${error.message}`);
    process.exitCode = 1;
  }
}
