# Graph Agent Guide

Use this file when editing `crates/graph/**`.

## Ownership

- `project.rs`: stable file registry and workspace metadata.
- `resolve/`: import resolution, tsconfig discovery, path aliases, platform extensions, pnpm mapping.
- `graph/build.rs`: edge construction and reference population.
- `graph/reachability.rs`: entry point reachability.
- `graph/re_exports/`: barrel and re-export propagation.
- `graph/cycles.rs`: strongly connected components and cycle reporting.

## Rules

- Keep FileId assignment stable and path-sorted.
- Treat workspaces and re-export chains as first-class behavior, not edge cases.
- Keep resolver fallbacks explicit. If tsconfig resolution degrades, preserve relative and bare import behavior where possible and surface a diagnostic.
- Normalize paths through shared helpers. Do not compare platform-specific path strings directly in tests.
- Avoid allocations in hot graph traversal paths unless the surrounding code already owns that cost.

## Validation

- Add fixture coverage for resolver and graph behavior together when reachability changes.
- Include Windows-style path coverage when editing path normalization.
- Run targeted graph tests before broader workspace tests.
