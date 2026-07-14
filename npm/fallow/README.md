# fallow

**Codebase intelligence for TypeScript and JavaScript.**

One binary finds unused code, circular dependencies, duplication, complexity hotspots, boundary violations, and design-system styling drift. An optional paid layer, Fallow Runtime, adds production execution evidence. No AI inside the analyzer, and no TypeScript compiler or Node.js runtime needed for static analysis: runs are deterministic, with typed output contracts and traceable explanations.

[![CI](https://github.com/fallow-rs/fallow/actions/workflows/ci.yml/badge.svg)](https://github.com/fallow-rs/fallow/actions/workflows/ci.yml)
[![npm](https://img.shields.io/npm/v/fallow.svg)](https://www.npmjs.com/package/fallow)
[![MIT License](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/fallow-rs/fallow/blob/main/LICENSE)

## Install

```bash
npm install --save-dev fallow   # or: pnpm add -D fallow / yarn add -D fallow
```

This installs the `fallow` CLI plus the `fallow-lsp` and `fallow-mcp` launchers, so editor and agent integrations resolve the project-local binary instead of whatever happens to be on `PATH`. For one-off use, run `npx fallow` without installing. Other channels (cargo, Docker, prebuilt binaries) are covered in the [installation guide](https://docs.fallow.tools/installation).

## Quick start

```bash
npx fallow                       # Full pipeline: dead code + duplication + health
npx fallow audit                 # Gate only what a PR changed: verdict pass/warn/fail
npx fallow health --score        # 0 to 100 health score with a letter grade
npx fallow dupes                 # Duplication; modes strict, mild (default), weak, semantic
npx fallow fix --dry-run         # Preview automatic cleanup
```

## Output and exit codes

Add `--format json --quiet` to any command for one typed JSON document on stdout. Exit code 1 means findings, not failure; 0 is clean (or an audit pass or warn verdict); 2 is a validation or runtime error, reported as a JSON error envelope rather than a stack trace.

Parsing the output in TypeScript? Import the typed shapes, version-pinned to the CLI you install:

```ts
import type { CheckOutput, FallowJsonOutput } from "fallow/types";
```

Every issue carries an `actions[]` array with an `auto_fixable` flag, so scripts and agents know which findings they can hand to `fallow fix`. The full contract lives at [docs.fallow.tools](https://docs.fallow.tools).

## What fallow reports

- Unused files, exports, types, enum and class members, and dependencies
- Circular dependencies and re-export cycles
- Code duplication as clone families, across four detection modes
- Complexity hotspots and a 0 to 100 health score
- Architecture boundary violations, with zero-config presets
- Design-system styling drift for CSS and CSS-in-JS (Sass/Less, CSS Modules, Tailwind, styled-components, Emotion, and more)
- A changed-file PR gate with per-finding attribution (`fallow audit`)
- Optional runtime intelligence: hot paths, cold code, runtime-weighted health, stale flags (licensed Fallow Runtime; a single local coverage capture is free)

For head-to-head timings against [knip](https://knip.dev) and [jscpd](https://github.com/kucherenko/jscpd), see [BENCHMARKS.md](https://github.com/fallow-rs/fallow/blob/main/BENCHMARKS.md): fallow is faster than knip on smaller projects, knip is faster on several larger repos, and jscpd's Rust rewrite is faster at raw duplication scanning.

## Built for agents

Agents get structured repo truth instead of inferring everything from grep: who imports a symbol, why an export counts as used, what a PR changed, which cleanup action is safest.

The bundled `fallow-mcp` server lives in `node_modules/.bin/` when installed as a devDependency, so launch it through your package manager's runner:

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

Swap `npx` for `pnpm exec` or `yarn` to match your package manager; a globally installed `fallow-mcp` works as `"command": "fallow-mcp"` directly. See the [MCP integration guide](https://docs.fallow.tools/integrations/mcp).

The package also ships a version-matched agent skill under `skills/fallow`, and `fallow/capabilities.json` mirrors `fallow schema` for tools that need CLI and issue-surface metadata without spawning the binary. TanStack Intent discovers both from `node_modules`:

```bash
npx @tanstack/intent list
npx @tanstack/intent load fallow#fallow
```

## Framework support

Over 100 built-in framework plugins covering Next.js, Nuxt, Remix, Qwik, SvelteKit, Gatsby, Astro, Angular, NestJS, AdonisJS, Ember, Expo Router, Vite, Webpack, Vitest, Jest, Playwright, Cypress, Storybook, ESLint, TypeScript, Tailwind, UnoCSS, Prisma, Drizzle, Convex, Turborepo, Hardhat, and more. Entry points are auto-detected from `package.json`, so the first run needs no configuration.

## Configuration

Works out of the box. To customize, run `fallow init` or create a config file in your project root:

```jsonc
// .fallowrc.json
{
  "$schema": "./node_modules/fallow/schema.json",
  "entry": ["src/workers/*.ts", "scripts/*.ts"],
  "ignorePatterns": ["**/*.generated.ts"],
  "rules": {
    "unused-files": "error",
    "unused-exports": "warn",
    "unused-types": "off"
  }
}
```

`$schema` gives editors autocomplete and validation and has no effect on analysis. The npm package ships a version-aligned schema at `./node_modules/fallow/schema.json`, so validation works offline with no editor trust prompt. TOML works too: `fallow init --toml` creates `fallow.toml`. Full reference: [configuration overview](https://docs.fallow.tools/configuration/overview).

## Documentation

- [docs.fallow.tools](https://docs.fallow.tools)
- [GitHub repository](https://github.com/fallow-rs/fallow)
- [Plugin authoring guide](https://github.com/fallow-rs/fallow/blob/main/docs/plugin-authoring.md)

## License

MIT
