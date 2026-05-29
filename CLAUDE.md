# Fallow: Rust-native codebase intelligence for TypeScript and JavaScript

Fallow is codebase intelligence for TypeScript and JavaScript. The free static layer finds unused files, exports, dependencies, types, enum members, class members, unresolved imports, unlisted deps, duplicate exports, circular dependencies, boundary violations, code duplication, and complexity hotspots, plus opt-in API hygiene checks such as private type leaks. A paid runtime intelligence layer (Fallow Runtime) adds production execution evidence (hot and cold paths, runtime-backed review, runtime-weighted health, stale-flag evidence, trends, alerts). Rust alternative to [knip](https://github.com/webpro-nl/knip) built on the Oxc parser ecosystem.

For shared domain vocabulary, term definitions, and flagged ambiguities: see @CONTEXT.md. For the feature-workflow chain (when /fallow-implement, /panel-review, /user-panel, /fallow-review are invoked and how the .plans/ artefact threads them together): see @.claude/rules/workflow.md.

## Project structure

```
crates/
  config/   -- Configuration types, custom framework presets, package.json parsing, workspace discovery
  types/    -- Shared type definitions (discover, extract, results, suppress, serde_path)
  extract/  -- AST extraction engine (visitor.rs, complexity.rs, sfc.rs, astro.rs, mdx.rs, css.rs, parse.rs, cache.rs, suppress.rs, tests/)
  graph/    -- Module graph construction (graph/), import resolution (resolve.rs), project state (project.rs)
  license/  -- Offline Ed25519 JWT verification for paid features (alg pinned, file+env load precedence, 7/30/hard-fail grace ladder)
  v8-coverage/ -- V8 ScriptCoverage parser + byte-offset-to-line/col mapper + Istanbul normalizer (open-source layer of Phase-2 runtime coverage)
  core/     -- Analysis orchestration: discovery, plugins, scripts, duplicates, cross-reference, caching, progress
    analyze/    -- Dead code detection (mod.rs orchestration, predicates.rs, unused_files/exports/deps/members.rs)
    plugins/    -- Plugin system + tooling.rs (general tooling dependency detection)
    duplicates/ -- Clone detection (families, normalize, tokenize)
  cli/      -- CLI binary, split into per-command modules
    audit.rs, check.rs, dupes.rs, health/, watch.rs, fix/, init.rs, list.rs, schema.rs, validate.rs, regression/, impact.rs
    license/    -- `fallow license {activate, status, refresh, deactivate}` with offline JWT verify plus live trial / refresh flows
    coverage/   -- `fallow coverage setup` resumable first-run state machine for runtime coverage
    report/     -- Output formatting (mod.rs dispatch, human/, json.rs, sarif.rs, compact.rs, markdown.rs)
    migrate/    -- Config migration (mod.rs, knip.rs, jscpd.rs)
  lsp/      -- LSP server, split into modules
    main.rs, diagnostics/, code_actions/, code_lens.rs, hover.rs
  mcp/      -- MCP server for AI agent integration (stdio transport, wraps CLI)
editors/
  vscode/   -- VS Code extension (LSP client, tree views, status bar, auto-download)
npm/
  fallow/   -- npm wrapper package with optionalDependencies pattern
action/       -- GitHub Action (composite)
  jq/         -- jq scripts for summaries, annotations, review comments, merging
  scripts/    -- Bash scripts (install, analyze, annotate, comment, review, summary)
  tests/      -- Unit tests for jq scripts (run: bash action/tests/run.sh)
ci/           -- GitLab CI template and supporting scripts
  jq/         -- jq scripts for GitLab MR formatting (comments, reviews, summaries, merging)
  scripts/    -- Bash scripts (comment.sh, review.sh)
  tests/      -- Unit tests for jq scripts (92 tests, run: bash ci/tests/run.sh)
tests/
  fixtures/ -- Integration test fixtures
decisions/ -- Architecture Decision Records (ADRs)
```

## Architecture

Pipeline: Config → File Discovery → Incremental Parallel Parsing (rayon + oxc_parser + oxc_semantic, cache-aware) → Script Analysis → Module Resolution (oxc_resolver) → Graph Construction → Re-export Chain Resolution → Dead Code Detection → Reporting

## Building & Testing

```bash
git config core.hooksPath .githooks  # Enable pre-commit hooks (fmt + clippy)
cargo build --workspace
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo run --bin fallow                       # Run all analyses (dead-code + dupes + health)
cargo run --bin fallow -- watch              # Watch mode
cargo run --bin fallow -- fix --dry-run      # Auto-fix preview
```

## Code conventions

- Config files: `.fallowrc.json` > `.fallowrc.jsonc` > `fallow.toml` > `.fallow.toml`
- No `detect` section in config; use `rules` with `"off"` severity
- No `output` in config; output format is CLI-only via `--format`
- Rules severity: `error` (fail CI, default) | `warn` (exit 0) | `off` (skip)
- Inline suppression: `// fallow-ignore-next-line [issue-type]` and `// fallow-ignore-file [issue-type]`
- Environment variables: `FALLOW_FORMAT`, `FALLOW_QUIET`, `FALLOW_BIN` (binary path for MCP), `FALLOW_CACHE_MAX_SIZE` (extraction cache cap in MB; default 256)
- See `.claude/rules/code-quality.md` for clippy, size assertions, and CI hardening details

## Key design decisions

Documented as Architecture Decision Records in [`decisions/`](decisions/). Key decisions:

- **No TypeScript compiler** ([ADR-001](decisions/001-no-typescript-compiler.md)): Syntactic analysis via Oxc parser + `oxc_semantic`. No type resolution, no tsc.
- **Flat edge storage** ([ADR-002](decisions/002-flat-edge-storage.md)): Contiguous `Vec<Edge>` with range indices for cache-friendly traversal.
- **FxHashMap/FxHashSet required** ([ADR-003](decisions/003-fxhashmap-over-std.md)): Standard `HashMap`/`HashSet` disallowed (enforced via `.clippy.toml`).
- **Path-sorted FileIds** ([ADR-004](decisions/004-path-sorted-file-ids.md)): Stable cross-run identity, not insertion order.
- **Re-export chain resolution** ([ADR-005](decisions/005-re-export-chain-resolution.md)): Iterative propagation through barrel files with cycle detection.
- **Hidden directory allowlist** ([ADR-006](decisions/006-hidden-directory-allowlist.md)): `.storybook`, `.vitepress`, `.well-known`, `.changeset`, `.github` traversed; other dotdirs skipped.

## Git conventions

- Conventional commits: `feat:`, `fix:`, `chore:`, `refactor:`, `test:`
- Signed commits (`git commit -S`)
- No AI attribution in commits

## Project communication

- Never reduce fallow to "dead code tool" in taglines or summaries; reference all 5 analysis areas (unused code, circular deps, duplication, complexity hotspots, boundary violations). Category is "codebase analyzer."
- Comparison pages must be research-backed with source links; never claim a competitor "can't" do something without checking
- Design specs are definitions, not implementations: tokens, rules, components, ASCII wireframes, table-described behavior; no CSS/JS/HTML code blocks

## Repo layout (for this working tree)

- `~/Sites/fallow-2/` is a working copy of fallow main; primary checkout is the bare-config'd `~/Sites/fallow/`
- `.internal/`, `quality/`, `reference/`, `benchmarks/fixtures/`, `benchmarks/knip6/` are gitignored symlinks; `.internal/` points at `~/Sites/fallow-cloud/.internal/` (single source of truth, edit only there); the rest point at `~/Sites/fallow/`
- `npm/fallow/skills/` is a vendored copy of `~/Sites/fallow-skills/`; refresh happens at release time, not manually
- Edit fallow skills in `~/Sites/fallow-skills/fallow/skills/fallow/`, never in the symlinked `~/.agents/skills/fallow/`
- GitHub org: `fallow-rs/fallow` (use `gh ... --repo fallow-rs/fallow`); never `bartwaardenburg/fallow`
- `fallow dead-code` is dead-code only (legacy alias `check` still works); bare `fallow` runs the full pipeline (dead-code + dupes + health)

## Worktree / parallel-agent rules

Multiple agents and background sessions frequently land commits in fallow main concurrently. Treat every working tree as racy:

- **Commit WIP early.** If a feature takes more than ~10 minutes and parallel sessions are active, switch to a feature branch (`git checkout -b feat/<name>`) and commit per chunk. Uncommitted state in main does not survive even one parallel `git stash` cycle, especially for untracked files.
- **Verify commit authors before every push.** Run `git log --format="%H %ae %s" <base>..HEAD` and abort if any author is not `bart@waardenburg.dev`, a contributor email, or `...@users.noreply.github.com`. Worktrees and pre-push hooks have leaked `test@example.com` and `test@test.com` commits in the past.
- **Never push fallow commits via fallow-2 (or any worktree) when WIP exists.** Fix the bare-repo push at its root (e.g. unset `GIT_DIR`/`GIT_WORK_TREE` in `.githooks/pre-push`) or create a fresh ephemeral worktree with `git -C <bare> worktree add /tmp/fallow-push <branch>`.
- **`combined.rs` is a merge-conflict magnet.** It absorbs orientation header, nudge, entry-point display, summary threading, baseline deltas, health options. Assign ALL `combined.rs` edits to a single agent that runs after parallel crate-level work finishes.
- **After cherry-picking from worktree agents, always run `cargo fmt --all`.** Worktree agents do not always produce rustfmt-compliant code.
- **After every worktree merge, scan for orphan conflict markers.** `grep -r '<<<<<<' crates/` (already auto-enforced by the conflict-marker-scan PostToolUse hook, but run manually before pushing).
- **After cleaning up worktrees, force-remove all of them and `cargo clean -p <crate>` before testing.** Stale worktree compilation artifacts make new code invisible to `cargo test --list`.
- **Worktree agents may skip commits.** After each worktree agent completes, verify with `git log <base>..<branch> --oneline`; if empty, check for unstaged changes in the worktree directory and commit manually before cleanup.

See `AGENTS.md` for AI agent integration guide.
