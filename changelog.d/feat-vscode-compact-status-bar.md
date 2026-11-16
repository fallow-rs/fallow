### Changed

- **The VS Code and Cursor extension shows one Fallow status bar item instead of up to five.** The item shows the finding count and the health grade, for example `Fallow 9 · C`. Its hover shows the issue breakdown, duplication, health, the audit verdict, the license state, the active workspace and whether findings are visible in the editor. A click opens a menu to run the analysis, hide or show findings, run the audit, switch the workspace, or open the settings. The item gets an error background when the audit fails. The item text shows the license only when it expires within 14 days, has expired, or is not valid. The separate workspace, audit, license and eye items are gone. All commands keep their ids, and the new `Fallow: Show Status Menu` command opens the menu.

### Deprecated

- **The `fallow.diagnostics.statusBar` setting has no effect.** It controlled the separate eye button, which is gone. Use **Hide findings in the editor** in the menu of the Fallow status bar item, or the `Fallow: Toggle Hide All Findings` command.
