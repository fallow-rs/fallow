# Migrating from fallow-core analyzer functions

ADR-008 makes `fallow-core` an internal implementation crate. Starting with
2.76.0, the top-level `fallow_core::analyze*` entry points plus the
detector helpers under `fallow_core::analyze::*` emit deprecation
warnings. The next minor release (target `2.77.0`, no earlier than 2026-Q3)
will flip `publish = false` on `fallow-core` so the crate is no longer
fetchable from crates.io.

Use the supported embedder API in `fallow_api` for CLI-shaped JSON output.
The programmatic API returns
`Result<serde_json::Value, ProgrammaticError>` whose JSON shape matches the
matching CLI command with `--format json`.

Use `fallow_engine` for in-process consumers that need typed analysis results.
It owns the migration boundary over the internal `fallow-core` backend and is
where editor, API, and embedding surfaces should move before depending on
typed `AnalysisResults`.

## Function mapping

| Deprecated `fallow_core` function | Replacement |
| --- | --- |
| `fallow_core::analyze`, `analyze_with_usages`, `analyze_with_trace`, `analyze_retaining_modules`, `analyze_with_parse_result`, `analyze_project` | `fallow_api::detect_dead_code` for CLI-shaped JSON, `fallow_api::run_dead_code` for typed output before serialization, or `fallow_engine` for typed in-process analysis |
| `fallow_core::analyze::find_dead_code_full` | `fallow_api::detect_dead_code` |
| `find_unused_files` | `fallow_api::detect_dead_code` |
| `find_unused_exports` | `fallow_api::detect_dead_code` |
| `find_duplicate_exports` | `fallow_api::detect_dead_code` |
| `find_unused_dependencies` | `fallow_api::detect_dead_code` |
| `find_unused_members` | `fallow_api::detect_dead_code` |
| Catalog and dependency-override finders | `fallow_api::detect_dead_code` |
| `find_boundary_violations` | `fallow_api::detect_boundary_violations` |
| `collect_feature_flags`, `correlate_with_dead_code` | No programmatic equivalent today. Use `fallow flags --format json`; the `guarded_dead_exports` field on each flag carries the dead-code correlation. |

For duplication clone detection, use
`fallow_api::detect_duplication` or `fallow_api::run_duplication`. For health,
complexity, hotspots, targets, and coverage-gap output, `fallow_api` owns the
JSON output contract through `compute_health_with_runner` /
`run_health_with_runner`. Use `fallow_engine::EngineHealthRunner` when you need
typed engine execution from Rust, or `fallow_api::compute_health` when you want
the supported JSON contract directly.

## Minimal example

```rust
use fallow_api::{AnalysisOptions, DeadCodeOptions, detect_dead_code};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = DeadCodeOptions {
        analysis: AnalysisOptions {
            root: Some(std::env::current_dir()?),
            ..AnalysisOptions::default()
        },
        ..DeadCodeOptions::default()
    };

    let json = detect_dead_code(&options)?;
    let total = json["summary"]["total_issues"].as_u64().unwrap_or(0);
    println!("{total} issues");
    Ok(())
}
```

The JSON contract is documented in `docs/output-schema.json`. Consumers that
want CLI parity should narrow typed envelopes by the top-level `kind` field and
deserialize into their own local DTOs if they need typed access. Set
`AnalysisOptions::legacy_envelope` only while migrating consumers that still
expect the previous root shape without `kind`.

## Semantic differences vs. the typed Rust API

The programmatic API runs the full analysis pipeline (discovery, parsing,
plugins, scripts, module resolution, graph construction, all detectors) for
every call. If you previously invoked one detector in isolation, the new call
still runs the entire pipeline. There is no per-detector programmatic entry
point today; if you need to filter, parse the returned JSON and select the
relevant array.

The JSON envelope wraps each finding in a typed `*Finding` shape carried over
from the CLI's `--format json` contract. Field access patterns differ from the
old Rust structs; for example:

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
