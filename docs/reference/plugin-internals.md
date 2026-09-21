# Plugin internals

Use this reference for built-in framework plugins, external plugin loading, and
plugin activation.

## Ownership

- `crates/core/src/plugins/`: built-in framework and tool behavior.
- `crates/config/src/external_plugin.rs`: external plugin schema and loading.
- `crates/config/src/config/`: plugin configuration and resolution.
- `crates/core/src/plugins/registry/`: built-in registry and activation
  predicates.
- `crates/engine/src/plugins.rs`: public facade and external plugin inspection.
- `plugin-schema.json`: generated public schema for external plugins.
- `docs/plugin-authoring.md`: contributor workflow for authoring plugins.

## Discovery and activation

Plugins may contribute entry points, always-used files, used exports, tooling
dependencies, config patterns, manifest-derived entries, and detection rules.
Activation must be derived from explicit project evidence such as a dependency,
config file, or declared combinator.

Framework-specific AST interpretation belongs in a built-in plugin. Portable
declarative behavior belongs in the external plugin contract.

## Invariants

- Detection must not activate on an unrelated package or same-named local
  symbol.
- Normalize paths before matching and keep workspace scope explicit.
- Seeded entries must still pass normal discovery and extension rules.
- Plugin output is additive. One malformed external plugin must produce a
  useful configuration error without corrupting unrelated built-ins.
- Generated schema and examples must move with external plugin fields.
- Do not document volatile built-in plugin counts as architecture.

## Runtime-provided specifiers

A plugin may contribute a `ProvidedDependencyRule` from parsed config, not only
as a static list. Use it for an import specifier the framework supplies at
runtime rather than npm, such as a Module Federation remote alias.

Scope the rule to the directory of the config file that declared it, not to the
whole project: a config inside a workspace package must not silence a finding in
a sibling package. Match the specifier exactly plus its `alias/` subpath prefix,
never a bare prefix that would also cover a sibling package name.

The rule suppresses unlisted-dependency findings only. It does not change
resolution, so a real installed package, a path alias, or a workspace package
with the same name still wins, and the import keeps crediting that package.

## Advisories about a config a plugin could not use

A plugin never prints an advisory about the config it read. It pushes a
`PluginConfigDiagnostic` onto its `PluginResult`, and the end of the plugin run
converts the set once and writes it to the workspace-diagnostic registry as a
plugin-stage kind. Two reasons:

- A plugin knows the fact but not the root the message renders against. In a
  workspace run its own `root` is the package root, while the diagnostic's path
  and message are project-root-relative. Keep the config path ABSOLUTE in the
  advisory so the registry's canonical dedupe and the serialized root-relative
  form both work from one value, and leave it out of the workspace-prefix pass.
- A `tracing::warn!` from inside a plugin reaches no envelope, no CI consumer and
  no dedupe, so a combined run printed it once per analysis and
  `--quiet --format json` never saw it at all.

Two kinds exist. Use `plugin-config-unreadable` when a declaration the user
wrote did not reach the analysis, which degrades the run and warns on stderr.
Use `plugin-effect-not-modeled` when the config was readable and fallow stood a
modeled default down instead, which loses nothing measurable, warns on no
channel, and must not fire on every run of a project that cannot change its
config. Record either one only where a finding was actually affected: a surface
whose patterns were retained, not every root that has a config file.

A result that carries only an advisory is not empty, so it survives the
registry's empty-result gate. The reason token is a kebab-case string from an
open set, and the sentence is composed once in `fallow-types` from the plugin,
the key and that token, so a plugin contributes no prose.

## Config paths read from a nested config

A path read out of a config file resolves against that file's directory unless
the config declares its own base, as webpack's `context` does. A tool config
that is not at the project root is therefore only correct for the tree it sits
in, which is what keeps a workspace package from seeding entries for a sibling.

An entry pattern is a glob while a config value is a literal path, so escape a
value before pushing it as a pattern. Bracketed route filenames and `*` in a
path would otherwise both miss the named file and cover files the config does
not name.

A config value that names a module request is credited as a dependency and is
never pushed as an entry pattern. An entry pattern is a glob over project files,
so a value a bundler resolves through module resolution, one without a leading
`./`, `../` or `/` and without a source extension, would match no file while its
package still needs the credit. A value carrying glob syntax is exempt: no module
resolution accepts a glob, so `src/pages/**` stays a pattern. A trailing resource
query is dropped first, because a bundler hands it to the loader rather than
resolving it as part of the request, so `pkg/client?reload=true` credits `pkg`
while the `?` of `src/pag?.ts` still marks a glob. Bundler `entry` values and
Module Federation `exposes` targets share one predicate for this.

A declared `always_used` pattern is matched against the project-relative path
without a `**/` rewrite, so it covers a root-level file only. A plugin that
reads a config at any depth must push the resolved path onto
`always_used_files` itself, or the file it just consumed is reported as unused.

## Author verification

Use `plugin-check` as the primary read-only authoring check:

```bash
fallow plugin-check --format json --quiet
fallow list --plugins
```

`plugin-check` reports activation and manifest-entry evidence. Advisory
findings return success, while invalid configuration or serialization errors
return exit code 2.

For built-in changes, add a focused plugin test and an end-to-end fixture that
proves entry points and usage crediting.
