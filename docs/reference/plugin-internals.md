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
