### Fixed

- **The health score no longer measures duplication when the config has `duplicates.enabled: false`.** `fallow health --score` and the bare `fallow --score` still ran their own duplication pass and applied the duplication penalty, so a project that turned duplication detection off lost points for clones. The score now has no duplication measure and no duplication penalty for such a project, and the refactoring targets use no clone evidence. The score of such a project can go up by the size of that penalty, and the first `--trend` delta after the change of the key includes that step. A run that names duplication, such as `--only dupes,health`, gives the same score, so the score does not depend on the analyses that one run selects.
- **`fallow_api::run_combined` and the MCP combined analysis now read `duplicates.enabled`.** They ran the duplication domain with the key off, so they did not agree with the bare `fallow` command. A run that selects only the duplication domain names it, so it still runs.
- **The language server gives no clone diagnostics when the config has `duplicates.enabled: false`.** The editor showed `code-duplication` diagnostics for a project that turned duplication detection off.
- **`fallow dead-code --include-dupes` now runs its cross-reference when the config has `duplicates.enabled: false`.** The command ignored the flag with no message. The flag names the duplication analysis, as `--only dupes` and the `fallow dupes` command do, so it now runs.

### Changed

- **One rule now decides whether a run executes the duplication analysis.** A run that names the analysis executes it. Without a name, the config key of the command decides (`audit.dupes`), and without that key `duplicates.enabled` decides. The bare `fallow` command, `fallow audit`, `fallow_api::run_audit`, `fallow_api::run_combined`, the language server and the health score all use this rule.
