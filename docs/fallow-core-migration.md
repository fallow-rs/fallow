# Migrating from fallow-core analyzer functions

ADR-008 makes `fallow-core` an internal implementation crate. Starting with
2.76.0, the top-level `fallow_core::analyze*` entry points plus the
detector helpers under `fallow_core::analyze::*` emit deprecation
warnings. The next minor release (target `2.77.0`, no earlier than 2026-Q3)
will flip `publish = false` on `fallow-core` so the crate is no longer
fetchable from crates.io.

Use the supported embedder API in `fallow_cli::programmatic` instead. The
programmatic API returns `Result<serde_json::Value, ProgrammaticError>` whose
JSON shape matches the matching CLI command with `--format json`; it does not
return typed `AnalysisResults` or the bare finding structs from `fallow-core`.

## Function mapping

| Deprecated `fallow_core` function | Replacement |
| --- | --- |
| `fallow_core::analyze`, `analyze_with_usages`, `analyze_with_trace`, `analyze_retaining_modules`, `analyze_with_parse_result`, `analyze_project` | `fallow_cli::programmatic::detect_dead_code` (or `compute_health` / `detect_duplication` for those slices) |
| `fallow_core::analyze::find_dead_code_full` | `fallow_cli::programmatic::detect_dead_code` |
| `find_unused_files` | `fallow_cli::programmatic::detect_dead_code` |
| `find_unused_exports` | `fallow_cli::programmatic::detect_dead_code` |
| `find_duplicate_exports` | `fallow_cli::programmatic::detect_dead_code` |
| `find_unused_dependencies` | `fallow_cli::programmatic::detect_dead_code` |
| `find_unused_members` | `fallow_cli::programmatic::detect_dead_code` |
| Catalog and dependency-override finders | `fallow_cli::programmatic::detect_dead_code` |
| `find_boundary_violations` | `fallow_cli::programmatic::detect_boundary_violations` |
| `collect_feature_flags`, `correlate_with_dead_code` | No programmatic equivalent today. Use `fallow flags --format json`; the `guarded_dead_exports` field on each flag carries the dead-code correlation. |

For duplication clone detection, use
`fallow_cli::programmatic::detect_duplication`. For health, complexity,
hotspots, targets, and coverage-gap output, use
`fallow_cli::programmatic::compute_health` or
`fallow_cli::programmatic::compute_complexity`.

## Minimal example

```rust
use fallow_cli::programmatic::{AnalysisOptions, DeadCodeOptions, detect_dead_code};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = DeadCodeOptions {
        analysis: AnalysisOptions {
            root: std::env::current_dir()?,
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
previously matched Rust structs should now narrow typed envelopes by the
top-level `kind` field and deserialize into their own local DTOs if they need
typed access. Set `AnalysisOptions::legacy_envelope` only while migrating
consumers that still expect the previous root shape without `kind`.

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
