### Fixed

- **The VS Code extension opens the Settings editor in a way that cannot freeze it in Cursor.** In Cursor, `workbench.action.openSettings` with a search query can freeze the Settings editor. These actions now run that command through `runCommands`:
  - the `Fallow: Open Settings` command
  - the "Open Settings" action after a failed binary download
  - the "Set Path" action when `fallow-lsp` is missing
  - the "Open Setting" action of `Set Baseline at HEAD`

  The Settings editor opens with the same query, and no result goes back to the extension. If the Settings editor does not open, a warning names the setting to search for. In VS Code, the actions do not change. (Closes [#4595](https://github.com/fallow-rs/fallow/issues/4595))
