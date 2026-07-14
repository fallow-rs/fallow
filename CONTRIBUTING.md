# Contributing to Fallow

Thanks for your interest in contributing to fallow! This guide covers everything you need to get started.

## Getting started

Repository tooling and the published Node packages require Node.js 22 or later.

```bash
git clone https://github.com/fallow-rs/fallow.git
cd fallow
git config core.hooksPath .githooks    # Enable commit-msg/pre-commit/pre-push hooks
npm install                            # Install repo tooling such as commitlint
cargo build --workspace
npm run verify:fast                   # Canonical local feedback loop
```

On Windows, enable symlink checkout support before cloning. If you already
cloned the repo, enable it and check out the repo again:

```bash
git config --global core.symlinks true
```

The CLI's bundled GitLab CI templates under `crates/cli/templates/ci/` are
symlinks to the canonical workspace sources under `ci/`. Cargo dereferences
them when packaging the published crate.

## Development workflow

### Building

```bash
cargo build --workspace              # Debug build
cargo build --release -p fallow-cli  # Release build (CLI only)
```

### Testing

```bash
npm run verify                        # Alias for the canonical fast checks
npm run verify:fast                   # Formatting, linting, contracts, boundaries
npm run verify:full                   # Fast checks plus script/Rust tests, benches, docs, NAPI
cargo test -p fallow-core             # Focused single-crate test run
```

Run `npm install` at the repository root and `pnpm install` under
`editors/vscode` before either verification command. Full verification also
requires `npm ci` under `crates/napi` and a local platform compiler. It is the
most comprehensive local gate and is recommended before pushing substantial
changes.

Local verification is deliberately not a simulation of every CI job. Miri,
MSRV and cross-platform jobs, feature-specific and editor integration jobs,
release and publish jobs, and network or real-project smoke tests remain
CI-only. Run `npm run verify:full -- --help` to review the exact local scope and
exclusions.

### Running locally

```bash
cargo run --bin fallow -- dead-code       # Unused code analysis
cargo run --bin fallow -- dupes           # Duplication detection
cargo run --bin fallow -- health          # Complexity metrics
cargo run --bin fallow -- fix --dry-run   # Auto-fix preview
cargo run --bin fallow -- list --plugins  # Show detected plugins
```

### Benchmarks

```bash
cargo bench --bench analysis                                    # Criterion benchmarks
cd benchmarks && npm run generate && npm run bench              # Comparative vs knip
cd benchmarks && npm run generate:dupes && npm run bench:dupes  # vs jscpd
cd benchmarks && npm run generate:circular && npm run bench:circular  # vs madge/dpdm
```

## Project structure

The workspace follows the ownership boundaries in
[`docs/architecture-invariants.md`](docs/architecture-invariants.md):

| Path | Responsibility |
| --- | --- |
| `crates/types/` | Shared typed contracts, issue metadata, suppressions, and envelope data. |
| `crates/config/` | Configuration loading, typed configuration, presets, and workspace discovery. |
| `crates/extract/` | Parser-facing facts for JavaScript, TypeScript, framework files, MDX, and CSS. |
| `crates/graph/` | Module graph construction, import resolution, dependency traversal, cycles, and impact facts. |
| `crates/security/` | Shared security matcher catalogue and candidate helpers. |
| `crates/core/` | Internal detector backend used by `fallow-engine` for private detector phases. It is not the supported embedder surface. |
| `crates/engine/` | Analysis sessions, discovery, parsing, graph construction, and typed analysis orchestration. |
| `crates/output/` | Shared output contracts, action builders, summaries, SARIF builders, and reusable formatter pieces. |
| `crates/api/` | Supported Rust facade and programmatic workflow adapters. |
| `crates/cli/` | CLI protocol adapter, command dispatch, terminal interaction, and protocol-specific serialization. |
| `crates/lsp/` | LSP adapter for diagnostics, code actions, hover, and code lens. |
| `crates/mcp/` | MCP adapter exposing fallow as typed tools for AI agents. |
| `crates/multicall/` | Unpublished binary crate bundling the CLI, LSP, and MCP servers into the single `fallow` binary shipped by the npm platform packages. |
| `crates/napi/` | NAPI adapter and Node.js bindings over the supported Rust facade. |
| `crates/license/` | Offline signed-license verification used by licensed CLI capabilities. |
| `crates/v8-coverage/` | V8 coverage parsing and source-offset mapping. |
| `crates/benchmarks/` | CodSpeed and Criterion benchmark suites for the Rust workspace. |
| `editors/vscode/` | VS Code extension and generated TypeScript contract consumer. |
| `editors/zed/` | Zed extension. |
| `editors/nvim/` | Neovim integration documentation. |
| `npm/fallow/` | npm wrapper, launchers, generated types, and bundled agent skill. |
| `npm/<platform>/` | Platform-specific native binary packages consumed by the npm wrapper. |

Dependency direction is deliberate. Foundation and analysis crates do not
depend on protocol adapters. `fallow-core` remains an internal detector backend
behind `fallow-engine`; `fallow-output` shapes reusable contracts without
starting analysis; `fallow-api` provides the supported Rust facade. The CLI,
LSP, MCP, and NAPI adapters translate their protocol options and call
`fallow-api` or `fallow-engine` rather than calling `fallow-core` directly.

## Adding a framework plugin

The most common contribution is adding support for a new framework. Each plugin lives in `crates/core/src/plugins/` as a single Rust file.

1. Create `crates/core/src/plugins/my_framework.rs`
2. Implement the `Plugin` trait (see existing plugins for examples)
3. Register it in `crates/core/src/plugins/mod.rs`
4. Add tests

A minimal plugin needs:
- `name()` — framework name
- `enablers()` — package.json dependencies that activate the plugin
- `entry_patterns()` — glob patterns for entry point files
- Optionally: `resolve_config()` for AST-based config parsing

See the [Plugin Authoring Guide](docs/plugin-authoring.md) for the full trait API and external plugin format.

## Adding an analyzer or finding

New built-in findings need more than detector code: rule metadata, output
formats, suppressions, LSP/MCP surfaces, fixtures, and generated contract files
must move together. Use the [Analyzer Authoring Guide](docs/analyzer-authoring.md)
before adding a new issue kind or framework-specific analyzer.

## Adding a known tooling dependency

Some dev tools are used through the CLI or config rather than imported in source (`typescript`, `prettier`, `husky`, `@types/*`), so they should never be reported as unused devDependencies. These live in a data-driven catalogue at `crates/core/data/tooling.toml`. Adding one is a single-file, one-entry change with no regeneration step:

```toml
# A whole package family (every member is tooling):
[[prefix]]
pattern = "@types/"
notes = "TypeScript type definitions"

# A single package:
[[exact]]
name = "typescript"
ecosystem = "core"
```

- Use `[[prefix]]` when every package under a scope or name family is tooling (matched with `name.starts_with(pattern)`); use `[[exact]]` for a single package name. `notes` / `ecosystem` are optional, for human context only.
- Do **not** add framework-plugin packages (`vite-plugin-*`, `prettier-plugin-*`, `eslint-plugin-*`, `@rollup/plugin-*`, or scoped forms like `@ianvs/prettier-plugin-sort-imports`). Those must be credited by the relevant plugin's config parser when they actually appear in the config file; listing them here would hide a declared-but-unused plugin. The catalogue's parse tests reject such entries.
- Run `cargo test -p fallow-core plugins::tooling` to validate the catalogue (it checks the TOML parses, has no empty/whitespace prefixes, no duplicates, and no framework-plugin entries). The file is embedded into the binary via `include_str!`, so a passing test means a working release.

## Editing the JSON output contract

Fallow's JSON output schema lives in `docs/output-schema.json` (JSON Schema draft-07) and is consumed by downstream tools (VS Code extension TypeScript codegen, GitHub Action jq scripts, AI agents using AJV validation).

The schema covers two layers, with different ownership rules:

### Layer 1: types derived from Rust

Rust-owned schema types live across `crates/types/src/` (analysis and base payloads), `crates/output/src/` (health, action, utility, and envelope wire shapes), `crates/api/src/` (programmatic and attribution wrappers), and `crates/config/src/` (configuration inputs). The types, output, and API crates gate their `JsonSchema` derives behind a `schema` feature; config derives `JsonSchema` unconditionally because it also owns the configuration schema. The authoritative output-contract inventory is `derived_definition_names()` plus the corresponding registrations and imports in `crates/cli/src/bin/schema_emit.rs`.

The CLI's `schema-emit` feature enables the feature-gated derives through `fallow-cli/schema` and includes config's always-available schema types, so a single `cargo run -p fallow-cli --features schema-emit --bin fallow-schema-emit` covers the whole tree.

A drift gate (`cargo test -p fallow-cli --features schema-emit --bin fallow-schema-emit`) compares the derived shape against the committed `docs/output-schema.json` and fails when:
- a Rust struct gains a field that is missing from the schema,
- a Rust struct loses a field that is still listed in the schema,
- a Rust field is required but the schema has it optional (or vice versa),
- any structural divergence on the Rust-owned definitions (full equality after canonicalization erases cosmetic differences: doc-comment prose, `oneOf`/`anyOf` choice, single-arm `allOf` wrappers, schemars integer-width hints, `Option<T>` nullable-union form).

To regenerate `docs/output-schema.json` against the Rust source of truth:

```bash
cargo run -p fallow-cli --features schema-emit --bin fallow-schema-emit > docs/output-schema.json
```

Phase 8 closed the prose-and-shape escape hatch: every type in `derived_definition_names()` is regenerated from Rust, with descriptions sourced from `///` doc comments and per-envelope titles from `#[schemars(title = "...")]`. Editing the committed schema by hand on any in-scope definition will fail the strict gate on the next `cargo test`.

### Layer 2: hand-written sections

The following surfaces of `docs/output-schema.json` stay hand-maintained today:

- **Top-level metadata** (`$schema`, `$id`, `$comment`, `title`); the `merge_with_committed` step in `crates/cli/src/bin/schema_emit.rs` preserves them verbatim. `$id` is the canonical raw GitHub URL and is used by consumers to SHA-pin a schema revision (see `docs/backwards-compatibility.md`). `description` is now overwritten by `rewrite_document_root_one_of` to keep it in sync with the typed root.
- **Definitions in `HAND_MAINTAINED_ALLOW_LIST`** inside `drift_tests` in `crates/cli/src/bin/schema_emit.rs`: today this is `CloneFamilyAction`, `CloneGroupAction`, and `CoverageAnalyzeOutput`. Each entry carries a reason linking it to the meta-issue ladder rung that retires it. The strict drift gate's orphan check fires on any other hand-maintained definition, so the allow-list is the canonical record of what stays hand-written and why.
- **`CoverageAnalyzeOutput`** is also listed in `HAND_MAINTAINED_ROOT_ENVELOPES` so it remains reachable from the document root; the `hand_maintained_root_envelopes_appear_in_root_one_of` drift test fires if the entry is silently dropped from the root `oneOf`.

The `committed_property_refs_match_derived_property_refs` drift test catches `$ref`-value drift between derived and committed property shapes (e.g. if a future change repoints `CombinedOutput.dupes` away from `DuplicationReport`); this is a check, not a hand-maintained section.

The document-root `oneOf` is now derived from Rust as of #384 item 6: the typed `FallowOutput` enum in `crates/output/src/root_envelopes.rs` wraps every object-shaped envelope and emits the root union via the `rewrite_document_root_one_of` step in `crates/cli/src/bin/schema_emit.rs`. Editing the root `oneOf` by hand will be reverted on the next regeneration. The root union is `[FallowOutput, CodeClimateOutput, ...HAND_MAINTAINED_ROOT_ENVELOPES]`: `CodeClimateOutput` (a bare JSON array via `#[serde(transparent)]`) is a sibling branch because the planned future move to `#[serde(tag = "kind")]` requires every variant of `FallowOutput` to serialize as an object, which the Code Climate / GitLab Code Quality spec forbids for that one envelope.

If you add a new finding type or utility shape, derive `JsonSchema` on the matching Rust struct, register it in `derived_definition_names()`, and the drift gate forces the schema to follow. **Adding a new top-level envelope** also requires adding a new variant to `FallowOutput` in `crates/output/src/root_envelopes.rs` so the document root keeps documenting every wire shape.

### After editing the schema

Regenerate the complete contract bundle from the repository root after changing
any schema or generated contract surface:

```bash
npm run generate:contracts
```

Before opening a pull request, confirm every committed contract matches a fresh
generation:

```bash
npm run generate:contracts:check
```

## Git conventions

- **Conventional commits**: `feat:`, `fix:`, `chore:`, `refactor:`, `test:`, `docs:`
- **Commit linting**: `npm run commitlint -- --last --verbose` uses the same rule set as CI
- **Signed commits**: `git commit -S`
- Pre-commit hooks run `cargo fmt` and `cargo clippy` automatically

## Code style

- Follow existing patterns — the codebase is consistent
- `cargo clippy --workspace -- -D warnings` must pass (pedantic lints enabled)
- `cargo fmt --all -- --check` must pass
- No `unsafe` without justification
- Prefer early returns with guard clauses

## Submitting changes

1. Fork the repository
2. Create a feature branch from `main`
3. Make your changes with conventional commit messages
4. Ensure `cargo test --workspace` and `cargo clippy --workspace -- -D warnings` pass
5. Open a pull request against `main`

## Reporting issues

- **Bug reports**: [Open an issue](https://github.com/fallow-rs/fallow/issues/new?template=bug_report.yml) with reproduction steps
- **Feature requests**: [Open an issue](https://github.com/fallow-rs/fallow/issues/new?template=feature_request.yml) describing the problem and proposed solution
- **False positives**: Include the fallow output and a minimal reproduction

## Documentation

Documentation lives at [docs.fallow.tools](https://docs.fallow.tools). For documentation improvements, open a PR or issue.
