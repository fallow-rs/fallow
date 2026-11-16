### Changed

- **The VS Code and Cursor extension shows one Fallow status bar item instead of up to five.** The item shows the finding count and the health grade, for example `Fallow 9 · C`. Its hover shows the issue breakdown, duplication, health, the audit verdict, the license state, the active workspace and whether findings are visible in the editor. A click opens a menu to run the analysis, hide or show findings, run the audit, switch the workspace, or open the settings. The item gets an error background when the audit fails. When all findings are hidden in the editor, the item keeps a closed-eye icon, also next to an audit or progress icon. The separate workspace, audit, license and eye items are gone. All commands keep their ids, and the new `Fallow: Show Status Menu` command opens the menu.

- **The Fallow status bar item shows a license only when it needs action.** The item text shows the license when it expires within 14 days, has expired, or is not valid. When the license is past its grace window (paid features are blocked), the item also gets a warning background. A failed audit keeps the error background, also when the license is blocked. An expired license that still works shows the notice without a background. License material that the CLI cannot read, for example a stale `FALLOW_LICENSE` value, now shows "license invalid" with a warning icon. Before, the extension hid it. The hover shows the tier, the seats, the features and the expiry, and it offers a refresh when the CLI suggests one.

### Deprecated

- **The `fallow.diagnostics.statusBar` setting has no effect.** It controlled the separate eye button, which is gone. Use **Hide findings in the editor** in the menu of the Fallow status bar item, or the `Fallow: Toggle Hide All Findings` command.
