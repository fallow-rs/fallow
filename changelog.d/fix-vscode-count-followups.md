### Fixed

- **VS Code counts component health findings at error severity.** With `"prop-drilling": "error"`, `"thin-wrapper": "error"` or `"duplicate-prop-shape": "error"`, the CLI counts these findings in its total and fails the run on them. The VS Code sidebar total, the notification and the status bar now count them too. The sidebar shows `advisory` only for a category whose findings the total does not count. The status bar hover also says that the Problems panel lists each location of a finding, so its count can be higher than the total.
