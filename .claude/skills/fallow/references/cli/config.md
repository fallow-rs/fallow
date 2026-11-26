# `config`: Show Resolved Config

Prints the loaded config file path and the resolved config (with `extends` merged) as JSON. Useful for verifying which config fallow picked up, especially in monorepos.

```bash
fallow config            # path on first line, JSON below
fallow config --path     # only the path (scriptable)
fallow config --resolved --for audit   # each decision of an audit run and its cause
```

`--resolved` prints the decisions of a run as JSON: for each analysis whether it runs and its production mode, and the run settings, each with its source (a flag, an environment variable by name, a config key with its file, or a default). `--for` picks the command whose rules apply (`combined`, `audit`, `dead-code`, `dupes`, `health`); the default is the bare run. A config that does not resolve exits 2 with a structured error.

## Flags

<!-- generated:flags:config:start -->
| Flag | Type | Default | Description |
|---|---|---|---|
| `--path` | `bool` | `false` | Print only the config file path, no JSON |
| `--resolved` | `bool` | `false` | Print each decision of a run and its cause as JSON: for each analysis whether it runs and its production mode, and the run settings, each with the flag, environment variable, config key or default that gave it. Environment variables show by name only |
| `--for` | `combined\|audit\|dead-code\|dupes\|health` | - | The command whose rules `--resolved` applies. Without it, the rules of the bare `fallow` run apply |
| `--only` | `dead-code\|dupes\|health\|architecture` | - | With `--resolved`: run only these analyses, as `--only` of the bare run or of `fallow audit` does |
| `--skip` | `dead-code\|dupes\|health\|architecture` | - | With `--resolved`: skip these analyses, as `--skip` of the bare run or of `fallow audit` does |
| `--production-dead-code` | `bool` | `false` | With `--resolved`: run dead-code analysis in production mode |
| `--production-health` | `bool` | `false` | With `--resolved`: run health analysis in production mode |
| `--production-dupes` | `bool` | `false` | With `--resolved`: run duplication analysis in production mode |

Common global flags for this command: [`--format`](global-flags.md), [`--quiet`](global-flags.md), [`--config`](global-flags.md), [`--root`](global-flags.md).
<!-- generated:flags:config:end -->
## Exit Codes

| Code | Meaning |
|------|---------|
| `0` | Config file found and loaded |
| `2` | Error (parse failure, explicit `--config` path missing) |
| `3` | No config file found; defaults are in effect |

Honors the global `--config <path>` flag: if passed, that path is loaded directly instead of walking the directory tree.

The `loaded config: <path>` line is also emitted to stderr automatically at the start of every human-format CLI run (suppressed by `--quiet` and non-human formats).
