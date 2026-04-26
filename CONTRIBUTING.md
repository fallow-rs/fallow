# Contributing to Fallow

Thanks for your interest in contributing to fallow! This guide covers everything you need to get started.

## Getting started

```bash
git clone https://github.com/fallow-rs/fallow.git
cd fallow
git config core.hooksPath .githooks    # Enable pre-commit + pre-push hooks
cargo build --workspace
./scripts/check.sh
```

## Development workflow

### Building

```bash
cargo build --workspace              # Debug build
cargo build --release -p fallow-cli  # Release build (CLI only)
```

### Testing

```bash
./scripts/check.sh                  # CI-aligned local check suite
cargo test --workspace               # All Rust tests
cargo test -p fallow-core            # Single crate
cargo clippy --workspace -- -D warnings
cargo fmt --all -- --check
```

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

```
crates/
  cli/      — CLI binary and output formatting
  config/   — Configuration types, presets, workspace discovery
  core/     — Analysis engine: plugins, discovery, parsing, resolution, graph, detection
  extract/  — AST extraction (JS/TS, Vue/Svelte SFC, Astro, MDX, CSS)
  graph/    — Module graph construction and resolution
  types/    — Shared types across crates
  lsp/      — LSP server
  mcp/      — MCP server for AI agents
editors/
  vscode/   — VS Code extension
npm/
  fallow/   — npm wrapper package
```

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

## Git conventions

- **Conventional commits**: `feat:`, `fix:`, `chore:`, `refactor:`, `test:`, `docs:`
- **Signed commits**: `git commit -S`
- Pre-commit hooks run `cargo fmt`, `cargo clippy`, and `typos` automatically
- Pre-push hooks run `./scripts/check.sh`, rustdoc warnings, and fast Miri checks automatically

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
4. Ensure `./scripts/check.sh` passes locally
5. Open a pull request against `main`

## Reporting issues

- **Bug reports**: [Open an issue](https://github.com/fallow-rs/fallow/issues/new?template=bug_report.yml) with reproduction steps
- **Feature requests**: [Open an issue](https://github.com/fallow-rs/fallow/issues/new?template=feature_request.yml) describing the problem and proposed solution
- **False positives**: Include the fallow output and a minimal reproduction

## Documentation

Documentation lives at [docs.fallow.tools](https://docs.fallow.tools). For documentation improvements, open a PR or issue.
