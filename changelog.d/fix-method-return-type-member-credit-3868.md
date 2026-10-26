### Fixed

- **`fallow dead-code` credits a member access on the declared return type of an instance method.** A call such as `manager.current()?.onDown?.()`, where `current(): Tool | undefined`, now credits `Tool.onDown`. The implementer `SelectTool.onDown` is no longer reported as an unused class member. A class return type, such as `manager.widget().paint()` with `widget(): Widget`, now credits `Widget.paint`. Static, private, and method-generic return types give no credit. Thanks to @Smrtnyk for the report.
  (Closes [#3868](https://github.com/fallow-rs/fallow/issues/3868))
