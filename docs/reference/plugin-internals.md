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

## Reading Module Federation options

One reader serves the standalone `module-federation.config.*` file and the
Federation plugin call inside a webpack, rspack, rsbuild, vite or `next.config.*`
file. A call is found wherever it sits in the config program, because a plugin
list is a nested array, a variable, a tool-specific key such as
`tools.rspack.plugins`, or a hook body such as the Next.js `webpack(config)`
hook as often as it is a literal array. The walk runs over the AST that the
plugin's `read` already parsed, so the reader adds no parse of its own.

Position must never be the accept gate. A call is read only when the callee name
is a known Federation plugin AND the first argument is an object that declares
`exposes`, `remotes` or `shared`. Without the shape gate a library that exports a
same-named function would seed entry points in a project that does not use
Module Federation. The gate applies to the whole options value, after the
reader resolves it.

The reader resolves the first argument through a small set of shapes: an object
literal, a name, an object spread of one of these, `Object.assign(...)` over
them, and the argument of a wrapper call. A name resolves to a top-level binding
of the same file, including `export const`, and then to a relative ESM import of
a sibling config. A relative CommonJS `require('./x')` resolves to the value
that module exports as a whole. A standalone config goes through the same
resolver for the value it exports, so both paths read a wrapper the same way.

A known identity wrapper (`createModuleFederationConfig`, `defineConfig`)
passes its argument through, so the argument is read with no advisory. Any
other call can add to or change what it returns. The reader reads the object
literal that the call receives as a lower bound, and records the
`unrecognized-call` advisory against each Federation key that literal declares.
The allowlist lives in the Federation reader only. The shared
`extract_object_from_expression` still reads the first object argument of any
call, because many config readers depend on it for wrappers such as `withMDX`.

A followed relative import or `require` whose target cannot be read records the
`import-target-unreadable` advisory against each Federation key that the
readable part does not declare. A missing file and a file whose export is not a
readable options value are both unreadable targets. The
parser resolves a same-file name only when the program holds one binding of it,
as a top-level `const` or `let`, and no expression writes to the binding or to
one of its members. A name that a hook body or a parameter declares again, and a
binding that a later statement reassigns or mutates, name another object at the
call, so the resolver declines. A package `require` does not resolve. An
argument that does not resolve and has no relative binding is silent. A spread or an `Object.assign`
argument that does not resolve records the `spread` advisory against each
Federation key that the readable part does not declare, because the hidden part
can declare that key. When the readable part declares no Federation key, only a
callee that names Module Federation beyond doubt records it. The bare
`federation` callee does not, because other libraries export a function of that
name. Resolution stops after a fixed number of steps, so a
spread cycle ends as an unreadable spread.

The array form of `exposes` is read. A bundler uses a string element both as the
public name and as the module request, and an object element goes through the
same mapping reader as the object form. An element that holds glob syntax, a
nested array or a value that is not a string joins the `unreadable-entries`
advisory, because a bundler resolves one element as one request. The array form
of `remotes` stays
unread: a bundler derives the request scope of an element from the whole
container location, so the alias is not a bare specifier a provider rule can
cover, and splitting the element on `@` would provide a specifier the bundler
does not route to the remote. That declaration keeps its `array-form` advisory.

A workspace package is read with its own directory as the plugin root. An
`exposes` target that climbs out of that directory, such as
`../shared/src/Thing.tsx`, keeps its leading `../` segments in the entry
pattern, and the reader marks the rule as parent-relative. The workspace prefix
pass resolves the segments of a marked rule against the package prefix, so the
target names a file in a sibling workspace. An unmarked rule keeps the plain
prefix, because other plugins emit patterns relative to a config directory,
such as the Storybook `../src/**`, and these must not climb out of the
workspace. A target that climbs out of the
project keeps the segments and matches no project file. It records nothing,
because the run loses nothing that it could measure.

A standalone `module-federation.config.*` that declares `exposes`, `remotes` or
`shared` credits the build plugin packages, such as `@module-federation/enhanced`,
`@module-federation/rsbuild-plugin` and `@module-federation/vite`, because no
config file imports them. It never credits `@module-federation/runtime`, which
application code imports and credits on its own.

A `shared` entry credits the package it names as a referenced dependency, the
same credit an exposed module request gets. The reader takes each key of the
object form, each string element of the array form, an object element of the
array form as the object form, and the string `import` and `packageName` of an
entry descriptor. A trailing `/` shares every subpath and credits the same
package. A relative key shares a project module and gets no credit. An unread
`shared` value records no advisory: it only withholds credit, so the package
still reports as unused, which is the behavior before the reader read `shared`.

Also unread: the runtime `registerRemotes` and `loadRemote` calls.

## Config paths read from a nested config

A path read out of a config file resolves against that file's directory unless
the config declares its own base, as webpack and rspack `context` and rsbuild
`root` do. A webpack config in a config directory (`config/`, `build/` or
`webpack/`) runs from the package root, so its Federation `exposes` targets and
`remotes` scope resolve against the parent of that directory.

A file with a `webpack.<target>` name in a config directory counts as a webpack
config only when it exports a configuration: an object with at least one webpack
config key, an array of configurations, or a `merge(...)` call. A helper module
such as `config/webpack.paths.js` stays reportable. The generic keys `name`
and `dependencies` do not count, and a `merge(...)` call counts only for the
`webpack-merge` functions. The `build/` directory also holds build output, so a
`webpack.config.*` file there is not read, because it can be compiled or stale.
A built-in ignore pattern excludes `build/` from source discovery, so config
discovery probes the filesystem for a pattern under it. It probes such a
pattern also for a plugin that already resolved a config, because the first
discovery phase never saw these files. A tool config
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

Rollup `input`, rolldown `input` and vite `build.rollupOptions.input` are the
exception. These tools resolve an `input` value with no importer: a resolve
plugin can read it as a module request, and without one it is a path relative
to the working directory. A value that the predicate calls a module request can
therefore name either one, so it keeps the entry pattern AND credits the
package. The package credit is skipped when the value names a file under the
plugin root (the value, the value with a source extension, or a directory
index). Then the value is a path, and the credit must not hide an unused package
with the same first segment. Vite `build.lib.entry` stays a path only, because
vite resolves it against its root with `path.resolve`. Webpack keeps the module
request reading, because webpack resolves an entry without `./` as a module.

A bundler resolves an entry path without a source extension the way it resolves
an import: as a file with each extension, then as a directory through its index
file. A webpack, rspack or rsbuild entry such as `./lib` therefore also yields
`lib.{ext}` and `lib/index.{ext}` patterns. The entry is classified as a path or
a module request before it is joined to a `context` or `root` directory,
because the joined value no longer carries its `./` prefix.

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
