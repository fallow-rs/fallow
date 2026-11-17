### Fixed

- **A Jest `moduleNameMapper` target that names a package now credits that package.** A mapping such as `'\\.(css|less)$': 'identity-obj-proxy'` no longer gives an unused dev dependency for `identity-obj-proxy`. A target with a capture group after the package name, such as `lodash-es/$1`, credits `lodash-es`. (Closes [#4712](https://github.com/fallow-rs/fallow/issues/4712))
- **A Vite or Vitest alias with a regular expression `find` now credits a package replacement.** An entry such as `{ find: /\.(css|less)$/, replacement: 'identity-obj-proxy' }` in `resolve.alias` or `test.alias` no longer gives an unused dev dependency for `identity-obj-proxy`. (Closes [#4713](https://github.com/fallow-rs/fallow/issues/4713))
