# `plugin-check`: Verify external plugins

Read-only dry-run of your external plugins. Reports, per plugin, whether it activated (with the unmet `detection`/`enabler` requirement when inactive), and for `manifestEntries` rules which manifests each matched, what it seeded (with `path_exists`), and typed warnings (`manifests-matched-none`, `when-excluded-all`, `field-path-unresolved`, `entries-empty`, `manifest-parse-failed`, `field-values-limit-exceeded`, `entry-expansion-limit-exceeded`, `entry-outside-root`, `seeded-paths-missing`). Run it after authoring a `fallow-plugin-*.jsonc` to verify it before a full analysis. Deterministic output; always exits 0 (advisory, never a gate).

```bash
fallow plugin-check --format json
```
