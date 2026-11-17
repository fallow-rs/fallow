### Fixed

- **A Jest `moduleNameMapper` target that names a package now credits that package.** A mapping such as `'\\.(css|less)$': 'identity-obj-proxy'` no longer gives an unused dev dependency for `identity-obj-proxy`. A target with a capture group after the package name, such as `lodash-es/$1`, credits `lodash-es`. (Closes [#4712](https://github.com/fallow-rs/fallow/issues/4712))
