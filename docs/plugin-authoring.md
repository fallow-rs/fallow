# Plugin Authoring Guide

Fallow supports external plugin definitions that let you add framework and tool support without writing Rust code. External plugins provide the same declarative capabilities as built-in plugins.

## Quick Start

Create a file named `fallow-plugin-<name>.jsonc` in your project root:

```jsonc
{
  "$schema": "https://raw.githubusercontent.com/fallow-rs/fallow/main/plugin-schema.json",
  "name": "my-framework",
  "enablers": ["my-framework"],
  "entryPoints": ["src/routes/**/*.{ts,tsx}"],
  "alwaysUsed": ["src/setup.ts"],
  "toolingDependencies": ["my-framework-cli"],
  "usedExports": [
    { "pattern": "src/routes/**/*.{ts,tsx}", "exports": ["default", "loader", "action"] }
  ]
}
```

That's it. Fallow automatically discovers `fallow-plugin-*` files in your project root.

## Supported Formats

| Format | Extension | Comments | `$schema` support |
|--------|-----------|----------|-------------------|
| JSONC  | `.jsonc`  | `//` and `/* */` | Yes |
| JSON   | `.json`   | No | Yes |
| TOML   | `.toml`   | `#` | No |

All formats use `camelCase` field names. We recommend JSONC for its comment support and `$schema` IDE autocomplete. Generate the schema with:

```bash
fallow plugin-schema
```

## Plugin File Format

### Required

| Field | Type | Description |
|-------|------|-------------|
| `name` | string | Unique plugin name (shown in `fallow list --plugins`) |

### Optional

| Field | Type | Description |
|-------|------|-------------|
| `detection` | object | Rich activation logic (dependency, fileExists, all/any) |
| `enablers` | string[] | Package names that activate this plugin |
| `entryPoints` | string[] | Glob patterns for framework entry point files |
| `entryPointRole` | string | Reachability role for entry points: runtime, test, or support |
| `manifestEntries` | object[] | Entry points derived from framework manifest files |
| `configPatterns` | string[] | Glob patterns for config files (marked always-used) |
| `alwaysUsed` | string[] | Glob patterns for files always considered used |
| `toolingDependencies` | string[] | Packages used via CLI, not source imports |
| `usedExports` | object[] | Exports always considered used in matching files |
| `usedClassMembers` | array | Class members invoked by the framework at runtime |

### `enablers`

Package names checked against `package.json` dependencies. The plugin activates if **any** enabler matches. Only used when `detection` is not set.

Supports prefix matching with a trailing `/`:

```jsonc
{
  "enablers": ["@myorg/"]  // matches @myorg/core, @myorg/cli, etc.
}
```

### `detection`

Rich activation logic with boolean combinators. Takes priority over `enablers` when set.

```jsonc
{
  // Activate when a specific package is installed
  "detection": { "type": "dependency", "package": "next" }
}
```

```jsonc
{
  // Activate when a config file exists
  "detection": { "type": "fileExists", "pattern": "nuxt.config.*" }
}
```

```jsonc
{
  // Combine conditions
  "detection": {
    "type": "all",
    "conditions": [
      { "type": "dependency", "package": "@my-org/core" },
      { "type": "fileExists", "pattern": "my-org.config.*" }
    ]
  }
}
```

### `entryPoints`

Glob patterns for files that serve as entry points to your application. These files are never flagged as unused, and their imports are traced through the module graph.

```jsonc
{
  "entryPoints": [
    "src/routes/**/*.{ts,tsx}",
    "src/middleware.{ts,js}",
    "src/plugins/**/*.ts"
  ]
}
```

### `entryPointRole`

Controls how every entry from `entryPoints` and `manifestEntries` contributes
to coverage reachability. Use `runtime` for application roots, `test` for test
roots, or `support` for setup and configuration roots. The default is
`support`.

```jsonc
{
  "entryPoints": ["src/main.ts"],
  "entryPointRole": "runtime"
}
```

### `manifestEntries`

Derives entry points from matching JSON or JSONC manifest files. Each rule
selects manifests, can require exact field values through `when`, and resolves
its `entries` relative to the manifest directory. Entry paths may interpolate a
dotted manifest field. A string creates one path and an array creates one path
per value.

```jsonc
{
  "entryPointRole": "runtime",
  "manifestEntries": [
    {
      "manifests": "**/framework.jsonc",
      "format": "jsonc",
      "when": { "enabled": true },
      "entries": [
        { "path": "public/index.{ts,tsx}", "when": { "browser": true } },
        { "path": "${modules}/index.ts" }
      ]
    }
  ]
}
```

### `configPatterns`

Glob patterns for framework config files. When the plugin is active, these files are marked as always-used (they won't be flagged as unused files).

```jsonc
{
  "configPatterns": [
    "my-framework.config.{ts,js,mjs}",
    ".my-frameworkrc.{json,yaml}"
  ]
}
```

### `alwaysUsed`

Files that should always be considered used when this plugin is active, even if nothing imports them.

```jsonc
{
  "alwaysUsed": [
    "src/setup.ts",
    "public/**/*",
    "src/global.d.ts"
  ]
}
```

### `toolingDependencies`

Packages that are tooling dependencies -- used via CLI commands or config files, not imported in source code. These won't be flagged as unused dev dependencies.

```jsonc
{
  "toolingDependencies": [
    "my-framework-cli",
    "@my-framework/dev-tools"
  ]
}
```

### `usedExports`

Exports that are always considered used for files matching a glob pattern. Use this for convention-based frameworks where specific export names have special meaning.
Use `"*"` when every export in matching convention files is consumed by the framework.

```jsonc
{
  "usedExports": [
    { "pattern": "src/routes/**/*.{ts,tsx}", "exports": ["default", "loader", "action", "meta"] },
    { "pattern": "src/**/*.stories.{ts,tsx}", "exports": ["*"] },
    { "pattern": "src/middleware.ts", "exports": ["default"] }
  ]
}
```

### `usedClassMembers`

Marks class members that a framework invokes without a visible source-level
reference. A string or glob applies to every class. Use a scoped object for
common member names that should apply only when a class extends a named base
class or implements a named interface. A scoped rule must define `extends` or
`implements`.

```jsonc
{
  "usedClassMembers": [
    "frameworkInit",
    "lifecycle*",
    { "implements": "CellRenderer", "members": ["refresh"] },
    { "extends": "BaseCommand", "members": ["execute"] }
  ]
}
```

## Discovery

Fallow discovers external plugins in this order (first occurrence of a plugin name wins):

1. **Explicit paths** from the `plugins` config field
2. **`.fallow/plugins/`** directory -- all `*.jsonc`, `*.json`, `*.toml` files
3. **Project root** -- `fallow-plugin-*.{jsonc,json,toml}` files

### Using the `plugins` config field

Point to specific plugin files or directories:

```jsonc
// .fallowrc.json
{
  "plugins": [
    "tools/fallow-plugins/",
    "vendor/my-plugin.jsonc",
    "vendor/another-plugin.json"
  ]
}
```

### Using `.fallow/plugins/`

Place plugin files in `.fallow/plugins/` for automatic discovery:

```
my-project/
  .fallow/
    plugins/
      my-framework.jsonc
      custom-tool.json
  src/
  package.json
```

### Using project root

Name plugin files with the `fallow-plugin-` prefix:

```
my-project/
  fallow-plugin-my-framework.jsonc
  fallow-plugin-custom-tool.json
  src/
  package.json
```

## Examples

### React Router / TanStack Router

```jsonc
{
  "$schema": "https://raw.githubusercontent.com/fallow-rs/fallow/main/plugin-schema.json",
  "name": "react-router",
  "enablers": ["react-router", "@tanstack/react-router"],
  "entryPoints": [
    "src/routes/**/*.{ts,tsx}",
    "app/routes/**/*.{ts,tsx}"
  ],
  "configPatterns": [
    "react-router.config.{ts,js}"
  ],
  "toolingDependencies": ["@react-router/dev"],
  "usedExports": [
    { "pattern": "src/routes/**/*.{ts,tsx}", "exports": ["default", "loader", "action", "meta", "handle", "shouldRevalidate"] },
    { "pattern": "app/routes/**/*.{ts,tsx}", "exports": ["default", "loader", "action", "meta", "handle", "shouldRevalidate"] }
  ]
}
```

### Custom CMS

```jsonc
{
  "$schema": "https://raw.githubusercontent.com/fallow-rs/fallow/main/plugin-schema.json",
  "name": "my-cms",
  "enablers": ["@my-cms/core"],
  "entryPoints": ["content/**/*.{ts,tsx}", "schemas/**/*.ts"],
  "alwaysUsed": ["cms.config.ts", "content/**/*.mdx"],
  "configPatterns": ["cms.config.{ts,js}"],
  "toolingDependencies": ["@my-cms/cli"],
  "usedExports": [
    { "pattern": "content/**/*.{ts,tsx}", "exports": ["default", "metadata", "getStaticProps"] }
  ]
}
```

### Internal Tooling

```jsonc
{
  // Internal build system plugin
  "$schema": "https://raw.githubusercontent.com/fallow-rs/fallow/main/plugin-schema.json",
  "name": "our-build-system",
  "enablers": ["@internal/build"],
  "configPatterns": [
    "build.config.{ts,js}",
    ".buildrc"
  ],
  "alwaysUsed": [
    "scripts/build/**/*.ts",
    "config/**/*.ts"
  ],
  "toolingDependencies": [
    "@internal/build",
    "@internal/lint-rules",
    "@internal/test-utils"
  ]
}
```

## Sharing Plugins

External plugins are plain files -- share them however you share config:

- **Git**: check `fallow-plugin-*` files into your repo
- **Monorepo**: put shared plugins in a central `tools/` directory and reference via `plugins` config
- **npm package**: publish a package containing plugin files, then reference them: `plugins = ["node_modules/@my-org/fallow-plugins/"]`

## JSON Schema

Generate the JSON Schema for plugin files to enable IDE autocomplete and validation:

```bash
fallow plugin-schema > plugin-schema.json
```

Reference it in your plugin files:

```jsonc
{
  "$schema": "./plugin-schema.json",
  "name": "my-plugin",
  "enablers": ["my-pkg"]
}
```

## Built-in vs External Plugins

| Capability | Built-in | External |
|-----------|---------|---------|
| Entry points | Yes | Yes |
| Always-used files | Yes | Yes |
| Used exports | Yes | Yes |
| Tooling dependencies | Yes | Yes |
| Config file patterns | Yes | Yes |
| AST-based config parsing | Yes | No |
| Custom detection logic | Yes | Yes (dependency, fileExists, all/any combinators) |

External plugins cover the vast majority of use cases. AST-based config parsing (extracting entry points from `vite.config.ts`, resolving ESLint plugin short names, etc.) requires a built-in Rust plugin.

## Verifying

Run the read-only authoring check first:

```bash
fallow plugin-check --format json --quiet
fallow list --plugins
```

`plugin-check` reports whether each external plugin activated and shows
manifest-entry evidence. Advisory findings return success. Invalid
configuration or serialization errors return exit code 2.

`list --plugins` then confirms the active plugin inventory.
