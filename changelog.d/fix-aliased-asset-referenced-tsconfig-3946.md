### Fixed

- **`fallow dead-code` no longer reports an aliased asset import as an unlisted dependency when the alias is in a referenced tsconfig.** A solution-style root `tsconfig.json` can reference a config whose `include` starts with `./`, for example `"./app/javascript/**/*"`. Fallow did not match that pattern to the source files, so it did not apply the `paths` of that config. An import such as `@circle-assets/images/logo.png` then gave the unlisted dependency `@circle-assets/images`. Fallow now removes `./` and `..` segments from `include` and `exclude` patterns before it matches them. Thanks to @agneym for the report and the public reproduction.
  (Closes [#3946](https://github.com/fallow-rs/fallow/issues/3946))
