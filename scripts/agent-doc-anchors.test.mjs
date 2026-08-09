import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const REPOSITORY_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");

/** Skill trees whose markdown ships to agents and to external linters. */
const SKILL_ROOTS = [".agents/skills", ".claude/skills", "npm/fallow/skills"];

const INLINE_LINK = /\[[^\]]*\]\((#[^)\s]+)\)/gu;
const HEADING = /^#{1,6}\s+(.*)$/u;

/** GitHub's heading slug: drop inline code fences and other punctuation, keep
 * word characters and hyphens, then join the remaining words with hyphens.
 * Markdown linters that flag MD051 resolve anchors the same way. */
const slugFor = (heading) =>
  heading
    .replaceAll("`", "")
    .toLowerCase()
    .replaceAll(/[^\w\s-]/gu, "")
    .trim()
    .replaceAll(/\s/gu, "-");

const markdownFilesUnder = (root) => {
  const absolute = join(REPOSITORY_ROOT, root);
  let entries;
  try {
    entries = readdirSync(absolute, { withFileTypes: true });
  } catch {
    return [];
  }
  return entries.flatMap((entry) => {
    const path = join(root, entry.name);
    if (entry.isDirectory()) return markdownFilesUnder(path);
    return entry.isFile() && entry.name.endsWith(".md") ? [path] : [];
  });
};

const brokenAnchorsIn = (path) => {
  const text = readFileSync(join(REPOSITORY_ROOT, path), "utf8");
  const lines = text.split("\n");

  const slugs = new Set();
  for (const line of lines) {
    const heading = HEADING.exec(line);
    if (heading) slugs.add(slugFor(heading[1].trim()));
  }

  const broken = [];
  for (const [index, line] of lines.entries()) {
    for (const [, anchor] of line.matchAll(INLINE_LINK)) {
      const slug = anchor.slice(1);
      if (!slugs.has(slug)) broken.push(`${path}:${index + 1}: ${anchor}`);
    }
  }
  return broken;
};

test("skill markdown carries no dangling in-document anchors", () => {
  const files = SKILL_ROOTS.flatMap(markdownFilesUnder);
  assert.ok(files.length > 0, "expected to find skill markdown to check");

  const broken = files.flatMap(brokenAnchorsIn);
  assert.deepEqual(
    broken,
    [],
    `these links point at headings that do not exist:\n  ${broken.join("\n  ")}`,
  );
});

test("slugFor matches the anchors GitHub generates", () => {
  assert.equal(slugFor("`schema`: Capability Manifest"), "schema-capability-manifest");
  assert.equal(slugFor("Global Flags"), "global-flags");
  // An ampersand is dropped rather than replaced, which silently doubles the
  // hyphen; headings should spell "and" instead.
  assert.equal(slugFor("Complexity & Health"), "complexity--health");
});
