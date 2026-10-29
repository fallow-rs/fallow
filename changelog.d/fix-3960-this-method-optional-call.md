### Fixed

- **`fallow dead-code` credits a member access on the declared return type of a method or getter that a class calls through `this`.** A call such as `this.current()?.onDown?.()`, where `private current(): Tool | undefined`, now credits `Tool.onDown`. The implementer `SelectTool.onDown` is no longer reported as an unused class member. The fix also covers a `#`-private method (`this.#current()`) and a private getter (`this.active?.onDown?.()`). A static method and a return type that names a type parameter of the method give no credit. Thanks @Smrtnyk for the report.
  (Closes [#3960](https://github.com/fallow-rs/fallow/issues/3960))
