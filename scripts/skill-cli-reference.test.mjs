import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join, normalize } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { cliReferenceFiles } from "./generate-agent-docs.mjs";
import { listFiles } from "./vendor-skills.mjs";
import { RELEASED_SKILLS_ROOT, releasedSkillNames } from "./released-skills.mjs";

// The MCP server serves each released skill file in one `resources/read`
// response, and Claude Code saves a response of more than 50,000 characters to
// a file. The CLI reference of the `fallow` skill is therefore split into one
// file per command under `references/cli/`, with `references/cli-reference.md`
// as the index.

const REPO_ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const SKILLS_ROOT = join(REPO_ROOT, ...RELEASED_SKILLS_ROOT);
const FALLOW_SKILL = join(SKILLS_ROOT, "fallow");
const MAX_SKILL_FILE_CHARS = 50_000;

const read = (path) => readFileSync(path, "utf8");

/** GitHub heading anchors of a Markdown text, outside code fences. */
const headingAnchors = (text) => {
  const anchors = new Set();
  let fence = false;
  for (const line of text.split("\n")) {
    if (line.startsWith("```")) {
      fence = !fence;
    }
    const heading = !fence && /^#{1,6} (.*)$/u.exec(line);
    if (heading) {
      anchors.add(
        heading[1]
          .trim()
          .toLowerCase()
          .replace(/[^\p{L}\p{N}\s_-]/gu, "")
          .replace(/\s/gu, "-"),
      );
    }
  }
  return anchors;
};

/** Relative Markdown links of a text, outside code fences. */
const relativeLinks = (text) => {
  const prose = text.replace(/^```[\s\S]*?^```/gmu, "");
  return [...prose.matchAll(/\]\(([^)\s]+)\)/gu)]
    .map(([, target]) => target)
    .filter((target) => !/^[a-z]+:/u.test(target));
};

test("every released skill file fits in one MCP response", () => {
  const tooLarge = releasedSkillNames(REPO_ROOT).flatMap((name) =>
    listFiles(join(SKILLS_ROOT, name))
      .map((file) => ({
        file: `${name}/${file}`,
        chars: [...read(join(SKILLS_ROOT, name, file))].length,
      }))
      .filter(({ chars }) => chars > MAX_SKILL_FILE_CHARS),
  );
  assert.deepEqual(tooLarge, []);
});

// The package check (`intent validate npm/fallow/skills` in CI) also needs a
// direct link from SKILL.md to each reference file.
test("SKILL.md and the CLI reference index link each per-command file", () => {
  const skill = read(join(FALLOW_SKILL, "SKILL.md"));
  const index = read(join(FALLOW_SKILL, "references", "cli-reference.md"));
  const perCommand = cliReferenceFiles(FALLOW_SKILL).filter(
    (file) => file !== "references/cli-reference.md",
  );
  assert.ok(perCommand.length > 0, "the fallow skill has per-command CLI reference files");
  for (const file of perCommand) {
    assert.ok(skill.includes(`](${file})`), `SKILL.md links ${file}`);
    const relative = file.slice("references/".length);
    assert.ok(index.includes(`](${relative})`), `cli-reference.md links ${relative}`);
  }
});

test("every relative link in the fallow skill resolves", () => {
  const broken = [];
  const markdown = listFiles(FALLOW_SKILL).filter((file) => file.endsWith(".md"));
  for (const file of markdown) {
    const path = join(FALLOW_SKILL, file);
    const text = read(path);
    for (const target of relativeLinks(text)) {
      const [targetFile, anchor] = target.split("#");
      const resolved = targetFile ? normalize(join(dirname(path), targetFile)) : path;
      if (!existsSync(resolved)) {
        broken.push(`${file}: ${target}`);
        continue;
      }
      if (anchor && resolved.endsWith(".md") && !headingAnchors(read(resolved)).has(anchor)) {
        broken.push(`${file}: ${target}`);
      }
    }
  }
  assert.deepEqual(broken, []);
});
