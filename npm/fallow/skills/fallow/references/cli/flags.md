# `flags`: Feature Flag Detection

Detects feature flag patterns in the codebase. Identifies environment variable flags (`process.env.FEATURE_*`), SDK calls from common providers (LaunchDarkly, Statsig, Unleash, GrowthBook, Split, PostHog, Vercel Flags, ConfigCat, Flagsmith, Optimizely, Eppo), and config object patterns (opt-in). Reports flag locations, detection confidence, and cross-references with dead code findings.

## Flags

<!-- generated:flags:flags:start -->
| Flag | Type | Default | Description |
|---|---|---|---|
| `--retirement` | `bool` | `false` | Add a retirement report: one row per flag, with the reasons the flag can be retired. Advisory only; nothing is removed |
| `--reason` | `single-read-site\|test-only\|literal-constant\|identical-branches\|empty-branch\|guards-dead-code\|defined-never-read\|fully-rolled-out\|archived-in-vendor\|missing-in-vendor\|vendor-only` | - | Keep only retirement rows with this reason (repeatable) |
| `--sort` | `age\|sites\|name` | `age` | Order of the retirement rows |
| `--flag-age` | `blame\|pickaxe\|off` | `blame` | How to measure flag age: blame (lower bound), pickaxe (first commit with the name, slower) or off |
| `--min-age` | `string` | - | Keep only retirement rows at least this many days old |
| `--flag-state` | `string` | - | Vendor flag export (JSON, read offline) that adds the fully-rolled-out, archived-in-vendor, missing-in-vendor and vendor-only reasons |
| `--top` | `string` | - | Show only the top N flags |
| `--max-flag-age` | `string` | - | Exit with code 1 when a flag in scope is older than this many days. Opt-in; needs a flag age |

Common global flags for this command: [`--format`](global-flags.md), [`--quiet`](global-flags.md), [`--changed-since`](global-flags.md), [`--workspace`](global-flags.md).
<!-- generated:flags:flags:end -->
## Examples

```bash
# Detect all feature flags with JSON output
fallow flags --format json --quiet

# Top 10 flags
fallow flags --format json --quiet --top 10

# Flags at least 90 days old, oldest first. A row with an empty
# `reasons` array is not a retirement candidate.
fallow flags --retirement --min-age 90 --format json --quiet

# Single workspace package
fallow flags --format json --quiet --workspace my-package
```

## JSON Output Structure

```json
{
  "schema_version": 7,
  "version": "3.49.0",
  "elapsed_ms": 116,
  "feature_flags": [],
  "total_flags": 0
}
```
