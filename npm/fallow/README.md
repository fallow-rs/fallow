# fallow

Codebase intelligence for TypeScript and JavaScript, built in Rust.

[![CI](https://github.com/fallow-rs/fallow/actions/workflows/ci.yml/badge.svg)](https://github.com/fallow-rs/fallow/actions/workflows/ci.yml)
[![npm](https://img.shields.io/npm/v/fallow.svg)](https://www.npmjs.com/package/fallow)
[![MIT License](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/fallow-rs/fallow/blob/main/LICENSE)

Static analysis finds what is connected. Runtime intelligence finds what actually runs. Both land in the same report. The free static layer analyzes your codebase for unused files, exports, dependencies, and types, detects circular dependencies, finds duplicated code blocks, surfaces complexity hotspots, and enforces architecture boundaries. An optional paid runtime layer (Fallow Runtime) adds production execution evidence so you can delete and refactor with confidence. **5-41x faster** than [knip](https://knip.dev) v5 (**2-18x faster** than knip v6), **8-29x faster** than [jscpd](https://github.com/kucherenko/jscpd) for duplication detection, with no Node.js runtime dependency.

Static analysis is free and open source. Runtime intelligence is the paid team layer.

## Installation

```bash
npm install --save-dev fallow   # or: pnpm add -D fallow / yarn add -D fallow / bun add -d fallow
```

Installs the `fallow` CLI plus the companion `fallow-lsp` and `fallow-mcp` binaries in your project.

The package also ships a version-matched Agent Skill under `skills/fallow`.
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

## Usage

```bash
fallow                           # All analyses -- zero config, sub-second
fallow dead-code                 # Unused code only
fallow dupes                     # Duplication detection -- find copy-paste clones
fallow dupes --mode semantic     # Catch clones with renamed variables
fallow health                    # Complexity metrics -- cyclomatic + cognitive
fallow fix --dry-run             # Preview auto-removal of unused exports and deps
```

## What it finds

- **Unused files** -- not reachable from any entry point
- **Unused exports** -- exported symbols never imported elsewhere
- **Unused types** -- type aliases and interfaces never referenced
- **Unused dependencies** -- packages in `dependencies` never imported
- **Unused devDependencies** -- dev packages not referenced
- **Unused enum members** -- enum values never referenced
- **Unused class members** -- class methods and properties never referenced (tracks instance usage: `const svc = new MyService(); svc.greet()` counts `greet` as used)
- **Unresolved imports** -- import specifiers that cannot be resolved
- **Unlisted dependencies** -- imported packages missing from `package.json`
- **Duplicate exports** -- same symbol exported from multiple modules
- **Circular dependencies** -- import cycles in the module graph
- **Type-only dependencies** -- production deps only used via `import type`

## Code duplication

```bash
fallow dupes                       # Default: mild mode
fallow dupes --mode semantic       # Catch clones with renamed variables
fallow dupes --threshold 5         # Fail CI if duplication exceeds 5%
fallow dupes --save-baseline       # Save current duplication as baseline
```

4 detection modes (strict, mild, weak, semantic), clone family grouping with refactoring suggestions, baseline tracking, and cross-language TS/JS matching.

## Framework support

95 built-in plugins covering Next.js, Nuxt, Remix, Qwik, SvelteKit, Gatsby, Astro, Angular, NestJS, AdonisJS, Expo Router, Vite, Webpack, Vitest, Jest, Playwright, Cypress, Storybook, ESLint, TypeScript, Tailwind, UnoCSS, Prisma, Drizzle, Convex, Turborepo, Hardhat, and many more. Auto-detected from your `package.json`.

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
