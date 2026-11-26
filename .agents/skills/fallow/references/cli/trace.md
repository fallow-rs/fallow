# `trace`: Symbol Call Chains

Walk the callers and callees of one exported symbol through the module graph. Callers are the modules that import the symbol (walked up); callees are the symbol's module's import-symbol edges plus its intra-module call sites (walked down). Best-effort and syntactic per ADR-001: resolved and unresolved callees are reported honestly, never silently dropped. This is its own surface, never folded into the ranked review brief.

The target is a positional argument, formatted as `FILE:SYMBOL` (for example `src/utils.ts:formatDate`). When neither `--callers` nor `--callees` is given, both directions are walked.

```bash
fallow trace src/utils.ts:formatDate
fallow trace src/utils.ts:formatDate --callers --depth 3
fallow trace --path src/main.ts src/chart.ts --format json --quiet
fallow trace --path src/main.ts src/chart.ts --eager-only --format json --quiet
```

Each `--path` hop carries `type_only` and `dynamic`. A `dynamic` hop loads its target only on demand (`import()`, a lazy glob) or on another thread (a worker, a fork). `--eager-only` follows static value imports only, so its route explains why a module is in the `--entry-weight` eager set of `fallow list`.

<!-- generated:flags:trace:start -->
| Flag | Type | Default | Description |
|---|---|---|---|
| `--dependency` | `string` | - | How the code uses each imported name of a package: file and call counts, one hop through project wrappers, and a count of each use that the trace cannot resolve |
| `--specifier` | `string` | - | With `--dependency`, report only these imported names (repeatable or comma-separated). Turns on the site page |
| `--sites` | `bool` | `false` | With `--dependency`, list the usage sites with file, line and column |
| `--limit` | `string` | - | With `--dependency`, the largest number of sites on a page (1 to 500, default 50). Turns on the site page |
| `--cursor` | `string` | - | With `--dependency`, the `next_cursor` of the previous page. Turns on the site page |
| `--path` | `string` | - | Shortest import path between two modules, as two file paths (e.g. `--path src/app.ts src/db.ts`). Mutually exclusive with the symbol target and the call-chain flags |
| `--eager-only` | `bool` | `false` | With `--path`, follow only static value imports, so the route explains why TO loads before FROM runs. `import()`, lazy globs, worker loads and `import type` do not qualify |
| `--callers` | `bool` | `false` | Walk UP to callers (modules that import the symbol). When neither `--callers` nor `--callees` is set, both directions are walked |
| `--callees` | `bool` | `false` | Walk DOWN to callees (the symbol's module's import-symbol edges plus unresolved call sites). When neither flag is set, both are walked |
| `--depth` | `string` | - | Chain depth bound for both directions (default 2). Symbol-level is best-effort, so a shallow bound keeps the trace legible |

Common global flags for this command: [`--format`](global-flags.md), [`--quiet`](global-flags.md), [`--root`](global-flags.md), [`--config`](global-flags.md).
<!-- generated:flags:trace:end -->
