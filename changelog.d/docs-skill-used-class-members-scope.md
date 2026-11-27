### Changed

- **The configuration reference of the fallow skill now says that a scoped `usedClassMembers` rule skips the named class.** A rule with `"extends": "Base"` applies to the subclasses of `Base`, not to `Base` itself. To keep all members of one class, put `// fallow-ignore-file unused-class-member` in its file.
  (Refs [#5233](https://github.com/fallow-rs/fallow/issues/5233))
