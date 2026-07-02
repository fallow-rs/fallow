# fallow

**Deterministic codebase intelligence for TypeScript, JavaScript, and styling.**

Quality, risk, architecture, dependencies, duplication, design-system drift, and safe cleanup evidence for humans, CI, and agents.

[![CI](https://github.com/fallow-rs/fallow/actions/workflows/ci.yml/badge.svg)](https://github.com/fallow-rs/fallow/actions/workflows/ci.yml)
[![npm](https://img.shields.io/npm/v/fallow.svg)](https://www.npmjs.com/package/fallow)
[![MIT License](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/fallow-rs/fallow/blob/main/LICENSE)

Fallow turns a frontend repository into a trusted quality report: health score, changed-code risk, hotspots, duplication, architecture issues, dependency hygiene, styling consistency, and cleanup opportunities.

It helps you answer: what changed, what got riskier, what should be reviewed, what should be refactored, and what can be safely removed. No AI inside the analyzer. Fallow produces deterministic findings, typed output contracts, and traceable explanations that downstream tools can trust.

Static analysis is open source. An optional runtime layer adds production execution evidence. Rust-native, sub-second, 123 framework plugins, no Node.js runtime dependency for analysis. fallow's edge is doing more in one pass: dead code, duplication, dependencies, complexity, architecture, styling consistency, and security candidates in a single tool. For head-to-head timings against [knip](https://knip.dev) and [jscpd](https://github.com/kucherenko/jscpd), see the [benchmarks](https://github.com/fallow-rs/fallow/tree/main/benchmarks): fallow is faster than knip on smaller projects, knip is faster on several larger repos, and jscpd's Rust rewrite is faster for raw duplication scanning.

## Installation

```bash
npm install --save-dev fallow   # or: pnpm add -D fallow / yarn add -D fallow / bun add -d fallow
```

Installs the `fallow` CLI plus the companion `fallow-lsp` and `fallow-mcp` binaries in your project.

The package also ships a version-matched Agent Skill under `skills/fallow`.
For tools that need CLI and issue-surface metadata without spawning the binary,
`fallow/capabilities.json` mirrors `fallow schema` for the installed version.
TanStack Intent discovers it from `node_modules` automatically:

```bash
npx @tanstack/intent list
npx @tanstack/intent load fallow#fallow
```

For one-off CLI use without project-local skill discovery, run `npx fallow`.

Parsing fallow's JSON output in TypeScript? Import the typed shapes:

```ts
import type { CheckOutput, FallowJsonOutput } from "fallow/types";
```

The types are generated from the same schema as the VS Code extension and pin to the CLI version you install. See [docs.fallow.tools](https://docs.fallow.tools) for the full output contract.

## Quick start

```bash
npx fallow audit                 # PR-style audit: verdict pass / warn / fail
npx fallow audit --format json   # Machine-readable audit (for CI and agents)
npx fallow health --score        # Quality score and grade
npx fallow                       # Full codebase analysis: health + duplication + cleanup
npx fallow dead-code             # Cleanup-specific findings
npx fallow fix --dry-run         # Preview automatic cleanup
```

## What Fallow reports

- **Quality score** -- compact health score with grade and trend delta when snapshot history is enabled
- **PR risk** -- changed-code analysis with pass / warn / fail verdict and per-finding attribution
- **Hotspots** -- functions, files, and packages combining complexity, churn, size, and coupling
- **Duplication** -- clone families across four detection modes (strict, mild, weak, semantic)
- **Design-system styling** -- CSS, Sass/Less, CSS Modules, Tailwind/shadcn/CVA, StyleX/PandaCSS, vanilla-extract, styled-components, and Emotion consistency signals
- **Architecture** -- circular dependencies, boundary violations, re-export chains
- **Dependency hygiene** -- unused, unlisted, unresolved, duplicate, and type-only deps; pnpm catalog and overrides
- **Cleanup opportunities** -- unused files, exports, types, enum members, class members, stale suppressions
- **Runtime intelligence (optional)** -- hot paths, cold code, runtime-weighted health, stale flags

Cleanup opportunities are findings that look safe to review for removal because no graph evidence supports keeping them. Dead code is one category of cleanup, not the product identity.

## Code duplication

```bash
fallow dupes                       # Default: mild mode
fallow dupes --mode semantic       # Catch clones with renamed variables
fallow dupes --threshold 5         # Fail CI if duplication exceeds 5%
fallow dupes --save-baseline       # Save current duplication as baseline
```

Four detection modes (strict, mild, weak, semantic), clone family grouping with refactoring suggestions, baseline tracking, and cross-language TS/JS matching.

## Built for agents

Fallow gives AI agents structured repo truth instead of forcing them to infer everything from grep. Agents call the CLI or the MCP server to answer:

- Who imports this symbol?
- Why is this export considered used or unused?
- What changed in this PR?
- Which files are risky to touch?
- What duplicate siblings exist?
- What cleanup action is safest?

Every issue in `--format json` carries a machine-actionable `actions` array with an `auto_fixable` flag so agents can self-correct.

### MCP server

Agents that speak MCP can launch the bundled `fallow-mcp` server. Installed as a devDependency, the binary lives in `node_modules/.bin/` and is not on your `PATH`, so launch it through your package manager's runner:

```json
{
  "mcpServers": {
    "fallow": {
      "command": "npx",
      "args": ["fallow-mcp"]
    }
  }
}
```

Swap `npx` for `pnpm exec` / `yarn` / `bunx` to match your package manager. If `fallow-mcp` is installed globally (on your `PATH`), `"command": "fallow-mcp"` works directly. See the [MCP integration guide](https://docs.fallow.tools/integrations/mcp).

## Framework support

123 built-in plugins covering Next.js, Nuxt, Remix, Qwik, SvelteKit, Gatsby, Astro, Angular, NestJS, AdonisJS, Ember, Expo Router, Vite, Webpack, Vitest, Jest, Playwright, Cypress, Storybook, ESLint, TypeScript, Tailwind, UnoCSS, Prisma, Drizzle, Convex, Turborepo, Hardhat, and many more. Auto-detected from your `package.json`.

## Configuration

Create a config file in your project root, or run `fallow init`:

```jsonc
// .fallowrc.json
{
  "$schema": "https://raw.githubusercontent.com/fallow-rs/fallow/main/schema.json",
  "entry": ["src/workers/*.ts", "scripts/*.ts"],
  "ignorePatterns": ["**/*.generated.ts"],
  "rules": {
    "unused-files": "error",
    "unused-exports": "warn",
    "unused-types": "off"
  }
}
```

Also supports TOML (`fallow init --toml` creates `fallow.toml`).

## Documentation

- [Full documentation](https://docs.fallow.tools)
- [GitHub repository](https://github.com/fallow-rs/fallow)
- [Plugin Authoring Guide](https://github.com/fallow-rs/fallow/blob/main/docs/plugin-authoring.md)

## License

MIT
