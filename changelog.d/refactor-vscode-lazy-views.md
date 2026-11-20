### Fixed

- **The VS Code extension no longer fills a view with a result that the user no longer wants.** A Health, Security Candidates or Runtime Coverage run that ended after the user disabled the feature, ran Clear Coverage or started a newer run filled the view again. The extension now drops such a result.
- **The VS Code Health and Security Candidates views load again when an input of their run changes.** A loaded Health view kept the data of the old settings after a change of `fallow.production`, `fallow.configPath`, `fallow.changedSince`, `fallow.workspace` or `fallow.allowRemoteExtends`, and after a change in the workspace picker. The Health view now runs again. A visible Security Candidates view scans again, and a hidden one scans again when the user opens it.
