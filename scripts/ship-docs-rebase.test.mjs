import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { resolve } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { createShipRepo } from "./ship-test-repo.mjs";

const SCRIPT = fileURLToPath(new URL("./ship-docs-rebase.mjs", import.meta.url));

// A small stand-in for the manifest generator of fallow-rs/docs: one line
// per `.mdx` file with its size, in path order. The real generator fails
// on a file with an unknown extension in a content directory. This
// stand-in fails on a `.DS_Store` or a `.unsupported` file at the root.
const GENERATOR = `import { readFileSync, readdirSync, writeFileSync } from "node:fs";
const unsupported = readdirSync(".").find((name) => name === ".DS_Store" || name.endsWith(".unsupported"));
if (unsupported !== undefined) {
  throw new Error("Unsupported file type: " + unsupported);
}
const files = readdirSync(".").filter((name) => name.endsWith(".mdx")).sort();
const text = JSON.stringify(files.map((name) => [name, readFileSync(name).length]), null, 2) + "\\n";
if (process.argv[2] === "--write") {
  writeFileSync("public-content-manifest.json", text);
} else if (readFileSync("public-content-manifest.json", "utf8") !== text) {
  console.error("public-content-manifest.json is stale.");
  process.exit(1);
}
`;

const PACKAGE = JSON.stringify({
  name: "docs-fixture",
  private: true,
  scripts: { test: "node --version" },
});

const manifest = (...pages) =>
  `${JSON.stringify(
    pages.toSorted().map((name) => [name, `${name} text\n`.length]),
    null,
    2,
  )}\n`;

// Add pages and a manifest that lists every page of the current tree.
const addPages = (repo, message, pages, listed) =>
  repo.commit(message, {
    ...Object.fromEntries(pages.map((name) => [name, `${name} text\n`])),
    "public-content-manifest.json": manifest(...listed),
  });

/** The full hash of the rebase result that the script printed. */
const newTipOf = (result) => {
  const match = result.output.match(/new tip ([0-9a-f]{40})\./u);
  assert.notEqual(match, null, result.output);
  return match[1];
};

/** The path of the temporary worktree that the script kept. */
const keptWorktreeOf = (result) => {
  const match = result.output.match(/The temporary worktree (\S+) holds/u);
  assert.notEqual(match, null, result.output);
  return match[1];
};

const rebaseInProgressIn = (repo, dir) =>
  existsSync(resolve(dir, repo.git("-C", dir, "rev-parse", "--git-path", "rebase-merge")));

const docsRepo = (t, { onMain, onBranch }) => {
  const repo = createShipRepo("ship-docs-rebase-");
  t.after(repo.cleanup);
  repo.commit("chore: start", {
    "scripts/public-content.mjs": GENERATOR,
    "package.json": PACKAGE,
    ".gitignore": ".DS_Store\n",
    "index.mdx": "index.mdx text\n",
    "public-content-manifest.json": manifest("index.mdx"),
  });
  repo.git("push", "--quiet", "origin", "main");
  repo.git("switch", "--quiet", "-c", "feat");
  onBranch(repo);
  repo.git("push", "--quiet", "origin", "feat");
  repo.git("switch", "--quiet", "main");
  onMain(repo);
  repo.git("push", "--quiet", "origin", "main");
  return repo;
};

test("each commit that conflicts on the manifest gets a regenerated manifest", (t) => {
  const repo = docsRepo(t, {
    onMain: (r) => addPages(r, "docs: main page", ["b.mdx"], ["index.mdx", "b.mdx"]),
    onBranch: (r) => {
      addPages(r, "docs: first page", ["c.mdx"], ["index.mdx", "c.mdx"]);
      addPages(r, "docs: second page", ["d.mdx"], ["index.mdx", "c.mdx", "d.mdx"]);
    },
  });

  const result = repo.runNode(SCRIPT, ["--branch", "feat", "--push"]);

  assert.equal(result.status, 0, result.output);
  assert.match(result.output, /Rebased feat onto origin\/main: 2 commits/u);
  assert.match(
    result.output,
    /Checks passed: node scripts\/public-content\.mjs --check, npm test\./u,
  );
  const tip = newTipOf(result);
  assert.equal(
    repo.git("show", `${tip}~1:public-content-manifest.json`),
    manifest("index.mdx", "b.mdx", "c.mdx").trimEnd(),
  );
  assert.equal(
    repo.git("show", `${tip}:public-content-manifest.json`),
    manifest("index.mdx", "b.mdx", "c.mdx", "d.mdx").trimEnd(),
  );
  repo.git("fetch", "--quiet", "origin");
  assert.equal(repo.git("rev-parse", "origin/feat"), tip);
});

test("a commit that only changed the manifest is skipped when it becomes empty", (t) => {
  const repo = docsRepo(t, {
    onMain: (r) => addPages(r, "docs: main page", ["b.mdx"], ["index.mdx", "b.mdx"]),
    onBranch: (r) =>
      r.commit("chore: edit the manifest", {
        "public-content-manifest.json": manifest("index.mdx", "a.mdx"),
      }),
  });

  const result = repo.runNode(SCRIPT, ["--branch", "feat"]);

  assert.equal(result.status, 0, result.output);
  assert.match(result.output, /Skipped 1 commits that became empty/u);
  assert.equal(newTipOf(result), repo.git("rev-parse", "origin/main"));
});

test("a conflict outside the manifest stops the rebase", (t) => {
  const repo = docsRepo(t, {
    onMain: (r) => r.commit("docs: main edit", { "index.mdx": "main text\n" }),
    onBranch: (r) => r.commit("docs: branch edit", { "index.mdx": "branch text\n" }),
  });

  const result = repo.runNode(SCRIPT, ["--branch", "feat"]);

  assert.equal(result.status, 1, result.output);
  assert.match(
    result.output,
    /STOP: feat: Conflicts outside public-content-manifest\.json: index\.mdx/u,
  );
  assert.equal(repo.git("branch", "--show-current"), "main");
  assert.equal(rebaseInProgressIn(repo, repo.work), false);
  assert.equal(rebaseInProgressIn(repo, keptWorktreeOf(result)), true);
});

test("the tree check finds a page change that the branch made in a merge commit", (t) => {
  const repo = docsRepo(t, {
    onMain: (r) => addPages(r, "docs: main page", ["b.mdx"], ["index.mdx", "b.mdx"]),
    onBranch: (r) => addPages(r, "docs: branch page", ["c.mdx"], ["index.mdx", "c.mdx"]),
  });
  repo.git("switch", "--quiet", "feat");
  assert.throws(() => repo.git("merge", "--quiet", "--no-edit", "main"));
  repo.write("index.mdx", "index.mdx text, edited in the merge\n");
  repo.runNode("scripts/public-content.mjs", ["--write"]);
  repo.git("add", "--all");
  repo.git("commit", "--quiet", "--no-edit");
  repo.git("push", "--quiet", "origin", "feat");
  repo.git("switch", "--quiet", "main");

  const result = repo.runNode(SCRIPT, ["--branch", "feat", "--push"]);

  assert.equal(result.status, 1, result.output);
  assert.match(
    result.output,
    /Paths where the rebase result differs from a merge of the branch into origin\/main:\n\s+index\.mdx\n/u,
  );
  assert.match(result.output, /The branch has 1 merge commits\./u);
  assert.doesNotMatch(result.output, /Pushed/u);
});

test("a failed manifest check after the rebase skips the push", (t) => {
  // The commit on main changes a page and leaves the manifest stale.
  const repo = docsRepo(t, {
    onMain: (r) => r.commit("docs: main edit", { "index.mdx": "a longer main text\n" }),
    onBranch: (r) => r.commit("docs: branch page", { "c.mdx": "c.mdx text\n" }),
  });

  const result = repo.runNode(SCRIPT, ["--branch", "feat", "--push"]);

  assert.equal(result.status, 1, result.output);
  assert.match(
    result.output,
    /CHECK FAILED: feat\n\s+node scripts\/public-content\.mjs --check \(exit 1\)/u,
  );
  assert.doesNotMatch(result.output, /Pushed/u);
});

test("the script refuses a checkout without the manifest generator", (t) => {
  const repo = createShipRepo("ship-docs-rebase-other-");
  t.after(repo.cleanup);
  repo.commit("chore: start", { "README.md": "other\n" });

  const result = repo.runNode(SCRIPT, ["--branch", "feat"]);

  assert.equal(result.status, 2, result.output);
  assert.match(result.output, /Run this script in a fallow-rs\/docs checkout/u);
});

test("untracked and ignored files of the caller checkout stay out of the manifest", (t) => {
  // The manifest generator reads the directory. The rebase runs in a fresh
  // worktree, so a draft page or a `.DS_Store` of the caller does not reach
  // it.
  const repo = docsRepo(t, {
    onMain: (r) => addPages(r, "docs: main page", ["b.mdx"], ["index.mdx", "b.mdx"]),
    onBranch: (r) => addPages(r, "docs: branch page", ["c.mdx"], ["index.mdx", "c.mdx"]),
  });
  repo.write("draft.mdx", "draft.mdx text\n");
  repo.write(".DS_Store", "");

  const result = repo.runNode(SCRIPT, ["--branch", "feat", "--push"]);

  assert.equal(result.status, 0, result.output);
  const tip = newTipOf(result);
  assert.equal(
    repo.git("show", `${tip}:public-content-manifest.json`),
    manifest("index.mdx", "b.mdx", "c.mdx").trimEnd(),
  );
  repo.git("fetch", "--quiet", "origin");
  assert.equal(repo.git("rev-parse", "origin/feat"), tip);
  assert.equal(repo.git("status", "--porcelain", "--untracked-files=all"), "?? draft.mdx");
});

test("a failed manifest generator stops the rebase with the normal stop report", (t) => {
  const repo = docsRepo(t, {
    onMain: (r) => addPages(r, "docs: main page", ["b.mdx"], ["index.mdx", "b.mdx"]),
    onBranch: (r) =>
      r.commit("docs: branch page", {
        "c.mdx": "c.mdx text\n",
        "notes.unsupported": "notes\n",
        "public-content-manifest.json": manifest("index.mdx", "c.mdx"),
      }),
  });

  const result = repo.runNode(SCRIPT, ["--branch", "feat"]);

  assert.equal(result.status, 1, result.output);
  assert.match(
    result.output,
    /STOP: feat: node scripts\/public-content\.mjs --write failed[^]*Unsupported file type: notes\.unsupported/u,
  );
  assert.match(result.output, /The rebase is still in progress\./u);
  assert.equal(rebaseInProgressIn(repo, keptWorktreeOf(result)), true);
  assert.equal(rebaseInProgressIn(repo, repo.work), false);
});
