<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/fallow-rs/fallow/main/assets/logo-dark.svg">
    <source media="(prefers-color-scheme: light)" srcset="https://raw.githubusercontent.com/fallow-rs/fallow/main/assets/logo.svg">
    <img src="https://raw.githubusercontent.com/fallow-rs/fallow/main/assets/logo.svg" alt="fallow" width="290">
  </picture>
</p>

<p align="center">
  <strong>Codebase intelligence for TypeScript and JavaScript.</strong><br>
  Find unused code, circular dependencies, duplication, complexity hotspots, and architecture drift.<br>
  One Rust binary. No TypeScript compiler, no Node.js runtime, no configuration to start.
</p>

<p align="center">
  <a href="https://www.npmjs.com/package/fallow"><img src="https://img.shields.io/npm/v/fallow.svg" alt="npm"></a>
  <a href="https://www.npmjs.com/package/fallow"><img src="https://img.shields.io/npm/dm/fallow.svg" alt="npm downloads"></a>
  <a href="https://github.com/fallow-rs/fallow/actions/workflows/ci.yml"><img src="https://github.com/fallow-rs/fallow/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/fallow-rs/fallow/actions/workflows/coverage.yml"><img src="https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/fallow-rs/fallow/badges/coverage.json" alt="Coverage"></a>
  <a href="https://app.codspeed.io/fallow-rs/fallow?utm_source=badge"><img src="https://img.shields.io/endpoint?url=https://codspeed.io/badge.json" alt="CodSpeed"></a>
  <a href="https://github.com/fallow-rs/fallow/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="MIT License"></a>
  <a href="https://github.com/sponsors/BartWaardenburg"><img src="https://img.shields.io/github/sponsors/BartWaardenburg?label=sponsor&logo=githubsponsors&color=ea4aaa" alt="Sponsor"></a>
</p>

<p align="center">
  <a href="https://docs.fallow.tools">Docs</a> ·
  <a href="https://docs.fallow.tools/quickstart">Quickstart</a> ·
  <a href="https://docs.fallow.tools/integrations/ci">CI</a> ·
  <a href="https://docs.fallow.tools/integrations/mcp">MCP</a> ·
  <a href="BENCHMARKS.md">Benchmarks</a> ·
  <a href="#sponsors">Sponsor</a>
</p>

---

Most repositories carry code that nobody dares to delete. To delete it, you must prove that nothing uses it. fallow reads the whole repository as one dependency graph, from import edges to styling tokens, and reports what the graph shows.

```bash
npx fallow
```

```
Audit scope: 19 changed files vs HEAD~15 (8fbfcb054..HEAD)

● Unused files (2)
  packages/vitest/src/public/reporters.ts
  test/coverage-test/test/configuration-options.test-d.ts

● Circular dependencies (6)
  packages/vitest/src/integrations/vi.ts
    → wait.ts → vi.ts

✗ dead code: 156 issues · complexity: 6 findings · duplication: 8 clone groups · 19 changed files (1.05s)
  audit gate excluded 163 inherited findings (run with --gate all to enforce)
```

<sub>Excerpt from <code>fallow audit</code> on the vitest monorepo, over its last 15 commits. The gate passed: the 163 inherited findings existed before the change, so they do not block it.</sub>

## Why fallow

- **Zero configuration.** Over 100 built-in [framework plugins](https://docs.fallow.tools/frameworks/built-in) find entry points and framework conventions for you.
- **Fast.** Syntactic analysis in Rust with [Oxc](https://oxc.rs). Large monorepos finish in seconds. See [Performance](#performance).
- **Deterministic.** The same input gives the same output, with stable fingerprints. There is no AI inside the analyzer.
- **Safe to adopt.** `fallow audit` fails only on findings that a change introduces. A legacy backlog does not block day one.
- **Built for scripts and agents.** Every command has a typed JSON contract, an MCP server, and an agent skill.

## Quick start

```bash
# Run the full pipeline: dead code, duplication, and health
npx fallow

# Gate only what a pull request changed
npx fallow audit

# Propose a config for this project (read-only)
npx fallow recommend

# Add fallow to the project
npm install --save-dev fallow
```

The npm package includes the `fallow`, `fallow-lsp`, and `fallow-mcp` launchers. pnpm, yarn, `cargo install fallow-cli`, and Docker are in the [installation guide](https://docs.fallow.tools/installation).

## What fallow finds

| Analysis | Command |
|---|---|
| [Unused files, exports, types, enum and class members, and dependencies](https://docs.fallow.tools/analysis/dead-code) | `fallow dead-code` |
| [Circular dependencies and re-export cycles](https://docs.fallow.tools/analysis/dead-code) | `fallow dead-code` |
| [Code duplication](https://docs.fallow.tools/analysis/duplication) in JS, TS, CSS, and Vue, Svelte, and Astro components | `fallow dupes` |
| [Complexity hotspots and a 0 to 100 health score](https://docs.fallow.tools/explanations/health) | `fallow health` |
| [Architecture boundary violations](https://docs.fallow.tools/analysis/boundaries), with `bulletproof`, `layered`, `hexagonal`, and `feature-sliced` presets | `fallow dead-code` |
| [Design-system styling drift](https://docs.fallow.tools/analysis/css-analysis) in CSS and CSS-in-JS | `fallow health --css` |
| [Changed-file gate](https://docs.fallow.tools/cli/audit) with a pass, warn, or fail verdict | `fallow audit` |
| [Auto-fix](https://docs.fallow.tools/analysis/auto-fix) with a dry-run preview | `fallow fix --dry-run` |
| Security candidates, ranked by reachability from entry points (opt-in) | `fallow security` |
| Functions that may share intent with different syntax, from a pinned local model (opt-in) | `fallow similar-code` |

Add `--type-aware` for exact TypeScript symbol identity across aliases, re-exports, and packages. This optional pass removes false positives from interfaces and base classes. See the [type-aware analysis contract](docs/type-aware-analysis.md). fallow can also merge [runtime coverage](https://docs.fallow.tools/analysis/runtime-coverage) into health and audit reports.

`npx fallow viz` opens an interactive HTML map of the project. Every command is in the [CLI reference](https://docs.fallow.tools/cli/global-flags), and `fallow schema` prints the full capability manifest as JSON.

## Your first run

A finding on the first run usually means that fallow does not know an entry point, or that it analyzes generated files. `npx fallow recommend` detects the stack and proposes a config. You can also write one by hand:

```json
{
  "$schema": "./node_modules/fallow/schema.json",
  "ignorePatterns": ["**/*.generated.ts"]
}
```

To keep a known backlog out of CI, save a baseline with `--save-baseline` and pass `--baseline` on each run. The [adoption guide](https://docs.fallow.tools/adoption) shows the staged path, and [configuration](https://docs.fallow.tools/configuration/overview) has the full reference.

To keep an intentional export, add a suppression comment:

```ts
// fallow-ignore-next-line unused-export -- kept for plugin consumers
export const keepThis = 1;
```

## CI

GitHub Actions:

```yaml
- uses: actions/checkout@v4
  with:
    fetch-depth: 0
- uses: fallow-rs/fallow@v3
```

GitLab CI:

```yaml
include:
  - remote: 'https://raw.githubusercontent.com/fallow-rs/fallow/v3.30.0/ci/gitlab-ci.yml'

fallow:
  extends: .fallow
```

The Action installs the fallow version from the project's `package.json`. It can post PR comments, inline review comments, and SARIF for Code Scanning. Output formats include SARIF, CodeClimate, GitHub annotations, and Markdown. The [CI guide](https://docs.fallow.tools/integrations/ci) covers inputs, permissions, and staged rollout.

## Built for agents

```json
{ "mcpServers": { "fallow": { "command": "npx", "args": ["fallow-mcp"] } } }
```

- `npx fallow agent install` sets up Claude Code, Codex, and Cursor in one pass: an `AGENTS.md` task map, the fallow skill, the MCP server, and a commit gate. `--dry-run` shows the plan first.
- Add `--format json --quiet` to any command for one typed JSON document on stdout. Each finding has an `actions[]` array and an `auto_fixable` flag. Types ship as `fallow/types`.
- Exit code 0 means clean, 1 means findings, and 2 means an error. The [MCP guide](https://docs.fallow.tools/integrations/mcp) and [agent skills](https://docs.fallow.tools/integrations/agent-skills) have the details.

## Editors and integrations

- [VS Code extension](https://docs.fallow.tools/integrations/vscode), plus Zed and [Neovim](https://docs.fallow.tools/integrations/neovim) setups in [`editors/`](editors/)
- `fallow-lsp`: diagnostics, hover, code actions, and code lenses in any LSP editor
- The Node API [`@fallow-cli/fallow-node`](https://docs.fallow.tools/integrations/node-bindings) exports `detectDeadCode`, `detectCircularDependencies`, `detectBoundaryViolations`, `detectDuplication`, `detectSimilarCode`, `detectFeatureFlags`, `computeComplexity`, and `computeHealth`. The [package API reference](crates/napi/README.md) has the options and return types
- [README badges](https://docs.fallow.tools/integrations/badges): `fallow health --format badge > badge.svg`

## Performance

On the dead-code benchmark set, fallow analyzes fastify in 64ms, where knip 6 takes 205ms. On preact, fallow takes 74ms and knip 6 takes 2.01s (27.1x). knip is faster on astro and TypeScript, and jscpd is faster at raw duplication scans. fallow also completes next.js (20,558 files), vite, and vue/core, where knip stops with errors on the projects' own config files.

Measured on fallow 2.100.0, Apple M5, median of 5 cold runs. [BENCHMARKS.md](BENCHMARKS.md) has the method, the full tables, and the scripts to run them again.

## Migrating from other tools

`npx fallow migrate` converts knip, jscpd, and stylelint config into fallow config. See the guides [from knip](https://docs.fallow.tools/migration/from-knip) and [from jscpd](https://docs.fallow.tools/migration/from-jscpd), and the [comparison page](https://docs.fallow.tools/migration/comparison).

## Sponsors

fallow is free, MIT licensed, and developed in the open. Sponsorship pays for maintainer time: new framework plugins, fixes for false positives, and the real-world test corpus that keeps findings correct.

If your team runs fallow in CI, please consider [sponsoring the project](https://github.com/sponsors/BartWaardenburg). Company sponsors get their logo in this section.

<p align="center">
  <a href="https://github.com/sponsors/BartWaardenburg"><img src="https://img.shields.io/badge/Sponsor%20fallow-ea4aaa?style=for-the-badge&logo=githubsponsors&logoColor=white" alt="Sponsor fallow on GitHub"></a>
</p>

<!-- Sponsor logos go here. -->

## Contributing

Missing a framework plugin? Found a false positive? [Open an issue](https://github.com/fallow-rs/fallow/issues). [CONTRIBUTING.md](CONTRIBUTING.md) covers the development setup, and [docs/README.md](docs/README.md) is the start point for maintainer documentation. See also the [roadmap](ROADMAP.md) and the [security policy](SECURITY.md).

<a href="https://github.com/fallow-rs/fallow/graphs/contributors">
  <img src="https://contrib.rocks/image?repo=fallow-rs/fallow" alt="Contributors">
</a>

## License

MIT. See [LICENSE](LICENSE).
