---
paths:
  - "crates/graph/**"
---

# fallow-graph crate

Key modules:
- `project.rs` — `ProjectState` struct: owns the file registry (stable FileIds sorted by path) and workspace metadata
- `resolve.rs` — oxc_resolver-based import resolution + glob-based dynamic import pattern resolution. Cross-workspace imports resolve through node_modules symlinks via canonicalize. Pnpm `.pnpm` virtual store paths mapped back to workspace source files. React Native platform extensions resolved via `resolve_file` fallback. Per-file tsconfig path alias resolution (`TsconfigDiscovery::Auto`).
- `graph/mod.rs` — `ModuleGraph` struct, `build()` orchestrator, public query methods
- `graph/types.rs` — `ModuleNode`, `ReExportEdge`, `ExportSymbol`, `SymbolReference`, `ReferenceKind`
- `graph/build.rs` — Phase 1 (edge construction) and Phase 2 (reference population)
- `graph/reachability.rs` — Phase 3 (BFS reachability from entry points)
- `graph/re_exports.rs` — Phase 4 (re-export chain propagation through barrel files)
- `graph/cycles.rs` — Circular dependency detection (Tarjan's SCC + elementary cycle enumeration)

Cross-workspace resolution: Unified module graph across npm/yarn/pnpm workspaces and TypeScript project references. Package.json `exports` field subpath imports resolve via oxc_resolver with output→source fallback (dist/build/out/esm/cjs → src).
