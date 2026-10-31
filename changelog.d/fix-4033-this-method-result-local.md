### Fixed

- **`fallow dead-code` credits a member access through a local that holds the result of a method called on `this`.** Code such as `const old = this.current(); old?.deactivate?.();`, where `private current(): Tool | undefined`, now credits `Tool.deactivate`, as the direct form `this.current()?.deactivate?.()` already does. The implementer `SelectTool.deactivate` is no longer reported as an unused class member. The fix also covers `this.current?.()`, a `#`-private method, and a method that comes after the call site. A cast, a call with arguments, and a call in a plain `function` give no credit. Thanks @Smrtnyk for the report.
  (Closes [#4033](https://github.com/fallow-rs/fallow/issues/4033))
