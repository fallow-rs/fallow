### Fixed

- **`fallow dead-code` no longer reports the class members of a typed parameter that a computed key reads.** A function can call a member of a typed parameter through a key that is not a string literal, for example `view[action]()` or `view[action as keyof View]()`. Such a key can name any member, so fallow now counts the call as a use of the whole object and credits each member of the class. A string-literal key such as `view["zoomIn"]()` still credits only the member that it names.
  (Closes [#4812](https://github.com/fallow-rs/fallow/issues/4812))
