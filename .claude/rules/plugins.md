---
paths:
  - "crates/core/src/plugins/**"
  - "crates/config/src/external_plugin.rs"
---

# Plugin system

107 built-in plugins implementing the `Plugin` trait with enablers (package.json detection), static patterns, and optional `resolve_config()` for AST-based config parsing.

## Rich config parsing (22 plugins)

- **ESLint**: Legacy plugin/extends/parser short-name resolution (top-level AND inside `overrides[*]`), flat config plugin keys, JSON config, shared config following (reads imported config packages' entry points one level deep to discover peer deps), relative-path `extends` chain following (`./config/base.js`, `../shared/eslintrc.json`) with cycle protection and depth cap, settings["import/resolver"] (string/array/object formats)
- **Vite**: rollupOptions.input, lib.entry, optimizeDeps include/exclude, ssr.external/noExternal
- **Jest**: preset, setupFiles, globalSetup/Teardown, testMatch, transform, reporters, testEnvironment, watchPlugins, resolver, snapshotSerializers, testRunner, runner, JSON config
- **Storybook**: addons, framework (string/object), stories, core.builder, typescript.reactDocgen
- **Tailwind**: content globs, plugins (require/strings), presets
- **Webpack**: entry (string/array/object/descriptor with context), resolve.alias mappings, plugins require(), externals, module.rules loader extraction
- **TypeScript**: extends (string/array TS 5.0+), compilerOptions.types → @types/*, jsxImportSource, plugins, references, JSONC
- **Babel**: presets/plugins with short-name resolution, extends, JSON/.babelrc
- **Rollup**: input entries, external deps
- **PostCSS**: plugins (object keys, require() calls, string arrays)
- **Prettier**: plugins array (JSON/.prettierrc and JS configs)
- **Nuxt**: modules, css, plugins, extends, postcss plugins; path aliases (`~`, `~~`, `#shared`)
- **Drizzle**: schema field (string/array/glob/directory → entry points), out directory
- **Angular**: angular.json projects.*.architect.build.options → entry points; peer dep awareness
- **Vitest**: test.include, setupFiles, globalSetup, environment, reporters, coverage.provider, typecheck.checker, browser.provider; projects[*] nested extraction
- **Stryker**: documented default `stryker.conf.{json,js,mjs,cjs}` / `stryker.config.{json,js,mjs,cjs}` configs plus fallow-compatible `jsonc` and `ts` variants stay alive at the root or inside workspaces; static imports/requires are credited; `plugins` / `appendPlugins` credit explicit package names; `checkers: ["typescript"]` credits `@stryker-mutator/typescript-checker`; known `testRunner` short names credit their runner packages
- **Nx**: project.json targets.*.executor → deps; targets.*.options.{main, browser, styles, scripts, tsConfig} → entry points; targets.*.options.stylePreprocessorOptions.includePaths → SCSS include paths (with `{projectRoot}`/`{workspaceRoot}` token expansion)
- **Prisma**: `generator { provider = "..." }` extraction from default `schema.prisma` / `prisma/schema.prisma` files and the multi-file `prisma/schema/*.prisma` layout; credits custom-generator npm packages; skips `datasource` providers, shell-command form (`node ./gen.js`), relative-path form, and commented-out providers. Custom schema paths configured via `prisma.config.ts`'s `schema` field are out of scope (filesystem fallback is non-recursive); users with non-canonical layouts fall back to `ignoreDependencies`.
- **AdonisJS**: v5 `.adonisrc.json` (`preloads`, `providers`, `commands`, `aceProviders` mixed string / `{ file, environment }` forms; `aliases` → path-alias table; `metaFiles[].pattern` → always-used; `types` declarations). v6 / v7 `adonisrc.ts` (walks `defineConfig({...})` for thunk-wrapped `() => import('SPEC')` in `preloads` / `providers` / `commands`, `directories.*` overrides → extra entry patterns, project `package.json#imports` → Node subpath path aliases). `@ioc:` v5 IoC virtual import prefix suppresses `unlisted-dependency` for runtime-resolved container imports.
- **Fumadocs**: activates on `fumadocs-mdx`, `fumadocs-core`, `fumadocs-ui`, or a root/workspace `source.config.{ts,tsx,js,jsx,mts,mjs,cts,cjs}` file. Keeps `source.config.*` and generated `.source/**/*.{ts,tsx,js,jsx,mts,mjs,cts,cjs}` modules alive, traverses the hidden `.source` directory when active, suppresses `fumadocs-mdx:*` virtual imports, credits packages imported by `source.config.*`, and extracts literal `dir` values from `defineCollections`, `defineDocs`, and direct `defineConfig({ collections })` object entries as Markdown/MDX/JSON/YAML content roots.
- **Mintlify**: activates on the `mint` / `mintlify` dependency or a root/workspace `docs.json` / `mint.json` file. Keeps `docs.json` / `mint.json` always-used, credits `mint` and `mintlify` as tooling dependencies, and marks `{md,mdx}` under the config file's directory as support entry points so docs content does not surface as unused. The content pattern is scoped to the docs root (workspace runs prefix it from the monorepo root); the whole docs-root tree is credited rather than parsing the `navigation` graph.
- **Oxlint**: parses `.oxlintrc.json`, `oxlint.json`, and `oxlint.config.ts`; `jsPlugins` string entries, tuple first-elements, and alias-object specifier values are credited as npm dependencies; relative/absolute plugin paths are recorded as support setup files; built-in Oxlint plugin names are ignored for dependency credit.
- **Wuchale**: keeps documented `wuchale.config.js` files alive at the root or inside workspaces; credits package imports/requires from those config files; treats `wuchale` and `@wuchale/vite-plugin` as tooling dependencies; and follows static `configFile` values passed to `@wuchale/vite-plugin` in `vite.config.*` only when they point at JavaScript config modules. Dynamic values and unsupported `wuchale.config.ts` files remain reportable.
## Plugin trait extensions
- `path_aliases()` for framework-specific alias resolution (Nuxt `~/`, Next.js `@/`)
- `virtual_module_prefixes()` for framework virtual modules (Docusaurus `@theme/`, `@docusaurus/`, TanStack Start `tanstack-start-manifest:`)
- `virtual_package_suffixes()` for framework virtual package conventions (Vitest `/__mocks__`). Matches as `ends_with` on the extracted package name, suppressing `unlisted-dependency` reports for non-npm specifiers like `@aws-sdk/__mocks__`.

## External plugins
Standalone definitions in JSONC/JSON/TOML or inline via `framework` config field. Discovered from `plugins` config, `.fallow/plugins/`, and `fallow-plugin-*` files. See `docs/plugin-authoring.md`.
