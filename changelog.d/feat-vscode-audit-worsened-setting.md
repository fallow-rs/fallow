### Added

- **The VS Code extension has a `fallow.audit.worsened` setting.** The
  setting selects how an inherited complexity finding that got worse affects
  the audit verdict in the editor. The values are `warn`, `error` and `off`,
  the same as `fallow audit --worsened`. The default `auto` uses
  `audit.worsened` from the project config. The extension sends the flag only
  to a CLI that supports it. With an older CLI, the audit runs without the
  flag and the output channel shows a note.
