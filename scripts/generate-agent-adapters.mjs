#!/usr/bin/env node
/**
 * Generate Claude skill and reviewer-agent adapters from the client-neutral
 * `.agents/skills` and `.agents/agents` source trees.
 *
 * Codex and other Agent Skills clients consume `.agents/skills` and
 * `.agents/agents` directly. Claude receives byte-stable generated copies
 * under `.claude/skills` and `.claude/agents`.
 */

import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { basename, dirname, join, relative, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const SKILL_GENERATED_MARKER = "<!-- Generated from .agents/skills. Do not edit. -->";
const AGENT_GENERATED_MARKER = "<!-- Generated from .agents/agents. Do not edit. -->";
const AGENT_TEMPLATE_NAME = "_template.md";

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

const renderAdapter = ({ body, frontmatter }, marker) =>
  `---\n${frontmatter}\n---\n${marker}\n${body}`;

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

const skillAdapterPath = (repoRoot, name) => join(repoRoot, ".claude", "skills", name, "SKILL.md");

const staleGeneratedSkillAdapters = (repoRoot, names) => {
  const root = join(repoRoot, ".claude", "skills");
  if (!existsSync(root)) {
    return [];
  }
  return readdirSync(root, { withFileTypes: true })
    .filter((entry) => entry.isDirectory() && !names.has(entry.name))
    .map((entry) => join(root, entry.name, "SKILL.md"))
    .filter(
      (path) => existsSync(path) && readFileSync(path, "utf8").includes(SKILL_GENERATED_MARKER),
    );
};

/// Files a skill ships alongside its SKILL.md, relative to the skill directory.
/// A skill whose SKILL.md links to `references/x.md` is broken in the adapter
/// tree unless those files travel with it.
const companionFiles = (skillDir, prefix = "") => {
  const root = join(skillDir, prefix);
  if (!existsSync(root)) {
    return [];
  }
  return readdirSync(root, { withFileTypes: true }).flatMap((entry) => {
    const relativePath = prefix ? `${prefix}/${entry.name}` : entry.name;
    if (entry.isDirectory()) {
      return companionFiles(skillDir, relativePath);
    }
    return relativePath === "SKILL.md" ? [] : [relativePath];
  });
};

const generateSkillAdapters = (repoRoot, check) => {
  const skills = canonicalSkills(repoRoot);
  const drifted = [];
  for (const skill of skills) {
    const destination = skillAdapterPath(repoRoot, skill.name);
    const expected = renderAdapter(skill, SKILL_GENERATED_MARKER);
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

    // A companion deleted from the source would otherwise linger in the
    // generated tree forever, since the loop above only visits source files.
    const expectedCompanions = new Set(companionFiles(sourceDir));
    for (const orphan of companionFiles(dirname(destination))) {
      if (expectedCompanions.has(orphan)) {
        continue;
      }
      const orphanPath = join(dirname(destination), orphan);
      drifted.push(relative(repoRoot, orphanPath));
      if (!check) {
        rmSync(orphanPath, { force: true });
      }
    }
  }

  const names = new Set(skills.map(({ name }) => name));
  for (const stalePath of staleGeneratedSkillAdapters(repoRoot, names)) {
    drifted.push(relative(repoRoot, stalePath));
    if (!check) {
      rmSync(dirname(stalePath), { recursive: true, force: true });
    }
  }
  return drifted;
};

const canonicalAgents = (repoRoot = REPO_ROOT) => {
  const sourceRoot = join(repoRoot, ".agents", "agents");
  if (!existsSync(sourceRoot)) {
    throw new Error(`missing canonical agent root: ${relative(repoRoot, sourceRoot)}`);
  }
  return readdirSync(sourceRoot, { withFileTypes: true })
    .filter(
      (entry) => entry.isFile() && entry.name.endsWith(".md") && entry.name !== AGENT_TEMPLATE_NAME,
    )
    .map((entry) => {
      const sourcePath = join(sourceRoot, entry.name);
      const expectedName = basename(entry.name, ".md");
      const source = readFileSync(sourcePath, "utf8");
      const parsed = parseFrontmatter(source, relative(repoRoot, sourcePath));
      if (parsed.name !== expectedName) {
        throw new Error(
          `${relative(repoRoot, sourcePath)}: name ${parsed.name} does not match file ${expectedName}`,
        );
      }
      return { ...parsed, sourcePath };
    })
    .toSorted((left, right) => left.name.localeCompare(right.name));
};

const agentAdapterPath = (repoRoot, name) => join(repoRoot, ".claude", "agents", `${name}.md`);

const staleGeneratedAgentAdapters = (repoRoot, names) => {
  const root = join(repoRoot, ".claude", "agents");
  if (!existsSync(root)) {
    return [];
  }
  return readdirSync(root, { withFileTypes: true })
    .filter((entry) => entry.isFile() && entry.name.endsWith(".md"))
    .filter((entry) => !names.has(basename(entry.name, ".md")))
    .map((entry) => join(root, entry.name))
    .filter((path) => readFileSync(path, "utf8").includes(AGENT_GENERATED_MARKER));
};

const generateAgentDefinitionAdapters = (repoRoot, check) => {
  const agents = canonicalAgents(repoRoot);
  const drifted = [];
  for (const agent of agents) {
    const destination = agentAdapterPath(repoRoot, agent.name);
    const expected = renderAdapter(agent, AGENT_GENERATED_MARKER);
    const current = existsSync(destination) ? readFileSync(destination, "utf8") : null;
    if (current !== expected) {
      drifted.push(relative(repoRoot, destination));
      if (!check) {
        mkdirSync(dirname(destination), { recursive: true });
        writeFileSync(destination, expected);
      }
    }
  }

  const names = new Set(agents.map(({ name }) => name));
  for (const stalePath of staleGeneratedAgentAdapters(repoRoot, names)) {
    drifted.push(relative(repoRoot, stalePath));
    if (!check) {
      rmSync(stalePath, { force: true });
    }
  }
  return drifted;
};

export const generateAgentAdapters = ({ check = false, repoRoot = REPO_ROOT } = {}) => {
  const drifted = [
    ...generateSkillAdapters(repoRoot, check),
    ...generateAgentDefinitionAdapters(repoRoot, check),
  ];
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
