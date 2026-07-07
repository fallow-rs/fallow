# Migrating from fallow-core analyzer functions

`fallow-core` is an internal implementation crate. Starting with 2.76.0, the
top-level `fallow_core::analyze*` entry points plus the detector helpers under
`fallow_core::analyze::*` emit deprecation warnings. `fallow-core` is published
only because `fallow-engine` uses it as a private detector backend; it is not
the supported Rust embedder surface. Use the published `fallow-api` facade or
the typed `fallow-engine` layer instead.

Use the supported embedder API in `fallow_api`. New Rust consumers should call
the typed `run_*` functions (`run_dead_code`, `run_duplication`,
`run_feature_flags`, `run_health`, `run_circular_dependencies`,
`run_boundary_violations`) and serialize only at their own protocol boundary
via the matching `serialize_*_programmatic_json` function.

Use `fallow_engine` for in-process consumers that need typed analysis results.
It owns the adapter boundary over the internal `fallow-core` backend and is
where editor, API, and embedding surfaces should depend before using typed
`AnalysisResults`.

## Architecture north star

For the contributor-facing crate rules, IO boundaries, contract rules, and
final ownership policy, see [`architecture-invariants.md`](architecture-invariants.md).

Fallow should be a deterministic repo-intelligence engine with thin integration
surfaces, not a CLI with libraries arranged around it. That means new analysis
flows should start in `fallow-engine`, expose typed contracts through
`fallow-api`, and serialize only at CLI, LSP, MCP, NAPI, CI, or other protocol
boundaries.

Use these boundaries when adding new product flows:

- **Engine-first**: config resolution, discovery, parsing, graph construction,
  cache/fingerprint handling, and typed analysis results belong behind
  `fallow-engine`.
- **Contracts-first**: outward-facing issue metadata, root envelopes, JSON,
  SARIF, CodeClimate, suppressions, docs anchors, LSP diagnostic metadata, and
  TypeScript aliases must derive from typed contract registries instead of
  scattered string tables.
- **Output-owned protocol formatting**: SARIF, CodeClimate, GitHub/GitLab
  markdown, compact, and human render helpers should live in `fallow-output` or
  protocol adapter crates once analysis findings have been normalized. API
  layers may walk typed result structures, but they should delegate common
  result assembly, fingerprints, docs metadata, and formatter invariants to the
  shared output boundary.
- **Session reuse before broad persistence**: grow `AnalysisSession` toward a
  lightweight query model for shared discovery, parsed modules, graph state,
  changed files, fingerprints, and production modes. Persisted cache expansion
  should only follow when invalidation is covered by transparency tests.
- **Repo-policy as code**: project-specific boundaries, zones, banned imports,
  banned exports, banned calls, and framework-aware facts are the preferred
  extension path before a general arbitrary-code plugin runtime.
- **Core stays backend-only**: `fallow-core` may keep private detector
  implementation details and compatibility shims, but public or product
  surfaces should depend on `fallow-engine` or `fallow-api`, not `fallow-core`.

## Function mapping

| Deprecated `fallow_core` function | Replacement |
| --- | --- |
| `fallow_core::analyze`, `analyze_with_usages`, `analyze_with_trace`, `analyze_retaining_modules` | `fallow_api::run_dead_code` for typed output before serialization, or `fallow_engine` for lower-level in-process analysis |
| Removed top-level wrappers such as `analyze_with_parse_result` and `analyze_project` | Use `fallow_engine::AnalysisSession` internally, or `fallow_api` for supported programmatic surfaces |
| `fallow_core::analyze::find_dead_code_full` | `fallow_api::run_dead_code` |
| `find_unused_files` | `fallow_api::run_dead_code` |
| `find_unused_exports` | `fallow_api::run_dead_code` |
| `find_duplicate_exports` | `fallow_api::run_dead_code` |
| `find_unused_dependencies` | `fallow_api::run_dead_code` |
| `find_unused_members` | `fallow_api::run_dead_code` |
| Catalog and dependency-override finders | `fallow_api::run_dead_code` |
| `find_boundary_violations` | `fallow_api::run_boundary_violations` |
| `collect_feature_flags`, `correlate_with_dead_code` | `fallow_api::run_feature_flags` for typed output before serialization. The `guarded_dead_exports` field on each flag carries the dead-code correlation. |

For duplication clone detection, use
`fallow_api::run_duplication`. For health, complexity, hotspots, targets, and
coverage-gap output, use `fallow_api::run_health` or
`fallow_api::run_health_with_runner` for typed output. If a Rust embedder needs
JSON, call the matching `serialize_*_programmatic_json` function at its
protocol boundary.

## Minimal example

```rust
use fallow_api::{AnalysisOptions, DeadCodeOptions, run_dead_code};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = DeadCodeOptions {
        analysis: AnalysisOptions {
            root: Some(std::env::current_dir()?),
            ..AnalysisOptions::default()
        },
        ..DeadCodeOptions::default()
    };

    let output = run_dead_code(&options)?;
    let total = output.output.summary.total_issues;
    println!("{total} issues");
    Ok(())
}
```

The JSON contract is documented in `docs/output-schema.json`. Consumers that
want CLI parity can call the matching `serialize_*_programmatic_json` function
on a typed programmatic output at their protocol boundary. Object-shaped JSON
roots always carry the top-level `kind` discriminator; consumers should branch
on `kind` rather than probing for unique field presence.

## Semantic differences vs. the typed Rust API

The programmatic API runs the full analysis pipeline (discovery, parsing,
plugins, scripts, module resolution, graph construction, all detectors) for
every call. If you previously invoked one detector in isolation, the new call
still runs the entire pipeline. There is no per-detector programmatic entry
point today; if you need to filter, use the typed `run_*` output's retained
result arrays. Consumers that intentionally need JSON can serialize the typed
output and select the relevant JSON array at their boundary.

The JSON compatibility envelope wraps each finding in the same `*Finding` shape
as the typed programmatic output. JSON field access patterns differ from the old
Rust structs; for example:

```jsonc
// old (Rust):     results.unused_exports[i].export.path
// new (JSON):     json["unused_exports"][i]["export"]["path"]
```

Introspect the shape against any real fixture with:

```bash
fallow check --format json --root path/to/project | jq '.unused_exports[0]'
```

`ProgrammaticError` carries the same exit-code ladder as the CLI
(`exit_code: 0` ok, `2` generic, `7` network, etc.) so CI integrations that
branch on exit codes work identically through the programmatic surface.

## Removed compatibility debt

- The previous root-envelope compatibility options have been removed. Tagged
  root envelopes are the only supported object-shaped JSON protocol.
