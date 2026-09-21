import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync, mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const REPO_ROOT = fileURLToPath(new URL("..", import.meta.url));
const SCRIPT = "scripts/check_telemetry_doc_sync.py";
const CANONICAL = "docs/telemetry.md";

const run = (cwd, env = {}) =>
  spawnSync("python3", [SCRIPT], { cwd, encoding: "utf8", env: { ...process.env, ...env } });

const git = (cwd, args) => {
  const result = spawnSync(
    "git",
    ["-c", "core.excludesFile=/dev/null", "-c", "commit.gpgsign=false", ...args],
    {
      cwd,
      encoding: "utf8",
      env: {
        ...process.env,
        GIT_AUTHOR_EMAIL: "checks@example.invalid",
        GIT_AUTHOR_NAME: "Checks",
        GIT_COMMITTER_EMAIL: "checks@example.invalid",
        GIT_COMMITTER_NAME: "Checks",
      },
    },
  );
  assert.equal(result.status, 0, `git ${args.join(" ")}: ${result.stderr}`);
};

/** A throwaway checkout carrying just the script and the canonical document. */
const createCheckout = (root) => {
  const checkout = join(root, "checkout");
  for (const relativePath of [SCRIPT, CANONICAL]) {
    const destination = join(checkout, relativePath);
    mkdirSync(join(destination, ".."), { recursive: true });
    copyFileSync(join(REPO_ROOT, relativePath), destination);
  }
  return checkout;
};

test("telemetry documentation parity fails closed when companions are absent", () => {
  const root = mkdtempSync(join(tmpdir(), "fallow-telemetry-parity-"));
  const result = run(REPO_ROOT, {
    FALLOW_DOCS_DIR: join(root, "missing-docs"),
    FALLOW_SKILLS_DIR: join(root, "missing-skills"),
  });

  assert.equal(result.status, 1);
  assert.match(result.stderr, /expected companion doc not found/u);
});

test("telemetry documentation parity looks beside the main checkout, not beside a worktree", () => {
  const root = mkdtempSync(join(tmpdir(), "fallow-telemetry-worktree-"));
  const checkout = createCheckout(root);
  git(checkout, ["init", "--quiet"]);
  git(checkout, ["add", "-f", "--", "."]);
  git(checkout, ["commit", "--quiet", "-m", "checkout"]);
  const worktree = join(checkout, ".worktrees", "topic");
  git(checkout, ["worktree", "add", "--quiet", "--detach", worktree, "HEAD"]);

  // Neither directory holds a companion clone, so both runs stand down. What the
  // assertion carries is WHERE each run looked: a worktree must resolve the
  // companion beside the clone it belongs to, never beside itself.
  for (const cwd of [checkout, worktree]) {
    const result = run(cwd, { FALLOW_DOCS_DIR: "", FALLOW_SKILLS_DIR: "" });
    assert.equal(result.status, 0, result.stderr);
    assert.match(
      result.stdout,
      /skipped: no companion checkout at .*fallow-docs; set FALLOW_DOCS_DIR/u,
    );
    assert.match(
      result.stdout,
      /skipped: no companion checkout at .*fallow-skills; set FALLOW_SKILLS_DIR/u,
    );
    assert.doesNotMatch(result.stdout, /\.worktrees/u);
    assert.match(result.stdout, /parity was not checked/u);
  }
});
