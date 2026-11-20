### Added

- **`fallow config --resolved` shows the base ref of an audit.** For `--for audit`, `settings.audit_base` gives the base that `--base` or `FALLOW_AUDIT_BASE` names, with its source, and is `null` when the audit detects the base.

### Fixed

- **The MCP `audit` tool picks the same base on both routes.** With `FALLOW_CHANGED_SINCE` set, the typed `audit` and `decision_surface` tools compared with that ref before `FALLOW_AUDIT_BASE` and before the detected base. They now use the `base` parameter first, then `FALLOW_AUDIT_BASE`, then the automatic base, as the tool description says and as the CLI route does. `FALLOW_CHANGED_SINCE` gives the base only when the audit detects none.
