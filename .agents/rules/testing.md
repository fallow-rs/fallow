---
paths:
  - "**/tests/**"
  - "**/*_test.rs"
  - "**/tests.rs"
  - "tests/fixtures/**"
---

# Testing conventions

## Integration tests
- Each crate has a `tests/` directory with an `integration_test.rs` hub that includes sub-modules via `#[path]`
- Helper utilities in `common.rs`: `fixture_path(name)` resolves `tests/fixtures/{name}/`, `create_config(root)` builds a minimal `ResolvedConfig`
- Test fixtures at workspace root `tests/fixtures/` — each fixture is a minimal but complete project with `package.json`, `tsconfig.json`, and source files
- Tests call `fallow_core::analyze(&config)` and assert on the structured `AnalysisResults`

## Snapshot tests (insta)
- CLI output snapshots in `crates/cli/tests/snapshot_tests.rs` with snapshots stored in `crates/cli/tests/snapshots/*.snap`
- Redact dynamic values before snapshotting: versions (`env!("CARGO_PKG_VERSION")`), elapsed time, absolute paths
- Use `insta::assert_snapshot!("descriptive_name", value)` — the name must be unique and descriptive
- Run `cargo insta review` to accept/reject snapshot changes

## Unit tests
- Prefer separate `tests.rs` files included via `mod tests;` over inline `#[cfg(test)]` blocks
- Use focused helper functions (e.g., `tokenize(code)`) to minimize boilerplate

## Cross-platform paths
- Normalize `\\` to `/` before string comparison (see code-quality.md)
- Use `Path::ends_with()` / `Path::components()` for separator-agnostic checks
- Never hardcode OS-specific separators in assertions

## New fixture checklist
When adding a test fixture:
1. Create `tests/fixtures/{name}/` with `package.json` (at minimum)
2. Add `tsconfig.json` if TypeScript resolution is involved
3. Keep fixtures minimal — only the files needed to reproduce the scenario
4. Add a test in the relevant crate's integration test module

## Performance tests
- Criterion benchmarks in `benches/`
- `bench-real-world.yml` runs against real-world open source projects
