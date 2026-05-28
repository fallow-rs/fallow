# Fallow: Common Workflow Patterns & Recipes

Step-by-step workflows for common fallow usage scenarios.

---

## Table of Contents

- [Full Project Audit](#full-project-audit)
- [PR Dead Code Check](#pr-dead-code-check)
- [CI Pipeline Setup](#ci-pipeline-setup)
- [Incremental Adoption with Baselines](#incremental-adoption-with-baselines)
- [Monorepo Analysis](#monorepo-analysis)
- [Duplication Threshold CI Gate](#duplication-threshold-ci-gate)
- [Migration from knip](#migration-from-knip)
- [Migration from jscpd](#migration-from-jscpd)
- [Safe Auto-Fix Workflow](#safe-auto-fix-workflow)
- [Production vs Full Audit](#production-vs-full-audit)
- [Debugging False Positives](#debugging-false-positives)
- [Combined Dead Code + Duplication](#combined-dead-code--duplication)
- [Custom Plugin Setup](#custom-plugin-setup)
- [GitHub Code Scanning Integration](#github-code-scanning-integration)
- [Guard `git push` with a Claude Code PreToolUse hook](#guard-git-push-with-a-claude-code-pretooluse-hook)

---

## Full Project Audit

Complete codebase hygiene audit.

### Step 1: Run full analysis

```bash
fallow dead-code --format json --quiet
```

### Step 2: Review issue counts

Parse `total_issues` and individual arrays (`unused_files`, `unused_exports`, etc.) to understand the scope.

### Step 3: Find duplication

```bash
fallow dupes --format json --quiet
```

### Step 4: Preview auto-fix

```bash
fallow fix --dry-run --format json --quiet
```

### Step 5: Apply fixes (after user confirmation)

```bash
fallow fix --yes --format json --quiet
```

### Step 6: Verify

```bash
fallow dead-code --format json --quiet
```

---

## PR Dead Code Check

Check if a pull request introduces new dead code.

### Step 1: Analyze changed files

```bash
fallow dead-code --format json --quiet --changed-since main --fail-on-issues
```

Exit code 1 if the PR introduces new dead code. Exit code 0 if clean.

### Step 2: If issues found, show specifics

```bash
fallow dead-code --format json --quiet --changed-since main
```

Parse the JSON to list specific files and exports that became unused.

---

## CI Pipeline Setup

### GitHub Actions: Basic

```yaml
- name: Dead code check
  run: npx fallow dead-code --fail-on-issues --quiet
```

### GitHub Actions: With SARIF Upload

```yaml
- name: Fallow analysis
  run: npx fallow dead-code --ci > fallow.sarif
  continue-on-error: true  # --ci sets --fail-on-issues; continue to upload SARIF even if issues found

- name: Upload SARIF
  uses: github/codeql-action/upload-sarif@v3
  with:
    sarif_file: fallow.sarif
```

### GitHub Actions: Using the Official Action

```yaml
- uses: fallow-rs/fallow@v2
  with:
    command: dead-code
    fail-on-issues: true
    changed-since: main
```

### GitHub Actions: With Health Score

```yaml
- uses: fallow-rs/fallow@v2
  with:
    score: true
    changed-since: main
```

Computes a health score (0-100 with letter grade) in combined mode and enables the health delta header in PR comments.

### GitHub Actions: Severity-Aware PR Quality Gate (Audit)

```yaml
- uses: fallow-rs/fallow@v2
  with:
    command: audit
    gate: new-only        # default; fails only on findings introduced by this PR
    fail-on-issues: true
```

Runs `fallow audit` to combine dead-code + complexity + duplication scoped to changed files. The gate respects rule severity from `.fallowrc.json`, so `unused-exports: warn` projects do not fail when a PR touches a file with pre-existing warn-tier findings. Use `gate: all` to fail on every finding in changed files.

The action exposes `outputs.verdict` (`pass`/`warn`/`fail`) and `outputs.gate` for downstream conditionals; `outputs.issues` holds the introduced count under `gate: new-only` and the total count under `gate: all`.

```yaml
- uses: fallow-rs/fallow@v2
  id: fallow
  with:
    command: audit

- name: Block release on regression
  if: steps.fallow.outputs.verdict == 'fail'
  run: exit 1
```

Three additional outputs surface silent failures in the action's PR comment / review steps. `outputs.changed-files-unavailable` (`true`/`false`, default `false`) signals that the analyze step could not enumerate PR-changed files (transient GitHub API failure, expired token, missing permissions), so analysis ran against the full codebase. `outputs.post-skipped-reason` (`none`/`pagination_failure`) signals the Post review comments step aborted to avoid duplicate threads. `outputs.dedup-lookup-failed` (`true`/`false`) signals a dedup lookup failed on either the Post PR comment or Post review comments step. All three are always emitted regardless of which failure path was taken, so downstream `if:` gates can match positively without absent-vs-false ambiguity. Gate on these to detect degraded posting state and re-run the action.

### GitHub Actions: Inline PR Annotations (No Advanced Security)

The official action supports inline PR annotations via GitHub workflow commands. This does not require Advanced Security (unlike SARIF upload) and works on any GitHub plan.

```yaml
- uses: fallow-rs/fallow@v2
  with:
    command: dead-code
    changed-since: main
    annotations: true
    max-annotations: 50   # default: 50, limits annotation count
```

Annotations appear as inline warnings on the PR diff. They work with all commands (`dead-code`, `dupes`, `health`, and the default combined mode). The `max-annotations` input prevents annotation flooding on large projects.

### GitHub Actions: PR-Scoped Check

```yaml
- name: Check for new dead code
  run: npx fallow dead-code --format json --quiet --changed-since ${{ github.event.pull_request.base.sha }} --fail-on-issues
```

### GitHub Actions: Duplication Gate

```yaml
- name: Duplication check
  run: npx fallow dupes --format json --quiet --threshold 5 --mode mild
```

Fails if overall duplication exceeds 5%.

### GitHub Actions: PR-Scoped Duplication Check

```yaml
- name: Check duplication in changed files
  run: npx fallow dupes --format json --quiet --changed-since ${{ github.event.pull_request.base.sha }}
```

Only reports duplication in files modified by the PR.

### GitLab CI: Using the Official Template

```yaml
include:
  - remote: 'https://raw.githubusercontent.com/fallow-rs/fallow/main/ci/gitlab-ci.yml'

fallow:
  extends: .fallow
  variables:
    FALLOW_COMMAND: "dead-code"
    FALLOW_FAIL_ON_ISSUES: "true"
```

Generates Code Quality reports (inline MR annotations) automatically. In MR pipelines, `--changed-since` is automatically set to the target branch — no manual configuration needed.

If runners cannot reach `raw.githubusercontent.com`, run `fallow ci-template gitlab --vendor`, commit the generated `ci/` and `action/` files, and use GitLab's local include syntax:

```yaml
include:
  - local: 'ci/gitlab-ci.yml'
```

### GitLab CI: With MR Summary Comments

```yaml
include:
  - remote: 'https://raw.githubusercontent.com/fallow-rs/fallow/main/ci/gitlab-ci.yml'

fallow:
  extends: .fallow
  variables:
    FALLOW_COMMENT: "true"
    FALLOW_SUMMARY_SCOPE: "diff"
```

Posts a summary comment on the MR with issue counts and findings. In MR pipelines, `--changed-since` is auto-detected from `$CI_MERGE_REQUEST_TARGET_BRANCH_NAME`, so only issues from changed files are reported. `FALLOW_SUMMARY_SCOPE: "diff"` also hides project-level dependency/catalog/override findings whose anchor line is outside the diff. Requires `GITLAB_TOKEN` CI/CD variable (project access token with `api` scope); `CI_JOB_TOKEN` is read-only for MR notes in the official GitLab API.

### GitLab CI: With Inline Code Review Comments

```yaml
include:
  - remote: 'https://raw.githubusercontent.com/fallow-rs/fallow/main/ci/gitlab-ci.yml'

fallow:
  extends: .fallow
  variables:
    FALLOW_REVIEW: "true"
    FALLOW_REVIEW_GUIDANCE: "true"
```

Posts inline review comments directly on the MR diff lines where issues were found. `FALLOW_REVIEW_GUIDANCE: "true"` adds collapsed "What to do" guidance blocks to each inline finding. This gives developers precise feedback without leaving the code review flow. Can be combined with `FALLOW_COMMENT: "true"` for both a summary and inline comments. Requires `GITLAB_TOKEN`.

### GitLab CI: Combined MR Comments + Review

```yaml
include:
  - remote: 'https://raw.githubusercontent.com/fallow-rs/fallow/main/ci/gitlab-ci.yml'

fallow:
  extends: .fallow
  variables:
    FALLOW_COMMENT: "true"
    FALLOW_SUMMARY_SCOPE: "diff"
    FALLOW_REVIEW: "true"
    FALLOW_REVIEW_GUIDANCE: "true"
    FALLOW_FAIL_ON_ISSUES: "true"
```

Posts both a summary comment and inline review comments on the MR. `FALLOW_SUMMARY_SCOPE: "diff"` only affects the sticky summary; inline review comments remain anchored to diff lines. The template auto-detects the package manager (npm/pnpm/yarn) from lockfiles, so review comments show the correct commands for the project (e.g., `pnpm remove` instead of `npm uninstall`).

### GitLab CI: With Health Score and Trend

```yaml
include:
  - remote: 'https://raw.githubusercontent.com/fallow-rs/fallow/main/ci/gitlab-ci.yml'

fallow:
  extends: .fallow
  variables:
    FALLOW_SCORE: "true"
    FALLOW_TREND: "true"
    FALLOW_COMMENT: "true"
```

Computes the health score and compares against saved snapshots. The MR comment includes a health delta header showing score changes. `FALLOW_TREND` implies `FALLOW_SCORE`.

### GitLab CI: Manual (Without Template)

```yaml
fallow:
  image: node:20-slim
  script:
    - npx fallow dead-code --fail-on-issues --quiet --format json > fallow-results.json
  artifacts:
    paths:
      - fallow-results.json
```

---

## Incremental Adoption with Baselines

For large projects with existing dead code. Adopt gradually without fixing everything at once.

### Step 1: Save current state as baseline

```bash
fallow dead-code --format json --quiet --save-baseline fallow-baselines/dead-code.json
```

### Step 2: Commit the baseline

```bash
git add fallow-baselines/dead-code.json
git commit -m "chore: add fallow baseline"
```

### Step 3: CI only fails on NEW issues

```bash
fallow dead-code --format json --quiet --baseline fallow-baselines/dead-code.json --fail-on-issues
```

### Step 4: Gradually fix and update baseline

As you fix existing issues, regenerate the baseline:

```bash
fallow dead-code --format json --quiet --save-baseline fallow-baselines/dead-code.json
```

### Duplication baseline

Same pattern works for duplication:

```bash
fallow dupes --format json --quiet --save-baseline fallow-baselines/dupes.json
fallow dupes --format json --quiet --baseline fallow-baselines/dupes.json --threshold 5
```

---

## Monorepo Analysis

### Analyze the full monorepo

```bash
fallow dead-code --format json --quiet
```

Fallow auto-detects workspaces from `package.json` workspaces or `pnpm-workspace.yaml`.

### Analyze a single package

```bash
fallow dead-code --format json --quiet --workspace my-package
```

Full cross-workspace graph is built (so imports between packages are resolved), but only issues in `my-package` are reported.

### Per-package CI

Run analysis for each workspace package separately:

```bash
fallow dead-code --format json --quiet --workspace package-a --fail-on-issues
fallow dead-code --format json --quiet --workspace package-b --fail-on-issues
```

### List all discovered files across workspaces

```bash
fallow list --files --format json --quiet
```

---

## Duplication Threshold CI Gate

Enforce a maximum duplication percentage.

### Step 1: Measure current duplication

```bash
fallow dupes --format json --quiet
```

Check `duplication_percentage` in the JSON output.

### Step 2: Set threshold slightly above current

If current duplication is 3.8%, set threshold to 5%:

```bash
fallow dupes --format json --quiet --threshold 5
```

Exits with code 1 if duplication exceeds 5%.

### Step 3: Tighten over time

As you reduce duplication, lower the threshold.

### Cross-directory only

To ignore duplication within the same directory (local helpers, similar test files):

```bash
fallow dupes --format json --quiet --threshold 5 --skip-local
```

---

## Migration from knip

### Step 1: Preview migration

```bash
fallow migrate --dry-run
```

Shows what config would be generated. Auto-detects `knip.json`, `.knip.json`, `knip.jsonc`, `.knip.jsonc`, or `package.json#knip`.

### Step 2: Apply migration

```bash
fallow migrate
```

Creates `.fallowrc.json` with mapped settings:
- knip `rules`/`exclude`/`include` → fallow `rules` (error/warn/off)
- knip `ignore` → fallow `ignorePatterns`
- knip `ignoreDependencies` → fallow `ignoreDependencies`
- knip `ignoreExportsUsedInFile` → fallow `ignoreExportsUsedInFile` (boolean and `{ type, interface }` object form both supported; fallow groups type aliases and interfaces under one issue, so the two type-kind fields behave identically)
- Unmappable fields generate warnings with suggestions

### Step 3: Compare results

```bash
# Run fallow
fallow dead-code --format json --quiet

# Compare with knip output
npx knip --reporter json
```

### Step 4: Remove knip config

Once satisfied, remove the old `knip.json` and uninstall knip.

---

## Migration from jscpd

### Step 1: Preview migration

```bash
fallow migrate --dry-run
```

Auto-detects `.jscpd.json` or `package.json#jscpd`.

### Step 2: Apply migration

```bash
fallow migrate
```

Maps jscpd settings:
- `minTokens` → `duplicates.minTokens`
- `minLines` → `duplicates.minLines`
- `threshold` → `duplicates.threshold`
- `mode` → `duplicates.mode`

### Step 3: Compare results

```bash
fallow dupes --format json --quiet
```

### Detection mode mapping

| jscpd | fallow |
|-------|--------|
| Default (exact tokens) | `strict` |
| — | `mild` (fallow default, syntax normalized) |
| — | `weak` (literal normalization) |
| — | `semantic` (variable rename detection) |

---

## Safe Auto-Fix Workflow

### Step 1: Dry-run first

```bash
fallow fix --dry-run --format json --quiet
```

### Step 2: Review each proposed change

Parse the JSON `changes` array. Each entry shows:
- `path`: file to be modified
- `action`: what will happen (`remove_export`, `remove_dependency`)
- `name`: the symbol or dependency being removed
- `line`: the line number

### Step 3: Confirm with user before applying

Show the proposed changes. Wait for user confirmation.

### Step 4: Apply

```bash
fallow fix --yes --format json --quiet
```

### Step 5: Verify

```bash
fallow dead-code --format json --quiet
```

### Step 6: Run project tests

After auto-fix, always run the project's test suite to verify nothing broke.

---

## Production vs Full Audit

### Full audit (default)

```bash
fallow dead-code --format json --quiet
```

Includes all files, all scripts, all dependencies (including devDependencies).

### Production audit

```bash
fallow dead-code --format json --quiet --production
```

Differences:
- Excludes: `*.test.*`, `*.spec.*`, `*.stories.*`, `__tests__/**`, `__mocks__/**`
- Only analyzes: `start`, `build`, `serve`, `preview`, `prepare` scripts
- Skips: unused devDependency detection
- Adds: type-only production dependency detection

Use production mode for:
- Checking what ships to users
- Finding dependencies that should be devDependencies
- CI pipelines focused on production bundle

Use full mode for:
- Complete codebase hygiene
- Finding unused test utilities
- Auditing devDependency usage

---

## Debugging False Positives

### Trace an export's usage chain

```bash
fallow dead-code --format json --quiet --trace src/utils.ts:myFunction
```

Shows where `myFunction` is imported (or not imported) and why it's flagged.

### Trace all edges for a file

```bash
fallow dead-code --format json --quiet --trace-file src/utils.ts
```

Shows all imports/exports for the file and their resolution status.

### Trace a dependency

```bash
fallow dead-code --format json --quiet --trace-dependency lodash
```

Shows all files that import lodash.

### If the trace shows it IS used

The export might be consumed through a pattern fallow can't resolve (fully dynamic import, reflection). Add a suppression:

```typescript
// fallow-ignore-next-line unused-export
export const dynamicallyUsed = createHandler();
```

### If the trace shows it's NOT used

The export is genuinely unused. Consider removing it or marking it as intentionally kept:

```typescript
// fallow-ignore-next-line unused-export
export const publicApi = createWidget();  // Used by external consumers
```

---

## Combined Dead Code + Duplication

Cross-reference dead code with duplication findings to find high-priority cleanup targets.

### Step 1: Run combined analysis

```bash
fallow dead-code --format json --quiet --include-dupes
```

This adds duplication context to dead code findings, identifying clone instances that exist in unused files or overlap with unused exports.

### Step 2: Prioritize cleanup

Focus on findings that are BOTH dead code and duplicated:
- Unused files containing duplicate code → delete the file entirely
- Unused exports that are clones of other exports → remove the duplicate

---

## Custom Plugin Setup

For frameworks not covered by the 114 built-in plugins.

### Option 1: Inline framework config

```jsonc
// .fallowrc.json
{
  "framework": [
    {
      "name": "my-framework",
      "enablers": ["my-framework"],
      "entryPoints": ["src/routes/**/*.ts", "src/middleware/**/*.ts"]
    }
  ]
}
```

### Option 2: External plugin file

Create `.fallow/plugins/my-framework.jsonc`:

```jsonc
{
  "name": "my-framework",
  "detection": { "dependency": "my-framework" },
  "entryPoints": ["src/routes/**/*.ts"],
  "alwaysUsedFiles": ["src/bootstrap.ts"],
  "usedExports": {
    "src/config.ts": ["default"]
  },
  "toolingDependencies": ["my-framework-cli"]
}
```

### Option 3: Plugin directory

```jsonc
// .fallowrc.json
{
  "plugins": ["tools/plugins/"]
}
```

Place `.jsonc`, `.json`, or `.toml` plugin files in that directory.

---

## GitHub Code Scanning Integration

Upload fallow results to GitHub's Code Scanning dashboard.

### Step 1: Generate SARIF output

```bash
fallow dead-code --format sarif --quiet > fallow.sarif
```

### Step 2: Upload via GitHub Action

```yaml
- name: Upload SARIF
  uses: github/codeql-action/upload-sarif@v3
  with:
    sarif_file: fallow.sarif
```

### All-in-one with `--ci`

```bash
fallow dead-code --ci > fallow.sarif
```

The `--ci` flag is equivalent to `--format sarif --fail-on-issues --quiet`. Note: `--fail-on-issues` means exit code 1 if issues exist, in CI scripts use `continue-on-error: true` or `|| true` to ensure the SARIF upload step still runs.

---

## Guard `git push` with a Claude Code PreToolUse hook

Use this when Claude Code is allowed to run Git commands in a repository that already uses fallow.

The pattern is a local agent gate, not a Git hook. Claude Code intercepts its own `Bash` tool calls before execution. When Claude tries `git commit` or `git push`, the hook runs:

```bash
fallow audit --format json --quiet --explain
```

Behavior:

- `pass`: allow the command
- `warn`: allow the command
- `fail`: exit 2 and write the raw audit JSON to stderr
- runtime error JSON like `{ "error": true, ... }`: fail open, do not block

Because Claude receives stderr as tool feedback on a blocked `PreToolUse` call, it can read the structured findings (including `_meta.docs` links and `actions`), fix the code, and retry the Git command.

Install it automatically:

```bash
fallow hooks install --target agent
```

Remove it later with:

```bash
fallow hooks uninstall --target agent
```

Manual files:

### `.claude/settings.json`

```json
{
  "$schema": "https://json.schemastore.org/claude-code-settings.json",
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          {
            "type": "command",
            "command": "\"$CLAUDE_PROJECT_DIR\"/.claude/hooks/fallow-gate.sh"
          }
        ]
      }
    ]
  }
}
```

### `.claude/hooks/fallow-gate.sh`

Prefer `fallow hooks install --target agent` to install this file. The script is written and maintained by fallow itself; the canonical source is [`crates/cli/src/setup_hooks/fallow-gate.sh`](https://github.com/fallow-rs/fallow/blob/main/crates/cli/src/setup_hooks/fallow-gate.sh).

Behavior you can rely on:
- Runs only when the intercepted command matches `git commit` or `git push`; otherwise exits 0.
- Resolves `fallow` from PATH first, then `npx --no-install fallow` as a fallback. Skips with a stderr notice if neither is available or if `jq` is missing.
- Enforces a version floor via `FALLOW_GATE_MIN_VERSION` (default `2.46.0`). Binaries below the floor are blocked with an upgrade hint. Set the env var to the empty string to disable the check.
- Runs `fallow audit --format json --quiet --explain` and, on verdict=`fail`, writes the full JSON envelope to stderr preceded by `fallow-gate: blocked by fallow <version> at <binary>` so the responsible binary is always identifiable.
- On runtime error (`{"error": true, ...}`) or unexpected non-zero exit, fails open with a one-line stderr notice; warn verdicts pass through silently.

Codex fallback (add to repo root `AGENTS.md`):

```md
Before any `git commit` or `git push`, run `fallow audit --format json --quiet --explain`. If the verdict is `fail`, fix the reported findings before retrying. Treat JSON runtime errors like `{ "error": true, ... }` as non-blocking.
```

Keep `fallow audit` in CI alongside this local gate. The hook only runs for Claude Code, not for human pushes or other agents, so it is a reinforcement layer rather than a replacement for server-side enforcement.

### Remove the hook

```bash
fallow hooks uninstall --target agent
```

Removes the fallow-gate handler from `.claude/settings.json` (preserving any other handlers in the same matcher group), deletes `.claude/hooks/fallow-gate.sh` if it still carries the `# Generated by fallow setup-hooks.` marker, and strips the managed block from `AGENTS.md`. Idempotent: a second run reports `unchanged` / `not present` and exits 0.

Use `--force` to remove a hook script that the user has edited (the marker is no longer present). Use `--dry-run` to preview without touching files.

### Distinguish from `fallow hooks install --target git`

`fallow hooks install --target git` is a different target: it scaffolds a shell-level Git pre-commit hook under `.git/hooks/` that runs `fallow` on changed files. That is the *human* enforcement path. `fallow hooks install --target agent` is the *agent* enforcement path, targeting `.claude/` and `AGENTS.md`. Both can live in the same repo: git hooks catch human commits, the agent gate catches agent commits.
