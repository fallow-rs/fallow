// Temporary git repositories for the tests of the ship helpers. Each fixture
// has a bare `origin` and a clone. The tests commit on `main` and on a
// feature branch through the clone and push both to `origin`.

import { execFileSync, spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";

const GIT_LOCATION_VARIABLES = new Set([
  "GIT_DIR",
  "GIT_WORK_TREE",
  "GIT_COMMON_DIR",
  "GIT_INDEX_FILE",
]);

// A private global config keeps the signing and rerere settings of the
// person who runs the tests out of the fixture.
const GLOBAL_CONFIG = `[user]
\tname = Ship Test
\temail = ship-test@example.invalid
[commit]
\tgpgsign = false
[init]
\tdefaultBranch = main
`;

/**
 * Create a fixture: `root` holds `origin.git` and the clone `work`. Returns
 * helpers that run git and node in the clone with an isolated environment.
 */
export const createShipRepo = (prefix) => {
  const root = realpathSync(mkdtempSync(join(tmpdir(), prefix)));
  const globalConfig = join(root, "gitconfig");
  writeFileSync(globalConfig, GLOBAL_CONFIG);
  const env = Object.fromEntries(
    Object.entries(process.env).filter(([name]) => !GIT_LOCATION_VARIABLES.has(name)),
  );
  // `TMPDIR` puts the temporary worktrees of the scripts in `root`, so the
  // cleanup removes them too.
  Object.assign(env, {
    GIT_CONFIG_GLOBAL: globalConfig,
    GIT_CONFIG_NOSYSTEM: "1",
    TMPDIR: root,
  });

  const origin = join(root, "origin.git");
  const work = join(root, "work");
  const git = (...args) =>
    execFileSync("git", args, { cwd: work, env, encoding: "utf8", stdio: "pipe" }).trim();
  execFileSync("git", ["init", "--quiet", "--bare", origin], { env });
  execFileSync("git", ["clone", "--quiet", origin, work], { env, stdio: "pipe" });

  const write = (path, text) => {
    mkdirSync(dirname(join(work, path)), { recursive: true });
    writeFileSync(join(work, path), text);
  };

  /** Write `files` and commit them on the current branch. */
  const commit = (message, files) => {
    for (const [path, text] of Object.entries(files)) {
      write(path, text);
    }
    git("add", "--all");
    git("commit", "--quiet", "-m", message);
    return git("rev-parse", "HEAD");
  };

  /**
   * Run a node script in the clone and return `{ status, output }`. The
   * variables in `extraEnv` are added to the isolated environment.
   */
  const runNode = (script, args, extraEnv = {}) => {
    const result = spawnSync(process.execPath, [script, ...args], {
      cwd: work,
      env: { ...env, ...extraEnv },
      encoding: "utf8",
    });
    return { status: result.status, output: `${result.stdout}${result.stderr}` };
  };

  const cleanup = () => rmSync(root, { recursive: true, force: true });
  return { root, work, env, git, write, commit, runNode, cleanup };
};
