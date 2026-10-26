### Fixed

- **`fallow dead-code` no longer reports `tsrs` and `fallow-type-aware` as unused devDependencies.** Fallow loads these type-aware backend packages at run time, so no script or import names them. The tooling catalogue now credits both packages by name. Thanks [@Smrtnyk](https://github.com/Smrtnyk) for the report. (Closes [#3873](https://github.com/fallow-rs/fallow/issues/3873))
