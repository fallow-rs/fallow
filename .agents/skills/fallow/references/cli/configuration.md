# Configuration File Format

Config files are searched in priority order: `.fallowrc.json` > `.fallowrc.jsonc` > `fallow.toml` > `.fallow.toml`. Both `.fallowrc.json` and `.fallowrc.jsonc` are parsed as JSON-with-comments; the `.jsonc` extension lets editors auto-detect JSONC syntax highlighting.

## JSON Format (`.fallowrc.json` / `.fallowrc.jsonc`)

```jsonc
{
  "$schema": "https://raw.githubusercontent.com/fallow-rs/fallow/main/schema.json",

  // Entry points (glob patterns)
  "entry": ["src/index.ts", "scripts/*.ts"],

  // Files to ignore (glob patterns)
  "ignorePatterns": ["**/*.generated.ts", "**/*.d.ts"],

  // Dependencies to ignore (exact names, or globs such as "@acme/*")
  "ignoreDependencies": ["autoprefixer", "@acme/*"],

  // Suppress unused-export findings when the symbol is referenced inside its
  // declaring file (knip parity). Boolean or { type, interface } object form.
  "ignoreExportsUsedInFile": true,

  // Per-issue-type severity
  "rules": {
    "unused-files": "error",
    "unused-exports": "warn",
    "unused-types": "off",
    "unused-dependencies": "error",
    "unused-dev-dependencies": "warn",
    "unused-enum-members": "error",
    "unused-class-members": "warn",
    "unresolved-imports": "error",
    "unlisted-dependencies": "error",
    "duplicate-exports": "warn",
    "circular-dependencies": "warn",
    "boundary-violation": "error",
    "type-only-dependencies": "error",
    "test-only-dependencies": "warn",
    "stale-suppressions": "warn"
  },

  // Per-path rule overrides
  "overrides": [
    {
      "files": ["**/*.test.ts", "**/*.spec.ts"],
      "rules": { "unused-exports": "off" }
    }
  ],

  // Duplication settings
  "duplicates": {
    "mode": "mild",
    "near": false,
    "minTokens": 50,
    "minLines": 5,
    "threshold": 0,
    "ignoreDefaults": true,
    "ignoredClones": ["dup:6f12ab34:2"],
    "skipLocal": false,
    "ignore": ["**/*.generated.ts"]
  },

  // Extraction cache settings. FALLOW_CACHE_DIR overrides cache.dir.
  "cache": {
    "dir": "/tmp/fallow-cache",
    "maxSizeMb": 256
  },

  // Architecture boundaries (preset, custom zones/rules, or auto-discovered feature zones)
  // Presets: "layered", "hexagonal", "feature-sliced", "bulletproof", "deep-modules"
  // "deep-modules" makes each folder of src/modules (else src/features) a zone with its
  // index file as the public entry, plus an "app" zone (src/**) and a "tests" zone
  // (test/**, tests/**, e2e/**). It fails with exit 2 when neither folder exists.
  // "presetPublicFiles": true gives the bulletproof features and the feature-sliced
  // slices their index files as public entry files (off by default).
  // Rules accept an optional `allowTypeOnly: [zones]` list that admits type-only imports
  // (`import type`, inline `{ type Foo }`, namespace type imports, and `export type` re-exports)
  // to the listed zones even when not present in `allow`. Mixed-specifier imports still fire.
  // A zone can list its public entry files: `"publicFiles": ["src/billing/index.ts"]`. Other
  // zones may then import only those files of the zone. On an autoDiscover zone, each glob
  // is relative to each discovered folder: `"publicFiles": ["index.ts"]`.
  "boundaries": {
    "preset": "bulletproof"
    // Or:
    // "zones": [
    //   { "name": "app", "patterns": ["src/app/**"] },
    //   { "name": "features", "patterns": ["src/features/**"], "autoDiscover": ["src/features"] },
    //   { "name": "shared", "patterns": ["src/shared/**"] }
    // ],
    // "rules": [
    //   { "from": "app", "allow": ["features", "shared"] },
    //   { "from": "features", "allow": ["shared"], "allowTypeOnly": ["features"] }
    // ]
  },

  // Resolve framework convention auto-imports (Nuxt components) as graph edges.
  // Edges for `<Card001 />`-style template tags are always synthesized; setting
  // this to true also drops the Nuxt component entry patterns so an
  // unreferenced component is reported as unused-file. Kept conservative: an
  // unmodelled `components:` or `imports:` config keeps the entry patterns, but
  // a config that switches the scan off (`components: { dirs: [] }`,
  // `imports: { scan: false }`) is treated like the default. Default false.
  "autoImports": false,

  // Production mode
  "production": false,

  // Workspace packages that are public libraries.
  // Exported API surface from these packages is not flagged as unused.
  "publicPackages": ["@myorg/shared-lib", "@myorg/utils"],

  // Glob patterns for files that are dynamically loaded at runtime.
  // These files are treated as always-used and never flagged as unused.
  "dynamicallyLoaded": ["plugins/**/*.ts", "locales/**/*.json"],

  // Inherit from base config (prefer local paths or trusted npm packages)
  "extends": ["./base-config.json", "npm:@my-org/fallow-config"],

  // Custom external plugins
  "plugins": ["tools/plugins/"],

  // Inline framework definitions
  "framework": [
    {
      "name": "my-framework",
      "enablers": ["my-framework"],
      "entryPoints": ["src/routes/**/*.ts"]
    }
  ]
}
```

## TOML Format (`fallow.toml`)

```toml
entry = ["src/index.ts", "scripts/*.ts"]
ignorePatterns = ["**/*.generated.ts"]
ignoreDependencies = ["autoprefixer"]
ignoreExportsUsedInFile = true
production = false
publicPackages = ["@myorg/shared-lib", "@myorg/utils"]
dynamicallyLoaded = ["plugins/**/*.ts", "locales/**/*.json"]

[rules]
unused-files = "error"
unused-exports = "warn"
unused-types = "off"

[duplicates]
mode = "mild"
near = false
minTokens = 50
minLines = 5
ignoreDefaults = true
ignoredClones = ["dup:6f12ab34:2"]

[[overrides]]
files = ["**/*.test.ts"]
[overrides.rules]
unused-exports = "off"

[boundaries]
preset = "bulletproof"
```

## Configuration field notes

- `ignoreExportsUsedInFile`: knip-compatible; suppress unused-export findings when the exported symbol is referenced inside the file that declares it. Boolean (`true` covers all kinds) or `{ "type": true, "interface": true }` object form for knip parity. Fallow groups type aliases and interfaces under the same `unused-types` issue, so both type-kind fields behave identically. References inside the export specifier itself (`export { foo }`, `export default foo`) do not count as same-file uses; those exports are still reported when no other in-file expression references the binding
- `publicPackages`: workspace packages that are public libraries; exported API surface from these packages is not flagged as unused
- `entry`: glob patterns for files that are manual entry points. A single `*` stays within one path segment; use `**` to match across directories
- `dynamicallyLoaded`: glob patterns for files loaded at runtime (plugin dirs, locale files); treated as always-used. A single `*` stays within one path segment; use `**` to match across directories
- `markupExtensions`: file extensions without the dot (for example `["tpl", "erb"]`) of server-side template files whose text counts as CSS class references for `unreferenced-css-class` in `fallow health --css`. `liquid`, `njk`, `hbs`, `handlebars`, `twig`, and `blade.php` are always read. Any class-like word in a template counts as used, so templates are more lenient than Markdown. These files never join discovery or the module graph. A value that ends in a script, stylesheet, data or asset extension (also `module.css` or `d.ts`) is a config error
- Path globs in other fields (`ignorePatterns`, `ignoreFindings`, `overrides[].files`, `ignoreExports[].file`, `duplicates.ignore`, `health.ignore`, boundary zones, rule-pack and `framework[]` globs) use the same rule: a single `*` stays within one path segment. Write `**/*.test.ts` to match test files at any depth
- `cache.dir`: override the persistent extraction cache directory. `FALLOW_CACHE_DIR` wins over this config field, and `--no-cache` disables caching entirely
- `cache.maxSizeMb`: cap the serialized extraction cache size in megabytes. `FALLOW_CACHE_MAX_SIZE` wins over this config field
- `workspaces.changedSince`: map exact workspace roots (as `fallow list --workspaces` prints them) to Git refs. `check`, `dead-code` and `dupes` then report the findings of a mapped package only for files changed since its ref; unlisted packages and root files stay in full scope. A global `--changed-since` replaces the map for one run; `--no-package-baselines` or `FALLOW_PACKAGE_BASELINES=false` turns it off. A key that names no workspace, or a ref Git cannot resolve, leaves every package in full scope with a warning and `request_outcomes["package-baselines"]` as `not-applied`. Save a whole-project baseline with `--no-package-baselines`.
- `usedClassMembers`: class method/property names that extend the built-in Angular/React lifecycle allowlist with framework-invoked names. Each entry is a plain string (global suppression) or a scoped object `{ extends?, implements?, members }` matching only classes with the given heritage. Strings can be exact names (`"agInit"`) or glob patterns (`"*"` matches every member, `"enter*"` prefix, `"*Handler"` suffix, `"on*Event"` combined). Use scoped rules for common names like `refresh` or `execute` to avoid false negatives on unrelated classes; global strings for unique names like `agInit`. Example: `["agInit", { "implements": "ICellRendererAngularComp", "members": ["refresh"] }, { "extends": "BaseCommand", "members": ["execute"] }, { "extends": "GrammarBaseListener", "members": ["enter*", "exit*"] }]`. Glob patterns that match zero members emit a `WARN` so dead allowlist entries surface. An unconstrained scoped rule (no `extends` or `implements`) is rejected at load time. Use plugin-level `usedClassMembers` in a `.fallow/plugins/*.jsonc` file for library-specific allowlists
- `resolve.conditions`: additional package.json `exports` / `imports` condition names to honor during module resolution. Baseline conditions (`development`, `import`, `require`, `default`, `types`, `node`, plus `react-native` / `browser` under RN/Expo) are always included; user entries prepend ahead of them. Use for community conditions like `worker`, `edge-light`, `deno`, or custom bundler conditions. Example: `{ "resolve": { "conditions": ["worker", "edge-light"] } }`
- `unusedComponentProps.ignorePattern`: opt-in regex that exempts a component prop from `unused-component-props` when the prop's LOCAL destructure binding name matches (the leading-underscore "accepted-but-intentionally-unused" convention, mirroring TS `noUnusedParameters` + ESLint `varsIgnorePattern` / `argsIgnorePattern`). Applies to Vue, Svelte, Astro, and React/Preact props. The match is on the local alias (`_stage` in `let { stage: _stage } = $props()`), not the public prop name the finding reports (`stage`); matching is unanchored like ESLint's `RegExp.test`, so anchor with `^_`. An invalid regex fails config load. Example: `{ "unusedComponentProps": { "ignorePattern": "^_" } }`
- `circularDependencies.ignoreLazyImports`: opt-in boolean, default `false`. When `true`, `circular-dependencies` skips an import edge on which every import loads its target on demand or on another thread: an `import()` inside a function, a template `import()`, a lazy `import.meta.glob`, a worker URL, or a webpack worker loader request such as `worker-loader!./work.js`. A top-level `await import()`, `require()`, an eager `import.meta.glob`, and an edge that also has a static import stay. The filter runs before cycles are counted, so lazy cycles do not use the per-group cycle limit. Example: `{ "circularDependencies": { "ignoreLazyImports": true } }`
- `circularDependencies.allow`: list of accepted cycles, default `[]`. Each entry lists the files of one cycle in import order, relative to the project root, without the repeat of the first file. An entry accepts a cycle that has exactly these files in this order, from any start file. A cycle with another set of files stays reported, also when it shares an import with an accepted cycle. In a monorepo, write the path from the project root (`packages/app/src/a.ts`). The match is case-sensitive. Globs are not supported. An entry needs at least 2 files and lists each file once, or the config does not load. An entry that matches no cycle gives a `circular-dependencies-allow-unmatched` diagnostic, except in a `--production` run or with the rule off. The detector lists at most 20 cycles per group of files that import each other and no cycle with more than 12 files; an accepted cycle does not count toward the 20. An entry with more than 12 files, or with all files in a group that reached the limit, gives no diagnostic. A `fallow-ignore-next-line circular-dependency` comment on an accepted cycle becomes a stale suppression, so remove it when you add the entry. `fallow migrate` translates knip `cycles.allow`. Example: `{ "circularDependencies": { "allow": [["src/a.ts", "src/b.ts"]] } }`
- `effect.schemaExportsAsTypes`: opt-in boolean, default `false`. When `true`, an unused exported Effect schema is reported under `unused-types`, not `unused-exports`, so `"unused-types": "off"` hides it. A schema is a class that extends a `Schema` class factory (`Schema.Class`, `Schema.TaggedClass`, `Schema.TaggedError`, `Schema.TaggedRequest`, `Schema.Error`, `Schema.Opaque`) or a const whose value is a `Schema` constructor, member or pipe chain (`Schema.Struct({...})`, `Schema.String.pipe(Schema.brand("Id"))`). `Schema` must be an import from `effect`, `effect/Schema` or `@effect/schema`. Each `pipe` argument must be a `Schema` transform such as `Schema.brand(...)`. A re-export of a schema from another module (`export { User } from "./schemas"`, or an import that the module exports again) also counts. In a module that imports Effect, a chain on an imported schema (`User.pipe(Schema.brand("Id"))`, `User.annotations({...})`) and an alias (`export const Copy = User`) also count. A chain on a member of a namespace import (`S.User.pipe(...)`) does not count. Decoders such as `Schema.decodeUnknownSync(User)` and `pipe(Schema.decodeUnknownSync)` stay unused exports. Example: `{ "effect": { "schemaExportsAsTypes": true } }`
