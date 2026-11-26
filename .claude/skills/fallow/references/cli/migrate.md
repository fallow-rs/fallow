# `migrate`: Config Migration

Migrates configuration from knip and/or jscpd to fallow. Auto-detects config files.

## Flags

<!-- generated:flags:migrate:start -->
| Flag | Type | Default | Description |
|---|---|---|---|
| `--toml` | `bool` | `false` | Output as `fallow.toml` (mutually exclusive with `--jsonc`) |
| `--jsonc` | `bool` | `false` | Write to `.fallowrc.jsonc` instead of `.fallowrc.json`. Same JSONC content either way; the `.jsonc` extension lets editors auto-detect JSON-with-comments syntax highlighting |
| `--dry-run` | `bool` | `false` | Preview without writing |
| `--from` | `string` | - | Specify source config file path |

Common global flags for this command: [`--root`](global-flags.md), [`--config`](global-flags.md).
<!-- generated:flags:migrate:end -->
Without `--jsonc` or `--toml`, fallow auto-mirrors the source extension: a `knip.jsonc` migration writes `.fallowrc.jsonc`, a `knip.json` migration writes `.fallowrc.json`.

## Detected Source Configs

- `knip.json`, `knip.jsonc`, `.knip.json`, `.knip.jsonc`
- `package.json` embedded `knip` field
- `.jscpd.json`
- `package.json` embedded `jscpd` field

## Examples

```bash
fallow migrate --dry-run        # preview
fallow migrate                  # auto-detect; mirrors source extension
fallow migrate --jsonc          # force .fallowrc.jsonc output
fallow migrate --toml           # output as fallow.toml
fallow migrate --from knip.jsonc
```
