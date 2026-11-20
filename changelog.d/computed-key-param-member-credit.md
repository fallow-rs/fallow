### Fixed

- **`fallow dead-code` no longer reports the class members of a typed parameter that a computed key reads.** A function can call a member of a typed parameter through a computed key, for example `view[action]()`. When `action` has a type that is a union of string literals, such as `"zoomIn" | "zoomOut"`, fallow credits only the members that the union names. A key of another type, for example `view[action as keyof View]()`, can name any member, so fallow counts the call as a use of the whole object. A string-literal key such as `view["zoomIn"]()` still credits only the member that it names.
  (Closes [#4812](https://github.com/fallow-rs/fallow/issues/4812))
