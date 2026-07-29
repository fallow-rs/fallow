---
paths:
  - "**/*.rs"
  - "Cargo.toml"
  - ".clippy.toml"
---

# Rust code quality

## Clippy configuration
- Lint groups: `all`, `pedantic`, `nursery`, `cargo` (priority -1) with strategic allow-list
- Restriction lints including `excessive_nesting` (threshold 7 in `.clippy.toml`), `allow_attributes_without_reason`, `unimplemented`; see `[workspace.lints.clippy]` in `Cargo.toml` for the full list
- SIG-aligned thresholds in `.clippy.toml`: `too_many_lines` (150 LOC, ratchet toward 100), `too_many_arguments` (7 params), `cognitive_complexity` (25). These map to SIG Unit Size, Unit Interfacing, and Unit Complexity properties respectively
- Compiler lints: `unsafe_op_in_unsafe_fn`, `unused_unsafe`, `non_ascii_idents`, `tail_expr_drop_order`
- All suppressions use `#[expect(clippy::..., reason = "...")]` — warns when unnecessary, preventing dead annotations. Every `#[allow]` and `#[expect]` must include a `reason` attribute. Use `#[allow]` only for pedantic-only or target-dependent lints where `#[expect]` would be unfulfilled.

## Size assertions
`ModuleNode` (96 bytes), `ModuleInfo` (400 bytes), `ExportInfo` (112 bytes), `ImportInfo` (96 bytes), `Edge` (32 bytes), `MemberAccess` (48 bytes), `ImportedName` / `ExportName` (24 bytes each), prevents accidental struct bloat. Source of truth is `const _: () = assert!(...)` in `crates/types/src/extract.rs`; this doc is informational and may lag the code.

## Formatting
`.rustfmt.toml` with `style_edition = "2024"`.

## Build profiles
- **Dev**: `debug = false` for faster builds. Selective `opt-level` for proc-macro crates (`serde_derive`, `clap_derive`) and snapshot test deps (`insta`, `similar`).
- **Release**: `lto = true`, `codegen-units = 1`, `strip = "symbols"`, `panic = "abort"` (no unwind tables — smaller binary).

## Cross-platform test paths
CI runs on Linux, macOS, and Windows. Tests that assert on file paths MUST normalize separators:
- Use `.replace('\\', "/")` on any `to_string_lossy()` path before comparing with string literals containing `/`
- Or use `Path::ends_with()` / `Path::components()` which are separator-agnostic
- Never hardcode `"src\\foo.ts"` or `"src/foo.ts"` without normalizing first

## Disallowed types
`HashMap` and `HashSet` from `std::collections` are forbidden (configured in `.clippy.toml`). Use `FxHashMap` / `FxHashSet` from `rustc_hash` — faster hashing, deterministic iteration order for test stability. New proc-macro dependencies must be added to `[profile.dev.package.*]` with `opt-level = 1` (see existing entries for `serde_derive`, `clap_derive`).

## Typo checking
CI runs `typos` (configured in `_typos.toml`). All code, comments, and test strings must pass `typos` before committing. When tests need intentionally invalid identifiers, use clearly synthetic names (e.g. `nonexistent`, `invalid_zone`) rather than misspellings of real words.

## CI hardening
- `permissions: {}` deny-all baseline on all workflows
- `git diff --exit-code` to catch uncommitted generated code
- `--document-private-items` rustdoc check
- `cargo-shear` for unused dependency detection
- `zizmor` for GitHub Actions security scanning
- `cargo-bloat` for binary size tracking
- `cargo-modules` for module coupling analysis (SIG Module Coupling property)
